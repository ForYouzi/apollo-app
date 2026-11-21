//! Claude Code's numbered box: SonicTerm's `path_target_hardwrap_markdown_tests.rs`
//! (origin/main 3284d2e9) on Apollo's grid, and the helpers the other box test
//! files share. Left out, as they test SonicTerm-only machinery:
//! - `padded_markdown_closer_tail_stops_at_pane_border`, and the `│x` border
//!   case of `padded_markdown_target_joins_across_hanging_indent`: a
//!   multiplexer's pane border inside one grid (Apollo panes are separate
//!   terminals, so its box rule has no pane columns to split);
//! - `padded_markdown_structural_cells_refuse_wide_flags`: sets WIDE flags on
//!   ASCII cells, which no output written to a terminal can do;
//! - the bracketed-rule half of `closer_tail_at_edge_above_undisplayed_row_is_not_proven`:
//!   SonicTerm's frame observer on its edge rule, which Apollo keeps as ported
//!   at 68250eef and which reads the row's own wrap flag (the box half is here);
//! - `hardwrapped_closer_followed_by_non_ascii_resolves` and
//!   `hardwrapped_closer_tail_that_wraps_on_stays_inert`: edge-rule cases,
//!   already in `hardwrap_tests.rs`.

use super::super::Vt;
use super::hardwrap_tests::{at, hard, pick, spans};
use super::{BOX_READS, Joined, MAX_LINK_BYTES, MAX_LINK_ROWS, numbered};

/// The link target the screenshot shape carries, cut after `Usag`.
const DOC_URI: &str = "https://github.com/D0n9X1n/bufferlines.nvim/wiki/Installation-and-Usage-zh-CN";

/// `lines` followed by the next numbered source line replaced, as Claude Code's Update view shows
/// it: removed, then added. The box rule reads a box only where consecutive line numbers or a
/// replaced line prove it is one, and a replaced line also proves a diff's sign column.
pub(super) fn with_next<Line: AsRef<str>>(lines: &[Line]) -> Vec<String> {
    let mut out = lines.iter().map(|line| line.as_ref().to_string()).collect::<Vec<_>>();
    // Each numbered row's gutter end and number; a continuation that opens with digits sits right
    // of the head's gutter, so the last row with the narrowest gutter numbers the box's lines.
    let numbered = out.iter().filter_map(|line| {
        let indent = line.len() - line.trim_start().len();
        let digits = line[indent..].chars().take_while(char::is_ascii_digit).count();
        let after = line[indent + digits..].chars().next();
        let number = line[indent..indent + digits].parse::<u64>().ok()?;
        ((1..=6).contains(&digits) && after == Some(' ')).then_some((indent + digits, number))
    });
    let last = numbered.fold(None, |best: Option<(usize, u64)>, (gutter_end, number)| match best {
        Some((best_end, _)) if best_end < gutter_end => best,
        _ => Some((gutter_end, number)),
    });
    if let Some((gutter_end, number)) = last {
        // The next line keeps the gutter's right edge, removed and then added.
        out.push(format!("{:>gutter_end$} - x", number + 1));
        out.push(format!("{:>gutter_end$} + y", number + 1));
    }
    out
}

/// Display cells `s` takes: CJK and fullwidth punctuation take two.
fn width(s: &str) -> usize {
    s.chars().map(|c| if ('\u{1100}'..).contains(&c) && !('\u{300}'..='\u{36f}').contains(&c) { 2 } else { 1 }).sum()
}

/// `lines` padded with blanks to `content` cells, as a box draws them.
fn pad<Line: AsRef<str>>(lines: &[Line], content: usize) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let line = line.as_ref();
            assert!(width(line) <= content, "line wider than the box: {line}");
            format!("{line}{}", " ".repeat(content - width(line)))
        })
        .collect()
}

/// `lines` and the box's next numbered line [`with_next`] adds, padded to `content` cells.
fn padded_rows(lines: &[&str], content: usize) -> Vec<String> {
    pad(&with_next(lines), content)
}

/// The diff gutter the view draws: blanks, a two-digit line number, a blank, then the sign column.
pub(super) fn gutter(number: u32, sign: char) -> String {
    format!("    {number:>2} {sign}")
}

