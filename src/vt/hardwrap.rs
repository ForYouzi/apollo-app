//! URLs a program broke across rows, joined: SonicTerm's two hard-wrap rules
//! on Apollo's rows. `edge` is `path_target_hardwrap_uri.rs` at 68250eef: an
//! `http(s)` URL opened by `(` or `[` at the pane edge joins across rows once
//! its closer is found. `numbered` is `numbered_box_uri_in`
//! (`path_target_hardwrap_markdown.rs`, `_delimited.rs`, `_layout.rs` at
//! origin/main 3284d2e9, from 1008de56): any URL in one numbered line of
//! Claude Code's diff or markdown box. A joined URL opens whole, exactly as
//! the rows spell it, or not at all; never a prefix.

#[path = "hardwrap_delimited.rs"]
mod delimited;
#[path = "hardwrap_layout.rs"]
mod layout;

use super::{Row, Vt};
use crate::links::url_scan;
use delimited::{Delimited, delimited_url, row_local_uri};
use ghostty_sys::GHOSTTY_POINT_TAG_SCREEN;
use layout::{agreed, diff_contradicted, diff_only_sign, markdown_target};

/// Rows one joined link may span.
pub(super) const MAX_LINK_ROWS: usize = 32;
/// Bytes one joined link may hold.
pub(super) const MAX_LINK_BYTES: usize = 4096;
/// Deepest indent a continuation row may start at.
const MAX_INDENT: u16 = 8;

/// One cell of a hard row.
#[derive(Clone, Debug, PartialEq)]
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

#[derive(Debug, PartialEq)]
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
        Joined::None => numbered(vt, y, x, top),
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
        // The wrapper the join drops must be plain text, as the closer must.
        if !r.cells[open].plain() || !opens_http(&r.cells, open + 1) {
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

// ---- Claude Code's numbered box ---------------------------------------------
//
// SonicTerm's `numbered_box_uri_in` (`path_target_hardwrap_markdown.rs`, origin/main 3284d2e9),
// on Apollo's rows. Its `HardwrapUri::Complete` is `Joined::Whole`, `Incomplete` is
// `Joined::Unproven` and `NotApplicable` is `Joined::None`. Where SonicTerm reads data Apollo's
// rows lack, the rule adapts:
// - a wide character's second column (`WIDE_CONT`): Apollo's rows skip it, so each row is laid
//   out by column again and the column after a two-cell character becomes a zero-width cell;
// - combining extras are a cell that is not `single`, and an OSC 8 hyperlink is `linked`;
// - the frame is the viewport, `[top, top + rows)` screen rows, and a row below it exists when
//   the screen holds one;
// - the pane is the whole grid: Apollo panes are separate terminals, so the pane-border checks
//   (`pane_columns_at`, `spans_join_across_pane_border`) have nothing to split.

/// The screen cell under the pointer.
#[derive(Clone, Copy)]
struct At {
    row: u32,
    col: u16,
}

/// A wide character's second column, which the box layout puts back as a zero-width cell.
fn cont(cell: &Cell) -> bool {
    cell.width == 0
}

/// Whether `cell` is padding: a plain blank. A blank that carries combining marks or an OSC 8
/// link is text the rule cannot read, so it is never trimmed away as padding.
fn box_blank(cell: &Cell) -> bool {
    cell.blank() && cell.plain()
}

/// One displayed row of the box, laid out by column.
struct BoxRow {
    cells: Vec<Cell>,
    /// The row's first and last non-blank columns; `None` when it holds no text.
    text: Option<(usize, usize)>,
    /// The column of the blank after one to six digits that start the row's text: its gutter end.
    gutter_end: Option<usize>,
    /// The line number those digits spell, when the row has a gutter.
    number: Option<u64>,
    soft_wrapped: bool,
}

impl BoxRow {
    fn read(vt: &Vt, y: u32) -> Option<BoxRow> {
        #[cfg(test)]
        BOX_READS.with(|reads| reads.borrow_mut().push(y));
        let hard = row(vt, y)?;
        let right = usize::from(vt.cols);
        let mut cells: Vec<Cell> = (0..vt.cols).map(|x| Cell { ch: ' ', x, width: 1, single: true, linked: false }).collect();
        for cell in hard.cells {
            let x = usize::from(cell.x);
            if cell.width == 2 && x + 1 < right {
                cells[x + 1] = Cell { ch: ' ', x: cell.x + 1, width: 0, single: true, linked: cell.linked };
            }
            if x < right {
                cells[x] = cell;
            }
        }
        let non_blank = |column: &usize| !box_blank(&cells[*column]);
        let text = (0..right).find(non_blank).map(|first| (first, (first..right).rev().find(non_blank).unwrap_or(first)));
        let gutter_end = text.and_then(|(first, _)| {
            let digit_count = (first..right).take_while(|column| cells[*column].ch.is_ascii_digit()).count();
            let end = first + digit_count;
            let blank_after = end < right && cells[end].ch == ' ';
            ((1..=6).contains(&digit_count) && blank_after).then_some(end)
        });
        let number = text.zip(gutter_end).and_then(|((first, _), end)| cells[first..end].iter().map(|cell| cell.ch).collect::<String>().parse().ok());
        Some(BoxRow { cells, text, gutter_end, number, soft_wrapped: hard.continues })
    }

    /// Whether the row holds no text at or before column `end`.
    fn blank_through(&self, end: usize) -> bool {
        self.text.is_none_or(|(first, _)| first > end)
    }

    /// The first non-blank column at or after `column`; `None` when the row has no text there.
    fn first_from(&self, column: usize) -> Option<usize> {
        let (first, last) = self.text?;
        (first.max(column)..=last).find(|column| !box_blank(&self.cells[*column]))
    }

    /// The columns of this row's text a line rebuild reads, from `start` to its last text.
    fn text_from(&self, start: Option<usize>) -> usize {
        start.zip(self.text).map_or(0, |(start, (_, last))| (last + 1).saturating_sub(start))
    }
}

/// The displayed rows one decision may read, each read at most once.
struct BoxRows<'vt> {
    vt: &'vt Vt,
    /// The frame's first screen row and the row just past its last; no decision reads outside.
    top: u32,
    end: u32,
    rows: Vec<Option<Option<BoxRow>>>,
}

