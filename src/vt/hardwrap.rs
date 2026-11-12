//! URLs a program broke across rows, joined: SonicTerm's two hard-wrap rules
//! (`path_target_hardwrap_uri.rs`, `path_target_hardwrap_markdown.rs` at
//! 68250eef) on Apollo's rows. Only the wrapper proves the join: an
//! `http(s)` URL opened by `(` or `[` joins across rows once its closer is
//! found. Without one, `https://example.com` and a next row's `and` would read
//! as `https://example.comand`.

use super::{Row, Vt};

/// Rows one joined link may span.
pub(super) const MAX_LINK_ROWS: usize = 32;
/// Bytes one joined link may hold.
pub(super) const MAX_LINK_BYTES: usize = 4096;
/// Deepest indent a continuation row may start at.
const MAX_INDENT: u16 = 8;

/// One cell of a hard row.
#[derive(Clone, Debug)]
pub(super) struct Cell {
    pub ch: char,
    pub x: u16,
    pub width: u16,
    /// One character, no combining marks.
    single: bool,
    /// Part of a marked (OSC 8) link.
    linked: bool,
}

impl Cell {
    /// Single-width, one character, no marked link: a structural cell a join may drop.
    fn plain(&self) -> bool {
        self.single && self.width == 1 && !self.linked
    }

    fn blank(&self) -> bool {
        self.ch == ' '
    }

    /// Text that can't be part of a URL a program broke: anything not plain
    /// ASCII URL text, and the wrappers themselves.
    fn breaks(&self) -> bool {
        !self.plain() || !self.ch.is_ascii() || self.ch.is_whitespace() || self.ch.is_control() || matches!(self.ch, '(' | ')' | '[' | ']')
    }
}

/// A joined link: each row's cells it covers, top first.
pub(super) type Parts = Vec<(u32, Vec<Cell>)>;

pub(super) enum Joined {
    /// A whole URL across rows.
    Whole(Parts),
    /// The pointer is on a wrapped URL whose end wasn't found: its first row
    /// alone would open the wrong address.
    Unproven,
    /// No wrapped URL here.
    None,
}

/// The URL a program broke across rows under (`y`, `x`), looking no higher than `top`.
pub(super) fn link(vt: &Vt, y: u32, x: u16, top: u32) -> Joined {
    match edge(vt, y, x, top) {
        Joined::None => padded(vt, y, x, top),
        found => found,
    }
}

struct Hard {
    cells: Vec<Cell>,
    /// The terminal wrapped the row above onto this one.
    continues: bool,
    /// The terminal wraps this row onto the next.
    wraps: bool,
}

fn row(vt: &Vt, y: u32) -> Option<Hard> {
    let Row { text, spots, wraps } = vt.link_row(y)?;
    let cells = spots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let end = spots.get(i + 1).map_or(text.len(), |n| n.byte);
            let mut chars = text[s.byte..end].chars();
            let ch = chars.next().unwrap_or(' ');
            Cell { ch, x: s.x, width: s.width, single: chars.next().is_none(), linked: s.uri.is_some() }
        })
        .collect();
    Some(Hard { cells, continues: y > 0 && vt.wraps(y - 1), wraps })
}

fn opens_http(cells: &[Cell], from: usize) -> bool {
    let spells = |p: &str| cells.get(from..from + p.len()).is_some_and(|c| c.iter().map(|c| c.ch).eq(p.chars()));
    spells("https://") || spells("http://")
}

/// The host and the slash after it are all there: a cut inside the authority
/// would let the next row choose the host.
fn authority_ok(url: &str) -> bool {
    let rest = &url[if url.starts_with("https://") { 8 } else { 7 }..];
    rest.find(['/', '?', '#']).is_some_and(|i| i > 0 && rest.as_bytes()[i] == b'/')
}

/// Whether what follows the closer at `close` ends the URL: up to the next
/// blank, only sentence punctuation or non-ASCII text, which no URL holds
/// (`)。` ends it, `)x` doesn't). A tail running into a terminal wrap may go on.
fn closer_ends(cells: &[Cell], close: usize, wraps: bool) -> bool {
    for c in &cells[close + 1..] {
        if c.ch.is_whitespace() {
            return true;
        }
        let prose = !c.ch.is_ascii() || matches!(c.ch, '.' | ',' | ';' | ':' | '!' | '?');
        if !c.single || c.linked || !prose {
            return false;
        }
    }
    !wraps
}

