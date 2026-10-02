//! Helpers compartidos por los adapters: clasificación de texto plano y búsqueda en el PATH.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;
use symphony_adapter_common::{
    find_on_path, has_token, json_str, looks_like_error, parse_retry_after_ms,
};

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

#[test]
fn retry_after_is_read_from_relative_durations() {
    let ms = |t: &str| parse_retry_after_ms(t);
    assert_eq!(ms("Retry-After: 30"), Some(30_000));
    assert_eq!(
        ms("rate limit exceeded. Try again in 28 seconds"),
        Some(28_000)
    );
    assert_eq!(ms("429: retry after 5 minutes"), Some(300_000));
    assert_eq!(ms("Please try again in 1h 30m."), Some(5_400_000));
    assert_eq!(ms("try again in 2 minutes and 30 seconds"), Some(150_000));
    assert_eq!(ms("retry in 1.5s"), Some(1_500));
    assert_eq!(ms("RETRY-AFTER=250ms"), Some(250));
    assert_eq!(ms("try again in 3 days"), Some(259_200_000));
}

#[test]
fn retry_after_ignores_what_is_not_a_relative_duration() {
    // Una hora de reloj depende de la zona horaria: no se interpreta.
    assert_eq!(
        parse_retry_after_ms("You've hit your usage limit. Try again at 5 PM."),
        None
    );
    assert_eq!(parse_retry_after_ms("waiting for network"), None);
    assert_eq!(parse_retry_after_ms("retry after soon"), None);
    assert_eq!(parse_retry_after_ms(""), None);
    // Un valor absurdo se acota a una semana.
    assert_eq!(
        parse_retry_after_ms("retry after 9999 days"),
        Some(7 * 86_400_000)
    );
}
