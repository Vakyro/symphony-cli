//! Executors vivos (P06.S1, S5): lanzar el CLI, leer su salida, cerrarlo y
//! reemplazarlo. AGENT ≠ MODEL: el agente, su task, su worktree y su rama no
//! cambian; cambia el run abierto.
//!
//! Cada executor vivo tiene un canal de control (parar, escribir en stdin,
//! suspender, reanudar). La tarea que lee su salida es la única dueña del proceso.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use symphony_adapter_common::{
    AgentEvent, ProviderAdapter, ProviderError, ResumeRequest, SpawnRequest,
};
use symphony_core::{
    AgentId, AgentState, ExecutorChangeId, FailoverPolicy, FailureType, HandoffId, MessageId,
    ProjectId, ProviderFailureId, RunEndReason, RunId, RunStatus, TaskId, TaskStatus,
};
use symphony_git::Repo;
use symphony_process::{ExitStatus, OutputLine, Supervised};
use symphony_store::repo;
use tokio::sync::{mpsc, oneshot};

use crate::bus::{BusEvent, EventSource};
use crate::runtime::{Runtime, now_ms};

/// Lo que hace falta para lanzar un executor.
/// Cada cuánto se toma la foto del árbol de procesos de un run.
const PROCESS_SAMPLE_EVERY: Duration = Duration::from_secs(5);
/// CPU del árbol entre dos muestras que cuenta como latido (4 % de un núcleo en 5 s).
const CPU_ALIVE_MS: u64 = 200;
/// La CPU solo cuenta como latido si el CLI escribió algo en este tiempo: un proceso colgado en
/// un bucle activo no vive para siempre. Dogfooding 2026-10-27: un agente de Antigravity siguió
/// «vivo» casi 4 h sin escribir nada tras llenarse el disco.
/// ponytail: tope fijo; Antigravity no escribe hasta terminar, así que una corrida suya de más
/// de una hora cae aquí. Configurable si alguien lo necesita.
const CPU_BEAT_MAX_SILENCE: Duration = Duration::from_secs(60 * 60);

/// Límites del SO según `[performance]` del `config.toml` (se relee en cada lanzamiento).
pub(crate) fn os_limits(home: &Path) -> symphony_process::Limits {
    let limits = symphony_core::load_or_create(&symphony_core::SymphonyHome::user_for(home))
        .map(|c| symphony_scheduler::OsLimits::from_config(&c.performance))
        .unwrap_or_else(|e| {
            // Auditoría P13.S5, hallazgo 13: un `config.toml` roto no puede quitar los límites
            // en silencio.
            tracing::warn!(error = %e, "config.toml ilegible: el agente arranca sin límites del SO");
            Default::default()
        });
    symphony_process::Limits {
        max_memory_bytes: limits.max_memory_bytes,
        cpu_cores: limits.cpu_cores,
        max_processes: limits.max_processes,
    }
}

/// El pump ya no puede publicar (la base dejó de aceptar escrituras): mata el árbol y drena la
/// salida pendiente. Sin esto el reenviador de `symphony_process` se bloquea con la cola llena y
/// `wait` espera para siempre a un CLI que sigue vivo (auditoría P13.S5, hallazgo 5).
async fn abort_unpublishable(proc: &mut Supervised) {
    let _ = proc.terminate_tree();
    while proc.next_output().await.is_some() {}
}

/// De dónde sale el agente y qué pide el reemplazo (`Runtime::pick_replacement`).
struct Replace<'a> {
    provider: &'a str,
    model: &'a str,
    /// El login fue rechazado (se aparta el proveedor entero, no solo el modelo).
    auth: bool,
    /// Rotación a propósito (umbral de contexto del chat): se aparta el proveedor actual.
    rotate: bool,
    /// Tamaño del handoff en tokens, para descartar modelos cuya ventana no lo cabe.
    needed_context: Option<u64>,
}

/// Foto del repo principal antes de un run; el worktree del agente no se incluye.
struct MainWorkspace {
    root: PathBuf,
    before: WorkspaceSnapshot,
}

#[derive(Debug, PartialEq, Eq)]
struct WorkspaceSnapshot {
    head: String,
    status: String,
}

impl WorkspaceSnapshot {
    fn capture(root: &Path) -> Result<Self, String> {
        let repo = Repo::at(root);
        let head = repo.head_commit().map_err(|e| e.to_string())?;
        let status = repo.status_porcelain().map_err(|e| e.to_string())?;
        Ok(Self { head, status })
    }

    fn shown_status(&self) -> &str {
        if self.status.is_empty() {
            "(limpio)"
        } else {
            &self.status
        }
    }
}

pub(crate) struct Launch {
    pub adapter: Arc<dyn ProviderAdapter>,
    pub project_id: ProjectId,
    pub agent_id: AgentId,
    pub task_id: TaskId,
    pub run_id: RunId,
    pub provider_id: String,
    pub model_id: String,
    pub worktree: PathBuf,
    pub cli_model: String,
    pub prompt: String,
    /// Sesión del CLI a continuar (`resume_spec`) en vez de abrir una nueva.
    pub resume_session: Option<String>,
}

/// Órdenes para el executor vivo de un agente.
pub(crate) enum Control {
    /// Mata el árbol de procesos y cierra el run así. El estado del agente lo decide quien pide.
    Stop {
        status: RunStatus,
        end: RunEndReason,
        done: oneshot::Sender<()>,
    },
    /// El watchdog (P06.S6) perdió el latido: mata el árbol, cierra el run
    /// `FAILED/NO_HEARTBEAT` y deja al agente `FAILED` con recovery item.
    NoHeartbeat {
        done: oneshot::Sender<()>,
    },
    Send {
        bytes: Vec<u8>,
        done: oneshot::Sender<Result<(), String>>,
    },
    Suspend(oneshot::Sender<Result<(), String>>),
    Resume(oneshot::Sender<Result<(), String>>),
}

/// Quién pidió la parada, para el cierre del run en el pump.
#[derive(Debug, Clone, Copy, PartialEq)]
enum StopKind {
    User,
    NoHeartbeat,
}

pub struct LiveRun {
    pub(crate) run_id: RunId,
    pub(crate) adapter: Arc<dyn ProviderAdapter>,
    pub(crate) project_id: ProjectId,
    pub(crate) control: mpsc::UnboundedSender<Control>,
}

/// Fallos que justifican cambiar de executor (P06.S5): cuota agotada o login.
/// Cuánto antes de la muerte de un run cuenta un límite o un login rechazado como su causa.
/// En Signalia (2026-10-03) pasaron ~15 min entre el rate limit y el corte por falta de latido.
const DEATH_CAUSE_WINDOW_MS: i64 = 30 * 60 * 1000;

/// Un run que muere sin explicación poco después de un límite registrado murió por eso: el motivo
/// de cierre, el tipo de item del Recovery Center y la frase (F5 de `docs/research/uso-signalia.md`).
fn death_cause(f: FailureType) -> Option<(RunEndReason, &'static str, &'static str)> {
    match f {
        FailureType::TempRateLimit | FailureType::Rpm | FailureType::Tpm => Some((
            RunEndReason::RateLimited,
            "RATE_LIMITED",
            "el proveedor limitó las solicitudes (rate limit) y el executor dejó de responder",
        )),
        FailureType::DailyQuota
        | FailureType::WeeklyQuota
        | FailureType::ModelLimit
        | FailureType::AccountLimit => Some((
            RunEndReason::QuotaExhausted,
            "RATE_LIMITED",
            "el proveedor se quedó sin cuota y el executor dejó de responder",
        )),
        FailureType::Auth => Some((
            RunEndReason::AuthError,
            "AUTH_ERROR",
            "el proveedor rechazó el login y el executor dejó de responder",
        )),
        _ => None,
    }
}

pub(crate) fn needs_failover(e: &ProviderError) -> bool {
    !e.transient
        && matches!(
            e.failure_type,
            FailureType::DailyQuota
                | FailureType::WeeklyQuota
                | FailureType::ModelLimit
                | FailureType::AccountLimit
                | FailureType::Auth
        )
}

/// Cuántas líneas de stderr se guardan con un run caído.
const ERR_TAIL_LINES: usize = 3;

/// Error de las operaciones sobre un agente existente, con frase legible.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct AgentOpError(pub String);

fn op_err(e: impl std::fmt::Display) -> AgentOpError {
    AgentOpError(e.to_string())
}

/// Prefijo con el que cada CLI reconoce una skill al inicio del mensaje: `/nombre` en Claude,
/// `$nombre` en Codex (P07.5.S8).
fn skill_prefix(provider: &str) -> Option<char> {
    match provider {
        "anthropic" => Some('/'),
        "openai" => Some('$'),
        _ => None,
    }
}

/// Añade el mensaje nuevo del usuario al prompt del handoff. Claude solo interpreta `/skill` si
/// es lo primero del mensaje (probado: dentro del prompt de handoff no se aplica), así que si el
/// mensaje empieza por el prefijo de skill del proveedor destino, esa invocación se sube al
/// inicio y el resto del handoff pasa como sus argumentos.
// ponytail: los argumentos de la skill incluyen todo el handoff; suficiente para skills de estilo
// o de tarea. `/skill` escrito para el otro proveedor no se traduce.
fn with_user_message(prompt: &str, message: &str, provider: &str) -> String {
    let body = format!(
        "{prompt}

## Mensaje nuevo del usuario (respóndelo primero)
{message}"
    );
    let trimmed = message.trim_start();
    match skill_prefix(provider) {
        Some(p) if trimmed.starts_with(p) && trimmed.len() > 1 => {
            let (invocation, rest) = trimmed
                .split_once(char::is_whitespace)
                .unwrap_or((trimmed, ""));
            format!(
                "{invocation} {}

{body}",
                rest.trim()
            )
        }
        _ => body,
    }
}

impl Runtime {
    fn read_op<T>(
        &self,
        f: impl FnOnce(&rusqlite::Connection) -> Result<T, repo::RepoError>,
    ) -> Result<T, AgentOpError> {
        let conn = self
            .reader
            .lock()
            .map_err(|_| AgentOpError("lector de la base no disponible".into()))?;
        f(&conn).map_err(op_err)
    }

    /// Lanza el CLI. Si no arranca: run `FAILED`, agente `FAILED` con razón y recovery item.
    ///
    /// Tipo de retorno explícito: el executor puede lanzar a su sucesor (failover) y eso
    /// haría recursivo al tipo del future.
    pub(crate) fn launch(
        &self,
        l: Launch,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        Box::pin(self.launch_inner(l))
    }