impl BoxRows<'_> {
    fn contains(&self, y: u32) -> bool {
        (self.top..self.end).contains(&y)
    }

    /// Whether the screen holds a row below the frame: the view is scrolled back. No row is read.
    fn rows_below(&self) -> bool {
        self.vt.grid_ref(GHOSTTY_POINT_TAG_SCREEN, 0, self.end).is_some()
    }

    /// Screen row `y`; `None` outside the frame.
    fn get(&mut self, y: u32) -> Option<&BoxRow> {
        let index = usize::try_from(y.checked_sub(self.top)?).ok()?;
        if index >= self.rows.len() {
            return None;
        }
        if self.rows[index].is_none() {
            self.rows[index] = Some(BoxRow::read(self.vt, y));
        }
        self.rows[index].as_ref().and_then(Option::as_ref)
    }
}

/// How a numbered line places its text after the gutter end `E`, in Claude Code's own columns:
/// a markdown row's text starts at `E + 1`; a diff row keeps `E + 1` for its sign (`+`, `-` or
/// blank), repeats it on every continuation, and starts its text at `E + 2`. The hang column, where
/// a continuation's text starts when the box cut the source line, is that text column. A box is a
/// diff unless one of its rows holds other text at `E + 1`: a diff never does, while markdown can
/// open a line with a space or a list marker, so no single row tells the two apart.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoxLayout {
    Markdown,
    Diff(Option<char>),
}

impl BoxLayout {
    /// The column a line's text starts at, after a gutter that ends at `gutter_end`.
    fn hang(self, gutter_end: usize) -> usize {
        match self {
            Self::Markdown => gutter_end + 1,
            Self::Diff(_) => gutter_end + 2,
        }
    }

    /// Whether `row` continues a line of this layout: it soft-wraps nowhere, writes nothing into
    /// the gutter, holds text, and a diff row repeats the line's sign column exactly.
    fn continues(self, row: &BoxRow, gutter_end: usize) -> bool {
        if row.soft_wrapped || !row.blank_through(gutter_end) || row.text.is_none() {
            return false;
        }
        let Self::Diff(sign) = self else { return true };
        let cell = row.cells.get(gutter_end + 1);
        match sign {
            Some(sign) => cell.is_some_and(|cell| cell.plain() && cell.ch == sign),
            None => cell.is_none_or(box_blank),
        }
    }

    /// The first column of `row`'s own text: from the hang column on, so a diff sign is gutter.
    fn text_start(self, row: &BoxRow, gutter_end: usize) -> Option<usize> {
        row.first_from(self.hang(gutter_end))
    }
}

/// The first column a box row's text can start at: Claude Code indents its line numbers past the
/// two columns its `⏺` and `⎿` markers use, so a row whose text starts left of it is outside any
/// box.
const BOX_TEXT_MIN_COL: usize = 4;

/// Whether the row below a line's last row proves the line ended there: a blank row, a numbered
/// row with the same gutter, or a row whose text starts left of `BOX_TEXT_MIN_COL` does. `None`, a
/// soft wrap, a numbered row with another gutter, or a row shaped like a continuation that does
/// not continue this line may hold more of it, so none proves an end.
fn row_ends_line(row: Option<&BoxRow>, gutter_end: usize) -> bool {
    let Some(row) = row else { return false };
    if row.soft_wrapped {
        return false;
    }
    let Some((first, _)) = row.text else { return true };
    match row.gutter_end {
        Some(end) => end == gutter_end,
        None => first < BOX_TEXT_MIN_COL,
    }
}

