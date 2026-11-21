//! Numbered box lines that hold a URL mid-row: quoted arguments, code comments and prose.
//! SonicTerm's `path_target_hardwrap_markdown_quoted_tests.rs` (origin/main 3284d2e9) on
//! Apollo's grid. Left out: `box_in_a_right_hand_pane_ends_at_the_pane_margin`, a multiplexer's
//! pane split inside one grid (Apollo panes are separate terminals, so the box rule's pane is the
//! whole grid). `box_decision_reads_a_bounded_window` counts rows read instead of SonicTerm's
//! cells read, since Apollo reads whole rows.

use super::hardwrap_box_tests::{assert_joined, box_line, decide_in_frame, gutter, joined_url, reads, target_at, url_spans, with_next};
use super::hardwrap_tests::{at, hard, spans};
use super::{Joined, MAX_LINK_BYTES, MAX_LINK_ROWS, numbered};

/// A long URL in a quoted command argument, as Claude Code's diff view shows it.
const QUOTED_URI: &str = "https://github.com/iamcco/markdown-preview.nvim/releases/download/v$version/markdown-preview-win.zip";

/// The row and column of the first `https` in `lines`.
fn url_start(lines: &[String]) -> (usize, usize) {
    lines.iter().enumerate().find_map(|(index, line)| Some((index, line.find("https")?))).unwrap()
}

/// The screenshot: a context row whose quoted URL the box cuts mid-word, among removed and added
/// comment rows cut at the same width. Every fragment resolves the whole URL, without the quotes,
/// the gutter or the signs the comment rows repeat.
#[test]
fn quoted_url_on_context_row_joins_at_the_box_cut() {
    const CUT: usize = 79;
    const HANG: usize = 8;
    let mut lines = Vec::new();
    lines.extend(box_line(&gutter(85, ' '), ' ', "          try {", CUT, HANG));
    lines.extend(box_line(&gutter(86, ' '), ' ', "            $zip = Join-Path $temp 'preview.zip'", CUT, HANG));
    let url_row = lines.len();
    let source = format!("            Invoke-WebRequest -UseBasicParsing \"{QUOTED_URI}\" -OutFile $zip");
    lines.extend(box_line(&gutter(87, ' '), ' ', &source, CUT, HANG));
    let removed = "            # .NET, not Expand-Archive: started from pwsh, Windows PowerShell can load pwsh's incompatible Archive module.";
    lines.extend(box_line(&gutter(88, '-'), '-', removed, CUT, HANG));
    let added = "            # Unzip with .NET: started from pwsh, Windows PowerShell can load pwsh's incompatible Archive module.";
    lines.extend(box_line(&gutter(88, '+'), '+', added, CUT, HANG));

    let start = lines[url_row].find("https").unwrap();
    assert_eq!(lines[url_row].len(), CUT, "the URL row must reach the cut");
    let row = url_row as u16;
    let spans = url_spans(row, start, QUOTED_URI.len(), CUT, HANG);
    let (end_row, _, end_col) = *spans.last().unwrap();
    assert_joined(&lines, CUT as u16 + 6, &spans, QUOTED_URI);
    // The closing quote, the gutter and the opening quote are not part of the destination.
    let vt = hard(CUT as u16 + 6, 16, &lines);
    for (row, col) in [(end_row, end_col), (row + 1, 7), (row, start as u16 - 1)] {
        assert_eq!(at(&vt, row, col), None, "row {row} col {col}");
    }
}

/// On an added or removed row each continuation repeats the sign, which is gutter, not URL text;
/// single and double quotes both end the URL.
#[test]
fn quoted_url_on_signed_row_skips_the_repeated_sign() {
    for quote in ['"', '\''] {
        for sign in ['+', '-'] {
            let source = format!("  get {quote}{QUOTED_URI}{quote} -o out");
            let lines = box_line(&gutter(87, sign), sign, &source, 70, 8);
            let start = lines[0].find("https").unwrap();
            let spans = url_spans(0, start, QUOTED_URI.len(), 70, 8);
            assert_joined(&lines, 80, &spans, QUOTED_URI);
        }
    }
}

