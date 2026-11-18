//! Links in terminal text: SonicTerm's URL and file path scanner (`url_scan`),
//! what a match points to, and what clicking it does.

use fancy_regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

mod activation;
mod system;
// SonicTerm's link scanner, as vendored: some of its API is only for its tests.
#[allow(dead_code, deprecated)]
mod url_open;
#[allow(dead_code, deprecated, unused_imports)]
mod url_scan;

pub use activation::{Opener, activate, safe_folder};
/// What a click on a link means, without carrying it out.
#[allow(unused_imports)]
pub use activation::{Activation, activation};
#[cfg(test)]
pub(crate) use activation::{Call, Recorder};
pub use system::SystemOpener;
pub use url_scan::PathStyle;
use url_scan::DetectedTarget;

/// URL schemes Apollo links besides SonicTerm's (http, https, mailto, file://), from Ghostty's pattern.
const EXTRA_SCHEMES: [&str; 12] = ["ftp://", "ssh://", "ssh:", "git://", "tel:", "magnet:", "ipfs://", "ipns://", "gemini://", "gopher://", "news:", "file:"];

/// Apollo's other-scheme URLs in `text`: not after a word, `.`, `/`, `-` or `:`
/// (`xftp://`); Ghostty's URL characters, with brackets the URL opened and
/// closed (`a(b(c)d)`, `[::1]`); no final `,` or `.`.
fn extra_urls(text: &str) -> Vec<Range<usize>> {
    // `\w` exactly as Ghostty's pattern means it (combining marks included: `cafe\u{301}`).
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\w$").expect("word pattern"));
    let word = |c: char| WORD.is_match(c.encode_utf8(&mut [0; 4])).unwrap_or(false);
    let body = |c: char| word(c) || "-.~:/?#@!$&*+,;=%".contains(c);
    let mut out = Vec::new();
    let mut from = 0;
    while from < text.len() {
        let start = from;
        from += text[from..].chars().next().map_or(1, char::len_utf8);
        let Some(scheme) = EXTRA_SCHEMES.iter().find(|s| text.get(start..start + s.len()).is_some_and(|t| t.eq_ignore_ascii_case(s))) else {
            continue;
        };
        if text[..start].chars().next_back().is_some_and(|c| word(c) || "./-:".contains(c)) {
            continue;
        }
        let (mut end, mut open) = (start + scheme.len(), Vec::new());
        let mut last = end;
        for (i, c) in text[end..].char_indices() {
            match c {
                '(' | '[' => open.push(c),
                ')' | ']' if open.last() == Some(&if c == ')' { '(' } else { '[' }) => {
                    open.pop();
                }
                c if body(c) => {}
                _ => break,
            }
            end = start + scheme.len() + i + c.len_utf8();
            if open.is_empty() {
                last = end;
            }
        }
        let end = text[..last].trim_end_matches([',', '.']).len();
        if end > start + scheme.len() {
            out.push(start..end);
            from = end;
        }
    }
    out
}

/// `$NAME/…`, which SonicTerm doesn't link: Ghostty's form, a variable first.
static VAR_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?<![\w$/~])\$[A-Za-z_]\w*/[\w\-.~:/?#@!$&*+;=%]+(?<![,.:])").expect("var path pattern"));

/// Windows UNC paths (`\\server\share\x`), which SonicTerm doesn't link.
static UNC_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?<![\w\\])\\\\[\w.\-]+\\[\w\-.~:\\@!$&+;=%]+(?<![,.:])").expect("unc path pattern"));

/// Where a link's text is looked up: the terminal's folder and home, and which
/// path grammar its text uses.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub cwd: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub style: PathStyle,
    /// Environment variables for `$NAME/…` paths.
    pub env: fn(&str) -> Option<std::ffi::OsString>,
}

