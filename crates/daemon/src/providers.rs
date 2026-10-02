//! Registro de proveedores (P05.S6, FLOW §4.3): detecta cada CLI y llena
//! `providers` y `models`. El core solo conoce la lista de adapters, nunca un
//! proveedor en particular.

use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use symphony_adapter_antigravity::AntigravityAdapter;
use symphony_adapter_claude::ClaudeAdapter;
use symphony_adapter_codex::CodexAdapter;
use symphony_adapter_common::{AdapterError, ProviderAdapter};
use symphony_adapter_copilot::CopilotAdapter;
use symphony_adapter_kimi::KimiAdapter;
use symphony_store::{StoreError, WriterHandle, repo};

use std::sync::Arc;

/// Adapters incluidos en esta versión.
pub fn builtin() -> Vec<Box<dyn ProviderAdapter>> {
    vec![
        Box::new(ClaudeAdapter::default()),
        Box::new(CodexAdapter::default()),
        Box::new(KimiAdapter::default()),
        Box::new(AntigravityAdapter::default()),
        Box::new(CopilotAdapter::default()),
    ]
}

/// Adapters como Arc para el Runtime.
pub fn builtin_arc() -> Vec<Arc<dyn ProviderAdapter>> {
    vec![
        Arc::new(ClaudeAdapter::default()),
        Arc::new(CodexAdapter::default()),
        Arc::new(KimiAdapter::default()),
        Arc::new(AntigravityAdapter::default()),
        Arc::new(CopilotAdapter::default()),
    ]
}

/// Resultado de detectar un proveedor.
#[derive(Debug, Clone)]
pub struct Detected {
    pub provider: repo::Provider,
    pub models: Vec<repo::Model>,
}

/// Corre `detect` de cada adapter (lanza procesos: llamar fuera del runtime async).
/// En paralelo: el arranque tarda lo que el CLI más lento (`copilot --version` ~2 s), no la suma.
pub fn detect_all(adapters: &[Box<dyn ProviderAdapter>]) -> Vec<Detected> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = adapters
            .iter()
            .map(|a| scope.spawn(move || detect_one(a.as_ref())))
            .collect();
        adapters
            .iter()
            .zip(handles)
            .map(|(a, h)| {
                h.join().unwrap_or_else(|_| {
                    tracing::warn!(provider = a.provider_id(), "`detect` entró en pánico");
                    describe(a.as_ref(), "ERROR", None, None)
                })
            })
            .collect()
    })
}

fn detect_one(a: &dyn ProviderAdapter) -> Detected {
    let (setup_state, cli_path, cli_version) = match a.detect() {
        Ok(d) => (
            "READY",
            Some(d.cli_path.display().to_string()),
            Some(d.version),
        ),
        Err(AdapterError::NotInstalled(_)) => ("NOT_FOUND", None, None),
        Err(e) => {
            tracing::warn!(provider = a.provider_id(), error = %e, "no se pudo detectar el CLI");
            ("ERROR", None, None)
        }
    };
    describe(a, setup_state, cli_path, cli_version)
}

fn describe(
    a: &dyn ProviderAdapter,
    setup_state: &str,
    cli_path: Option<String>,
    cli_version: Option<String>,
) -> Detected {
    let models = if setup_state == "READY" {
        a.list_models()
            .into_iter()
            .map(|m| repo::Model {
                id: m.id,
                provider_id: a.provider_id().into(),
                cli_model_id: m.cli_model_id,
                display_name: m.display_name,
                context_window: None,
            })
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
                symphony_store::health::ensure_account(tx, &d.provider.id, now)?;
                for m in &d.models {
                    repo::upsert_model(tx, m, now)?;
                }
            }
            Ok(())
        }))
        .await
}

