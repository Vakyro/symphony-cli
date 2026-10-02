//! Regresiones de la revisión de código de P10: salud, cuota y uso sobre la base real.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rusqlite::Connection;
use symphony_adapter_common::{ProviderError, QuotaSnapshot};
use symphony_core::{FailureType, ProviderFailureId, ProviderState, RunId};
use symphony_daemon::health;
use symphony_store::health as db;

const NOW: i64 = 1_000_000_000_000;

fn seeded() -> (Connection, RunId) {
    let conn = symphony_store::open_in_memory().unwrap();
    conn.execute_batch(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','demo','/repo','main',0);
         INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','p1','ACTIVE',0);
         INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('t1','p1','T-1','WORK','uno','RUNNING',0,0);
         INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('anthropic','Claude','claude','CLI','READY');
         INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at) VALUES ('claude/sonnet','anthropic','sonnet','Sonnet',0);
         INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, requested_model_id, failover_policy, context_mode, created_at, updated_at)
             VALUES ('a1','p1','s1','t1',1,'RUNNING','EXACT','claude/sonnet','ANY','BALANCED',0,0);",
    )
    .unwrap();
    db::ensure_account(&conn, "anthropic", 0).unwrap();
    let run = RunId::new();
    conn.execute(
        "INSERT INTO agent_runs (id, agent_id, seq, provider_id, account_id, model_id, status, started_at)
         VALUES (?1, 'a1', 1, 'anthropic', 'acct-anthropic', 'claude/sonnet', 'RUNNING', 0)",
        [run.to_string()],
    )
    .unwrap();
    (conn, run)
}

fn error(kind: FailureType) -> ProviderError {
    ProviderError {
        failure_type: kind,
        raw_code: None,
        message: "x".into(),
        retry_after_ms: None,
        resets_at: None,
        transient: false,
    }
}

/// Como el recorder y el failover: primero se guarda el fallo, después se actualiza la salud.
fn fail(conn: &Connection, run: RunId, kind: FailureType, at: i64) -> ProviderState {
    let err = error(kind);
    health::record_failure_row(
        conn,
        ProviderFailureId::new(),
        Some(run),
        "anthropic",
        Some("claude/sonnet"),
        &err,
        at,
    )
    .unwrap();
    health::on_failure(conn, "anthropic", Some("claude/sonnet"), &err, at).unwrap();
    db::get_health(conn, "anthropic", None)
        .unwrap()
        .unwrap()
        .state
}

#[test]
fn the_current_failure_is_not_counted_twice() {
    let (conn, run) = seeded();
    // Red: un fallo degrada; el segundo la deja sin conexión.
    assert_eq!(
        fail(&conn, run, FailureType::Network, NOW),
        ProviderState::Degraded
    );
    assert_eq!(
        fail(&conn, run, FailureType::Network, NOW + 1),
        ProviderState::Offline
    );
}

#[test]
fn a_provider_is_throttled_after_three_rate_limits_not_two() {
    let (conn, run) = seeded();
    assert_eq!(
        fail(&conn, run, FailureType::TempRateLimit, NOW),
        ProviderState::RateLimited
    );
    assert_eq!(
        fail(&conn, run, FailureType::TempRateLimit, NOW + 1),
        ProviderState::RateLimited
    );
    assert_eq!(
        fail(&conn, run, FailureType::TempRateLimit, NOW + 2),
        ProviderState::Throttled
    );
}

fn quota(conn: &Connection, run: RunId, window: &str, used: f64, resets_at: i64, at: i64) {
    conn.execute(
        "INSERT INTO events (project_id, agent_id, run_id, type, source, payload_json, occurred_at)
         VALUES ('p1','a1',?1,'QuotaUpdated','JSON_STREAM',?2,?3)",
        rusqlite::params![
            run.to_string(),
            format!(r#"{{"event":"quota","window":"{window}","used_fraction":{used},"resets_at":{resets_at}}}"#),
            at
        ],
    )
    .unwrap();
    let q = QuotaSnapshot {
        window: window.into(),
        used_fraction: used,
        resets_at: Some(resets_at),
    };
    health::on_quota(conn, "anthropic", 0.2, &q, at).unwrap();
}

#[test]
fn a_low_quota_warning_clears_when_its_window_has_reset() {
    let (conn, run) = seeded();
    let reset_secs = NOW / 1000 + 100;
    quota(&conn, run, "five_hour", 0.9, reset_secs, NOW);
    let low = db::get_health(&conn, "anthropic", None).unwrap().unwrap();
    assert_eq!(low.state, ProviderState::QuotaLow);

    // La ventana se reinició y llega un informe nuevo con la cuota entera.
    let later = NOW + 200_000;
    quota(&conn, run, "five_hour", 0.05, reset_secs + 18_000, later);
    let ok = db::get_health(&conn, "anthropic", None).unwrap().unwrap();
    assert_eq!(ok.state, ProviderState::Healthy);
    assert_eq!(ok.remaining, Some(0.95));

    // Y si el único informe que queda es de una ventana ya reiniciada, la cuota está entera.
    let (conn, run) = seeded();
    quota(&conn, run, "five_hour", 0.9, reset_secs, NOW);
    let stale = QuotaSnapshot {
        window: "five_hour".into(),
        used_fraction: 0.9,
        resets_at: Some(reset_secs),
    };
    health::on_quota(&conn, "anthropic", 0.2, &stale, NOW + 200_000).unwrap();
    let h = db::get_health(&conn, "anthropic", None).unwrap().unwrap();
    assert_eq!(
        h.state,
        ProviderState::Healthy,
        "no queda bloqueado por un informe viejo"
    );
}

#[test]
fn usage_keeps_one_row_per_run_with_the_peak_context_not_a_running_sum() {
    let (conn, run) = seeded();
    for tokens in [10_000u64, 20_000, 55_000, 40_000] {
        health::on_usage(&conn, run, tokens, NOW).unwrap();
    }
    let rows = db::usage_since(&conn, 0).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].runs, rows[0].tokens_in),
        (1, 55_000),
        "el pico, no 125.000"
    );
    assert_eq!(
        db::provider_tokens_since(&conn, "anthropic", 0).unwrap(),
        55_000
    );
}
