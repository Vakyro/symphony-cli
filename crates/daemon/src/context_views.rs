//! Vistas de solo lectura del context engine (P09.S6): `symphony context inspect|stats|raw`.

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};
use symphony_object_store::ObjectStore;

/// El último handoff de un agente con el detalle de lo que entró al prompt y con qué fidelidad.
pub fn inspect(conn: &Connection, agent: &str) -> rusqlite::Result<Value> {
    let head = conn
        .query_row(
            "SELECT id, mode, tokens_raw_estimate, tokens_sent, build_ms, outcome, created_at
             FROM handoffs WHERE agent_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT 1",
            [agent],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    json!({
                        "mode": r.get::<_, String>(1)?,
                        "tokens_raw_estimate": r.get::<_, Option<i64>>(2)?,
                        "tokens_sent": r.get::<_, Option<i64>>(3)?,
                        "build_ms": r.get::<_, Option<i64>>(4)?,
                        "outcome": r.get::<_, Option<String>>(5)?,
                        "created_at": r.get::<_, i64>(6)?,
                    }),
                ))
            },
        )
        .optional()?;
    let Some((id, mut handoff)) = head else {
        return Ok(json!({ "agent_id": agent, "handoff": null, "items": [] }));
    };
    let mut stmt = conn.prepare(
        "SELECT section, path, fidelity, tokens FROM handoff_items WHERE handoff_id = ?1 ORDER BY rowid",
    )?;
    let items: Vec<Value> = stmt
        .query_map([&id], |r| {
            Ok(json!({
                "section": r.get::<_, String>(0)?,
                "path": r.get::<_, Option<String>>(1)?,
                "fidelity": r.get::<_, Option<i64>>(2)?,
                "tokens": r.get::<_, i64>(3)?,
            }))
        })?
        .collect::<Result<_, _>>()?;
    handoff["id"] = json!(id);
    Ok(json!({ "agent_id": agent, "handoff": handoff, "items": items }))
}

/// Números del context engine: qué se comprimió y cuánto, qué cuestan los handoffs y cuántas
/// recuperaciones fallaron.
pub fn stats(conn: &Connection) -> rusqlite::Result<Value> {
    let rows = |sql: &str| -> rusqlite::Result<Vec<Value>> {
        let mut stmt = conn.prepare(sql)?;
        let cols = stmt.column_count();
        let names: Vec<String> = stmt
            .column_names()
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let out = stmt
            .query_map([], |r| {
                let mut o = serde_json::Map::new();
                for (i, n) in names.iter().enumerate().take(cols) {
                    let v: rusqlite::types::Value = r.get(i)?;
                    o.insert(
                        n.clone(),
                        match v {
                            rusqlite::types::Value::Integer(n) => json!(n),
                            rusqlite::types::Value::Real(f) => json!(f),
                            rusqlite::types::Value::Text(t) => json!(t),
                            _ => Value::Null,
                        },
                    );
                }
                Ok(Value::Object(o))
            })?
            .collect::<Result<_, _>>()?;
        Ok(out)
    };
    let one = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0));
    Ok(json!({
        "objects": one("SELECT COUNT(*) FROM context_objects")?,
        "chunks": one("SELECT COUNT(*) FROM context_chunks")?,
        "compression": rows(
            "SELECT compressor, COUNT(*) AS objects, SUM(tokens_original) AS tokens_original,
                    SUM(tokens_compressed) AS tokens_compressed
             FROM context_objects WHERE compressor IS NOT NULL GROUP BY compressor ORDER BY compressor",
        )?,
        "handoffs": rows(
            "SELECT mode, COUNT(*) AS handoffs, SUM(tokens_raw_estimate) AS tokens_raw,
                    SUM(tokens_sent) AS tokens_sent,
                    SUM(outcome = 'NEEDED_RETRIEVAL') AS needed_retrieval,
                    SUM(outcome = 'FAILED_TO_CONTINUE') AS failed_to_continue
             FROM handoffs WHERE checkpoint_id IS NOT NULL GROUP BY mode ORDER BY mode",
        )?,
        "retrievals": {
            "total": one("SELECT COUNT(*) FROM context_retrievals")?,
            "misses": one("SELECT COUNT(*) FROM context_retrievals WHERE found = 0")?,
            "tokens_returned": one("SELECT COALESCE(SUM(tokens_returned), 0) FROM context_retrievals")?,
        },
    }))
}

/// El original completo de un objeto de contexto (`ctx://…`), tal como se guardó.
pub fn raw(conn: &Connection, objects: &ObjectStore, uri: &str) -> Result<Value, String> {
    symphony_context::uri::CtxUri::parse(uri).map_err(|e| e.to_string())?;
    let row = symphony_store::context::object_by_uri(conn, uri)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no existe `{uri}`"))?;
    let bytes = objects.get(&row.blob_hash).map_err(|e| e.to_string())?;
    Ok(json!({
        "uri": row.uri,
        "kind": row.kind,
        "bytes": bytes.len(),
        "text": String::from_utf8_lossy(&bytes),
        "compressor": row.compressor,
        "tokens_original": row.tokens_original,
        "tokens_compressed": row.tokens_compressed,
    }))
}
