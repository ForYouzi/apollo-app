//! URLs a program broke across rows, joined: SonicTerm's hard-wrap cases
//! (`path_target_hardwrap_uri_tests.rs` at 68250eef) on Apollo's grid. The
//! numbered-box cases are in `hardwrap_box*_tests.rs`. Cases that test
//! SonicTerm-only parts (its pane borders, preview state, settings, frame
//! observer) are left out.

use super::super::Vt;
use super::MAX_LINK_ROWS;

const ISSUE_URI: &str = "https://github.com/D0n9X1n/SonicTerm/issues/1349";
const HARDWRAP_HEAD: &str = "* Done: #1349 (";
const LONG_URI: &str = "https://example.com/aaaaaaaaaa/bbbbbbbbbb/cccccccccc/dddddddddd";

/// `uri` after `head`, cut at `cols` and continued at each indent, `closer` after the last part.
fn hardwrap_lines(cols: usize, head: &str, uri: &str, indents: &[usize], closer: char) -> Vec<String> {
    let (first, mut rest) = uri.split_at(cols - head.len());
    let mut lines = vec![format!("{head}{first}")];
    for (i, indent) in indents.iter().enumerate() {
        let pad = " ".repeat(*indent);
        if i + 1 == indents.len() {
            assert!(indent + rest.len() < cols, "final fragment and closer must fit");
            lines.push(format!("{pad}{rest}{closer}"));
        } else {
            let (part, next) = rest.split_at(cols - indent);
            lines.push(format!("{pad}{part}"));
            rest = next;
        }
    }
    lines
}

/// A `cols` × `rows` terminal a program wrote `lines` to as hard rows.
pub(super) fn hard(cols: u16, rows: u16, lines: &[String]) -> Vt {
    let mut vt = Vt::new(cols, rows);
    vt.write(lines.join("\r\n").as_bytes());
    vt
}

/// What the app's `link_at` callback does with no folder: `links::pick`, and
/// the whole text for a URL the rows proved exactly.
pub(super) fn pick(text: &str, at: usize, scan: crate::vt::LinkScan) -> Option<std::ops::Range<usize>> {
    if scan == crate::vt::LinkScan::Exact {
        return Some(0..text.len());
    }
    let ctx = crate::links::Ctx::new(None);
    crate::links::pick(text, at, scan == crate::vt::LinkScan::Joined, &ctx, |_| None).map(|(r, _)| r)
}

/// What the app links at a cell.
pub(super) fn at(vt: &Vt, row: u16, col: u16) -> Option<String> {
    vt.link_at(col, row, pick).map(|hit| hit.target)
}

pub(super) fn spans(vt: &Vt, row: u16, col: u16) -> Vec<(u16, u16, u16)> {
    vt.link_at(col, row, pick).map_or(Vec::new(), |hit| hit.spans.to_vec())
}

// ---- a wrapper opened at the pane edge -------------------------------------

#[test]
fn hardwrapped_parenthesized_url_resolves_complete_destination() {
    let lines = hardwrap_lines(39, HARDWRAP_HEAD, ISSUE_URI, &[2], ')');
    assert_eq!(lines[0].len(), 39, "first fragment must reach the right edge");
    let start = HARDWRAP_HEAD.len() as u16;
    let tail = (ISSUE_URI.len() - (39 - HARDWRAP_HEAD.len())) as u16;
    let vt = hard(39, 10, &lines);
    for (row, col) in (start..39).map(|c| (0, c)).chain((2..2 + tail).map(|c| (1, c))) {
        assert_eq!(at(&vt, row, col).as_deref(), Some(ISSUE_URI), "row {row} col {col}");
        assert_eq!(spans(&vt, row, col), vec![(0, start, 38), (1, 2, 2 + tail - 1)], "row {row} col {col}");
    }
    for (row, col) in [(0, start - 1), (1, 0), (1, 2 + tail)] {
        assert_eq!(at(&vt, row, col), None, "row {row} col {col}");
    }
}

#[test]
fn hardwrapped_url_preserves_percent_query_and_hyphen_at_cut() {
    let uri = "https://example.com/a-b/c?x=%2Fy&z=1";
    let head = "x (";
    let cols = head.len() + "https://example.com/a".len();
    let lines = hardwrap_lines(cols, head, uri, &[2], ')');
    assert!(lines[0].ends_with("/a") && lines[1].starts_with("  -b/"));
    assert_eq!(at(&hard(cols as u16, 10, &lines), 1, 4).as_deref(), Some(uri));
}

