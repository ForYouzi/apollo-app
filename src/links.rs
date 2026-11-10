//! Links in terminal text: Ghostty's URL and file path pattern, what a match
//! points to, and what clicking it does.

use fancy_regex::Regex;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

mod activation;
mod system;

pub use activation::{Opener, activate, safe_folder};
/// What a click on a link means, without carrying it out.
#[allow(unused_imports)]
pub use activation::{Activation, activation};
#[cfg(test)]
pub(crate) use activation::{Call, Recorder};
pub use system::SystemOpener;

/// Ghostty's default link pattern (`src/config/url.zig`), plus Windows drive and UNC paths.
static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    let schemes = r"https?://|mailto:|ftp://|file:|ssh:|git://|ssh://|tel:|magnet:|ipfs://|ipns://|gemini://|gopher://|news:";
    let ipv6 = r"(?:\[[:0-9a-fA-F]+(?:[:0-9a-fA-F]*)+\](?::[0-9]+)?)";
    let scheme_chars = r"[\w\-.~:/?#@!$&*+,;=%]";
    let path_chars = r"[\w\-.~:\/?#@!$&*+;=%]";
    let bracketed = r"(?:[\(\[]\w*[\)\]])?";
    let no_trailing_punctuation = r"(?<![,.])";
    let no_trailing_colon = r"(?<!:)";
    let dotted = r"(?=[\w\-.~:\/?#@!$&*+;=%]*\.)";
    let undotted = r"(?![\w\-.~:\/?#@!$&*+;=%]*\.)";
    let dotted_spaces = r"(?:(?<!:) (?!\w+:\/\/)(?!\.{0,2}\/)(?!~\/)[\w\-.~:\/?#@!$&*+;=%]*[\/.])*";
    let any_spaces = r"(?:(?<!:) (?!\w+:\/\/)(?!\.{0,2}\/)(?!~\/)[\w\-.~:\/?#@!$&*+;=%]+)*";
    let rooted = r"(?:\.\.\/|\.\/|(?<!\w)~\/|(?:[\w][\w\-.]*\/)*(?<!\w)\$[A-Za-z_]\w*\/|\.[\w][\w\-.]*\/|(?<![\w~\/])\/(?!\/))";
    // Schemes are case-insensitive (RFC 3986), so `FILE:` and `HTTPS:` match whole.
    let scheme_url = format!("(?i:{schemes})(?:{ipv6}|{scheme_chars}+{bracketed})+{no_trailing_punctuation}");
    let rooted_path = format!(
        "{rooted}(?:{dotted}{path_chars}+{dotted_spaces}{no_trailing_colon}|{undotted}{path_chars}+{any_spaces}{no_trailing_colon})"
    );
    let bare_path = format!(r"{dotted}(?<!\$\d*)(?<!\w)[\w][\w\-.]*\/{path_chars}+{no_trailing_colon}");
    // Windows drive (`C:\x`, `C:/x`) and UNC (`\\server\share\x`) paths, which Ghostty lacks.
    let drive = r"(?<![\w/\\])[A-Za-z]:[\\/][\w\-.~:\\/@!$&+;=%]+(?<![,.:])";
    let unc = r"(?<![\w\\])\\\\[\w.\-]+\\[\w\-.~:\\@!$&+;=%]+(?<![,.:])";
    Regex::new(&format!("{scheme_url}|{rooted_path}|{bare_path}|{drive}|{unc}")).expect("link pattern")
});

