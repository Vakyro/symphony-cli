//! P03.S3: repositorios de Fase 1 contra una base temporal.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rusqlite::Connection;
use symphony_core::{
    AgentId, AgentState, ContextMode, ExecutionMode, FailoverPolicy, ProjectId, RunEndReason,
    RunId, RunStatus, SessionId, TaskId, TaskStatus, WorktreeId,
};
use symphony_store::repo::{self, RepoError};

struct Fixture {
    conn: Connection,
    project: ProjectId,
    session: SessionId,
}

fn fixture() -> Fixture {
    let conn = symphony_store::open_in_memory().unwrap();
    let project = ProjectId::new();
    repo::insert_project(
        &conn,
        &repo::Project {
            id: project,
            name: "demo".into(),
            root_path: "/repo".into(),
            default_branch: "main".into(),
            created_at: 1,
        },
    )
    .unwrap();
    let session = SessionId::new();
    repo::start_session(&conn, session, project, 4242, 1).unwrap();
    for (id, name) in [("anthropic", "Claude"), ("openai", "Codex")] {
        repo::upsert_provider(
            &conn,
            &repo::Provider {
                id: id.into(),
                display_name: name.into(),
                cli_name: name.to_lowercase(),
                cli_path: None,
                cli_version: None,
                setup_state: "READY".into(),
                hooks_supported: true,
                hooks_can_hold: Some(id == "anthropic"),
            },
            1,
        )
        .unwrap();
    }
    for (id, provider) in [("claude/sonnet", "anthropic"), ("openai/sol", "openai")] {
        let m = repo::Model {
            id: id.into(),
            provider_id: provider.into(),
            cli_model_id: id.into(),
            display_name: id.into(),
            context_window: None,
        };
        repo::upsert_model(&conn, &m, 1).unwrap();
    }
    Fixture {
        conn,
        project,
        session,
    }
}

fn task(f: &Fixture) -> TaskId {
    let id = TaskId::new();
    let t = repo::Task {
        id,
        project_id: f.project,
        code: repo::next_task_code(&f.conn, f.project).unwrap(),
        title: "Fix refresh token".into(),
        description: None,
        status: TaskStatus::Ready,
        status_reason: None,
        priority: 0,
    };
    repo::insert_task(&f.conn, &t, 2).unwrap();
    id
}

fn agent(f: &Fixture, task: TaskId) -> AgentId {
    let id = AgentId::new();
    let a = repo::Agent {
        id,
        project_id: f.project,
        session_id: f.session,
        task_id: task,
        worktree_id: None,
        number: repo::next_agent_number(&f.conn, f.project).unwrap(),
        state: AgentState::Created,
        state_reason: None,
        execution_mode: ExecutionMode::Exact,
        requested_model_id: Some("claude/sonnet".into()),
        requested_profile_id: None,
        failover_policy: FailoverPolicy::Any,
        context_mode: ContextMode::Balanced,
        priority: 0,
    };
    repo::insert_agent(&f.conn, &a, 3).unwrap();
    id
}

#[test]
fn projects() {
    let f = fixture();
    let p = repo::project_by_root(&f.conn, "/repo").unwrap().unwrap();
    assert_eq!(p.id, f.project);
    assert_eq!(repo::get_project(&f.conn, f.project).unwrap().name, "demo");
    assert!(repo::project_by_root(&f.conn, "/otro").unwrap().is_none());
    assert!(matches!(
        repo::get_project(&f.conn, ProjectId::new()),
        Err(RepoError::NotFound { .. })
    ));
    let dup = repo::Project {
        id: ProjectId::new(),
        name: "x".into(),
        root_path: "/repo".into(),
        default_branch: "main".into(),
        created_at: 1,
    };
    assert!(
        repo::insert_project(&f.conn, &dup).is_err(),
        "root_path es UNIQUE"
    );
}

#[test]
fn sessions() {
    let f = fixture();
    assert_eq!(
        repo::unclosed_sessions(&f.conn, f.project).unwrap(),
        vec![(f.session, Some(4242))]
    );
    repo::end_session(&f.conn, f.session, repo::SessionStatus::Closed, 9).unwrap();
    assert!(
        repo::unclosed_sessions(&f.conn, f.project)
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        repo::end_session(&f.conn, f.session, repo::SessionStatus::Closed, 10),
        Err(RepoError::NotFound { .. })
    ));
}

