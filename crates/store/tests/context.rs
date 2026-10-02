//! Objetos de contexto, búsqueda FTS5/BM25 y retrievals (migración 003).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rusqlite::Connection;
use symphony_core::{ContextObjectId, RunId};
use symphony_store::context::{self as ctx, NewChunk};

fn seeded() -> (Connection, ContextObjectId, ContextObjectId, RunId) {
    let conn = symphony_store::open_in_memory().unwrap();
    conn.execute_batch(
        "INSERT INTO projects (id, name, root_path, default_branch, created_at) VALUES ('p1','uno','/a','main',0), ('p2','dos','/b','main',0);
         INSERT INTO sessions (id, project_id, status, started_at) VALUES ('s1','p1','ACTIVE',0);
         INSERT INTO tasks (id, project_id, code, kind, title, status, created_at, updated_at) VALUES ('t1','p1','T-1','WORK','uno','RUNNING',0,0);
         INSERT INTO providers (id, display_name, cli_name, adapter_mode, setup_state) VALUES ('anthropic','Claude','claude','CLI','READY');
         INSERT INTO models (id, provider_id, cli_model_id, display_name, discovered_at) VALUES ('claude/sonnet','anthropic','sonnet','Sonnet',0);
         INSERT INTO agents (id, project_id, session_id, task_id, number, state, execution_mode, requested_model_id, failover_policy, context_mode, created_at, updated_at)
             VALUES ('a1','p1','s1','t1',1,'RUNNING','EXACT','claude/sonnet','ANY','BALANCED',0,0);
         INSERT INTO blobs (hash, size_bytes, stored_bytes, created_at) VALUES ('h1',1,1,0), ('h2',1,1,0);",
    )
    .unwrap();
    symphony_store::health::ensure_account(&conn, "anthropic", 0).unwrap();
    let (o1, o2) = (ContextObjectId::new(), ContextObjectId::new());
    for (id, uri, project, hash) in [
        (o1, "ctx://file/auth.ts", "p1", "h1"),
        (o2, "ctx://file/otro.ts", "p2", "h2"),
    ] {
        conn.execute(
            "INSERT INTO context_objects (id, uri, project_id, kind, blob_hash, created_at) VALUES (?1, ?2, ?3, 'FILE', ?4, 0)",
            rusqlite::params![id.to_string(), uri, project, hash],
        )
        .unwrap();
    }
    let run = RunId::new();
    conn.execute(
        "INSERT INTO agent_runs (id, agent_id, seq, provider_id, account_id, model_id, status, started_at)
         VALUES (?1,'a1',1,'anthropic','acct-anthropic','claude/sonnet','RUNNING',0)",
        [run.to_string()],
    )
    .unwrap();
    (conn, o1, o2, run)
}

fn chunk(seq: i64, text: &str) -> NewChunk {
    NewChunk {
        seq,
        start_line: seq * 10 + 1,
        end_line: seq * 10 + 10,
        text: text.into(),
    }
}

#[test]
fn search_ranks_by_bm25_and_stays_inside_the_project() {
    let (conn, o1, o2, _) = seeded();
    ctx::replace_chunks(
        &conn,
        o1,
        &[
            chunk(0, "function refresh(token) { return rotate(token) }"),
            chunk(1, "refresh refresh refresh 401 unauthorized after refresh"),
            chunk(2, "class Cache { get() {} }"),
        ],
    )
    .unwrap();
    ctx::replace_chunks(&conn, o2, &[chunk(0, "refresh en el otro proyecto")]).unwrap();

    let q = symphony_context::chunk::fts_query("refresh 401").unwrap();
    let hits = ctx::search(&conn, "p1", &q, None, 10).unwrap();
    assert_eq!(hits.len(), 2, "el otro proyecto no aparece");
    assert!(hits.iter().all(|h| h.uri == "ctx://file/auth.ts"));
    assert_eq!(hits[0].seq, 1, "el chunk con más coincidencias va primero");
    assert!(hits[0].score >= hits[1].score);
    assert!(hits[0].snippet.contains('['), "{}", hits[0].snippet);
    assert_eq!(hits[0].start_line, Some(11));

    // Limitada a un objeto, y sin resultados cuando no hay coincidencia.
    assert_eq!(
        ctx::search(&conn, "p1", &q, Some("ctx://file/auth.ts"), 1)
            .unwrap()
            .len(),
        1
    );
    assert!(
        ctx::search(&conn, "p1", &q, Some("ctx://file/otro.ts"), 5)
            .unwrap()
            .is_empty()
    );
    let none = symphony_context::chunk::fts_query("zzzz").unwrap();
    assert!(ctx::search(&conn, "p1", &none, None, 5).unwrap().is_empty());
}

#[test]
fn hostile_search_text_never_breaks_the_query() {
    let (conn, o1, _, _) = seeded();
    ctx::replace_chunks(&conn, o1, &[chunk(0, "alpha beta gamma")]).unwrap();
    for user in [
        "alpha AND",
        "\"alpha",
        "alpha*",
        "NOT alpha",
        "a:b",
        "(alpha",
        "alpha OR OR",
        "'; DROP TABLE x;--",
    ] {
        if let Some(q) = symphony_context::chunk::fts_query(user) {
            ctx::search(&conn, "p1", &q, None, 5).unwrap_or_else(|e| panic!("{user}: {e}"));
        }
    }
}

#[test]
fn replacing_chunks_reindexes_and_compression_keeps_the_original_reference() {
    let (conn, o1, _, _) = seeded();
    ctx::replace_chunks(&conn, o1, &[chunk(0, "viejo contenido unico")]).unwrap();
    ctx::replace_chunks(&conn, o1, &[chunk(0, "contenido nuevo distinto")]).unwrap();
    let find = |w: &str| {
        let q = symphony_context::chunk::fts_query(w).unwrap();
        ctx::search(&conn, "p1", &q, None, 5).unwrap().len()
    };
    assert_eq!(find("viejo"), 0);
    assert_eq!(find("nuevo"), 1);

    ctx::set_compression(&conn, o1, "896 passed, 1 failed", "LOG_COLLAPSE", 5000, 12).unwrap();
    let row = ctx::object_by_uri(&conn, "ctx://file/auth.ts")
        .unwrap()
        .unwrap();
    assert_eq!(row.compressed_text.as_deref(), Some("896 passed, 1 failed"));
    assert_eq!(row.blob_hash, "h1", "el original sigue en su blob");
    assert_eq!(
        (row.tokens_original, row.tokens_compressed),
        (Some(5000), Some(12))
    );
    assert!(
        ctx::object_by_uri(&conn, "ctx://file/no-existe")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        ctx::object_by_id(&conn, &row.id).unwrap().unwrap().uri,
        row.uri
    );
}

#[test]
fn retrievals_are_counted_with_their_misses() {
    let (conn, o1, _, run) = seeded();
    let id = o1.to_string();
    ctx::record_retrieval(
        &conn,
        run,
        &id,
        "SEARCH",
        Some("refresh 401"),
        Some(300),
        true,
        1,
    )
    .unwrap();
    ctx::record_retrieval(&conn, run, &id, "LINES", None, Some(120), true, 2).unwrap();
    ctx::record_retrieval(&conn, run, &id, "RETRIEVE", None, None, false, 3).unwrap();
    assert_eq!(ctx::retrieval_stats(&conn, run).unwrap(), (3, 1, 420));
    assert!(
        ctx::record_retrieval(&conn, run, &id, "ADIVINAR", None, None, true, 4).is_err(),
        "la operación la valida el CHECK"
    );
}