/// Whether `cell` continues a wide character only when the cell before it, `lead`, is that
/// character's wide lead; any other wide continuation hides what the cell holds.
fn orphan_wide_cont(cell: &Cell, lead: Option<&Cell>) -> bool {
    cont(cell) && !lead.is_some_and(|lead| lead.width == 2)
}

/// One cell's byte in a rebuilt line: plain ASCII text, blank padding, or text no URL may join.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LineByte {
    /// A plain ASCII cell at `(row, column)`.
    Text(u32, u16),
    /// A blank cell, or the blank between rows the source line separated.
    Blank,
    /// Plain non-ASCII text: no URL character, so it ends a URL cleanly.
    Prose,
    /// A cell with combining marks, OSC 8 ownership or an orphan wide continuation: it may hide
    /// URL text, so a URL token that holds it never opens.
    Unsafe,
}

/// Whether `character` can be part of one URL token, as `find_urls` reads URL bodies.
fn url_body_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || "-._~!$&*+,;=:/?#[]@%()".contains(character)
}

/// Whether `byte` of a rebuilt line, holding `character`, can be part of one URL token: URL body
/// text, or an unsafe cell that may hide some.
fn token_byte(byte: LineByte, character: char) -> bool {
    match byte {
        LineByte::Text(..) => url_body_char(character),
        LineByte::Unsafe => true,
        LineByte::Blank | LineByte::Prose => false,
    }
}

/// The byte of a cell that is not plain: an ASCII cell, or one with marks or an OSC 8 link, may
/// hide URL text and is unsafe; a plain wide non-ASCII character ends a URL cleanly.
fn cell_line_byte(cell: &Cell) -> LineByte {
    let hides_text = cell.ch.is_ascii() || !cell.single || cell.linked;
    if hides_text { LineByte::Unsafe } else { LineByte::Prose }
}

/// Where the numbered row that heads the pointed row's line is.
enum LineHead {
    /// This displayed row heads the line.
    Row(u32),
    /// No displayed row heads it: the walk reached the frame's top inside the box, so the head
    /// may lie above the frame.
    AboveFrame,
    /// The pointed row is not inside a box line.
    Outside,
}

/// One numbered line's geometry inside the box, shared by its rebuild and its decision.
struct BoxLine {
    gutter_end: usize,
    layout: BoxLayout,
    /// Whether the joined URL must sit inside a markdown `](`…`)` pair: the box is not proven, so
    /// only that delimiter shows the rows hold one source line.
    markdown_pair: bool,
    /// The box's cut: the widest text column of its rows, which every wrapped row reaches.
    cut_end: usize,
    /// The line's head row, when it lies within the rebuilt rows.
    head_row: Option<u32>,
    /// The first and last rows rebuilt: the line's rows around the pointed row, within
    /// `MAX_LINK_ROWS` rows and `MAX_LINK_BYTES` bytes of it, since no URL this rule joins spans
    /// more.
    first_row: u32,
    last_row: u32,
    /// Whether the line may continue above `first_row` or below `last_row` into rows the rebuild
    /// did not read, the frame does not display, or this rule cannot read.
    open_above: bool,
    open_below: bool,
}

