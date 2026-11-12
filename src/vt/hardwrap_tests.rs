//! URLs a program broke across rows, joined: SonicTerm's hard-wrap cases
//! (`path_target_hardwrap_uri_tests.rs`, `path_target_hardwrap_markdown_tests.rs`
//! at 68250eef) on Apollo's grid. Cases that test SonicTerm-only parts (its
//! pane borders, preview state, settings, frame observer) are left out.

use super::super::Vt;
use super::{MAX_LINK_BYTES, MAX_LINK_ROWS};

const ISSUE_URI: &str = "https://github.com/D0n9X1n/SonicTerm/issues/1349";
const HARDWRAP_HEAD: &str = "* Done: #1349 (";
const LONG_URI: &str = "https://example.com/aaaaaaaaaa/bbbbbbbbbb/cccccccccc/dddddddddd";
const DOC_URI: &str = "https://github.com/D0n9X1n/bufferlines.nvim/wiki/Installation-and-Usage-zh-CN";

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

/// Display cells `s` takes: CJK and fullwidth punctuation take two.
fn width(s: &str) -> usize {
    s.chars().map(|c| if ('\u{1100}'..).contains(&c) && !('\u{300}'..='\u{36f}').contains(&c) { 2 } else { 1 }).sum()
}

/// `lines` padded with blanks to `content` cells, as a box draws them.
fn padded(lines: &[&str], content: usize) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            assert!(width(line) <= content, "line wider than the box: {line}");
            format!("{line}{}", " ".repeat(content - width(line)))
        })
        .collect()
}

/// A `cols` × `rows` terminal a program wrote `lines` to as hard rows.
fn hard(cols: u16, rows: u16, lines: &[String]) -> Vt {
    let mut vt = Vt::new(cols, rows);
    vt.write(lines.join("\r\n").as_bytes());
    vt
}

/// What the app links at a cell, as `links::pick` decides from no folder.
fn at(vt: &Vt, row: u16, col: u16) -> Option<String> {
    let ctx = crate::links::Ctx::new(None);
    vt.link_at(col, row, |text, at, joined| crate::links::pick(text, at, joined, &ctx, |_| None).map(|(r, _)| r)).map(|hit| hit.target)
}

