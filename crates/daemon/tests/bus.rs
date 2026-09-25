//! P05.S2: un suscriptor lento no bloquea al productor ni pierde eventos persistidos.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use symphony_adapter_common::AgentEvent;
use symphony_daemon::bus::{BusEvent, EventBus, EventSource};
use symphony_store::Writer;
use tokio::sync::broadcast::error::RecvError;

fn ev(i: i64) -> BusEvent {
    BusEvent {
        project_id: "p1".into(),
        agent_id: Some("a1".into()),
        run_id: None,
        source: EventSource::Hook,
        event: AgentEvent::AssistantText {
            text: format!("msg {i}"),
        },
        occurred_at: i,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_subscriber_never_blocks_the_producer_nor_loses_persisted_events() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    symphony_store::open(&db)
        .unwrap()
        .execute_batch(
            "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','p','/p','main',0);
             INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','p1','ACTIVE',0);
             INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at)
                 VALUES ('t1','p1','T-1','WORK','t','RUNNING',0,0);
             INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode,
                                 failover_policy, context_mode, created_at, updated_at)
                 VALUES ('a1','p1','s1','t1',1,'RUNNING','DECIDE_LATER','ANY','BALANCED',0,0);",
        )
        .unwrap();
    let writer = Writer::start(&db).unwrap();
    let bus = EventBus::new(writer.handle(), 16);

    // Un suscriptor que no lee nunca (TUI colgada) y otro que lee todo.
    let mut stuck = bus.subscribe();
    let mut live = bus.subscribe();
    let reader = tokio::spawn(async move {
        let mut got = 0u32;
        loop {
            match live.recv().await {
                Ok(_) => got += 1,
                Err(RecvError::Lagged(n)) => got += u32::try_from(n).unwrap_or(0),
                Err(RecvError::Closed) => return got,
            }
        }
    });

    let n = 5_000;
    let t = Instant::now();
    for i in 0..n {
        bus.publish(ev(i)).await.unwrap();
    }
    let elapsed = t.elapsed();
    assert!(
        elapsed < Duration::from_secs(10),
        "el productor tardó {elapsed:?}"
    );

    // Todo quedó persistido, en orden.
    let stats = writer.handle().flush().await.unwrap();
    assert_eq!(stats.events_written, n as u64);
    assert_eq!(stats.events_failed, 0);
    let conn = writer.reader().unwrap();
    let (count, types): (i64, String) = conn
        .query_row(
            "SELECT COUNT(*), GROUP_CONCAT(DISTINCT type) FROM events",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((count, types.as_str()), (n, "AssistantText"));
    let last: String = conn
        .query_row(
            "SELECT payload_json FROM events ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(last.contains(&format!("msg {}", n - 1)), "{last}");

    // El suscriptor colgado se atrasó (perdió los viejos) pero no frenó nada.
    assert!(matches!(stuck.recv().await, Err(RecvError::Lagged(_))));

    // Estado actual por agente.
    let state = bus.state().borrow().clone();
    let snap = &state["a1"];
    assert_eq!(
        (snap.last_event, snap.last_event_at, snap.events),
        ("AssistantText", n - 1, n as u64)
    );

    drop(bus);
    assert_eq!(reader.await.unwrap(), n as u32);
    writer.shutdown();
}