impl Ctx {
    pub fn new(cwd: Option<PathBuf>) -> Self {
        Self { cwd, home: std::env::home_dir(), style: PathStyle::native(), env: |name| std::env::var_os(name) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Picked {
    Link(Target),
    /// A live terminal's address.
    Terminal(String),
}

/// The link under byte `at` and the text it covers, first match wins: a URL;
/// a live terminal address (`address` turns a word into one, `None` if not live);
/// a path SonicTerm finds, in its order, that exists; then `$NAME/…`, a markdown
/// link's `(path)` and `\\server\share\…` paths that exist. In lines joined back
/// together only a file that exists proves the join right.
pub fn pick(
    text: &str,
    at: usize,
    joined: bool,
    ctx: &Ctx,
    address: impl Fn(&str) -> Option<String>,
) -> Option<(Range<usize>, Picked)> {
    if !text.is_char_boundary(at) || at >= text.len() {
        return None;
    }
    // A URL owns its whole span, closing brackets included, even when it leads nowhere.
    if let Some(range) = url_spans(text).into_iter().find(|r| r.contains(&at)) {
        let target = url_target(&text[range.clone()])?;
        // A `file:` URL's trimmed tail counts: `file:///tmp/..` must not open `/tmp/`.
        let tail = text[range.end..].find(|c: char| c.is_whitespace() || "\"'`)]}>".contains(c)).map_or(text.len(), |i| range.end + i);
        if matches!(target, Target::File(_)) && dot_segment(&text[range.start..tail]) {
            return None;
        }
        if joined && !matches!(&target, Target::File(p) if p.exists()) {
            return None;
        }
        return Some((range, Picked::Link(target)));
    }
    if !joined
        && let Some(range) = word(text, at)
        && let Some(found) = address(&text[range.clone()])
    {
        return Some((range, Picked::Terminal(found)));
    }
    let col = text[..at].chars().count();
    if let Some((range, path)) = first_existing(url_scan::target_candidates_at_char_col_for_style(text, col, ctx.style, true), ctx) {
        return Some((range, Picked::Link(Target::File(path))));
    }
    fallbacks(text, at)
        .into_iter()
        .find_map(|(range, kind)| kind.resolve(&text[range.clone()], ctx).map(|p| (range, Picked::Link(Target::File(p)))))
}

/// The first candidate naming a file that exists, skipping any whose literal
/// (`missing_before`) exists: SonicTerm offers those only as a second reading.
fn first_existing(found: Vec<url_scan::TargetMatch>, ctx: &Ctx) -> Option<(Range<usize>, PathBuf)> {
    found
        .into_iter()
        .filter(|m| !m.missing_before.iter().any(|t| typed(t, ctx).is_some()))
        .find_map(|m| typed(&m.target, ctx).map(|path| (m.start..m.end, path)))
}

/// Every URL in `text`: SonicTerm's and Apollo's other schemes. A closing `)` or
/// `]` SonicTerm dropped comes back when the URL opened one (`foo(bar)`, `[::1]`).
fn url_spans(text: &str) -> Vec<Range<usize>> {
    let mut sonic: Vec<Range<usize>> = url_scan::find_urls(text).into_iter().map(|m| m.start..m.end).collect();
    for span in &mut sonic {
        let b = text.as_bytes();
        loop {
            let (open, close) = match b.get(span.end) {
                Some(b')') => (b'(', b')'),
                Some(b']') => (b'[', b']'),
                _ => break,
            };
            let part = &b[span.clone()];
            if part.iter().filter(|&&c| c == open).count() <= part.iter().filter(|&&c| c == close).count() {
                break;
            }
            span.end += 1;
        }
    }
    // Where two overlap, the one starting first wins (`ftp://a/?next=(https://b)`).
    let mut all: Vec<Range<usize>> = sonic.into_iter().chain(extra_urls(text)).collect();
    all.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    let mut spans: Vec<Range<usize>> = Vec::new();
    for span in all {
        if spans.last().is_none_or(|kept| kept.end <= span.start) {
            spans.push(span);
        }
    }
    spans
}

/// Whether a `file:` URL's path has a `.` or `..` part (RFC 3986's dot
/// segments), percent-encoded or not, once a sentence's `,;:!?` is off its end.
fn dot_segment(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url).trim_end_matches([',', ';', ':', '!', '?']);
    path.split(['/', '\\']).any(|part| {
        let part = percent_decode(part).unwrap_or_else(|| part.as_bytes().to_vec());
        part == b"." || part == b".."
    })
}

/// What a URL opens: a `file:` URL names a file (`None` if it can't), others open as they are.
fn url_target(url: &str) -> Option<Target> {
    if url.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:")) {
        return file_url_path(url).map(Target::File);
    }
    Some(Target::Url(url.to_owned()))
}

/// The file a SonicTerm path names, if it exists here: a bare name only in the terminal's folder.
fn typed(target: &DetectedTarget, ctx: &Ctx) -> Option<PathBuf> {
    match target {
        DetectedTarget::Uri(_) => None,
        DetectedTarget::PathCandidate(path) => existing(path, ctx),
        DetectedTarget::BareName(name) => existing(ctx.cwd.as_ref()?.join(name).to_str()?, ctx),
        DetectedTarget::SourceReference(source) => existing(&source.path, ctx),
    }
}

/// `path` made absolute (`~/` from home, relative from the terminal's folder), if it exists.
fn existing(path: &str, ctx: &Ctx) -> Option<PathBuf> {
    let p = match path.strip_prefix("~/").or_else(|| path.strip_prefix(r"~\")) {
        Some(rest) => ctx.home.as_ref()?.join(rest),
        None => PathBuf::from(path),
    };
    let full = if p.is_absolute() { p } else { ctx.cwd.as_ref()?.join(p) };
    // Windows ignores a trailing period when checking whether a path exists.
    if cfg!(windows) && full.extension().is_some_and(|ext| ext.is_empty()) {
        return None;
    }
    full.exists().then_some(full)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fallback {
    Var,
    Markdown,
    Unc,
}

impl Fallback {
    /// The file, without a sentence's period or a `:42` / `:42:7` place in it, if it exists.
    fn resolve(self, text: &str, ctx: &Ctx) -> Option<PathBuf> {
        let mut bare = text.strip_suffix('.').unwrap_or(text);
        for _ in 0..2 {
            match bare.rsplit_once(':') {
                Some((head, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => bare = head,
                _ => break,
            }
        }
        if self != Fallback::Var {
            return existing(text, ctx).or_else(|| existing(bare, ctx));
        }
        let expand = |s: &str| -> Option<PathBuf> {
            let (name, rest) = s.strip_prefix('$')?.split_once('/')?;
            let base = match name {
                "PWD" => ctx.cwd.clone()?,
                "HOME" => ctx.home.clone()?,
                _ => PathBuf::from((ctx.env)(name)?),
            };
            // A relative variable (`DIR=src`) is relative to the terminal's folder.
            let base = if base.is_absolute() { base } else { ctx.cwd.as_ref()?.join(base) };
            Some(base.join(rest)).filter(|p| p.exists())
        };
        expand(text).or_else(|| expand(bare))
    }
}

/// Paths SonicTerm doesn't find, covering byte `at`.
fn fallbacks(text: &str, at: usize) -> Vec<(Range<usize>, Fallback)> {
    let mut out = Vec::new();
    for (re, kind) in [(&*VAR_PATH, Fallback::Var), (&*UNC_PATH, Fallback::Unc)] {
        out.extend(re.find_iter(text).flatten().filter(|m| m.range().contains(&at)).map(|m| (m.range(), kind)));
    }
    // A markdown link's target: `[label](path)`.
    if let Some(open) = text[..at].rfind("](").map(|i| i + 2)
        && let Some(len) = text[open..].find(')')
        && (open..open + len).contains(&at)
        && !text[open..open + len].contains(|c: char| c.is_whitespace() || c == '(')
    {
        out.push((open..open + len, Fallback::Markdown));
    }
    out
}

/// Every link span covering byte `at`, without checking that files exist: URLs,
/// then SonicTerm's paths (shortest first), then the fallbacks.
pub fn spans(text: &str, at: usize, style: PathStyle) -> Vec<Range<usize>> {
    if !text.is_char_boundary(at) || at >= text.len() {
        return Vec::new();
    }
    if let Some(range) = url_spans(text).into_iter().find(|r| r.contains(&at)) {
        return vec![range];
    }
    let col = text[..at].chars().count();
    let mut out: Vec<Range<usize>> =
        url_scan::target_candidates_at_char_col_for_style(text, col, style, false).into_iter().rev().map(|m| m.start..m.end).collect();
    out.extend(fallbacks(text, at).into_iter().map(|(r, _)| r));
    out
}

/// Whether `text` is exactly one URL SonicTerm's scanner finds, with one `://`:
/// what a join of rows a program broke must spell to count as one link.
pub fn whole_url(text: &str) -> bool {
    let found = url_scan::find_urls(text);
    found.len() == 1 && found[0].start == 0 && found[0].url == text && text.matches("://").count() == 1
}

/// The narrowest link covering byte `at`, without checking that files exist,
/// read as POSIX text on every OS (for tests of where links show).
#[cfg(test)]
pub fn find(text: &str, at: usize) -> Option<Range<usize>> {
    spans(text, at, PathStyle::Posix).into_iter().next()
}

/// The address-shaped word under the pointer, excluding surrounding punctuation.
pub fn word(text: &str, at: usize) -> Option<Range<usize>> {
    let allowed = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '/' | '.');
    if !text.get(at..)?.chars().next().is_some_and(allowed) {
        return None;
    }
    let start = text[..at].char_indices().rev().find(|(_, c)| !allowed(*c)).map_or(0, |(i, c)| i + c.len_utf8());
    let end = text[at..].char_indices().find(|(_, c)| !allowed(*c)).map_or(text.len(), |(i, _)| at + i);
    let trimmed = text[start..end].trim_matches('.');
    let start = start + text[start..end].len() - text[start..end].trim_start_matches('.').len();
    let end = start + trimmed.len();
    (start <= at && at < end).then_some(start..end)
}

/// Bare addresses refer to the hovered terminal's machine, never its names.
pub fn terminal_address<'a>(word: &str, source: &str, live: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let address = if word.contains('/') {
        word.to_owned()
    } else {
        crate::mirror::qualify(word, source.split_once('/').map(|(machine, _)| machine))
    };
    live.into_iter().any(|a| a == address).then_some(address)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Url(String),
    File(PathBuf),
}

/// The local path a `file:` URL names: percent-decoded, `localhost` or no host;
/// on Windows `/C:/x` → `C:\x` and other hosts → UNC. `None` if not one, or if
/// it hides a separator (`%2f`, `%5c`) or has a `.` or `..` part.
pub fn file_url_path(url: &str) -> Option<PathBuf> {
    decode_file_url(url, cfg!(windows))
}

fn decode_file_url(url: &str, windows: bool) -> Option<PathBuf> {
    let rest = url.get(..5).filter(|s| s.eq_ignore_ascii_case("file:")).map(|_| &url[5..])?;
    let rest = rest.split(['?', '#']).next()?;
    let (host, path) = match rest.strip_prefix("//") {
        Some(r) => r.find('/').map_or((r, ""), |i| r.split_at(i)),
        None => ("", rest),
    };
    if !path.starts_with('/') {
        return None;
    }
    let lower = path.to_ascii_lowercase();
    if lower.contains("%2f") || lower.contains("%5c") {
        return None;
    }
    if dot_segment(path) {
        return None;
    }
    let path = String::from_utf8(percent_decode(path)?).ok()?;
    let local = host.is_empty() || host.eq_ignore_ascii_case("localhost");
    if !windows {
        return local.then(|| PathBuf::from(path));
    }
    let path = path.replace('/', "\\");
    if !local {
        return Some(PathBuf::from(format!(r"\\{host}{path}")));
    }
    let b = path.as_bytes();
    let drive = b.len() >= 3 && b[1].is_ascii_alphabetic() && b[2] == b':';
    Some(PathBuf::from(if drive { &path[1..] } else { &path[..] }))
}

fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let (mut out, b) = (Vec::new(), s.as_bytes());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            // Exactly two hex digits: `from_str_radix` alone would also take a sign (`%+A`).
            let hex = b.get(i + 1..i + 3).filter(|h| h.iter().all(u8::is_ascii_hexdigit))?;
            out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

/// What a marked (OSC 8) link opens. File paths count only when they exist here;
/// relative ones need the terminal's folder. A `file:` URL always names a file.
pub fn resolve(link: &str, cwd: Option<&Path>) -> Option<Target> {
    if link.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:")) {
        return file_url_path(link).map(Target::File);
    }
    let scheme = link.split_once(':').is_some_and(|(s, _)| {
        s.len() > 1 && s.starts_with(|c: char| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    });
    if scheme {
        return Some(Target::Url(link.to_owned()));
    }
    let home = std::env::home_dir();
    let expanded = if let Some(rest) = link.strip_prefix("~/") {
        home?.join(rest)
    } else if let Some(var) = link.strip_prefix('$') {
        let (name, rest) = var.split_once('/').unwrap_or((var, ""));
        let base = match name {
            "PWD" => cwd?.to_path_buf(),
            "HOME" => home?,
            _ => PathBuf::from(std::env::var_os(name)?),
        };
        base.join(rest)
    } else {
        PathBuf::from(link)
    };
    let full = if expanded.is_absolute() { expanded } else { cwd?.join(expanded) };
    // Windows ignores a trailing period when checking whether a path exists.
    if !(cfg!(windows) && full.extension().is_some_and(|ext| ext.is_empty())) && full.exists() {
        return Some(Target::File(full));
    }
    // The period ending a sentence, or `:42` / `:42:7` naming a place in the file.
    let mut bare = full.to_str()?.strip_suffix('.').unwrap_or(full.to_str()?);
    for _ in 0..2 {
        match bare.rsplit_once(':') {
            Some((head, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => bare = head,
            _ => break,
        }
    }
    Path::new(bare).exists().then(|| Target::File(PathBuf::from(bare)))
}


fn has_scheme(link: &str) -> bool {
    link.split_once(':').is_some_and(|(s, _)| {
        s.len() > 1 && s.starts_with(|c: char| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    })
}

// The period ending a sentence, or `:42` / `:42:7` naming a place in the file.
// Also a range, `:12-20` or `:12–20`, as Apollo's scanner reads one.
fn without_suffix(path: &str) -> &str {
    let mut bare = path.strip_suffix('.').unwrap_or(path);
    let number = |n: &str| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit());
    for _ in 0..2 {
        match bare.rsplit_once(':') {
            Some((head, n)) if number(n) || n.split_once(['-', '\u{2013}']).is_some_and(|(a, b)| number(a) && number(b)) => bare = head,
            _ => break,
        }
    }
    bare
}

/// A link's text and what Apollo opens for it (`native`), as mod link handlers see it.
pub struct Resolved<'a> {
    raw: &'a str,
    spelling: &'a str,
    pub native: Option<Target>,
}

/// A link's text with what Apollo already found it opens (`pick` or `resolve`).
/// Its file spelling drops a place (`:42`, `:42:7`, `:12-20`) or a final `.`,
/// unless the file found is named with them.
pub fn resolved(raw: &str, native: Option<Target>) -> Resolved<'_> {
    // The whole text is the file's name when the file found ends with it (`main.rs:7` exists).
    let literal = matches!(&native, Some(Target::File(p)) if p.file_name().and_then(|n| n.to_str())
        .is_some_and(|n| raw.rsplit(['/', '\\']).next() == Some(n)));
    let spelling = if literal || has_scheme(raw) { raw } else { without_suffix(raw) };
    Resolved { raw, spelling, native }
}

/// File rules see all file URIs. Only an empty authority or localhost
/// can name a local file.
fn handler_file_uri(link: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = link.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("file") {
        return None;
    }
    let rest = rest.split(['?', '#']).next()?;
    Some(if let Some(rest) = rest.strip_prefix("//").or_else(|| rest.strip_prefix(r"\\")) {
        rest.split_at(rest.find(['/', '\\']).unwrap_or(rest.len()))
    } else {
        ("", rest)
    })
}

fn decode_file_url_path(path: &str) -> Option<String> {
    let rest = path.split(['?', '#']).next()?;
    let mut bytes = Vec::with_capacity(rest.len());
    let mut i = 0;
    while i < rest.len() {
        if rest.as_bytes()[i] == b'%' {
            bytes.push(u8::from_str_radix(rest.get(i + 1..i + 3)?, 16).ok()?);
            i += 3;
        } else {
            bytes.push(rest.as_bytes()[i]);
            i += 1;
        }
    }
    String::from_utf8(bytes).ok()
}

fn uri_path(path: &str) -> PathBuf {
    // `/C:/Users/...` names `C:\Users\...`.
    if cfg!(windows) && path.as_bytes().get(2) == Some(&b':') && path.as_bytes()[1].is_ascii_alphabetic() {
        return PathBuf::from(path[1..].replace('/', "\\"));
    }
    PathBuf::from(path)
}

/// A path the system reaches over the network or through a device (`\\host\share`,
/// `\\?\UNC\…`, `\\.\…`); a verbatim disk path (`\\?\C:\…`) is local.
#[cfg(windows)]
fn network_path(path: &Path) -> bool {
    use std::path::{Component, Prefix};
    matches!(path.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..) | Prefix::DeviceNS(..)))
}