/// Claude Code's numbered diff and markdown box wraps each long source line at its own width,
/// short of the pane edge, and hangs the rest under the line number. This rebuilds the numbered
/// line under (`y`, `x`) from its rows (gutter and repeated `+`/`-` signs dropped; a row that
/// reaches the box's cut joins the next directly, one that stops short or a continuation right of
/// the hang column adds a blank) and finds the URL under the pointer in it, as `find_urls` reads
/// it. Once that URL spans two rows or may run into rows not read, it opens whole or is
/// `Unproven`, never a prefix; a one-row URL is left to the single-row rules (`None`).
pub(super) fn numbered(vt: &Vt, y: u32, x: u16, top: u32) -> Joined {
    if x >= vt.cols {
        return Joined::None;
    }
    // Reads stay within the displayed frame, each row at most once: the head search and the box's
    // layout evidence may reach its top and bottom; the rebuilt rows stay within `MAX_LINK_ROWS`
    // of the pointer.
    let end = top.saturating_add(u32::from(vt.rows));
    let mut rows = BoxRows { vt, top, end, rows: (top..end).map(|_| None).collect() };
    let pointed = At { row: y, col: x };
    let (head_row, gutter_end, line_sign) = match box_line_head(&mut rows, pointed.row) {
        LineHead::Row(head_row) => {
            let Some(head) = rows.get(head_row) else { return Joined::None };
            let Some(gutter_end) = head.gutter_end.filter(|_| !head.soft_wrapped) else { return Joined::None };
            let sign_cell = head.cells.get(gutter_end + 1);
            let unreadable = sign_cell.is_some_and(|cell| matches!(cell.ch, '+' | '-') && !cell.plain());
            let sign = sign_cell.map(|cell| cell.ch).filter(|sign| matches!(sign, '+' | '-'));
            // An unreadable sign leaves the line's sign unknown, an `Err`.
            (Some(head_row), gutter_end, (!unreadable).then_some(sign).ok_or(()))
        }
        LineHead::AboveFrame => {
            // No numbered row below shows the box: the line cannot be read; the single-row rules decide.
            let Some((gutter_end, sign)) = headless_line(&mut rows, pointed.row) else { return Joined::None };
            (None, gutter_end, sign)
        }
        LineHead::Outside => return Joined::None,
    };
    let evidence = box_evidence(&mut rows, pointed.row, gutter_end);
    // A replaced line, or signed rows beside context rows, proves a diff.
    let diff_proven = evidence.replaced_line || (evidence.signed_row && evidence.context_row);
    // Consecutive numbers prove a box when it is a diff, or when every row reaching its cut wrapped
    // there: numbered tool output whose rows happen to share a width shows a row that reaches it
    // and ends, and never signs its rows beside unsigned ones.
    let proven = evidence.replaced_line || (evidence.consecutive && (evidence.cut_wrapped || diff_proven));
    if let Some(decided) = row_local_uri(&mut rows, pointed) {
        // The pointed row alone holds the URL whole: no refusal for wrapped URLs applies. Outside a
        // proven box it stays the single-row rules', as before this rule (stricter than SonicTerm).
        return if proven { decided } else { Joined::None };
    }
    if !proven && narrower_gutter_nearby(&mut rows, pointed.row, gutter_end) {
        // A narrower gutter nearby shows the head was misread.
        return Joined::Unproven;
    }
    // The layouts the box leaves possible. Text after the gutter proves markdown; a diff may be
    // `diff_proven`. A box whose rows are all signed, or all blank there, may be either, so a URL
    // opens only when both readings find the same one. An unproven box is read the same way, and
    // only a markdown `](`…`)` pair joins in it, as a padded markdown target.
    let mut readings = Vec::with_capacity(2);
    let mut unreadable = false;
    if evidence.markdown_text {
        readings.push(BoxLayout::Markdown);
    } else {
        let Ok(sign) = line_sign else {
            // Which rows continue a diff line cannot be read; in an unproven box the single-row
            // rules decide.
            return if proven { Joined::Unproven } else { Joined::None };
        };
        // A continuation of this line that no diff row could be outweighs evidence elsewhere in
        // the box: the line is markdown when its head is unsigned, and unreadable when signed.
        let contradicted = diff_contradicted(&mut rows, (head_row, gutter_end, sign), pointed.row);
        match (contradicted, sign) {
            (true, Some(_)) => {
                // Neither layout reads the line, so a URL it breaks across rows stays inert, while
                // one on a single row stays its own.
                unreadable = true;
                readings.push(BoxLayout::Markdown);
            }
            (true, None) => readings.push(BoxLayout::Markdown),
            (false, _) => {
                readings.push(BoxLayout::Diff(sign));
                if !diff_proven {
                    // The rows may be a markdown list or indented text.
                    readings.push(BoxLayout::Markdown);
                }
            }
        }
    }
    let mut decided = None;
    for layout in readings {
        let reading = match box_line(&mut rows, (head_row, gutter_end, layout, evidence.cut_end), pointed.row) {
            Some(mut line) => {
                line.markdown_pair = !proven;
                box_line_uri(&mut rows, &line, pointed)
            }
            None => Joined::None,
        };
        decided = Some(match decided {
            None => reading,
            // A markdown target the diff reading joins opens as the padded markdown rule always
            // read it, a leading sign as the diff's, so no such link stops opening.
            Some(earlier) if markdown_target(&mut rows, &earlier) => earlier,
            Some(earlier) => agreed(earlier, reading),
        });
    }
    // An `unreadable` line opens no URL it breaks across rows in any layout; a URL its delimiters
    // close on one row reads the same in every layout.
    match decided {
        Some(Joined::Whole(parts)) if unreadable && parts.len() > 1 => Joined::Unproven,
        // Outside a proven box a one-row URL stays the single-row rules' (stricter than SonicTerm).
        Some(Joined::Whole(parts)) if !proven && parts.len() == 1 => Joined::None,
        Some(decided) => decided,
        None => Joined::Unproven,
    }
}

/// What the box around the pointed row shows: its contiguous numbered rows and hanging
/// continuations the frame displays.
struct BoxEvidence {
    /// A row holds text right after the gutter other than a diff sign: the box is markdown.
    markdown_text: bool,
    /// A numbered row leaves the sign column blank: a diff context line.
    context_row: bool,
    /// Two numbered rows in order spell consecutive line numbers, which numbered source shows
    /// and other indented output almost never does.
    consecutive: bool,
    /// A `-` row and a later `+` row carry the same line number: a diff replaced that line, which
    /// a markdown list or other output never shows.
    replaced_line: bool,
    /// A numbered row holds a sign right after the gutter in a shape markdown never writes there.
    signed_row: bool,
    /// The box's cut: the widest text column of all its displayed rows, which every row the box
    /// wrapped reaches, however far from the pointer.
    cut_end: usize,
    /// Every displayed row that reaches `cut_end` is followed by a row that may continue its line,
    /// as a row the box cut is.
    cut_wrapped: bool,
}

