//! Registro de proveedores (P05.S6, FLOW §4.3): detecta cada CLI y llena
//! `providers` y `models`. El core solo conoce la lista de adapters, nunca un
//! proveedor en particular.

use serde_json::{Value, json};
use symphony_adapter_claude::ClaudeAdapter;
use symphony_adapter_codex::CodexAdapter;
use symphony_adapter_common::{AdapterError, ProviderAdapter};
use symphony_store::{StoreError, WriterHandle, repo};

use std::sync::Arc;

/// Adapters incluidos en esta versión (Kimi, Antigravity y Copilot llegan en P11).
pub fn builtin() -> Vec<Box<dyn ProviderAdapter>> {
    vec![
        Box::new(ClaudeAdapter::default()),
        Box::new(CodexAdapter::default()),
    ]
}

/// Adapters como Arc para el Runtime.
pub fn builtin_arc() -> Vec<Arc<dyn ProviderAdapter>> {
    vec![
        Arc::new(ClaudeAdapter::default()),
        Arc::new(CodexAdapter::default()),
    ]
}

/// Resultado de detectar un proveedor.
#[derive(Debug, Clone)]
pub struct Detected {
    pub provider: repo::Provider,
    pub models: Vec<repo::Model>,
}

/// Corre `detect` de cada adapter (lanza procesos: llamar fuera del runtime async).
pub fn detect_all(adapters: &[Box<dyn ProviderAdapter>]) -> Vec<Detected> {
    adapters
        .iter()
        .map(|a| {
            let (setup_state, cli_path, cli_version) = match a.detect() {
                Ok(d) => ("READY", Some(d.cli_path.display().to_string()), Some(d.version)),
                Err(AdapterError::NotInstalled(_)) => ("NOT_FOUND", None, None),
                Err(e) => {
                    tracing::warn!(provider = a.provider_id(), error = %e, "no se pudo detectar el CLI");
                    ("ERROR", None, None)
                }
            };
            let models = if setup_state == "READY" {
                a.list_models()
                    .into_iter()
                    .map(|m| repo::Model { id: m.id, provider_id: a.provider_id().into(), cli_model_id: m.cli_model_id, display_name: m.display_name, context_window: None })
                    .collect()
            } else {
                Vec::new()
            };
            Detected {
                provider: repo::Provider {
                    id: a.provider_id().into(),
                    display_name: a.display_name().into(),
                    cli_name: a.cli_name().into(),
                    cli_path,
                    cli_version,
                    setup_state: setup_state.into(),
                    hooks_supported: a.supports_hooks(),
                    hooks_can_hold: a.hooks_can_hold(),
                },
                models,
            }
        })
        .collect()
}

/// Guarda lo detectado por el writer único.
pub async fn save(
    writer: &WriterHandle,
    detected: Vec<Detected>,
    now: i64,
) -> Result<(), StoreError> {
    writer
        .write(Box::new(move |tx| {
            for d in &detected {
                repo::upsert_provider(tx, &d.provider, now)?;
                for m in &d.models {
                    repo::upsert_model(tx, m, now)?;
                }
            }
            Ok(())
        }))
        .await
}

/// Filas para `providers.list`.
pub fn list(conn: &rusqlite::Connection) -> Result<Value, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.display_name, p.setup_state, p.cli_version, p.cli_path, p.enabled,
                (SELECT COUNT(*) FROM models m WHERE m.provider_id = p.id AND m.enabled = 1)
         FROM providers p ORDER BY p.display_name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(json!({
            "id": r.get::<_, String>(0)?,
            "display_name": r.get::<_, String>(1)?,
            "setup_state": r.get::<_, String>(2)?,
            "cli_version": r.get::<_, Option<String>>(3)?,
            "cli_path": r.get::<_, Option<String>>(4)?,
            "enabled": r.get::<_, i64>(5)? == 1,
            "models": r.get::<_, i64>(6)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<Result<_, _>>()?))
}
