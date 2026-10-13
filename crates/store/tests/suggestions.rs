//! Sugerencias de modelo y desempeño aprendido en el almacén (P14.S1, S3).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rusqlite::Connection;
use symphony_core::{AgentId, ProjectId, SuggestionStatus, learning};
use symphony_store::open_in_memory;
use symphony_store::suggestions::{
    NewSuggestion, get, insert, is_muted, pending_of, pending_of_project, record_outcome, resolve,
};

fn seeded() -> (Connection, AgentId, ProjectId) {
    let conn = open_in_memory().unwrap();
    let (agent, project) = (AgentId::new(), ProjectId::new());
    conn.execute_batch(&format!(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('{project}','d','/r','main',0);
         INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','{project}','ACTIVE',0);
         INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('t1','{project}','T-1','WORK','x','RUNNING',0,0);
         INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, failover_policy, context_mode, created_at, updated_at)
             VALUES ('{agent}','{project}','s1','t1',1,'RUNNING','DECIDE_LATER','ANY','BALANCED',0,0);
         INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('anthropic','Claude','claude','CLI','READY');
         INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at) VALUES
            ('claude/haiku','anthropic','haiku','Haiku',0), ('claude/sonnet','anthropic','sonnet','Sonnet',0);"
    ))
    .unwrap();
    (conn, agent, project)
}

fn new(agent: AgentId, id: &str) -> NewSuggestion {
    NewSuggestion {
        id: id.into(),
        agent_id: agent,
        current_model_id: "claude/haiku".into(),
        suggested_model_id: "claude/sonnet".into(),
        reason: "documentación → refactor de varios archivos".into(),
        confidence: 0.8,
    }
}

#[test]
fn a_suggestion_is_pending_until_the_user_resolves_it_and_there_is_only_one_at_a_time() {
    let (conn, agent, project) = seeded();
    assert!(insert(&conn, &new(agent, "s1"), 10).unwrap());
    // Una pendiente por agente: la segunda no entra.
    assert!(!insert(&conn, &new(agent, "s2"), 11).unwrap());
    let p = pending_of(&conn, agent).unwrap().unwrap();
    assert_eq!((p.id.as_str(), p.status), ("s1", SuggestionStatus::Pending));
    assert_eq!(p.confidence, Some(0.8));
    assert_eq!(pending_of_project(&conn, project).unwrap().len(), 1);

    assert!(resolve(&conn, "s1", SuggestionStatus::Kept, 20).unwrap());
    // Ya resuelta: no se resuelve dos veces ni vuelve a pendiente.
    assert!(!resolve(&conn, "s1", SuggestionStatus::Accepted, 21).unwrap());
    assert!(!resolve(&conn, "s1", SuggestionStatus::Pending, 22).unwrap());
    let done = get(&conn, "s1").unwrap().unwrap();
    assert_eq!(
        (done.status, done.resolved_at),
        (SuggestionStatus::Kept, Some(20))
    );
    assert!(pending_of(&conn, agent).unwrap().is_none());
    // Con la anterior resuelta entra otra, si es otra sugerencia (no la misma que se mantuvo).
    let mut s3 = new(agent, "s3");
    s3.suggested_model_id = "claude/haiku".into();
    s3.current_model_id = "claude/sonnet".into();
    assert!(insert(&conn, &s3, 30).unwrap());
}

#[test]
fn muting_stops_every_later_suggestion_for_that_agent() {
    let (conn, agent, _) = seeded();
    insert(&conn, &new(agent, "s1"), 10).unwrap();
    assert!(!is_muted(&conn, agent).unwrap());
    resolve(&conn, "s1", SuggestionStatus::Muted, 11).unwrap();
    assert!(is_muted(&conn, agent).unwrap());
    assert!(!insert(&conn, &new(agent, "s2"), 12).unwrap());
    assert!(pending_of(&conn, agent).unwrap().is_none());
}

#[test]
fn a_learned_adjustment_appears_only_after_the_minimum_of_samples() {
    let (conn, _, _) = seeded();
    conn.execute_batch(
        "INSERT INTO profile_models (profile_id, model_id, base_score) VALUES ('@code','claude/haiku',0.6);",
    )
    .unwrap();
    let state = |conn: &Connection| -> (i64, f64, Option<f64>) {
        conn.query_row(
            "SELECT sample_count, outcome_score_sum, learned_adjustment FROM profile_models
             WHERE profile_id = '@code' AND model_id = 'claude/haiku'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    };
    for i in 1..learning::MIN_SAMPLES {
        record_outcome(
            &conn,
            &format!("run{i}"),
            "@code",
            "claude/haiku",
            learning::Outcome::Clean,
        )
        .unwrap();
        let (n, sum, adj) = state(&conn);
        assert_eq!(
            (n, sum, adj),
            (i, i as f64, None),
            "antes del mínimo no hay ajuste"
        );
    }
    record_outcome(
        &conn,
        "run-10",
        "@code",
        "claude/haiku",
        learning::Outcome::Clean,
    )
    .unwrap();
    let (n, _, adj) = state(&conn);
    assert_eq!(n, learning::MIN_SAMPLES);
    let adj = adj.expect("con las muestras mínimas ya hay ajuste");
    assert!(adj > 0.0 && adj <= learning::MAX_ADJUSTMENT, "{adj}");

    // Malos resultados lo bajan, sin pasar del tope.
    for i in 0..200 {
        record_outcome(
            &conn,
            &format!("fail{i}"),
            "@code",
            "claude/haiku",
            learning::Outcome::Failed,
        )
        .unwrap();
    }
    let (_, _, adj) = state(&conn);
    assert_eq!(adj, Some(-learning::MAX_ADJUSTMENT));
    // Un par (profile, modelo) que no existe no hace nada ni falla.
    record_outcome(
        &conn,
        "run-x",
        "@nada",
        "claude/haiku",
        learning::Outcome::Clean,
    )
    .unwrap();
    // El mismo run no suma dos veces.
    let before = state(&conn);
    record_outcome(
        &conn,
        "fail0",
        "@code",
        "claude/haiku",
        learning::Outcome::Failed,
    )
    .unwrap();
    assert_eq!(state(&conn), before, "una muestra por run");
}

#[test]
fn keeping_a_suggestion_stops_the_same_one_from_coming_back() {
    let (conn, agent, _) = seeded();
    insert(&conn, &new(agent, "s1"), 10).unwrap();
    resolve(&conn, "s1", SuggestionStatus::Kept, 11).unwrap();
    // El mismo modelo sugerido no vuelve...
    assert!(!insert(&conn, &new(agent, "s2"), 12).unwrap());
    // ...pero otro modelo sugerido sí puede (no es la misma sugerencia).
    let mut other = new(agent, "s3");
    other.suggested_model_id = "claude/haiku".into();
    other.current_model_id = "claude/sonnet".into();
    assert!(insert(&conn, &other, 13).unwrap());
}