#[test]
fn tasks_and_status_transitions() {
    let f = fixture();
    let t1 = task(&f);
    let t2 = task(&f);
    assert_eq!(repo::get_task(&f.conn, t1).unwrap().code, "T-1");
    assert_eq!(repo::get_task(&f.conn, t2).unwrap().code, "T-2");

    assert_eq!(
        repo::set_task_status(&f.conn, t1, TaskStatus::Running, None, 5).unwrap(),
        TaskStatus::Ready
    );
    // Transición inválida: error tipado y el estado no cambia.
    let err = repo::set_task_status(&f.conn, t1, TaskStatus::Backlog, None, 6).unwrap_err();
    assert!(matches!(err, RepoError::Transition(_)), "{err}");
    assert_eq!(
        repo::get_task(&f.conn, t1).unwrap().status,
        TaskStatus::Running
    );

    repo::set_task_status(&f.conn, t1, TaskStatus::Done, None, 7).unwrap();
    let done: Option<i64> = f
        .conn
        .query_row(
            "SELECT completed_at FROM tasks WHERE id=?1",
            [t1.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(done, Some(7));
}

#[test]
fn agents_states_and_reasons() {
    let f = fixture();
    let t = task(&f);
    let a = agent(&f, t);
    assert_eq!(repo::get_agent(&f.conn, a).unwrap().number, 1);

    repo::set_agent_state(&f.conn, a, AgentState::Ready, None, 4).unwrap();
    repo::set_agent_state(&f.conn, a, AgentState::Running, None, 5).unwrap();
    // Los estados de espera exigen una frase humana (FLOW §7).
    assert!(matches!(
        repo::set_agent_state(&f.conn, a, AgentState::WaitingResource, None, 6),
        Err(RepoError::MissingReason { .. })
    ));
    repo::set_agent_state(
        &f.conn,
        a,
        AgentState::WaitingResource,
        Some("esperando el test suite de Agent #2"),
        6,
    )
    .unwrap();
    let got = repo::get_agent(&f.conn, a).unwrap();
    assert_eq!(got.state, AgentState::WaitingResource);
    assert_eq!(
        got.state_reason.as_deref(),
        Some("esperando el test suite de Agent #2")
    );

    // Inválida: WAITING_RESOURCE → COMPLETED no está en la tabla.
    assert!(matches!(
        repo::set_agent_state(&f.conn, a, AgentState::Completed, None, 7),
        Err(RepoError::Transition(_))
    ));
    assert_eq!(
        repo::get_agent(&f.conn, a).unwrap().state,
        AgentState::WaitingResource
    );

    // Home: agentes vivos, por número.
    let t2 = task(&f);
    let b = agent(&f, t2);
    let live: Vec<_> = repo::live_agents(&f.conn, f.project)
        .unwrap()
        .into_iter()
        .map(|a| a.id)
        .collect();
    assert_eq!(live, vec![a, b]);
    repo::set_agent_state(&f.conn, b, AgentState::Cancelled, None, 8).unwrap();
    assert_eq!(repo::live_agents(&f.conn, f.project).unwrap().len(), 1);
}

#[test]
fn worktrees() {
    let f = fixture();
    let t = task(&f);
    let a = agent(&f, t);
    let w = WorktreeId::new();
    repo::insert_worktree(
        &f.conn,
        &repo::Worktree {
            id: w,
            project_id: f.project,
            path: "/wt/agent-001".into(),
            branch: "symphony/s1/agent-001".into(),
            base_ref: "main".into(),
            deps_strategy: "PNPM_STORE".into(),
            status: "CREATING".into(),
        },
        3,
    )
    .unwrap();
    repo::set_agent_worktree(&f.conn, a, w, 4).unwrap();
    assert_eq!(repo::get_agent(&f.conn, a).unwrap().worktree_id, Some(w));
    repo::set_worktree_status(&f.conn, w, "READY", Some("abc123"), 5).unwrap();
    assert!(
        repo::set_worktree_status(&f.conn, w, "HAPPY", None, 6).is_err(),
        "CHECK de status"
    );
    repo::set_worktree_status(&f.conn, w, "REMOVED", None, 7).unwrap();
    let (status, head, removed): (String, String, Option<i64>) = f
        .conn
        .query_row(
            "SELECT status, head_commit, removed_at FROM worktrees WHERE id=?1",
            [w.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (status.as_str(), head.as_str(), removed),
        ("REMOVED", "abc123", Some(7))
    );
}

#[test]
fn providers_and_models_upsert() {
    let f = fixture();
    let mut m = repo::models_of(&f.conn, "anthropic").unwrap().remove(0);
    m.display_name = "Sonnet".into();
    repo::upsert_model(&f.conn, &m, 50).unwrap();
    let models = repo::models_of(&f.conn, "anthropic").unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].display_name, "Sonnet");
    let seen: i64 = f
        .conn
        .query_row(
            "SELECT last_seen_at FROM models WHERE id='claude/sonnet'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(seen, 50);
}

#[test]
fn runs_carry_the_model_not_the_agent() {
    let f = fixture();
    let t = task(&f);
    let a = agent(&f, t);

    let r1 = RunId::new();
    let run = repo::open_run(&f.conn, r1, a, "anthropic", "claude/sonnet", 10).unwrap();
    assert_eq!((run.seq, run.status), (1, RunStatus::Starting));
    repo::set_run_process(&f.conn, r1, 999, Some("cli-session"), Some("/t.jsonl")).unwrap();
    repo::heartbeat(&f.conn, r1, 11).unwrap();
    assert_eq!(
        repo::current_run(&f.conn, a).unwrap().unwrap().model_id,
        "claude/sonnet"
    );

    // Un solo executor vivo por agente.
    assert!(repo::open_run(&f.conn, RunId::new(), a, "openai", "openai/sol", 12).is_err());
    // No se puede "cerrar" un run dejándolo RUNNING.
    assert!(
        repo::close_run(
            &f.conn,
            r1,
            RunStatus::Running,
            RunEndReason::Completed,
            None,
            12
        )
        .is_err()
    );

    // Failover: se cierra el run y se abre otro con otro proveedor. El agente no cambia.
    let agent_before = repo::get_agent(&f.conn, a).unwrap();
    repo::close_run(
        &f.conn,
        r1,
        RunStatus::HandedOff,
        RunEndReason::QuotaExhausted,
        None,
        13,
    )
    .unwrap();
    assert!(repo::current_run(&f.conn, a).unwrap().is_none());
    let r2 = RunId::new();
    let run2 = repo::open_run(&f.conn, r2, a, "openai", "openai/sol", 14).unwrap();
    assert_eq!(run2.seq, 2);
    assert_eq!(
        repo::current_run(&f.conn, a).unwrap().unwrap().model_id,
        "openai/sol"
    );
    assert_eq!(repo::get_agent(&f.conn, a).unwrap(), agent_before);

    let history = repo::runs_of(&f.conn, a).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].end_reason, Some(RunEndReason::QuotaExhausted));
    assert!(matches!(
        repo::close_run(
            &f.conn,
            r1,
            RunStatus::Exited,
            RunEndReason::Completed,
            None,
            15
        ),
        Err(RepoError::NotFound { .. })
    ));
}