#[test]
fn hardwrapped_url_allows_unicode_prose_prefix() {
    let mut lines = hardwrap_lines(39, "done (", ISSUE_URI, &[2], ')');
    lines[0] = lines[0].replacen("done (", "完成 (", 1);
    assert_eq!(at(&hard(39, 10, &lines), 1, 4).as_deref(), Some(ISSUE_URI));
}

#[test]
fn hardwrapped_url_supports_square_brackets_and_three_rows() {
    let lines = hardwrap_lines(30, "x [", LONG_URI, &[2, 2], ']');
    assert_eq!(lines.len(), 3);
    let vt = hard(30, 10, &lines);
    for row in 0..3 {
        assert_eq!(at(&vt, row, 5).as_deref(), Some(LONG_URI), "row {row}");
        assert_eq!(spans(&vt, row, 5).len(), 3, "row {row}");
    }
}

#[test]
fn hardwrapped_http_accepts_inclusive_bounds() {
    let uri = format!("http://example.test/{}", "a".repeat(169));
    let lines = hardwrap_lines(32, "x [", &uri, &[8; 7], ']');
    assert_eq!(lines.len(), 8);
    let vt = hard(32, 10, &lines);
    for row in 0..8 {
        assert_eq!(at(&vt, row, 10).as_deref(), Some(uri.as_str()), "row {row}");
        assert_eq!(spans(&vt, row, 10).len(), 8);
    }
    // One cell deeper is not one continued line.
    let lines = hardwrap_lines(32, "x [", &uri[..45], &[9], ']');
    assert_ne!(at(&hard(32, 10, &lines), 1, 10).as_deref(), Some(&uri[..45]));
}

#[test]
fn hardwrap_cut_before_path_delimiter_stays_inert() {
    let uri = "https://example.com/page";
    let head = "* see this one here (";
    let cols = head.len() + "https://example.com".len();
    let lines = hardwrap_lines(cols, head, uri, &[2], ')');
    assert!(lines[0].ends_with("https://example.com"));
    assert_eq!(at(&hard(cols as u16, 10, &lines), 0, head.len() as u16 + 5), None, "truncated authority must be inert");
}

#[test]
fn hardwrap_requires_path_slash_before_query_or_fragment() {
    for uri in ["https://example.com?next=/abcde", "https://example.com#name/abcde"] {
        let lines = hardwrap_lines(29, "x (", uri, &[2], ')');
        assert_eq!(at(&hard(29, 10, &lines), 0, 5), None, "{uri}");
    }
}

#[test]
fn hardwrap_never_shadows_a_softwrapped_chain() {
    let mut vt = Vt::new(12, 10);
    vt.write(format!("x ({ISSUE_URI})").as_bytes());
    // The opener row wraps on: the terminal's join owns every fragment, the first included.
    for (row, col) in [(1, 2), (0, 5)] {
        assert_eq!(at(&vt, row, col).as_deref(), Some(ISSUE_URI), "row {row}");
    }
}

#[test]
fn hardwrap_incomplete_does_not_consume_unrelated_targets() {
    let other = "https://other.test/ok";
    let mut lines = hardwrap_lines(39, HARDWRAP_HEAD, ISSUE_URI, &[2], ')');
    lines[1] = lines[1].replace(')', "");
    lines.push(format!("see {other} here"));
    let vt = hard(39, 10, &lines);
    assert_eq!(at(&vt, 0, HARDWRAP_HEAD.len() as u16 + 5), None, "an unproven prefix stays inert");
    assert_eq!(at(&vt, 2, 8).as_deref(), Some(other));
}

#[test]
fn hardwrap_unequal_indent_stays_inert() {
    let lines = hardwrap_lines(30, "x (", LONG_URI, &[2, 4], ')');
    let vt = hard(30, 10, &lines);
    assert_eq!(at(&vt, 0, 5), None);
    assert_ne!(at(&vt, 2, 5).as_deref(), Some(LONG_URI));
}

#[test]
fn hardwrap_second_scheme_is_never_joined() {
    let joined = format!("{LONG_URI}https://b.test/y");
    let lines = hardwrap_lines(30, "x (", &joined, &[2, 2], ')');
    assert_ne!(at(&hard(30, 10, &lines), 1, 5), Some(joined));
}

