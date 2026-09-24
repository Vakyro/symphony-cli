//! P03.S2: 4 productores concurrentes, 10k eventos, lecturas en paralelo.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde_json::json;
use symphony_store::{NewEvent, Writer};

const PRODUCERS: usize = 4;
const PER_PRODUCER: usize = 2_500;

fn seed(writer_db: &std::path::Path) {
    let conn = symphony_store::open(writer_db).unwrap();
    conn.execute(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','demo','/repo','main',0)",
        [],
    )
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_producers_ten_thousand_events_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("symphony.db");
    seed(&db);
    let writer = Writer::start(&db).unwrap();

    // Un lector consulta todo el tiempo mientras se escribe: WAL no lo bloquea.
    let stop = Arc::new(AtomicBool::new(false));
    let reads = Arc::new(AtomicU64::new(0));
    let reader = {
        let conn = writer.reader().unwrap();
        let (stop, reads) = (stop.clone(), reads.clone());
        std::thread::spawn(move || -> Result<(), rusqlite::Error> {
            while !stop.load(Ordering::Relaxed) {
                let _: i64 = conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
                reads.fetch_add(1, Ordering::Relaxed);
            }
            Ok(())
        })
    };

    let mut tasks = Vec::new();
    for producer in 0..PRODUCERS {
        let handle = writer.handle();
        tasks.push(tokio::spawn(async move {
            for seq in 0..PER_PRODUCER {
                let ev = NewEvent {
                    project_id: "p1".into(),
                    agent_id: None,
                    run_id: None,
                    event_type: "TestEvent".into(),
                    source: "SYSTEM".into(),
                    payload_json: Some(json!({"producer": producer, "seq": seq}).to_string()),
                    occurred_at: 0,
                };
                handle.event(ev).await.unwrap();
            }
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    let stats = writer.handle().flush().await.unwrap();
    stop.store(true, Ordering::Relaxed);
    reader
        .join()
        .unwrap()
        .expect("el lector recibió un error (¿SQLITE_BUSY?)");

    assert_eq!(stats.events_written, (PRODUCERS * PER_PRODUCER) as u64);
    assert_eq!(stats.events_failed, 0);
    assert!(
        stats.batches < (PRODUCERS * PER_PRODUCER) as u64,
        "no hubo lotes: {stats:?}"
    );
    assert!(reads.load(Ordering::Relaxed) > 0);

    // Orden por productor conservado: `seq` crece con el id autoincremental.
    let conn = writer.reader().unwrap();
    let mut stmt = conn
        .prepare("SELECT json_extract(payload_json,'$.producer'), json_extract(payload_json,'$.seq') FROM events ORDER BY id")
        .unwrap();
    let rows: Vec<(i64, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(rows.len(), PRODUCERS * PER_PRODUCER);
    let mut last = [-1i64; PRODUCERS];
    for (producer, seq) in rows {
        let p = producer as usize;
        assert_eq!(seq, last[p] + 1, "productor {p} fuera de orden");
        last[p] = seq;
    }
    writer.shutdown();
}

#[tokio::test]
async fn bad_event_does_not_drop_its_batch() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("symphony.db");
    seed(&db);
    let writer = Writer::start(&db).unwrap();
    let handle = writer.handle();
    let ev = |project: &str| NewEvent {
        project_id: project.into(),
        agent_id: None,
        run_id: None,
        event_type: "X".into(),
        source: "SYSTEM".into(),
        payload_json: None,
        occurred_at: 0,
    };
    for i in 0..10 {
        // El quinto apunta a un proyecto inexistente: viola la FK.
        handle
            .event(ev(if i == 4 { "no-existe" } else { "p1" }))
            .await
            .unwrap();
    }
    let stats = handle.flush().await.unwrap();
    assert_eq!((stats.events_written, stats.events_failed), (9, 1));
}

#[tokio::test]
async fn write_closure_and_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("symphony.db");
    let writer = Writer::start(&db).unwrap();
    let handle = writer.handle();
    handle
        .write(Box::new(|tx| {
            tx.execute(
                "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p9','x','/x','main',0)",
                [],
            )?;
            Ok(())
        }))
        .await
        .unwrap();
    // Un error dentro del closure hace rollback y vuelve al que llamó.
    let dup = handle
        .write(Box::new(|tx| {
            tx.execute(
                "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p9','x','/y','main',0)",
                [],
            )?;
            Ok(())
        }))
        .await;
    assert!(dup.is_err());

    writer.shutdown();
    // Después del shutdown, los handles que quedaron fallan limpio.
    assert!(handle.flush().await.is_err());
    let conn = symphony_store::open_reader(&db).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
}
