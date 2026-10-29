//! P13.S2: el parser de líneas del stdout de Claude no hace panic con ninguna
//! entrada (basura, JSON roto, JSON arbitrario con campos raros) y es
//! determinista. Es la versión proptest del target cargo-fuzz `claude_stream`:
//! corre en stable, en `cargo nextest` y en CI.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.

use proptest::prelude::*;
use serde_json::Value;
use symphony_adapter_claude::ClaudeAdapter;
use symphony_adapter_common::ProviderAdapter;

/// JSON arbitrario: la forma más común de línea del stream (`stream-json`).
fn arb_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        "[^\"]{0,32}".prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 32, 8, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::hash_map("[-_a-z0-9]{1,12}", inner, 0..6)
                .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

proptest! {
    /// Cualquier línea de texto (o basura que el CLI escupa por stdout) no hace
    /// panic y produce los mismos eventos si se parsea dos veces.
    #[test]
    fn any_line_never_panics_and_is_deterministic(line in "\\PC{0,512}") {
        let a = ClaudeAdapter::default();
        let events = a.parse_stream_line(&line);
        prop_assert_eq!(events, a.parse_stream_line(&line));
    }

    /// Cualquier JSON, con los campos donde el stream real los pone, no hace panic.
    #[test]
    fn any_json_never_panics(v in arb_json()) {
        let a = ClaudeAdapter::default();
        for line in [
            // Tal cual, y con la envoltura típica de cada tipo de evento del stream.
            v.to_string(),
            serde_json::json!({ "type": "assistant", "message": v }).to_string(),
            serde_json::json!({ "type": "stream_event", "event": v }).to_string(),
            serde_json::json!({ "type": "result", "usage": v }).to_string(),
        ] {
            let events = a.parse_stream_line(&line);
            prop_assert_eq!(events, a.parse_stream_line(&line));
        }
    }
}

#[test]
fn known_minimal_cases_still_parse() {
    let a = ClaudeAdapter::default();
    assert_eq!(a.parse_stream_line(""), Vec::new());
    assert_eq!(a.parse_stream_line("no es json"), Vec::new());
    assert_eq!(a.parse_stream_line("{}"), Vec::new());
    assert_eq!(
        a.parse_stream_line("{\"type\":\"result\",\"result\":\"ok\"}"),
        vec![symphony_adapter_common::AgentEvent::TurnFinished {
            last_message: Some("ok".into()),
        }]
    );
}