/// A bare URL in a code comment that the box cuts several times joins across every row, and the
/// prose after it on the last row is not part of it.
#[test]
fn bare_url_in_comment_joins_across_rows() {
    let uri = "https://example.com/docs/aaaaaaaaaa/bbbbbbbbbb/cccccccccc/dddddddddd/eeeeeeeeee";
    let lines = box_line(&gutter(12, ' '), ' ', &format!("  // see {uri} for details"), 40, 8);
    let start = lines[0].find("https").unwrap();
    let spans = url_spans(0, start, uri.len(), 40, 8);
    assert!(spans.len() >= 3, "fixture must wrap the URL over at least three rows");
    assert_joined(&lines, 60, &spans, uri);
}

/// A URL that starts on a continuation row of a long prose line, and wraps into the next one, is
/// found in the rebuilt line like any other.
#[test]
fn url_starting_on_a_continuation_row_joins() {
    let uri = "https://example.com/guide/aaaaaaaaaaaaaaaaaaaa";
    let lines = box_line(&gutter(3, ' '), ' ', &format!("  This long line wraps well before the link at {uri}."), 40, 8);
    let (url_row, start) = url_start(&lines);
    assert!(url_row > 0, "fixture must start the URL on a continuation row");
    let spans = url_spans(url_row as u16, start, uri.len(), 40, 8);
    assert_joined(&lines, 60, &spans, uri);
}

/// A closing quote alone on the continuation row ends a URL that filled the row above exactly.
#[test]
fn closer_alone_on_continuation_keeps_the_row_url() {
    let uri = "https://example.com/aaaa/bbbb";
    let head = gutter(7, ' ');
    let cut = head.len() + "  get \"".len() + uri.len();
    let lines = box_line(&head, ' ', &format!("  get \"{uri}\" -o out"), cut, 8);
    assert!(lines[1].trim_start().starts_with('"'), "fixture must put the closer on the next row");
    let vt = hard(60, 10, &with_next(&lines));
    assert_eq!(at(&vt, 0, lines[0].find("https").unwrap() as u16 + 3).as_deref(), Some(uri));
}

/// A URL that ends at the cut keeps its own row when the next row is a new numbered line, a blank
/// row, or a continuation that starts right of the box's hang column, whose source began with a
/// blank.
#[test]
fn box_url_refusals_keep_the_row_url() {
    let prefix = "https://example.com/aaaa/bbbb";
    let first = format!("{}  get \"{prefix}", gutter(7, ' '));
    let cut = first.len();
    let cases: [(&str, String); 3] = [
        ("next line", format!("{}  cccc\" -o out", gutter(8, ' '))),
        ("blank row", String::new()),
        ("blank at the hang", "         cccc\" -o out".to_string()),
    ];
    for (label, second) in cases {
        // A later line reaches the same cut, so the first row's end is the box's cut.
        let lines = [first.clone(), second, format!("{}  {}", gutter(9, ' '), "x".repeat(cut - 10)), format!("        {}", "y".repeat(10))];
        let vt = hard(60, 10, &lines);
        assert_eq!(at(&vt, 0, first.find("https").unwrap() as u16 + 3).as_deref(), Some(prefix), "{label}");
    }
}

