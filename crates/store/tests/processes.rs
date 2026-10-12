//! Árbol de procesos: altas, picos, cierres y atribución a la tool call (P08.S4).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use symphony_core::RunId;
use symphony_store::open_in_memory;
use symphony_store::processes::{ProcSample, close_processes, processes_of, sync_processes};

fn sample(pid: u32, parent: Option<u32>, cmd: &str, rss: u64, cpu: u64) -> ProcSample {
    ProcSample {
        pid,
        parent_pid: parent,
        command: cmd.into(),
        rss_bytes: rss,
        cpu_time_ms: cpu,
    }
}

fn seeded(run: RunId) -> rusqlite::Connection {
    let conn = open_in_memory().unwrap();
    let agent = symphony_core::AgentId::new();
    conn.execute_batch(&format!(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','d','/r','main',0);
         INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','p1','ACTIVE',0);
         INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('t1','p1','T-1','WORK','x','RUNNING',0,0);
         INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('anthropic','C','claude','CLI','READY');
         INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at) VALUES ('claude/sonnet','anthropic','sonnet','S',0);
         INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, requested_model_id, failover_policy, context_mode, created_at, updated_at)
             VALUES ('{agent}','p1','s1','t1',1,'RUNNING','EXACT','claude/sonnet','ANY','BALANCED',0,0);
         INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at)
             VALUES ('{run}','{agent}',1,'anthropic','claude/sonnet','RUNNING',0);
         INSERT INTO tool_calls (id, agent_id, run_id, tool_name, command, op_class, status, requested_at)
             VALUES ('tc1','{agent}','{run}','Bash','pnpm test',3,'RUNNING',1);"
    ))
    .unwrap();
    conn
}

#[test]
fn a_tree_is_recorded_updated_and_closed() {
    let run = RunId::new();
    let conn = seeded(run);

    // Foto 1: el CLI (100) con un hijo (200) y un nieto (300) que aparece antes en el vector.
    sync_processes(
        &conn,
        run,
        &[
            sample(300, Some(200), "node test.js", 30, 5),
            sample(100, None, "claude", 100, 10),
            sample(200, Some(100), "pnpm test", 50, 20),
        ],
        1_000,
    )
    .unwrap();
    let rows = processes_of(&conn, run).unwrap();
    assert_eq!(rows.len(), 3);
    let by_pid = |pid: u32| rows.iter().find(|r| r.pid == pid).unwrap();
    assert_eq!(by_pid(100).parent_pid, None);
    assert_eq!(by_pid(200).parent_pid, Some(100));
    assert_eq!(by_pid(300).parent_pid, Some(200));
    // Con una sola tool call corriendo, los procesos nuevos se le atribuyen.
    let attributed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM processes WHERE tool_call_id = 'tc1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attributed, 3);

    // Foto 2: el nieto terminó; el hijo subió su memoria y bajó después (el pico se queda).
    sync_processes(
        &conn,
        run,
        &[
            sample(100, None, "claude", 90, 40),
            sample(200, Some(100), "pnpm test", 400, 90),
        ],
        2_000,
    )
    .unwrap();
    sync_processes(
        &conn,
        run,
        &[
            sample(100, None, "claude", 90, 50),
            sample(200, Some(100), "pnpm test", 60, 95),
        ],
        3_000,
    )
    .unwrap();
    let rows = processes_of(&conn, run).unwrap();
    assert_eq!(rows.len(), 3, "sin duplicados");
    assert_eq!(by_pid_row(&rows, 300).ended_at, Some(2_000));
    let hijo = by_pid_row(&rows, 200);
    assert_eq!((hijo.peak_rss_bytes, hijo.cpu_time_ms), (400, 95));
    assert_eq!(hijo.ended_at, None);

    // Al terminar el run, todo lo abierto se cierra.
    close_processes(&conn, run, 4_000).unwrap();
    let rows = processes_of(&conn, run).unwrap();
    assert!(rows.iter().all(|r| r.ended_at.is_some()));
    assert_eq!(by_pid_row(&rows, 300).ended_at, Some(2_000), "no se pisa");
}

fn by_pid_row(
    rows: &[symphony_store::processes::ProcessRow],
    pid: u32,
) -> &symphony_store::processes::ProcessRow {
    rows.iter().find(|r| r.pid == pid).unwrap()
}

#[test]
fn a_reused_pid_with_another_command_opens_a_new_row() {
    let run = RunId::new();
    let conn = seeded(run);
    sync_processes(&conn, run, &[sample(500, None, "node a.js", 10, 1)], 1_000).unwrap();
    sync_processes(
        &conn,
        run,
        &[sample(500, None, "python b.py", 20, 2)],
        2_000,
    )
    .unwrap();
    let rows = processes_of(&conn, run).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].ended_at, Some(2_000));
    assert_eq!(rows[1].command.as_deref(), Some("python b.py"));
    assert_eq!(rows[1].ended_at, None);
}
