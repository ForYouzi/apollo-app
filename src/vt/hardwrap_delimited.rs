//! The extent a URL's own delimiters fix inside a rebuilt numbered-box line, and the URLs one row
//! holds whole: SonicTerm's `path_target_hardwrap_delimited.rs` (origin/main 3284d2e9).

use super::url_scan::{self, UrlMatch};
use super::{At, BoxRows, Joined, LineByte, box_blank, cell_line_byte, cont, orphan_wide_cont, url_body_char};

/// What a URL's opening delimiter says about its extent.
pub(super) enum Delimited {
    /// No delimiter opens the URL, so the scanner reads it.
    Absent,
    /// The URL its delimiters fix, checked whole.
    Url(UrlMatch),
    /// A delimiter opens the URL but ends it at another character, or its text is no URL: the
    /// destination is not readable, so no shorter reading may open.
    Unresolved,
}

/// The extent of the URL starting at byte `scheme` of the rebuilt `text`, in the token ending at
/// `token_end`. After `(`, `](` or `[` it runs to the matching `)` or `]`, so a `)` the URL holds
/// stays and markdown after the closer does not join; when the token ends first with the URL's
/// own brackets balanced, the URL ends there. After prose `(` or `[`, trailing `.`, `,`, `;` or `:`
/// is trimmed as the scanner trims it, while a trailing `!` or `?`, which a URL may end with, is
/// `Unresolved`. After a quote, a backtick or `<` it runs to the same quote, backtick or `>`; a
/// token ending at a blank leaves the delimiter wrapping more than the URL, and any other
/// character, or a doubled quote, is `Unresolved`.
pub(super) fn delimited_url(text: &str, scheme: usize, token_end: usize) -> Delimited {
    let bytes = text.as_bytes();
    let Some(&opener) = scheme.checked_sub(1).and_then(|before| bytes.get(before)) else {
        return Delimited::Absent;
    };
    let end = match opener {
        b'(' => bracket_end(bytes, (b'(', b')'), scheme, token_end),
        b'[' => bracket_end(bytes, (b'[', b']'), scheme, token_end),
        b'"' | b'\'' | b'`' => quote_end(bytes, (opener, opener), token_end),
        b'<' => quote_end(bytes, (b'<', b'>'), token_end),
        _ => Err(Delimited::Absent),
    };
    let mut end = match end {
        Ok(end) => end,
        Err(decided) => return decided,
    };
    let markdown = opener == b'(' && scheme >= 2 && bytes[scheme - 2] == b']';
    // Prose brackets: trailing sentence punctuation is not the URL's, as the scanner reads.
    let prose = matches!(opener, b'(' | b'[') && !markdown;
    while prose && end > scheme && matches!(bytes[end - 1], b'.' | b',' | b';' | b':') {
        end -= 1;
    }
    if prose && end > scheme && matches!(bytes[end - 1], b'!' | b'?') {
        // Either the URL or the sentence may own the mark, so its extent is not readable.
        return Delimited::Unresolved;
    }
    let url = text.get(scheme..end).filter(|url| url_scan::is_url(url));
    url.map_or(Delimited::Unresolved, |url| Delimited::Url(UrlMatch { start: scheme, end, url: url.to_string() }))
}

/// The end of a URL an `open` bracket opens: its matching `close`, or the token's end when the
/// token ends first with the URL's own brackets balanced; `Unresolved` when it ends inside one.
fn bracket_end(bytes: &[u8], (open, close): (u8, u8), scheme: usize, token_end: usize) -> Result<usize, Delimited> {
    // Depth counts the URL's own brackets; a `close` at depth 0 closes the opener.
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate().take(token_end).skip(scheme) {
        if *byte == close && depth == 0 {
            return Ok(index);
        }
        depth = depth + usize::from(*byte == open) - usize::from(*byte == close);
    }
    (depth == 0).then_some(token_end).ok_or(Delimited::Unresolved)
}