/// Rows captured from Claude Code's Update view in a 168-column pane: the box cuts at column 161
/// and hangs every continuation at the content column, two columns after the line number. The
/// same geometry with the cut moved inside the URL joins it, and `$zip` stays one word.
#[test]
fn captured_update_view_rows_join_at_their_cut() {
    let invoke = "      4      Invoke-WebRequest -UseBasicParsing \"https://github.com/iamcco/markdown-preview.nvim/releases/download/v$version/markdown-preview-win.zip\" -OutFile $";
    let captured = [
        "      2    try {".to_string(),
        "      3      $zip = Join-Path $temp \"preview.zip\"".to_string(),
        invoke.to_string(),
        "         zip".to_string(),
        "      5 -    # first comment line that is long enough to wrap inside the diff box of the terminal user interface for sure".to_string(),
        "      5 +    # second comment line that is long enough to wrap inside the diff box of the terminal user interface for sure".to_string(),
        "      6    }".to_string(),
        "      7  end".to_string(),
    ];
    assert_eq!(invoke.len(), 161, "the captured row reaches the box's cut");
    let vt = hard(168, 12, &captured);
    assert_eq!(at(&vt, 2, invoke.find("https").unwrap() as u16 + 4).as_deref(), Some(QUOTED_URI));

    // The same box with the cut inside the URL: the URL row reaches column 161, the rest hangs at 9.
    let head = "      4      Invoke-WebRequest -UseBasicParsing \"";
    let source = format!("{}{QUOTED_URI}\" -OutFile $zip", &head[7..]);
    let lines = box_line(&head[..7], ' ', &source, 161 - QUOTED_URI.len() + 40, 9);
    let start = lines[0].find("https").unwrap();
    let spans = url_spans(0, start, QUOTED_URI.len(), lines[0].len(), 9);
    assert_joined(&lines, 168, &spans, QUOTED_URI);
}

/// A URL the box broke across rows opens whole or not at all: a continuation that opens its own
/// scheme, an unsafe cell anywhere in the token (a combining mark or an OSC 8 link, also after
/// trimmed punctuation or on a non-ASCII cell) leaves every fragment inert instead of opening the
/// first row's prefix.
#[test]
fn broken_url_with_unreadable_fragment_never_opens_a_prefix() {
    let first = format!("{}  get \"https://example.test/aaaa", gutter(1, ' '));
    let cases = [
        ("second scheme", "        https://b.test/x\"".to_string()),
        ("combining on the first fragment cell", "        b\u{301}bbb\"".to_string()),
        ("combining after trimmed punctuation", "        bbbb)b\u{301}ccc\"".to_string()),
        ("combining on non-ASCII", "        bbbbé\u{301}\"".to_string()),
        ("hyperlinked fragment", "        \x1b]8;;https://declared.test/\x1b\\bbbb\x1b]8;;\x1b\\\"".to_string()),
    ];
    for (label, second) in cases {
        let lines = [first.clone(), second];
        for (row, col) in [(0, first.len() as u16 - 3), (1, 10)] {
            let display = target_at(&lines, 60, row, col);
            assert!(display.as_deref().is_none_or(|d| !d.starts_with("https://example.test")), "{label} row {row}: {display:?}");
        }
    }
    // The same rows without the unreadable cell join, so each refusal comes from that cell.
    let plain = [first.clone(), "        bbbb)bccc\"".to_string()];
    assert_eq!(target_at(&plain, 60, 0, first.len() as u16 - 3).as_deref(), Some("https://example.test/aaaabbbb)bccc"));
}

/// A URL one byte past `MAX_LINK_BYTES` never opens its first rows' prefix.
#[test]
fn broken_url_past_the_byte_cap_stays_inert() {
    let uri = format!("https://example.test/{}", "a".repeat(MAX_LINK_BYTES + 1 - 21));
    assert_eq!(uri.len(), MAX_LINK_BYTES + 1);
    let lines = box_line(&gutter(1, ' '), ' ', &format!("  \"{uri}\" x"), 150, 8);
    assert_eq!(target_at(&lines, 160, 0, 20), None);
    assert_eq!(target_at(&lines, 160, 1, 20), None);
}