#[tokio::test]
async fn repositories_through_the_single_writer() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("symphony.db");
    let writer = symphony_store::Writer::start(&db).unwrap();
    let project = ProjectId::new();
    writer
        .handle()
        .write(Box::new(move |tx| {
            repo::insert_project(
                tx,
                &repo::Project {
                    id: project,
                    name: "w".into(),
                    root_path: "/w".into(),
                    default_branch: "main".into(),
                    created_at: 1,
                },
            )?;
            Ok(())
        }))
        .await
        .unwrap();
    let reader = writer.reader().unwrap();
    assert_eq!(repo::get_project(&reader, project).unwrap().root_path, "/w");
    writer.shutdown();
}

#[test]
fn home_rows_show_current_model_per_live_agent() {
    let f = fixture();
    let t1 = task(&f);
    let a1 = agent(&f, t1);
    let t2 = task(&f);
    let a2 = agent(&f, t2);
    repo::open_run(&f.conn, RunId::new(), a1, "anthropic", "claude/sonnet", 10).unwrap();
    let rows = repo::home_rows(&f.conn, f.project).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].agent_id, rows[0].model_id.as_deref()),
        (a1, Some("claude/sonnet"))
    );
    assert_eq!((rows[1].agent_id, rows[1].model_id.as_deref()), (a2, None));
    assert_eq!(rows[0].task_code, "T-1");
    repo::set_agent_state(&f.conn, a2, AgentState::Cancelled, None, 11).unwrap();
    assert_eq!(repo::home_rows(&f.conn, f.project).unwrap().len(), 1);
}