/// Lay one source line out as the box does: `head` (its gutter) and `text` cut every `cut_cols`
/// columns, each continuation hanging at `hang` with the line's sign repeated just before it.
pub(super) fn box_line(head: &str, sign: char, text: &str, cut_cols: usize, hang: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let first_len = (cut_cols - head.len()).min(text.len());
    rows.push(format!("{head}{}", &text[..first_len]));
    let mut rest = &text[first_len..];
    while !rest.is_empty() {
        let take = (cut_cols - hang).min(rest.len());
        rows.push(format!("{}{sign}{}", " ".repeat(hang - 1), &rest[..take]));
        rest = &rest[take..];
    }
    rows
}

/// The spans (row, first column, column past the last) a URL of `uri_len` bytes covers when it
/// starts at `(row, col)` of a box line cut at `cut_cols` whose continuations hang at `hang`.
pub(super) fn url_spans(row: u16, col: usize, uri_len: usize, cut_cols: usize, hang: usize) -> Vec<(u16, u16, u16)> {
    let mut spans = Vec::new();
    let (mut row, mut col, mut left) = (row, col, uri_len);
    while left > 0 {
        let take = (cut_cols - col).min(left);
        spans.push((row, col as u16, (col + take) as u16));
        left -= take;
        row += 1;
        col = hang;
    }
    spans
}

/// What the app links at a cell, and where it shows, from one `link_at`.
pub(super) fn hit(vt: &Vt, row: u16, col: u16) -> Option<(String, Vec<(u16, u16, u16)>)> {
    vt.link_at(col, row, pick).map(|hit| (hit.target, hit.spans))
}

/// Every pointer inside `spans` of `lines`, shown above the box's next numbered line, resolves to
/// `expected` with exactly those spans highlighted.
pub(super) fn assert_joined(lines: &[String], cols: u16, spans: &[(u16, u16, u16)], expected: &str) {
    let lines = with_next(lines);
    // The pane holds every line, so no row scrolls into history.
    let vt = hard(cols, lines.len() as u16 + 4, &lines);
    let shown: Vec<_> = spans.iter().map(|&(row, start, end)| (row, start, end - 1)).collect();
    for &(row, start, end) in spans {
        for col in start..end {
            let (target, found) = hit(&vt, row, col).unwrap_or_else(|| panic!("no target at row {row} col {col}"));
            assert_eq!(target, expected, "row {row} col {col}");
            assert_eq!(found, shown, "row {row} col {col}");
        }
    }
}

/// The target at `(row, col)` of `lines`, shown above the box's next numbered line, in a
/// `cols`-wide pane.
pub(super) fn target_at(lines: &[String], cols: u16, row: u16, col: u16) -> Option<String> {
    at(&hard(cols, 40, &with_next(lines)), row, col)
}

/// The target at `(row, col)` of exactly `lines` in a `cols` by `rows` pane.
pub(super) fn exact_target_at(lines: &[String], (cols, rows): (u16, u16), row: u16, col: u16) -> Option<String> {
    at(&hard(cols, rows, lines), row, col)
}

/// Write `lines` as hard rows into a `cols` by `rows` terminal and decide the box rule at screen
/// cell `(row, col)` in the frame whose top screen row is `view_top`.
pub(super) fn decide_in_frame(lines: &[String], (cols, rows): (u16, u16), view_top: u32, (row, col): (u32, u16)) -> Joined {
    numbered(&hard(cols, rows, lines), row, col, view_top)
}

/// The URL a decision opens, if it opens one.
pub(super) fn joined_url(decided: &Joined) -> Option<String> {
    let Joined::Whole(parts) = decided else { return None };
    Some(parts.iter().flat_map(|(_, cells)| cells.iter().map(|c| c.ch)).collect())
}

/// The spans (screen row, first column, column past the last) a decision opens.
pub(super) fn joined_spans(decided: &Joined) -> Vec<(u32, u16, u16)> {
    let Joined::Whole(parts) = decided else { return Vec::new() };
    parts.iter().map(|(y, cells)| (*y, cells[0].x, cells[cells.len() - 1].x + 1)).collect()
}

