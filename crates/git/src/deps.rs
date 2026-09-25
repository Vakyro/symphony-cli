//! Dependencias por worktree (STACK §12.2, IDEA §5.9): evitar una instalación
//! completa por agente. Nunca se cambia el package manager del proyecto ni su
//! configuración: se usa lo que el proyecto ya usa.
//!
//! Orden de preferencia: pnpm con su store compartido → enlace al
//! `node_modules` del repo base si el lockfile es idéntico → instalación
//! (operación de clase 4: la ejecuta el scheduler, no este módulo).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Pnpm,
    Npm,
    Yarn,
    Bun,
    Cargo,
    Uv,
    Poetry,
}

/// Lockfiles en orden de detección (STACK §12.2).
const LOCKFILES: [(&str, PackageManager); 8] = [
    ("pnpm-lock.yaml", PackageManager::Pnpm),
    ("package-lock.json", PackageManager::Npm),
    ("yarn.lock", PackageManager::Yarn),
    ("bun.lock", PackageManager::Bun),
    ("bun.lockb", PackageManager::Bun),
    ("Cargo.lock", PackageManager::Cargo),
    ("uv.lock", PackageManager::Uv),
    ("poetry.lock", PackageManager::Poetry),
];

/// Valores de `worktrees.deps_strategy` (DB §3.C).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepsStrategy {
    PnpmStore,
    Link,
    Install,
    None,
}

impl DepsStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PnpmStore => "PNPM_STORE",
            Self::Link => "LINK",
            Self::Install => "INSTALL",
            Self::None => "NONE",
        }
    }
}

/// Qué hacer con las dependencias de un worktree nuevo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepsPlan {
    pub manager: Option<PackageManager>,
    pub strategy: DepsStrategy,
    /// Hash BLAKE3 del lockfile (`worktrees.deps_lock_hash`).
    pub lock_hash: Option<String>,
    /// Comando a ejecutar en el worktree (`PNPM_STORE` o `INSTALL`).
    pub command: Option<Vec<String>>,
}

#[derive(Debug, thiserror::Error)]
pub enum DepsError {
    #[error("E/S con {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("no se pudo enlazar {link} → {target}: {detail}")]
    Link {
        link: PathBuf,
        target: PathBuf,
        detail: String,
    },
}

/// Package manager y lockfile del directorio, si hay uno reconocible.
pub fn detect(root: &Path) -> Option<(PackageManager, PathBuf)> {
    LOCKFILES
        .iter()
        .map(|(f, pm)| (*pm, root.join(f)))
        .find(|(_, p)| p.is_file())
}

pub fn lock_hash(lockfile: &Path) -> Result<String, DepsError> {
    let bytes = std::fs::read(lockfile).map_err(|source| DepsError::Io {
        path: lockfile.to_path_buf(),
        source,
    })?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn install_command(pm: PackageManager) -> Option<Vec<String>> {
    let cmd: &[&str] = match pm {
        PackageManager::Pnpm => &["pnpm", "install", "--frozen-lockfile", "--prefer-offline"],
        PackageManager::Npm => &["npm", "ci", "--no-audit", "--no-fund"],
        PackageManager::Yarn => &["yarn", "install", "--frozen-lockfile"],
        PackageManager::Bun => &["bun", "install", "--frozen-lockfile"],
        PackageManager::Cargo | PackageManager::Uv | PackageManager::Poetry => return None,
    };
    Some(cmd.iter().map(|s| s.to_string()).collect())
}

/// Decide la estrategia para `worktree`, comparando con el repo base.
pub fn plan(worktree: &Path, base: &Path) -> Result<DepsPlan, DepsError> {
    let Some((pm, lockfile)) = detect(worktree) else {
        return Ok(DepsPlan {
            manager: None,
            strategy: DepsStrategy::None,
            lock_hash: None,
            command: None,
        });
    };
    let hash = lock_hash(&lockfile)?;
    let (strategy, command) = match pm {
        // pnpm ya comparte su store (hardlinks) entre instalaciones del mismo disco.
        PackageManager::Pnpm => (DepsStrategy::PnpmStore, install_command(pm)),
        PackageManager::Cargo | PackageManager::Uv | PackageManager::Poetry => {
            (DepsStrategy::None, None)
        }
        PackageManager::Npm | PackageManager::Yarn | PackageManager::Bun => {
            let base_lock = detect(base).filter(|(b, _)| *b == pm).map(|(_, p)| p);
            let same_lock = match base_lock {
                Some(p) => lock_hash(&p)? == hash,
                None => false,
            };
            if same_lock && base.join("node_modules").is_dir() {
                (DepsStrategy::Link, None)
            } else {
                (DepsStrategy::Install, install_command(pm))
            }
        }
    };
    Ok(DepsPlan {
        manager: Some(pm),
        strategy,
        lock_hash: Some(hash),
        command,
    })
}

/// Aplica `LINK`: `<worktree>/node_modules` apunta al `node_modules` del repo base.
/// En Windows es una junction (no pide privilegios); en Unix, un symlink.
pub fn link_node_modules(worktree: &Path, base: &Path) -> Result<(), DepsError> {
    let target = base.join("node_modules");
    let link = worktree.join("node_modules");
    if link.exists() {
        return Ok(());
    }
    let err = |detail: String| DepsError::Link {
        link: link.clone(),
        target: target.clone(),
        detail,
    };
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, &link).map_err(|e| err(e.to_string()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Una ruta de Windows no puede contener comillas: entre comillas es seguro para cmd.
        let out = Command::new("cmd")
            .raw_arg(format!(
                "/c mklink /J \"{}\" \"{}\"",
                link.display(),
                target.display()
            ))
            .stdin(Stdio::null())
            .output()
            .map_err(|e| err(e.to_string()))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(err(String::from_utf8_lossy(&out.stderr).trim().to_string()))
        }
    }
}

/// Corre el comando del plan en el worktree (pnpm o instalación). Bloquea hasta que termina.
/// El scheduler (P08) decide cuándo llamarlo: es una operación de clase 4.
pub fn run_command(
    worktree: &Path,
    command: &[String],
) -> Result<std::process::ExitStatus, DepsError> {
    let (program, args) = command.split_first().ok_or_else(|| DepsError::Io {
        path: worktree.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "comando vacío"),
    })?;
    // En Windows los package managers de Node son shims `.cmd` (LEARNINGS Test A).
    let program = if cfg!(windows) {
        format!("{program}.cmd")
    } else {
        program.clone()
    };
    Command::new(&program)
        .args(args)
        .current_dir(worktree)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|source| DepsError::Io {
            path: worktree.to_path_buf(),
            source,
        })
}