/// A leading `-` or `+` on a context line's continuation is URL text, not a diff sign.
#[test]
fn context_continuation_opening_with_a_hyphen_joins() {
    let first = format!("{}  get \"https://example.test/markdown", gutter(87, ' '));
    let lines = [first.clone(), "        -preview.zip\"".to_string()];
    assert_eq!(target_at(&lines, 60, 0, first.len() as u16 - 3).as_deref(), Some("https://example.test/markdown-preview.zip"));
}

/// A continuation whose text starts with digits and a blank is not a new numbered line: the URL
/// it starts still joins the next continuation.
#[test]
fn continuation_starting_with_digits_keeps_its_head() {
    let lines = [format!("  1 {}", "x".repeat(28)), "    123 \"https://a.test/aaaaaaaa".to_string(), "    bbbb\"".to_string()];
    assert_eq!(target_at(&lines, 40, 1, 12).as_deref(), Some("https://a.test/aaaaaaaabbbb"));
}

/// The row cap bounds the joined URL, not the source line: a two-row URL that starts deep in a
/// long wrapped line still joins.
#[test]
fn two_row_url_deep_in_a_long_line_joins() {
    let uri = "https://example.test/aaaaaaaaaabbbbbbbbbb";
    let prose = "word ".repeat(31 * 7);
    let lines = box_line(&gutter(1, ' '), ' ', &format!("  {prose}\"{uri}\" x"), 40, 8);
    let (row, start) = url_start(&lines);
    assert!(row >= MAX_LINK_ROWS - 1, "fixture must start the URL deep in the line");
    let spans = url_spans(row as u16, start, uri.len(), 40, 8);
    assert!(spans.len() == 2, "fixture must split the URL over two rows: {spans:?}");
    assert_joined(&lines, 60, &spans, uri);
}

/// A closing quote inside the frame ends a URL even when the view is scrolled back and the line's
/// next row is not displayed.
#[test]
fn quoted_url_closed_above_an_undisplayed_row_joins() {
    let lines = [
        format!("{}x", gutter(1, '-')),
        format!("{}y", gutter(1, '+')),
        format!("{}  get \"https://example.test/aaaa", gutter(2, ' ')),
        "        bbbb\"".to_string(),
        "        more".to_string(),
    ];
    // Five rows into a four-row terminal: the frame shows screen rows 0 to 3, the fifth is below it.
    let decided = decide_in_frame(&lines, (60, 4), 0, (2, 20));
    assert_eq!(joined_url(&decided).as_deref(), Some("https://example.test/aaaabbbb"), "closed URL: {decided:?}");
}

/// Hovering a cell of a large box stays bounded: each decision reads each displayed row at most
/// once, whatever the number of rows the box holds.
#[test]
fn box_decision_reads_a_bounded_window() {
    let lines = (0..200).map(|index| format!("{} {}", gutter(index % 90 + 10, ' '), "z".repeat(60))).collect::<Vec<_>>();
    let vt = hard(80, 200, &lines);
    let (_, mut read) = reads(|| numbered(&vt, 100, 20, 0));
    let count = read.len();
    read.sort_unstable();
    read.dedup();
    assert_eq!(read.len(), count, "a row was read twice");
    assert!(count <= 200 && read.iter().all(|y| *y < 200), "read {count} rows");
}

/// Hovering a delimiter that opens a numbered row's text resolves nothing and never panics.
#[test]
fn delimiter_at_the_text_start_resolves_nothing() {
    for line in ["  1 \"hello\"", "  1 {", "  1 (x)"] {
        assert_eq!(target_at(&[line.to_string()], 60, 0, 4), None, "{line:?}");
    }
}

/// A URL that fits both limits joins even when it starts past the rows `MAX_LINK_BYTES` of prose
/// would fill: the rebuild reads the line around the pointer, not from its head.
#[test]
fn url_after_long_prose_on_a_wide_row_joins() {
    let uri = format!("https://example.test/{}", "a".repeat(980));
    let lines = box_line("  1 ", ' ', &format!("{}\"{uri}\"", "word ".repeat(3200)), 1000, 4);
    let (row, start) = url_start(&lines);
    let spans = url_spans(row as u16, start, uri.len(), 1000, 4);
    assert!(spans.len() >= 2, "fixture must wrap the URL: {spans:?}");
    assert_joined(&lines, 1010, &spans, &uri);
}

