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
    let bus = EventBus::new(
        writer.handle(),
        16,
        symphony_object_store::ObjectStore::new(dir.path().join("objects")),
    );

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

/// P11.5.S3: el texto parcial se difunde y se acumula por agente, pero no se persiste ni toca el
/// estado; el mensaje completo lo reemplaza.
#[tokio::test(flavor = "multi_thread")]
async fn partial_text_is_ephemeral_and_replaced_by_the_full_message() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    symphony_store::open(&db)
        .unwrap()
        .execute_batch(
            "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','p','/p','main',0);",
        )
        .unwrap();
    let writer = Writer::start(&db).unwrap();
    let bus = EventBus::new(
        writer.handle(),
        16,
        symphony_object_store::ObjectStore::new(dir.path().join("objects")),
    );
    let mut rx = bus.subscribe();
    let delta = |agent: &str, text: &str| BusEvent {
        project_id: "p1".into(),
        agent_id: Some(agent.into()),
        run_id: None,
        source: EventSource::JsonStream,
        event: AgentEvent::AssistantDelta { text: text.into() },
        occurred_at: 1,
    };

    assert_eq!(bus.partial("a1"), "");
    bus.publish(delta("a1", "Hola, ")).await.unwrap();
    bus.publish(delta("a1", "mundo")).await.unwrap();
    bus.publish(delta("a2", "otro")).await.unwrap();
    assert_eq!(bus.partial("a1"), "Hola, mundo");
    assert_eq!(bus.partial("a2"), "otro");
    // Los suscriptores (la TUI) reciben cada trozo.
    for _ in 0..3 {
        assert!(matches!(
            rx.recv().await.unwrap().event,
            AgentEvent::AssistantDelta { .. }
        ));
    }
    // Nada en la base ni en el estado por agente.
    let stats = writer.handle().flush().await.unwrap();
    assert_eq!(stats.events_written, 0);
    assert!(bus.state().borrow().is_empty());

    // El mensaje completo reemplaza al parcial de ese agente (y solo de ese).
    let mut full = ev(2);
    full.agent_id = Some("a1".into());
    full.project_id = "p1".into();
    // `a1` no existe en `agents`: la persistencia del evento no es lo que se prueba aquí.
    let _ = bus.publish(full).await;
    assert_eq!(bus.partial("a1"), "");
    assert_eq!(bus.partial("a2"), "otro");

    drop(bus);
    writer.shutdown();
}
