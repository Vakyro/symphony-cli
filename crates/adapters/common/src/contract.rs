//! Suite de contrato que todo adapter tiene que pasar (P05.S1). Cada adapter
//! aporta fixtures reales de su CLI (sanitizados) y llama a [`check`] en un test.

use serde_json::Value;
use symphony_core::FailureType;

use crate::{AgentEvent, ProviderAdapter, SpawnRequest};

/// Casos con la salida esperada, expresada como nombres de `events.type`.
#[derive(Debug, Default)]
pub struct Fixtures {
    /// Líneas del stream de stdout → tipos de evento esperados, en orden.
    pub stream: Vec<(String, Vec<&'static str>)>,
    /// Payloads de hooks → tipos de evento esperados, en orden.
    pub hooks: Vec<(Value, Vec<&'static str>)>,
    /// Textos de error → clasificación esperada.
    pub errors: Vec<(String, FailureType)>,
}

/// Entradas que un adapter recibe de verdad y no pueden romperlo.
const GARBAGE: &[&str] = &[
    "",
    "   ",
    "not json",
    "{",
    "[]",
    "null",
    "42",
    "{\"type\":null}",
    "{\"type\":\"desconocido\",\"x\":1}",
    "\u{1b}[31mcolor\u{1b}[0m",
    "{\"type\":\"assistant\",\"message\":{\"content\":\"no es una lista\"}}",
];

fn names(events: &[AgentEvent]) -> Vec<&'static str> {
    events.iter().map(AgentEvent::type_name).collect()
}

/// Corre la suite. Devuelve la lista de fallas (vacía = pasa).
pub fn check(adapter: &dyn ProviderAdapter, fixtures: &Fixtures) -> Vec<String> {
    let mut failures = Vec::new();
    let id = adapter.provider_id();

    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        failures.push(format!("provider_id inválido: `{id}`"));
    }

    // 1. Robustez: nada de pánicos ni eventos inventados con entradas basura.
    for g in GARBAGE {
        let ev = adapter.parse_stream_line(g);
        if !ev.is_empty() {
            failures.push(format!("stream basura {g:?} produjo {:?}", names(&ev)));
        }
        let _ = adapter.parse_hook(&serde_json::from_str(g).unwrap_or(Value::Null));
        let _ = adapter.parse_error(g);
    }

    // 2. Fixtures del CLI real.
    for (line, want) in &fixtures.stream {
        let got = names(&adapter.parse_stream_line(line));
        if &got != want {
            failures.push(format!("stream {line}: esperaba {want:?}, obtuve {got:?}"));
        }
    }
    for (payload, want) in &fixtures.hooks {
        let got = names(&adapter.parse_hook(payload));
        if &got != want {
            failures.push(format!("hook {payload}: esperaba {want:?}, obtuve {got:?}"));
        }
    }
    for (text, want) in &fixtures.errors {
        match adapter.parse_error(text) {
            Some(e) if e.failure_type == *want => {
                if e.message.contains("sk-") || e.message.to_lowercase().contains("bearer ") {
                    failures.push(format!("error {text:?}: el mensaje no está redactado"));
                }
            }
            other => failures.push(format!("error {text:?}: esperaba {want}, obtuve {other:?}")),
        }
    }

    // 3. El comando de spawn corre en el worktree, con el modelo pedido y las variables del agente.
    let req = SpawnRequest {
        worktree: std::env::temp_dir(),
        model: "modelo-de-prueba".into(),
        prompt: "hola".into(),
        session_id: Some("00000000-0000-4000-8000-000000000000".into()),
        hook: None,
        env: vec![("SYMPHONY_AGENT_ID".into(), "agent-contract".into())],
    };
    match adapter.spawn_spec(&req) {
        Ok(spec) => {
            if spec.cwd.as_deref() != Some(req.worktree.as_path()) {
                failures.push("spawn_spec no corre en el worktree".into());
            }
            if !spec
                .args
                .iter()
                .any(|a| a.to_string_lossy().contains("modelo-de-prueba"))
            {
                failures.push("spawn_spec no pasa el modelo pedido".into());
            }
            if !spec
                .env
                .iter()
                .any(|(k, v)| k == "SYMPHONY_AGENT_ID" && v == "agent-contract")
            {
                failures.push("spawn_spec no pasa SYMPHONY_AGENT_ID".into());
            }
            if spec.args.iter().any(|a| a.to_string_lossy() == "hola") {
                failures.push("el prompt no puede ir como argumento (va por stdin)".into());
            }
            if !spec.stdin {
                failures.push("spawn_spec tiene que dejar stdin abierto para el prompt".into());
            }
        }
        Err(e) => failures.push(format!("spawn_spec falló: {e}")),
    }
    if adapter.encode_prompt("hola").is_empty() {
        failures.push("encode_prompt vacío".into());
    }
    failures
}