#[cfg(not(windows))]
fn network_path(_: &Path) -> bool {
    false
}

fn can_verify_handler_path(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};

        let bytes = path.as_os_str().as_encoded_bytes();
        if bytes.starts_with(br"\\") || bytes.starts_with(b"//")
            || matches!(path.components().next(), Some(Component::Prefix(prefix))
                if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..) | Prefix::DeviceNS(..)))
        {
            return false;
        }
    }
    path.has_root()
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Url,
    File,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "RuleSpec")]
pub struct Rule {
    kind: Kind,
    pattern: regex::Regex,
}

#[derive(Deserialize)]
struct RuleSpec {
    kind: Kind,
    pattern: String,
}

impl TryFrom<RuleSpec> for Rule {
    type Error = regex::Error;

    fn try_from(spec: RuleSpec) -> Result<Self, Self::Error> {
        Ok(Self { kind: spec.kind, pattern: regex::Regex::new(&spec.pattern)? })
    }
}

pub type Handlers = Vec<(String, std::rc::Rc<[Rule]>)>;

#[derive(Clone, Debug, PartialEq)]
pub struct Click {
    /// Each mod receives the link view selected by its first matching rule.
    pub handlers: Vec<(String, Value)>,
    pub native: Option<Target>,
}

impl Resolved<'_> {
    pub fn click(self, remote: bool, active: &Handlers) -> Option<Click> {
        let Self { raw, spelling, native } = self;
        if active.is_empty() {
            return native.map(native_click);
        }

        let is_url = has_scheme(raw);
        let uri = handler_file_uri(raw);
        let decoded = uri.and_then(|(_, path)| decode_file_url_path(path));
        let file_spelling = if let Some((_, path)) = uri {
            Some(decoded.as_deref().unwrap_or(path))
        } else if !is_url {
            // The desktop's files cannot tell us whether a remote suffix is literal.
            Some(if remote { without_suffix(raw) } else { spelling })
        } else {
            None
        };

        let mut handlers = Vec::new();
        let mut file_link = None;
        let mut url_link = None;
        for (id, rules) in active {
            if handlers.iter().any(|(name, _)| name == id) {
                continue;
            }
            let Some(rule) = rules.iter().find(|rule| match rule.kind {
                Kind::Url => is_url && rule.pattern.is_match(raw),
                Kind::File => file_spelling.is_some_and(|path| rule.pattern.is_match(path)),
            }) else { continue };
            let link = match rule.kind {
                Kind::Url => url_link.get_or_insert_with(|| json!({"kind": "url", "raw": raw, "url": raw})),
                Kind::File => file_link.get_or_insert_with(|| {
                    let url_file;
                    let path = if remote {
                        None
                    } else {
                        match &native {
                            // Apollo opens a local `file:` URL's folder even when the file is gone.
                            // Never a network path (UNC) on hover: that probe can block and authenticate.
                            Some(Target::File(path)) if !network_path(path) && path.exists() => Some(path.as_path()),
                            _ if uri.is_some_and(|(host, _)| host.is_empty() || host.eq_ignore_ascii_case("localhost")) => {
                                url_file = decoded.as_deref().map(uri_path)
                                    .filter(|path| can_verify_handler_path(path) && path.exists());
                                url_file.as_deref()
                            }
                            _ => None,
                        }
                    };
                    json!({"kind": "file", "raw": raw, "path": path.and_then(Path::to_str)})
                }),
            };
            handlers.push((id.clone(), link.clone()));
        }
        if handlers.is_empty() {
            return native.map(native_click);
        }

        let native = if remote && file_spelling.is_some() { None } else { native };
        Some(Click { handlers, native })
    }
}

fn native_click(native: Target) -> Click {
    Click { handlers: Vec::new(), native: Some(native) }
}

/// Returns true when a mod handles the click and false when native opening can proceed.
pub async fn dispatch<T, F>(
    handlers: impl IntoIterator<Item = (String, T)>,
    mut call: impl FnMut(&str, T) -> F,
) -> Result<bool, (String, String)>
where
    F: std::future::Future<Output = Result<String, String>>,
{
    for (name, entry) in handlers {
        let result = call(&name, entry).await.map_err(|error| (name.clone(), error))?;
        match result.as_str() {
            "handled" => return Ok(true),
            "pass" => {}
            _ => return Err((name, format!("handleLink returned {result:?}; expected \"handled\" or \"pass\""))),
        }
    }
    Ok(false)
}

/// Follows a link on a worker thread: deciding what it means touches the
/// filesystem, which must never stall the UI.
pub fn open(target: Target, opener: std::sync::Arc<dyn Opener + Send + Sync>) {
    if let Err(e) = std::thread::Builder::new().name("apollo-link".into()).spawn(move || activate(&target, &*opener)) {
        tracing::warn!("cannot start a thread to follow a link: {e}");
    }
}