#[test]
fn hardwrap_without_wrapper_never_joins() {
    let head = "* Done: #1349 ";
    let (first, rest) = ISSUE_URI.split_at(39 - head.len());
    let vt = hard(39, 10, &[format!("{head}{first}"), format!("  {rest}")]);
    assert_ne!(at(&vt, 0, 20).as_deref(), Some(ISSUE_URI));
}

#[test]
fn hardwrap_closer_on_first_row_keeps_existing_detection() {
    let uri = "https://a.test/b";
    assert_eq!(at(&hard(39, 10, &[format!("x ({uri}) more")]), 0, 6).as_deref(), Some(uri));
}

#[test]
fn hardwrap_scan_leaves_osc8_authoritative() {
    let declared = "https://example.com/declared";
    let label = "label-".repeat(6);
    let vt = hard(39, 10, &[format!("x (\x1b]8;;{declared}\x1b\\{label}\x1b]8;;\x1b\\)")]);
    assert_eq!(at(&vt, 0, 6).as_deref(), Some(declared));
}

#[test]
fn hardwrap_opener_must_be_plain() {
    // A combining mark or a marked link on the `(` the join drops makes it no wrapper.
    for opener in ["(\u{301}", "\x1b]8;;https://declared.test/\x1b\\(\x1b]8;;\x1b\\"] {
        let vt = hard(30, 10, &[format!("x {opener}https://example.com/aaaaaaa"), "  bbbb)".into()]);
        assert_ne!(at(&vt, 1, 3).as_deref(), Some("https://example.com/aaaaaaabbbb"), "{opener:?}");
    }
    let vt = hard(30, 10, &["x (https://example.com/aaaaaaa".into(), "  bbbb)".into()]);
    assert_eq!(at(&vt, 1, 3).as_deref(), Some("https://example.com/aaaaaaabbbb"), "the plain fixture joins");
}

#[test]
fn hardwrap_file_uri_is_never_joined() {
    let uri = "file:///tmp/aaaaaaaa/bbbbbbbb/cccccccc/dddddddd";
    let lines = hardwrap_lines(30, "x (", uri, &[2], ')');
    assert_ne!(at(&hard(30, 10, &lines), 1, 4).as_deref(), Some(uri));
}

#[test]
fn rewritten_continuation_is_read_fresh() {
    let lines = hardwrap_lines(39, HARDWRAP_HEAD, ISSUE_URI, &[2], ')');
    let mut vt = hard(39, 10, &lines);
    assert_eq!(at(&vt, 0, 20).as_deref(), Some(ISSUE_URI));
    vt.write(b"\x1b[2;1H\x1b[2K");
    assert_eq!(at(&vt, 0, 20), None, "the closer row is gone: nothing proves the join");
}

#[test]
fn hardwrapped_closer_followed_by_non_ascii_resolves() {
    for (suffix, joins) in [("。", true), ("了解更多", true), ("x", false), (".", true), ("), more", false)] {
        let mut lines = hardwrap_lines(39, HARDWRAP_HEAD, ISSUE_URI, &[2], ')');
        lines[1].push_str(suffix);
        let got = at(&hard(39, 10, &lines), 1, 3);
        assert_eq!(got.as_deref() == Some(ISSUE_URI), joins, "suffix {suffix:?}: {got:?}");
    }
}

#[test]
fn hardwrapped_closer_tail_that_wraps_on_stays_inert() {
    let mut vt = Vt::new(30, 10);
    vt.write(format!("x (https://example.com/aaaaaaa\r\n  bb){}tail)", "é".repeat(25)).as_bytes());
    for (row, col) in [(0, 5), (1, 2)] {
        assert_ne!(at(&vt, row, col).as_deref(), Some("https://example.com/aaaaaaabb"), "row {row}");
    }
}

#[test]
fn hardwrap_row_bound_leaves_a_longer_chain_unjoined() {
    // 29 cells on the first row, 30 on each of the next, the closer on row `MAX_LINK_ROWS + 1`.
    let uri = format!("https://example.test/{}", "a".repeat(29 + (MAX_LINK_ROWS - 1) * 30 + 10 - 21));
    let lines = hardwrap_lines(32, "x [", &uri, &vec![2; MAX_LINK_ROWS], ']');
    assert_eq!(lines.len(), MAX_LINK_ROWS + 1);
    let vt = hard(32, MAX_LINK_ROWS as u16 + 2, &lines);
    assert_ne!(at(&vt, 0, 10).as_deref(), Some(uri.as_str()));
}
