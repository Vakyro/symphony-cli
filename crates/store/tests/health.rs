//! Salud, cuota y uso en la base (migración 002).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rusqlite::Connection;
use symphony_core::{
    FailureType, Health, HealthEvent, ProviderState, QuotaCertainty, RunId, UsageSource,
};
use symphony_store::health as db;

fn seeded() -> Connection {
    let conn = symphony_store::open_in_memory().unwrap();
    conn.execute_batch(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','demo','/repo','main',0);
         INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','p1','ACTIVE',0);
         INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('t1','p1','T-1','WORK','uno','RUNNING',0,0);
         INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('anthropic','Claude','claude','CLI','READY');
         INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at) VALUES ('claude/sonnet','anthropic','sonnet','Sonnet',0), ('claude/haiku','anthropic','haiku','Haiku',0);
         INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, requested_model_id, failover_policy, context_mode, created_at, updated_at)
             VALUES ('a1','p1','s1','t1',1,'RUNNING','EXACT','claude/sonnet','ANY','BALANCED',0,0);",
    )
    .unwrap();
    db::ensure_account(&conn, "anthropic", 0).unwrap();
    conn
}

fn run(conn: &Connection, id: &str) -> RunId {
    conn.execute(
        "INSERT INTO agent_runs (id, agent_id, seq, provider_id, account_id, model_id, status, started_at)
         VALUES (?1, 'a1', (SELECT COUNT(*) + 1 FROM agent_runs), 'anthropic', 'acct-anthropic', 'claude/sonnet', 'EXITED', 0)",
        [id],
    )
    .unwrap();
    id.parse().unwrap_or_else(|_| RunId::new())
}

