//! DAG de tareas, reacciones y milestones (P08.S5).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rusqlite::{Connection, params};
use symphony_core::{AgentId, AgentState, MilestoneStatus, ProjectId, TaskId, TaskStatus};
use symphony_store::deps::{self, DepError, Finished, Reaction};
use symphony_store::open_in_memory;

struct World {
    conn: Connection,
    project: ProjectId,
}

fn world() -> World {
    let conn = open_in_memory().unwrap();
    let project = ProjectId::new();
    conn.execute_batch(&format!(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('{project}','d','/r','main',0);
         INSERT INTO sessions (id, project_id, status, started_at) VALUES ('{}','{project}','ACTIVE',0);
         INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('anthropic','C','claude','CLI','READY');
         INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at) VALUES ('claude/sonnet','anthropic','sonnet','S',0);",
        symphony_core::SessionId::new()
    ))
    .unwrap();
    World { conn, project }
}

impl World {
    /// Una tarea con su agente en `state`.
    fn task(&self, n: u32, state: AgentState, status: TaskStatus) -> (TaskId, AgentId) {
        let (task, agent) = (TaskId::new(), AgentId::new());
        let session: String = self
            .conn
            .query_row("SELECT id FROM sessions", [], |r| r.get(0))
            .unwrap();
        self.conn
            .execute(
                "INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'WORK', ?4, ?5, 0, 0)",
                params![
                    task.to_string(),
                    self.project.to_string(),
                    format!("T-{n}"),
                    format!("tarea {n}"),
                    status.as_str()
                ],
            )
            .unwrap();
        self.conn
            .execute(
                "INSERT INTO agents (id, project_id, session_id, task_id, number, state, state_reason, execution_mode,
                                     requested_model_id, failover_policy, context_mode, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'EXACT', 'claude/sonnet', 'ANY', 'BALANCED', 0, 0)",
                params![
                    agent.to_string(),
                    self.project.to_string(),
                    session,
                    task.to_string(),
                    n,
                    state.as_str(),
                    state.requires_reason().then_some("x")
                ],
            )
            .unwrap();
        (task, agent)
    }

    fn finish(&self, task: TaskId, agent: AgentId, ok: bool, status: TaskStatus) {
        let to = if ok {
            AgentState::Completed
        } else {
            AgentState::Failed
        };
        self.conn
            .execute(
                "UPDATE agents SET state = ?2, state_reason = ?3 WHERE id = ?1",
                params![agent.to_string(), to.as_str(), (!ok).then_some("falló")],
            )
            .unwrap();
        self.conn
            .execute(
                "UPDATE tasks SET status = ?2 WHERE id = ?1",
                params![task.to_string(), status.as_str()],
            )
            .unwrap();
    }

    fn agent_state(&self, agent: AgentId) -> (String, Option<String>) {
        self.conn
            .query_row(
                "SELECT state, state_reason FROM agents WHERE id = ?1",
                [agent.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    fn task_status(&self, task: TaskId) -> String {
        self.conn
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                [task.to_string()],
                |r| r.get(0),
            )
            .unwrap()
    }
}

#[test]
fn a_ready_task_with_an_unmet_dependency_waits_and_says_for_what() {
    let w = world();
    let (a, _) = w.task(12, AgentState::Running, TaskStatus::Running);
    let (b, _) = w.task(16, AgentState::Running, TaskStatus::Running);
    let (c, c_agent) = w.task(18, AgentState::Ready, TaskStatus::Ready);

    assert!(deps::add_dependency(&w.conn, c, a, 1).unwrap().waiting);
    assert!(deps::add_dependency(&w.conn, c, b, 2).unwrap().waiting);
    let (state, reason) = w.agent_state(c_agent);
    assert_eq!(state, "WAITING_DEPENDENCY");
    assert_eq!(reason.as_deref(), Some("espera a T-12 y T-16"));
    assert_eq!(w.task_status(c), "WAITING");
    assert_eq!(deps::unmet_dependencies(&w.conn, c).unwrap().len(), 2);
}

#[test]
fn cycles_self_dependencies_and_other_projects_are_rejected() {
    let w = world();
    let (a, _) = w.task(1, AgentState::Ready, TaskStatus::Ready);
    let (b, _) = w.task(2, AgentState::Ready, TaskStatus::Ready);
    let (c, _) = w.task(3, AgentState::Ready, TaskStatus::Ready);
    deps::add_dependency(&w.conn, b, a, 1).unwrap();
    deps::add_dependency(&w.conn, c, b, 2).unwrap();

    assert!(matches!(
        deps::add_dependency(&w.conn, a, c, 3),
        Err(DepError::Cycle { .. })
    ));
    assert!(matches!(
        deps::add_dependency(&w.conn, a, a, 3),
        Err(DepError::SelfDependency)
    ));
    // Un atajo no es un ciclo.
    deps::add_dependency(&w.conn, c, a, 4).unwrap();

    // Otro proyecto.
    let other = ProjectId::new();
    w.conn
        .execute_batch(&format!(
            "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('{other}','o','/o','main',0);"
        ))
        .unwrap();
    let foreign = TaskId::new();
    w.conn
        .execute(
            "INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at)
             VALUES (?1, ?2, 'T-1', 'WORK', 'x', 'READY', 0, 0)",
            params![foreign.to_string(), other.to_string()],
        )
        .unwrap();
    assert!(matches!(
        deps::add_dependency(&w.conn, a, foreign, 5),
        Err(DepError::OtherProject)
    ));
}

#[test]
fn finishing_the_last_dependency_releases_the_agent_to_ready() {
    let w = world();
    let (a, a_agent) = w.task(1, AgentState::Running, TaskStatus::Running);
    let (b, b_agent) = w.task(2, AgentState::Running, TaskStatus::Running);
    let (c, c_agent) = w.task(3, AgentState::Ready, TaskStatus::Ready);
    deps::add_dependency(&w.conn, c, a, 1).unwrap();
    deps::add_dependency(&w.conn, c, b, 2).unwrap();

    w.finish(a, a_agent, true, TaskStatus::Done);
    let r = deps::reconcile_dependents(&w.conn, a, Finished::Done, 10).unwrap();
    assert!(r.is_empty(), "todavía espera a T-2");
    assert_eq!(
        w.agent_state(c_agent).1.as_deref(),
        Some("espera a T-2"),
        "la frase se actualiza"
    );

    w.finish(b, b_agent, true, TaskStatus::Done);
    let r = deps::reconcile_dependents(&w.conn, b, Finished::Done, 20).unwrap();
    assert_eq!(
        r,
        vec![Reaction::Released {
            agent: c_agent,
            task: c
        }]
    );
    assert_eq!(w.agent_state(c_agent).0, "READY");
    assert_eq!(w.task_status(c), "READY");
    // Idempotente: reevaluar no vuelve a liberarlo.
    assert!(
        deps::reconcile_dependents(&w.conn, b, Finished::Done, 30)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_failed_dependency_blocks_the_dependent_and_opens_a_recovery_item() {
    let w = world();
    let (a, a_agent) = w.task(1, AgentState::Running, TaskStatus::Running);
    let (c, c_agent) = w.task(3, AgentState::Ready, TaskStatus::Ready);
    deps::add_dependency(&w.conn, c, a, 1).unwrap();

    w.finish(a, a_agent, false, TaskStatus::Failed);
    let r = deps::reconcile_dependents(&w.conn, a, Finished::Failed, 10).unwrap();
    assert!(matches!(&r[..], [Reaction::Blocked { because, .. }] if because.contains("T-1 falló")));
    let (state, reason) = w.agent_state(c_agent);
    assert_eq!(state, "BLOCKED");
    assert!(
        reason
            .unwrap()
            .contains("replanificar, ignorar la dependencia o cancelar")
    );
    assert_eq!(w.task_status(c), "BLOCKED");
    let kind: String = w
        .conn
        .query_row("SELECT kind FROM recovery_items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kind, "DEPENDENCY_FAILED");

    // «Ignorar la dependencia»: se quita la arista y ya no espera a nadie.
    assert!(deps::remove_dependency(&w.conn, c, a).unwrap());
    assert!(deps::unmet_dependencies(&w.conn, c).unwrap().is_empty());
}

#[test]
fn a_milestone_goes_to_review_when_its_tasks_are_done_and_waits_for_a_decision() {
    let w = world();
    let (a, _) = w.task(1, AgentState::Completed, TaskStatus::Running);
    let (b, _) = w.task(2, AgentState::Completed, TaskStatus::Running);
    let m = deps::create_milestone(&w.conn, w.project, "Authentication complete", true).unwrap();
    deps::assign_task(&w.conn, a, Some(m), 1).unwrap();
    deps::assign_task(&w.conn, b, Some(m), 1).unwrap();
    assert_eq!(deps::milestone_of_task(&w.conn, a).unwrap(), Some(m));

    assert_eq!(deps::evaluate_milestone(&w.conn, m, 5).unwrap(), None);
    w.conn
        .execute(
            "UPDATE tasks SET status = 'DONE' WHERE id = ?1",
            [a.to_string()],
        )
        .unwrap();
    assert_eq!(
        deps::evaluate_milestone(&w.conn, m, 6).unwrap(),
        None,
        "falta T-2"
    );
    w.conn
        .execute(
            "UPDATE tasks SET status = 'DONE' WHERE id = ?1",
            [b.to_string()],
        )
        .unwrap();
    assert_eq!(
        deps::evaluate_milestone(&w.conn, m, 7).unwrap(),
        Some(MilestoneStatus::InReview)
    );
    let got = deps::get_milestone(&w.conn, m).unwrap();
    assert_eq!((got.tasks_done, got.tasks_total), (2, 2));

    // Pedir cambios y volver a completar lo devuelve a revisión; aprobar lo cierra.
    deps::decide_milestone(&w.conn, m, MilestoneStatus::ChangesRequested, 8).unwrap();
    assert_eq!(
        deps::evaluate_milestone(&w.conn, m, 9).unwrap(),
        Some(MilestoneStatus::InReview)
    );
    deps::decide_milestone(&w.conn, m, MilestoneStatus::Approved, 10).unwrap();
    let got = deps::get_milestone(&w.conn, m).unwrap();
    assert_eq!(
        (got.status, got.decided_at),
        (MilestoneStatus::Approved, Some(10))
    );
    // Ya aprobado: no se decide de nuevo.
    assert!(deps::decide_milestone(&w.conn, m, MilestoneStatus::Held, 11).is_err());

    // Sin aprobación manual, pasa directo a APPROVED.
    let (c, _) = w.task(3, AgentState::Completed, TaskStatus::Done);
    let free = deps::create_milestone(&w.conn, w.project, "Sin gate", false).unwrap();
    deps::assign_task(&w.conn, c, Some(free), 12).unwrap();
    assert_eq!(
        deps::evaluate_milestone(&w.conn, free, 13).unwrap(),
        Some(MilestoneStatus::Approved)
    );
}
