//! Que Symphony no se coma el disco (pedido de Leo, 2026-10-24).
//!
//! - Los agentes de un proyecto Rust comparten el `build-dir` de Cargo (lo intermedio) en
//!   `<home>/cache/cargo-build/<n>`, con [`TARGET_SLOTS`] carpetas: las dependencias se compilan
//!   pocas veces y no una por worktree, y Cargo (que bloquea su carpeta mientras compila) no pone
//!   en fila a todos los agentes. Los binarios finales quedan en el `target/` de cada worktree:
//!   compartido, el `symphonyd.exe` de un agente pisaba el de otro (dogfooding 2026-10-26).
//! - El worktree de un agente cancelado, ya integrado o completado hace más de 7 días se borra
//!   (la rama queda; el diff sale del checkpoint). Uno con cambios sin commitear no se toca.

use std::path::{Path, PathBuf};

use symphony_git::Repo;
use symphony_store::repo;

use crate::runtime::{Runtime, now_ms};

/// Tope del `target/` compartido: al arrancar, si lo pasa, se borra entero (es caché).
/// ponytail: tope fijo; hacerlo configurable si alguien lo pide.
const CARGO_TARGET_CAP: u64 = 15 * 1024 * 1024 * 1024;

/// Cuánto se guarda el worktree de un agente completado que nadie integró con Symphony.
const COMPLETED_KEEP_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// Cuántos `target/` compartidos hay. Con uno solo, seis agentes compilando esperaban el candado
/// de Cargo y sus comandos vencían (dogfooding 2026-10-24); uno por agente llenaba el disco.
/// ponytail: fijo; ajustarlo al número de agentes simultáneos si eso cambia.
const TARGET_SLOTS: u32 = 3;

fn cargo_cache(home: &Path) -> PathBuf {
    home.join("cache").join("cargo-build")
}

/// Donde estaba la caché antes de 2026-10-26 (un `target/` entero compartido): se borra.
fn legacy_cargo_target(home: &Path) -> PathBuf {
    home.join("cache").join("cargo-target")
}

/// `CARGO_BUILD_BUILD_DIR` compartido para un worktree de Symphony con `Cargo.toml`, salvo que
/// el usuario ya haya elegido dónde compila.
pub fn cargo_env(home: &Path, worktree: &Path) -> Vec<(String, String)> {
    // En Windows las rutas no distinguen mayúsculas (`Desktop` y `desktop` son lo mismo).
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    let ours = Path::new(&norm(worktree)).starts_with(norm(&home.join("worktrees")));
    if !ours
        || !worktree.join("Cargo.toml").is_file()
        || std::env::var_os("CARGO_TARGET_DIR").is_some()
        || std::env::var_os("CARGO_BUILD_BUILD_DIR").is_some()
    {
        return Vec::new();
    }
    // `agent-007` → carpeta 1: agentes consecutivos caen en carpetas distintas.
    let slot = worktree
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.rsplit('-').next())
        .and_then(|n| n.parse::<u32>().ok())
        .map_or(0, |n| n % TARGET_SLOTS);
    vec![(
        "CARGO_BUILD_BUILD_DIR".into(),
        cargo_cache(home)
            .join(slot.to_string())
            .display()
            .to_string(),
    )]
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

impl Runtime {
    /// Borra los worktrees que ya no hacen falta y, si `trim_cache`, el `target/` compartido
    /// cuando pasa el tope. Best effort: lo que falla queda para la próxima vez.
    pub async fn reclaim_disk(&self, trim_cache: bool) {
        let rows = {
            let Ok(conn) = self.reader.lock() else { return };
            repo::reclaimable_worktrees(&conn, now_ms() - COMPLETED_KEEP_MS).unwrap_or_default()
        };
        let home = self.home.clone();
        let removed = tokio::task::spawn_blocking(move || {
            if trim_cache {
                let _ = std::fs::remove_dir_all(legacy_cargo_target(&home));
                let cache = cargo_cache(&home);
                if dir_size(&cache) > CARGO_TARGET_CAP {
                    tracing::info!(path = %cache.display(), "caché de Cargo compartida sobre el tope: se borra");
                    let _ = std::fs::remove_dir_all(&cache);
                }
            }
            rows.into_iter()
                .filter_map(|(id, path, root)| {
                    let path = PathBuf::from(path);
                    let repo = Repo::at(&root);
                    if path.exists() {
                        if !Repo::at(&path).is_clean().unwrap_or(false) {
                            tracing::info!(path = %path.display(), "worktree con cambios sin commitear: no se borra");
                            return None;
                        }
                        // Si git no puede (en Windows, un archivo abierto), se reintenta otro día:
                        // borrar a mano lo dejaría a medias.
                        repo.worktree_remove(&path, true).ok()?;
                    }
                    let _ = repo.worktree_prune();
                    Some(id)
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        if removed.is_empty() {
            return;
        }
        tracing::info!(count = removed.len(), "worktrees terminados borrados");
        let _ = self
            .writer
            .write(Box::new(move |t| {
                let now = now_ms();
                for id in removed {
                    repo::set_worktree_status(t, id, "REMOVED", None, now)?;
                }
                Ok(())
            }))
            .await;
    }
}
