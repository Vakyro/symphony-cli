//! Helpers compartidos por los adapters: clasificación de texto plano y búsqueda en el PATH.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;
use symphony_adapter_common::{find_on_path, has_token, json_str, looks_like_error};

#[test]
fn status_codes_match_only_as_whole_tokens() {
    assert!(has_token("error 429: slow down", "429"));
    assert!(has_token("HTTP/1.1 401", "401"));
    assert!(has_token("(429)", "429"));
    assert!(!has_token("processed 4290 files", "429"));
    assert!(!has_token("/var/a401b/cache", "401"));
    assert!(!has_token("", "429"));
}

#[test]
fn only_error_looking_lines_are_errors() {
    assert!(looks_like_error("Error: Model \"x\" is not available."));
    assert!(looks_like_error("  error: invalid model"));
    assert!(looks_like_error("FATAL: boom"));
    assert!(!looks_like_error("warning: quota almost reached"));
    assert!(!looks_like_error("Loaded 4012 entries"));
    assert!(!looks_like_error(""));
}

#[test]
fn json_str_reads_only_string_fields() {
    let v = json!({"a": "x", "n": 3});
    assert_eq!(json_str(&v, "a").as_deref(), Some("x"));
    assert_eq!(json_str(&v, "n"), None);
    assert_eq!(json_str(&v, "missing"), None);
}

#[test]
fn a_missing_program_is_not_found_on_the_path() {
    assert!(find_on_path("symphony-no-existe-xyz").is_none());
}
