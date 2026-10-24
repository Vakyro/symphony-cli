//! Objetos de contexto, chunks con búsqueda FTS5/BM25 y retrievals (DB §3.G, migración 003).

use rusqlite::{Connection, OptionalExtension, params};
use symphony_core::{ContextObjectId, FactId, RunId};

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

/// Un objeto por su dirección. `ctx://file/…` es relativo al proyecto: sin `project` se toma
/// el más reciente con esa dirección.
pub fn object_by_uri(
    conn: &Connection,
    project: Option<&str>,
    uri: &str,
) -> Result<Option<ObjectRow>, RepoError> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {OBJECT_COLS} FROM context_objects WHERE uri = ?1 AND (?2 IS NULL OR project_id = ?2)
                 ORDER BY created_at DESC, rowid DESC LIMIT 1"
            ),
            params![uri, project],
            object_row,
        )
        .optional()?)
}

/// Lo que pasó al registrar un archivo del proyecto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileUpsert {
    /// Mismo contenido que ya estaba: nada que hacer.
    Unchanged,
    /// Nuevo o cambiado; `previous` es el blob anterior, para soltar su referencia.
    Stored {
        id: ContextObjectId,
        previous: Option<String>,
    },
}

/// Registra (o actualiza) el archivo `ctx://file/<ruta>` del proyecto con su blob nuevo.
/// Al cambiar, la versión comprimida anterior deja de valer.
pub fn upsert_file(
    conn: &Connection,
    project: &str,
    uri: &str,
    blob_hash: &str,
    now: i64,
) -> Result<FileUpsert, RepoError> {
    let current: Option<(String, String)> = conn
        .query_row(
            "SELECT id, blob_hash FROM context_objects WHERE project_id = ?1 AND uri = ?2",
            params![project, uri],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match current {
        Some((_, hash)) if hash == blob_hash => Ok(FileUpsert::Unchanged),
        Some((id, hash)) => {
            conn.execute(
                "UPDATE context_objects SET blob_hash = ?2, compressed_text = NULL, compressor = 'NONE',
                        tokens_original = NULL, tokens_compressed = NULL, created_at = ?3 WHERE id = ?1",
                params![id, blob_hash, now],
            )?;
            let id = id.parse().map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(FileUpsert::Stored {
                id,
                previous: Some(hash),
            })
        }
        None => {
            let id = ContextObjectId::new();
            conn.execute(
                "INSERT INTO context_objects (id, uri, project_id, kind, blob_hash, compressor, created_at)
                 VALUES (?1, ?2, ?3, 'FILE', ?4, 'NONE', ?5)",
                params![id.to_string(), uri, project, blob_hash, now],
            )?;
            Ok(FileUpsert::Stored { id, previous: None })
        }
    }
}

/// Las URIs de los archivos indexados del proyecto.
pub fn file_uris(conn: &Connection, project: &str) -> Result<Vec<String>, RepoError> {
    let mut stmt =
        conn.prepare("SELECT uri FROM context_objects WHERE project_id = ?1 AND kind = 'FILE'")?;
    let uris = stmt
        .query_map([project], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(uris)
}

/// Borra el archivo `uri` del proyecto con sus chunks y recuperaciones. Devuelve su blob.
pub fn delete_file(
    conn: &Connection,
    project: &str,
    uri: &str,
) -> Result<Option<String>, RepoError> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT id, blob_hash FROM context_objects WHERE project_id = ?1 AND uri = ?2 AND kind = 'FILE'",
            params![project, uri],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, hash)) = row else {
        return Ok(None);
    };
    conn.execute("DELETE FROM context_chunks WHERE object_id = ?1", [&id])?;
    conn.execute("DELETE FROM context_retrievals WHERE object_id = ?1", [&id])?;
    conn.execute(
        "UPDATE handoff_items SET object_id = NULL WHERE object_id = ?1",
        [&id],
    )?;
    conn.execute("DELETE FROM context_objects WHERE id = ?1", [&id])?;
    Ok(Some(hash))
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactRow {
    pub id: String,
    pub project_id: String,
    pub key: String,
    pub value: String,
    pub kind: String,
    pub status: String,
    pub superseded_by_id: Option<String>,
    pub source_agent_id: Option<String>,
    pub source_event_id: Option<i64>,
    pub created_at: i64,
}