fn holds(parts: &Parts, y: u32, x: u16) -> bool {
    parts.iter().any(|(py, cells)| *py == y && cells.iter().any(|c| c.x <= x && x < c.x + c.width))
}

/// A wrapper opened at the pane edge: `see (https://exa` / `  mple.com/x)`.
/// Continuations share one indent of at most eight cells; the first row holds
/// the whole authority; the joined text is one URL with one `://`.
fn edge(vt: &Vt, y: u32, x: u16, top: u32) -> Joined {
    let first = y.saturating_sub(MAX_LINK_ROWS as u32 - 1).max(top);
    for start_y in (first..=y).rev() {
        let Some(r) = row(vt, start_y) else { continue };
        if r.continues || r.wraps {
            continue;
        }
        // Only a wrapper at the row's last break can open a body that runs to the edge.
        let Some(open) = r.cells.iter().rposition(Cell::breaks) else { continue };
        let closer = match r.cells[open].ch {
            '(' => ')',
            '[' => ']',
            _ => continue,
        };
        if !opens_http(&r.cells, open + 1) {
            continue;
        }
        match edge_chain(vt, (start_y, open + 1), closer, (y, x)) {
            Joined::None => continue,
            found => return found,
        }
    }
    Joined::None
}

fn edge_chain(vt: &Vt, (first_y, first_start): (u32, usize), closer: char, (y, x): (u32, u16)) -> Joined {
    let mut parts: Parts = Vec::new();
    let mut joined = String::new();
    let mut indent = None;
    let refuse = |parts: &Parts| if holds(parts, y, x) { Joined::Unproven } else { Joined::None };
    for offset in 0..MAX_LINK_ROWS as u32 {
        let row_y = first_y + offset;
        let Some(r) = row(vt, row_y) else { return refuse(&parts) };
        if r.continues {
            return refuse(&parts);
        }
        let start = if offset == 0 { first_start } else { r.cells.iter().position(|c| !c.blank()).unwrap_or(r.cells.len()) };
        let close = r.cells.iter().skip(start).position(|c| c.ch == closer).map(|i| start + i);
        if offset == 0 && close.is_some() {
            // Closed on its own row: ordinary text.
            return Joined::None;
        }
        let end = close.unwrap_or(r.cells.len());
        let body = r.cells.get(start..end).unwrap_or_default();
        if body.is_empty() || body.iter().any(Cell::breaks) {
            return refuse(&parts);
        }
        if offset > 0 {
            let text: String = body.iter().take(8).map(|c| c.ch).collect();
            if text.starts_with("https://") || text.starts_with("http://") {
                return refuse(&parts);
            }
            let at = r.cells[start].x;
            if at > MAX_INDENT || *indent.get_or_insert(at) != at {
                return refuse(&parts);
            }
        }
        parts.push((row_y, body.to_vec()));
        if joined.len() + body.len() > MAX_LINK_BYTES {
            return refuse(&parts);
        }
        joined.extend(body.iter().map(|c| c.ch));
        if offset == 0 && !authority_ok(&joined) {
            return refuse(&parts);
        }
        if joined.matches("://").count() != 1 {
            return refuse(&parts);
        }
        if let Some(close) = close {
            if !r.cells[close].plain() || !closer_ends(&r.cells, close, r.wraps) || !crate::links::whole_url(&joined) {
                return refuse(&parts);
            }
            return if holds(&parts, y, x) { Joined::Whole(parts) } else { Joined::None };
        }
    }
    refuse(&parts)
}

/// A numbered row's gutter: blanks, one to six digits, a blank, an optional diff sign.
struct Gutter {
    /// The cell the row's content starts at.
    content: usize,
    sign: Option<char>,
}