fn spans(vt: &Vt, row: u16, col: u16) -> Vec<(u16, u16, u16)> {
    let ctx = crate::links::Ctx::new(None);
    vt.link_at(col, row, |text, at, joined| crate::links::pick(text, at, joined, &ctx, |_| None).map(|(r, _)| r)).map_or(Vec::new(), |hit| hit.spans.to_vec())
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
    let vt = hard(30, 10, &["x (https://example.com/aaaaaaaaa".into(), "  bbbb)".into()]);
    assert_eq!(at(&vt, 1, 3).as_deref(), Some("https://example.com/aaaaaaaaabbbb"), "the plain fixture joins");
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

// ---- Claude Code's padded, numbered box -------------------------------------

#[test]
fn padded_markdown_target_joins_across_hanging_indent() {
    let cut = DOC_URI.find("Usag").unwrap() + "Usag".len();
    let first = format!("    60 [安装]({}", &DOC_URI[..cut]);
    let first_cols = width(&first) as u16;
    let tail_len = (DOC_URI.len() - cut) as u16;
    // `    60 [安装](`: seven gutter cells, `[`, two wide cells, `]`, `(`.
    let start = 14u16;
    for tail in [")。", ") 。", ")", ")。其他"] {
        let second = format!("       {}{tail}", &DOC_URI[cut..]);
        let vt = hard(100, 10, &padded(&[&first, &second], first_cols as usize + 8));
        let pointers = (start..first_cols).map(|c| (0, c)).chain((7..7 + tail_len).map(|c| (1, c)));
        for (row, col) in pointers {
            assert_eq!(at(&vt, row, col).as_deref(), Some(DOC_URI), "tail {tail:?} row {row} col {col}");
            assert_eq!(spans(&vt, row, col), vec![(0, start, first_cols - 1), (1, 7, 7 + tail_len - 1)], "tail {tail:?}");
        }
        assert_eq!(at(&vt, 1, 7 + tail_len), None, "the closer isn't the link's: tail {tail:?}");
    }
}

#[test]
fn padded_markdown_issue_shapes_join() {
    // The issue's shapes: a numbered row, and a `+` diff row with prose before the link.
    let cut = DOC_URI.find("Usag").unwrap() + 4;
    let vt = hard(110, 10, &padded(&[&format!("    60 [安装与使用]({}", &DOC_URI[..cut]), &format!("       {})。", &DOC_URI[cut..])], 95));
    assert_eq!(at(&vt, 0, 30).as_deref(), Some(DOC_URI));
    assert_eq!(at(&vt, 1, 9).as_deref(), Some(DOC_URI));
    let nf = "https://github.com/ryanoasis/nerd-fonts/wiki";
    let cut = nf.find("nerd-fon").unwrap() + 8;
    let vt = hard(90, 10, &padded(&[&format!("    59 +… [Nerd Font]({}", &nf[..cut]), &format!("       +{})。…", &nf[cut..])], 70));
    assert_eq!(at(&vt, 0, 30).as_deref(), Some(nf));
    assert_eq!(at(&vt, 1, 9).as_deref(), Some(nf));
}

#[test]
fn padded_markdown_target_joins_three_rows() {
    let uri = "https://example.com/aaaaaaaaaa/bbbbbbbbbb/cccccccccc/dddddddddd/eeeeeeeeee";
    let first = format!("  1 [doc]({}", &uri[..30]);
    let middle_len = first.len() - 4;
    let middle = format!("    {}", &uri[30..30 + middle_len]);
    let last = format!("    {}).", &uri[30 + middle_len..]);
    let vt = hard(90, 10, &padded(&[&first, &middle, &last], first.len() + 6));
    for (row, col) in [(0, 12), (1, 4), (2, 5)] {
        assert_eq!(at(&vt, row, col).as_deref(), Some(uri), "row {row}");
        assert_eq!(spans(&vt, row, col).len(), 3, "row {row}");
    }
}

#[test]
fn padded_markdown_target_skips_repeated_diff_sign() {
    let uri = "https://github.com/ryanoasis/nerd-fonts/wiki";
    let first = format!("    59 +see [Nerd Font]({}", &uri[..38]);
    let second = format!("       +{}) and more", &uri[38..]);
    let vt = hard(90, 10, &padded(&[&first, &second], first.len() + 7));
    for (row, col) in [(0, 30), (1, 9)] {
        assert_eq!(at(&vt, row, col).as_deref(), Some(uri), "row {row}");
    }
    assert_eq!(at(&vt, 1, 7), None, "the repeated sign is gutter");
    // The whole URL fits on the first row and the next holds only the closer.
    let whole = "https://github.com/ryanoasis/nerd-fonts";
    let first = format!("    59 +see [Nerd Font]({whole}");
    let vt = hard(90, 10, &padded(&[&first, "       +)。其他插件"], first.len() + 7));
    assert_eq!(at(&vt, 0, 30).as_deref(), Some(whole));
}

#[test]
fn padded_markdown_refusals_keep_single_row_detection() {
    let prefix = "https://example.com/aaaa/bbbb";
    let cases: [(&str, String, String, &str); 10] = [
        ("plain paren", format!("    1 see ({prefix}"), "      cccc)".into(), prefix),
        ("no gutter", format!("See [x]({prefix}"), "    cccc) next item".into(), prefix),
        ("comma", format!("  1 [a]({prefix},"), "    cccc)".into(), prefix),
        ("gutter row", format!("    60 [a]({prefix}"), "    61)".into(), prefix),
        ("unaligned", format!("  1 [a]({prefix}"), "     cccc)".into(), prefix),
        ("space", format!("  1 [a]({prefix}"), "    cc cc)".into(), prefix),
        ("second scheme", format!("  1 [a]({prefix}"), "    https://b.test/x)".into(), prefix),
        ("other sign", format!("    59 +[a]({prefix}"), "       -cccc)".into(), prefix),
        ("unexpected sign", format!("    59 [a]({prefix}"), "      +cccc)".into(), prefix),
        ("split authority", "  1 [a](https://good.test".into(), "    @evil.test/cccc)".into(), "https://good.test"),
    ];
    for (label, first, second, row_url) in cases {
        let vt = hard(90, 10, &padded(&[&first, &second], first.len() + 6));
        let col = first.find("https").unwrap() as u16 + 2;
        assert_eq!(at(&vt, 0, col).as_deref(), Some(row_url), "{label}");
        assert!(at(&vt, 1, 8).is_none_or(|t| !t.starts_with(row_url)), "{label} continuation");
    }
}

#[test]
fn padded_markdown_refuses_unsafe_opener() {
    let vt = hard(60, 10, &padded(&["  1 [a](\u{301}https://example.com/aaaa/bbbb", "    cccc)"], 40));
    for (row, col) in [(0, 12), (1, 5)] {
        assert!(at(&vt, row, col).is_none_or(|t| !t.contains("cccc")), "row {row}");
    }
}

#[test]
fn padded_markdown_refuses_soft_wrap_after_first_continuation() {
    let first = "  1 [x](https://example.test/aaaaaaaa";
    let middle = format!("    {}   ", "b".repeat(first.len() - 4));
    let mut vt = Vt::new(40, 10);
    vt.write(format!("{first}\r\n{middle}    cc)").as_bytes());
    assert!(at(&vt, 0, 12).is_none_or(|t| !t.contains("bbb")));
}

#[test]
fn padded_markdown_refuses_closer_that_wraps_on() {
    let mut vt = Vt::new(40, 10);
    vt.write(format!("  1 [x](https://example.test/a\r\n    {})tail)", "b".repeat(35)).as_bytes());
    assert!(at(&vt, 0, 12).is_none_or(|t| !t.contains("bbb")));
}

#[test]
fn padded_markdown_never_joins_hyperlinked_fragment() {
    let first = "  1 [x](https://example.test/aaaa/bbbb";
    let vt = hard(60, 10, &[format!("{first}{}\r\n    \x1b]8;;https://declared.test/\x1b\\cccc\x1b]8;;\x1b\\)", " ".repeat(6))]);
    assert_eq!(at(&vt, 1, 5).as_deref(), Some("https://declared.test/"));
    assert!(at(&vt, 0, 12).is_none_or(|t| !t.contains("cccc")));
}

#[test]
fn padded_markdown_structural_cells_must_be_plain() {
    let combining = |c: char| format!("{c}\u{301}");
    let linked = |c: char| format!("\x1b]8;;https://declared.test/\x1b\\{c}\x1b]8;;\x1b\\");
    let mutations: [(&str, &dyn Fn(char) -> String); 2] = [("combining", &combining), ("hyperlink", &linked)];
    for marked in ["first sign", "]", "(", ")", "second sign"] {
        for (label, mutate) in mutations {
            let cell = |pos: &str, c: char| if pos == marked { mutate(c) } else { c.to_string() };
            let first = format!(
                "    59 {}see [Nerd Font{}{}https://github.com/ryanoasis/nerd-fon{}",
                cell("first sign", '+'),
                cell("]", ']'),
                cell("(", '('),
                " ".repeat(7)
            );
            let second = format!("       {}ts/wiki{} and more", cell("second sign", '+'), cell(")", ')'));
            let vt = hard(90, 10, &[first, second]);
            for (row, col) in [(0, 30), (1, 9)] {
                assert!(at(&vt, row, col).is_none_or(|t| !t.contains("nerd-fonts/wiki")), "{label} on {marked} at row {row}");
            }
        }
    }
    let first = format!("    59 +see [Nerd Font](https://github.com/ryanoasis/nerd-fon{}", " ".repeat(7));
    let vt = hard(90, 10, &[first, "       +ts/wiki) and more".into()]);
    assert_eq!(at(&vt, 1, 9).as_deref(), Some("https://github.com/ryanoasis/nerd-fonts/wiki"));
}

#[test]
fn padded_markdown_middle_row_must_end_at_the_cut() {
    let first = "  1 [doc](https://example.com/aaaa/bbbb";
    // Past the first row's cut, or short of it: either way not one target.
    for middle_len in [first.len() - 4 + 3, 4] {
        let middle = format!("    {}", "c".repeat(middle_len));
        let vt = hard(90, 10, &padded(&[first, &middle, "    more)."], first.len() + 8));
        assert_eq!(at(&vt, 0, 12).as_deref(), Some("https://example.com/aaaa/bbbb"), "middle {middle_len}");
    }
}

/// A chain `rows` long in a padded box `cols` wide, and the URL it holds.
fn padded_chain(cols: usize, middle_rows: usize, last_body: &str) -> (Vec<String>, String) {
    let head = "  1 [x](https://example.test/";
    let first = format!("{head}{}", "a".repeat(cols - 1 - head.len()));
    let middle = format!("    {}", "b".repeat(first.len() - 4));
    let mut uri = first[8..].to_string();
    let mut lines = vec![first];
    for _ in 0..middle_rows {
        uri.push_str(&middle[4..]);
        lines.push(middle.clone());
    }
    uri.push_str(last_body);
    lines.push(format!("    {last_body})"));
    (lines, uri)
}

#[test]
fn padded_markdown_enforces_row_bound() {
    for (total, joins) in [(MAX_LINK_ROWS, true), (MAX_LINK_ROWS + 1, false)] {
        let (lines, uri) = padded_chain(50, total - 2, "zz");
        let lines = padded(&lines.iter().map(String::as_str).collect::<Vec<_>>(), 49);
        let vt = hard(60, 40, &lines);
        let got = at(&vt, 0, 12);
        assert_eq!(got.as_deref() == Some(uri.as_str()), joins, "rows {total}: {got:?}");
        assert!(joins || got.is_none_or(|t| !t.contains("zz")));
    }
}

#[test]
fn padded_markdown_enforces_byte_bound() {
    for (extra, joins) in [(0usize, true), (1, false)] {
        let (probe, _) = padded_chain(140, 29, "");
        let fixed = probe[0].len() - 8 + 29 * (probe[1].len() - 4);
        let (lines, uri) = padded_chain(140, 29, &"z".repeat(MAX_LINK_BYTES - fixed + extra));
        assert_eq!(uri.len(), MAX_LINK_BYTES + extra);
        let lines = padded(&lines.iter().map(String::as_str).collect::<Vec<_>>(), 139);
        let got = at(&hard(150, 40, &lines), 0, 12);
        assert_eq!(got.as_deref() == Some(uri.as_str()), joins, "extra {extra}");
    }
}