/// The screen rows the box rule reads while running `decide`.
pub(super) fn reads(decide: impl FnOnce() -> Joined) -> (Joined, Vec<u32>) {
    BOX_READS.with(|r| r.borrow_mut().clear());
    let decided = decide();
    (decided, BOX_READS.with(|r| r.take()))
}

/// Claude Code's numbered markdown row: the target is cut before the pane edge, padding follows
/// it, and the continuation hangs at the `[` column; every fragment resolves the whole target.
#[test]
fn padded_markdown_target_joins_across_hanging_indent() {
    let cut = DOC_URI.find("Usag").unwrap() + "Usag".len();
    let first = format!("    60 [安装]({}", &DOC_URI[..cut]);
    let first_cols = width(&first) as u16;
    let tail_len = (DOC_URI.len() - cut) as u16;
    // The target opens after `    60 [安装](`: seven gutter cells, `[`, two wide cells, `]`, `(`.
    let start = 14u16;
    for tail in [")。", ") 。", ")", ")。其他"] {
        let second = format!("       {}{tail}", &DOC_URI[cut..]);
        let vt = hard(100, 10, &padded_rows(&[&first, &second], first_cols as usize + 8));
        let pointers = (start..first_cols).map(|c| (0, c)).chain((7..7 + tail_len).map(|c| (1, c)));
        for (row, col) in pointers {
            assert_eq!(at(&vt, row, col).as_deref(), Some(DOC_URI), "tail {tail:?} row {row} col {col}");
            assert_eq!(spans(&vt, row, col), vec![(0, start, first_cols - 1), (1, 7, 7 + tail_len - 1)], "tail {tail:?}");
        }
        // The closer and the cells after it never join the destination footprint.
        assert_eq!(at(&vt, 1, 7 + tail_len), None, "tail {tail:?}");
    }
}

/// Apollo's issue shapes: a numbered row, and a `+` diff row with prose before the link. The box
/// shows no second line number, so only the markdown `](`…`)` pair joins them.
#[test]
fn padded_markdown_issue_shapes_join() {
    let cut = DOC_URI.find("Usag").unwrap() + 4;
    let vt = hard(110, 10, &pad(&[&format!("    60 [安装与使用]({}", &DOC_URI[..cut]), &format!("       {})。", &DOC_URI[cut..])], 95));
    assert_eq!(at(&vt, 0, 30).as_deref(), Some(DOC_URI));
    assert_eq!(at(&vt, 1, 9).as_deref(), Some(DOC_URI));
    let nf = "https://github.com/ryanoasis/nerd-fonts/wiki";
    let cut = nf.find("nerd-fon").unwrap() + 8;
    let vt = hard(90, 10, &pad(&[&format!("    59 +… [Nerd Font]({}", &nf[..cut]), &format!("       +{})。…", &nf[cut..])], 70));
    assert_eq!(at(&vt, 0, 30).as_deref(), Some(nf));
    assert_eq!(at(&vt, 1, 9).as_deref(), Some(nf));
}

/// A target split over three rows joins only when the middle row ends at the first row's column.
#[test]
fn padded_markdown_target_joins_three_rows() {
    let uri = "https://example.com/aaaaaaaaaa/bbbbbbbbbb/cccccccccc/dddddddddd/eeeeeeeeee";
    let first = format!("  1 [doc]({}", &uri[..30]);
    let middle_len = first.len() - 4;
    let middle = format!("    {}", &uri[30..30 + middle_len]);
    let last = format!("    {}).", &uri[30 + middle_len..]);
    let vt = hard(90, 10, &padded_rows(&[&first, &middle, &last], first.len() + 6));
    for (row, col) in [(0, 12), (1, 4), (2, 5)] {
        assert_eq!(at(&vt, row, col).as_deref(), Some(uri), "row {row}");
        assert_eq!(spans(&vt, row, col).len(), 3, "row {row}");
    }
}

