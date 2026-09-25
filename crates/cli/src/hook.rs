//! `symphony hook emit`: lo ejecutan los hooks de los CLIs (Claude Code, Codex).
//! Lee el JSON del hook por stdin y lo manda al daemon (IDEA §5.3).
//!
//! Reglas:
//! - Sin `SYMPHONY_AGENT_ID` no hace nada: el usuario usa el CLI fuera de Symphony.
//! - Nunca rompe al CLI: cualquier problema (daemon caído, timeout, JSON raro)
//!   termina con exit 0 y sin salida.
//! - Nunca arranca el daemon.
//! - En P05 no imprime nada: el CLI sigue su flujo normal de permisos. Imprimir
//!   `allow` le saltearía al usuario sus propias reglas; el `deny` llega con el scheduler (P08).

use std::io::Read;
use std::time::Duration;

use serde_json::{Value, json};
use symphony_protocol::{Message, Outcome, Request, transport};

/// Límite del payload: un hook normal pesa unos KB.
const MAX_PAYLOAD: u64 = 4 * 1024 * 1024;
/// Cuánto se espera la decisión del daemon. Tiene que ser menor que el `timeout`
/// del hook en el CLI (ADR-0003: Codex no mata hooks vencidos).
const DECISION_TIMEOUT: Duration = Duration::from_secs(10);

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

pub fn emit() {
    let Some(agent_id) = env("SYMPHONY_AGENT_ID") else {
        return;
    };
    let mut input = String::new();
    if std::io::stdin()
        .take(MAX_PAYLOAD)
        .read_to_string(&mut input)
        .is_err()
    {
        return;
    }
    let Ok(payload) = serde_json::from_str::<Value>(&input) else {
        return;
    };
    let Ok(home) = symphony_core::SymphonyHome::resolve() else {
        return;
    };
    let params = json!({
        "agent_id": agent_id,
        "run_id": env("SYMPHONY_RUN_ID"),
        "project_id": env("SYMPHONY_PROJECT_ID"),
        "provider": env("SYMPHONY_PROVIDER"),
        "payload": payload,
    });
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    let _decision = rt.block_on(async {
        let exchange = async {
            let mut conn = transport::connect(home.root()).await.ok()?;
            let id = symphony_core::RunId::new().to_string();
            conn.send(&Message::Request(Request::new(&id, "hook.emit", params)))
                .await
                .ok()?;
            match conn.recv().await.ok()? {
                Some(Message::Response(r)) if r.id == id => match r.outcome {
                    Outcome::Ok(v) => Some(v),
                    Outcome::Error(_) => None,
                },
                _ => None,
            }
        };
        tokio::time::timeout(DECISION_TIMEOUT, exchange)
            .await
            .ok()
            .flatten()
    });
}