fn box_evidence(rows: &mut BoxRows<'_>, pointed_row: u32, gutter_end: usize) -> BoxEvidence {
    let mut evidence = BoxEvidence {
        markdown_text: false,
        context_row: false,
        consecutive: false,
        replaced_line: false,
        signed_row: false,
        cut_end: 0,
        cut_wrapped: true,
    };
    // Each numbered row's screen row, line number and sign.
    let mut numbers: Vec<(u32, u64, Option<char>)> = Vec::new();
    let mut read = |rows: &mut BoxRows<'_>, row_number: u32| -> bool {
        let Some(row) = rows.get(row_number) else { return false };
        let numbered = row.gutter_end == Some(gutter_end);
        let continuation = row.text.is_some() && row.blank_through(gutter_end);
        if row.soft_wrapped || !(numbered || continuation) {
            return false;
        }
        let cell = row.cells.get(gutter_end + 1);
        evidence.markdown_text |= cell.is_some_and(|cell| !box_blank(cell) && !matches!(cell.ch, '+' | '-'));
        evidence.context_row |= numbered && cell.is_none_or(box_blank);
        evidence.signed_row |= numbered && cell.is_some_and(|cell| diff_only_sign(row, gutter_end, cell.ch));
        evidence.cut_end = evidence.cut_end.max(row.text.map_or(0, |(_, last)| last));
        if let Some(number) = row.number.filter(|_| numbered) {
            let sign = cell.map(|cell| cell.ch).filter(|sign| matches!(sign, '+' | '-'));
            numbers.push((row_number, number, sign));
        }
        true
    };
    let mut below = pointed_row;
    while below < rows.end && read(rows, below) {
        below += 1;
    }
    let mut above = pointed_row;
    while above > rows.top && read(rows, above - 1) {
        above -= 1;
    }
    // A row that reaches the cut and is followed by a row that ends its line was not wrapped there.
    for row_number in above..below {
        let reaches = rows.get(row_number).is_some_and(|row| row.text.is_some_and(|(_, last)| last == evidence.cut_end));
        let next = row_number + 1;
        let ended = if rows.contains(next) { row_ends_line(rows.get(next), gutter_end) } else { !rows.rows_below() };
        if reaches && ended {
            evidence.cut_wrapped = false;
            break;
        }
    }
    numbers.sort_unstable_by_key(|(row_number, _, _)| *row_number);
    evidence.consecutive = numbers.windows(2).any(|pair| pair[1].1 == pair[0].1 + 1);
    evidence.replaced_line = numbers.windows(2).any(|pair| pair[0].1 == pair[1].1 && pair[0].2 == Some('-') && pair[1].2 == Some('+'));
    evidence
}

/// Whether a numbered row whose gutter ends left of `gutter_end`, in the run of non-blank rows
/// around `pointed_row` the frame displays, may own the pointed row: the pointed row is blank
/// through that narrower gutter, so a head read with `gutter_end` may be one of its continuations.
fn narrower_gutter_nearby(rows: &mut BoxRows<'_>, pointed_row: u32, gutter_end: usize) -> bool {
    let narrower = |rows: &mut BoxRows<'_>, row_number: u32| -> Option<Option<usize>> {
        let row = rows.get(row_number)?;
        row.text?;
        Some(row.gutter_end.filter(|end| *end < gutter_end))
    };
    let mut ends: Vec<usize> = (pointed_row..rows.end).map_while(|row| narrower(rows, row)).flatten().collect();
    let top = rows.top;
    ends.extend((top..pointed_row).rev().map_while(|row| narrower(rows, row)).flatten().collect::<Vec<_>>());
    let Some(pointed) = rows.get(pointed_row) else { return false };
    ends.iter().any(|end| pointed.blank_through(*end))
}