/// On a `+` diff row the sign repeated before the continuation is gutter, not URL text: the target
/// joins without it, and a continuation that holds only the closer keeps the first row's URL.
#[test]
fn padded_markdown_target_skips_repeated_diff_sign() {
    let uri = "https://github.com/ryanoasis/nerd-fonts/wiki";
    let first = format!("    59 +see [Nerd Font]({}", &uri[..38]);
    let second = format!("       +{}) and more", &uri[38..]);
    let vt = hard(90, 10, &padded_rows(&[&first, &second], first.len() + 7));
    for (row, col) in [(0, 30), (1, 9)] {
        assert_eq!(at(&vt, row, col).as_deref(), Some(uri), "row {row}");
    }
    // The repeated sign itself is not part of the target.
    assert_eq!(at(&vt, 1, 7), None);
    // The whole URL fits on the first row and the continuation holds only the closer: the first
    // row's own URL stays clickable, never a URL with the sign appended.
    let whole = "https://github.com/ryanoasis/nerd-fonts";
    let first = format!("    59 +see [Nerd Font]({whole}");
    let vt = hard(90, 10, &padded_rows(&[&first, "       +)。其他插件"], first.len() + 7));
    assert_eq!(at(&vt, 0, 30).as_deref(), Some(whole));
}

/// The box wraps by characters, so a URL that reaches the cut continues in the next row whatever
/// precedes it: a prose parenthesis, a comma at the cut, a host split across rows. A lone
/// continuation fixes the hang column wherever it starts; a space in the fragment ends the URL.
#[test]
fn padded_box_joins_every_wrapped_url_shape() {
    let prefix = "https://example.com/aaaa/bbbb";
    let split_host = "https://good.test@evil.test/cccc";
    let cases: [(&str, String, String, &str); 6] = [
        ("plain paren", format!("    1 see ({prefix}"), "      cccc)".into(), "https://example.com/aaaa/bbbbcccc"),
        ("comma", format!("  1 [a]({prefix},"), "    cccc)".into(), "https://example.com/aaaa/bbbb,cccc"),
        // A continuation one column right of the hang began with a blank: the URL ended there.
        ("unaligned", format!("  1 [a]({prefix}"), "     cccc)".into(), "https://example.com/aaaa/bbbb"),
        ("space", format!("  1 [a]({prefix}"), "    cc cc)".into(), "https://example.com/aaaa/bbbbcc"),
        ("split authority", "  1 [a](https://good.test".into(), "    @evil.test/cccc)".into(), split_host),
        ("bare", format!("  1 see {prefix}"), "    cccc and more".into(), "https://example.com/aaaa/bbbbcccc"),
    ];
    for (label, first, second, expected) in cases {
        let vt = hard(90, 10, &padded_rows(&[&first, &second], first.len() + 6));
        let col = first.find("https").unwrap() as u16 + 2;
        assert_eq!(at(&vt, 0, col).as_deref(), Some(expected), "{label}");
    }
    // The split host is the destination the source line names, which the hover preview shows.
    let joined = crate::links::url_scan::find_urls(&format!("{split_host})"));
    assert_eq!(joined.first().map(|found| found.url.as_str()), Some(split_host));
}

/// Shapes that are not one numbered line never open a joined destination. Rows with no gutter
/// keep the first row's own URL; a row shaped like a continuation that does not continue the line
/// (another sign, a sign inside the gutter) may still hold the rest of the URL, so the first row's
/// fragment stays inert rather than opening as a prefix.
#[test]
fn padded_markdown_refusals_keep_single_row_detection() {
    let prefix = "https://example.com/aaaa/bbbb";
    let cases: [(&str, String, String, Option<&str>); 3] = [
        // Without a numbered gutter the row is not the box, even when a later row looks aligned.
        ("no gutter", format!("See [x]({prefix}"), "    cccc) next item".into(), Some(prefix)),
        // A sign that differs from the opener row's starts no continuation of this line.
        ("other sign", format!("    59 +[a]({prefix}"), "       -cccc)".into(), None),
        // A sign inside the gutter of an unsigned opener row is no continuation either.
        ("unexpected sign", format!("    59 [a]({prefix}"), "      +cccc)".into(), None),
    ];
    for (label, first, second, row_url) in cases {
        // The box's previous line, a diff context row, shows its consecutive numbers above the
        // malformed row.
        let previous = if first.starts_with("    59") { "    58  x" } else { "" };
        let vt = hard(90, 10, &padded_rows(&[previous, &first, &second], first.len() + 6));
        let col = first.find("https").unwrap() as u16 + 2;
        assert_eq!(at(&vt, 1, col).as_deref(), row_url, "{label}");
        // The continuation never resolves to a destination built from the first row's prefix.
        assert!(at(&vt, 2, 8).is_none_or(|t| !t.starts_with(prefix)), "{label} continuation");
    }
}

