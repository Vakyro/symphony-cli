//! P05.S6: `symphony providers` con un PATH simulado (ninguno, solo Claude, Claude + Codex).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::{fake_agent, symphonyd};

use std::path::Path;
use std::process::Command;

/// Corre `symphony providers` con un home propio y solo `bin` en el PATH.
fn providers(bin: &Path, home: &Path) -> String {
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_symphony"))
            .args(args)
            .env("SYMPHONY_HOME", home)
            .env("SYMPHONYD", symphonyd())
            .env("PATH", bin)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let listed = run(&["providers"]);
    run(&["daemon", "stop"]);
    listed
}

fn row<'a>(out: &'a str, name: &str) -> &'a str {
    out.lines()
        .find(|l| l.starts_with(name))
        .unwrap_or_else(|| panic!("sin fila {name}:\n{out}"))
}

#[test]
fn detection_with_simulated_path() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = |name: &str| bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));

    // Ninguno.
    let out = providers(&bin, &dir.path().join("h1"));
    assert!(out.starts_with("PROVEEDOR"), "{out}");
    assert!(
        row(&out, "Claude").contains("NOT_FOUND") && row(&out, "Codex").contains("NOT_FOUND"),
        "{out}"
    );

    // Solo "Claude" (fake-agent con ese nombre responde --version como Claude Code).
    std::fs::copy(fake_agent(), exe("claude")).unwrap();
    let out = providers(&bin, &dir.path().join("h2"));
    let claude = row(&out, "Claude");
    assert!(
        claude.contains("READY") && claude.contains("9.9.9"),
        "{out}"
    );
    assert!(claude.contains(" 4 "), "4 modelos de Claude: {out}");
    assert!(row(&out, "Codex").contains("NOT_FOUND"), "{out}");

    // Claude + Codex.
    std::fs::copy(fake_agent(), exe("codex")).unwrap();
    let out = providers(&bin, &dir.path().join("h3"));
    assert!(row(&out, "Claude").contains("READY"), "{out}");
    let codex = row(&out, "Codex");
    assert!(codex.contains("READY") && codex.contains("9.9.9"), "{out}");
}
