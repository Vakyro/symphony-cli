//! Servidor IPC del daemon: una sola instancia por directorio de Symphony,
//! métodos `ping`, `status` y `shutdown`, timeout por request y apagado ordenado.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use symphony_protocol::transport::{self, ListenerExt as _, LocalStream};
use symphony_protocol::{Connection, Message, PROTOCOL_VERSION, Request, Response};
use symphony_store::{Writer, repo};

use crate::bus::{BusEvent, EventBus, EventSource};

/// Eventos que un suscriptor de la TUI puede atrasarse antes de perder los viejos.
const TUI_BUFFER: usize = 1024;
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
    #[error("base de datos: {0}")]
    Store(#[from] symphony_store::StoreError),
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
        let lock_path = transport::lock_path(home);
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
    /// Conexión de solo lectura (las escrituras van por el `Writer`).
    reader: Mutex<rusqlite::Connection>,
    bus: EventBus,
    writer: symphony_store::WriterHandle,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Recuperación al arrancar (P03.S5): con el lock tomado, toda sesión `ACTIVE`
/// quedó de un daemon que murió. Se marca `INTERRUPTED` y va al Recovery Center.
async fn recover(writer: &Writer) -> Result<(), DaemonError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    writer
        .handle()
        .write(Box::new(move |t| {
            let recovered = repo::interrupt_orphan_sessions(t, now_ms())?;
            let _ = tx.send(recovered.len());
            Ok(())
        }))
        .await?;
    let recovered = rx.await.unwrap_or(0);
    if recovered > 0 {
        tracing::warn!(
            sesiones = recovered,
            "sesiones interrumpidas por un cierre inesperado; ver Recovery Center"
        );
    }
    Ok(())
}

/// Corre el daemon hasta que se cancele `shutdown` (señal o request `shutdown`).
pub async fn serve(home: &Path, shutdown: CancellationToken) -> Result<(), DaemonError> {
    let _lock = InstanceLock::acquire(home)?;
    let db_path = symphony_core::SymphonyHome::at(home).db_path();
    let writer = Writer::start(&db_path)?;
    recover(&writer).await?;
    if let Err(e) = refresh_providers(&writer.handle()).await {
        tracing::warn!(error = %e, "no se pudo registrar a los proveedores");
    }
    let reader = writer.reader()?;
    let listener = transport::listen(home).map_err(io("no se pudo abrir el socket IPC"))?;
    tracing::info!(pid = std::process::id(), home = %home.display(), "daemon listo");
    let state = Arc::new(State {
        started: Instant::now(),
        home: home.to_path_buf(),
        shutdown: shutdown.clone(),
        reader: Mutex::new(reader),
        bus: EventBus::new(writer.handle(), TUI_BUFFER),
        writer: writer.handle(),
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
    transport::cleanup(home);
    writer.shutdown();
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
        "hook.emit" => hook_emit(req, state).await,
        "providers.list" => providers_list(req, state),
        "providers.refresh" => match refresh_providers(&state.writer).await {
            Ok(()) => providers_list(req, state),
            Err(e) => Response::error(req.id, "store_error", e.to_string()),
        },
        other => Response::error(
            req.id,
            "unknown_method",
            format!("método desconocido: `{other}`"),
        ),
    }
}

/// Detecta los CLIs (fuera del runtime: lanza procesos) y guarda el resultado.
async fn refresh_providers(
    writer: &symphony_store::WriterHandle,
) -> Result<(), symphony_store::StoreError> {
    let detected =
        tokio::task::spawn_blocking(|| crate::providers::detect_all(&crate::providers::builtin()))
            .await
            .unwrap_or_default();
    crate::providers::save(writer, detected, now_ms()).await
}

fn providers_list(req: Request, state: &State) -> Response {
    let listed = state.reader.lock().ok().map(|c| crate::providers::list(&c));
    match listed {
        Some(Ok(v)) => Response::ok(req.id, json!({ "providers": v })),
        Some(Err(e)) => Response::error(req.id, "store_error", e.to_string()),
        None => Response::error(req.id, "store_error", "lector de la base no disponible"),
    }
}

/// Un hook de un CLI (vía `symphony hook emit`) → eventos canónicos → bus.
/// En P05 la decisión es siempre `allow`; el scheduler (P08) podrá retener o denegar.
async fn hook_emit(req: Request, state: &State) -> Response {
    let p = &req.params;
    let text = |k: &str| {
        p.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let (Some(agent_id), Some(project_id)) = (text("agent_id"), text("project_id")) else {
        return Response::error(req.id, "invalid_params", "faltan `agent_id` o `project_id`");
    };
    let Some(payload) = p.get("payload").filter(|v| v.is_object()) else {
        return Response::error(
            req.id,
            "invalid_params",
            "`payload` tiene que ser el JSON del hook",
        );
    };
    let known = state
        .reader
        .lock()
        .ok()
        .and_then(|c| {
            c.query_row(
                "SELECT 1 FROM agents WHERE id = ?1 AND project_id = ?2",
                [&agent_id, &project_id],
                |_| Ok(()),
            )
            .ok()
        })
        .is_some();
    if !known {
        return Response::error(
            req.id,
            "unknown_agent",
            format!("el agente `{agent_id}` no existe en este proyecto"),
        );
    }
    // El esquema de hooks es común a Claude Code y Codex (P01 Test B). Con el
    // registro de adapters (P05.S6) se usa el `parse_hook` del proveedor.
    let events = symphony_adapter_common::hooks::parse_standard_hook(payload);
    let n = events.len();
    for event in events {
        let ev = BusEvent {
            project_id: project_id.clone(),
            agent_id: Some(agent_id.clone()),
            run_id: text("run_id"),
            source: EventSource::Hook,
            event,
            occurred_at: now_ms(),
        };
        if state.bus.publish(ev).await.is_err() {
            return Response::error(
                req.id,
                "store_closed",
                "la base de datos no acepta escrituras",
            );
        }
    }
    Response::ok(req.id, json!({ "decision": "allow", "events": n }))
}

fn status(state: &State) -> Value {
    let recovery_open = state
        .reader
        .lock()
        .ok()
        .and_then(|conn| repo::open_recovery_count(&conn).ok());
    json!({
        "recovery_open": recovery_open,
        "pid": std::process::id(),
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": PROTOCOL_VERSION,
        "uptime_ms": u64::try_from(state.started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "home": state.home.display().to_string(),
    })
}