/// A combining mark on the `(` the join would remove makes that delimiter unsafe, so nothing joins.
#[test]
fn padded_markdown_refuses_unsafe_opener() {
    let vt = hard(60, 10, &padded_rows(&["  1 [a](\u{301}https://example.com/aaaa/bbbb", "    cccc)"], 40));
    for (row, col) in [(0, 12), (1, 5)] {
        let display = at(&vt, row, col);
        assert!(display.as_deref().is_none_or(|d| !d.contains("cccc")), "{display:?}");
    }
}

/// A soft wrap into a later continuation means that row is not the box's hard-broken text.
#[test]
fn padded_markdown_refuses_soft_wrap_after_first_continuation() {
    let first = "  1 [x](https://example.test/aaaaaaaa";
    let middle = format!("    {}   ", "b".repeat(first.len() - 4));
    // The middle row fills the grid, so four more blanks wrap and place `cc)` at the content column.
    let mut vt = Vt::new(40, 10);
    vt.write(format!("{first}\r\n{middle}    cc)").as_bytes());
    assert!(vt.wraps(1), "fixture must soft-wrap into the third row");
    assert_eq!(vt.link_row(2).unwrap().text.find('c'), Some(4), "the wrapped closer row must start its body at the content column");
    let display = at(&vt, 0, 12);
    assert!(display.as_deref().is_none_or(|d| !d.contains("bbb")), "{display:?}");
}

/// One padded opener shape: the text before the target on the first row, the continuation indent,
/// and the closer. The markdown head hangs at the column after its gutter; the quoted diff-context
/// head keeps a blank sign column, so it hangs one column further right.
#[derive(Clone, Copy)]
struct PaddedShape {
    head: &'static str,
    indent: usize,
    closer: char,
}

const MARKDOWN_SHAPE: PaddedShape = PaddedShape { head: "  1 [x](", indent: 4, closer: ')' };
const QUOTED_SHAPE: PaddedShape = PaddedShape { head: "  1   Get \"", indent: 5, closer: '"' };

/// Build a padded chain of `shape` in a `cols`-wide pane whose rows after the first fill to the
/// opener row's cut, ending with `last_body` and the closer.
fn padded_chain(shape: PaddedShape, cols: usize, middle_rows: usize, last_body: &str) -> (Vec<String>, String) {
    let head = format!("{}https://example.test/", shape.head);
    let first = format!("{head}{}", "a".repeat(cols - 1 - head.len()));
    let indent = " ".repeat(shape.indent);
    let middle = format!("{indent}{}", "b".repeat(first.len() - shape.indent));
    let mut uri = first[shape.head.len()..].to_string();
    let mut lines = vec![first];
    for _ in 0..middle_rows {
        uri.push_str(&middle[shape.indent..]);
        lines.push(middle.clone());
    }
    uri.push_str(last_body);
    lines.push(format!("{indent}{last_body}{}", shape.closer));
    (lines, uri)
}

/// A chain of exactly `MAX_LINK_ROWS` rows joins; one more row keeps the first row alone. Both
/// opener shapes share the bound, so a quoted target cannot grow past it either.
#[test]
fn padded_markdown_enforces_row_bound() {
    for shape in [MARKDOWN_SHAPE, QUOTED_SHAPE] {
        for (total_rows, joins) in [(MAX_LINK_ROWS, true), (MAX_LINK_ROWS + 1, false)] {
            let (lines, uri) = padded_chain(shape, 50, total_rows - 2, "zz");
            let lines = padded_rows(&lines.iter().map(String::as_str).collect::<Vec<_>>(), 49);
            let vt = hard(60, 40, &lines);
            let display = at(&vt, 0, (shape.head.len() + 4) as u16);
            assert_eq!(display.as_deref() == Some(uri.as_str()), joins, "{} rows {total_rows}: {display:?}", shape.head);
            // Past the cap no prefix opens either: the whole target stays inert.
            assert!(joins || display.is_none(), "{} rows {total_rows}: {display:?}", shape.head);
        }
    }
}