    /// Skills y servidores MCP compartidos para este run (P09.S9): deja al día su índice y
    /// devuelve las variables que cada adapter traduce a su CLI. Nunca falla el lanzamiento:
    /// sin proyecto legible o con un archivo roto, el agente arranca sin lo compartido.
    async fn shared_env(&self, l: &Launch) -> Vec<(String, String)> {
        use crate::shared;
        use symphony_store::shared as index;
        let Ok(project) = self.read_op(|c| repo::get_project(c, l.project_id)) else {
            return Vec::new();
        };
        let root = PathBuf::from(&project.root_path);
        let skills = shared::scan_skills(
            symphony_core::SymphonyHome::user_for(&self.home).root(),
            Some(&root),
        );
        let mut warnings = Vec::new();
        let servers = shared::scan_mcp(
            symphony_core::SymphonyHome::user_for(&self.home).root(),
            Some(&root),
            &mut warnings,
        );
        for w in &warnings {
            tracing::warn!("{w}");
        }
        let pid = l.project_id.to_string();
        let found_skills: Vec<_> = skills.iter().map(shared::SkillDef::found).collect();
        let found_servers: Vec<_> = servers.iter().map(shared::McpDef::found).collect();
        let (p, now) = (pid.clone(), now_ms());
        // Con un `mcp.toml` roto no se borra nada: el interruptor del usuario no se pierde.
        let prune = warnings.is_empty();
        let _ = self
            .writer
            .write(Box::new(move |t| {
                index::sync_skills(t, &p, &found_skills, now, prune)?;
                index::sync_mcp(t, &p, &found_servers, prune)?;
                Ok(())
            }))
            .await;
        let skill_rows = self
            .read_op(|c| index::list_skills(c, &pid))
            .unwrap_or_default();
        let mcp_rows = self
            .read_op(|c| index::list_mcp(c, &pid))
            .unwrap_or_default();

        let dir = self.home.join("runtime").join(&pid).join("claude-plugin");
        shared::launch_env(
            &l.provider_id,
            &dir,
            &skills,
            &servers,
            &|s| {
                skill_rows
                    .iter()
                    .find(|r| r.name == s.name && r.scope == s.scope)
                    .is_none_or(|r| r.enabled)
            },
            &|d| {
                mcp_rows
                    .iter()
                    .find(|r| r.name == d.name && r.scope == d.scope)
                    .map_or(d.enabled_default, |r| r.enabled)
            },
        )
    }

    /// Antes de cada run se fotografía el repo principal, no el worktree compartido del agente.
    /// Si Git o el proyecto no se pueden leer, el run sigue y el fallo queda en el log.
    async fn watch_main_workspace(&self, project: ProjectId) -> Option<MainWorkspace> {
        let root = match self.read_op(|c| repo::get_project(c, project)) {
            Ok(p) => PathBuf::from(p.root_path),
            Err(e) => {
                tracing::warn!(error = %e, "no se pudo localizar el repo principal para vigilarlo");
                return None;
            }
        };
        let capture_root = root.clone();
        let before =
            tokio::task::spawn_blocking(move || WorkspaceSnapshot::capture(&capture_root)).await;
        match before {
            Ok(Ok(before)) => Some(MainWorkspace { root, before }),
            Ok(Err(e)) => {
                tracing::warn!(root = %root.display(), error = %e, "no se pudo fotografiar el repo principal");
                None
            }
            Err(e) => {
                tracing::warn!(root = %root.display(), error = %e, "no se pudo fotografiar el repo principal");
                None
            }
        }
    }

    /// Un cambio de HEAD o de `git status` mientras corría el agente es daño del workspace,
    /// aunque el CLI haya terminado bien: queda visible en el Recovery Center.
    async fn check_main_workspace(&self, l: &Launch, watched: Option<MainWorkspace>) {
        let Some(watched) = watched else {
            return;
        };
        let root = watched.root.clone();
        let capture_root = root.clone();
        let after =
            tokio::task::spawn_blocking(move || WorkspaceSnapshot::capture(&capture_root)).await;
        let after = match after {
            Ok(Ok(after)) => after,
            Ok(Err(e)) => {
                tracing::warn!(root = %root.display(), error = %e, "no se pudo revisar el repo principal al cerrar el run");
                return;
            }
            Err(e) => {
                tracing::warn!(root = %root.display(), error = %e, "no se pudo revisar el repo principal al cerrar el run");
                return;
            }
        };
        if watched.before == after {
            return;
        }
        let detail = format!(
            "el repo principal {} cambió mientras corría el agente.\nHEAD antes: {}\nHEAD después: {}\ngit status antes:\n{}\ngit status después:\n{}",
            root.display(),
            watched.before.head,
            after.head,
            watched.before.shown_status(),
            after.shown_status(),
        );
        let (project, agent, run) = (l.project_id, l.agent_id, l.run_id);
        let result = self
            .writer
            .write(Box::new(move |t| {
                repo::open_recovery_item(
                    t,
                    project,
                    Some(agent),
                    Some(run),
                    "WORKSPACE_DAMAGED",
                    &detail,
                    now_ms(),
                )?;
                Ok(())
            }))
            .await;
        if let Err(e) = result {
            tracing::error!(agent = %agent, error = %e, "no se pudo registrar el daño del repo principal");
        }
    }

