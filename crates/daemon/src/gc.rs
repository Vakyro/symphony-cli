//! Que Symphony no se coma el disco (pedido de Leo, 2026-10-24).
//!
//! - Los agentes de un proyecto Rust comparten un `target/` (`<home>/cache/cargo-target`): las
//!   dependencias se compilan una vez y no una por worktree.
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

fn cargo_target(home: &Path) -> PathBuf {
    home.join("cache").join("cargo-target")
}

/// `CARGO_TARGET_DIR` compartido para un worktree de Symphony con `Cargo.toml`, salvo que el
/// usuario ya haya elegido uno.
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
    {
        return Vec::new();
    }
    vec![(
        "CARGO_TARGET_DIR".into(),
        cargo_target(home).display().to_string(),
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
                let target = cargo_target(&home);
                if dir_size(&target) > CARGO_TARGET_CAP {
                    tracing::info!(path = %target.display(), "target/ compartido sobre el tope: se borra");
                    let _ = std::fs::remove_dir_all(&target);
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
