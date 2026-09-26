//! Servidor IPC del daemon: una sola instancia por directorio de Symphony,
//! métodos `ping`, `status` y `shutdown`, timeout por request y apagado ordenado.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use symphony_protocol::transport::{self, ListenerExt as _, LocalStream};
use symphony_protocol::{Connection, Event, Message, PROTOCOL_VERSION, Request, Response};
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
    runtime: crate::runtime::Runtime,
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
    let runtime_reader = writer.reader()?;
    let listener = transport::listen(home).map_err(io("no se pudo abrir el socket IPC"))?;
    tracing::info!(pid = std::process::id(), home = %home.display(), "daemon listo");

    let bus = EventBus::new(
        writer.handle(),
        TUI_BUFFER,
        symphony_object_store::ObjectStore::new(
            symphony_core::SymphonyHome::at(home).objects_dir(),
        ),
    );
    let hook_cmd = std::env::current_exe().ok().map(|exe| {
        let sibling = exe
            .parent()
            .map(|d| d.join(format!("symphony{}", std::env::consts::EXE_SUFFIX)));
        let prog = sibling
            .filter(|p| p.is_file())
            .unwrap_or_else(|| PathBuf::from("symphony"));
        symphony_adapter_common::HookCommand {
            program: prog,
            args: vec!["hook".into(), "emit".into()],
        }
    });
    let runtime = crate::runtime::Runtime::new(
        home,
        writer.handle(),
        runtime_reader,
        bus.clone(),
        crate::providers::builtin_arc(),
        hook_cmd,
    );

    let state = Arc::new(State {
        started: Instant::now(),
        home: home.to_path_buf(),
        shutdown: shutdown.clone(),
        reader: Mutex::new(reader),
        bus,
        writer: writer.handle(),
        runtime,
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
    state.bus.checkpoints_idle().await;
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
            Ok(Some(Message::Subscribe(sub))) => {
                if !sub
                    .topics
                    .iter()
                    .any(|t| t == TOPIC_AGENT_EVENT || t == "*")
                {
                    Response::error(
                        sub.id,
                        "unknown_topic",
                        format!("tópicos disponibles: `{TOPIC_AGENT_EVENT}`"),
                    )
                } else {
                    let ok = Response::ok(sub.id, json!({ "topics": [TOPIC_AGENT_EVENT] }));
                    if conn.send(&Message::Response(ok)).await.is_ok() {
                        stream_events(conn, &state).await;
                    }
                    return;
                }
            }
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

/// Tópico de los eventos canónicos de agentes (IDEA §5.3).
pub const TOPIC_AGENT_EVENT: &str = "agent.event";
/// El suscriptor se atrasó y perdió eventos: tiene que releer el estado.
pub const TOPIC_LAGGED: &str = "bus.lagged";

/// Conexión suscrita: reenvía el bus hasta que el cliente cierre o el daemon se apague.
/// Lo que llega ya está persistido (el bus guarda antes de difundir).
async fn stream_events(mut conn: Connection<LocalStream>, state: &State) {
    use tokio::sync::broadcast::error::RecvError;
    let mut rx = state.bus.subscribe();
    loop {
        let event = tokio::select! {
            () = state.shutdown.cancelled() => return,
            // Una suscripción no manda nada más: cualquier mensaje o EOF la cierra.
            _ = conn.recv() => return,
            ev = rx.recv() => ev,
        };
        let msg = match event {
            Ok(ev) => Event::new(
                TOPIC_AGENT_EVENT,
                json!({
                    "project_id": ev.project_id,
                    "agent_id": ev.agent_id,
                    "run_id": ev.run_id,
                    "type": ev.event.type_name(),
                    "source": ev.source.as_str(),
                    "occurred_at": ev.occurred_at,
                }),
            ),
            Err(RecvError::Lagged(missed)) => Event::new(TOPIC_LAGGED, json!({ "missed": missed })),
            Err(RecvError::Closed) => return,
        };
        if conn.send(&Message::Event(msg)).await.is_err() {
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
        "agent.create" | "agent.spawn" => agent_create(req, state).await,
        "agent.list" | "agents.list" => agent_list(req, state),
        "agent.inspect" => agent_inspect(req, state).await,
        "agent.send" => agent_send(req, state).await,
        "agent.pause" => agent_pause(req, state).await,
        "agent.resume" => agent_resume(req, state).await,
        "agent.stop" => agent_stop(req, state).await,
        "agent.kill" => agent_kill(req, state).await,
        "agent.switch" => agent_switch(req, state).await,
        "agent.diff" => agent_diff(req, state).await,
        "agent.logs" => agent_logs(req, state).await,
        "agent.attach" => agent_attach(req, state).await,
        "agent.activity" => agent_view(req, state, |c, a| {
            crate::views::activity(c, a, 200).map(|v| json!({ "items": v }))
        }),
        "agent.history" => agent_view(req, state, crate::views::history),
        "models.list" => read_view(req, state, |c| {
            crate::views::models(c).map(|v| json!({ "models": v }))
        }),
        "provider.set_enabled" => provider_set_enabled(req, state).await,
        "project.status" => project_status(req, state).await,
        "project.init" => project_init(req).await,
        "recovery.list" => recovery_list(req, state),
        "recovery.act" => recovery_act(req, state).await,
        other => Response::error(
            req.id,
            "unknown_method",
            format!("método desconocido: `{other}`"),
        ),
    }
}

async fn agent_create(req: Request, state: &State) -> Response {
    let p = &req.params;
    let title = match p.get("title").and_then(Value::as_str) {
        Some(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => return Response::error(req.id, "invalid_params", "falta `title` con la tarea"),
    };
    let project_root = match p.get("project_root").and_then(Value::as_str) {
        Some(r) => PathBuf::from(r),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let description = p
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_string);
    let execution = if let Some(model) = p.get("model").and_then(Value::as_str) {
        crate::runtime::Execution::Exact(model.to_string())
    } else if let Some(exact) = p.get("exact").and_then(Value::as_str) {
        crate::runtime::Execution::Exact(exact.to_string())
    } else if let Some(profile) = p.get("profile").and_then(Value::as_str) {
        crate::runtime::Execution::Profile(profile.to_string())
    } else {
        crate::runtime::Execution::DecideLater
    };
    let failover = match enum_param(p, "failover", symphony_core::FailoverPolicy::Any) {
        Ok(v) => v,
        Err(e) => return Response::error(req.id, "invalid_params", e),
    };
    let context_mode = match enum_param(p, "context_mode", symphony_core::ContextMode::Balanced) {
        Ok(v) => v,
        Err(e) => return Response::error(req.id, "invalid_params", e),
    };
    let priority = p.get("priority").and_then(Value::as_i64).unwrap_or(0);

    let create_req = crate::runtime::CreateAgent {
        project_root,
        title,
        description,
        execution,
        failover,
        context_mode,
        priority,
    };

    // En su propia tarea: si el request vence (REQUEST_TIMEOUT), la creación
    // termina igual (o hace su rollback) en vez de cortarse con el worktree a medias.
    let runtime = state.runtime.clone();
    let created = tokio::spawn(async move { runtime.create_agent(create_req).await }).await;
    let created = match created {
        Ok(result) => result,
        Err(e) => return Response::error(req.id, "internal", e.to_string()),
    };
    match created {
        Ok(c) => Response::ok(
            req.id,
            json!({
                "agent_id": c.agent_id.to_string(),
                "task_id": c.task_id.to_string(),
                "number": c.number,
                "task_code": c.task_code,
                "worktree": c.worktree.display().to_string(),
                "branch": c.branch,
                "state": c.state.as_str(),
                "state_reason": c.state_reason,
                "run": c.run.map(|(r, m)| json!({ "run_id": r.to_string(), "model": m })),
            }),
        ),
        Err(e) => Response::error(req.id, e.code(), e.to_string()),
    }
}

fn agent_list(req: Request, state: &State) -> Response {
    let p = &req.params;
    let all = p.get("all").and_then(Value::as_bool).unwrap_or(false);
    let project_id = p
        .get("project_id")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<symphony_core::ProjectId>().ok())
        .or_else(|| {
            p.get("project_root").and_then(Value::as_str).and_then(|r| {
                state.reader.lock().ok().and_then(|conn| {
                    repo::project_by_root(&conn, r)
                        .ok()
                        .flatten()
                        .map(|pr| pr.id)
                })
            })
        });
    let rows = match state.reader.lock() {
        Ok(conn) => repo::list_agents_rows(&conn, project_id, all),
        Err(_) => {
            return Response::error(req.id, "store_error", "lector de la base no disponible");
        }
    };
    match rows {
        Ok(agents) => {
            let list: Vec<Value> = agents
                .into_iter()
                .map(|a| {
                    json!({
                        "agent_id": a.agent_id.to_string(),
                        "number": a.number,
                        "state": a.state.as_str(),
                        "state_reason": a.state_reason,
                        "task_code": a.task_code,
                        "task_title": a.task_title,
                        "provider_id": a.provider_id,
                        "model_id": a.model_id,
                    })
                })
                .collect();
            Response::ok(req.id, json!({ "agents": list }))
        }
        Err(e) => Response::error(req.id, "store_error", e.to_string()),
    }
}

fn resolve_agent_id(state: &State, p: &Value) -> Result<symphony_core::AgentId, String> {
    let ident = p
        .get("agent")
        .or_else(|| p.get("agent_id"))
        .or_else(|| p.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| "falta parámetro `agent` o `agent_id`".to_string())?;
    let project_id = p
        .get("project_id")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<symphony_core::ProjectId>().ok())
        .or_else(|| {
            p.get("project_root").and_then(Value::as_str).and_then(|r| {
                state.reader.lock().ok().and_then(|conn| {
                    repo::project_by_root(&conn, r)
                        .ok()
                        .flatten()
                        .map(|pr| pr.id)
                })
            })
        });
    state
        .runtime
        .find_agent(ident, project_id)
        .map(|a| a.id)
        .map_err(|e| e.0)
}

async fn agent_inspect(req: Request, state: &State) -> Response {
    let agent_id = match resolve_agent_id(state, &req.params) {
        Ok(id) => id,
        Err(e) => return Response::error(req.id, "agent_not_found", e),
    };
    match state.runtime.inspect(agent_id).await {
        Ok(v) => Response::ok(req.id, v),
        Err(e) => Response::error(req.id, "agent_error", e.0),
    }
}

async fn agent_send(req: Request, state: &State) -> Response {
    let agent_id = match resolve_agent_id(state, &req.params) {
        Ok(id) => id,
        Err(e) => return Response::error(req.id, "agent_not_found", e),
    };
    let text = match req
        .params
        .get("text")
        .or_else(|| req.params.get("message"))
        .and_then(Value::as_str)
    {
        Some(t) => t,
        None => return Response::error(req.id, "invalid_params", "falta `text` o `message`"),
    };
    match state.runtime.send_message(agent_id, text).await {
        Ok(()) => Response::ok(req.id, json!({ "ok": true })),
        Err(e) => Response::error(req.id, "agent_error", e.0),
    }
}

async fn agent_pause(req: Request, state: &State) -> Response {
    let agent_id = match resolve_agent_id(state, &req.params) {
        Ok(id) => id,
        Err(e) => return Response::error(req.id, "agent_not_found", e),
    };
    match state.runtime.pause(agent_id).await {
        Ok(()) => Response::ok(req.id, json!({ "ok": true, "state": "PAUSED" })),
        Err(e) => Response::error(req.id, "agent_error", e.0),
    }
}

async fn agent_resume(req: Request, state: &State) -> Response {
    let agent_id = match resolve_agent_id(state, &req.params) {
        Ok(id) => id,
        Err(e) => return Response::error(req.id, "agent_not_found", e),
    };
    match state.runtime.resume(agent_id).await {
        Ok(()) => Response::ok(req.id, json!({ "ok": true, "state": "RUNNING" })),
        Err(e) => Response::error(req.id, "agent_error", e.0),
    }
}

async fn agent_stop(req: Request, state: &State) -> Response {
    let agent_id = match resolve_agent_id(state, &req.params) {
        Ok(id) => id,
        Err(e) => return Response::error(req.id, "agent_not_found", e),
    };
    match state.runtime.stop(agent_id).await {
        Ok(()) => Response::ok(req.id, json!({ "ok": true, "state": "CANCELLED" })),
        Err(e) => Response::error(req.id, "agent_error", e.0),
    }
}

async fn agent_kill(req: Request, state: &State) -> Response {
    let agent_id = match resolve_agent_id(state, &req.params) {
        Ok(id) => id,
        Err(e) => return Response::error(req.id, "agent_not_found", e),
    };
    match state.runtime.kill(agent_id).await {
        Ok(()) => Response::ok(req.id, json!({ "ok": true, "state": "CANCELLED" })),
        Err(e) => Response::error(req.id, "agent_error", e.0),
    }
}

async fn agent_switch(req: Request, state: &State) -> Response {
    let agent_id = match resolve_agent_id(state, &req.params) {
        Ok(id) => id,
        Err(e) => return Response::error(req.id, "agent_not_found", e),
    };
    let model = match req.params.get("model").and_then(Value::as_str) {
        Some(m) => m,
        None => return Response::error(req.id, "invalid_params", "falta `model`"),
    };
    match state.runtime.switch(agent_id, model).await {
        Ok(run_id) => Response::ok(
            req.id,
            json!({ "ok": true, "run_id": run_id.to_string(), "model": model }),
        ),
        Err(e) => Response::error(req.id, "agent_error", e.0),
    }
}

async fn agent_diff(req: Request, state: &State) -> Response {
    let agent_id = match resolve_agent_id(state, &req.params) {
        Ok(id) => id,
        Err(e) => return Response::error(req.id, "agent_not_found", e),
    };
    match state.runtime.diff(agent_id).await {
        Ok(diff) => Response::ok(req.id, json!({ "diff": diff })),
        Err(e) => Response::error(req.id, "agent_error", e.0),
    }
}

async fn agent_logs(req: Request, state: &State) -> Response {
    let agent_id = match resolve_agent_id(state, &req.params) {
        Ok(id) => id,
        Err(e) => return Response::error(req.id, "agent_not_found", e),
    };
    let limit = req
        .params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n as usize);
    match state.runtime.logs(agent_id, limit).await {
        Ok(logs) => {
            let messages: Vec<Value> = logs
                .into_iter()
                .map(|(role, content)| json!({ "role": role, "content": content }))
                .collect();
            Response::ok(req.id, json!({ "messages": messages }))
        }
        Err(e) => Response::error(req.id, "agent_error", e.0),
    }
}

/// «Abrir en el CLI» (ADR-0005). `open: false` solo devuelve el comando.
async fn agent_attach(req: Request, state: &State) -> Response {
    let agent_id = match resolve_agent_id(state, &req.params) {
        Ok(id) => id,
        Err(e) => return Response::error(req.id, "agent_not_found", e),
    };
    let open = req
        .params
        .get("open")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let (spec, cli) = match state.runtime.attach_spec(agent_id) {
        Ok(v) => v,
        Err(e) => return Response::error(req.id, "attach_unavailable", e.0),
    };
    let command = crate::attach::command_line(&spec);
    let opened = if open {
        crate::attach::open_terminal(&spec, &format!("Symphony · {cli}"))
    } else {
        Err("no se pidió abrir una terminal".into())
    };
    if opened.is_ok()
        && let Err(e) = state.runtime.note_attached(agent_id, cli).await
    {
        tracing::warn!(error = %e.0, "no se pudo anotar el attach en la conversación");
    }
    Response::ok(
        req.id,
        json!({
            "cli": cli,
            "command": command,
            "opened": opened.is_ok(),
            "error": opened.err(),
        }),
    )
}

/// Enum de DB en un parámetro: ausente → `default`; inválido → error (nunca el default en silencio).
fn enum_param<T>(p: &Value, key: &str, default: T) -> Result<T, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match p.get(key).filter(|v| !v.is_null()) {
        None => Ok(default),
        Some(v) => v
            .as_str()
            .ok_or_else(|| format!("`{key}` tiene que ser texto"))?
            .parse()
            .map_err(|e: T::Err| format!("`{key}`: {e}")),
    }
}