/// The numbered row whose line holds `pointed_row`. Walking up, a numbered row qualifies when its
/// gutter ends before the text of every row between it and the pointer; the qualifying row with
/// the smallest gutter end wins, the nearest on a tie, so a continuation whose text starts with
/// digits never heads a line its real head owns. The walk ends at a row outside the box, or at the
/// frame's top, above which the head may lie.
fn box_line_head(rows: &mut BoxRows<'_>, pointed_row: u32) -> LineHead {
    let mut leftmost_text = usize::MAX;
    let mut best: Option<(u32, usize)> = None;
    let top = rows.top;
    for row_number in (top..=pointed_row).rev() {
        let Some(row) = rows.get(row_number) else {
            return best.map_or(LineHead::Outside, |(head_row, _)| LineHead::Row(head_row));
        };
        let Some((first, _)) = row.text else {
            return best.map_or(LineHead::Outside, |(head_row, _)| LineHead::Row(head_row));
        };
        let candidate = row.gutter_end.filter(|end| *end < leftmost_text);
        if let Some(end) = candidate {
            if best.map(|(_, best_end)| best_end) == Some(end) {
                // An earlier line's head starts here, so the walk ends.
                break;
            }
            // A first or strictly smaller qualifying gutter end is the nearest real head so far.
            best = best.filter(|(_, best_end)| *best_end < end).or(Some((row_number, end)));
        }
        let outside = best.is_some_and(|(_, best_end)| first <= best_end) && candidate.is_none();
        if outside || row.soft_wrapped {
            break;
        }
        leftmost_text = leftmost_text.min(first);
        if row_number == top {
            // A head above the frame's top may still own the line.
            return match best {
                Some((head_row, _)) => LineHead::Row(head_row),
                None => LineHead::AboveFrame,
            };
        }
    }
    best.map_or(LineHead::Outside, |(head_row, _)| LineHead::Row(head_row))
}

/// The gutter end and sign of a line whose head lies above the frame, read from the box below:
/// the first numbered row past the pointer whose gutter ends left of every row between, and the
/// `+` or `-` the visible rows from the frame's top to the pointer share at the sign column, if
/// any, or `Err` when those rows disagree on it or hold one that is not plain. `None` when no
/// such row shows the box.
fn headless_line(rows: &mut BoxRows<'_>, pointed_row: u32) -> Option<(usize, Result<Option<char>, ()>)> {
    let mut leftmost_text = usize::MAX;
    let mut numbered = None;
    for row_number in pointed_row..rows.end {
        let row = rows.get(row_number)?;
        let (first, _) = row.text?;
        if row.soft_wrapped {
            return None;
        }
        if row_number > pointed_row && row.gutter_end.is_some_and(|end| end < leftmost_text) {
            numbered = Some(row_number);
            break;
        }
        leftmost_text = leftmost_text.min(first);
    }
    let gutter_end = rows.get(numbered?)?.gutter_end?;
    // A diff line's sign column is the same on all its rows; the rows from the frame's top to the
    // pointer belong to the pointed line or to lines whose heads also scrolled away.
    let mut sign = None;
    let top = rows.top;
    for row_number in top..=pointed_row {
        let row = rows.get(row_number)?;
        let row_sign = match row.cells.get(gutter_end + 1) {
            Some(cell) if cell.plain() && matches!(cell.ch, '+' | '-') => Some(cell.ch),
            Some(cell) if matches!(cell.ch, '+' | '-') => return Some((gutter_end, Err(()))),
            _ => None,
        };
        if row_number > top && row_sign != sign {
            return Some((gutter_end, Err(())));
        }
        sign = row_sign;
    }
    Some((gutter_end, Ok(sign)))
}

/// The geometry of the line around `pointed_row` whose head is `head_row` (or above the frame),
/// with gutter end `gutter_end` and `layout`; `None` when the rows do not form that line.
fn box_line(
    rows: &mut BoxRows<'_>,
    (head_row, gutter_end, layout, cut_end): (Option<u32>, usize, BoxLayout, usize),
    pointed_row: u32,
) -> Option<BoxLine> {
    let reach = (MAX_LINK_ROWS - 1) as u32;
    let line_row = |rows: &mut BoxRows<'_>, row_number: u32| {
        Some(row_number) == head_row || rows.get(row_number).is_some_and(|row| layout.continues(row, gutter_end))
    };
    let row_start = |rows: &mut BoxRows<'_>, row_number: u32| {
        let row = rows.get(row_number)?;
        Some(row.text_from(layout.text_start(row, gutter_end)))
    };
    if !line_row(rows, pointed_row) {
        return None;
    }
    // The line's rows above the pointer, up to its head, `reach` rows, or text past
    // `MAX_LINK_BYTES`, beyond which no URL covering the pointer can reach.
    let mut first_row = pointed_row;
    let mut above_bytes = 0;
    let open_above = loop {
        if Some(first_row) == head_row {
            break false;
        }
        let past_bound = pointed_row - first_row >= reach || above_bytes > MAX_LINK_BYTES;
        if first_row == rows.top || past_bound {
            break true;
        }
        if !line_row(rows, first_row - 1) {
            // The line's head is not where it was read, so its rows cannot be told apart.
            return None;
        }
        first_row -= 1;
        above_bytes += row_start(rows, first_row).unwrap_or(0);
    };
    // The line's rows below the pointer, within the same bounds.
    let mut last_row = pointed_row;
    let mut below_bytes = 0;
    let open_below = loop {
        let below = last_row + 1;
        if !rows.contains(below) {
            break rows.rows_below();
        }
        if !line_row(rows, below) {
            break !row_ends_line(rows.get(below), gutter_end);
        }
        if below - pointed_row > reach || below_bytes > MAX_LINK_BYTES {
            break true;
        }
        last_row = below;
        below_bytes += row_start(rows, below).unwrap_or(0);
    };
    Some(BoxLine {
        gutter_end,
        layout,
        markdown_pair: false,
        cut_end,
        head_row: head_row.filter(|head_row| *head_row >= first_row),
        first_row,
        last_row,
        open_above,
        open_below,
    })
}