/// A two-row URL whose numbered head is far above it, but still displayed, joins.
#[test]
fn two_row_url_far_below_its_head_joins() {
    let uri = "https://example.test/aaaaaaaaabbbbbbbbcccccccc";
    let lines = box_line("  1 ", ' ', &format!("{} \"{uri}\"", "x".repeat(2276)), 40, 4);
    let (row, start) = url_start(&lines);
    assert!(row > 2 * MAX_LINK_ROWS - 2, "fixture must put the URL far below its head");
    let spans = url_spans(row as u16, start, uri.len(), 40, 4);
    assert_eq!(spans.len(), 2, "fixture must split the URL over two rows");
    assert_joined(&lines, 60, &spans, uri);
}

/// When the head is above the displayed frame, a URL whose rows the frame shows still joins, and
/// one that may start above the frame stays inert.
#[test]
fn url_in_a_line_whose_head_scrolled_away() {
    let uri = "https://example.test/aaaaaaaaabbbbbbbbcccccccc";
    let mut lines = box_line("  1 ", ' ', &format!("{} \"{uri}\" x", "x".repeat(400)), 40, 4);
    lines.push(format!("  2 {}", "y".repeat(10)));
    lines.push(format!("  3 {}", "y".repeat(10)));
    let (row, start) = url_start(&lines);
    // The frame shows the URL's rows and the next two numbered rows, not the line's head.
    let view_top = lines.len() as u32 - 6;
    assert!(view_top > 0 && view_top < row as u32);
    let decided = decide_in_frame(&lines, (60, 6), view_top, (row as u32, start as u16 + 2));
    assert_eq!(joined_url(&decided).as_deref(), Some(uri), "headless line: {decided:?}");
    // On the frame's top row the prose may continue above, so the token there is not proven.
    let top = decide_in_frame(&lines, (60, 6), view_top, (view_top, 6));
    assert!(!matches!(top, Joined::Whole(_)), "{top:?}");
}

/// A continuation that holds only the repeated sign ends nothing: the URL closed above it opens.
#[test]
fn sign_only_continuation_keeps_the_closed_url() {
    let lines = ["  1 +get \"https://example.test/aaaa".to_string(), "    +bbbb\" more".to_string(), "    +".to_string()];
    assert_eq!(target_at(&lines, 60, 0, 20).as_deref(), Some("https://example.test/aaaabbbb"));
}

/// A blank cell carrying a combining mark is unreadable text, not padding, and a continuation's
/// sign carrying one is unreadable too: either leaves the broken URL inert instead of opening a
/// joined or truncated destination.
#[test]
fn unreadable_blank_or_sign_keeps_the_url_inert() {
    let cases = [
        ("combining blank", ["  1 get \"https://example.test/aaaa \u{301}", "    bbbb\""]),
        ("combining sign", ["  1 +get \"https://example.test/aaaa", "    +\u{301}bbbb\""]),
    ];
    for (label, lines) in cases {
        assert_eq!(target_at(&lines.map(str::to_string), 60, 0, 20), None, "{label}");
    }
}

/// On a context line a `+` or `-` at the text column is source text: the URL keeps it, and the
/// next row, which starts one column right of the text column, began with a blank, so its text is
/// never joined.
#[test]
fn context_continuation_with_a_sign_at_the_text_column_keeps_it_as_text() {
    for sign in ['+', '-'] {
        let lines = [
            format!("{}  get \"https://example.test/aaaa", gutter(1, ' ')),
            format!("        {sign}{}", "b".repeat(29)),
            "         cccc\"".to_string(),
            format!("{}x", gutter(2, '-')),
            format!("{}y", gutter(2, '+')),
        ];
        let decided = decide_in_frame(&lines, (60, 6), 0, (0, 20));
        let expected = format!("https://example.test/aaaa{sign}{}", "b".repeat(29));
        assert_eq!(joined_url(&decided), Some(expected), "{sign}: {decided:?}");
    }
}

