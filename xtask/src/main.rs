//! Tareas de desarrollo: `cargo xtask <tarea>` (STACK §4.2).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod er;

const CHECK: &[&[&str]] = &[
    &["fmt", "--all", "--check"],
    &[
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ],
    &["nextest", "run", "--workspace"],
];

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("check") => check(),
        Some("er") => {
            let root = workspace_root();
            match er::run(&root.join("migrations"), &root.join("docs/book/src/er.md")) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("xtask: er: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("uso: cargo xtask <check|er>");
            ExitCode::from(2)
        }
    }
}

/// Raíz del workspace (xtask vive en `<workspace>/xtask`).
fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .nth(1)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir.to_path_buf())
}

fn check() -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    for args in CHECK {
        eprintln!("xtask: cargo {}", args.join(" "));
        // El gate nunca gasta cuota (PLAN §6): los `live_*` se omiten aunque la terminal herede
        // `SYMPHONY_LIVE=1`. Para correr un live, llama a `cargo nextest` directamente.
        match Command::new(&cargo)
            .args(*args)
            .env_remove("SYMPHONY_LIVE")
            .status()
        {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!("xtask: falló `cargo {}` ({s})", args.join(" "));
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!("xtask: no se pudo lanzar cargo: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}
