//! SonicTerm's tests of the lexical URI check (its `url_open_tests.rs`, up to
//! the opener tests, which cover code Apollo doesn't take).

use super::*;

// ---- scheme allow-list: only http/https/mailto are accepted -----------

/// The opener dispatches exactly the web and mail schemes.
#[test]
fn accepts_the_three_dispatch_schemes() {
    for url in
        ["http://example.com", "https://example.com/path?q=1#frag", "mailto:user@example.com"]
    {
        assert!(validate(url).is_ok(), "should accept {url:?}");
    }
}

/// Scheme matching ignores ASCII case for every dispatch scheme.
#[test]
fn scheme_match_is_case_insensitive() {
    for url in ["HTTP://EXAMPLE.COM", "HtTpS://Example.com", "MAILTO:a@b.com"] {
        assert!(validate(url).is_ok(), "scheme compare must ignore case: {url:?}");
    }
}

#[test]
fn rejects_unsupported_schemes() {
    for url in [
        "javascript:alert(1)",
        "data:text/html,<script>",
        "ftp://example.com/file",
        "vbscript:msgbox",
        "ssh://host",
        "about:blank",
        "tel:+15551234",
        "\\\\host\\share",
    ] {
        let err = validate(url).expect_err("must reject unsupported scheme");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}

/// Filesystem URIs never reach a URI handler: the dispatch validator refuses
/// every `file:` URI, local or remote and in any case, so a caller that skips
/// local-target classification still cannot hand one to the platform opener.
#[test]
fn validate_refuses_filesystem_uris() {
    for uri in [
        "file:///etc/hosts",
        "file://host/share/x",
        "FILE:///etc/hosts",
        "file://localhost/etc/hosts",
    ] {
        let err = validate(uri).expect_err("the opener must refuse a filesystem URI");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{uri}");
        assert_eq!(err.to_string(), "scheme not allowed", "{uri}");
    }
}

#[test]
fn rejects_scheme_only_with_no_body() {
    // A bare scheme prefix has nothing to open. `validate` accepts the
    // prefix itself (policy is scheme + char safety), so document that
    // boundary explicitly rather than assume trailing content.
    assert!(validate("mailto:").is_ok(), "bare mailto: is scheme-valid");
    // But a near-miss that only partially spells a scheme is rejected.
    for url in ["http:/only-one-slash", "htp://typo.example", "mail:to@x"] {
        assert!(validate(url).is_err(), "near-miss scheme must fail: {url:?}");
    }
}

// ---- length + emptiness boundaries -------------------------------------

#[test]
fn rejects_empty_url() {
    let err = validate("").expect_err("empty must fail");
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn length_cap_boundary_is_4096_bytes() {
    // "https://" (8) + body == exactly 4096 is allowed; 4097 is not.
    let at_cap = format!("https://{}", "a".repeat(4096 - "https://".len()));
    assert_eq!(at_cap.len(), 4096);
    assert!(validate(&at_cap).is_ok(), "exactly 4096 bytes must pass");

    let over_cap = format!("https://{}", "a".repeat(4096 - "https://".len() + 1));
    assert_eq!(over_cap.len(), 4097);
    let err = validate(&over_cap).expect_err("4097 bytes must fail");
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

// ---- shell metacharacters / quotes / NUL / CR / LF ---------------------

#[test]
fn rejects_shell_metacharacters_and_quotes() {
    // Retained defense in depth: no dispatch path re-tokenizes the URI, but
    // each of these embedded in an otherwise-valid https URL is still refused.
    for meta in ['|', '^', '<', '>', '"', '\'', '`'] {
        let url = format!("https://example.com/{meta}evil");
        let err = validate(&url).expect_err("metacharacter must be rejected");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "char {meta:?} should fail");
    }
}

#[test]
fn rejects_nul_cr_lf() {
    for bad in ["https://example.com/\0", "https://example.com/\r", "https://example.com/\n"] {
        let err = validate(bad).expect_err("NUL/CR/LF must be rejected");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}

#[test]
fn rejects_c0_control_characters() {
    // Every C0 control (0x01..=0x1F) embedded in the body is refused.
    for code in 0x01u32..=0x1F {
        let url = format!("https://example.com/{}", char::from_u32(code).unwrap());
        assert!(validate(&url).is_err(), "C0 control U+{code:04X} must be rejected");
    }
}

#[test]
fn rejects_del_and_c1_control_characters() {
    // Regression guard for the gap where the old `(c as u32) < 0x20`
    // check let DEL (0x7F) and the C1 range (0x80..=0x9F) through even
    // though the module contract promises to reject control chars.
    let url_del = "https://example.com/\u{7f}";
    assert!(validate(url_del).is_err(), "DEL (0x7F) must be rejected");

    for code in 0x80u32..=0x9F {
        let url = format!("https://example.com/{}", char::from_u32(code).unwrap());
        assert!(validate(&url).is_err(), "C1 control U+{code:04X} must be rejected");
    }
}

#[test]
fn accepts_printable_ascii_and_percent_encoding() {
    // The safe body should still allow ordinary URL characters and the
    // %-encoded form of the bytes we reject in raw form.
    for url in [
        "https://example.com/a/b/c?x=1&_y=2".replace('&', "%26"),
        "https://example.com/%20space%0Aencoded".to_string(),
        "https://example.com/~user/(paren)".to_string(),
    ] {
        assert!(validate(&url).is_ok(), "printable/encoded body must pass: {url:?}");
    }
}