fn fact_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<FactRow> {
    Ok(FactRow {
        id: r.get(0)?,
        project_id: r.get(1)?,
        key: r.get(2)?,
        value: r.get(3)?,
        kind: r.get(4)?,
        status: r.get(5)?,
        superseded_by_id: r.get(6)?,
        source_agent_id: r.get(7)?,
        source_event_id: r.get(8)?,
        created_at: r.get(9)?,
    })
}

const FACT_COLS: &str = "id, project_id, key, value, kind, status, superseded_by_id, source_agent_id, source_event_id, created_at";

/// Registra un hecho vigente sobre el proyecto (`CURRENT`). Si ya existía un hecho `CURRENT` con esa
/// clave, pasa a `SUPERSEDED` y apunta al nuevo con `superseded_by_id`.
#[allow(clippy::too_many_arguments)]
pub fn set_fact(
    conn: &Connection,
    project_id: &str,
    key: &str,
    value: &str,
    kind: &str,
    source_agent_id: Option<&str>,
    source_event_id: Option<i64>,
    now: i64,
) -> Result<FactRow, RepoError> {
    let new_id = FactId::new().to_string();
    conn.execute(
        "UPDATE project_facts SET status = 'SUPERSEDED'
         WHERE project_id = ?1 AND key = ?2 AND status = 'CURRENT'",
        params![project_id, key],
    )?;
    conn.execute(
        "INSERT INTO project_facts (id, project_id, key, value, kind, status, source_agent_id, source_event_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'CURRENT', ?6, ?7, ?8)",
        params![new_id, project_id, key, value, kind, source_agent_id, source_event_id, now],
    )?;
    conn.execute(
        "UPDATE project_facts SET superseded_by_id = ?1
         WHERE project_id = ?2 AND key = ?3 AND status = 'SUPERSEDED' AND superseded_by_id IS NULL",
        params![new_id, project_id, key],
    )?;
    Ok(FactRow {
        id: new_id,
        project_id: project_id.to_string(),
        key: key.to_string(),
        value: value.to_string(),
        kind: kind.to_string(),
        status: "CURRENT".to_string(),
        superseded_by_id: None,
        source_agent_id: source_agent_id.map(str::to_string),
        source_event_id,
        created_at: now,
    })
}

/// Marca un hecho en conflicto (`CONFLICT`), por ejemplo cuando dos fuentes se contradicen.
pub fn set_fact_conflict(conn: &Connection, fact_id: &str) -> Result<bool, RepoError> {
    let changed = conn.execute(
        "UPDATE project_facts SET status = 'CONFLICT' WHERE id = ?1",
        params![fact_id],
    )?;
    Ok(changed > 0)
}

/// Lista los hechos de un proyecto. Con `status` solo los de ese estado (`CURRENT`, `SUPERSEDED`, `CONFLICT`).
/// Sin `status`, primero `CURRENT`, luego `CONFLICT`, luego `SUPERSEDED`.
pub fn list_facts(
    conn: &Connection,
    project_id: &str,
    status: Option<&str>,
) -> Result<Vec<FactRow>, RepoError> {
    if let Some(st) = status {
        let mut stmt = conn.prepare(&format!(
            "SELECT {FACT_COLS} FROM project_facts WHERE project_id = ?1 AND status = ?2 ORDER BY key, created_at DESC"
        ))?;
        let rows = stmt.query_map(params![project_id, st], fact_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    } else {
        let mut stmt = conn.prepare(&format!(
            "SELECT {FACT_COLS} FROM project_facts WHERE project_id = ?1
             ORDER BY CASE status WHEN 'CURRENT' THEN 0 WHEN 'CONFLICT' THEN 1 ELSE 2 END, key, created_at DESC"
        ))?;
        let rows = stmt.query_map(params![project_id], fact_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

/// Hechos vigentes (`CURRENT`) de un proyecto.
pub fn get_current_facts(conn: &Connection, project_id: &str) -> Result<Vec<FactRow>, RepoError> {
    list_facts(conn, project_id, Some("CURRENT"))
}