fn read_view(
    req: Request,
    state: &State,
    f: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<Value>,
) -> Response {
    let res = match state.reader.lock() {
        Ok(conn) => f(&conn),
        Err(_) => {
            return Response::error(req.id, "store_error", "lector de la base no disponible");
        }
    };
    match res {
        Ok(v) => Response::ok(req.id, v),
        Err(e) => Response::error(req.id, "store_error", e.to_string()),
    }
}

fn agent_view(
    req: Request,
    state: &State,
    f: impl FnOnce(&rusqlite::Connection, &str) -> rusqlite::Result<Value>,
) -> Response {
    match resolve_agent_id(state, &req.params) {
        Ok(id) => {
            let id = id.to_string();
            read_view(req, state, |c| f(c, &id))
        }
        Err(e) => Response::error(req.id, "agent_not_found", e),
    }
}

async fn provider_set_enabled(req: Request, state: &State) -> Response {
    let p = &req.params;
    let (Some(id), Some(enabled)) = (
        p.get("id").and_then(Value::as_str).map(str::to_string),
        p.get("enabled").and_then(Value::as_bool),
    ) else {
        return Response::error(req.id, "invalid_params", "faltan `id` y `enabled`");
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    let target = id.clone();
    let written = state
        .writer
        .write(Box::new(move |t| {
            let n = t.execute(
                "UPDATE providers SET enabled = ?2 WHERE id = ?1",
                rusqlite::params![target, i64::from(enabled)],
            )?;
            let _ = tx.send(n);
            Ok(())
        }))
        .await;
    match (written, rx.await) {
        (Ok(()), Ok(1)) => Response::ok(req.id, json!({ "id": id, "enabled": enabled })),
        (Ok(()), _) => Response::error(
            req.id,
            "provider_not_found",
            format!("no existe el proveedor `{id}`"),
        ),
        (Err(e), _) => Response::error(req.id, "store_error", e.to_string()),
    }
}

fn project_root_param(p: &Value) -> PathBuf {
    match p.get("project_root").and_then(Value::as_str) {
        Some(r) => PathBuf::from(r),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

/// Launch (FLOW §4.1): ¿es un repo?, ¿está inicializado?, ¿hay algo que recuperar?
async fn project_status(req: Request, state: &State) -> Response {
    let root = project_root_param(&req.params);
    let probe = root.clone();
    let found = tokio::task::spawn_blocking(move || {
        let repo = symphony_git::Repo::discover(&probe)?;
        let branch = repo.current_branch()?;
        Ok::<_, symphony_git::GitError>((repo.root().to_path_buf(), branch))
    })
    .await;
    let Ok(Ok((repo_root, branch))) = found else {
        return Response::ok(
            req.id,
            json!({ "path": root.display().to_string(), "is_repo": false }),
        );
    };
    let root_text = repo_root.display().to_string();
    let config = match symphony_core::load_project(&repo_root) {
        Ok(c) => c,
        Err(e) => return Response::error(req.id, "config_error", e.to_string()),
    };
    let dir_name = repo_root
        .file_name()
        .map_or_else(|| root_text.clone(), |n| n.to_string_lossy().into_owned());
    let known = state.reader.lock().ok().and_then(|conn| {
        let project = repo::project_by_root(&conn, &root_text).ok().flatten()?;
        let open: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM recovery_items WHERE project_id = ?1 AND status = 'OPEN'",
                [project.id.to_string()],
                |r| r.get(0),
            )
            .unwrap_or(0);
        Some((project.id.to_string(), open))
    });
    Response::ok(
        req.id,
        json!({
            "path": root.display().to_string(),
            "is_repo": true,
            "root": root_text,
            "name": config.as_ref().map_or(dir_name, |c| c.project.name.clone()),
            "branch": branch,
            "initialized": config.is_some(),
            "project_id": known.as_ref().map(|k| k.0.clone()),
            "recovery_open": known.map_or(0, |k| k.1),
        }),
    )
}

/// First-run (FLOW §4.2): guarda nombre, perfil de rendimiento y failover en `project.toml`.
async fn project_init(req: Request) -> Response {
    let p = &req.params;
    let root = project_root_param(p);
    let performance = match enum_param(
        p,
        "performance",
        symphony_core::PerformanceProfile::Balanced,
    ) {
        Ok(v) => v,
        Err(e) => return Response::error(req.id, "invalid_params", e),
    };
    let failover = match enum_param(p, "failover", symphony_core::FailoverPolicy::Any) {
        Ok(v) => v,
        Err(e) => return Response::error(req.id, "invalid_params", e),
    };
    let name = p.get("name").and_then(Value::as_str).map(str::to_string);
    let done = tokio::task::spawn_blocking(move || {
        let repo = symphony_git::Repo::discover(&root).map_err(|e| e.to_string())?;
        let branch = repo
            .current_branch()
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| "main".into());
        let dir_name = repo
            .root()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "proyecto".into());
        let name = name.filter(|n| !n.trim().is_empty()).unwrap_or(dir_name);
        symphony_core::init_project(
            repo.root(),
            &symphony_core::ProjectInit {
                name: name.trim(),
                default_branch: &branch,
                performance,
                failover,
            },
        )
        .map(|c| (repo.root().display().to_string(), c.project.name))
        .map_err(|e| e.to_string())
    })
    .await;
    match done {
        Ok(Ok((root, name))) => Response::ok(req.id, json!({ "root": root, "name": name })),
        Ok(Err(e)) => Response::error(req.id, "init_failed", e),
        Err(e) => Response::error(req.id, "init_failed", e.to_string()),
    }
}

