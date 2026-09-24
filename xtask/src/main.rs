//! Tareas de desarrollo: `cargo xtask <tarea>` (STACK §4.2).

use std::process::{Command, ExitCode};

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
        _ => {
            eprintln!("uso: cargo xtask check");
            ExitCode::from(2)
        }
    }
}

fn check() -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    for args in CHECK {
        eprintln!("xtask: cargo {}", args.join(" "));
        match Command::new(&cargo).args(*args).status() {
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
