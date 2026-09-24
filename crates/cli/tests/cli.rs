//! Tests `trycmd` del binario `symphony` (STACK §24.2).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::Command;

/// `symphonyd` vive en otro paquete: cargo no lo construye para los tests de este.
fn symphonyd() -> PathBuf {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_symphony"))
        .with_file_name(format!("symphonyd{}", std::env::consts::EXE_SUFFIX));
    if !bin.is_file() {
        let status = Command::new(env!("CARGO"))
            .args(["build", "-p", "symphony-daemon", "--bin", "symphonyd"])
            .status()
            .expect("cargo build symphonyd");
        assert!(status.success(), "no se pudo construir symphonyd");
    }
    bin
}

#[test]
fn cli_commands() {
    let home = tempfile::tempdir().unwrap();
    trycmd::TestCases::new()
        .env(
            "SYMPHONY_HOME",
            home.path().join("home").display().to_string(),
        )
        .env("SYMPHONYD", symphonyd().display().to_string())
        .case("tests/cmd/*.toml")
        .case("tests/cmd/*.md");
}