    async fn launch_inner(&self, l: Launch) -> Result<(), String> {
        self.refresh_graph(&l.worktree, Some(l.project_id.to_string()));
        let prompt = format!("{}{}", l.prompt, self.graph_context(&l.worktree));
        let main_workspace = self.watch_main_workspace(l.project_id).await;
        let mut env = vec![
            ("SYMPHONY_AGENT_ID".into(), l.agent_id.to_string()),
            ("SYMPHONY_PROJECT_ID".into(), l.project_id.to_string()),
            ("SYMPHONY_RUN_ID".into(), l.run_id.to_string()),
            ("SYMPHONY_TASK_ID".into(), l.task_id.to_string()),
            ("SYMPHONY_PROVIDER".into(), l.provider_id.clone()),
            ("SYMPHONY_HOME".into(), self.home.display().to_string()),
            ("SYMPHONY_WORKTREE".into(), l.worktree.display().to_string()),
            (
                "SYMPHONY_USER_HOME".into(),
                symphony_core::SymphonyHome::user_for(&self.home)
                    .root()
                    .display()
                    .to_string(),
            ),
        ];
        // Harness de Symphony (ADR-0019): el modelo trabaja con las herramientas del broker.
        let harness =
            symphony_core::load_or_create(&symphony_core::SymphonyHome::user_for(&self.home))
                .map(|c| c.harness.mode)
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "config.toml ilegible: harness por defecto");
                    Default::default()
                });
        if harness == symphony_core::HarnessMode::Symphony {
            env.push((symphony_adapter_common::ENV_HARNESS.into(), "1".into()));
            let style =
                symphony_core::load_or_create(&symphony_core::SymphonyHome::user_for(&self.home))
                    .map(|c| c.harness.style)
                    .unwrap_or_default();
            if style == symphony_core::HarnessStyle::Normal {
                env.push((
                    symphony_adapter_common::ENV_HARNESS_STYLE.into(),
                    "normal".into(),
                ));
            }
        }
        env.extend(crate::gc::cargo_env(&self.home, &l.worktree));
        env.extend(self.shared_env(&l).await);
        let spawn_req = SpawnRequest {
            worktree: l.worktree.clone(),
            model: l.cli_model.clone(),
            prompt: prompt.clone(),
            session_id: None,
            hook: self.hook.clone(),
            env,
        };
        // El prompt inicial (objetivo o handoff) es el primer mensaje del run (P06.S2).
        let prompt_ev = BusEvent {
            project_id: l.project_id.to_string(),
            agent_id: Some(l.agent_id.to_string()),
            run_id: Some(l.run_id.to_string()),
            source: EventSource::User,
            // Las reglas del plan por fases son de Symphony: el CLI las recibe, la conversación
            // guarda lo que pidió el usuario (y es lo que viaja en un handoff).
            event: AgentEvent::UserMessage {
                text: crate::plan::without_instructions(&l.prompt).to_string(),
            },
            occurred_at: now_ms(),
        };
        if self.bus.publish(prompt_ev).await.is_err() {
            return Err("la base de datos no acepta escrituras".into());
        }
        let started = async {
            let mut spec = match &l.resume_session {
                Some(id) => l.adapter.resume_spec(&ResumeRequest {
                    spawn: spawn_req.clone(),
                    cli_session_id: id.clone(),
                }),
                None => l.adapter.spawn_spec(&spawn_req),
            }
            .map_err(|e| e.to_string())?;
            // Capa 2 del scheduler (ADR-0002/0003): el SO hace cumplir el límite del árbol.
            spec.limits = os_limits(&self.home);
            let mut proc = symphony_process::spawn(spec)
                .await
                .map_err(|e| e.to_string())?;
            let g = proc.guarantees();
            if g.limits_enforced {
                tracing::debug!(run = %l.run_id, mechanism = g.mechanism, "grupo de procesos creado");
            } else {
                tracing::warn!(
                    run = %l.run_id,
                    mechanism = g.mechanism,
                    note = g.limits_note.as_deref().unwrap_or(""),
                    "el SO no puede aplicar los límites pedidos: solo hay control por concurrencia"
                );
            }
            // Un CLI que falla rápido (login, cuota) sale antes de leer su prompt: el `EPIPE` no es
            // un fallo de arranque; `pump` recoge su salida y su código de salida reales.
            let exited_early = |r: Result<(), symphony_process::ProcessError>| match r {
                Err(symphony_process::ProcessError::Stdin(e))
                    if e.kind() == std::io::ErrorKind::BrokenPipe =>
                {
                    Ok(())
                }
                other => other.map_err(|e| e.to_string()),
            };
            let framed = l.adapter.frame_prompt(&prompt, &l.worktree);
            exited_early(proc.write_stdin(&l.adapter.encode_prompt(&framed)).await)?;
            if l.adapter.close_stdin_after_prompt() {
                exited_early(proc.close_stdin().await)?;
            }
            Ok::<_, String>(proc)
        }
        .await;
        let proc = match started {
            Ok(p) => p,
            Err(e) => {
                let reason = format!("no se pudo lanzar {}: {e}", l.adapter.cli_name());
                self.finish_failed(&l, RunStatus::Failed, None, &reason)
                    .await;
                self.bus.run_ended(l.run_id);
                return Err(reason);
            }
        };
        let (run_id, pid) = (l.run_id, proc.pid().unwrap_or(0));
        let session = l.resume_session.clone();
        let _ = self
            .writer
            .write(Box::new(move |t| {
                Ok(repo::set_run_process(
                    t,
                    run_id,
                    pid,
                    session.as_deref(),
                    None,
                )?)
            }))
            .await;
        let (control, rx) = mpsc::unbounded_channel();
        // La inactividad se mide desde el arranque: sin esto, el watchdog mataba
        // como NO_HEARTBEAT a un CLI que tardaba un tick en imprimir su primera línea.
        self.beat(run_id);
        if let Ok(mut live) = self.live.lock() {
            live.insert(
                l.agent_id,
                LiveRun {
                    run_id,
                    adapter: l.adapter.clone(),
                    project_id: l.project_id,
                    control,
                },
            );
        }
        let task: Pin<Box<dyn Future<Output = ()> + Send>> =
            Box::pin(self.clone().pump(l, proc, rx, main_workspace));
        self.pumps.spawn(task);
        Ok(())
    }

    /// Foto del árbol de procesos del run hacia la tabla `processes` (P08.S4). Devuelve el tiempo
    /// de CPU acumulado del árbol, en ms.
    async fn record_processes(&self, l: &Launch, proc: &Supervised) -> u64 {
        let samples: Vec<symphony_store::processes::ProcSample> = proc
            .process_tree()
            .into_iter()
            .map(|p| symphony_store::processes::ProcSample {
                pid: p.pid,
                parent_pid: p.parent_pid,
                // Una línea de comando puede llevar secretos: se redacta como todo lo que se guarda.
                command: symphony_core::redact(&p.command).into_owned(),
                rss_bytes: p.rss_bytes,
                cpu_time_ms: p.cpu_time_ms,
            })
            .collect();
        if samples.is_empty() {
            return 0;
        }
        let cpu = samples.iter().map(|p| p.cpu_time_ms).sum();
        let run = l.run_id;
        let _ = self
            .writer
            .write(Box::new(move |t| {
                Ok(symphony_store::processes::sync_processes(
                    t,
                    run,
                    &samples,
                    now_ms(),
                )?)
            }))
            .await;
        cpu
    }

    /// Lee la salida del CLI y atiende el control hasta que el proceso termina.
    async fn pump(
        self,
        l: Launch,
        mut proc: Supervised,
        mut rx: mpsc::UnboundedReceiver<Control>,
        main_workspace: Option<MainWorkspace>,
    ) {
        let mut fatal: Option<ProviderError> = None;
        // Las últimas líneas de stderr: si el CLI se cae, dicen por qué (dogfooding 2026-10-24).
        let mut err_tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
        let mut control_open = true;
        let mut tree_cpu_ms = 0u64;
        let mut suspended = false;
        let mut last_line = std::time::Instant::now();
        // Más seguido que el watchdog: si no, un árbol ocupado parecería callado.
        // `interval` hace panic con 0: un `stale_after` de 0 no puede tumbar el pump.
        let mut monitor = tokio::time::interval(
            PROCESS_SAMPLE_EVERY
                .min(self.stale_after / 3)
                .max(Duration::from_millis(10)),
        );
        monitor.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let stopped = loop {
            tokio::select! {
                _ = monitor.tick() => {
                    // Un `cargo test` en frío puede pasar 15 min sin una línea: si el árbol gasta
                    // CPU, el agente trabaja, no está colgado (dogfooding 2026-10-24).
                    let cpu = self.record_processes(&l, &proc).await;
                    // Contra la muestra anterior, no contra el máximo: la suma baja cuando un hijo
                    // termina, y el siguiente comando no tiene que superar al anterior (auditoría
                    // P13.S5, hallazgo 1).
                    // Un árbol pausado está congelado a propósito, no colgado: sin salida ni CPU
                    // el watchdog lo mataría (auditoría P13.S5, hallazgo 7).
                    let busy = cpu >= tree_cpu_ms + CPU_ALIVE_MS
                        && last_line.elapsed() < CPU_BEAT_MAX_SILENCE;
                    if suspended || busy {
                        self.beat(l.run_id);
                    }
                    tree_cpu_ms = cpu;
                }
                line = proc.next_output() => match line {
                    None => break None,
                    Some(OutputLine::Stdout(line)) => {
                        self.beat(l.run_id);
                        last_line = std::time::Instant::now();
                        let mut end_turn = false;
                        if !self.handle_line(&l, &proc, &line, &mut fatal, &mut end_turn).await {
                            abort_unpublishable(&mut proc).await;
                            break None;
                        }
                        if end_turn {
                            let _ = proc.close_stdin().await;
                        }
                    }
                    Some(OutputLine::Stderr(line)) => {
                        self.beat(l.run_id);
                        last_line = std::time::Instant::now();
                        if !line.trim().is_empty() {
                            if err_tail.len() == ERR_TAIL_LINES {
                                err_tail.pop_front();
                            }
                            err_tail.push_back(line.trim().chars().take(300).collect());
                        }
                        let mut end_turn = false;
                        let events = l.adapter.parse_stderr_line(&line);
                        if !self.apply_events(&l, &proc, events, &mut fatal, &mut end_turn).await {
                            abort_unpublishable(&mut proc).await;
                            break None;
                        }
                        if end_turn {
                            let _ = proc.close_stdin().await;
                        }
                    }
                },
                ctl = rx.recv(), if control_open => match ctl {
                    None => control_open = false,
                    Some(Control::Stop { status, end, done }) => {
                        let _ = proc.terminate_tree();
                        break Some((status, end, done, StopKind::User));
                    }
                    Some(Control::NoHeartbeat { done }) => {
                        let _ = proc.terminate_tree();
                        break Some((
                            RunStatus::Failed,
                            RunEndReason::NoHeartbeat,
                            done,
                            StopKind::NoHeartbeat,
                        ));
                    }
                    Some(Control::Send { bytes, done }) => {
                        let _ = done.send(proc.write_stdin(&bytes).await.map_err(|e| e.to_string()));
                    }
                    Some(Control::Suspend(done)) => {
                        let r = proc.suspend().map_err(|e| e.to_string());
                        suspended = suspended || r.is_ok();
                        let _ = done.send(r);
                    }
                    Some(Control::Resume(done)) => {
                        let r = proc.resume().map_err(|e| e.to_string());
                        suspended = suspended && r.is_err();
                        let _ = done.send(r);
                    }
                },
            }
        };
        let exit = proc.wait().await;
        self.check_main_workspace(&l, main_workspace).await;
        let run = l.run_id;
        let _ = self
            .writer
            .write(Box::new(move |t| {
                Ok(symphony_store::processes::close_processes(
                    t,
                    run,
                    now_ms(),
                )?)
            }))
            .await;
        if let Ok(mut live) = self.live.lock()
            && live.get(&l.agent_id).is_some_and(|r| r.run_id == l.run_id)
        {
            live.remove(&l.agent_id);
        }
        if let Ok(mut activity) = self.activity.lock() {
            activity.remove(&l.run_id);
        }
        let code = match exit {
            ExitStatus::Exited(c) => Some(c),
            _ => None,
        };
        match (stopped, fatal) {
            (Some((status, end, done, kind)), fatal) => {
                let run = l.run_id;
                let (agent, project) = (l.agent_id, l.project_id);
                let (provider, model) = (l.provider_id.clone(), l.model_id.clone());
                let health_cfg = self.bus.health_config();
                let _ = self
                    .writer
                    .write(Box::new(move |t| {
                        let now = now_ms();
                        // Sin latido, pero con un límite o un login rechazado poco antes: esa es
                        // la causa, y así se registra (no `NO_HEARTBEAT`).
                        let cause = if matches!(kind, StopKind::NoHeartbeat) {
                            repo::recent_run_failure(t, run, now, DEATH_CAUSE_WINDOW_MS)?
                                .and_then(|(f, _)| death_cause(f))
                        } else {
                            None
                        };
                        let end = cause.map_or(end, |c| c.0);
                        repo::close_run(t, run, status, end, code, now)?;
                        // Un fallo de cuota o de login que llegó justo antes de la parada no se
                        // pierde: el recorder lo dejó para el failover, que ya no va a correr.
                        if let Some(err) = &fatal {
                            crate::health::record_failure_row(
                                t,
                                ProviderFailureId::new(),
                                Some(run),
                                &provider,
                                Some(&model),
                                err,
                                now,
                            )?;
                            crate::health::on_failure(t, &provider, Some(&model), err, now)?;
                        }
                        // Lo gastado hasta aquí cuenta aunque el run no haya terminado bien.
                        crate::health::on_run_finished(t, run, &health_cfg, now)?;
                        match kind {
                            StopKind::User => {
                                if status == RunStatus::HandedOff {
                                    repo::set_handoff_outcome(t, run, "CONTINUED")?;
                                }
                            }
                            StopKind::NoHeartbeat => {
                                repo::set_handoff_outcome(t, run, "FAILED_TO_CONTINUE")?;
                                let (kind_name, why) = match cause {
                                    Some((_, k, phrase)) => (k, phrase),
                                    None => ("NO_HEARTBEAT", "el executor dejó de responder"),
                                };
                                let reason = format!(
                                    "{why}; el workspace y el último checkpoint quedan intactos"
                                );
                                repo::set_agent_state(
                                    t,
                                    agent,
                                    AgentState::Failed,
                                    Some(&reason),
                                    now,
                                )?;
                                repo::open_recovery_item(
                                    t,
                                    project,
                                    Some(agent),
                                    Some(run),
                                    kind_name,
                                    &reason,
                                    now,
                                )?;
                            }
                        }
                        Ok(())
                    }))
                    .await;
                let _ = done.send(());
            }
            (None, Some(err)) => self.failover(&l, &err, code).await,
            (None, None) => self.finish_natural(&l, exit, err_tail).await,
        }
        self.bus.run_ended(l.run_id);
    }

    /// Una línea del stream: eventos al bus. `false` si la base ya no acepta escrituras.
    async fn handle_line(
        &self,
        l: &Launch,
        proc: &Supervised,
        line: &str,
        fatal: &mut Option<ProviderError>,
        end_turn: &mut bool,
    ) -> bool {
        let events = l.adapter.parse_stream_line(line);
        self.apply_events(l, proc, events, fatal, end_turn).await
    }

    /// Eventos de stdout o de stderr (ADR-0009) al bus. `false` si la base ya no acepta escrituras.
    async fn apply_events(
        &self,
        l: &Launch,
        proc: &Supervised,
        events: Vec<AgentEvent>,
        fatal: &mut Option<ProviderError>,
        end_turn: &mut bool,
    ) -> bool {
        for event in events {
            match &event {
                AgentEvent::SessionStarted {
                    cli_session_id: Some(cli),
                    ..
                } => {
                    let (run, cli, pid) = (l.run_id, cli.clone(), proc.pid().unwrap_or(0));
                    let _ = self
                        .writer
                        .write(Box::new(move |t| {
                            Ok(repo::set_run_process(t, run, pid, Some(&cli), None)?)
                        }))
                        .await;
                }
                AgentEvent::ProviderError(e) if needs_failover(e) => {
                    *fatal = Some(e.clone());
                    *end_turn = true;
                }
                // Un CLI que deja stdin abierto (Claude) espera otro mensaje al terminar
                // el turno y nunca sale: cerrar stdin lo termina con su exit code real.
                AgentEvent::TurnFinished { .. } if !l.adapter.close_stdin_after_prompt() => {
                    *end_turn = true;
                }
                _ => {}
            }
            let ev = BusEvent {
                project_id: l.project_id.to_string(),
                agent_id: Some(l.agent_id.to_string()),
                run_id: Some(l.run_id.to_string()),
                source: EventSource::JsonStream,
                event,
                occurred_at: now_ms(),
            };
            if self.bus.publish(ev).await.is_err() {
                return false;
            }
        }
        true
    }

    /// El CLI terminó solo, sin un fallo de proveedor que pida failover.
    async fn finish_natural(
        &self,
        l: &Launch,
        exit: ExitStatus,
        err_tail: std::collections::VecDeque<String>,
    ) {
        // ponytail: exit 0 = tarea terminada; el heartbeat (P06.S6) cubre los cuelgues.
        if exit == ExitStatus::Exited(0) {
            // Con plan por fases, el CLI que salió bien puede dejar fases pendientes (sesión
            // nueva), una tarea fallida (el agente se detiene) o el plan completo.
            if self.advance_plan(l).await {
                return;
            }
            // Gate de la tarea (P08.S6): validar antes de completar; un fallo vuelve al agente.
            if !self.task_is_chat(l.task_id) && self.run_gate(l).await {
                return;
            }
            // Pasó (o no hay gate): el trabajo queda en un commit de la rama del agente (F7).
            if !self.task_is_chat(l.task_id) {
                self.commit_agent_work(l).await;
            }
            self.commit_chat_turn(l).await;
            let (agent, task, run) = (l.agent_id, l.task_id, l.run_id);
            let (provider, model) = (l.provider_id.clone(), l.model_id.clone());
            let health_cfg = self.bus.health_config();
            let result = self
                .writer
                .write(Box::new(move |t| {
                    let now = now_ms();
                    repo::close_run(
                        t,
                        run,
                        RunStatus::Exited,
                        RunEndReason::Completed,
                        Some(0),
                        now,
                    )?;
                    repo::set_handoff_outcome(t, run, "CONTINUED")?;
                    // Un turno terminó bien: el proveedor está sano y se anota el uso estimado
                    // si su CLI no informó tokens.
                    crate::health::on_success(t, &provider, Some(&model), now)?;
                    crate::health::on_run_finished(t, run, &health_cfg, now)?;
                    if repo::get_task(t, task)?.code == repo::CHAT_TASK_CODE {
                        repo::set_agent_state(
                            t,
                            agent,
                            AgentState::Ready,
                            Some("esperando mensaje"),
                            now,
                        )?;
                        repo::set_task_status(t, task, TaskStatus::Ready, None, now)?;
                    } else {
                        repo::set_agent_state(t, agent, AgentState::Completed, None, now)?;
                        repo::set_task_status(t, task, TaskStatus::Done, None, now)?;
                    }
                    Ok(())
                }))
                .await;
            if let Err(e) = result {
                tracing::error!(agent = %agent, error = %e, "no se pudo cerrar el run");
            }
            return;
        }
        // Una señal que Symphony no mandó (abort, OOM killer) es un crash: `FAILED`.
        // `KILLED` queda para stop/kill del usuario (P06.S7).
        let cli = l.adapter.cli_name();
        let (code, reason) = match exit {
            ExitStatus::Exited(c) => (Some(c), format!("{cli} terminó con código {c}")),
            ExitStatus::Killed(Some(sig)) => (None, format!("{cli} terminó por la señal {sig}")),
            ExitStatus::Killed(None) | ExitStatus::Unknown => {
                (None, format!("{cli} terminó de forma inesperada"))
            }
        };
        let reason = if err_tail.is_empty() {
            reason
        } else {
            let tail = Vec::from(err_tail).join(
                "
",
            );
            format!("{reason}; stderr: {}", symphony_core::redact(&tail))
        };
        self.finish_failed(l, RunStatus::Failed, code, &reason)
            .await;
    }

    /// Deja el trabajo del agente en un commit de su rama al completarse la tarea
    /// (`[merge] commit_on_complete`, por defecto sí). Sin cambios no hace nada.
    async fn commit_agent_work(&self, l: &Launch) {
        let on = self
            .reader
            .lock()
            .ok()
            .and_then(|c| repo::get_project(&c, l.project_id).ok())
            .and_then(|p| {
                symphony_core::load_project(Path::new(&p.root_path))
                    .ok()
                    .flatten()
            })
            .and_then(|p| p.merge)
            .unwrap_or_default()
            .commit_on_complete;
        if !on {
            return;
        }
        let message = self
            .reader
            .lock()
            .ok()
            .and_then(|c| repo::get_task(&c, l.task_id).ok())
            .map_or_else(
                || "symphony: tarea completada".to_string(),
                |t| format!("{}: {}", t.code, t.title),
            );
        let worktree = l.worktree.clone();
        let agent = l.agent_id;
        match tokio::task::spawn_blocking(move || Repo::at(worktree).commit_all(&message)).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                tracing::warn!(agent = %agent, error = %e, "no se pudo commitear el trabajo del agente")
            }
            Err(e) => {
                tracing::warn!(agent = %agent, error = %e, "no se pudo commitear el trabajo del agente")
            }
        }
    }

    /// Turno terminado del chat: commit automático de lo que dejó en su worktree (P07.5.S5).
    /// Corre una vez por run terminado, no por evento; los demás agentes salen tras una lectura.
    async fn commit_chat_turn(&self, l: &Launch) {
        let (task, agent) = (l.task_id, l.agent_id);
        let is_chat = self
            .read_op(|c| Ok(repo::get_task(c, task)?.code == repo::CHAT_TASK_CODE))
            .unwrap_or(false);
        if !is_chat {
            return;
        }
        // El `TurnFinished` de este turno ya está encolado: se espera a que se escriba para contarlo.
        let _ = self.writer.flush().await;
        let turn = self
            .read_op(|c| repo::turns_finished(c, agent))
            .unwrap_or(0);
        let message = format!("chat: turno {turn} ({})", l.model_id);
        let worktree = l.worktree.clone();
        match tokio::task::spawn_blocking(move || Repo::at(worktree).commit_all(&message)).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                tracing::warn!(agent = %agent, error = %e, "no se pudo commitear el turno del chat")
            }
            Err(e) => {
                tracing::warn!(agent = %agent, error = %e, "no se pudo commitear el turno del chat")
            }
        }
    }

    /// Run `FAILED/CRASH`, agente `FAILED` con razón y recovery item `EXECUTOR_EXITED`.
    async fn finish_failed(&self, l: &Launch, status: RunStatus, code: Option<i32>, reason: &str) {
        let (project, agent, run, reason) =
            (l.project_id, l.agent_id, l.run_id, reason.to_string());
        let health_cfg = self.bus.health_config();
        let result = self
            .writer
            .write(Box::new(move |t| {
                let now = now_ms();
                // Salió con error poco después de un límite o un login rechazado: esa es la
                // causa (el `EXECUTOR_EXITED` genérico no ayudaba a decidir qué hacer).
                let cause = repo::recent_run_failure(t, run, now, DEATH_CAUSE_WINDOW_MS)?
                    .and_then(|(f, _)| death_cause(f));
                let end = cause.map_or(RunEndReason::Crash, |c| c.0);
                let (kind_name, reason) = match cause {
                    Some((_, k, phrase)) => (k, format!("{phrase}. {reason}")),
                    None => ("EXECUTOR_EXITED", reason),
                };
                repo::close_run(t, run, status, end, code, now)?;
                crate::health::on_run_finished(t, run, &health_cfg, now)?;
                repo::set_handoff_outcome(t, run, "FAILED_TO_CONTINUE")?;
                repo::set_agent_state(t, agent, AgentState::Failed, Some(&reason), now)?;
                repo::open_recovery_item(
                    t,
                    project,
                    Some(agent),
                    Some(run),
                    kind_name,
                    &reason,
                    now,
                )?;
                Ok(())
            }))
            .await;
        if let Err(e) = result {
            tracing::error!(agent = %agent, error = %e, "no se pudo cerrar el run");
        }
    }

    /// El mejor reemplazo según la política del agente (P10.S5, IDEA §5.7): el router elige entre
    /// los modelos utilizables, sin tocar lo que este agente ya agotó ni la reserva de cuota.
    /// `rotate`: rotación a propósito (umbral de contexto): se aparta el proveedor actual.
    fn pick_replacement(
        &self,
        c: &rusqlite::Connection,
        agent: &repo::Agent,
        why: Replace<'_>,
    ) -> Result<(Option<repo::EligibleModel>, Option<crate::routing::Pending>), repo::RepoError>
    {
        let Replace {
            provider: current_provider,
            model: current_model,
            auth,
            rotate,
            needed_context,
        } = why;
        if agent.failover_policy == FailoverPolicy::None {
            return Ok((None, None));
        }
        let profile = agent
            .requested_profile_id
            .clone()
            .unwrap_or_else(|| self.default_profile());
        use symphony_core::RejectReason;
        use symphony_router::Exclusion;
        // Lo que este agente ya agotó, o donde ya le rechazaron el login: no se vuelve.
        let failed: Vec<(String, String, String)> = c
            .prepare(
                "SELECT provider_id, model_id, end_reason FROM agent_runs
                 WHERE agent_id = ?1 AND end_reason IN ('QUOTA_EXHAUSTED','AUTH_ERROR')",
            )?
            .query_map([agent.id.to_string()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<Result<_, _>>()?;
        let mut excluded: Vec<Exclusion> = failed
            .into_iter()
            .map(|(provider, model, why)| {
                if why == "AUTH_ERROR" {
                    Exclusion {
                        model_id: None,
                        provider_id: Some(provider),
                        reason: RejectReason::Auth,
                    }
                } else {
                    Exclusion {
                        model_id: Some(model),
                        provider_id: None,
                        reason: RejectReason::Exhausted,
                    }
                }
            })
            .collect();
        let same = agent.failover_policy == FailoverPolicy::SameProvider;
        let mut choose = crate::routing::Choose {
            profile: profile.clone(),
            only_provider: same.then(|| current_provider.to_string()),
            needed_context,
            ..Default::default()
        };
        if rotate {
            if same {
                choose.skip_model = Some(current_model.to_string());
            } else {
                choose.skip_provider = Some(current_provider.to_string());
            }
        } else {
            excluded.push(if auth {
                Exclusion {
                    model_id: None,
                    provider_id: Some(current_provider.to_string()),
                    reason: RejectReason::Auth,
                }
            } else {
                Exclusion {
                    model_id: Some(current_model.to_string()),
                    provider_id: None,
                    reason: RejectReason::Exhausted,
                }
            });
        }
        choose.excluded = excluded;
        let decision = self.evaluate(c, &choose)?;
        let selected = decision.selected.clone();
        let next = match &selected {
            Some(id) => repo::eligible_model(c, id)?.ok(),
            None => None,
        };
        let pending = crate::routing::Pending {
            explanation: decision.explanation.clone(),
            selected,
            profile: Some(profile),
            trigger: symphony_core::RoutingTrigger::Failover,
            decision,
        };
        Ok((next, Some(pending)))
    }

    /// Cuota agotada o login rechazado (P06.S5, P10.S5, IDEA §5.7): el router elige otro
    /// executor según la política del agente; si no hay uno, `WAITING_PROVIDER` + recovery item.
    async fn failover(&self, l: &Launch, err: &ProviderError, code: Option<i32>) {
        let auth = err.failure_type == FailureType::Auth;
        let end = if auth {
            RunEndReason::AuthError
        } else {
            RunEndReason::QuotaExhausted
        };
        let who = l.adapter.display_name();
        let why = if auth {
            format!("{who} rechazó el login")
        } else {
            format!("{who} se quedó sin cuota")
        };
        let failure_id = ProviderFailureId::new();
        let (failed_provider, failed_model, failed_err) =
            (l.provider_id.clone(), l.model_id.clone(), err.clone());
        let run = l.run_id;

        // 1. El fallo y la salud primero: el router tiene que verlos al elegir.
        let recorded = self
            .writer
            .write(Box::new(move |t| {
                let now = now_ms();
                crate::health::record_failure_row(
                    t,
                    failure_id,
                    Some(run),
                    &failed_provider,
                    Some(&failed_model),
                    &failed_err,
                    now,
                )?;
                crate::health::on_failure(
                    t,
                    &failed_provider,
                    Some(&failed_model),
                    &failed_err,
                    now,
                )?;
                Ok(())
            }))
            .await;
        if let Err(e) = recorded {
            tracing::error!(agent = %l.agent_id, error = %e, "failover: no se pudo registrar el fallo");
            return;
        }

        // 2. El router elige el reemplazo, sin los modelos cuya ventana no cabe el handoff.
        let needed = self.handoff_tokens(l.agent_id).await;
        let picked = self.read_op(|c| {
            let agent = repo::get_agent(c, l.agent_id)?;
            let why = Replace {
                provider: &l.provider_id,
                model: &l.model_id,
                auth,
                rotate: false,
                needed_context: needed,
            };
            let (next, pending) = self.pick_replacement(c, &agent, why)?;
            Ok((agent, next, pending))
        });
        let (agent, next, pending) = match picked {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(agent = %l.agent_id, error = %e, "failover: no se pudo elegir el reemplazo");
                return;
            }
        };

        // 3. Cierra el run; si no hay reemplazo, la decisión se guarda aquí (si lo hay, queda
        //    atada al run nuevo).
        let status = if next.is_some() {
            RunStatus::HandedOff
        } else {
            RunStatus::Failed
        };
        let (agent_id, wait_decision) = (
            l.agent_id,
            next.is_none().then(|| pending.clone()).flatten(),
        );
        let health_cfg = self.bus.health_config();
        let closed = self
            .writer
            .write(Box::new(move |t| {
                let now = now_ms();
                repo::close_run(t, run, status, end, code, now)?;
                repo::set_handoff_outcome(t, run, "CONTINUED")?;
                crate::health::on_run_finished(t, run, &health_cfg, now)?;
                if let Some(p) = &wait_decision {
                    p.save(t, agent_id, None, now)?;
                }
                Ok(())
            }))
            .await;
        if let Err(e) = closed {
            tracing::error!(agent = %l.agent_id, error = %e, "failover: no se pudo cerrar el run");
            return;
        }

        let Some(model) = next else {
            let detail = if agent.failover_policy == FailoverPolicy::None {
                format!("{why}; el failover está desactivado para este agente")
            } else {
                format!("{why} y no hay otro proveedor disponible")
            };
            let (project, agent_id) = (l.project_id, l.agent_id);
            let kind = if auth { "AUTH_ERROR" } else { "RATE_LIMITED" };
            let result = self
                .writer
                .write(Box::new(move |t| {
                    let now = now_ms();
                    repo::insert_executor_change(
                        t,
                        &repo::NewExecutorChange {
                            id: ExecutorChangeId::new(),
                            agent_id,
                            from_run_id: run,
                            to_run_id: None,
                            reason: "FAILOVER",
                            failure_id: Some(failure_id),
                            checkpoint_id: None,
                            checkpoint_age_ms: None,
                        },
                        now,
                    )?;
                    repo::set_agent_state(
                        t,
                        agent_id,
                        AgentState::WaitingProvider,
                        Some(&detail),
                        now,
                    )?;
                    repo::open_recovery_item(
                        t,
                        project,
                        Some(agent_id),
                        Some(run),
                        kind,
                        &detail,
                        now,
                    )?;
                    Ok(())
                }))
                .await;
            if let Err(e) = result {
                tracing::error!(agent = %agent_id, error = %e, "failover: no se pudo dejar al agente esperando");
            }
            return;
        };
        let reason = format!("El executor anterior ({} · {}): {why}.", who, l.model_id);
        if let Err(e) = self
            .start_successor(
                &agent,
                Some(l.run_id),
                model,
                &reason,
                "FAILOVER",
                Some(failure_id),
                None,
                pending,
                None,
            )
            .await
        {
            tracing::error!(agent = %l.agent_id, error = %e, "failover: no arrancó el siguiente executor");
        }
    }

    /// Abre un run nuevo para el agente con `model` y lo lanza. Con `from_run`, es un
    /// cambio de executor: handoff desde el checkpoint, `executor_changes` y separador en
    /// la conversación (FLOW §13.4). Sin `from_run`, es su primer executor (prompt = objetivo).
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn start_successor(
        &self,
        agent: &repo::Agent,
        from_run: Option<RunId>,
        model: repo::EligibleModel,
        reason: &str,
        change: &'static str,
        failure_id: Option<ProviderFailureId>,
        message: Option<&str>,
        route: Option<crate::routing::Pending>,
        prompt_override: Option<String>,
    ) -> Result<RunId, AgentOpError> {
        let adapter = self
            .adapter(&model.provider_id)
            .ok_or_else(|| AgentOpError(format!("no hay adapter para `{}`", model.provider_id)))?;
        // Que el checkpoint incluya todo lo que llegó hasta ahora.
        self.bus.checkpoints_idle().await;
        let (prompt, handoff, change_row, separator) = match from_run {
            Some(from) => {
                let h = self
                    .prepare_handoff(agent.id, reason)
                    .await
                    .map_err(AgentOpError)?;
                let old = self.read_op(|c| repo::get_run(c, from))?;
                // El id del modelo ya lleva al proveedor: no se repite (observación de Leo, Día 2).
                let separator = format!(
                    "── Executor cambiado: {} → {}\nMotivo: {reason}\nAgente #{}, tarea y workspace sin cambios\nHandoff restaurado del checkpoint #{}",
                    old.model_id, model.model_id, agent.number, h.checkpoint_seq
                );
                // FLOW §13.3: el checkpoint puede estar atrasado; el código se rehace del worktree.
                let separator = if h.stale {
                    format!(
                        "{separator}\nAviso: el checkpoint #{} puede estar atrasado (hubo actividad después); el estado del código se reconstruyó del workspace",
                        h.checkpoint_seq
                    )
                } else {
                    separator
                };
                let change_row = (from, h.checkpoint_id, now_ms() - h.checkpoint_created_at);
                let handoff = (
                    h.checkpoint_id,
                    h.mode,
                    h.tokens_raw_estimate,
                    h.tokens_sent,
                    h.build_ms,
                    h.items,
                );
                // Una fase nueva no arrastra la conversación anterior (ADR-0011 §3): su prompt
                // lo arma el plan; el handoff queda registrado igual.
                (
                    prompt_override.unwrap_or(h.prompt),
                    handoff,
                    Some(change_row),
                    Some(separator),
                )
            }
            None => {
                let objective = self
                    .read_op(|c| repo::latest_checkpoint(c, agent.id))?
                    .map(|c| c.objective)
                    .ok_or_else(|| AgentOpError("el agente no tiene checkpoint".into()))?;
                let tokens = symphony_context::handoff::estimate_tokens(&objective);
                // El chat y los review agents no planifican por fases.
                let is_chat = self.read_op(|c| {
                    Ok(
                        repo::get_task(c, agent.task_id)?.code == repo::CHAT_TASK_CODE
                            || repo::task_kind(c, agent.task_id)? == "REVIEW",
                    )
                })?;
                let prompt = prompt_override
                    .unwrap_or_else(|| self.planned_prompt(agent.task_id, is_chat, &objective));
                (
                    prompt,
                    (None, agent.context_mode, tokens, tokens, 0, Vec::new()),
                    None,
                    None,
                )
            }
        };
        // El mensaje nuevo del usuario (cambio manual) va al final, para que lo responda primero.
        let prompt = match message {
            Some(m) => with_user_message(&prompt, m, &model.provider_id),
            None => prompt,
        };
        let run = RunId::new();
        let (agent_id, task_id, state) = (agent.id, agent.task_id, agent.state);
        let (provider, model_id) = (model.provider_id.clone(), model.model_id.clone());
        self.writer
            .write(Box::new(move |t| {
                let now = now_ms();
                // Un FAILED se reabre pasando por READY (reclaim, IDEA §5.10).
                if state == AgentState::Failed {
                    repo::set_agent_state(t, agent_id, AgentState::Ready, None, now)?;
                }
                if state != AgentState::Running {
                    repo::set_agent_state(t, agent_id, AgentState::Running, None, now)?;
                }
                if repo::get_task(t, task_id)?.status != TaskStatus::Running {
                    repo::set_task_status(t, task_id, TaskStatus::Running, None, now)?;
                }
                repo::open_run(t, run, agent_id, &provider, &model_id, now)?;
                if let Some(p) = &route {
                    p.save(t, agent_id, Some(run), now)?;
                }
                let (checkpoint_id, mode, raw, sent, build_ms, items) = handoff;
                repo::insert_handoff(
                    t,
                    &repo::NewHandoff {
                        id: HandoffId::new(),
                        agent_id,
                        checkpoint_id,
                        to_run_id: run,
                        mode,
                        tokens_raw_estimate: raw,
                        tokens_sent: sent,
                        build_ms,
                        items,
                    },
                    now,
                )?;
                if let Some((from, checkpoint_id, age)) = change_row {
                    repo::insert_executor_change(
                        t,
                        &repo::NewExecutorChange {
                            id: ExecutorChangeId::new(),
                            agent_id,
                            from_run_id: from,
                            to_run_id: Some(run),
                            reason: change,
                            failure_id,
                            checkpoint_id,
                            checkpoint_age_ms: Some(age),
                        },
                        now,
                    )?;
                }
                if let Some(text) = &separator {
                    repo::insert_message(
                        t,
                        &repo::NewMessage {
                            id: MessageId::new(),
                            agent_id,
                            run_id: Some(run),
                            role: repo::MessageRole::ExecutorChange,
                            content: Some(text.clone()),
                            content_object_id: None,
                        },
                        now,
                    )?;
                }
                Ok(())
            }))
            .await
            .map_err(op_err)?;

        let worktree = self
            .read_op(|c| {
                let wt = agent.worktree_id.ok_or_else(|| repo::RepoError::NotFound {
                    entity: "worktree",
                    id: agent.id.to_string(),
                })?;
                repo::get_worktree(c, wt)
            })?
            .path;
        let launch = Launch {
            adapter,
            project_id: agent.project_id,
            agent_id: agent.id,
            task_id: agent.task_id,
            run_id: run,
            provider_id: model.provider_id,
            model_id: model.model_id,
            worktree: PathBuf::from(worktree),
            cli_model: model.cli_model_id,
            prompt,
            resume_session: None,
        };
        // Si no arranca, `launch` ya dejó al agente `FAILED` con razón y recovery item.
        let (project, agent_id) = (agent.project_id, agent.id);
        let launched = self.launch(launch).await;
        // El mensaje se guarda aparte como `USER`: el primer `USER` del run es el prompt del
        // handoff y se descarta al armar la conversación (P07.5.S3).
        if let (Some(text), Ok(())) = (message, launched) {
            let ev = BusEvent {
                project_id: project.to_string(),
                agent_id: Some(agent_id.to_string()),
                run_id: Some(run.to_string()),
                source: EventSource::User,
                event: AgentEvent::UserMessage {
                    text: text.to_string(),
                },
                occurred_at: now_ms(),
            };
            self.bus.publish(ev).await.map_err(op_err)?;
        }
        Ok(run)
    }

    /// Manda una orden al executor vivo del agente y espera la respuesta.
    pub(crate) async fn control<T>(
        &self,
        agent: AgentId,
        make: impl FnOnce(oneshot::Sender<T>) -> Control,
    ) -> Option<T> {
        self.control_if(agent, None, make).await
    }

    /// Manda una orden al executor vivo del agente solo si sigue siendo `expected_run` (cuando se
    /// indica), comprobándolo bajo el lock de `live`, y espera la respuesta.
    pub(crate) async fn control_if<T>(
        &self,
        agent: AgentId,
        expected_run: Option<RunId>,
        make: impl FnOnce(oneshot::Sender<T>) -> Control,
    ) -> Option<T> {
        let (tx, rx) = oneshot::channel();
        let sent = self
            .live
            .lock()
            .ok()
            .and_then(|live| {
                live.get(&agent)
                    .filter(|r| expected_run.is_none_or(|run| r.run_id == run))
                    .map(|r| r.control.send(make(tx)).is_ok())
            })
            .unwrap_or(false);
        if !sent {
            return None;
        }
        rx.await.ok()
    }

    /// `true` si el agente tiene un executor vivo.
    pub fn is_live(&self, agent: AgentId) -> bool {
        self.live.lock().is_ok_and(|l| l.contains_key(&agent))
    }

    /// Cambio manual de modelo (P06.S5): `symphony switch <agente> --model <provider/model>`.
    /// Es una elección exacta del usuario: queda como el modelo pedido del agente.
    pub async fn switch(&self, agent_id: AgentId, model_id: &str) -> Result<RunId, AgentOpError> {
        self.switch_with_message(agent_id, model_id, None).await
    }

    /// Como `switch`, y además entrega `message` al modelo nuevo: el chat cambia de
    /// proveedor a mitad de conversación sin que el usuario repita nada (P07.5.S6).
    pub async fn switch_with_message(
        &self,
        agent_id: AgentId,
        model_id: &str,
        message: Option<&str>,
    ) -> Result<RunId, AgentOpError> {
        self.switch_inner(agent_id, model_id, message, None).await
    }

    /// Cambio de modelo eligiendo por profile (FLOW §8.3): el router escoge entre los modelos
    /// utilizables ahora y la decisión queda explicable. Devuelve el run y el modelo elegido.
    pub async fn switch_to_profile(
        &self,
        agent_id: AgentId,
        profile: &str,
        message: Option<&str>,
    ) -> Result<(RunId, String), AgentOpError> {
        let profile_name = profile.to_string();
        // El handoff que recibiría el modelo nuevo: el router descarta a quien no lo cabe.
        let needed = self.handoff_tokens(agent_id).await;
        let decision = self.read_op(|c| {
            if crate::routing::profile_weights(c, &profile_name)?.is_none() {
                return Ok(None);
            }
            let choose = crate::routing::Choose {
                profile: profile_name.clone(),
                needed_context: needed,
                ..Default::default()
            };
            Ok(Some(self.evaluate(c, &choose)?))
        })?;
        let Some(decision) = decision else {
            return Err(AgentOpError(format!("el profile `{profile}` no existe")));
        };
        let Some(model_id) = decision.selected.clone() else {
            return Err(AgentOpError(format!(
                "ningún modelo es elegible para {profile} ahora.\n{}",
                decision.explanation
            )));
        };
        let route = crate::routing::Pending {
            explanation: decision.explanation.clone(),
            selected: Some(model_id.clone()),
            profile: Some(profile.to_string()),
            trigger: symphony_core::RoutingTrigger::Switch,
            decision,
        };
        let run = self
            .switch_inner(agent_id, &model_id, message, Some(route))
            .await?;
        Ok((run, model_id))
    }

    async fn switch_inner(
        &self,
        agent_id: AgentId,
        model_id: &str,
        message: Option<&str>,
        profile_route: Option<crate::routing::Pending>,
    ) -> Result<RunId, AgentOpError> {
        let model = self
            .read_op(|c| repo::eligible_model(c, model_id))?
            .map_err(|reason| {
                AgentOpError(format!("no se puede cambiar a `{model_id}`: {reason}"))
            })?;
        if self.adapter(&model.provider_id).is_none() {
            return Err(AgentOpError(format!(
                "esta versión no tiene adapter para `{}`",
                model.provider_id
            )));
        }
        let agent = self.read_op(|c| repo::get_agent(c, agent_id))?;
        if !matches!(
            agent.state,
            AgentState::Running
                | AgentState::Ready
                | AgentState::WaitingProvider
                | AgentState::Paused
                | AgentState::Failed
                | AgentState::Blocked
        ) {
            return Err(AgentOpError(format!(
                "el agente #{} está {} y no puede cambiar de modelo",
                agent.number,
                agent.state.as_str()
            )));
        }
        // Parar el executor vivo, si hay (queda `HANDED_OFF / USER_SWITCH`).
        self.control(agent_id, |done| Control::Stop {
            status: RunStatus::HandedOff,
            end: RunEndReason::UserSwitch,
            done,
        })
        .await;
        let (agent, from_run) = self.read_op(|c| {
            let agent = repo::get_agent(c, agent_id)?;
            let last = repo::runs_of(c, agent_id)?.pop();
            Ok((agent, last))
        })?;
        // Un run que siguiera abierto sin proceso vivo (daemon anterior) se cierra acá.
        if let Some(open) = from_run.as_ref().filter(|r| r.ended_at.is_none()) {
            let run = open.id;
            self.writer
                .write(Box::new(move |t| {
                    Ok(repo::close_run(
                        t,
                        run,
                        RunStatus::HandedOff,
                        RunEndReason::UserSwitch,
                        None,
                        now_ms(),
                    )?)
                }))
                .await
                .map_err(op_err)?;
        }
        let model_for_agent = model.model_id.clone();
        let mut agent = agent;
        if agent.state == AgentState::Blocked {
            // BLOCKED → READY es la salida manual de un bloqueo.
            let id = agent.id;
            self.writer
                .write(Box::new(move |t| {
                    repo::set_agent_state(t, id, AgentState::Ready, None, now_ms())?;
                    Ok(())
                }))
                .await
                .map_err(op_err)?;
            agent.state = AgentState::Ready;
        }
        self.writer
            .write(Box::new(move |t| {
                Ok(repo::set_agent_exact_model(
                    t,
                    agent_id,
                    &model_for_agent,
                    now_ms(),
                )?)
            }))
            .await
            .map_err(op_err)?;
        let reason = format!("El usuario cambió el modelo a {}.", model.model_id);
        // El cambio manual se obedece y no se sustituye; igual se deja constancia de cómo
        // estaban los demás modelos para `/explain-route`.
        let exact = model.model_id.clone();
        let route = match profile_route {
            Some(r) => Some(r),
            None => self
                .read_op(|c| {
                    let choose = crate::routing::Choose {
                        profile: self.default_profile(),
                        allow_reserve: true,
                        ..Default::default()
                    };
                    let decision = self.evaluate(c, &choose)?;
                    Ok(crate::routing::Pending {
                        explanation: crate::routing::exact_explanation(
                            &exact,
                            &decision.explanation,
                        ),
                        selected: Some(exact),
                        profile: None,
                        trigger: symphony_core::RoutingTrigger::Switch,
                        decision,
                    })
                })
                .ok(),
        };
        self.start_successor(
            &agent,
            from_run.map(|r| r.id),
            model,
            &reason,
            "USER_SWITCH",
            None,
            message,
            route,
            None,
        )
        .await
    }

    /// La sesión del agente para abrirla en el CLI oficial (attach, ADR-0005).
    /// Nunca con el executor vivo: serían dos procesos sobre la misma sesión.
    /// Devuelve el spec y el nombre del CLI.
    pub fn attach_spec(
        &self,
        agent_id: AgentId,
    ) -> Result<(symphony_process::ProcessSpec, &'static str), AgentOpError> {
        let (agent, worktree, run, cli_model) = self.read_op(|c| {
            use rusqlite::OptionalExtension;
            let agent = repo::get_agent(c, agent_id)?;
            let worktree = agent
                .worktree_id
                .map(|w| repo::get_worktree(c, w))
                .transpose()?;
            let run = repo::runs_of(c, agent_id)?
                .into_iter()
                .rev()
                .find(|r| r.cli_session_id.is_some());
            let cli_model = match &run {
                Some(r) => c
                    .query_row(
                        "SELECT cli_model_id FROM models WHERE id = ?1",
                        [&r.model_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?,
                None => None,
            };
            Ok((agent, worktree, run, cli_model))
        })?;
        let n = agent.number;
        if self.is_live(agent_id) {
            return Err(AgentOpError(format!(
                "el agente #{n} está trabajando: espera a que termine su turno o detenlo antes de abrirlo en el CLI"
            )));
        }
        let Some(run) = run else {
            return Err(AgentOpError(format!(
                "el agente #{n} todavía no tiene una sesión en un CLI: asígnale un modelo primero"
            )));
        };
        let worktree =
            worktree.ok_or_else(|| AgentOpError(format!("el agente #{n} no tiene workspace")))?;
        let adapter = self.adapter(&run.provider_id).ok_or_else(|| {
            AgentOpError(format!(
                "esta versión no tiene adapter para `{}`",
                run.provider_id
            ))
        })?;
        let model = cli_model.unwrap_or_else(|| {
            run.model_id
                .split_once('/')
                .map_or(run.model_id.clone(), |(_, m)| m.to_string())
        });
        let session = run.cli_session_id.unwrap_or_default();
        let spec = adapter
            .attach_spec(&session, &model, std::path::Path::new(&worktree.path))
            .map_err(|e| AgentOpError(e.to_string()))?;
        Ok((spec, adapter.display_name()))
    }

    /// Deja constancia en la conversación de que Leo abrió la sesión en el CLI.
    pub async fn note_attached(&self, agent_id: AgentId, cli: &str) -> Result<(), AgentOpError> {
        let content = format!(
            "Sesión abierta en {cli} por el usuario. Lo que se haga ahí queda en el workspace del agente."
        );
        self.writer
            .write(Box::new(move |t| {
                repo::insert_message(
                    t,
                    &repo::NewMessage {
                        id: symphony_core::MessageId::new(),
                        agent_id,
                        run_id: None,
                        role: repo::MessageRole::System,
                        content: Some(content),
                        content_object_id: None,
                    },
                    now_ms(),
                )?;
                Ok(())
            }))
            .await
            .map_err(op_err)
    }

    /// Mensaje del usuario al executor vivo (ADR-0005). Claude lo recibe a media tarea
    /// por stdin; un CLI que solo acepta mensajes entre turnos (Codex) lo rechaza acá.
    pub async fn send_message(&self, agent: AgentId, text: &str) -> Result<(), AgentOpError> {
        let (adapter, project, run) = self.live_parts(agent)?;
        let bytes = adapter.encode_user_message(text).ok_or_else(|| {
            AgentOpError(format!(
                "{} solo recibe mensajes entre turnos",
                adapter.display_name()
            ))
        })?;
        self.control(agent, |done| Control::Send { bytes, done })
            .await
            .ok_or_else(|| AgentOpError("el executor terminó antes de recibir el mensaje".into()))?
            .map_err(AgentOpError)?;
        let ev = BusEvent {
            project_id: project.to_string(),
            agent_id: Some(agent.to_string()),
            run_id: Some(run.to_string()),
            source: EventSource::User,
            event: AgentEvent::UserMessage {
                text: text.to_string(),
            },
            occurred_at: now_ms(),
        };
        self.bus.publish(ev).await.map_err(op_err)
    }

    /// Política por umbral (P07.5.S6, apagada por defecto): si el último turno del chat
    /// usó tanto contexto como el umbral y hay otro executor elegible, el próximo mensaje
    /// va a ese otro con handoff en vez de reanudar la sesión del CLI.
    async fn rotation_target(
        &self,
        agent: &repo::Agent,
        last: &repo::AgentRun,
    ) -> Result<Option<(repo::EligibleModel, Option<crate::routing::Pending>)>, AgentOpError> {
        let limit = self
            .chat_switch_tokens
            .load(std::sync::atomic::Ordering::Relaxed);
        if limit == 0 {
            return Ok(None);
        }
        let due = self.read_op(|c| {
            Ok(
                repo::get_task(c, agent.task_id)?.code == repo::CHAT_TASK_CODE
                    && repo::last_turn_tokens(c, last.id)? >= limit,
            )
        })?;
        if !due {
            return Ok(None);
        }
        // Armar el handoff cuesta (git): solo cuando de verdad toca rotar (CONSTRAINTS A11).
        let needed = self.handoff_tokens(agent.id).await;
        self.read_op(|c| {
            let why = Replace {
                provider: &last.provider_id,
                model: &last.model_id,
                auth: false,
                rotate: true,
                needed_context: needed,
            };
            let (next, pending) = self.pick_replacement(c, agent, why)?;
            Ok(next.map(|n| (n, pending)))
        })
    }

    /// Tamaño (tokens) del handoff que recibiría el modelo nuevo, para que el router descarte a
    /// quien no lo cabe. `None` si no se puede armar (sin checkpoint o sin worktree): se elige
    /// como antes, sin ese dato.
    async fn handoff_tokens(&self, agent: AgentId) -> Option<u64> {
        // Un agente sin runs (recién liberado de una dependencia) arranca con su objetivo, no con
        // un handoff: no hay nada que medir y armarlo cuesta git.
        let has_run = self
            .read_op(|c| Ok(!repo::runs_of(c, agent)?.is_empty()))
            .ok()?;
        if !has_run {
            return None;
        }
        let h = self.prepare_handoff(agent, "estimate").await.ok()?;
        u64::try_from(h.tokens_sent).ok()
    }

    /// Mensaje después de terminado el turno (ADR-0005, adenda; P07.5.S1): retoma la
    /// sesión del CLI con `resume_spec`, con el mismo modelo y sin handoff, porque la
    /// conversación sigue en el CLI. Un agente `COMPLETED` se reabre.
    pub async fn continue_session(
        &self,
        agent_id: AgentId,
        text: &str,
    ) -> Result<RunId, AgentOpError> {
        if self.is_live(agent_id) {
            return Err(AgentOpError(
                "el agente está trabajando: su executor recibe el mensaje directamente".into(),
            ));
        }
        let (agent, last) = self.read_op(|c| {
            let agent = repo::get_agent(c, agent_id)?;
            let last = repo::runs_of(c, agent_id)?
                .into_iter()
                .rev()
                .find(|r| r.cli_session_id.is_some());
            Ok((agent, last))
        })?;
        if !matches!(agent.state, AgentState::Ready | AgentState::Completed) {
            let hint = match agent.state {
                AgentState::Failed => "; recupéralo con restart o reclaim",
                AgentState::Paused => "; reanúdalo primero",
                _ => "",
            };
            return Err(AgentOpError(format!(
                "el agente #{} está {} y no puede recibir mensajes ahora{hint}",
                agent.number,
                agent.state.as_str()
            )));
        }
        let (last, session) = last
            .and_then(|r| {
                let session = r.cli_session_id.clone()?;
                Some((r, session))
            })
            .ok_or_else(|| {
                AgentOpError(format!(
                    "el agente #{} todavía no tiene una sesión que continuar: asígnale un modelo primero",
                    agent.number
                ))
            })?;
        if let Some((next, route)) = self.rotation_target(&agent, &last).await? {
            let reason = format!(
                "El contexto de {} llegó al umbral configurado de tokens.",
                last.model_id
            );
            return self
                .start_successor(
                    &agent,
                    Some(last.id),
                    next,
                    &reason,
                    "FAILOVER",
                    None,
                    Some(text),
                    route,
                    None,
                )
                .await;
        }
        let model = self
            .read_op(|c| repo::eligible_model(c, &last.model_id))?
            .map_err(|why| {
                AgentOpError(format!(
                    "no se puede continuar con el modelo anterior: {why}"
                ))
            })?;
        let adapter = self
            .adapter(&model.provider_id)
            .ok_or_else(|| AgentOpError(format!("no hay adapter para `{}`", model.provider_id)))?;
        let worktree = self
            .read_op(|c| {
                let wt = agent.worktree_id.ok_or_else(|| repo::RepoError::NotFound {
                    entity: "worktree",
                    id: agent.id.to_string(),
                })?;
                repo::get_worktree(c, wt)
            })?
            .path;

        let run = RunId::new();
        let (task_id, state) = (agent.task_id, agent.state);
        let (provider, model_id) = (model.provider_id.clone(), model.model_id.clone());
        self.writer
            .write(Box::new(move |t| {
                let now = now_ms();
                if state == AgentState::Completed {
                    repo::set_agent_state(t, agent_id, AgentState::Ready, None, now)?;
                }
                repo::set_agent_state(t, agent_id, AgentState::Running, None, now)?;
                let task = repo::get_task(t, task_id)?.status;
                if task == TaskStatus::Done {
                    repo::set_task_status(t, task_id, TaskStatus::Ready, None, now)?;
                }
                if task != TaskStatus::Running {
                    repo::set_task_status(t, task_id, TaskStatus::Running, None, now)?;
                }
                repo::open_run(t, run, agent_id, &provider, &model_id, now)?;
                Ok(())
            }))
            .await
            .map_err(op_err)?;
        let launch = Launch {
            adapter,
            project_id: agent.project_id,
            agent_id,
            task_id,
            run_id: run,
            provider_id: model.provider_id,
            model_id: model.model_id,
            worktree: PathBuf::from(worktree),
            cli_model: model.cli_model_id,
            prompt: text.to_string(),
            resume_session: Some(session),
        };
        // Si no arranca, `launch` ya dejó al agente `FAILED` con razón y recovery item.
        self.launch(launch).await.map_err(AgentOpError)?;
        Ok(run)
    }

    /// Reabre un agente `FAILED` con un run nuevo desde su último checkpoint,
    /// con el mismo proveedor/modelo del run anterior (FLOW §16, IDEA §5.10).
    /// `change`/`resolution` son `RESTART` (crash) o `RECLAIM` (sin latido).
    async fn recover(
        &self,
        agent_id: AgentId,
        change: &'static str,
        resolution: &str,
        reason: &str,
    ) -> Result<RunId, AgentOpError> {
        if self.is_live(agent_id) {
            let number = self
                .read_op(|c| repo::get_agent(c, agent_id))
                .map_or_else(|_| "?".into(), |a| a.number.to_string());
            return Err(AgentOpError(format!(
                "el agente #{number} todavía tiene un executor vivo; detenlo antes de recuperarlo"
            )));
        }
        let agent = self.read_op(|c| repo::get_agent(c, agent_id))?;
        if agent.state != AgentState::Failed {
            return Err(AgentOpError(format!(
                "el agente #{} está {} y no necesita recuperación",
                agent.number,
                agent.state.as_str()
            )));
        }
        let last = self
            .read_op(|c| repo::runs_of(c, agent_id))?
            .pop()
            .ok_or_else(|| AgentOpError("el agente no tiene ningún run".into()))?;
        let model = self
            .read_op(|c| repo::eligible_model(c, &last.model_id))?
            .map_err(|why| {
                AgentOpError(format!("no se puede reabrir con el modelo anterior: {why}"))
            })?;
        if self.adapter(&model.provider_id).is_none() {
            return Err(AgentOpError(format!(
                "esta versión no tiene adapter para `{}`",
                model.provider_id
            )));
        }
        let run = self
            .start_successor(
                &agent,
                Some(last.id),
                model,
                reason,
                change,
                None,
                None,
                None,
                None,
            )
            .await?;
        let resolution = resolution.to_string();
        self.writer
            .write(Box::new(move |t| {
                repo::resolve_recovery_items(t, agent_id, &resolution, now_ms())?;
                Ok(())
            }))
            .await
            .map_err(op_err)?;
        Ok(run)
    }

    /// Reinicia un agente que falló (`CRASH` o similar) desde su checkpoint.
    pub async fn restart(&self, agent_id: AgentId) -> Result<RunId, AgentOpError> {
        self.recover(
            agent_id,
            "RESTART",
            "RESTART",
            "El usuario reinició el agente desde su último checkpoint.",
        )
        .await
    }

    /// Recupera un agente cuyo executor perdió el latido (`NO_HEARTBEAT`).
    /// El workspace no se toca: el handoff sale del checkpoint + git vivo.
    pub async fn reclaim(&self, agent_id: AgentId) -> Result<RunId, AgentOpError> {
        self.recover(
            agent_id,
            "RECLAIM",
            "RECLAIM",
            "Se recuperó el agente tras perder el latido del executor.",
        )
        .await
    }

    /// Suspende el árbol de procesos del executor (FLOW §7: `pause`). El agente queda `PAUSED`.
    pub async fn pause(&self, agent: AgentId) -> Result<(), AgentOpError> {
        self.live_parts(agent)?;
        let from = self.read_op(|c| repo::get_agent(c, agent))?.state;
        from.transition(AgentState::Paused).map_err(op_err)?;
        self.control(agent, Control::Suspend)
            .await
            .ok_or_else(|| AgentOpError("el executor terminó antes de pausarse".into()))?
            .map_err(AgentOpError)?;
        self.set_state(agent, AgentState::Paused).await
    }

    /// Reanuda un agente: un executor pausado vuelve a `RUNNING`; un agente `FAILED` (sin latido,
    /// con un límite de proveedor o con un crash) se reabre desde su último checkpoint con el mismo
    /// proveedor y modelo, sin pasar por `switch` (F6 de `docs/research/uso-signalia.md`).
    pub async fn resume(&self, agent: AgentId) -> Result<(), AgentOpError> {
        let from = self.read_op(|c| repo::get_agent(c, agent))?.state;
        if from == AgentState::Failed {
            let last = self
                .read_op(|c| repo::runs_of(c, agent))?
                .pop()
                .ok_or_else(|| AgentOpError("el agente no tiene ningún run".into()))?;
            // Sin latido (o un límite que lo causó): el workspace puede tener cambios sin
            // checkpoint; el resto se reinicia desde el último checkpoint.
            let silent = last.end_reason.is_some_and(|e| {
                matches!(
                    e,
                    RunEndReason::NoHeartbeat
                        | RunEndReason::RateLimited
                        | RunEndReason::QuotaExhausted
                        | RunEndReason::AuthError
                )
            });
            if silent {
                self.reclaim(agent).await?;
            } else {
                self.restart(agent).await?;
            }
            return Ok(());
        }
        self.live_parts(agent)?;
        if from != AgentState::Paused {
            return Err(AgentOpError(format!(
                "el agente no está pausado ({})",
                from.as_str()
            )));
        }
        self.control(agent, Control::Resume)
            .await
            .ok_or_else(|| AgentOpError("el executor ya no está vivo".into()))?
            .map_err(AgentOpError)?;
        self.set_state(agent, AgentState::Running).await
    }

    fn live_parts(
        &self,
        agent: AgentId,
    ) -> Result<(Arc<dyn ProviderAdapter>, ProjectId, RunId), AgentOpError> {
        self.live
            .lock()
            .ok()
            .and_then(|l| {
                l.get(&agent)
                    .map(|r| (r.adapter.clone(), r.project_id, r.run_id))
            })
            .ok_or_else(|| AgentOpError("el agente no tiene un executor corriendo".into()))
    }

    async fn set_state(&self, agent: AgentId, to: AgentState) -> Result<(), AgentOpError> {
        self.writer
            .write(Box::new(move |t| {
                repo::set_agent_state(t, agent, to, None, now_ms())?;
                Ok(())
            }))
            .await
            .map_err(op_err)
    }

    /// Detiene el agente y su executor (FLOW §7). El agente pasa a `CANCELLED`.
    pub async fn stop(&self, agent_id: AgentId) -> Result<(), AgentOpError> {
        let agent = self.read_op(|c| repo::get_agent(c, agent_id))?;
        if agent.state.is_terminal() {
            return Ok(());
        }
        if self.is_live(agent_id) {
            self.control(agent_id, |done| Control::Stop {
                status: RunStatus::Killed,
                end: RunEndReason::UserStop,
                done,
            })
            .await;
        }
        let task_id = agent.task_id;
        self.writer
            .write(Box::new(move |t| {
                let now = now_ms();
                if let Ok(Some(run)) = repo::current_run(t, agent_id) {
                    let _ = repo::close_run(
                        t,
                        run.id,
                        RunStatus::Killed,
                        RunEndReason::UserStop,
                        None,
                        now,
                    );
                }
                repo::set_agent_state(t, agent_id, AgentState::Cancelled, None, now)?;
                repo::set_task_status(t, task_id, TaskStatus::Cancelled, None, now)?;
                Ok(())
            }))
            .await
            .map_err(op_err)?;
        self.reclaim_disk(false).await;
        Ok(())
    }

    /// Alias de `stop`: no hay una parada más fuerte (`stop` ya mata el árbol completo) (FLOW §7).
    pub async fn kill(&self, agent_id: AgentId) -> Result<(), AgentOpError> {
        self.stop(agent_id).await
    }

    /// Resuelve un agente por ID o número (`1`, `#1`, `agent-1`).
    pub fn find_agent(
        &self,
        ident: &str,
        project_id: Option<ProjectId>,
    ) -> Result<repo::Agent, AgentOpError> {
        self.read_op(|c| repo::find_agent_by_ident(c, project_id, ident))
    }

    /// Obtiene el diff del worktree del agente contra su commit base (FLOW §7, Changes).
    pub async fn diff(&self, agent_id: AgentId) -> Result<String, AgentOpError> {
        let agent = self.read_op(|c| repo::get_agent(c, agent_id))?;
        let wt_id = agent
            .worktree_id
            .ok_or_else(|| AgentOpError("el agente no tiene worktree".into()))?;
        let wt = self.read_op(|c| repo::get_worktree(c, wt_id))?;
        let wt_path = PathBuf::from(&wt.path);
        let base_ref = wt.base_ref.clone();
        let live_diff = tokio::task::spawn_blocking(move || {
            if wt_path.exists() {
                symphony_git::Repo::at(&wt_path).diff(&base_ref).ok()
            } else {
                None
            }
        })
        .await
        .map_err(op_err)?;
        if let Some(d) = live_diff {
            return Ok(d);
        }
        let cp = self.read_op(|c| repo::latest_checkpoint(c, agent_id))?;
        if let Some(cp) = cp
            && let Some(diff_uri) = cp.diff_uri
            && let Some(hash) = diff_uri.split('/').next_back()
        {
            let objects = symphony_object_store::ObjectStore::new(
                symphony_core::SymphonyHome::at(&self.home).objects_dir(),
            );
            if let Ok(bytes) = objects.get(hash) {
                return Ok(String::from_utf8_lossy(&bytes).into_owned());
            }
        }
        Ok(String::new())
    }

    /// Obtiene el historial de mensajes de la conversación del agente (FLOW §7, Conversation).
    pub async fn logs(
        &self,
        agent_id: AgentId,
        limit: Option<usize>,
    ) -> Result<Vec<(String, String, i64)>, AgentOpError> {
        let records = self.read_op(|c| repo::agent_messages(c, agent_id, limit))?;
        let objects = symphony_object_store::ObjectStore::new(
            symphony_core::SymphonyHome::at(&self.home).objects_dir(),
        );
        let conn = self
            .reader
            .lock()
            .map_err(|_| AgentOpError("lector de la base no disponible".into()))?;
        Ok(records
            .into_iter()
            .map(|m| {
                let content = crate::handoff::message_text(&conn, &objects, &m);
                (m.role, content, m.created_at)
            })
            .collect())
    }

    /// Información detallada del agente para inspección (FLOW §7, Overview / History).
    pub async fn inspect(&self, agent_id: AgentId) -> Result<serde_json::Value, AgentOpError> {
        let (agent, task, worktree, active_run, latest_cp, runs) = self.read_op(|c| {
            let a = repo::get_agent(c, agent_id)?;
            let t = repo::get_task(c, a.task_id)?;
            let w = a
                .worktree_id
                .map(|wt| repo::get_worktree(c, wt))
                .transpose()?;
            let ar = repo::current_run(c, agent_id)?;
            let cp = repo::latest_checkpoint(c, agent_id)?;
            let rs = repo::runs_of(c, agent_id)?;
            Ok((a, t, w, ar, cp, rs))
        })?;
        let is_live = self.is_live(agent_id);
        let suggestion = self.pending_suggestion_json(agent_id);
        Ok(serde_json::json!({
            "suggestion": suggestion,
            "agent": {
                "id": agent.id.to_string(),
                "number": agent.number,
                "project_id": agent.project_id.to_string(),
                "session_id": agent.session_id.to_string(),
                "state": agent.state.as_str(),
                "state_reason": agent.state_reason,
                "execution_mode": agent.execution_mode.as_str(),
                "requested_model_id": agent.requested_model_id,
                "requested_profile_id": agent.requested_profile_id,
                "failover_policy": agent.failover_policy.as_str(),
                "context_mode": agent.context_mode.as_str(),
                "priority": agent.priority,
                "is_live": is_live,
            },
            "task": {
                "id": task.id.to_string(),
                "code": task.code,
                "title": task.title,
                "description": task.description,
                "status": task.status.as_str(),
                "status_reason": task.status_reason,
                "priority": task.priority,
            },
            "worktree": worktree.map(|w| serde_json::json!({
                "id": w.id.to_string(),
                "path": w.path,
                "branch": w.branch,
                "base_ref": w.base_ref,
                "deps_strategy": w.deps_strategy,
                "status": w.status,
            })),
            "current_run": active_run.map(|r| serde_json::json!({
                "id": r.id.to_string(),
                "seq": r.seq,
                "provider_id": r.provider_id,
                "model_id": r.model_id,
                "cli_session_id": r.cli_session_id,
                "pid": r.pid,
                "status": r.status.as_str(),
                "started_at": r.started_at,
            })),
            "latest_checkpoint": latest_cp.map(|cp| serde_json::json!({
                "id": cp.id.to_string(),
                "seq": cp.seq,
                "created_at": cp.created_at,
                "objective": cp.objective,
                "plan_tail": cp.plan_tail,
                "current_step": cp.current_step,
                "next_step": cp.next_step,
                "summary_json": cp.summary_json,
                "diff_uri": cp.diff_uri,
            })),
            "runs_count": runs.len(),
            "runs": runs.into_iter().map(|r| serde_json::json!({
                "id": r.id.to_string(),
                "seq": r.seq,
                "provider_id": r.provider_id,
                "model_id": r.model_id,
                "status": r.status.as_str(),
                "end_reason": r.end_reason.map(|e| e.as_str()),
                "started_at": r.started_at,
                "ended_at": r.ended_at,
            })).collect::<Vec<_>>(),
        }))
    }
}

#[cfg(test)]
mod skill_tests {
    use super::with_user_message;

    #[test]
    fn a_skill_invocation_moves_to_the_front_for_its_own_provider() {
        let p = with_user_message("HANDOFF", "/flintstone explica un worktree", "anthropic");
        assert!(
            p.starts_with("/flintstone explica un worktree\n\nHANDOFF"),
            "{p}"
        );
        // El mensaje sigue también al final, para quien no lea la invocación.
        assert!(p.ends_with("respóndelo primero)\n/flintstone explica un worktree"));
        let p = with_user_message("HANDOFF", "$deslop este archivo", "openai");
        assert!(p.starts_with("$deslop este archivo\n\nHANDOFF"), "{p}");
    }

    #[test]
    fn other_messages_and_other_providers_are_left_at_the_end() {
        let plain = "HANDOFF\n\n## Mensaje nuevo del usuario (respóndelo primero)\nhola";
        assert_eq!(with_user_message("HANDOFF", "hola", "anthropic"), plain);
        // `/x` para Codex y `$x` para Claude no son skills de ese CLI: no se tocan.
        assert!(with_user_message("H", "/flintstone x", "openai").starts_with('H'));
        assert!(with_user_message("H", "$flintstone x", "anthropic").starts_with('H'));
        // Una barra sola no es una invocación.
        assert!(with_user_message("H", "/", "anthropic").starts_with('H'));
        // Una skill sin argumentos también sube.
        assert!(
            with_user_message("H", "/flintstone", "anthropic").starts_with("/flintstone \n\nH")
        );
    }
}