/// The link in `text` that covers byte `at`.
pub fn find(text: &str, at: usize) -> Option<Range<usize>> {
    for m in PATTERN.find_iter(text) {
        let m = m.ok()?;
        if m.start() > at {
            return None;
        }
        if at < m.end() {
            return Some(m.range());
        }
    }
    None
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
/// on Windows `/C:/x` → `C:\x` and other hosts → UNC. `None` if not one.
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

/// What a link opens. File paths count only when they exist here; relative
/// ones need the terminal's folder. A `file:` URL always names a file.
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

/// A text match worth showing as a link. In lines joined back together only a
/// file that exists proves the join right.
pub fn candidate(link: &str, cwd: Option<&Path>, joined: bool) -> Option<Target> {
    resolve(link, cwd).filter(|t| !joined || matches!(t, Target::File(p) if p.exists()))
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

    fn first(text: &str) -> Option<&str> {
        PATTERN.find(text).unwrap().map(|m| m.as_str())
    }

    /// Ghostty's own cases (`src/config/url.zig`).
    #[test]
    fn matches_like_ghostty() {
        let cases = [
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
            ("also match http://example.com non-secure links", "http://example.com"),
            ("match tel://+12123456789 phone numbers", "tel://+12123456789"),
            ("match with query url https://example.com?query=1&other=2 and more text.", "https://example.com?query=1&other=2"),
            (
                "url with dashes [mode 2027](https://github.com/contour-terminal/terminal-unicode-core) for better unicode support",
                "https://github.com/contour-terminal/terminal-unicode-core",
            ),
            ("dot.http://example.com", "http://example.com"),
            ("weird characters https://example.com/~user/?query=1&other=2#hash and more", "https://example.com/~user/?query=1&other=2#hash"),
            ("square brackets https://example.com/[foo] and more", "https://example.com/[foo]"),
            (
                "[13]:TooManyStatements: TempFile#assign_temp_file_to_entity has approx 7 statements [https://example.com/docs/Too-Many-Statements.md]",
                "https://example.com/docs/Too-Many-Statements.md",
            ),
            ("match ftp://example.com ftp links", "ftp://example.com"),
            ("match file://example.com file links", "file://example.com"),
            ("match ssh://example.com ssh links", "ssh://example.com"),
            ("match git://example.com git links", "git://example.com"),
            ("/tmp/test.txt http://www.google.com", "/tmp/test.txt"),
            ("match tel:+18005551234 tel links", "tel:+18005551234"),
            ("match magnet:?xt=urn:btih:1234567890 magnet links", "magnet:?xt=urn:btih:1234567890"),
            ("match ipfs://QmSomeHashValue ipfs links", "ipfs://QmSomeHashValue"),
            ("match ipns://QmSomeHashValue ipns links", "ipns://QmSomeHashValue"),
            ("match gemini://example.com gemini links", "gemini://example.com"),
            ("match gopher://example.com gopher links", "gopher://example.com"),
            ("match news:comp.infosystems.www.servers.unix news links", "news:comp.infosystems.www.servers.unix"),
            ("/Users/ghostty.user/code/example.py", "/Users/ghostty.user/code/example.py"),
            ("/Users/ghostty.user/code/../example.py", "/Users/ghostty.user/code/../example.py"),
            ("/Users/ghostty.user/code/../example.py hello world", "/Users/ghostty.user/code/../example.py"),
            ("../example.py", "../example.py"),
            ("../example.py ", "../example.py"),
            ("first time ../example.py contributor ", "../example.py"),
            ("[link](/home/user/ghostty.user/example)", "/home/user/ghostty.user/example"),
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
            ("./spaces-end.   ", "./spaces-end."),
            ("./space middle", "./space middle"),
            ("../test folder/file.txt", "../test folder/file.txt"),
            ("/tmp/test folder/file.txt", "/tmp/test folder/file.txt"),
            ("/tmp/test  folder/file.txt", "/tmp/test"),
            ("diff --git a/src/font/shaper/harfbuzz.zig b/src/font/shaper/harfbuzz.zig", "a/src/font/shaper/harfbuzz.zig"),
            ("/tmp/foo /tmp/bar", "/tmp/foo"),
            ("/tmp/foo.txt /tmp/bar.txt", "/tmp/foo.txt"),
            ("src/config/url.zig", "src/config/url.zig"),
            ("app/folder/file.rb:1", "app/folder/file.rb:1"),
            ("modified:   src/config/url.zig", "src/config/url.zig"),
            ("lib/ghostty/terminal.zig:42:10", "lib/ghostty/terminal.zig:42:10"),
            ("some-pkg/src/file.txt more text", "some-pkg/src/file.txt"),
            ("src/foo.c,baz.txt", "src/foo.c"),
            ("~/foo/bar.txt", "~/foo/bar.txt"),
            ("open ~/Documents/notes.md please", "~/Documents/notes.md"),
            ("~/.config/ghostty/config", "~/.config/ghostty/config"),
            ("directory: ~/src/ghostty-org/ghostty", "~/src/ghostty-org/ghostty"),
            ("$HOME/src/config/url.zig", "$HOME/src/config/url.zig"),
            ("space dir: $PWD/src/ghostty/main.zig", "$PWD/src/ghostty/main.zig"),
            ("foo/$BAR/baz", "foo/$BAR/baz"),
            (".foo/bar/$VAR", ".foo/bar/$VAR"),
            (".config/ghostty/config", ".config/ghostty/config"),
            ("loaded from .local/share/ghostty/state.db now", ".local/share/ghostty/state.db"),
            ("../some/where", "../some/where"),
            ("  - shared/src/foo/SomeItem.m:12, shared/src/", "shared/src/foo/SomeItem.m:12"),
            ("foo.local/share", "foo.local/share"),
            ("2024/report.txt", "2024/report.txt"),
            ("./foo bar,baz", "./foo bar"),
            ("/tmp/foo bar,baz", "/tmp/foo bar"),
            ("./.config/ghostty: Needs upstream (main)", "./.config/ghostty"),
            ("./Downloads: Operation not permitted", "./Downloads"),
        ];
        for (input, expect) in cases {
            assert_eq!(first(input), Some(expect), "{input:?}");
        }
        for input in ["input/output", "foo/bar", "$10/bar", "$10/$20", "$10/bar.txt", "foo/bar,baz.txt", "foo$BAR/baz.txt", "foo~/bar.txt", "// foo bar", "//foo"] {
            assert_eq!(first(input), None, "{input:?}");
        }
    }

    #[test]
    fn matches_windows_paths() {
        for (input, expect) in [
            (r"see C:\src\main.rs:42:7, ok", r"C:\src\main.rs:42:7"),
            ("open C:/a/b.txt.", "C:/a/b.txt"),
            (r"at \\server\share\x.txt, done", r"\\server\share\x.txt"),
        ] {
            assert_eq!(first(input), Some(expect), "{input:?}");
        }
        assert_eq!(first("modified:   C"), None);
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

    #[test]
    fn schemes_match_in_any_case() {
        assert_eq!(first("see FILE:///tmp/report.txt ok"), Some("FILE:///tmp/report.txt"));
        assert_eq!(first("see HTTPS://Example.com ok"), Some("HTTPS://Example.com"));
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
        assert_eq!(candidate("file:///apollo-gone/x", None, true), None);
        assert!(candidate("file:///apollo-gone/x", None, false).is_some());
    }

    #[test]
    fn finds_the_link_under_the_pointer() {
        let text = "see https://a.com and /tmp/x.txt.";
        assert_eq!(find(text, 0), None);
        assert_eq!(find(text, 4).map(|r| &text[r]), Some("https://a.com"));
        assert_eq!(find(text, 16).map(|r| &text[r]), Some("https://a.com"));
        assert_eq!(find(text, 17), None);
        assert_eq!(find(text, 25).map(|r| &text[r]), Some("/tmp/x.txt."));
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
