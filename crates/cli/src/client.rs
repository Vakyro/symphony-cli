//! Cliente del daemon: conexión, autoarranque y llamadas (IDEA §10: el CLI
//! arranca el daemon si no está vivo).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use miette::Diagnostic;
use serde_json::Value;
use symphony_protocol::transport::{self, LocalStream};
use symphony_protocol::{Connection, Message, Outcome, ProtocolError, Request};

const START_TIMEOUT: Duration = Duration::from_secs(10);
const CALL_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum ClientError {
    #[error("no encontré el ejecutable `symphonyd`")]
    #[diagnostic(help(
        "instálalo junto a `symphony`, ponlo en el PATH o define SYMPHONYD con su ruta"
    ))]
    DaemonNotFound,
    #[error("no se pudo arrancar `{bin}`: {source}")]
    Spawn { bin: String, source: std::io::Error },
    #[error("el daemon terminó al arrancar")]
    #[diagnostic(help("{detail}"))]
    DaemonExited { detail: String },
    #[error("el daemon no respondió en {secs} s")]
    #[diagnostic(help("revisa los logs en {logs}"))]
    StartTimeout { secs: u64, logs: String },
    #[error("el daemon no respondió a `{method}` a tiempo")]
    CallTimeout { method: String },
    #[error("el daemon devolvió un error ({code}): {message}")]
    Remote { code: String, message: String },
    #[error("se perdió la conexión con el daemon")]
    Disconnected,
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub async fn try_connect(home: &Path) -> Option<Connection<LocalStream>> {
    transport::connect(home).await.ok()
}

/// Ruta de `symphonyd`: `$SYMPHONYD`, junto a este ejecutable o en el PATH.
fn daemon_binary() -> Result<PathBuf, ClientError> {
    if let Some(p) = std::env::var_os("SYMPHONYD").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let name = format!("symphonyd{}", std::env::consts::EXE_SUFFIX);
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(&name)));
    if let Some(path) = sibling.filter(|p| p.is_file()) {
        return Ok(path);
    }
    let path_var = std::env::var_os("PATH").ok_or(ClientError::DaemonNotFound)?;
    std::env::split_paths(&path_var)
        .map(|d| d.join(&name))
        .find(|p| p.is_file())
        .ok_or(ClientError::DaemonNotFound)
}

/// Lanza `symphonyd` desacoplado de esta terminal y espera a que responda.
/// Devuelve el pid del proceso lanzado.
pub async fn start_daemon(home: &Path) -> Result<u32, ClientError> {
    let bin = daemon_binary()?;
    let logs = home.join("logs");
    std::fs::create_dir_all(&logs)?;
    // stderr a archivo (no a un pipe que se rompe cuando este proceso termina).
    let err_path = logs.join("symphonyd-start.err");
    let mut cmd = Command::new(&bin);
    cmd.env("SYMPHONY_HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(File::create(&err_path)?);
    detach(&mut cmd);
    let mut child = cmd.spawn().map_err(|source| ClientError::Spawn {
        bin: bin.display().to_string(),
        source,
    })?;

    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if transport::connect(home).await.is_ok() {
            return Ok(child.id());
        }
        if let Some(status) = child.try_wait()? {
            let detail = std::fs::read_to_string(&err_path).unwrap_or_default();
            let detail = if detail.trim().is_empty() {
                format!("salió con {status}")
            } else {
                detail.trim().to_string()
            };
            return Err(ClientError::DaemonExited { detail });
        }
        if Instant::now() > deadline {
            return Err(ClientError::StartTimeout {
                secs: START_TIMEOUT.as_secs(),
                logs: logs.display().to_string(),
            });
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(windows)]
fn detach(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    stop_inheriting_std_handles();
}

/// `CreateProcess` con `bInheritHandles` hace que el hijo herede **todos** los
/// handles heredables, incluidos el stdin/stdout/stderr de este proceso. Si
/// quien nos lanzó (una terminal, un test) espera EOF en esos pipes, el daemon
/// los mantendría abiertos para siempre (LEARNINGS H4). Este proceso los sigue
/// usando igual; solo dejan de pasar a los hijos.
#[cfg(windows)]
#[allow(unsafe_code)]
fn stop_inheriting_std_handles() {
    use windows_sys::Win32::Foundation::{
        HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: GetStdHandle no toma punteros; devuelve un handle del proceso,
        // NULL o INVALID_HANDLE_VALUE, que se descartan antes de usarlo.
        let handle = unsafe { GetStdHandle(which) };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            continue;
        }
        // SAFETY: `handle` es un handle válido de este proceso; solo se cambia su
        // flag de herencia. Si falla (p. ej. una consola), no hay nada que heredar.
        unsafe {
            SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
        }
    }
}

#[cfg(unix)]
fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    // Grupo propio: un Ctrl-C en la terminal del usuario no llega al daemon.
    cmd.process_group(0);
}

/// Resultado de asegurar que haya un daemon corriendo.
pub enum Ensured {
    /// Ya había uno; aquí está la conexión.
    Running(Connection<LocalStream>),
    /// Se arrancó uno nuevo con este pid.
    Started(u32, Connection<LocalStream>),
}

/// Conecta con el daemon; si no está vivo, lo arranca.
///
/// Si el socket no responde pero el lock de instancia está tomado, hay un
/// daemon arrancando o apagándose: se espera a que conteste o suelte el lock
/// antes de lanzar otro (si no, el nuevo chocaría con el lock).
pub async fn ensure_running(home: &Path) -> Result<Ensured, ClientError> {
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if let Some(conn) = try_connect(home).await {
            return Ok(Ensured::Running(conn));
        }
        if !transport::daemon_lock_held(home) {
            let pid = start_daemon(home).await?;
            return Ok(Ensured::Started(pid, transport::connect(home).await?));
        }
        if Instant::now() > deadline {
            return Err(ClientError::StartTimeout {
                secs: START_TIMEOUT.as_secs(),
                logs: home.join("logs").display().to_string(),
            });
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub async fn connect_or_start(home: &Path) -> Result<Connection<LocalStream>, ClientError> {
    match ensure_running(home).await? {
        Ensured::Running(conn) | Ensured::Started(_, conn) => Ok(conn),
    }
}

/// Espera a que el daemon termine del todo (lock liberado), no solo a que cierre el socket.
pub async fn wait_stopped(home: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while transport::daemon_lock_held(home) {
        if Instant::now() > deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    true
}

pub async fn call(
    conn: &mut Connection<LocalStream>,
    method: &str,
    params: Value,
) -> Result<Value, ClientError> {
    let id = symphony_core::RunId::new().to_string();
    let exchange = async {
        conn.send(&Message::Request(Request::new(&id, method, params)))
            .await?;
        loop {
            match conn.recv().await? {
                Some(Message::Response(r)) if r.id == id => return Ok(r.outcome),
                Some(_) => continue,
                None => return Err(ClientError::Disconnected),
            }
        }
    };
    match tokio::time::timeout(CALL_TIMEOUT, exchange).await {
        Err(_) => Err(ClientError::CallTimeout {
            method: method.to_string(),
        }),
        Ok(Err(e)) => Err(e),
        Ok(Ok(Outcome::Ok(v))) => Ok(v),
        Ok(Ok(Outcome::Error(e))) => Err(ClientError::Remote {
            code: e.code,
            message: e.message,
        }),
    }
}