/// A line whose numbered head is displayed joins its URL. When the head scrolls above the frame
/// and no numbered row below shows the box, the rows cannot be told from other indented output,
/// so the rule leaves them to the single-row rules: a documented limit of reading the box.
#[test]
fn line_whose_head_scrolled_away_with_no_numbered_row_below_is_not_read() {
    let source = format!("{} \"https://example.test/aaaabbbb\" {}", "x".repeat(40), "word ".repeat(20));
    // `  1 ` ends its gutter at column 3 and the text opens with `x`, so the box hangs at column 4.
    let lines = box_line("  1 ", ' ', &source, 32, 4);
    let row_count = lines.len() as u16;
    let whole = decide_in_frame(&with_next(&lines), (60, row_count + 1), 0, (1, 20));
    assert_eq!(joined_url(&whole).as_deref(), Some("https://example.test/aaaabbbb"), "{whole:?}");
    let scrolled = decide_in_frame(&lines, (60, row_count - 1), 1, (1, 20));
    assert_eq!(scrolled, Joined::None);
}

/// A markdown box decides its layout from every row: one line opening with spaces is still a
/// markdown line, so its continuation one column right of the text column began with a blank and
/// the URL above it never runs into that text.
#[test]
fn indented_markdown_line_keeps_its_url_whole() {
    let lines = [
        "  11 Documentation links:".to_string(),
        "  12   see https://example.test/aaaa".to_string(),
        "      next paragraph".to_string(),
        "  13 More text".to_string(),
    ];
    assert_eq!(target_at(&lines, 70, 1, 25).as_deref(), Some("https://example.test/aaaa"));
}

/// A continuation that starts with three digits, one column further in, is no head even when the
/// row above it scrolled away: the URL it starts never opens a prefix.
#[test]
fn deeper_digit_continuation_at_the_frame_top_is_no_false_head() {
    let lines = [format!("  1 {}", "x".repeat(32)), "      123 \"https://example.test/aaaa".to_string(), "    bbbb\"".to_string(), "  2 next".to_string()];
    let full = decide_in_frame(&lines, (70, 4), 0, (1, 20));
    assert_eq!(joined_url(&full).as_deref(), Some("https://example.test/aaaabbbb"), "{full:?}");
    let scrolled = decide_in_frame(&lines, (70, 3), 1, (1, 20));
    assert_eq!(scrolled, Joined::Unproven);
}

/// Equal-width URLs on consecutive tool-output rows are separate links; the box rule never makes
/// them inert.
#[test]
fn aligned_tool_output_urls_stay_clickable() {
    let lines = ["  ⎿  https://example.test/aaaa".to_string(), "     https://example.test/bbbb".to_string(), "     https://example.test/cccc".to_string()];
    for (row, expected) in [(0, "aaaa"), (1, "bbbb"), (2, "cccc")] {
        assert_eq!(target_at(&lines, 70, row, 15), Some(format!("https://example.test/{expected}")), "row {row}");
    }
}

/// A continuation that starts with digits at the frame's top is not taken for a head when a row
/// below it shows the box's narrower gutter: its URL never opens a prefix.
#[test]
fn digit_continuation_at_the_frame_top_is_no_false_head() {
    let lines = [format!("  1 {}", "x".repeat(30)), "    123 \"https://example.test/aaaa".to_string(), "    bbbb\"".to_string(), "  2 next".to_string()];
    let full = decide_in_frame(&lines, (60, 4), 0, (1, 20));
    assert_eq!(joined_url(&full).as_deref(), Some("https://example.test/aaaabbbb"), "{full:?}");
    // The scrolled view cannot tell which line the row belongs to, so nothing opens.
    let scrolled = decide_in_frame(&lines, (60, 3), 1, (1, 20));
    assert_eq!(scrolled, Joined::Unproven);
}

