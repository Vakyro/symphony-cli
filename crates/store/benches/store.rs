//! P03.S6: línea base del store (STACK §25.1). `cargo bench -p symphony-store`.
// Código de bench: mismas reglas que los tests (CONSTRAINTS C3).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use symphony_core::{AgentId, ProjectId, RunId, SessionId, TaskId};
use symphony_store::{NewEvent, Writer, repo};

fn event(i: usize) -> NewEvent {
    NewEvent {
        project_id: "p1".into(),
        agent_id: None,
        run_id: None,
        event_type: "CommandFinished".into(),
        source: "HOOK".into(),
        payload_json: Some(format!(r#"{{"cmd":"npm test","exit":0,"i":{i}}}"#)),
        occurred_at: i as i64,
    }
}

fn fresh_db() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("bench.db");
    let conn = symphony_store::open(&db).unwrap();
    conn.execute("INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','b','/b','main',0)", [])
        .unwrap();
    (dir, db)
}

fn insert_events(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut group = c.benchmark_group("events_insert");
    group
        .sample_size(10)
        .measurement_time(Duration::from_secs(20));
    for n in [10_000usize, 100_000] {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_function(format!("{n}"), |b| {
            b.iter_batched(
                fresh_db,
                |(_dir, db)| {
                    let writer = Writer::start(&db).unwrap();
                    let handle = writer.handle();
                    rt.block_on(async {
                        for i in 0..n {
                            handle.event(event(i)).await.unwrap();
                        }
                        assert_eq!(handle.flush().await.unwrap().events_failed, 0);
                    });
                    writer.shutdown();
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

/// Home query con 50 agentes vivos (cada uno con su run abierto) y 100k eventos.
fn home_query(c: &mut Criterion) {
    let (_dir, db) = fresh_db();
    let conn = symphony_store::open(&db).unwrap();
    let project = ProjectId::new();
    conn.execute_batch("BEGIN").unwrap();
    repo::insert_project(
        &conn,
        &repo::Project {
            id: project,
            name: "h".into(),
            root_path: "/h".into(),
            default_branch: "main".into(),
            created_at: 0,
        },
    )
    .unwrap();
    let session = SessionId::new();
    repo::start_session(&conn, session, project, 1, 0).unwrap();
    conn.execute("INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('anthropic','Claude','claude','CLI','READY')", []).unwrap();
    conn.execute("INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at) VALUES ('claude/sonnet','anthropic','sonnet','Sonnet',0)", []).unwrap();
    for n in 1..=50 {
        let task = TaskId::new();
        repo::insert_task(
            &conn,
            &repo::Task {
                id: task,
                project_id: project,
                code: format!("T-{n}"),
                title: "t".into(),
                description: None,
                status: symphony_core::TaskStatus::Running,
                status_reason: None,
                priority: 0,
            },
            0,
        )
        .unwrap();
        let agent = AgentId::new();
        repo::insert_agent(
            &conn,
            &repo::Agent {
                id: agent,
                project_id: project,
                session_id: session,
                task_id: task,
                worktree_id: None,
                number: n,
                state: symphony_core::AgentState::Running,
                state_reason: None,
                execution_mode: symphony_core::ExecutionMode::DecideLater,
                requested_model_id: None,
                requested_profile_id: None,
                failover_policy: symphony_core::FailoverPolicy::Any,
                context_mode: symphony_core::ContextMode::Balanced,
                priority: 0,
            },
            0,
        )
        .unwrap();
        repo::open_run(&conn, RunId::new(), agent, "anthropic", "claude/sonnet", 0).unwrap();
    }
    let mut stmt = conn.prepare("INSERT INTO events (project_id, type, source, payload_json, occurred_at) VALUES ('p1','X','HOOK','{}',?1)").unwrap();
    for i in 0..100_000i64 {
        stmt.execute([i]).unwrap();
    }
    drop(stmt);
    conn.execute_batch("COMMIT").unwrap();
    drop(conn);

    let reader = symphony_store::open_reader(&db).unwrap();
    assert_eq!(repo::home_rows(&reader, project).unwrap().len(), 50);
    c.bench_function("home_query_50_agents_100k_events", |b| {
        b.iter(|| repo::home_rows(&reader, project).unwrap())
    });
}

criterion_group!(benches, insert_events, home_query);
criterion_main!(benches);
