//! Salud de proveedores, cuota y uso (DB §3.D, migración 002). Todo se escribe por el writer
//! único; las funciones reciben la conexión (o la transacción) que les pase quien llama.

use rusqlite::{Connection, OptionalExtension, params};
use symphony_core::{
    FailureType, Health, ProviderHealthId, ProviderState, QuotaCertainty, RunId, UsageRecordId,
    UsageSource,
};

use crate::repo::RepoError;

/// La cuenta `default` de un proveedor (una por proveedor: sin gestión de login, PLAN §2.9).
pub fn account_id(provider_id: &str) -> String {
    format!("acct-{provider_id}")
}

/// Crea la cuenta `default` y la fila de salud a nivel proveedor si todavía no existen.
pub fn ensure_account(conn: &Connection, provider_id: &str, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "INSERT OR IGNORE INTO provider_accounts (id, provider_id, label, auth_status, last_checked_at)
         VALUES (?1, ?2, 'default', 'UNKNOWN', ?3)",
        params![account_id(provider_id), provider_id, now],
    )?;
    if get_health(conn, provider_id, None)?.is_none() {
        put_health(conn, provider_id, None, &Health::unknown(now))?;
    }
    Ok(())
}

fn row_to_health(r: &rusqlite::Row<'_>) -> rusqlite::Result<Health> {
    let state: String = r.get(0)?;
    let certainty: String = r.get(1)?;
    Ok(Health {
        state: state.parse().unwrap_or(ProviderState::Unknown),
        certainty: certainty.parse().unwrap_or(QuotaCertainty::Unknown),
        remaining: r.get(2)?,
        evidence: r.get(3)?,
        retry_after_at: r.get(4)?,
        reset_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}

/// La salud guardada de un alcance: `model = None` es el nivel proveedor.
pub fn get_health(
    conn: &Connection,
    provider_id: &str,
    model: Option<&str>,
) -> Result<Option<Health>, RepoError> {
    Ok(conn
        .query_row(
            "SELECT state, quota_certainty, quota_remaining, evidence, retry_after_at, reset_at, updated_at
             FROM provider_health
             WHERE provider_id = ?1 AND COALESCE(account_id, '') = ?2 AND COALESCE(model_id, '') = ?3",
            params![provider_id, account_id(provider_id), model.unwrap_or("")],
            row_to_health,
        )
        .optional()?)
}

/// Guarda (crea o reemplaza) la salud de un alcance.
pub fn put_health(
    conn: &Connection,
    provider_id: &str,
    model: Option<&str>,
    h: &Health,
) -> Result<(), RepoError> {
    let account = account_id(provider_id);
    let updated = conn.execute(
        "UPDATE provider_health SET state = ?4, quota_certainty = ?5, quota_remaining = ?6, evidence = ?7,
             retry_after_at = ?8, reset_at = ?9, updated_at = ?10
         WHERE provider_id = ?1 AND COALESCE(account_id, '') = ?2 AND COALESCE(model_id, '') = ?3",
        params![
            provider_id,
            account,
            model.unwrap_or(""),
            h.state.as_str(),
            h.certainty.as_str(),
            h.remaining,
            h.evidence,
            h.retry_after_at,
            h.reset_at,
            h.updated_at
        ],
    )?;
    if updated == 0 {
        conn.execute(
            "INSERT INTO provider_health (id, provider_id, account_id, model_id, state, quota_certainty, quota_remaining, evidence, retry_after_at, reset_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                ProviderHealthId::new().to_string(),
                provider_id,
                account,
                model,
                h.state.as_str(),
                h.certainty.as_str(),
                h.remaining,
                h.evidence,
                h.retry_after_at,
                h.reset_at,
                h.updated_at
            ],
        )?;
    }
    Ok(())
}

/// Una fila de salud con su alcance.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthRow {
    pub provider_id: String,
    /// `None` = nivel proveedor.
    pub model_id: Option<String>,
    pub health: Health,
}

pub fn list_health(conn: &Connection) -> Result<Vec<HealthRow>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT state, quota_certainty, quota_remaining, evidence, retry_after_at, reset_at, updated_at, provider_id, model_id
         FROM provider_health ORDER BY provider_id, COALESCE(model_id, '')",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(HealthRow {
            health: row_to_health(r)?,
            provider_id: r.get(7)?,
            model_id: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Cuántos fallos de `kind` hubo desde `since` (para decidir `THROTTLED` y `OFFLINE`).
pub fn recent_failure_count(
    conn: &Connection,
    provider_id: &str,
    model: Option<&str>,
    kind: FailureType,
    since: i64,
) -> Result<u32, RepoError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM provider_failures
         WHERE provider_id = ?1 AND failure_type = ?2 AND occurred_at >= ?3 AND (?4 IS NULL OR model_id = ?4)",
        params![provider_id, kind.as_str(), since, model],
        |r| r.get(0),
    )?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