/// Filas para `providers.list`: el proveedor con su salud vigente (FLOW §12.1): estado, certeza de
/// cuota (nunca un porcentaje inventado), reserva, agentes que lo usan, último fallo y uso.
pub fn list(
    conn: &rusqlite::Connection,
    cfg: &crate::health::HealthConfig,
    now: i64,
) -> Result<Value, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.display_name, p.setup_state, p.cli_version, p.cli_path, p.enabled,
                (SELECT COUNT(*) FROM models m WHERE m.provider_id = p.id AND m.enabled = 1)
         FROM providers p ORDER BY p.display_name",
    )?;
    let rows: Vec<_> = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                json!({
                    "id": r.get::<_, String>(0)?,
                    "display_name": r.get::<_, String>(1)?,
                    "setup_state": r.get::<_, String>(2)?,
                    "cli_version": r.get::<_, Option<String>>(3)?,
                    "cli_path": r.get::<_, Option<String>>(4)?,
                    "enabled": r.get::<_, i64>(5)? == 1,
                    "models": r.get::<_, i64>(6)?,
                }),
            ))
        })?
        .collect::<Result<_, _>>()?;
    let week_ago = now - 7 * 24 * 3600 * 1000;
    let usage =
        symphony_store::health::usage_since(conn, week_ago).map_err(rusqlite::Error::from)?;
    let mut out = Vec::new();
    for (id, mut row) in rows {
        let health =
            symphony_store::health::get_health(conn, &id, None).map_err(rusqlite::Error::from)?;
        row["health"] = match health {
            Some(h) => json!({
                "state": h.effective_state(now).as_str(),
                "stored_state": h.state.as_str(),
                "certainty": h.certainty.as_str(),
                // Solo existe con certeza `KNOWN` (la base lo exige).
                "remaining": h.remaining,
                "evidence": h.evidence,
                "retry_after_at": h.retry_after_at,
                "reset_at": h.reset_at,
                "reserve": cfg.reserve_for(&id),
            }),
            None => json!({
                "state": "UNKNOWN", "stored_state": "UNKNOWN", "certainty": "UNKNOWN",
                "remaining": null, "evidence": null, "retry_after_at": null, "reset_at": null,
                "reserve": cfg.reserve_for(&id),
            }),
        };
        row["agents_active"] = json!(conn.query_row(
            "SELECT COUNT(DISTINCT agent_id) FROM agent_runs WHERE provider_id = ?1 AND ended_at IS NULL",
            [&id],
            |r| r.get::<_, i64>(0)
        )?);
        row["model_names"] = json!(
            conn.prepare(
                "SELECT display_name FROM models WHERE provider_id = ?1 AND enabled = 1 ORDER BY display_name"
            )?
            .query_map([&id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
        );
        row["last_failure"] = conn
            .query_row(
                "SELECT failure_type, message, occurred_at FROM provider_failures
                 WHERE provider_id = ?1 ORDER BY occurred_at DESC LIMIT 1",
                [&id],
                |r| {
                    Ok(json!({
                        "type": r.get::<_, String>(0)?,
                        "message": r.get::<_, Option<String>>(1)?,
                        "at": r.get::<_, i64>(2)?,
                    }))
                },
            )
            .optional()?
            .unwrap_or(Value::Null);
        let mine: Vec<_> = usage.iter().filter(|u| u.provider_id == id).collect();
        let sum = |source: &str, f: fn(&symphony_store::health::UsageRow) -> i64| -> i64 {
            mine.iter()
                .filter(|u| u.source == source)
                .map(|u| f(u))
                .sum()
        };
        row["usage_7d"] = json!({
            "reported_tokens": sum("REPORTED", |u| u.tokens_in + u.tokens_out),
            "estimated_tokens": sum("ESTIMATED", |u| u.tokens_in + u.tokens_out),
        });
        out.push(row);
    }
    Ok(Value::Array(out))
}

/// Filas para `usage.get`: uso por proveedor, modelo y origen (`REPORTED` / `ESTIMATED`).
pub fn usage(conn: &rusqlite::Connection, days: i64, now: i64) -> Result<Value, rusqlite::Error> {
    let since = now - days.max(1) * 24 * 3600 * 1000;
    let rows = symphony_store::health::usage_since(conn, since).map_err(rusqlite::Error::from)?;
    Ok(Value::Array(
        rows.into_iter()
            .map(|u| {
                json!({
                    "provider_id": u.provider_id, "model_id": u.model_id, "source": u.source,
                    "runs": u.runs, "tokens_in": u.tokens_in, "tokens_out": u.tokens_out,
                })
            })
            .collect(),
    ))
}
