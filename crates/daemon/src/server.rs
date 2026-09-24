//! Servidor IPC del daemon: una sola instancia por directorio de Symphony,
//! métodos `ping`, `status` y `shutdown`, timeout por request y apagado ordenado.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use symphony_protocol::transport::{self, ListenerExt as _, LocalStream};
use symphony_protocol::{Connection, Message, PROTOCOL_VERSION, Request, Response};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Tiempo máximo para atender un request (STACK §47, §49).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Tiempo que se espera a las conexiones abiertas al apagar.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("ya hay un daemon de Symphony corriendo para {home}{}", pid.map(|p| format!(" (pid {p})")).unwrap_or_default())]
    AlreadyRunning { home: PathBuf, pid: Option<u32> },
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
}

fn io(context: impl Into<String>) -> impl FnOnce(std::io::Error) -> DaemonError {
    let context = context.into();
    move |source| DaemonError::Io { context, source }
}

/// Lock exclusivo sobre `<home>/run/symphonyd.lock`. Lo suelta el sistema
/// operativo cuando el proceso termina, aunque sea por un crash.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
    pid_path: PathBuf,
}

impl InstanceLock {
    pub fn acquire(home: &Path) -> Result<Self, DaemonError> {
        let dir = transport::run_dir(home);
        std::fs::create_dir_all(&dir).map_err(io(format!("no se pudo crear {}", dir.display())))?;
        let lock_path = dir.join("symphonyd.lock");
        let pid_path = dir.join("symphonyd.pid");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(io(format!("no se pudo abrir {}", lock_path.display())))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                let pid = std::fs::read_to_string(&pid_path)
                    .ok()
                    .and_then(|s| s.trim().parse().ok());
                return Err(DaemonError::AlreadyRunning {
                    home: home.to_path_buf(),
                    pid,
                });
            }
            Err(TryLockError::Error(e)) => {
                return Err(io(format!("no se pudo bloquear {}", lock_path.display()))(
                    e,
                ));
            }
        }
        let mut pid_file = File::create(&pid_path)
            .map_err(io(format!("no se pudo escribir {}", pid_path.display())))?;
        writeln!(pid_file, "{}", std::process::id()).map_err(io("no se pudo escribir el pid"))?;
        Ok(Self {
            _file: file,
            pid_path,
        })
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.pid_path);
    }
}

struct State {
    started: Instant,
    home: PathBuf,
    shutdown: CancellationToken,
}

/// Corre el daemon hasta que se cancele `shutdown` (señal o request `shutdown`).
pub async fn serve(home: &Path, shutdown: CancellationToken) -> Result<(), DaemonError> {
    let _lock = InstanceLock::acquire(home)?;
    let listener = transport::listen(home).map_err(io("no se pudo abrir el socket IPC"))?;
    tracing::info!(pid = std::process::id(), home = %home.display(), "daemon listo");
    let state = Arc::new(State {
        started: Instant::now(),
        home: home.to_path_buf(),
        shutdown: shutdown.clone(),
    });
    let tracker = TaskTracker::new();

    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok(stream) => { tracker.spawn(handle(stream, state.clone())); }
                Err(e) => tracing::warn!(error = %e, "fallo al aceptar una conexión"),
            },
        }
    }

    tracker.close();
    if tokio::time::timeout(DRAIN_TIMEOUT, tracker.wait())
        .await
        .is_err()
    {
        tracing::warn!("había conexiones abiertas al apagar; se cierran");
    }
    drop(listener);
    #[cfg(unix)]
    let _ = std::fs::remove_file(transport::run_dir(home).join("symphonyd.sock"));
    tracing::info!("daemon detenido");
    Ok(())
}

async fn handle(stream: LocalStream, state: Arc<State>) {
    let mut conn = Connection::new(stream);
    loop {
        let msg = tokio::select! {
            () = state.shutdown.cancelled() => return,
            msg = conn.recv() => msg,
        };
        let reply = match msg {
            Ok(Some(Message::Request(req))) => {
                let id = req.id.clone();
                match tokio::time::timeout(REQUEST_TIMEOUT, dispatch(req, &state)).await {
                    Ok(resp) => resp,
                    Err(_) => Response::error(id, "timeout", "el daemon no respondió a tiempo"),
                }
            }
            Ok(Some(Message::Subscribe(sub))) => Response::error(
                sub.id,
                "unsupported",
                "las suscripciones a eventos llegan con el event bus (P05)",
            ),
            Ok(Some(_)) => Response::error(
                "",
                "invalid_message",
                "el cliente solo puede enviar request o subscribe",
            ),
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(error = %e, "mensaje inválido; se cierra la conexión");
                let _ = conn
                    .send(&Message::Response(Response::error(
                        "",
                        "bad_request",
                        e.to_string(),
                    )))
                    .await;
                return;
            }
        };
        if conn.send(&Message::Response(reply)).await.is_err() {
            return;
        }
    }
}

async fn dispatch(req: Request, state: &State) -> Response {
    if !(req.params.is_null() || req.params.is_object()) {
        return Response::error(req.id, "invalid_params", "`params` tiene que ser un objeto");
    }
    match req.method.as_str() {
        "ping" => Response::ok(req.id, json!({ "pong": true })),
        "status" => Response::ok(req.id, status(state)),
        "shutdown" => {
            tracing::info!("apagado pedido por un cliente");
            state.shutdown.cancel();
            Response::ok(req.id, json!({ "stopping": true }))
        }
        other => Response::error(
            req.id,
            "unknown_method",
            format!("método desconocido: `{other}`"),
        ),
    }
}

fn status(state: &State) -> Value {
    json!({
        "pid": std::process::id(),
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": PROTOCOL_VERSION,
        "uptime_ms": u64::try_from(state.started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "home": state.home.display().to_string(),
    })
}
