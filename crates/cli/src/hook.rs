//! `symphony hook emit`: lo ejecutan los hooks de los CLIs (Claude Code, Codex).
//! Lee el JSON del hook por stdin y lo manda al daemon (IDEA §5.3).
//!
//! Reglas:
//! - Sin `SYMPHONY_AGENT_ID` no hace nada: el usuario usa el CLI fuera de Symphony.
//! - Nunca rompe al CLI: cualquier problema (daemon caído, timeout, JSON raro)
//!   termina con exit 0 y sin salida.
//! - Nunca arranca el daemon.
//! - No imprime `allow`: eso le saltearía al usuario sus propias reglas. Solo imprime el
//!   `deny` con su razón cuando el scheduler rechaza una operación pesada (P08.S3).

use std::io::Read;
use std::time::Duration;

use serde_json::{Value, json};
use symphony_protocol::{Message, Outcome, Request, transport};

/// Límite del payload: un hook normal pesa unos KB.
const MAX_PAYLOAD: u64 = 4 * 1024 * 1024;
/// Cuánto se espera al daemon en eventos que no retienen.
const QUICK_TIMEOUT: Duration = Duration::from_secs(10);

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
    // Solo `PreToolUse` puede quedar retenido; esperar de más en el resto no sirve de nada.
    let provider = env("SYMPHONY_PROVIDER");
    let may_hold = payload.get("hook_event_name").and_then(Value::as_str) == Some("PreToolUse");
    let wait = if may_hold {
        Duration::from_secs(
            symphony_adapter_common::hooks::HoldTimes::for_provider(provider.as_deref())
                .client_secs,
        )
    } else {
        QUICK_TIMEOUT
    };
    let params = json!({
        "agent_id": agent_id,
        "run_id": env("SYMPHONY_RUN_ID"),
        "project_id": env("SYMPHONY_PROJECT_ID"),
        "provider": provider,
        "payload": payload,
    });
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    let decision = rt.block_on(async {
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
        tokio::time::timeout(wait, exchange).await.ok().flatten()
    });
    if let Some(out) = decision.as_ref().and_then(deny_output) {
        println!("{out}");
    }
}

/// Respuesta de `PreToolUse` que impide la operación, con la razón para el modelo
/// (formato común a Claude Code y Codex, P01 Test B).
fn deny_output(decision: &Value) -> Option<String> {
    if decision.get("decision").and_then(Value::as_str) != Some("deny") {
        return None;
    }
    let reason = decision
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("operación en cola en Symphony; reintenta en unos segundos");
    Some(
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }
        })
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_deny_decision_prints_something() {
        assert_eq!(
            deny_output(&json!({ "decision": "allow", "events": 1 })),
            None
        );
        let out = deny_output(&json!({ "decision": "deny", "reason": "en cola" })).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            v["hookSpecificOutput"]["permissionDecisionReason"],
            "en cola"
        );
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    }
}