fn project_id_param(state: &State, p: &Value) -> Option<String> {
    let root = p.get("project_root").and_then(Value::as_str)?;
    let conn = state.reader.lock().ok()?;
    repo::project_by_root(&conn, root)
        .ok()
        .flatten()
        .map(|pr| pr.id.to_string())
}

fn recovery_list(req: Request, state: &State) -> Response {
    let project = project_id_param(state, &req.params);
    if req.params.get("project_root").is_some() && project.is_none() {
        // Proyecto que Symphony todavía no conoce: nada que recuperar.
        return Response::ok(req.id, json!({ "items": [] }));
    }
    read_view(req, state, |c| {
        crate::views::recovery(c, project.as_deref()).map(|v| json!({ "items": v }))
    })
}

/// Acciones del Recovery Center (FLOW §16): `restart`, `reclaim`, `stop` o `dismiss`.
async fn recovery_act(req: Request, state: &State) -> Response {
    let p = &req.params;
    let (Some(id), Some(action)) = (
        p.get("id").and_then(Value::as_str).map(str::to_string),
        p.get("action").and_then(Value::as_str).map(str::to_string),
    ) else {
        return Response::error(req.id, "invalid_params", "faltan `id` y `action`");
    };
    let item = state
        .reader
        .lock()
        .ok()
        .and_then(|c| crate::views::recovery_item(&c, &id).ok().flatten());
    let Some((agent, _kind)) = item else {
        return Response::error(
            req.id,
            "recovery_not_found",
            "ese problema ya no está abierto",
        );
    };
    let agent = agent.and_then(|a| a.parse::<symphony_core::AgentId>().ok());
    let close = |resolution: Option<&'static str>| {
        let id = id.clone();
        state.writer.write(Box::new(move |t| {
            repo::close_recovery_item(t, &id, resolution, now_ms())?;
            Ok(())
        }))
    };
    let result = match (action.as_str(), agent) {
        ("dismiss", _) => close(None).await.map_err(|e| e.to_string()),
        ("restart", Some(a)) => state.runtime.restart(a).await.map(drop).map_err(|e| e.0),
        ("reclaim", Some(a)) => state.runtime.reclaim(a).await.map(drop).map_err(|e| e.0),
        ("stop", Some(a)) => match state.runtime.stop(a).await {
            Ok(()) => close(Some("ARCHIVE")).await.map_err(|e| e.to_string()),
            Err(e) => Err(e.0),
        },
        ("restart" | "reclaim" | "stop", None) => Err(format!(
            "`{action}` necesita un agente; este problema no tiene uno"
        )),
        _ => {
            return Response::error(
                req.id,
                "invalid_params",
                format!("acción desconocida: `{action}` (restart, reclaim, stop, dismiss)"),
            );
        }
    };
    match result {
        Ok(()) => Response::ok(req.id, json!({ "ok": true })),
        Err(e) => Response::error(req.id, "recovery_failed", e),
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
