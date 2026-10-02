//! Objetos de contexto, chunks con búsqueda FTS5/BM25 y retrievals (DB §3.G, migración 003).

use rusqlite::{Connection, OptionalExtension, params};
use symphony_core::{ContextObjectId, RunId};

use crate::repo::RepoError;

#[derive(Debug, Clone, PartialEq)]
pub struct NewChunk {
    pub seq: i64,
    pub start_line: i64,
    pub end_line: i64,
    pub text: String,
}

/// Reemplaza los chunks de un objeto (los triggers mantienen `context_fts` al día).
pub fn replace_chunks(
    conn: &Connection,
    object: ContextObjectId,
    chunks: &[NewChunk],
) -> Result<(), RepoError> {
    let id = object.to_string();
    conn.execute("DELETE FROM context_chunks WHERE object_id = ?1", [&id])?;
    let mut stmt = conn.prepare(
        "INSERT INTO context_chunks (object_id, seq, start_line, end_line, text) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for c in chunks {
        stmt.execute(params![id, c.seq, c.start_line, c.end_line, c.text])?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectRow {
    pub id: String,
    pub uri: String,
    pub project_id: String,
    pub agent_id: Option<String>,
    pub run_id: Option<String>,
    pub kind: String,
    pub blob_hash: String,
    pub compressed_text: Option<String>,
    pub compressor: Option<String>,
    pub tokens_original: Option<i64>,
    pub tokens_compressed: Option<i64>,
}

fn object_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ObjectRow> {
    Ok(ObjectRow {
        id: r.get(0)?,
        uri: r.get(1)?,
        project_id: r.get(2)?,
        agent_id: r.get(3)?,
        run_id: r.get(4)?,
        kind: r.get(5)?,
        blob_hash: r.get(6)?,
        compressed_text: r.get(7)?,
        compressor: r.get(8)?,
        tokens_original: r.get(9)?,
        tokens_compressed: r.get(10)?,
    })
}

const OBJECT_COLS: &str = "id, uri, project_id, agent_id, run_id, kind, blob_hash, compressed_text, compressor, tokens_original, tokens_compressed";

pub fn object_by_uri(conn: &Connection, uri: &str) -> Result<Option<ObjectRow>, RepoError> {
    Ok(conn
        .query_row(
            &format!("SELECT {OBJECT_COLS} FROM context_objects WHERE uri = ?1"),
            [uri],
            object_row,
        )
        .optional()?)
}

pub fn object_by_id(conn: &Connection, id: &str) -> Result<Option<ObjectRow>, RepoError> {
    Ok(conn
        .query_row(
            &format!("SELECT {OBJECT_COLS} FROM context_objects WHERE id = ?1"),
            [id],
            object_row,
        )
        .optional()?)
}

/// Guarda la versión compacta de un objeto (el original sigue en su blob).
pub fn set_compression(
    conn: &Connection,
    object: ContextObjectId,
    compressed_text: &str,
    compressor: &str,
    tokens_original: i64,
    tokens_compressed: i64,
) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE context_objects SET compressed_text = ?2, compressor = ?3, tokens_original = ?4, tokens_compressed = ?5 WHERE id = ?1",
        params![object.to_string(), compressed_text, compressor, tokens_original, tokens_compressed],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub uri: String,
    pub object_id: String,
    pub seq: i64,
    pub start_line: Option<i64>,
    pub end_line: Option<i64>,
    pub snippet: String,
    /// BM25 de SQLite: cuanto más negativo, más relevante. Se devuelve ya como «más es mejor».
    pub score: f64,
}

/// Búsqueda BM25 de `fts` (ya saneada con `symphony_context::chunk::fts_query`) en los objetos
/// de un proyecto. Si `uri` es `Some`, solo en ese objeto.
pub fn search(
    conn: &Connection,
    project: &str,
    fts: &str,
    uri: Option<&str>,
    limit: i64,
) -> Result<Vec<Hit>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT o.uri, o.id, c.seq, c.start_line, c.end_line,
                snippet(context_fts, 0, '[', ']', '…', 16), bm25(context_fts)
         FROM context_fts
         JOIN context_chunks c ON c.id = context_fts.rowid
         JOIN context_objects o ON o.id = c.object_id
         WHERE context_fts MATCH ?1 AND o.project_id = ?2 AND (?3 IS NULL OR o.uri = ?3)
         ORDER BY bm25(context_fts), o.uri, c.seq
         LIMIT ?4",
    )?;
    let rows = stmt.query_map(params![fts, project, uri, limit], |r| {
        let bm25: f64 = r.get(6)?;
        Ok(Hit {
            uri: r.get(0)?,
            object_id: r.get(1)?,
            seq: r.get(2)?,
            start_line: r.get(3)?,
            end_line: r.get(4)?,
            snippet: r.get(5)?,
            score: -bm25,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Un executor pidió más detalle por el MCP de contexto. `found = false` es un *retrieval miss*.
#[allow(clippy::too_many_arguments)]
pub fn record_retrieval(
    conn: &Connection,
    run: RunId,
    object_id: &str,
    operation: &str,
    query: Option<&str>,
    tokens_returned: Option<i64>,
    found: bool,
    now: i64,
) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO context_retrievals (id, run_id, object_id, operation, query, tokens_returned, found, requested_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            ContextObjectId::new().to_string(),
            run.to_string(),
            object_id,
            operation,
            query,
            tokens_returned,
            i64::from(found),
            now
        ],
    )?;
    Ok(())
}

/// Retrievals de un run: `(total, misses, tokens devueltos)`.
pub fn retrieval_stats(conn: &Connection, run: RunId) -> Result<(i64, i64, i64), RepoError> {
    Ok(conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(1 - found), 0), COALESCE(SUM(tokens_returned), 0)
         FROM context_retrievals WHERE run_id = ?1",
        [run.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?)
}