/// Rebuild `line` from its rows and decide the URL token under `pointed` in it.
fn box_line_uri(rows: &mut BoxRows<'_>, line: &BoxLine, pointed: At) -> Joined {
    // Each row from its own text start, after the gutter and any sign. A row that reaches the cut
    // joins the next row directly, as the box cut the source line there; a row that stops short
    // ended at a blank in the source, and a continuation that starts right of the hang column
    // began with one, so one blank separates those rows. A row with no text after its sign adds
    // nothing but that blank.
    let hang = line.layout.hang(line.gutter_end);
    let mut text = String::new();
    let mut bytes = Vec::new();
    let mut byte_rows = Vec::new();
    // The cell each byte came from; `None` for a blank the rebuild adds between rows.
    let mut byte_cells = Vec::new();
    for row_number in line.first_row..=line.last_row {
        let Some(row) = rows.get(row_number) else { return Joined::None };
        let start_col = line.layout.text_start(row, line.gutter_end);
        let row_last = row.text.map_or(0, |(_, last)| last);
        let continuation = Some(row_number) != line.head_row && row_number > line.first_row;
        if continuation && start_col.is_none_or(|start| start > hang) {
            text.push(' ');
            bytes.push(LineByte::Blank);
            byte_rows.push(row_number);
            byte_cells.push(None);
        }
        for column in start_col.unwrap_or(row_last + 1)..=row_last {
            let byte = match row.cells.get(column) {
                None => LineByte::Blank,
                Some(cell) if cont(cell) => {
                    // Adds no text after its wide lead, and hides what it holds without one.
                    let lead = column.checked_sub(1).and_then(|lead| row.cells.get(lead));
                    if !orphan_wide_cont(cell, lead) {
                        continue;
                    }
                    LineByte::Unsafe
                }
                Some(cell) if !cell.plain() => cell_line_byte(cell),
                Some(cell) if box_blank(cell) => LineByte::Blank,
                Some(cell) if cell.ch.is_ascii() && !cell.ch.is_control() => LineByte::Text(row_number, column as u16),
                Some(_) => LineByte::Prose,
            };
            text.push(if let LineByte::Text(..) = byte { row.cells[column].ch } else { ' ' });
            bytes.push(byte);
            byte_rows.push(row_number);
            byte_cells.push(Some((row_number, column as u16)));
        }
        if row_number < line.last_row && row_last < line.cut_end {
            // A row above the last that stops before the cut ended at a blank in the source.
            text.push(' ');
            bytes.push(LineByte::Blank);
            byte_rows.push(row_number);
            byte_cells.push(None);
        }
    }
    // An unsafe cell under the pointer belongs to the token too, so a URL holding it is refused,
    // never left to a single-row prefix (stricter than SonicTerm, which reads text cells only).
    let Some(pointed_byte) = byte_cells.iter().position(|cell| *cell == Some((pointed.row, pointed.col))) else {
        // The pointer is on padding, the gutter or a sign.
        return Joined::None;
    };
    box_token_uri(rows, line, (&text, &bytes, &byte_rows), pointed_byte)
}