/// A joined target of exactly `MAX_LINK_BYTES` joins; one byte more keeps the first row alone.
/// Both opener shapes share the cap, so no padded target grows without limit.
#[test]
fn padded_markdown_enforces_byte_bound() {
    for shape in [MARKDOWN_SHAPE, QUOTED_SHAPE] {
        for (extra, joins) in [(0usize, true), (1, false)] {
            let (probe, _) = padded_chain(shape, 140, 29, "");
            let fixed = probe[0].len() - shape.head.len() + 29 * (probe[1].len() - shape.indent);
            let (lines, uri) = padded_chain(shape, 140, 29, &"z".repeat(MAX_LINK_BYTES - fixed + extra));
            assert_eq!(uri.len(), MAX_LINK_BYTES + extra);
            let lines = padded_rows(&lines.iter().map(String::as_str).collect::<Vec<_>>(), 139);
            let display = at(&hard(150, 40, &lines), 0, (shape.head.len() + 4) as u16);
            // Past the cap no prefix opens either: the whole target stays inert.
            assert_eq!(display.as_deref(), joins.then_some(uri.as_str()), "{} extra {extra}", shape.head);
        }
    }
}

/// A closer at the grid's edge whose row soft-wraps into URL text is not the target's end.
#[test]
fn padded_markdown_refuses_closer_that_wraps_on() {
    let mut vt = Vt::new(40, 10);
    vt.write(format!("  1 [x](https://example.test/a\r\n    {})tail)", "b".repeat(35)).as_bytes());
    assert!(vt.wraps(1), "fixture must soft-wrap after the closer");
    let display = at(&vt, 0, 12);
    assert!(display.as_deref().is_none_or(|d| !d.contains("bbb")), "{display:?}");
}

/// Text another hyperlink owns is never joined into a padded target's reconstructed destination.
#[test]
fn padded_markdown_never_joins_hyperlinked_fragment() {
    let first = "  1 [x](https://example.test/aaaa/bbbb";
    let vt = hard(60, 10, &[format!("{first}{}\r\n    \x1b]8;;https://declared.test/\x1b\\cccc\x1b]8;;\x1b\\)", " ".repeat(6))]);
    assert_eq!(at(&vt, 1, 5).as_deref(), Some("https://declared.test/"));
    let display = at(&vt, 0, 12);
    assert!(display.as_deref().is_none_or(|d| !d.contains("cccc")), "{display:?}");
}

/// A padded chain whose closer is below the displayed frame, or whose opener is above it, is never
/// reconstructed, and the rule reads no row outside the frame.
#[test]
fn padded_markdown_stays_inside_the_frame() {
    let uri = "https://example.test/aaaa/bbbb";
    let lines = padded_rows(&[&format!("  1 [x]({}", &uri[..24]), &format!("    {})", &uri[24..])], 34);
    // Two hard rows written into a one-row terminal: the opener scrolls into history.
    let vt = hard(60, 1, &lines);
    // A one-row view shows either the opener (closer below) or the closer (opener above).
    for (view_top, row, col) in [(0u32, 0u32, 12u16), (1, 1, 5)] {
        let (decided, read) = reads(|| numbered(&vt, row, col, view_top));
        assert!(!matches!(decided, Joined::Whole(_)), "view_top {view_top}: {decided:?}");
        assert!(read.iter().all(|y| *y == view_top), "read outside the frame: {read:?}");
    }
}