/// The end of a URL an `open` quote, backtick or `<` opens: the `close` ending the token. A token
/// ending at a blank or the line's end leaves the delimiter wrapping more than the URL, so the
/// scanner reads it; another character, such as an apostrophe the URL holds, or a doubled quote
/// that escapes one, leaves it `Unresolved`.
fn quote_end(bytes: &[u8], (open, close): (u8, u8), token_end: usize) -> Result<usize, Delimited> {
    let doubled = open == close && bytes.get(token_end + 1) == Some(&close);
    match bytes.get(token_end) {
        Some(closer) if *closer == close && !doubled => Ok(token_end),
        None | Some(b' ') => Err(Delimited::Absent),
        Some(_) => Err(Delimited::Unresolved),
    }
}

/// The URL under `pointed` when its row alone holds it whole: its scheme follows a blank or an
/// opener on the row, and a closer or a blank followed by more text ends it there, so it cannot
/// run into another row. A URL its delimiters fix opens on that row; one a blank ends is left to the
/// single-row rules (`Joined::None`), which read it the same. `None` when the row does not show the
/// URL whole.
pub(super) fn row_local_uri(rows: &mut BoxRows<'_>, pointed: At) -> Option<Joined> {
    let row = rows.get(pointed.row)?;
    let (first_text, last_text) = row.text?;
    // One byte per column: plain ASCII text as itself, anything else as a blank no URL holds.
    let readable = |column: usize| {
        let cell = &row.cells[column];
        cell.plain() && cell.ch.is_ascii() && !cell.ch.is_control()
    };
    let text: String = (0..row.cells.len()).map(|column| if readable(column) { row.cells[column].ch } else { ' ' }).collect();
    let pointer = usize::from(pointed.col);
    let found = url_scan::find_url_at(&text, pointer)?;
    let bytes = text.as_bytes();
    // A cell that may hide URL text is unknown, not a blank (stricter than SonicTerm): when the
    // URL's token, run over such cells, holds one, the row cannot show the URL whole.
    let hidden = |column: usize| {
        let cell = &row.cells[column];
        let lead = column.checked_sub(1).map(|lead| &row.cells[lead]);
        if cont(cell) { orphan_wide_cont(cell, lead) } else { !cell.plain() && cell_line_byte(cell) == LineByte::Unsafe }
    };
    let in_token = |index: usize| url_body_char(bytes[index] as char) || hidden(index);
    let token_from = (0..found.start).rev().find(|index| !in_token(*index)).map_or(0, |index| index + 1);
    let token_to = (found.start..bytes.len()).find(|index| !in_token(*index)).unwrap_or(bytes.len());
    if (token_from..token_to).any(hidden) {
        return None;
    }
    let token_end = (found.start..bytes.len()).find(|index| !url_body_char(bytes[*index] as char)).unwrap_or(bytes.len());
    if found.start <= first_text {
        // The scheme opens the row's text: the row above may run into it.
        return None;
    }
    let opener_at = found.start - 1;
    let prose_bracket = matches!(bytes[opener_at], b'(' | b'[') && !(bytes[opener_at] == b'(' && opener_at >= 1 && bytes[opener_at - 1] == b']');
    let embedded = opener_at <= first_text || opener_at.checked_sub(1).is_some_and(|before| url_body_char(bytes[before] as char));
    if prose_bracket && embedded {
        // The URL may be part of a longer one that runs into it from before.
        return None;
    }
    // A blank ending the URL proves its end only when more text follows it on the row.
    let inner_blank = token_end < bytes.len() && token_end < last_text && box_blank(&row.cells[token_end]);
    match delimited_url(&text, found.start, token_end) {
        Delimited::Url(url) if (url.start..url.end).contains(&pointer) => {
            let closer = (url.end..token_end).any(|index| matches!(bytes[index], b')' | b']')) || matches!(bytes.get(token_end), Some(b'"' | b'\'' | b'`' | b'>'));
            (closer || inner_blank).then(|| Joined::Whole(vec![(pointed.row, row.cells[url.start..url.end].to_vec())]))
        }
        Delimited::Absent => {
            let after_blank = box_blank(&row.cells[found.start - 1]);
            (after_blank && inner_blank).then_some(Joined::None)
        }
        Delimited::Url(_) | Delimited::Unresolved => None,
    }
}