/// Cuántos fallos de cualquier tipo hubo desde `since` (el router los resta al puntaje).
pub fn recent_failures_by_model(
    conn: &Connection,
    since: i64,
) -> Result<Vec<(String, Option<String>, u32)>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT provider_id, model_id, COUNT(*) FROM provider_failures
         WHERE occurred_at >= ?1 GROUP BY provider_id, model_id",
    )?;
    let rows = stmt.query_map([since], |r| {
        let n: i64 = r.get(2)?;
        Ok((r.get(0)?, r.get(1)?, u32::try_from(n).unwrap_or(u32::MAX)))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Proveedor y modelo de un run.
pub fn run_scope(conn: &Connection, run: RunId) -> Result<(String, String), RepoError> {
    Ok(conn.query_row(
        "SELECT provider_id, model_id FROM agent_runs WHERE id = ?1",
        [run.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

/// Lo último que informó el proveedor por ventana (`five_hour`, `seven_day`…): `(ventana,
/// fracción usada, reinicio en ms)`. Sale de los eventos `QuotaUpdated` de sus runs.
pub fn latest_quota_windows(
    conn: &Connection,
    provider_id: &str,
) -> Result<Vec<(String, f64, Option<i64>)>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT json_extract(e.payload_json, '$.window'),
                json_extract(e.payload_json, '$.used_fraction'),
                json_extract(e.payload_json, '$.resets_at'),
                MAX(e.id)
         FROM events e JOIN agent_runs r ON r.id = e.run_id
         WHERE e.type = 'QuotaUpdated' AND r.provider_id = ?1
         GROUP BY json_extract(e.payload_json, '$.window')",
    )?;
    let rows = stmt.query_map([provider_id], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<f64>>(1)?,
            r.get::<_, Option<i64>>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        if let (Some(window), Some(used), reset) = row? {
            out.push((window, used, reset));
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewUsage {
    pub run_id: RunId,
    pub provider_id: String,
    pub model_id: String,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    pub source: UsageSource,
}

pub fn insert_usage(conn: &Connection, u: &NewUsage, now: i64) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO usage_records (id, run_id, provider_id, model_id, tokens_in, tokens_out, source, recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            UsageRecordId::new().to_string(),
            u.run_id.to_string(),
            u.provider_id,
            u.model_id,
            u.tokens_in.map(|t| i64::try_from(t).unwrap_or(i64::MAX)),
            u.tokens_out.map(|t| i64::try_from(t).unwrap_or(i64::MAX)),
            u.source.as_str(),
            now
        ],
    )?;
    Ok(())
}

/// Una sola fila `REPORTED` por run, con el mayor valor visto (los turnos se solapan).
pub fn upsert_reported_usage(
    conn: &Connection,
    run: RunId,
    provider_id: &str,
    model_id: &str,
    tokens_in: u64,
    now: i64,
) -> Result<(), RepoError> {
    let tokens = i64::try_from(tokens_in).unwrap_or(i64::MAX);
    let updated = conn.execute(
        "UPDATE usage_records SET tokens_in = MAX(COALESCE(tokens_in, 0), ?2), recorded_at = ?3
         WHERE run_id = ?1 AND source = 'REPORTED'",
        params![run.to_string(), tokens, now],
    )?;
    if updated == 0 {
        insert_usage(
            conn,
            &NewUsage {
                run_id: run,
                provider_id: provider_id.to_string(),
                model_id: model_id.to_string(),
                tokens_in: Some(tokens_in),
                tokens_out: None,
                source: UsageSource::Reported,
            },
            now,
        )?;
    }
    Ok(())
}

pub fn run_has_reported_usage(conn: &Connection, run: RunId) -> Result<bool, RepoError> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM usage_records WHERE run_id = ?1 AND source = 'REPORTED' LIMIT 1",
            [run.to_string()],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Estimación gruesa de un run (~4 caracteres por token) cuando su CLI no informa tokens:
/// lo que se le envió (mensajes `USER`) y lo que respondió (`ASSISTANT`).
pub fn estimate_run_tokens(conn: &Connection, run: RunId) -> Result<(u64, u64), RepoError> {
    let chars = |role: &str| -> Result<i64, RepoError> {
        Ok(conn.query_row(
            "SELECT COALESCE(SUM(COALESCE(LENGTH(m.content), b.size_bytes, 0)), 0)
             FROM messages m
             LEFT JOIN context_objects o ON o.id = m.content_object_id
             LEFT JOIN blobs b ON b.hash = o.blob_hash
             WHERE m.run_id = ?1 AND m.role = ?2",
            params![run.to_string(), role],
            |r| r.get(0),
        )?)
    };
    let to_tokens = |c: i64| u64::try_from(c).unwrap_or(0).div_ceil(4);
    Ok((to_tokens(chars("USER")?), to_tokens(chars("ASSISTANT")?)))
}

/// Uso agregado por proveedor, modelo y origen desde `since` (vista Usage).
#[derive(Debug, Clone, PartialEq)]
pub struct UsageRow {
    pub provider_id: String,
    pub model_id: String,
    pub source: String,
    pub runs: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
}

pub fn usage_since(conn: &Connection, since: i64) -> Result<Vec<UsageRow>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT provider_id, model_id, source, COUNT(*), COALESCE(SUM(tokens_in), 0), COALESCE(SUM(tokens_out), 0)
         FROM usage_records WHERE recorded_at >= ?1
         GROUP BY provider_id, model_id, source ORDER BY provider_id, model_id, source",
    )?;
    let rows = stmt.query_map([since], |r| {
        Ok(UsageRow {
            provider_id: r.get(0)?,
            model_id: r.get(1)?,
            source: r.get(2)?,
            runs: r.get(3)?,
            tokens_in: r.get(4)?,
            tokens_out: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Tokens de entrada (informados o estimados) de un proveedor desde `since`: base de la
/// cuota `ESTIMATED` cuando la config da un presupuesto por ventana.
pub fn provider_tokens_since(
    conn: &Connection,
    provider_id: &str,
    since: i64,
) -> Result<u64, RepoError> {
    let n: i64 = conn.query_row(
        "SELECT COALESCE(SUM(COALESCE(tokens_in, 0) + COALESCE(tokens_out, 0)), 0)
         FROM usage_records WHERE provider_id = ?1 AND recorded_at >= ?2",
        params![provider_id, since],
        |r| r.get(0),
    )?;
    Ok(u64::try_from(n).unwrap_or(0))
}