/// A `+` that opens every visible continuation of a markdown box is text, not a diff sign: the
/// destination keeps it whether or not the line's head is displayed.
#[test]
fn headless_markdown_line_keeps_a_leading_plus() {
    let lines = [
        format!("  1 {}", "x".repeat(27)),
        "    +\"https://example.test/aaaa".to_string(),
        "    +bbbb\"".to_string(),
        "  2 next".to_string(),
        "  3 last".to_string(),
    ];
    for view_top in [0u32, 1] {
        let rows = if view_top == 0 { 5 } else { 4 };
        let decided = decide_in_frame(&lines, (60, rows), view_top, (1, 20));
        assert_eq!(joined_url(&decided).as_deref(), Some("https://example.test/aaaa+bbbb"), "view_top {view_top}: {decided:?}");
    }
}

/// The byte bound counts URL text, not a signed line's repeated sign: a 4096-byte URL that spans
/// 32 rows of a signed line still joins.
#[test]
fn signed_line_byte_bound_counts_url_text_only() {
    let uri = format!("https://example.test/{}", "a".repeat(MAX_LINK_BYTES - 21));
    assert_eq!(uri.len(), MAX_LINK_BYTES);
    let source = format!("{}\"{uri}\"", " ".repeat(122));
    let lines = with_next(&box_line(&gutter(1, '+'), '+', &source, 143, 8));
    let (row, start) = url_start(&lines);
    let spans = url_spans(row as u16, start, uri.len(), 143, 8);
    assert!(spans.len() <= MAX_LINK_ROWS, "fixture fits the row bound");
    let decided = decide_in_frame(&lines, (160, 40), 0, (row as u32, start as u16 + 2));
    assert_eq!(joined_url(&decided), Some(uri), "signed line: {decided:?}");
}

/// Markdown evidence anywhere in the displayed box decides its layout, however far above the
/// pointer: an indented line's continuation one column right of the text column began with a
/// blank, so the URL never runs into it.
#[test]
fn markdown_evidence_far_above_the_pointer_decides_the_layout() {
    let mut lines = vec!["   1 Title".to_string()];
    lines.extend((2..=32).map(|number| format!("  {number:>2}   indented code")));
    lines.push("  33   see https://example.test/aaaa".to_string());
    lines.push("      next paragraph".to_string());
    assert_eq!(target_at(&lines, 70, 32, 23).as_deref(), Some("https://example.test/aaaa"));
}

/// When the head scrolled away but a numbered row below shows a markdown box, the visible rows'
/// differing first characters are text, not signs, and the wrapped URL joins.
#[test]
fn headless_markdown_line_with_a_plus_row_joins() {
    let lines = [
        "  12 xxxxxxxxxxxxxxxxxxxxxxxxxx".to_string(),
        "     +yyyyyyyyyyyyyyyyyyyyyyyyy".to_string(),
        "     \"https://example.test/aaaa".to_string(),
        "     bbbb\"".to_string(),
        "  13 next".to_string(),
        "  14 last".to_string(),
    ];
    let decided = decide_in_frame(&lines, (70, 5), 1, (2, 15));
    assert_eq!(joined_url(&decided).as_deref(), Some("https://example.test/aaaabbbb"), "{decided:?}");
}

