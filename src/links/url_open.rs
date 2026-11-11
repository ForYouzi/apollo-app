//! The lexical URI check SonicTerm's scanner shares with its opener: only the
//! `check_uri` / `validate` part of SonicTerm's `url_open.rs` (MIT, see
//! `url_scan.rs`). Apollo opens links itself (`links::activation`).

use std::io;

/// Strict allow-list check applied to every URI before dispatch: only `http`,
/// `https`, and `mailto` pass, so no `file:` URI reaches a handler. Public so
/// callers can ask whether the opener would accept a URI.
pub fn validate(url: &str) -> io::Result<()> {
    check_uri(url, DISPATCH_SCHEMES)
}

/// Schemes the opener dispatches. `file:` is absent on purpose: a filesystem
/// target is classified as a local path and opened through the path probes.
const DISPATCH_SCHEMES: &[&str] = &["http://", "https://", "mailto:"];

/// Shared lexical URI check: non-empty, at most 4096 bytes, a case-insensitive
/// prefix from `schemes`, no raw control character, and none of the forbidden
/// metacharacters `` | ^ < > " ' ` ``. Other shell-significant characters such
/// as `&`, `$`, and `;` pass, because no dispatch path re-tokenizes the URI.
/// The opener applies it with its dispatch schemes, and detection with its own.
pub(crate) fn check_uri(url: &str, schemes: &[&str]) -> io::Result<()> {
    if url.is_empty() {
        // When: url carries no scheme to match against the allow-list, so no
        // handler may be invoked for it.
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty url"));
    }
    if url.len() > 4096 {
        // When: url exceeds the 4096-byte cap, bounding what reaches the
        // platform handler regardless of scheme.
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "url too long"));
    }
    let lower = url.to_ascii_lowercase();
    let scheme_ok = schemes.iter().any(|scheme| lower.starts_with(scheme));
    if !scheme_ok {
        // When: scheme_ok finds no prefix from schemes, so the URI is outside the
        // caller's allow-list and no handler may receive it.
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "scheme not allowed"));
    }
    for character in url.chars() {
        match character {
            '|' | '^' | '<' | '>' | '"' | '\'' | '`' | '\r' | '\n' | '\0' => {
                // When: character is a shell metacharacter, refused as defense in
                // depth even though no dispatch path re-tokenizes the URI.
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "forbidden character"));
            }
            candidate if candidate.is_control() => {
                // When: candidate is any control code, which a URI must percent-encode
                // rather than carry raw.

                // Rejects the full Unicode control set: C0 (< 0x20),
                // DEL (0x7F), and C1 (0x80..=0x9F). A raw control code
                // is never legitimate in a URI (it must be %-encoded),
                // so refusing it keeps the allow-list honest against
                // untrusted OSC 8 input.
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "control character"));
            }
            _ => {
                // When: character is outside the forbidden and control sets, so it
                // survives validation unchanged.
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "url_open_tests.rs"]
mod url_open_tests;
