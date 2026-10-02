//! P10.S2: propiedades de los parsers de error de los cinco adapters (el equivalente a los
//! fuzz targets del PLAN, con proptest para que corran en CI sin nightly).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use proptest::prelude::*;
use symphony_adapter_antigravity::AntigravityAdapter;
use symphony_adapter_claude::ClaudeAdapter;
use symphony_adapter_codex::CodexAdapter;
use symphony_adapter_common::{ProviderAdapter, parse_retry_after_ms};
use symphony_adapter_copilot::CopilotAdapter;
use symphony_adapter_kimi::KimiAdapter;
use symphony_core::FailureType;

fn adapters() -> Vec<Box<dyn ProviderAdapter>> {
    vec![
        Box::new(ClaudeAdapter::default()),
        Box::new(CodexAdapter::default()),
        Box::new(KimiAdapter::default()),
        Box::new(AntigravityAdapter::default()),
        Box::new(CopilotAdapter::default()),
    ]
}

/// Texto con pinta de error de proveedor, con trozos que cada adapter reconoce.
fn error_like() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        Just("429".to_string()),
        Just("401".to_string()),
        Just("Too Many Requests".to_string()),
        Just("rate limit exceeded".to_string()),
        Just("usage limit".to_string()),
        Just("quota exceeded".to_string()),
        Just("RESOURCE_EXHAUSTED".to_string()),
        Just("Unauthorized".to_string()),
        Just("try again in 30 seconds".to_string()),
        Just("Retry-After: 12".to_string()),
        Just("Authorization: Bearer sk-AAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string()),
        Just("model not found".to_string()),
        Just("Error:".to_string()),
        "\\PC{0,24}",
    ];
    proptest::collection::vec(piece, 0..6).prop_map(|p| p.join(" "))
}

proptest! {
    /// Ningún parser entra en pánico con texto cualquiera, y todo lo que clasifican sale
    /// redactado y acotado.
    #[test]
    fn parsers_never_panic_and_always_redact(text in prop_oneof![error_like(), "\\PC{0,200}"]) {
        for a in adapters() {
            let _ = a.parse_stream_line(&text);
            let _ = a.parse_stderr_line(&text);
            let _ = a.parse_hook(&serde_json::from_str(&text).unwrap_or(serde_json::Value::Null));
            if let Some(e) = a.parse_error(&text) {
                prop_assert!(e.message.chars().count() <= 500);
                prop_assert!(!e.message.contains("sk-AAAA"), "{}: mensaje sin redactar", a.provider_id());
                // Un 429 temporal reintenta solo; un límite de cuota no es transitorio.
                if e.failure_type == FailureType::TempRateLimit {
                    prop_assert!(e.transient || a.provider_id() == "anthropic", "{}", a.provider_id());
                }
            }
        }
        let _ = parse_retry_after_ms(&text);
    }

    /// Un 429 («too many requests») nunca se clasifica como cuota agotada en ningún adapter.
    #[test]
    fn a_429_is_never_classified_as_exhausted(prefix in "[a-zA-Z ]{0,12}", suffix in "[a-zA-Z .]{0,12}") {
        let text = format!("{prefix} 429 Too Many Requests {suffix}");
        for a in adapters() {
            if let Some(e) = a.parse_error(&text) {
                prop_assert!(
                    !matches!(
                        e.failure_type,
                        FailureType::DailyQuota | FailureType::WeeklyQuota | FailureType::AccountLimit
                    ),
                    "{}: {text:?} → {:?}", a.provider_id(), e.failure_type
                );
            }
        }
    }

    /// El `retry_after` que se lee de un mensaje es el que se escribió.
    #[test]
    fn retry_after_roundtrips_seconds_and_minutes(n in 1u64..3600, minutes in proptest::bool::ANY) {
        let (text, want) = if minutes {
            (format!("rate limited, try again in {n} minutes"), n * 60_000)
        } else {
            (format!("rate limited, retry after {n} seconds"), n * 1000)
        };
        prop_assert_eq!(parse_retry_after_ms(&text), Some(want));
    }
}

#[test]
fn every_adapter_extracts_retry_after_from_its_rate_limit_text() {
    for a in adapters() {
        let text = "429 Too Many Requests: rate limit exceeded, try again in 28 seconds";
        let e = a
            .parse_error(text)
            .unwrap_or_else(|| panic!("{} no reconoce un 429", a.provider_id()));
        assert_eq!(
            e.retry_after_ms,
            Some(28_000),
            "{}: debe leer la espera del texto",
            a.provider_id()
        );
        assert_ne!(
            e.failure_type,
            FailureType::DailyQuota,
            "{}",
            a.provider_id()
        );
    }
}