/// The box hands the app exactly the URL its quotes end, so a final `!` or `.` the scanner would
/// trim as sentence punctuation, or a balanced `)`, stays: every URL cell on every row opens the
/// whole destination with every span, and the quotes and the gutter open nothing.
#[test]
fn quoted_box_url_opens_exactly_as_its_quotes_end_it() {
    let uris = [
        "https://example.test/aaaaaaaaaa/bbbbbbbbbb/cccccccccc/dddd!",
        "https://example.test/aaaaaaaaaa/bbbbbbbbbb/cccccccccc/dddd.",
        "https://en.wikipedia.org/wiki/Function_(mathematics)",
    ];
    for uri in uris {
        let lines = box_line(&gutter(87, ' '), ' ', &format!("  get \"{uri}\" -o out"), 40, 8);
        let start = lines[0].find("https").unwrap();
        let spans = url_spans(0, start, uri.len(), 40, 8);
        let (end_row, _, end_col) = *spans.last().unwrap();
        assert!(spans.len() >= 2 && end_col < 40, "fixture must wrap the URL and keep its closer on its last row: {spans:?}");
        assert_joined(&lines, 60, &spans, uri);
        let vt = hard(60, 20, &with_next(&lines));
        for (row, col) in [(end_row, end_col), (0, start as u16 - 1), (1, 7), (1, 0)] {
            assert_eq!(at(&vt, row, col), None, "{uri}: row {row} col {col}");
        }
    }
}

/// QA finding 1: the pointer on an unsafe cell (here a combining mark) inside a box URL the rows
/// break opens nothing, never the prefix before that cell.
#[test]
fn pointer_on_an_unsafe_cell_of_a_broken_url_opens_nothing() {
    let lines = [
        "      1 - get \"https://example.test/old\"".to_string(),
        "      1 + get \"https://example.test/aaax\u{301}".to_string(),
        "        +bbbb\"".to_string(),
    ];
    let vt = hard(60, 6, &lines);
    assert_eq!(at(&vt, 1, 39), None);
}

/// QA finding 2: an unsafe cell is unknown text, never a blank, so a URL right after one on a
/// continuation may be the rest of the URL above it: it opens nothing.
#[test]
fn url_after_an_unsafe_cell_on_a_continuation_opens_nothing() {
    let lines = [
        "      1 - get \"https://example.test/old\"".to_string(),
        "      1 + get \"https://example.test/aaaa".to_string(),
        "        +x\u{301}(https://evil.test/x)\"".to_string(),
    ];
    let vt = hard(60, 6, &lines);
    assert_eq!(at(&vt, 2, 13), None);
}

/// QA finding 3: outside a proven box a URL one row holds is the single-row rules', exactly as
/// before the box rule; inside a proven box the URL its quotes fix opens exactly, `!` included.
#[test]
fn one_row_url_outside_a_proven_box_reads_as_before() {
    for line in ["  1 get \"https://example.test/x!\"", "see \"https://example.test/x!\" ok"] {
        let vt = hard(60, 6, &[line.to_string()]);
        let col = line.find("https").unwrap() as u16 + 2;
        assert_eq!(at(&vt, 0, col).as_deref(), Some("https://example.test/x"), "{line:?}");
    }
    let lines = ["      1 - get \"https://example.test/old\"".to_string(), "      1 + get \"https://example.test/x!\" ok".to_string()];
    let vt = hard(60, 6, &lines);
    assert_eq!(at(&vt, 1, 20).as_deref(), Some("https://example.test/x!"));
    assert_eq!(spans(&vt, 1, 20), vec![(1, 15, 37)]);
}

/// QA finding 3, the rebuilt-line path: in a box the rows do not prove (one line number), a
/// destination whose `](` ends the row above sits whole on its own row; it opens what the
/// single-row rules open for that row, never the box's exact reading.
#[test]
fn one_row_destination_in_an_unproven_box_reads_as_before() {
    let second = "        https://example.test/x!)".to_string();
    let lines = ["      1 see the documentation for this at [docs](".to_string(), second.clone()];
    let alone = at(&hard(60, 6, &[second]), 0, 12);
    assert!(alone.is_some(), "the row alone opens a link");
    assert_eq!(at(&hard(60, 6, &lines), 1, 12), alone);
}
