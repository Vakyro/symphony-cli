//! Lo que el broker MCP (`symphony mcp serve`, P09.S8) le pide al daemon por IPC:
//! `context.search`, `context.retrieve` y `context.lines`. Solo leen el contexto de un proyecto;
//! cada respuesta lista los *retrievals* que el servidor registra en `context_retrievals`
//! (`found = false` es un *retrieval miss*).

use rusqlite::Connection;
use serde_json::{Value, json};
use symphony_context::chunk;
use symphony_context::handoff::estimate_tokens;
use symphony_context::uri::CtxUri;
use symphony_core::redact;
use symphony_object_store::ObjectStore;
use symphony_store::context as store;

pub const SEARCH_DEFAULT: i64 = 5;
pub const SEARCH_MAX: i64 = 20;
/// Tope de lo que `context.retrieve` devuelve de golpe: el agente pide el resto con `lines`.
pub const RETRIEVE_MAX_BYTES: usize = 64 * 1024;
pub const LINES_MAX: usize = 400;

/// Un acceso al contexto para `context_retrievals`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retrieval {
    pub object_id: String,
    pub operation: &'static str,
    pub query: Option<String>,
    pub tokens: i64,
    pub found: bool,
}

#[derive(Debug)]
pub struct Answer {
    pub value: Value,
    pub retrievals: Vec<Retrieval>,
}

/// `(código, mensaje)` de un error de IPC.
pub type Failure = (&'static str, String);

fn invalid(msg: impl Into<String>) -> Failure {
    ("invalid_params", msg.into())
}

fn parse_uri(uri: &str) -> Result<(), Failure> {
    CtxUri::parse(uri)
        .map(|_| ())
        .map_err(|e| invalid(e.to_string()))
}

fn store_err(e: impl std::fmt::Display) -> Failure {
    ("store_error", e.to_string())
}

/// BM25 sobre los objetos del proyecto; con `uri`, solo dentro de ese objeto.
pub fn search(
    conn: &Connection,
    project: &str,
    query: &str,
    uri: Option<&str>,
    limit: Option<i64>,
) -> Result<Answer, Failure> {
    let fts = chunk::fts_query(query).ok_or_else(|| invalid("`query` no tiene palabras"))?;
    if let Some(u) = uri {
        parse_uri(u)?;
    }
    let limit = limit.unwrap_or(SEARCH_DEFAULT).clamp(1, SEARCH_MAX);
    let hits = store::search(conn, project, &fts, uri, limit).map_err(store_err)?;
    let mut retrievals: Vec<Retrieval> = Vec::new();
    for h in &hits {
        let tokens = estimate_tokens(&redact(&h.snippet));
        match retrievals.iter_mut().find(|r| r.object_id == h.object_id) {
            Some(r) => r.tokens += tokens,
            None => retrievals.push(Retrieval {
                object_id: h.object_id.clone(),
                operation: "SEARCH",
                query: Some(query.to_string()),
                tokens,
                found: true,
            }),
        }
    }
    // Sin resultados dentro de un objeto conocido: miss. Sin objeto no hay a qué atarlo
    // (`context_retrievals.object_id` es obligatorio).
    if hits.is_empty()
        && let Some(u) = uri
        && let Some(o) = store::object_by_uri(conn, Some(project), u).map_err(store_err)?
    {
        retrievals.push(Retrieval {
            object_id: o.id,
            operation: "SEARCH",
            query: Some(query.to_string()),
            tokens: 0,
            found: false,
        });
    }
    let hits_json: Vec<Value> = hits
        .iter()
        .map(|h| {
            json!({
                "uri": h.uri,
                "start_line": h.start_line,
                "end_line": h.end_line,
                // Lo que sale hacia un proveedor pasa por el redactor, como el handoff.
                "snippet": redact(&h.snippet),
                "score": h.score,
            })
        })
        .collect();
    Ok(Answer {
        value: json!({ "query": query, "hits": hits_json }),
        retrievals,
    })
}

fn original(
    conn: &Connection,
    objects: &ObjectStore,
    project: &str,
    uri: &str,
) -> Result<(store::ObjectRow, String), Failure> {
    parse_uri(uri)?;
    let row = store::object_by_uri(conn, Some(project), uri)
        .map_err(store_err)?
        .ok_or_else(|| ("context_not_found", format!("no existe `{uri}`")))?;
    let bytes = objects.get(&row.blob_hash).map_err(store_err)?;
    Ok((row, String::from_utf8_lossy(&bytes).into_owned()))
}

/// El original de un objeto (hasta [`RETRIEVE_MAX_BYTES`]). Es la salida reversible de los
/// compresores: lo que el handoff resumió, aquí vuelve completo.
pub fn retrieve(
    conn: &Connection,
    objects: &ObjectStore,
    project: &str,
    uri: &str,
) -> Result<Answer, Failure> {
    let (row, full) = original(conn, objects, project, uri)?;
    let truncated = full.len() > RETRIEVE_MAX_BYTES;
    let text = if truncated {
        let mut end = RETRIEVE_MAX_BYTES;
        while !full.is_char_boundary(end) {
            end -= 1;
        }
        full[..end].to_string()
    } else {
        full.clone()
    };
    let text = redact(&text).into_owned();
    let tokens = estimate_tokens(&text);
    Ok(Answer {
        value: json!({
            "uri": row.uri,
            "kind": row.kind,
            "bytes": full.len(),
            "total_lines": full.lines().count(),
            "truncated": truncated,
            "text": text,
        }),
        retrievals: vec![Retrieval {
            object_id: row.id,
            operation: "RETRIEVE",
            query: None,
            tokens,
            found: true,
        }],
    })
}

/// Las líneas `from..=to` (1-indexadas) del original, hasta [`LINES_MAX`].
pub fn lines(
    conn: &Connection,
    objects: &ObjectStore,
    project: &str,
    uri: &str,
    from: i64,
    to: i64,
) -> Result<Answer, Failure> {
    if from < 1 || to < from {
        return Err(invalid("`from` ≥ 1 y `to` ≥ `from`"));
    }
    let (row, full) = original(conn, objects, project, uri)?;
    let total = full.lines().count();
    let from_u = usize::try_from(from).unwrap_or(usize::MAX);
    let to_u = usize::try_from(to)
        .unwrap_or(usize::MAX)
        .min(from_u.saturating_add(LINES_MAX - 1));
    let text = redact(&chunk::lines(&full, from_u, to_u)).into_owned();
    let found = from_u <= total;
    let tokens = estimate_tokens(&text);
    Ok(Answer {
        value: json!({
            "uri": row.uri,
            "from": from,
            "to": to_u.min(total),
            "total_lines": total,
            "text": text,
        }),
        retrievals: vec![Retrieval {
            object_id: row.id,
            operation: "LINES",
            query: None,
            tokens,
            found,
        }],
    })
}