/// Decide the URL token around `pointed_byte` of a rebuilt line: each byte's character (blanks,
/// prose and unsafe cells as spaces), what each byte is, and the row each came from.
fn box_token_uri(rows: &mut BoxRows<'_>, line: &BoxLine, (text, bytes, byte_rows): (&str, &[LineByte], &[u32]), pointed_byte: usize) -> Joined {
    let raw = text.as_bytes();
    let in_token = |index: usize| token_byte(bytes[index], raw[index] as char);
    if !in_token(pointed_byte) {
        // On a delimiter, not on URL text.
        return Joined::None;
    }
    let row_of = |byte: &LineByte| match byte {
        LineByte::Text(row, _) => Some(*row),
        _ => None,
    };
    let token_start = (0..pointed_byte).rev().find(|index| !in_token(*index)).map_or(0, |index| index + 1);
    let token_end = (pointed_byte..bytes.len()).find(|index| !in_token(*index)).unwrap_or(bytes.len());
    let token = &bytes[token_start..token_end];
    let open_start = line.open_above && token_start == 0;
    let open_end = line.open_below && token_end == bytes.len();
    let lowered = text[token_start..token_end].to_ascii_lowercase();
    let scheme_at = lowered.find("http://").into_iter().chain(lowered.find("https://")).min();
    // The scheme of the URL the pointer is on: the last one the token starts at or before it.
    let pointed_scheme = ["http://", "https://"]
        .iter()
        .flat_map(|scheme| lowered.match_indices(scheme).map(|(at, _)| at))
        .filter(|at| token_start + at <= pointed_byte)
        .max()
        .or(scheme_at);
    let opened_by_pair = pointed_scheme.is_some_and(|at| {
        let opener_at = token_start + at;
        text.get(opener_at.saturating_sub(2)..opener_at) == Some("](")
    });
    if line.markdown_pair && !opened_by_pair {
        // The rows may be other output, so the single-row rules decide before any refusal here.
        return Joined::None;
    }
    if (open_start || open_end) && scheme_at.is_none_or(|at| pointed_byte >= token_start + at) {
        // The token may run into rows the rebuild did not read, so its extent and scheme are
        // unknown and no prefix of it opens.
        return Joined::Unproven;
    }
    let multi_row = byte_rows[token_start] != byte_rows[token_end - 1];
    let Some(scheme_at) = scheme_at.filter(|_| multi_row) else { return Joined::None };
    if pointed_byte < token_start + scheme_at {
        // On text that leads the URL.
        return Joined::None;
    }
    // From here the token is a URL the box broke across rows: it opens whole or not at all.
    if token.contains(&LineByte::Unsafe) {
        return Joined::Unproven;
    }
    let matches = url_scan::find_urls(text);
    let scheme_start = token_start + pointed_scheme.unwrap_or(scheme_at);
    // Each earlier URL in the token ends at its own delimiters, or where the scanner ends it; when
    // one runs over the pointed scheme, that scheme is its text, not a URL of its own.
    let earlier_end = |start: usize| match delimited_url(text, start, token_end) {
        Delimited::Url(earlier) => earlier.end,
        Delimited::Absent => matches.iter().find(|found| found.start == start).map_or(start, |found| found.end),
        Delimited::Unresolved => token_end,
    };
    let swallowed = ["http://", "https://"]
        .iter()
        .flat_map(|scheme| lowered.match_indices(scheme).map(|(at, _)| token_start + at))
        .filter(|start| *start < scheme_start)
        .any(|start| earlier_end(start) > scheme_start);
    if swallowed {
        return Joined::Unproven;
    }
    let delimited = match delimited_url(text, scheme_start, token_end) {
        Delimited::Url(found) => Some(found),
        // Its closer is another character or its text is no URL: the scanner's shorter reading
        // would be a prefix of it.
        Delimited::Unresolved => return Joined::Unproven,
        Delimited::Absent => None,
    };
    let found = delimited.as_ref().or_else(|| matches.iter().find(|found| (found.start..found.end).contains(&pointed_byte)));
    let Some(found) = found.filter(|found| (found.start..found.end).contains(&pointed_byte)) else {
        // No valid URL as a whole, too long or malformed, or the pointer is after its closer.
        return Joined::Unproven;
    };
    let url_bytes = &bytes[found.start..found.end];
    // Each row's cells: (screen row, first column, column past the last).
    let mut spans: Vec<(u32, u16, u16)> = Vec::new();
    for byte in url_bytes {
        let LineByte::Text(row, column) = *byte else { return Joined::Unproven };
        match spans.last_mut() {
            Some(span) if span.0 == row && span.2 == column => span.2 += 1,
            _ => spans.push((row, column, column + 1)),
        }
    }
    let later_scheme = found.url.match_indices("://").skip(1).any(|(index, _)| row_of(&url_bytes[index]) != row_of(&url_bytes[0]));
    if spans.len() > MAX_LINK_ROWS || found.url.len() > MAX_LINK_BYTES || later_scheme || text.get(found.start..found.end) != Some(found.url.as_str()) {
        // Two destinations or an unbounded one would open.
        return Joined::Unproven;
    }
    if spans.len() < 2 && delimited.is_none() {
        // On one row the single-row rules decide; a delimited URL in a wrapped token stays this rule's.
        return Joined::None;
    }
    let pair = found.start >= 2 && text.get(found.start - 2..found.start) == Some("](") && text.get(found.end..found.end + 1) == Some(")");
    if line.markdown_pair && !pair && spans.len() > 1 {
        // A `](` URL spanning rows without its `)`, as before a title: inert, never a prefix.
        return Joined::Unproven;
    }
    let mut parts = Parts::new();
    for (row, start, end) in spans {
        let Some(cells) = rows.get(row).map(|r| r.cells[usize::from(start)..usize::from(end)].to_vec()) else { return Joined::Unproven };
        parts.push((row, cells));
    }
    Joined::Whole(parts)
}

#[cfg(test)]
thread_local! {
    /// The screen rows the box rule read, for tests that bound its reads.
    static BOX_READS: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
#[path = "hardwrap_tests.rs"]
mod hardwrap_tests;

#[cfg(test)]
#[path = "hardwrap_box_tests.rs"]
mod hardwrap_box_tests;

#[cfg(test)]
#[path = "hardwrap_box_quoted_tests.rs"]
mod hardwrap_box_quoted_tests;

#[cfg(test)]
#[path = "hardwrap_box_proof_tests.rs"]
mod hardwrap_box_proof_tests;
