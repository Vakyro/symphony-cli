//! Persistencia de Symphony (STACK §9, DB §1, §7). Solo el daemon abre la
//! base de datos, y con una sola conexión de escritura (PLAN §2.10).

use std::path::Path;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

pub mod repo;
mod writer;
pub use writer::{
    BATCH_MAX, NewEvent, WriteFn, Writer, WriterClosed, WriterHandle, WriterStats, open_reader,
};

/// Migraciones versionadas con `PRAGMA user_version` (DB §6).
/// `foreign_key_check` hace que una migración con FKs rotas falle al aplicarse.
const MIGRATION_LIST: &[M<'static>] =
    &[M::up(include_str!("../../../migrations/001_core.sql")).foreign_key_check()];

pub const MIGRATIONS: Migrations<'static> = Migrations::from_slice(MIGRATION_LIST);

/// PRAGMAs de DB §7. `journal_mode` va aparte porque devuelve una fila.
const PRAGMAS: &str = "
    PRAGMA synchronous = NORMAL;
    PRAGMA foreign_keys = ON;
    PRAGMA busy_timeout = 5000;
    PRAGMA temp_store = MEMORY;
    PRAGMA cache_size = -16000;
    PRAGMA wal_autocheckpoint = 1000;
";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("error de SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("no se pudo migrar la base de datos: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("no se pudo crear el directorio de la base de datos: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite no aceptó journal_mode=WAL (quedó en `{0}`)")]
    NoWal(String),
    #[error("el escritor de la base de datos ya no está corriendo")]
    WriterClosed,
}

fn configure(conn: &Connection, wal: bool) -> Result<(), StoreError> {
    if wal {
        let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(StoreError::NoWal(mode));
        }
    }
    conn.execute_batch(PRAGMAS)?;
    Ok(())
}

/// Abre (o crea) la base de datos, aplica los PRAGMAs y las migraciones pendientes.
pub fn open(path: &Path) -> Result<Connection, StoreError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut conn = Connection::open(path)?;
    configure(&conn, true)?;
    MIGRATIONS.to_latest(&mut conn)?;
    Ok(conn)
}

/// Base en memoria con el esquema completo, para tests. Sin WAL (no aplica en memoria).
pub fn open_in_memory() -> Result<Connection, StoreError> {
    let mut conn = Connection::open_in_memory()?;
    configure(&conn, false)?;
    MIGRATIONS.to_latest(&mut conn)?;
    Ok(conn)
}

#[cfg(test)]
mod tests;
