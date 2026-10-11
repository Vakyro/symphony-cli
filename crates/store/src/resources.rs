//! Muestras de recursos del sistema (DB §3.C `resource_samples`, P08.S2).
//! Se muestrea cada pocos segundos y, pasadas 24 h, se agrega por minuto.

use rusqlite::{Connection, params};
use symphony_core::PerformanceProfile;

use crate::repo::RepoError;

#[derive(Debug, Clone, PartialEq)]
pub struct NewSample {
    pub cpu_pct: f64,
    pub ram_pct: f64,
    pub heavy_slots_used: u32,
    pub heavy_slots_total: u32,
    pub process_count: Option<u32>,
    pub profile: PerformanceProfile,
    pub sampled_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SampleRow {
    pub cpu_pct: f64,
    pub ram_pct: f64,
    pub heavy_slots_used: i64,
    pub heavy_slots_total: i64,
    pub process_count: Option<i64>,
    pub profile: String,
    pub sampled_at: i64,
}

pub fn insert_sample(conn: &Connection, s: &NewSample) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO resource_samples
            (cpu_pct, ram_pct, heavy_slots_used, heavy_slots_total, process_count, performance_profile, sampled_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            s.cpu_pct,
            s.ram_pct,
            s.heavy_slots_used,
            s.heavy_slots_total,
            s.process_count,
            s.profile.as_str(),
            s.sampled_at
        ],
    )?;
    Ok(())
}

/// Muestras desde `since`, de la más vieja a la más nueva.
pub fn samples_since(conn: &Connection, since: i64) -> Result<Vec<SampleRow>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT cpu_pct, ram_pct, heavy_slots_used, heavy_slots_total, process_count, performance_profile, sampled_at
         FROM resource_samples WHERE sampled_at >= ?1 ORDER BY sampled_at, id",
    )?;
    let rows = stmt.query_map([since], |r| {
        Ok(SampleRow {
            cpu_pct: r.get(0)?,
            ram_pct: r.get(1)?,
            heavy_slots_used: r.get(2)?,
            heavy_slots_total: r.get(3)?,
            process_count: r.get(4)?,
            profile: r.get(5)?,
            sampled_at: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Reemplaza las muestras anteriores a `older_than` por una por cada `bucket_ms`
/// (promedio de CPU y RAM, máximo de slots). Idempotente: un cubo con una sola fila queda
/// como está. Devuelve cuántas filas se borraron.
pub fn compact_samples(
    conn: &Connection,
    older_than: i64,
    bucket_ms: i64,
) -> Result<usize, RepoError> {
    let bucket_ms = bucket_ms.max(1);
    let max_id: i64 = conn.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM resource_samples",
        [],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO resource_samples
            (cpu_pct, ram_pct, heavy_slots_used, heavy_slots_total, process_count, performance_profile, sampled_at)
         SELECT AVG(cpu_pct), AVG(ram_pct), MAX(heavy_slots_used), MAX(heavy_slots_total),
                CAST(ROUND(AVG(process_count)) AS INTEGER), MAX(performance_profile),
                (sampled_at / ?1) * ?1
         FROM resource_samples
         WHERE id <= ?2 AND sampled_at < ?3
         GROUP BY sampled_at / ?1
         HAVING COUNT(*) > 1",
        params![bucket_ms, max_id, older_than],
    )?;
    // Los originales de cada cubo que acaba de recibir su agregado (ids nuevos > max_id).
    let removed = conn.execute(
        "DELETE FROM resource_samples
         WHERE id <= ?2 AND sampled_at < ?3
           AND sampled_at / ?1 IN (SELECT sampled_at / ?1 FROM resource_samples WHERE id > ?2)",
        params![bucket_ms, max_id, older_than],
    )?;
    Ok(removed)
}