/// Every structural cell around the joined URL (the signs, `(` and the closer) must be plain: a
/// combining mark or OSC 8 ownership on any of them prevents the join.
#[test]
fn padded_markdown_structural_cells_must_be_plain() {
    let combining = |c: char| format!("{c}\u{301}");
    let hyperlinked = |c: char| format!("\x1b]8;;https://declared.test/\x1b\\{c}\x1b]8;;\x1b\\");
    let mutations: [(&str, &dyn Fn(char) -> String); 2] = [("combining", &combining), ("hyperlink", &hyperlinked)];
    // Each structural cell is marked alone: the opener row's sign, `(`, the closer and the
    // continuation's repeated sign.
    for marked in ["first sign", "(", ")", "second sign"] {
        for (label, mutate) in mutations {
            let cell = |position: &str, c: char| if position == marked { mutate(c) } else { c.to_string() };
            let first = format!(
                "    59 {}see [Nerd Font]{}https://github.com/ryanoasis/nerd-fon{}",
                cell("first sign", '+'),
                cell("(", '('),
                " ".repeat(7)
            );
            let second = format!("       {}ts/wiki{} and more", cell("second sign", '+'), cell(")", ')'));
            let vt = hard(90, 10, &with_next(&[first, second]));
            for (row, col) in [(0, 30), (1, 9)] {
                let display = at(&vt, row, col);
                assert!(display.as_deref().is_none_or(|d| !d.contains("nerd-fonts/wiki")), "{label} on {marked} at row {row}: {display:?}");
            }
        }
    }
    // Unmutated, the same fixture joins, so each refusal above comes from the marked cell.
    let first = format!("    59 +see [Nerd Font](https://github.com/ryanoasis/nerd-fon{}", " ".repeat(7));
    let vt = hard(90, 10, &with_next(&[first, "       +ts/wiki) and more".into()]));
    assert_eq!(at(&vt, 1, 9).as_deref(), Some("https://github.com/ryanoasis/nerd-fonts/wiki"));
}

/// The box rule reads the closer inside the frame, so on a scrolled-back view whose next row is not
/// displayed the URL ends there, and no row outside the frame is read.
#[test]
fn closer_tail_at_edge_above_undisplayed_row_is_not_proven() {
    // Below the box's previous line, a URL row that reaches the cut, above a closer row filled to
    // the grid edge with `é` and an undisplayed row.
    let first = format!("  2 [x](https://example.test/{}", "a".repeat(40 - 29));
    let lines = ["  1 x".to_string(), first, format!("    bbbb){}", "é".repeat(40 - 9)), "tail)".into()];
    let vt = hard(40, 3, &lines);
    let (decided, read) = reads(|| numbered(&vt, 2, 5, 0));
    let expected = format!("https://example.test/{}bbbb", "a".repeat(40 - 29));
    assert_eq!(joined_url(&decided), Some(expected), "closer in frame: {decided:?}");
    assert!(read.iter().all(|y| *y < 3), "read outside the frame: {read:?}");
}

/// Only a row that reaches the box's cut joins the next row directly. A middle row that stops
/// short ended at a blank in the source, so the URL ends with it; one that runs past the cut moves
/// the cut, so the first row stops short and keeps its own URL.
#[test]
fn padded_markdown_middle_row_must_end_at_the_cut() {
    let first = "  1 [doc](https://example.com/aaaa/bbbb";
    for (middle_len, expected) in [(first.len() - 4 + 3, "https://example.com/aaaa/bbbb"), (4, "https://example.com/aaaa/bbbbcccc")] {
        let middle = format!("    {}", "c".repeat(middle_len));
        let vt = hard(90, 10, &padded_rows(&[first, &middle, "    more)."], first.len() + 8));
        assert_eq!(at(&vt, 0, 12).as_deref(), Some(expected), "middle {middle_len}");
    }
}

/// The live twin of the undisplayed-tail case: with no row below the closer row, a non-ASCII tail
/// that reaches the grid's edge ends the target, and the padded chain below the box's previous
/// line joins.
#[test]
fn padded_markdown_closer_tail_at_edge_joins_on_the_live_view() {
    let first = format!("  2 [x](https://example.test/{}", "a".repeat(40 - 29));
    let lines = ["  1 x".to_string(), first, format!("    bbbb){}", "é".repeat(40 - 9))];
    let decided = numbered(&hard(40, 3, &lines), 2, 5, 0);
    let expected = format!("https://example.test/{}bbbb", "a".repeat(40 - 29));
    assert_eq!(joined_url(&decided), Some(expected), "live twin must join: {decided:?}");
}