#[test]
fn ensure_account_is_idempotent_and_creates_the_provider_level_row() {
    let conn = seeded();
    db::ensure_account(&conn, "anthropic", 5).unwrap();
    let accounts: i64 = conn
        .query_row("SELECT COUNT(*) FROM provider_accounts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(accounts, 1);
    let h = db::get_health(&conn, "anthropic", None).unwrap().unwrap();
    assert_eq!(h.state, ProviderState::Unknown);
    assert_eq!(db::list_health(&conn).unwrap().len(), 1);
}

#[test]
fn provider_and_model_scopes_are_independent_rows() {
    let conn = seeded();
    let limited = Health::unknown(10).apply(
        &HealthEvent::Failure {
            kind: FailureType::ModelLimit,
            retry_after_at: None,
            reset_at: Some(99_999),
            recent_same: 0,
        },
        10,
    );
    db::put_health(&conn, "anthropic", Some("claude/sonnet"), &limited).unwrap();
    // Reemplazar no duplica.
    db::put_health(&conn, "anthropic", Some("claude/sonnet"), &limited).unwrap();

    assert_eq!(db::list_health(&conn).unwrap().len(), 2);
    let provider = db::get_health(&conn, "anthropic", None).unwrap().unwrap();
    let model = db::get_health(&conn, "anthropic", Some("claude/sonnet"))
        .unwrap()
        .unwrap();
    assert_eq!(provider.state, ProviderState::Unknown);
    assert_eq!(model.state, ProviderState::Exhausted);
    assert_eq!(model.reset_at, Some(99_999));
    assert!(
        db::get_health(&conn, "anthropic", Some("claude/haiku"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn known_quota_roundtrips_and_the_check_rejects_a_guessed_percentage() {
    let conn = seeded();
    let known = Health {
        certainty: QuotaCertainty::Known,
        remaining: Some(0.35),
        ..Health::unknown(1)
    };
    db::put_health(&conn, "anthropic", None, &known).unwrap();
    let back = db::get_health(&conn, "anthropic", None).unwrap().unwrap();
    assert_eq!(back.remaining, Some(0.35));
    assert_eq!(back.certainty, QuotaCertainty::Known);

    let guessed = Health {
        certainty: QuotaCertainty::Estimated,
        remaining: Some(0.35),
        ..Health::unknown(2)
    };
    assert!(db::put_health(&conn, "anthropic", None, &guessed).is_err());
}

#[test]
fn recent_failures_are_counted_by_kind_scope_and_time() {
    let conn = seeded();
    let r = run(&conn, "01J0000000000000000000R001");
    let ins = |id: &str, kind: &str, model: Option<&str>, at: i64| {
        conn.execute(
            "INSERT INTO provider_failures (id, provider_id, account_id, model_id, run_id, failure_type, occurred_at)
             VALUES (?1,'anthropic','acct-anthropic',?2,?3,?4,?5)",
            rusqlite::params![id, model, r.to_string(), kind, at],
        )
        .unwrap();
    };
    ins("f1", "TEMP_RATE_LIMIT", Some("claude/sonnet"), 100);
    ins("f2", "TEMP_RATE_LIMIT", Some("claude/haiku"), 200);
    ins("f3", "TEMP_RATE_LIMIT", Some("claude/sonnet"), 50);
    ins("f4", "AUTH", None, 300);

    let n = |model: Option<&str>, kind, since| {
        db::recent_failure_count(&conn, "anthropic", model, kind, since).unwrap()
    };
    assert_eq!(n(None, FailureType::TempRateLimit, 60), 2);
    assert_eq!(n(None, FailureType::TempRateLimit, 0), 3);
    assert_eq!(n(Some("claude/sonnet"), FailureType::TempRateLimit, 0), 2);
    assert_eq!(n(None, FailureType::Auth, 0), 1);
    let by_model = db::recent_failures_by_model(&conn, 60).unwrap();
    assert!(by_model.contains(&("anthropic".into(), Some("claude/sonnet".into()), 1)));
    assert!(by_model.contains(&("anthropic".into(), None, 1)));
}

#[test]
fn quota_windows_come_from_the_latest_quota_events_of_the_provider() {
    let conn = seeded();
    let r = run(&conn, "01J0000000000000000000R002");
    let ev = |window: &str, used: f64| {
        conn.execute(
            "INSERT INTO events (project_id, agent_id, run_id, type, source, payload_json, occurred_at)
             VALUES ('p1','a1',?1,'QuotaUpdated','JSON_STREAM',?2,0)",
            rusqlite::params![
                r.to_string(),
                format!(r#"{{"event":"quota","window":"{window}","used_fraction":{used},"resets_at":1700}}"#)
            ],
        )
        .unwrap();
    };
    ev("five_hour", 0.2);
    ev("seven_day", 0.5);
    ev("five_hour", 0.6); // el más reciente de la ventana gana
    let mut windows = db::latest_quota_windows(&conn, "anthropic").unwrap();
    windows.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        windows,
        vec![
            ("five_hour".to_string(), 0.6, Some(1700)),
            ("seven_day".to_string(), 0.5, Some(1700))
        ]
    );
    assert!(
        db::latest_quota_windows(&conn, "openai")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn usage_is_recorded_aggregated_and_estimated_from_messages() {
    let conn = seeded();
    let r = run(&conn, "01J0000000000000000000R003");
    conn.execute_batch(&format!(
        "INSERT INTO messages (id, agent_id, run_id, role, content, created_at) VALUES
           ('m1','a1','{r}','USER','{}',1), ('m2','a1','{r}','ASSISTANT','{}',2);",
        "x".repeat(400),
        "y".repeat(80)
    ))
    .unwrap();
    assert!(!db::run_has_reported_usage(&conn, r).unwrap());
    assert_eq!(db::estimate_run_tokens(&conn, r).unwrap(), (100, 20));

    let usage = |source, tin| db::NewUsage {
        run_id: r,
        provider_id: "anthropic".into(),
        model_id: "claude/sonnet".into(),
        tokens_in: Some(tin),
        tokens_out: None,
        source,
    };
    db::insert_usage(&conn, &usage(UsageSource::Reported, 1_000), 10).unwrap();
    db::insert_usage(&conn, &usage(UsageSource::Reported, 500), 11).unwrap();
    db::insert_usage(&conn, &usage(UsageSource::Estimated, 100), 12).unwrap();
    assert!(db::run_has_reported_usage(&conn, r).unwrap());

    let rows = db::usage_since(&conn, 0).unwrap();
    assert_eq!(rows.len(), 2, "REPORTED y ESTIMATED por separado");
    let reported = rows.iter().find(|u| u.source == "REPORTED").unwrap();
    assert_eq!((reported.runs, reported.tokens_in), (2, 1_500));
    assert_eq!(
        db::provider_tokens_since(&conn, "anthropic", 11).unwrap(),
        600
    );
}
