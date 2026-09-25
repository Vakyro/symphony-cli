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
            PathBuf::from(env!("CARGO_BIN_EXE_symphony"))
                .with_file_name(format!("symphonyd{}", std::env::consts::EXE_SUFFIX))
        })
        .clone()
}
