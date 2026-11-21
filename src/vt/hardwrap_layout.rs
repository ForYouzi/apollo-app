//! Which layout a numbered box's rows allow: the signed rows only a diff writes, and the
//! continuations no diff shows. SonicTerm's `path_target_hardwrap_layout.rs` (origin/main 3284d2e9).

use super::{BoxRow, BoxRows, Joined, box_blank};

/// Whether a diff line with `sign`, headed at `head_row` (or above the frame) with gutter end
/// `gutter_end`, is contradicted by its own rows: an unnumbered row from the head down to past
/// `pointed_row` that writes another character where a diff line repeats its sign. A diff shows
/// no such row, so the line is markdown.
pub(super) fn diff_contradicted(rows: &mut BoxRows<'_>, (head_row, gutter_end, sign): (Option<u32>, usize, Option<char>), pointed_row: u32) -> bool {
    let first = head_row.map_or(rows.top, |head| head + 1);
    for row_number in first..rows.end {
        let Some(row) = rows.get(row_number) else { return false };
        if row.soft_wrapped || row.gutter_end.is_some() || !row.blank_through(gutter_end) {
            // The line ended.
            return false;
        }
        let cell = row.cells.get(gutter_end + 1).filter(|cell| !box_blank(cell));
        if cell.is_some_and(|cell| Some(cell.ch) != sign) {
            // The row writes another character where a diff line repeats its sign.
            return true;
        }
        if row_number > pointed_row && cell.is_none() && sign.is_some() {
            // A row past the pointer leaving a signed line's column blank ends it.
            return false;
        }
    }
    false
}

/// Whether the sign `sign` right after a numbered row's gutter end `gutter_end` opens a line only a
/// diff shows. A `+` does unless the row repeats it to the end (`+++`): markdown rarely bullets
/// with `+`, while an added line indented two blanks is common. A `-` does unless the row is a
/// markdown list item (a sign then one or two blanks before text) or a rule (`---`), both common.
pub(super) fn diff_only_sign(row: &BoxRow, gutter_end: usize, sign: char) -> bool {
    if !matches!(sign, '+' | '-') {
        return false;
    }
    let after = |offset: usize| row.cells.get(gutter_end + 1 + offset);
    let blanks = (1..=3).take_while(|offset| after(*offset).is_some_and(box_blank)).count();
    let list_item = sign == '-' && (1..=2).contains(&blanks) && after(blanks + 1).is_some_and(|cell| !box_blank(cell));
    let last = row.text.map_or(gutter_end + 1, |(_, last)| last);
    let rule = (gutter_end + 1..=last).all(|column| row.cells.get(column).is_some_and(|cell| cell.ch == sign));
    !list_item && !rule
}

/// Whether `decided` opens a URL a markdown `](` opens on the row where it starts.
pub(super) fn markdown_target(rows: &mut BoxRows<'_>, decided: &Joined) -> bool {
    let Joined::Whole(parts) = decided else { return false };
    let Some((row, start)) = parts.first().and_then(|(row, cells)| Some((*row, usize::from(cells.first()?.x)))) else { return false };
    let Some(row) = rows.get(row).filter(|_| start >= 2) else { return false };
    row.cells[start - 2].ch == ']' && row.cells[start - 1].ch == '('
}

/// One decision from two readings of a box: the same URL with the same cells, or neither owning
/// the token, stands; any other pair means the box's layout decides the destination, which it
/// does not show, so nothing opens.
pub(super) fn agreed(earlier: Joined, later: Joined) -> Joined {
    match (earlier, later) {
        (Joined::Whole(first), Joined::Whole(second)) if first == second => Joined::Whole(first),
        (Joined::None, Joined::None) => Joined::None,
        // A URL one reading keeps on one row and the other leaves to the single-row rules is a
        // one-row URL either way, so the single-row rules decide it.
        (Joined::Whole(one_row), Joined::None) | (Joined::None, Joined::Whole(one_row)) if one_row.len() == 1 => Joined::None,
        _ => Joined::Unproven,
    }
}
