// Cada archivo de tests usa solo parte de estos helpers.
#![allow(dead_code)]

//! Helpers compartidos por los tests del CLI.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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

/// Detiene el daemon del home y espera a que su proceso termine, incluso si el test falla.
///
/// `daemon stop` espera a que se libere el lock de instancia, pero en Windows el proceso puede
/// seguir destruyendo su runtime durante unos instantes. Nextest examina los hijos justo al salir
/// del test, así que ese intervalo se marca como `LEAK`.
pub struct DaemonGuard {
    home: PathBuf,
}

impl DaemonGuard {
    pub fn new(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_symphony"));
        command
            .env("SYMPHONY_HOME", &self.home)
            .env("SYMPHONYD", symphonyd());
        command
    }

    fn pid(&self) -> Option<u32> {
        let output = self.command().args(["daemon", "status"]).output().ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.trim().strip_prefix("pid:"))
            .and_then(|pid| pid.trim().parse().ok())
    }

    /// Para el daemon y no devuelve el control hasta que el proceso ya no exista.
    pub fn stop(&self) -> std::io::Result<Output> {
        let pid = self.pid();
        let output = self.command().args(["daemon", "stop"]).output();
        wait_for_process_exit(pid);
        output
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Para el daemon de `home` y espera a que su proceso muera del todo.
///
/// Para los tests que paran el daemon al final con `symphony daemon stop`: la orden devuelve el
/// control cuando el daemon suelta el lock, no cuando el proceso sale, y nextest mira justo en ese
/// hueco. El PID se lee antes de parar, que es el único momento en que existe.
pub fn stop_daemon(home: &Path) -> Output {
    DaemonGuard::new(home).stop().expect("daemon stop")
}

#[cfg(windows)]
fn wait_for_process_exit(pid: Option<u32>) {
    if let Some(pid) = pid {
        // `stop` ya intentó el apagado ordenado. Si el ejecutable sigue vivo, terminamos todo su
        // árbol y esperamos a que taskkill regrese antes de dejar terminar el test.
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output();
    }
}

#[cfg(not(windows))]
fn wait_for_process_exit(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };
    let pid = pid.to_string();
    let alive = || {
        Command::new("kill")
            .args(["-0", &pid])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while alive() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if alive() {
        let _ = Command::new("kill").args(["-9", &pid]).status();
    }
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