/// A `file:` URL for `p` in tests, verbatim prefix dropped (`\\?\C:\x`, `\\?\UNC\s\x`).
#[cfg(test)]
pub(crate) fn file_url(p: &Path) -> String {
    let s = p.to_str().expect("a UTF-8 test path");
    let s = match (s.strip_prefix(r"\\?\UNC\"), s.strip_prefix(r"\\?\")) {
        (Some(unc), _) => format!(r"\\{unc}"),
        (None, Some(local)) => local.to_owned(),
        (None, None) => s.to_owned(),
    };
    let path = s.replace('\\', "/");
    if path.starts_with("//") {
        format!("file:{path}")
    } else if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What Apollo resolves `raw` to, ready for mod link handlers.
    fn for_handlers<'a>(raw: &'a str, cwd: Option<&Path>) -> Resolved<'a> {
        resolved(raw, resolve(raw, cwd))
    }

    fn native_target(link: &str, cwd: Option<&Path>) -> Option<Target> {
        resolve(link, cwd)
    }

    fn rules(value: Value) -> std::rc::Rc<[Rule]> {
        serde_json::from_value::<Vec<Rule>>(value).unwrap().into()
    }

    #[test]
    fn rules_match_the_link_kind_and_full_destination_in_layout_order() {
        let localhost = rules(json!([
            {"kind": "url", "pattern": "^https?://localhost(?::[0-9]+)?(?:[/?#]|$)"},
            {"kind": "url", "pattern": "#anchor$"}
        ]));
        let active = [
            ("specialist".into(), localhost.clone()),
            ("viewer".into(), rules(json!([{"kind": "file", "pattern": "\\.png$"}]))),
            ("browser".into(), rules(json!([{"kind": "url", "pattern": "^https?:"}]))),
            ("specialist".into(), localhost),
        ].to_vec();
        let url = "http://localhost:8123/view.png?a=1#anchor";
        let hit = for_handlers(url, None).click(true, &active).unwrap();
        let payload = json!({"kind":"url", "raw":url, "url":url});
        assert_eq!(hit.handlers, [("specialist".to_owned(), payload.clone()), ("browser".to_owned(), payload)]);
        assert_eq!(hit.native, Some(Target::Url(url.into())));

        let other = for_handlers("http://localhost.example.com", None).click(false, &active).unwrap();
        assert_eq!(other.handlers.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["browser"]);
        let foreign = "file://server/plot.png";
        let hit = for_handlers(foreign, None).click(false, &active).unwrap();
        assert_eq!(hit.handlers, [("viewer".to_owned(), json!({"kind":"file", "raw":foreign, "path":null}))]);
        // Apollo opens another host's `file:` URL only where the system can reach it (Windows UNC).
        assert_eq!(hit.native, native_target(foreign, None));
    }

    #[test]
    fn handlers_use_the_native_match_and_opener_destination() {
        use Kind::{File, Url};

        let tmp = if cfg!(windows) { std::env::temp_dir() } else { crate::test_tmp() };
        let cwd = tmp.join(format!("herdr-link-targets-{}", std::process::id()));
        for name in ["src/main.rs", "plot.png", "test folder/file.txt", "folder/file.txt", "peer/t7"] {
            let path = cwd.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        #[cfg(unix)]
        std::fs::write(cwd.join("src/main.rs:7"), "").unwrap();
        let literal = if cfg!(unix) { "./src/main.rs:7" } else { "./src/main.rs" };
        let active = vec![("editor".into(), rules(json!([
            {"kind": "url", "pattern": r"^(?:https?://localhost:|tel:|file://nas/)"},
            {"kind": "file", "pattern": r"\.(png|rs|txt|ini)$|^(\./src/main\.rs:7|src/\.\.)$"}
        ])))];
        let unmatched = vec![("unused".into(), rules(json!([{"kind":"file","pattern":"^$"},{"kind":"url","pattern":"^$"}])))];
        let f = |path: &str| Some(Target::File(cwd.join(path)));
        let u = |url: &str| Some(Target::Url(url.into()));
        let localhost = "http://localhost:8080/docs";
        let full_url = "http://localhost:8123/view.png?a=1#anchor";
        let nas = "file://nas/tmp/plot.png";
        let slashed = cwd.join("plot.png").to_string_lossy().replace('\\', "/");
        let encoded: String = slashed.bytes().map(|b| {
            if b.is_ascii_alphanumeric() || b"/-._~:".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }
        }).collect();
        let root = if cfg!(windows) { "/" } else { "" };
        let file_uri = format!("file://localhost{root}{encoded}");
        let upper_uri = format!("FiLe://LOCALHOST{root}{encoded}");
        let uri_file = Some(Target::File(uri_path(&format!("{root}{slashed}"))));

        // No pointer means OSC 8. The first target is master's opener
        // destination, the second is the destination after a handler passes.
        let cases = [
            ("(http://localhost:8080/docs)", Some("docs"), false, Some((Url, localhost)), u(localhost), u(localhost)),
            (full_url, None, true, Some((Url, full_url)), u(full_url), u(full_url)),
            ("tel:12345", Some("12345"), false, Some((Url, "tel:12345")), u("tel:12345"), u("tel:12345")),
            ("src/main.rs:12:5: error", Some("main.rs"), false, Some((File, "src/main.rs:12:5")), f("src/main.rs"), f("src/main.rs")),
            ("./src/main.rs:7", Some("main.rs"), false, Some((File, "./src/main.rs:7")), f(literal), f(literal)),
            // SonicTerm's scanner, which Apollo uses, links no bare `..` part.
            ("src/..", Some(".."), false, None, None, None),
            ("./test folder/file.txt", Some("file.txt"), false, Some((File, "./test folder/file.txt")), f("./test folder/file.txt"), f("./test folder/file.txt")),
            ("--out=$PWD/plot.png", Some("plot.png"), false, Some((File, "$PWD/plot.png")), f("plot.png"), f("plot.png")),
            ("./x.png", Some("x.png"), false, Some((File, "./x.png")), None, None),
            ("./gone.rs:42:7", Some("gone.rs"), false, Some((File, "./gone.rs:42:7")), None, None),
            ("./plot.png", Some("plot.png"), true, Some((File, "./plot.png")), f("./plot.png"), None),
            ("./x.png", Some("x.png"), true, Some((File, "./x.png")), None, None),
            (file_uri.as_str(), None, false, Some((File, file_uri.as_str())), uri_file.clone(), uri_file.clone()),
            (file_uri.as_str(), None, true, Some((File, file_uri.as_str())), uri_file.clone(), None),
            // Apollo reads a `file:` scheme in any case.
            (upper_uri.as_str(), None, true, Some((File, upper_uri.as_str())), uri_file.clone(), None),
            // What Apollo opens for a drive-letter `file:` URL is its own resolver's call.
            ("file:C:/Windows/win.ini", Some("win.ini"), true, Some((File, "file:C:/Windows/win.ini")), native_target("file:C:/Windows/win.ini", None), None),
            ("FiLe:C:/Windows/win.ini", None, true, Some((File, "FiLe:C:/Windows/win.ini")), native_target("FiLe:C:/Windows/win.ini", None), None),
            (r"file:C:\Windows\win.ini", None, true, Some((File, r"file:C:\Windows\win.ini")), native_target(r"file:C:\Windows\win.ini", None), None),
            // Another host's `file:` URL: Apollo opens it only where the system reaches it (Windows UNC).
            (nas, Some("plot.png"), false, Some((Url, nas)), native_target(nas, None), native_target(nas, None)),
            ("https://example.com/outside", Some("outside"), false, None, u("https://example.com/outside"), u("https://example.com/outside")),
            ("can't open plot.png", Some("plot.png"), false, None, None, None),
            ("peer/t7", Some("t7"), false, None, None, None),
        ];

        for (text, pointer, remote, handled, expected_native, expected_pass) in cases {
            let raw = match pointer {
                Some(word) => {
                    let at = text.find(word).unwrap() + word.len() - 1;
                    // As the app picks: an existing file from the folder, else the longest reading.
                    let ctx = Ctx::new(Some(cwd.clone()));
                    // `peer/t7` is a live terminal here: a peek, never a link.
                    match pick(text, at, false, &ctx, |w| (w == "peer/t7").then(|| w.to_owned())) {
                        Some((_, Picked::Terminal(_))) => None,
                        Some((range, _)) => Some(&text[range]),
                        None => spans(text, at, PathStyle::Posix).into_iter().max_by_key(|r| r.len()).map(|range| &text[range]),
                    }
                }
                None => Some(text),
            };
            let click = |rules: &Handlers| raw.and_then(|raw| for_handlers(raw, Some(&cwd)).click(remote, rules));
            let expected_payload = handled.map(|(kind, expected_raw)| {
                assert_eq!(raw, Some(expected_raw), "{text}");
                match kind {
                    Url => json!({"kind":"url", "raw":expected_raw, "url":expected_raw}),
                    File => {
                        let path = match &expected_pass { Some(Target::File(path)) => Some(path.as_path()), _ => None };
                        json!({"kind":"file", "raw":expected_raw, "path":path.map(Path::to_string_lossy)})
                    }
                }
            });
            let hit = click(&active);
            let payload = hit.as_ref().and_then(|click| click.handlers.first().map(|(_, link)| link));
            assert_eq!(payload, expected_payload.as_ref(), "handler payload for {text}");
            assert_eq!(hit.and_then(|click| click.native), expected_pass, "opener after pass for {text}");
            assert_eq!(click(&Vec::new()), expected_native.clone().map(native_click), "no handlers for {text}");
            assert_eq!(click(&unmatched), expected_native.map(native_click), "no rule matched {text}");
        }
        let at = "peer/t7".len() - 1;
        assert_eq!(terminal_address(&"peer/t7"[word("peer/t7", at).unwrap()], "t1", ["peer/t7"]).as_deref(), Some("peer/t7"));
        std::fs::remove_dir_all(cwd).unwrap();
    }

    #[test]
    fn file_rules_see_a_source_place_dropped() {
        let dir = std::env::temp_dir().join(format!("apollo-link-place-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/x.rs"), "").unwrap();
        let active = vec![("editor".into(), rules(json!([{"kind":"file", "pattern":r"^src/x\.rs$"}])))];
        let ctx = Ctx::new(Some(dir.clone()));
        for text in ["src/x.rs:12-20", "src/x.rs:12\u{2013}20", "src/x.rs:12:3", "src/x.rs."] {
            let (range, picked) = pick(text, 1, false, &ctx, |_| None).unwrap_or_else(|| panic!("no link in {text}"));
            let Picked::Link(target) = picked else { panic!("{text}") };
            let hit = resolved(&text[range.clone()], Some(target)).click(false, &active).unwrap();
            assert_eq!(hit.handlers.len(), 1, "{text} ({:?})", &text[range]);
            // Remote, or a file that isn't here: the rule sees the same name.
            let remote = resolved(&text[range.clone()], None).click(true, &active).unwrap();
            assert_eq!(remote.handlers.len(), 1, "remote {text}");
        }
        let gone = vec![("editor".into(), rules(json!([{"kind":"file", "pattern":r"^src/gone\.rs$"}])))];
        assert_eq!(resolved("src/gone.rs:12-20", None).click(false, &gone).unwrap().handlers.len(), 1);
        // A file named with digits: the place after it never reads as its name.
        std::fs::write(dir.join("src/12"), "").unwrap();
        let digits = vec![("editor".into(), rules(json!([{"kind":"file", "pattern":r"^\./src/12$"}])))];
        for text in ["./src/12:7", "./src/12:12", "./src/12:12-20"] {
            let (range, picked) = pick(text, 3, false, &ctx, |_| None).unwrap_or_else(|| panic!("no link in {text}"));
            let Picked::Link(native) = picked else { panic!("{text}") };
            assert_eq!(resolved(&text[range], Some(native)).click(false, &digits).unwrap().handlers.len(), 1, "{text}");
        }
        // A file named with its place keeps it (`main.rs:7` is the file).
        #[cfg(unix)]
        {
            std::fs::write(dir.join("src/main.rs:7"), "").unwrap();
            let literal = vec![("editor".into(), rules(json!([{"kind":"file", "pattern":r"^src/main\.rs:7$"}])))];
            let native = resolve("src/main.rs:7", Some(&dir));
            assert_eq!(resolved("src/main.rs:7", native).click(false, &literal).unwrap().handlers.len(), 1);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_verbatim_disk_path_is_local_and_unc_is_not() {
        let dir = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        assert!(dir.to_string_lossy().starts_with(r"\\?\"), "{dir:?}");
        assert!(!network_path(&dir));
        assert!(network_path(Path::new(r"\\host\share\x.png")));
        assert!(network_path(Path::new(r"\\?\UNC\host\share\x.png")));
        assert!(network_path(Path::new(r"\\.\device\x")));
    }

    #[test]
    fn handlers_never_probe_a_network_path() {
        let unc = PathBuf::from(r"\\unreachable-host\share\plot.png");
        let active = vec![("files".into(), rules(json!([{"kind":"file", "pattern":r"\.png$"}])))];
        let raw = r"\\unreachable-host\share\plot.png";
        let hit = resolved(raw, Some(Target::File(unc.clone()))).click(false, &active).unwrap();
        assert_eq!(hit.handlers[0].1["path"], Value::Null);
        assert_eq!(hit.native, Some(Target::File(unc)));
    }

    #[test]
    fn file_uris_offer_both_views_and_keep_the_local_opener() {
        let dir = std::env::temp_dir().join(format!("herdr-file-uri-handlers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plot.png"), "").unwrap();
        let slashed = dir.to_string_lossy().replace('\\', "/");
        let root = if cfg!(windows) { "/" } else { "" };
        let encoded: String = slashed.bytes().map(|b| {
            if b.is_ascii_alphanumeric() || b"/-._~:".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }
        }).collect();
        let active = vec![
            ("files".into(), rules(json!([{"kind":"file", "pattern":r"\.png$"}]))),
            ("urls".into(), rules(json!([{"kind":"url", "pattern":"(?i)^file:"}]))),
        ];
        let urls_only = vec![active[1].clone()];
        let drive_uri = "file:C:/Windows/win.ini";
        assert_eq!(for_handlers(drive_uri, None).click(true, &urls_only).unwrap().native, None);
        let unmatched = vec![("unused".into(), rules(json!([{"kind":"url", "pattern":"^https:"}])))];
        let existing = uri_path(&format!("{root}{slashed}/plot.png"));
        for (name, exists) in [("plot.png", true), ("missing.png", false)] {
            let variants = [
                (format!("file:{root}{encoded}/{name}"), "", true),
                (format!("file://{root}{encoded}/{name}"), "", true),
                (format!("file://localhost{root}{encoded}/{name}"), "localhost", true),
                (format!("file://host{root}{encoded}/{name}"), "host", false),
                (format!(r"file:\\nas\share\{name}"), "nas", false),
                (format!(r"file://\\nas\share\{name}"), "", false),
                #[cfg(windows)]
                (format!("file:%2F%2Fnas%2Fshare%2F{name}"), "", false),
                #[cfg(windows)]
                (format!("file:%5C%5C%3F%5CUNC%5Cnas%5Cshare%5C{name}"), "", false),
                #[cfg(windows)]
                (format!("file:%5C%5C.%5Cdevice%5C{name}"), "", false),
                (format!("FiLe:{name}"), "", false),
            ];
            for (raw, authority, can_verify_local) in variants {
                let (host, spelling) = handler_file_uri(&raw).unwrap();
                assert_eq!(host, authority, "{raw}");
                if host.is_empty() && !can_verify_local {
                    // Check the guard before the click can reach a filesystem probe.
                    let path = uri_path(&decode_file_url_path(spelling).unwrap());
                    assert!(!can_verify_handler_path(&path), "{raw}");
                }
                let original = native_target(&raw, None);
                for remote in [false, true] {
                    let verified = exists && !remote && can_verify_local;
                    let hit = for_handlers(&raw, None).click(remote, &active).unwrap();
                    let path = verified.then(|| existing.to_str().unwrap());
                    let url = json!({"kind":"url", "raw":raw, "url":raw});
                    let file = json!({"kind":"file", "raw":raw, "path":path});
                    let native = if remote { None } else { original.clone() };
                    assert_eq!(hit.handlers, [("files".to_owned(), file), ("urls".to_owned(), url.clone())], "{raw}");
                    assert_eq!(hit.native, native, "{raw}");

                    let hit = for_handlers(&raw, None).click(remote, &urls_only).unwrap();
                    assert_eq!(hit.handlers, [("urls".to_owned(), url)], "{raw}");
                    assert_eq!(hit.native, native, "{raw}");
                    assert_eq!(
                        for_handlers(&raw, None).click(remote, &unmatched),
                        original.clone().map(native_click),
                        "{raw}"
                    );
                }
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_non_utf8_path_is_null_in_the_payload_and_exact_for_a_pass() {
        use std::os::unix::ffi::OsStringExt;

        let dir = crate::test_tmp().join(format!("herdr-link-non-utf8-{}", std::process::id()));
        let cwd = dir.join(std::ffi::OsString::from_vec(b"assets-\xff".to_vec()));
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(cwd.join("plot.png"), "").unwrap();
        let active = vec![("files".into(), rules(json!([{"kind":"file", "pattern":r"\.png$"}])))];
        let raw = "./plot.png";
        let hit = for_handlers(raw, Some(&cwd)).click(false, &active).unwrap();
        assert_eq!(hit.handlers[0].1, json!({"kind":"file", "raw":raw, "path":null}));
        assert_eq!(hit.native, Some(Target::File(cwd.join(raw))));

        let uri = format!("file://{}/assets-%FF/plot.png", dir.display());
        let hit = for_handlers(&uri, None).click(false, &active).unwrap();
        assert_eq!(hit.handlers[0].1, json!({"kind":"file", "raw":uri, "path":null}));
        // Apollo's resolver opens no `file:` URL whose path isn't UTF-8.
        assert_eq!(hit.native, native_target(&uri, None));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dispatch_keeps_passing_in_order_and_stops_on_handled_or_error() {
        use std::{future::{Future, ready}, pin::pin, task::{Context, Poll, Waker}};

        for (second, expected, calls) in [
            (Ok("pass"), Ok(false), 3),
            (Ok("handled"), Ok(true), 2),
            (Ok("invalid"), Err(("second".to_owned(), "handleLink returned \"invalid\"; expected \"handled\" or \"pass\"".into())), 2),
            (Err("tunnel failed"), Err(("second".to_owned(), "tunnel failed".into())), 2),
        ] {
            let mut visited = Vec::new();
            let handlers = [("first".into(), Ok("pass")), ("second".into(), second), ("third".into(), Ok("pass"))];
            {
                let mut future = pin!(dispatch(handlers, |name, result| {
                    visited.push(name.to_owned());
                    ready(result.map(str::to_owned).map_err(str::to_owned))
                }));
                assert_eq!(future.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Ready(expected));
            }
            assert_eq!(visited, ["first", "second", "third"][..calls]);
        }
    }

    /// A scratch folder with `files` in it (a trailing `/` makes a folder), and
    /// links looked up from `<root>/w/x` with home `<root>/home`.
    struct Fixture {
        root: PathBuf,
        ctx: Ctx,
    }

    impl Fixture {
        fn new(name: &str, files: &[&str]) -> Self {
            let root = std::env::temp_dir().join(format!("apollo-links-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let cwd = root.join("w/x");
            std::fs::create_dir_all(&cwd).unwrap();
            let ctx = Ctx { cwd: Some(cwd), home: Some(root.join("home")), style: PathStyle::Posix, env: |name| (name == "DIR").then(|| "src".into()) };
            let f = Fixture { root, ctx };
            for file in files {
                f.add(file);
            }
            f
        }

        /// Where `path` lands: absolute ones under the root, `~/` in home, others from the cwd.
        fn at(&self, path: &str) -> PathBuf {
            if let Some(rest) = path.strip_prefix('/') {
                self.root.join(rest)
            } else if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("$HOME/")) {
                self.ctx.home.as_ref().unwrap().join(rest)
            } else if let Some(rest) = path.strip_prefix("$PWD/") {
                self.ctx.cwd.as_ref().unwrap().join(rest)
            } else {
                self.ctx.cwd.as_ref().unwrap().join(path)
            }
        }

        fn add(&self, path: &str) {
            let p = self.at(path);
            if path.ends_with('/') {
                std::fs::create_dir_all(p).unwrap();
            } else {
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, "").unwrap();
            }
        }

        /// Absolute paths in `s` moved under the root.
        fn rooted(&self, s: &str) -> String {
            let root = self.root.to_str().unwrap();
            let mut out = String::new();
            let mut prev = ' ';
            for c in s.chars() {
                if c == '/' && matches!(prev, ' ' | '(' | '"' | '\'') {
                    out.push_str(root);
                }
                out.push(c);
                prev = c;
            }
            out
        }

        fn pick(&self, text: &str, at: usize) -> Option<(Range<usize>, Picked)> {
            pick(text, at, false, &self.ctx, |_| None)
        }

        /// The text picked at the start of `want` in `text`.
        fn picked<'a>(&self, text: &'a str, want: &str) -> Option<&'a str> {
            self.pick(text, text.find(want)?).map(|(r, _)| &text[r])
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Ghostty's cases Apollo kept, through `pick` with the files they name in place.
    #[cfg(unix)]
    #[test]
    fn picks_like_ghostty_where_files_exist() {
        let urls = [
            ("hello https://example.com world", "https://example.com"),
            ("https://example.com/foo(bar) more", "https://example.com/foo(bar)"),
            ("https://example.com/foo(bar)baz more", "https://example.com/foo(bar)baz"),
            ("Link inside (https://example.com) parens", "https://example.com"),
            ("Link period https://example.com. More text.", "https://example.com"),
            ("Link trailing comma https://example.com, more text.", "https://example.com"),
            ("Link in double quotes \"https://example.com\" and more", "https://example.com"),
            ("Link in single quotes 'https://example.com' and more", "https://example.com"),
            ("some file with https://google.com https://duckduckgo.com links.", "https://google.com"),
            ("and links in it. links https://yahoo.com mailto:test@example.com ssh://1.2.3.4", "https://yahoo.com"),
            ("and links in it. links https://yahoo.com mailto:test@example.com ssh://1.2.3.4", "mailto:test@example.com"),
            ("and links in it. links https://yahoo.com mailto:test@example.com ssh://1.2.3.4", "ssh://1.2.3.4"),
            ("also match http://example.com non-secure links", "http://example.com"),
            ("match tel://+12123456789 phone numbers", "tel://+12123456789"),
            ("match with query url https://example.com?query=1&other=2 and more text.", "https://example.com?query=1&other=2"),
            (
                "url with dashes [mode 2027](https://github.com/contour-terminal/terminal-unicode-core) for better unicode support",
                "https://github.com/contour-terminal/terminal-unicode-core",
            ),
            ("weird characters https://example.com/~user/?query=1&other=2#hash and more", "https://example.com/~user/?query=1&other=2#hash"),
            ("square brackets https://example.com/[foo] and more", "https://example.com/[foo]"),
            (
                "[13]:TooManyStatements: TempFile#assign_temp_file_to_entity has approx 7 statements [https://example.com/docs/Too-Many-Statements.md]",
                "https://example.com/docs/Too-Many-Statements.md",
            ),
            ("match ftp://example.com ftp links", "ftp://example.com"),
            ("match ssh://example.com ssh links", "ssh://example.com"),
            ("match git://example.com git links", "git://example.com"),
            ("match tel:+18005551234 tel links", "tel:+18005551234"),
            ("match magnet:?xt=urn:btih:1234567890 magnet links", "magnet:?xt=urn:btih:1234567890"),
            ("match ipfs://QmSomeHashValue ipfs links", "ipfs://QmSomeHashValue"),
            ("match ipns://QmSomeHashValue ipns links", "ipns://QmSomeHashValue"),
            ("match gemini://example.com gemini links", "gemini://example.com"),
            ("match gopher://example.com gopher links", "gopher://example.com"),
            ("match news:comp.infosystems.www.servers.unix news links", "news:comp.infosystems.www.servers.unix"),
            ("Serving HTTP on :: port 8000 (http://[::]:8000/)", "http://[::]:8000/"),
            ("IPv6 address https://[2001:db8::1]:8080/path", "https://[2001:db8::1]:8080/path"),
            ("IPv6 localhost http://[::1]:3000", "http://[::1]:3000"),
            ("Complex IPv6 https://[2001:db8:85a3:8d3:1319:8a2e:370:7348]:443/", "https://[2001:db8:85a3:8d3:1319:8a2e:370:7348]:443/"),
            ("IPv6 with path https://[2001:db8::1]/path/to/resource", "https://[2001:db8::1]/path/to/resource"),
            ("IPv6 with query https://[2001:db8::1]:8080/api?param=value&other=123", "https://[2001:db8::1]:8080/api?param=value&other=123"),
            ("IPv6 compressed http://[2001:db8::]:80/", "http://[2001:db8::]:80/"),
            ("IPv6 multiple zeros http://[2001:0:0:0:0:0:0:1]", "http://[2001:0:0:0:0:0:0:1]"),
            ("IPv6 link-local https://[fe80::1234:5678:9abc]", "https://[fe80::1234:5678:9abc]"),
            ("IPv6 multicast http://[ff02::1]/stream", "http://[ff02::1]/stream"),
            ("IPv6 in markdown [link](http://[2001:db8::1]/docs)", "http://[2001:db8::1]/docs"),
            ("see HTTPS://Example.com ok", "HTTPS://Example.com"),
        ];
        let f = Fixture::new("ghostty-urls", &[]);
        for (text, want) in urls {
            assert_eq!(f.picked(text, want), Some(want), "{text:?}");
        }
        // (text, what links, the file that makes it one)
        let paths = [
            ("/tmp/test.txt http://www.google.com", "/tmp/test.txt", "/tmp/test.txt"),
            ("/Users/ghostty.user/code/example.py", "/Users/ghostty.user/code/example.py", "/Users/ghostty.user/code/example.py"),
            ("/Users/ghostty.user/code/../example.py", "/Users/ghostty.user/code/../example.py", "/Users/ghostty.user/example.py"),
            ("/Users/ghostty.user/code/../example.py hello world", "/Users/ghostty.user/code/../example.py", "/Users/ghostty.user/example.py"),
            ("../example.py", "../example.py", "../example.py"),
            ("../example.py ", "../example.py", "../example.py"),
            ("first time ../example.py contributor ", "../example.py", "../example.py"),
            ("[link](/home/user/ghostty.user/example)", "/home/user/ghostty.user/example", "/home/user/ghostty.user/example"),
            ("./spaces-end.   ", "./spaces-end.", "./spaces-end."),
            ("./space middle", "./space middle", "./space middle"),
            ("../test folder/file.txt", "../test folder/file.txt", "../test folder/file.txt"),
            ("/tmp/test folder/file.txt", "/tmp/test folder/file.txt", "/tmp/test folder/file.txt"),
            ("/tmp/test  folder/file.txt", "/tmp/test", "/tmp/test"),
            ("diff --git a/src/font/shaper/harfbuzz.zig b/src/font/shaper/harfbuzz.zig", "a/src/font/shaper/harfbuzz.zig", "a/src/font/shaper/harfbuzz.zig"),
            ("/tmp/foo /tmp/bar", "/tmp/foo", "/tmp/foo"),
            ("/tmp/foo.txt /tmp/bar.txt", "/tmp/foo.txt", "/tmp/foo.txt"),
            ("src/config/url.zig", "src/config/url.zig", "src/config/url.zig"),
            ("app/folder/file.rb:1", "app/folder/file.rb:1", "app/folder/file.rb"),
            ("modified:   src/config/url.zig", "src/config/url.zig", "src/config/url.zig"),
            ("lib/ghostty/terminal.zig:42:10", "lib/ghostty/terminal.zig:42:10", "lib/ghostty/terminal.zig"),
            ("some-pkg/src/file.txt more text", "some-pkg/src/file.txt", "some-pkg/src/file.txt"),
            ("src/foo.c,baz.txt", "src/foo.c", "src/foo.c"),
            ("~/foo/bar.txt", "~/foo/bar.txt", "~/foo/bar.txt"),
            ("open ~/Documents/notes.md please", "~/Documents/notes.md", "~/Documents/notes.md"),
            ("~/.config/ghostty/config", "~/.config/ghostty/config", "~/.config/ghostty/config"),
            ("directory: ~/src/ghostty-org/ghostty", "~/src/ghostty-org/ghostty", "~/src/ghostty-org/ghostty/"),
            ("$HOME/src/config/url.zig", "$HOME/src/config/url.zig", "$HOME/src/config/url.zig"),
            ("space dir: $PWD/src/ghostty/main.zig", "$PWD/src/ghostty/main.zig", "$PWD/src/ghostty/main.zig"),
            (".config/ghostty/config", ".config/ghostty/config", ".config/ghostty/config"),
            ("loaded from .local/share/ghostty/state.db now", ".local/share/ghostty/state.db", ".local/share/ghostty/state.db"),
            ("../some/where", "../some/where", "../some/where"),
            ("  - shared/src/foo/SomeItem.m:12, shared/src/", "shared/src/foo/SomeItem.m:12", "shared/src/foo/SomeItem.m"),
            ("foo.local/share", "foo.local/share", "foo.local/share"),
            ("2024/report.txt", "2024/report.txt", "2024/report.txt"),
            ("./foo bar,baz", "./foo", "./foo"),
            ("/tmp/foo bar,baz", "/tmp/foo", "/tmp/foo"),
            ("./.config/ghostty: Needs upstream (main)", "./.config/ghostty", "./.config/ghostty"),
            ("./Downloads: Operation not permitted", "./Downloads", "./Downloads/"),
        ];
        for (i, (text, want, file)) in paths.into_iter().enumerate() {
            // A `..` steps out of a folder that has to exist.
            let parents: Vec<&str> = want.match_indices("/../").map(|(i, _)| &want[..=i]).collect();
            let f = Fixture::new(&format!("ghostty-{i}"), &[&[file][..], &parents[..]].concat());
            let (text, want) = (f.rooted(text), f.rooted(want));
            let got = f.picked(&text, &want);
            assert_eq!(got, Some(want.as_str()), "{text:?}");
            let Some((_, Picked::Link(Target::File(p)))) = f.pick(&text, text.find(&want).unwrap()) else { panic!("{text:?}") };
            assert!(p.exists(), "{text:?} -> {p:?}");
        }
    }

    /// What Apollo links differently from Ghostty since it follows SonicTerm (#3).
    #[cfg(unix)]
    #[test]
    fn ghostty_cases_sonicterm_changed() {
        let f = Fixture::new("changed", &["foo/bar", "input/output", "foo/$BAR/baz", ".foo/bar/$VAR", "./foo bar,baz"]);
        // A scheme run into a word isn't a link, nor a file: URL naming no local path.
        assert_eq!(f.picked("dot.http://example.com", "http"), None);
        assert_eq!(f.picked("match file://example.com file links", "file"), None);
        // A variable after the start of a path.
        assert_eq!(f.picked("foo/$BAR/baz", "foo"), None);
        assert_eq!(f.picked(".foo/bar/$VAR", ".foo"), None);
        // Relative words link when they exist, the whole spaced name first.
        assert_eq!(f.picked("input/output", "input"), Some("input/output"));
        assert_eq!(f.picked("foo/bar", "foo"), Some("foo/bar"));
        assert_eq!(f.picked("./foo bar,baz", "./foo"), Some("./foo bar,baz"));
        let empty = Fixture::new("changed-empty", &[]);
        for text in ["input/output", "foo/bar", "$10/bar", "$10/$20", "$10/bar.txt", "foo/bar,baz.txt", "foo$BAR/baz.txt", "foo~/bar.txt", "// foo bar", "//foo"] {
            for at in 0..text.len() {
                assert_eq!(empty.pick(text, at), None, "{text:?} at {at}");
            }
        }
    }

    #[test]
    fn rejected_text_stays_unlinked_even_when_the_file_exists() {
        let f = Fixture::new("rejects", &["src/main.rs"]);
        for text in ["xhttp://a.com", "src/main.rs:0", "'src/main.rs'tail", "xftp://a.com", "a-ssh://h"] {
            for at in 0..text.len() {
                let got = f.pick(text, at);
                assert!(!matches!(&got, Some((_, Picked::Link(Target::Url(_))))), "{text:?} at {at}: {got:?}");
                if text != "xftp://a.com" && text != "a-ssh://h" {
                    assert_eq!(got, None, "{text:?} at {at}");
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn other_schemes_win_over_files_named_like_them() {
        let names = ["tel:+18005551234", "news:comp.infosystems.www.servers.unix", "magnet:?xt=urn:btih:1234567890"];
        let f = Fixture::new("schemes", &names);
        for name in names {
            for at in 0..name.len() {
                assert_eq!(f.pick(name, at), Some((0..name.len(), Picked::Link(Target::Url(name.into())))), "{name} at {at}");
            }
        }
    }

    #[test]
    fn urls_keep_their_brackets_and_leave_wrappers_out() {
        let f = Fixture::new("brackets", &[]);
        for url in ["https://a.com/foo(bar)", "https://example.com/?q=(value)", "https://[::1]", "http://[::1]:3000", "https://example.com/?a=1&b=2"] {
            let text = format!("see {url} ok");
            for at in 4..4 + url.len() {
                assert_eq!(f.pick(&text, at), Some((4..4 + url.len(), Picked::Link(Target::Url(url.into())))), "{url} at {at}");
            }
        }
        for (text, url) in [("(https://a.com)", "https://a.com"), ("[https://a.com]", "https://a.com"), ("(see https://a.com/x(y))", "https://a.com/x(y)")] {
            let start = text.find(url).unwrap();
            let end = start + url.len();
            assert_eq!(f.pick(text, start).map(|(r, _)| r), Some(start..end), "{text}");
            assert_eq!(f.pick(text, end), None, "{text}: the closer isn't the link's");
        }
    }

    #[test]
    fn live_addresses_win_over_same_named_files() {
        let f = Fixture::new("addresses", &["t1", "macbook/t1", "src/main.rs"]);
        let live = |w: &str| ["t1", "macbook/t1"].contains(&w).then(|| w.to_owned());
        for text in ["t1", "macbook/t1"] {
            for at in 0..text.len() {
                assert_eq!(pick(text, at, false, &f.ctx, live), Some((0..text.len(), Picked::Terminal(text.into()))), "{text}");
            }
            // Not live: the file links.
            assert!(matches!(f.pick(text, 0), Some((_, Picked::Link(Target::File(_))))), "{text}");
        }
        // Joined lines never make an address.
        assert!(!matches!(pick("t1", 0, true, &f.ctx, live), Some((_, Picked::Terminal(_)))));
        // URLs and other paths stay theirs.
        assert!(matches!(pick("https://t1", 9, false, &f.ctx, live), Some((_, Picked::Link(Target::Url(_))))));
        assert!(matches!(pick("src/main.rs", 0, false, &f.ctx, live), Some((_, Picked::Link(Target::File(_))))));
    }

    #[test]
    fn source_references_highlight_the_place_and_open_the_file() {
        let f = Fixture::new("sources", &["src/main.rs", "src/x.rs"]);
        let main = f.at("src/main.rs");
        let text = "é src/main.rs:12";
        let start = text.find("src").unwrap();
        for at in start..text.len() {
            assert_eq!(f.pick(text, at), Some((start..text.len(), Picked::Link(Target::File(main.clone())))), "at {at}");
        }
        for text in ["src/x.rs:12:3", "src/x.rs:12-20", "see src/x.rs:12:3, ok"] {
            let span = if text.starts_with("see") { "src/x.rs:12:3" } else { text };
            assert_eq!(f.picked(text, span), Some(span), "{text}");
            assert_eq!(f.pick(text, text.find(span).unwrap()).unwrap().1, Picked::Link(Target::File(f.at("src/x.rs"))));
        }
    }

    #[test]
    fn spaced_and_tool_wrapped_paths_link_the_file_that_exists() {
        let f = Fixture::new("spaced", &["my dir/a.txt", "dir/a.txt", "src/a.rs", "docs/b.md"]);
        assert_eq!(f.picked("open my dir/a.txt now", "my"), Some("my dir/a.txt"));
        assert_eq!(f.picked("open my dir/a.txt now", "dir"), Some("my dir/a.txt"));
        assert_eq!(f.picked("⏺ Read(src/a.rs)", "src"), Some("src/a.rs"));
        assert_eq!(f.picked("go `src/a.rs` now", "src"), Some("src/a.rs"));
        assert_eq!(f.picked("Done: src/a.rs, docs/b.md.", "src"), Some("src/a.rs"));
        assert_eq!(f.picked("Done: src/a.rs, docs/b.md.", "docs"), Some("docs/b.md"));
        assert_eq!(f.picked("see [notes](docs/b.md) ok", "docs"), Some("docs/b.md"));
        assert_eq!(f.picked("see [notes](docs/gone.md) ok", "docs"), None);
    }

    #[cfg(unix)]
    #[test]
    fn bare_names_link_only_in_the_terminals_folder() {
        let f = Fixture::new("bare", &["Cargo.toml", "README.md"]);
        assert_eq!(f.picked("Cargo.toml  README.md  src", "README"), Some("README.md"));
        assert_eq!(f.picked("Cargo.toml  README.md  src", "Cargo"), Some("Cargo.toml"));
        assert_eq!(f.picked("Cargo.toml  README.md  src", "src"), None);
        assert_eq!(f.picked("the word Cargo", "Cargo"), None);
        let nowhere = Ctx { cwd: None, ..f.ctx.clone() };
        assert_eq!(pick("README.md", 0, false, &nowhere, |_| None), None);
    }

    #[test]
    fn missing_literals_come_before_their_alternatives() {
        let text = "src/a.rs, b.rs";
        let at = text.find("b.rs").unwrap();
        // SonicTerm offers `b.rs` only once the literal `src/a.rs, b.rs` is missing.
        let col = text[..at].chars().count();
        let found = url_scan::target_candidates_at_char_col_for_style(text, col, PathStyle::Posix, true);
        assert!(found.iter().any(|m| m.missing_before == vec![DetectedTarget::PathCandidate(text.into())]));
        let f = Fixture::new("missing", &["b.rs"]);
        assert_eq!(f.pick(text, at).map(|(r, _)| &text[r]), Some("b.rs"));
        f.add(text);
        assert_eq!(f.pick(text, at).map(|(r, _)| &text[r]), Some(text));
        // Even listed first, an alternative waits for its literal to be missing.
        let mut reordered = found.clone();
        reordered.sort_by_key(|m| m.missing_before.is_empty());
        assert!(!reordered[0].missing_before.is_empty());
        assert_eq!(first_existing(reordered, &f.ctx).map(|(r, _)| &text[r]), Some(text));
    }

    #[test]
    fn file_urls_hiding_separators_or_parents_never_open() {
        let f = Fixture::new("file-urls", &["tmp/a/b", "etc/passwd"]);
        let root = f.root.to_str().unwrap().replace('\\', "/");
        let root = root.trim_start_matches('/');
        for bad in [
            format!("file:///{root}/tmp/a%2fb"),
            format!("file:///{root}/tmp/a%2Fb"),
            format!("file:///{root}/tmp/a%5cb"),
            format!("file:///{root}/tmp/%2e%2e/etc/passwd"),
            format!("file:///{root}/tmp/../etc/passwd"),
        ] {
            assert_eq!(file_url_path(&bad), None, "{bad}");
            assert_eq!(f.pick(&bad, 2), None, "text {bad}");
            assert_eq!(resolve(&bad, None), None, "marked {bad}");
            let rec = Recorder::default();
            activate(&Target::Url(bad.clone()), &rec);
            assert!(rec.calls.take().iter().all(|c| !matches!(c, Call::Reveal(_))), "activated {bad}");
        }
        let good = format!("file:///{root}/tmp/a/b");
        assert_eq!(f.pick(&good, 2).unwrap().1, Picked::Link(Target::File(file_url_path(&good).unwrap())));
        for text in ["see FILE:/tmp/x#top ok", "see file:/tmp/x ok", "see FILE:///tmp/report.txt ok"] {
            let start = text.find(['F', 'f']).unwrap();
            let end = text[start..].find(' ').unwrap() + start;
            assert_eq!(f.pick(text, start).map(|(r, _)| r), Some(start..end), "{text}");
        }
    }

    #[test]
    fn file_urls_trimmed_by_the_scanner_still_count_their_dots() {
        let f = Fixture::new("dots", &["tmp/x"]);
        let root = f.root.to_str().unwrap().replace('\\', "/");
        let root = root.trim_start_matches('/');
        for tail in ["..", "%2e.", "%2E%2E", ".", "./x", "..,", "..;", "../x,"] {
            let text = format!("see file:///{root}/tmp/{tail} then continue");
            for at in 4..text.find(" then").unwrap() {
                assert_eq!(f.pick(&text, at), None, "{text} at {at}");
            }
            assert_eq!(file_url_path(&format!("file:///{root}/tmp/{tail}")), None, "{tail}");
        }
        // A name of three dots is a name, not a step up.
        assert!(decode_file_url("file:///tmp/.../x", false).is_some());
        #[cfg(unix)] // Windows can't make one.
        {
            f.add("tmp/.../x");
            let text = format!("see file:///{root}/tmp/.../x ok");
            let Some((_, Picked::Link(Target::File(p)))) = f.pick(&text, 4) else { panic!("{text}") };
            assert!(p.ends_with("tmp/.../x"), "{p:?}");
        }
        // A trailing period ends the sentence, not the path.
        let text = format!("see file:///{root}/tmp/x. ok");
        let Some((_, Picked::Link(Target::File(p)))) = f.pick(&text, 4) else { panic!("{text}") };
        assert!(p.ends_with("tmp/x"), "{p:?}");
    }

    #[test]
    fn other_schemes_keep_their_brackets() {
        let f = Fixture::new("extra-brackets", &[]);
        for url in [
            "ftp://example.com/a(b-c)",
            "ssh://h/x[1.2]",
            "gemini://a.org/p(q)r",
            "ftp://h/a(b)c(d)",
            "ftp://example.com/a(b)(c)",
            "ftp://example.com/a(b(c)d)",
            "ftp://example.com/?next=(https://example.org)",
            "ssh://[::1]:22/x",
            "ftp://a.com/cafe\u{301}.txt",
        ] {
            let text = format!("see {url} ok");
            for at in (4..4 + url.len()).filter(|&at| text.is_char_boundary(at)) {
                assert_eq!(f.pick(&text, at), Some((4..4 + url.len(), Picked::Link(Target::Url(url.into())))), "{url} at {at}");
            }
        }
        assert_eq!(f.pick("(ftp://a.com)", 1).map(|(r, _)| r), Some(1..12));
        // Decomposed Unicode (macOS file names) stays in the link, `file:` included.
        let text = "see file:/tmp/cafe\u{301}.txt ok";
        assert_eq!(f.pick(text, 4).map(|(r, _)| &text[r]), Some("file:/tmp/cafe\u{301}.txt"));
        assert_eq!(f.pick("see ftp://a.com/x(y ok", 4).map(|(r, _)| r), Some(4..17), "an unclosed bracket isn't the URL's");
        // A URL inside another's query belongs to the outer one; on its own it links itself.
        let text = "see https://a.com/?u=ftp://b.com ok";
        assert_eq!(f.pick(text, text.find("ftp").unwrap()).map(|(r, _)| &text[r]), Some("https://a.com/?u=ftp://b.com"));
    }

    #[test]
    fn relative_variables_resolve_from_the_terminals_folder() {
        let f = Fixture::new("vars", &["src/main.rs"]);
        // `DIR` is `src` in the fixture's environment.
        assert_eq!(f.pick("$DIR/main.rs", 0), Some((0..12, Picked::Link(Target::File(f.at("src/main.rs"))))));
        assert_eq!(f.pick("$DIR/gone.rs", 0), None);
        assert_eq!(f.pick("$NOPE/main.rs", 0), None);
    }

    /// SonicTerm reads Windows paths with its Windows grammar on every OS; Apollo
    /// adds UNC paths.
    #[test]
    fn finds_windows_paths() {
        for (text, want) in [
            (r"see C:\src\main.rs:42:7, ok", r"C:\src\main.rs:42:7"),
            ("open C:/a/b.txt.", "C:/a/b.txt"),
            (r"at \\server\share\x.txt, done", r"\\server\share\x.txt"),
        ] {
            let at = text.find(want).unwrap();
            assert!(spans(text, at, PathStyle::Windows).iter().any(|r| &text[r.clone()] == want), "{text:?}: {:?}", spans(text, at, PathStyle::Windows));
        }
        assert!(spans("modified:   C", 12, PathStyle::Windows).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn picks_windows_drive_and_unc_paths_that_exist() {
        let dir = std::env::temp_dir().join(format!("apollo-win-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join(r"src\main.rs"), "").unwrap();
        let ctx = Ctx::new(Some(dir.clone()));
        let path = dir.join(r"src\main.rs");
        let text = format!(r"see {}:42:7, ok", path.display());
        let at = text.find(&path.display().to_string()).unwrap();
        assert_eq!(pick(&text, at, false, &ctx, |_| None).unwrap().1, Picked::Link(Target::File(path.clone())));
        // The same file through the C$ administrative share, when the runner allows it.
        let unc = format!(r"\\localhost\{}$\{}", &path.display().to_string()[..1], &path.display().to_string()[3..]);
        if Path::new(&unc).exists() {
            let text = format!("at {unc}, done");
            assert_eq!(pick(&text, 5, false, &ctx, |_| None).map(|(r, _)| text[r].to_owned()), Some(unc));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_file_urls_drop_the_verbatim_prefix() {
        for (path, url) in [
            (r"\\?\C:\a\b.txt", "file:///C:/a/b.txt"),
            (r"\\?\UNC\srv\share\d", "file://srv/share/d"),
            (r"C:\a", "file:///C:/a"),
            ("/tmp/x", "file:///tmp/x"),
        ] {
            assert_eq!(file_url(Path::new(path)), url, "{path}");
        }
        assert_eq!(decode_file_url(&file_url(Path::new(r"\\?\C:\a\b.txt")), true), Some(PathBuf::from(r"C:\a\b.txt")));
        assert_eq!(decode_file_url(&file_url(Path::new(r"\\?\UNC\srv\share\d")), true), Some(PathBuf::from(r"\\srv\share\d")));
    }

    #[test]
    fn activation_is_part_of_the_links_api() {
        let url = Target::Url("https://a.com".into());
        assert_eq!(crate::links::activation(&url), crate::links::Activation::OpenUrl("https://a.com".into()));
        assert_eq!(crate::links::activation(&Target::Url("file:///%zz".into())), crate::links::Activation::Ignore);
    }

    #[cfg(windows)]
    #[test]
    fn resolves_windows_drive_paths() {
        let exe = r"C:\Windows\System32\notepad.exe";
        assert_eq!(resolve(&format!("{exe}:3"), None), Some(Target::File(exe.into())));
        assert_eq!(resolve(r"C:\apollo-missing\x.txt", None), None);
    }

    #[test]
    fn query_links_keep_their_full_target() {
        for url in ["https://example.com/?a=1&b=2", "https://localhost:17480/login?t=abc%3D%3D"] {
            let text = format!("Dashboard: {url}");
            let range = find(&text, text.find(url).unwrap()).unwrap();
            assert_eq!(&text[range.clone()], url);
            assert_eq!(resolve(&text[range], None), Some(Target::Url(url.into())));
        }
        let u = "file:///C:/Windows/System32/winver.exe";
        let text = format!("Dashboard: {u}");
        let range = find(&text, text.find(u).unwrap()).unwrap();
        assert_eq!(&text[range.clone()], u);
        assert_eq!(resolve(u, None), Some(Target::File(file_url_path(u).unwrap())));
    }

    #[test]
    fn file_urls_decode_to_paths() {
        assert_eq!(decode_file_url("file:///tmp/a%20b.txt", false), Some(PathBuf::from("/tmp/a b.txt")));
        assert_eq!(decode_file_url("file://localhost/tmp/x", false), Some(PathBuf::from("/tmp/x")));
        assert_eq!(decode_file_url("FILE:/tmp/x#top", false), Some(PathBuf::from("/tmp/x")));
        assert_eq!(decode_file_url("file:///C:/Users/a%20b/x.txt", true), Some(PathBuf::from(r"C:\Users\a b\x.txt")));
        assert_eq!(decode_file_url("file://server/share/x", true), Some(PathBuf::from(r"\\server\share\x")));
        for bad in ["file://server/share/x", "file:///%zz", "file:///%C3%28", "file:relative", "https://a.com", "file:///tmp/%+A"] {
            assert_eq!(decode_file_url(bad, false), None, "{bad}");
        }
    }

    #[test]
    fn file_links_always_resolve_to_files() {
        let gone = "file:///C:/apollo-missing/x.txt";
        assert_eq!(resolve(gone, None), Some(Target::File(file_url_path(gone).unwrap())));
        assert_eq!(resolve("file:///%zz", None), None);
        assert_eq!(resolve("file:///tmp/%+A", None), None);
        let ctx = Ctx::new(None);
        assert_eq!(pick("file:///apollo-gone/x", 0, true, &ctx, |_| None), None);
        assert!(pick("file:///apollo-gone/x", 0, false, &ctx, |_| None).is_some());
    }

    #[test]
    fn extracts_address_words_at_the_pointer() {
        for (text, expected) in [
            ("<t1hg>", "t1hg"),
            ("[\"macbook/t2w\"]", "macbook/t2w"),
            ("terminal=t1hg,", "t1hg"),
            ("'my-machine_2/t2w'.", "my-machine_2/t2w"),
            ("before t1hg after", "t1hg"),
            ("é t1hg", "t1hg"),
            ("<build.box/t1>.", "build.box/t1"),
            ("1.2.3.4/t1", "1.2.3.4/t1"),
            ("...t1hg.", "t1hg"),
        ] {
            let at = text.find(expected).unwrap();
            for (i, _) in expected.char_indices() {
                assert_eq!(word(text, at + i).map(|r| &text[r]), Some(expected), "{text}");
            }
        }
        for (text, at) in [("<t1>", 0), ("t1 ", 2), ("", 0), ("t1", 2), ("é", 1)] {
            assert_eq!(word(text, at), None);
        }
    }

    #[test]
    fn matches_only_live_addresses_relative_to_the_source_machine() {
        let live = ["t1", "t2", "macbook/t1", "macbook/t2w", "my-machine_2/t3", "build.box/t1", "box/t1"];
        for (word, source, expected) in [
            ("t1", "t2", Some("t1")),
            ("t1", "macbook/t2w", Some("macbook/t1")),
            ("t1", "build.box/t2", Some("build.box/t1")),
            ("t2", "macbook/t1", None),
            ("macbook/t2w", "t1", Some("macbook/t2w")),
            ("my-machine_2/t3", "macbook/t1", Some("my-machine_2/t3")),
            ("t99", "t1", None),
            ("Build", "t1", None),
            ("t", "t1", None),
            ("other/t1", "t1", None),
            ("macbook/t1/extra", "t1", None),
        ] {
            assert_eq!(terminal_address(word, source, live), expected.map(str::to_owned), "{word} in {source}");
        }
    }

    #[test]
    fn dotted_machines_never_match_a_different_suffix_machine() {
        let text = "<build.box/t1>.";
        let range = word(text, text.find("t1").unwrap()).unwrap();
        assert_eq!(&text[range.clone()], "build.box/t1");
        assert_eq!(terminal_address(&text[range.clone()], "t2", ["box/t1"]), None);
        assert_eq!(terminal_address(&text[range], "t2", ["box/t1", "build.box/t1"]), Some("build.box/t1".into()));
    }

    #[test]
    fn files_resolve_only_when_they_exist() {
        let dir = std::env::temp_dir().join(format!("herdr-links-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "").unwrap();
        let file = Target::File(dir.join("src/main.rs"));
        assert_eq!(resolve("src/main.rs", Some(&dir)), Some(file.clone()));
        assert_eq!(resolve("src/main.rs:42:7", Some(&dir)), Some(file.clone()));
        assert_eq!(resolve("src/main.rs.", Some(&dir)), Some(file.clone()));
        assert_eq!(resolve("src/.", Some(&dir)), Some(Target::File(dir.join("src/."))));
        assert_eq!(resolve("src/..", Some(&dir)), Some(Target::File(dir.join("src/.."))));
        assert_eq!(resolve("$PWD/src/main.rs", Some(&dir)), Some(file.clone()));
        assert_eq!(resolve(dir.join("src/main.rs").to_str().unwrap(), None), Some(file));
        assert_eq!(resolve("src/gone.rs", Some(&dir)), None);
        assert_eq!(resolve("src/main.rs", None), None);
        assert_eq!(resolve("https://a.com/x", None), Some(Target::Url("https://a.com/x".into())));
        #[cfg(unix)]
        {
            let dotted = dir.join("src/main.rs.");
            std::fs::write(&dotted, "").unwrap();
            assert_eq!(resolve("src/main.rs.", Some(&dir)), Some(Target::File(dotted)));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
