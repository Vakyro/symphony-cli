//! P06.S2: el recorder deduplica lo que llega por hook y por stream, y redacta comandos.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use symphony_adapter_common::{AgentEvent, ToolKind};
use symphony_core::{AgentId, ProjectId, RunId};
use symphony_daemon::bus::{BusEvent, EventBus, EventSource};
use symphony_object_store::ObjectStore;
use symphony_store::Writer;

#[tokio::test(flavor = "multi_thread")]
async fn duplicates_from_hook_and_stream_are_recorded_once() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    // IDs ULID: el recorder descarta los que no lo son.
    let (project, agent, run) = (ProjectId::new(), AgentId::new(), RunId::new());
    symphony_store::open(&db)
        .unwrap()
        .execute_batch(&format!(
            "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('{project}','p','/p','main',0);
             INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','{project}','ACTIVE',0);
             INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('t1','{project}','T-1','WORK','t','RUNNING',0,0);
             INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, failover_policy, context_mode, created_at, updated_at)
                 VALUES ('{agent}','{project}','s1','t1',1,'RUNNING','DECIDE_LATER','ANY','BALANCED',0,0);
             INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('fake','Fake','fake-agent','CLI','READY');
             INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at, last_seen_at) VALUES ('fake/fast','fake','fast','Fast',0,0);
             INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at) VALUES ('{run}','{agent}',1,'fake','fake/fast','RUNNING',0);"
        ))
        .unwrap();
    let writer = Writer::start(&db).unwrap();
    let bus = EventBus::new(
        writer.handle(),
        16,
        ObjectStore::new(dir.path().join("objects")),
    );
    let ev = |source, event| BusEvent {
        project_id: project.to_string(),
        agent_id: Some(agent.to_string()),
        run_id: Some(run.to_string()),
        source,
        event,
        occurred_at: 1,
    };
    let requested = |id: Option<&str>, command: &str| AgentEvent::ToolRequested {
        tool_use_id: id.map(str::to_string),
        tool: "Bash".into(),
        kind: ToolKind::Command,
        command: Some(command.into()),
    };
    let finished = |id: Option<&str>, ok| AgentEvent::ToolFinished {
        tool_use_id: id.map(str::to_string),
        tool: "Bash".into(),
        kind: ToolKind::Command,
        ok,
        exit_code: Some(if ok { 0 } else { 1 }),
    };
    let secret = "curl -H \"Authorization: Bearer sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789\" https://x";
    let user = |text: &str| AgentEvent::UserMessage { text: text.into() };
    for e in [
        // Mismo prompt por el runtime y por el hook UserPromptSubmit.
        ev(EventSource::User, user("haz X")),
        ev(EventSource::Hook, user("haz X")),
        // La misma herramienta por stream y por hook, pedido y final.
        ev(EventSource::JsonStream, requested(Some("toolu_1"), secret)),
        ev(EventSource::Hook, requested(Some("toolu_1"), secret)),
        ev(EventSource::Hook, finished(Some("toolu_1"), true)),
        ev(EventSource::JsonStream, finished(Some("toolu_1"), false)),
        // Sin id: se empareja por orden de llegada.
        ev(EventSource::Hook, requested(None, "npm test")),
        ev(EventSource::Hook, requested(None, "npm run build")),
        ev(EventSource::Hook, finished(None, false)),
        ev(EventSource::Hook, finished(None, true)),
        // Un final sin pedido se registra entero.
        ev(EventSource::JsonStream, finished(Some("toolu_9"), true)),
    ] {
        bus.publish(e).await.unwrap();
    }
    writer.handle().flush().await.unwrap();
    let conn = writer.reader().unwrap();
    let users: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE role = 'USER'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(users, 1);
    let calls: Vec<(Option<String>, String)> = conn
        .prepare("SELECT command, status FROM tool_calls ORDER BY rowid")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(calls.len(), 4, "{calls:?}");
    let first = calls[0].0.as_deref().unwrap();
    assert!(
        !first.contains("sk-ant-api03"),
        "comando sin redactar: {first}"
    );
    assert_eq!(calls[0].1, "DONE", "gana el primer final");
    assert_eq!(
        calls[1..]
            .iter()
            .map(|c| (c.0.as_deref(), c.1.as_str()))
            .collect::<Vec<_>>(),
        [
            (Some("npm test"), "FAILED"),
            (Some("npm run build"), "DONE"),
            (None, "DONE")
        ]
    );
    // Todo quedó también en `events`: el recorder no reemplaza al log.
    let events: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(events, 11);
    drop(bus);
    writer.shutdown();
}