fn gutter(cells: &[Cell]) -> Option<Gutter> {
    let digits = cells.iter().position(|c| !c.blank())?;
    let count = cells[digits..].iter().take_while(|c| c.ch.is_ascii_digit()).count();
    if !(1..=6).contains(&count) || cells.get(digits + count).is_none_or(|c| c.ch != ' ') {
        return None;
    }
    let after = digits + count + 1;
    let sign = cells.get(after).map(|c| c.ch).filter(|c| matches!(c, '+' | '-'));
    if sign.is_some() && !cells[after].plain() {
        return None;
    }
    Some(Gutter { content: after + usize::from(sign.is_some()), sign })
}

/// Claude Code's numbered diff and markdown view: `    60 [label](https://…Usag`
/// cut at its own width with blank padding after it, then `       e-zh-CN)。`
/// hanging at the content column (a `+`/`-` sign repeated). Only a markdown
/// link target joins; a middle row must end where the first did.
fn padded(vt: &Vt, y: u32, x: u16, top: u32) -> Joined {
    let first = y.saturating_sub(MAX_LINK_ROWS as u32 - 1).max(top);
    for start_y in (first..=y).rev() {
        if let found @ Joined::Whole(_) = padded_chain(vt, start_y, (y, x)) {
            return found;
        }
    }
    Joined::None
}

fn padded_chain(vt: &Vt, first_y: u32, (y, x): (u32, u16)) -> Joined {
    let Some(r) = row(vt, first_y) else { return Joined::None };
    if r.continues {
        return Joined::None;
    }
    let Some(g) = gutter(&r.cells) else { return Joined::None };
    let Some(last) = (g.content..r.cells.len()).rev().find(|&i| !r.cells[i].blank()) else { return Joined::None };
    let Some(open) = (g.content..=last).rev().find(|&i| r.cells[i].breaks()) else { return Joined::None };
    let target = open > g.content
        && r.cells[open].ch == '('
        && r.cells[open - 1].ch == ']'
        && r.cells[open].plain()
        && r.cells[open - 1].plain()
        && opens_http(&r.cells, open + 1);
    // A first row ending in sentence punctuation is where a word wrapper breaks.
    if !target || matches!(r.cells[last].ch, '.' | ',' | ';' | ':' | '!' | '?') {
        return Joined::None;
    }
    let body = &r.cells[open + 1..=last];
    let mut joined: String = body.iter().map(|c| c.ch).collect();
    if !authority_ok(&joined) {
        return Joined::None;
    }
    let cut = r.cells[last].x;
    let mut parts: Parts = vec![(first_y, body.to_vec())];
    for offset in 1..MAX_LINK_ROWS as u32 {
        let row_y = first_y + offset;
        let Some(r) = row(vt, row_y) else { return Joined::None };
        if r.continues {
            return Joined::None;
        }
        let c = &r.cells;
        let blank_to = |end: usize| c.iter().take(end).all(Cell::blank);
        // The continuation repeats the first row's sign, or leaves its column blank.
        let gutter_matches = match g.sign {
            Some(sign) => blank_to(g.content - 1) && c.get(g.content - 1).is_some_and(|s| s.ch == sign && s.plain()),
            None => blank_to(g.content),
        };
        if !gutter_matches || c.get(g.content).is_none_or(Cell::blank) {
            return Joined::None;
        }
        let close = (g.content..c.len()).find(|&i| c[i].ch == ')');
        let end = match close {
            Some(close) => close,
            // A middle row, cut at the box width like the first.
            None => match (g.content..c.len()).rev().find(|&i| !c[i].blank()) {
                Some(i) if c[i].x == cut => i + 1,
                _ => return Joined::None,
            },
        };
        let body = &c[g.content..end];
        if body.is_empty() || body.iter().any(Cell::breaks) || joined.len() + body.len() > MAX_LINK_BYTES {
            return Joined::None;
        }
        joined.extend(body.iter().map(|c| c.ch));
        parts.push((row_y, body.to_vec()));
        let Some(close) = close else { continue };
        let whole = c[close].plain() && closer_ends(c, close, r.wraps) && crate::links::whole_url(&joined);
        return if whole && holds(&parts, y, x) { Joined::Whole(parts) } else { Joined::None };
    }
    Joined::None
}

#[cfg(test)]
#[path = "hardwrap_tests.rs"]
mod hardwrap_tests;
