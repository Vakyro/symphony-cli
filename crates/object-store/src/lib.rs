//! Object store (STACK §10, DB §3.G `blobs`): blobs direccionados por
//! contenido en `~/.symphony/objects/<h[0:2]>/<h>`, con hash BLAKE3 del
//! original y contenido comprimido con zstd.
//!
//! Garantías:
//! - Dedupe: el mismo contenido se guarda una sola vez.
//! - Atomicidad: se escribe a un temporal del mismo directorio, `fsync` y
//!   `rename`. Un crash nunca deja un blob truncado con nombre válido.
//! - Integridad: `get` verifica que el contenido descomprimido tenga el hash pedido.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, OptionalExtension, params};

/// Nivel moderado para el camino interactivo (STACK §10.2).
const ZSTD_LEVEL: i32 = 3;
/// Prefijo de los temporales; lo que empiece así y sea viejo es basura de un crash.
const TMP_PREFIX: &str = ".tmp-";

#[derive(Debug, thiserror::Error)]
pub enum ObjectError {
    #[error("E/S en el object store ({path}): {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("error de SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("no existe el blob {0}")]
    NotFound(String),
    #[error("el blob {hash} está corrupto: su contenido tiene hash {actual}")]
    Corrupt { hash: String, actual: String },
    #[error("hash inválido `{0}`: se esperan 64 caracteres hex")]
    InvalidHash(String),
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ObjectError + '_ {
    move |source| ObjectError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Resultado de un `put`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub hash: String,
    pub size_bytes: u64,
    pub stored_bytes: u64,
    /// `false` si el contenido ya estaba (dedupe).
    pub created: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GcReport {
    pub blobs_removed: u64,
    pub orphan_files_removed: u64,
    pub temp_files_removed: u64,
}

#[derive(Debug, Clone)]
pub struct ObjectStore {
    root: PathBuf,
}

fn validate_hash(hash: &str) -> Result<(), ObjectError> {
    if hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err(ObjectError::InvalidHash(hash.to_string()))
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl ObjectStore {
    /// `root` es `~/.symphony/objects`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_of(&self, hash: &str) -> PathBuf {
        self.root.join(&hash[..2]).join(hash)
    }

    /// Escribe el archivo del blob (sin tocar la base). Idempotente.
    pub fn write_object(&self, bytes: &[u8]) -> Result<Stored, ObjectError> {
        let hash = blake3::hash(bytes).to_hex().to_string();
        let path = self.path_of(&hash);
        let size_bytes = bytes.len() as u64;
        if let Ok(meta) = std::fs::metadata(&path) {
            return Ok(Stored {
                hash,
                size_bytes,
                stored_bytes: meta.len(),
                created: false,
            });
        }
        let dir = path.parent().unwrap_or(&self.root).to_path_buf();
        std::fs::create_dir_all(&dir).map_err(io(&dir))?;
        let compressed = zstd::bulk::compress(bytes, ZSTD_LEVEL).map_err(io(&path))?;
        let mut tmp = tempfile::Builder::new()
            .prefix(TMP_PREFIX)
            .tempfile_in(&dir)
            .map_err(io(&dir))?;
        tmp.write_all(&compressed).map_err(io(tmp.path()))?;
        tmp.as_file().sync_all().map_err(io(&path))?;
        match tmp.persist_noclobber(&path) {
            Ok(_) => Ok(Stored {
                hash,
                size_bytes,
                stored_bytes: compressed.len() as u64,
                created: true,
            }),
            // Otro escritor ganó la carrera con el mismo contenido: es el mismo blob.
            Err(e) if path.exists() => {
                drop(e);
                Ok(Stored {
                    hash,
                    size_bytes,
                    stored_bytes: compressed.len() as u64,
                    created: false,
                })
            }
            Err(e) => Err(ObjectError::Io {
                path,
                source: e.error,
            }),
        }
    }

    /// Guarda el contenido y registra la fila en `blobs` (con `ref_count = 0`).
    pub fn put(
        &self,
        conn: &Connection,
        bytes: &[u8],
        mime: Option<&str>,
    ) -> Result<Stored, ObjectError> {
        let stored = self.write_object(bytes)?;
        conn.execute(
            "INSERT INTO blobs (hash, size_bytes, stored_bytes, codec, mime, ref_count, created_at)
             VALUES (?1, ?2, ?3, 'zstd', ?4, 0, ?5) ON CONFLICT(hash) DO NOTHING",
            params![
                stored.hash,
                stored.size_bytes as i64,
                stored.stored_bytes as i64,
                mime,
                now_ms()
            ],
        )?;
        Ok(stored)
    }

    /// Lee y descomprime un blob, verificando su hash.
    pub fn get(&self, hash: &str) -> Result<Vec<u8>, ObjectError> {
        validate_hash(hash)?;
        let path = self.path_of(hash);
        let compressed = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(ObjectError::NotFound(hash.to_string()));
            }
            Err(e) => return Err(ObjectError::Io { path, source: e }),
        };
        let bytes =
            zstd::stream::decode_all(compressed.as_slice()).map_err(|_| ObjectError::Corrupt {
                hash: hash.to_string(),
                actual: "(zstd inválido)".into(),
            })?;
        let actual = blake3::hash(&bytes).to_hex().to_string();
        if actual != hash {
            return Err(ObjectError::Corrupt {
                hash: hash.to_string(),
                actual,
            });
        }
        Ok(bytes)
    }

    pub fn add_ref(&self, conn: &Connection, hash: &str) -> Result<(), ObjectError> {
        let n = conn.execute(
            "UPDATE blobs SET ref_count = ref_count + 1 WHERE hash = ?1",
            [hash],
        )?;
        if n == 0 {
            return Err(ObjectError::NotFound(hash.to_string()));
        }
        Ok(())
    }

    /// Suelta una referencia. Nunca baja de 0.
    pub fn release(&self, conn: &Connection, hash: &str) -> Result<(), ObjectError> {
        conn.execute(
            "UPDATE blobs SET ref_count = MAX(ref_count - 1, 0) WHERE hash = ?1",
            [hash],
        )?;
        Ok(())
    }

    /// Borra blobs sin referencias, archivos sin fila y temporales de crashes,
    /// todos más viejos que `grace` (para no competir con un `put` en curso).
    pub fn gc(&self, conn: &Connection, grace: Duration) -> Result<GcReport, ObjectError> {
        let mut report = GcReport::default();
        let cutoff_ms = now_ms() - i64::try_from(grace.as_millis()).unwrap_or(i64::MAX);

        let dead: Vec<String> = conn
            .prepare("SELECT hash FROM blobs WHERE ref_count = 0 AND created_at < ?1")?
            .query_map([cutoff_ms], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for hash in dead {
            // Primero la fila (si alguien lo referenció entre medio, el WHERE lo protege).
            let n = conn.execute(
                "DELETE FROM blobs WHERE hash = ?1 AND ref_count = 0",
                [&hash],
            )?;
            if n == 1 {
                let path = self.path_of(&hash);
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(ObjectError::Io { path, source: e }),
                }
                report.blobs_removed += 1;
            }
        }

        let cutoff = SystemTime::now()
            .checked_sub(grace)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let Ok(shards) = std::fs::read_dir(&self.root) else {
            return Ok(report);
        };
        for shard in shards.flatten() {
            let Ok(files) = std::fs::read_dir(shard.path()) else {
                continue;
            };
            for file in files.flatten() {
                let old = file
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t < cutoff);
                if !old {
                    continue;
                }
                let name = file.file_name().to_string_lossy().into_owned();
                if name.starts_with(TMP_PREFIX) {
                    if std::fs::remove_file(file.path()).is_ok() {
                        report.temp_files_removed += 1;
                    }
                    continue;
                }
                let known: Option<i64> = conn
                    .query_row("SELECT 1 FROM blobs WHERE hash = ?1", [&name], |r| r.get(0))
                    .optional()?;
                if known.is_none() && std::fs::remove_file(file.path()).is_ok() {
                    report.orphan_files_removed += 1;
                }
            }
        }
        Ok(report)
    }
}
