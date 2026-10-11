// Cada archivo de tests usa solo parte de estos helpers.
#![allow(dead_code)]

//! Helpers compartidos por los tests del CLI.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// `symphonyd` al día. Vive en otro paquete, así que cargo no lo reconstruye
/// para los tests de este: se construye una vez por proceso (rápido si ya está al día).
pub fn symphonyd() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let status = Command::new(env!("CARGO"))
                .args(["build", "-q", "-p", "symphony-daemon", "--bin", "symphonyd"])
                .status()
                .expect("cargo build symphonyd");
            assert!(status.success(), "no se pudo construir symphonyd");
            symphony_testkit::pinned_bin(
                &PathBuf::from(env!("CARGO_BIN_EXE_symphony"))
                    .with_file_name(format!("symphonyd{}", std::env::consts::EXE_SUFFIX)),
            )
        })
        .clone()
}

/// `fake-agent` al día (paquete testkit), para simular CLIs en el PATH.
pub fn fake_agent() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let status = Command::new(env!("CARGO"))
                .args([
                    "build",
                    "-q",
                    "-p",
                    "symphony-testkit",
                    "--bin",
                    "fake-agent",
                ])
                .status()
                .expect("cargo build fake-agent");
            assert!(status.success(), "no se pudo construir fake-agent");
            symphony_testkit::pinned_bin(
                &PathBuf::from(env!("CARGO_BIN_EXE_symphony"))
                    .with_file_name(format!("fake-agent{}", std::env::consts::EXE_SUFFIX)),
            )
        })
        .clone()
}

/// El CI no tiene CLIs reales: un `claude` falso (`<home>/../bin`) delante del PATH deja un
/// proveedor elegible, igual que en la máquina de quien desarrolla.
pub fn path_with_fake_claude(home: &std::path::Path) -> std::ffi::OsString {
    let bin = home.parent().unwrap().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let claude = bin.join(format!("claude{}", std::env::consts::EXE_SUFFIX));
    if !claude.exists() {
        std::fs::copy(fake_agent(), &claude).unwrap();
    }
    let rest = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&rest))).unwrap()
}
