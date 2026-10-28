//! Runtime de agentes (P06): crea agentes (FLOW §6) y lanza su executor.
//!
//! Orden de creación: primero el worktree (git, fuera de la base) y después una
//! sola transacción con project, session, task, worktree, agente, checkpoint
//! inicial y run. Si algo falla antes de confirmar, se deshace el worktree: no
//! queda un agente "medio roto". Un fallo al lanzar el CLI sí deja al agente,
//! en `FAILED` con su razón y un item en el Recovery Center (su workspace sirve).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use symphony_adapter_common::{HookCommand, ProviderAdapter};
use symphony_core::{
    AgentId, AgentState, CheckpointId, ContextMode, ExecutionMode, FailoverPolicy, MilestoneId,
    ProjectId, RunId, SessionId, TaskId, TaskStatus, WorktreeId,
};
use symphony_git::{GitError, Repo, deps};
use symphony_store::{WriterHandle, repo};
use tokio_util::task::TaskTracker;

use crate::bus::EventBus;
use crate::executor::{Control, Launch};

/// Cómo se elige el executor (FLOW §6).
#[derive(Debug, Clone, PartialEq)]
pub enum Execution {
    /// Modelo canónico exacto (`claude/sonnet`).
    Exact(String),
    /// Profile (`@code`); el routing llega en P10.
    Profile(String),
    DecideLater,
}

#[derive(Debug, Clone)]
pub struct CreateAgent {
    pub project_root: PathBuf,
    pub title: String,
    pub description: Option<String>,
    pub execution: Execution,
    pub failover: FailoverPolicy,
    pub context_mode: ContextMode,
    pub priority: i64,
    /// Chat general del proyecto: idempotente, rama `symphony/chat` sobre la principal.
    pub chat: bool,
    /// Tareas de las que depende (códigos `T-12` o ids). Mientras alguna no esté `DONE`, el
    /// agente queda en `WAITING_DEPENDENCY` y no arranca (FLOW §9.3).
    pub depends_on: Vec<String>,
    /// Es la tarea de un review agent (`kind = REVIEW`): no planifica por fases.
    pub review: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Created {
    pub agent_id: AgentId,
    pub task_id: TaskId,
    pub number: i64,
    pub task_code: String,
    pub worktree: PathBuf,
    pub branch: String,
    pub state: AgentState,
    pub state_reason: Option<String>,
    /// Run abierto y su modelo, si arrancó un executor.
    pub run: Option<(RunId, String)>,
}

/// Errores de FLOW §6. Los que vienen de la tarea la devuelven para no perderla.
#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    #[error("la tarea está vacía")]
    EmptyTask,
    #[error("{} no es un repositorio git: {source}", path.display())]
    NotARepo {
        path: PathBuf,
        #[source]
        source: GitError,
    },
    #[error(
        "el profile `{profile}` no existe. Disponibles: @code, @debug, @fast, @reasoning, @docs, @review, @conserve, @write, @media, @research"
    )]
    UnknownProfile { profile: String, task: String },
    #[error("ningún modelo es elegible para {profile} ahora.\n{explanation}")]
    NoEligibleModel {
        profile: String,
        explanation: String,
        task: String,
    },
    #[error(
        "no hay ningún proveedor listo; configura uno en Provider Setup (la tarea no se perdió)"
    )]
    NoEligibleProvider { task: String },
    #[error(
        "el modelo exacto no está disponible: {reason}. Opciones: esperar, escoger otro modelo, usar un profile o cancelar"
    )]
    ExactModelUnavailable {
        model: String,
        reason: String,
        task: String,
    },
    #[error(
        "no se pudo preparar el workspace: {detail}. Puedes reintentar o cancelar; no se creó el agente"
    )]
    Workspace { detail: String, task: String },
    #[error("la tarea `{reference}` de la que depende no existe en este proyecto")]
    UnknownDependency { reference: String, task: String },
    #[error("base de datos: {0}")]
    Store(String),
}

impl CreateError {
    /// Código estable para el protocolo IPC.
    pub fn code(&self) -> &'static str {
        match self {
            Self::EmptyTask => "empty_task",
            Self::NotARepo { .. } => "not_a_repo",
            Self::UnknownProfile { .. } => "unknown_profile",
            Self::NoEligibleModel { .. } => "no_eligible_model",
            Self::NoEligibleProvider { .. } => "no_eligible_provider",
            Self::ExactModelUnavailable { .. } => "exact_model_unavailable",
            Self::Workspace { .. } => "workspace_failed",
            Self::UnknownDependency { .. } => "unknown_dependency",
            Self::Store(_) => "store_error",
        }
    }
}

fn store_err(e: impl std::fmt::Display) -> CreateError {
    CreateError::Store(e.to_string())
}

/// Runtime de agentes. Clonarlo es barato: comparte el estado.
#[derive(Clone)]
pub struct Runtime {
    pub(crate) inner: Arc<Inner>,
}

impl std::ops::Deref for Runtime {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.inner
    }
}

pub struct Inner {
    pub(crate) home: PathBuf,
    pub(crate) writer: WriterHandle,
    pub(crate) reader: Mutex<rusqlite::Connection>,
    pub(crate) bus: EventBus,
    pub(crate) adapters: Vec<Arc<dyn ProviderAdapter>>,
    /// Comando de hook que se inyecta en cada CLI (`symphony hook emit`).
    pub(crate) hook: Option<HookCommand>,
    /// Una creación a la vez: número de agente y ruta del worktree no chocan.
    creating: tokio::sync::Mutex<()>,
    /// Tareas que leen la salida de cada executor.
    pub(crate) pumps: TaskTracker,
    /// Executors vivos: canal de control de cada uno.
    pub(crate) live: Mutex<HashMap<AgentId, crate::executor::LiveRun>>,
    /// Watchdog (P06.S6): cada cuánto se persiste el latido y cuánto
    /// silencio se tolera antes de declarar el run `NO_HEARTBEAT`.
    pub(crate) heartbeat_every: Duration,
    pub(crate) stale_after: Duration,
    /// Última actividad (línea de salida) de cada run vivo, en memoria.
    pub(crate) activity: Mutex<HashMap<RunId, Instant>>,
    /// Una integración a la rama del chat a la vez (P08.S7).
    pub(crate) integration: tokio::sync::Mutex<()>,
    /// Cola de operaciones pesadas (la pone `serve`; los tests sin daemon no la tienen).
    pub(crate) scheduling: std::sync::OnceLock<Arc<crate::scheduling::Scheduling>>,
    /// Umbral de tokens para rotar el proveedor del chat (P07.5.S6). 0 = apagado.
    pub(crate) chat_switch_tokens: AtomicU64,
    /// Profile con el que se evalúa un failover cuando el agente no tiene uno (`routing.default_profile`).
    pub(crate) default_profile: Mutex<String>,
    /// File watchers, uno por proyecto (P09.S5).
    pub(crate) watchers: crate::watcher::Watchers,
    /// Lotes vivos (P11.5.S5): milestone → tope de simultáneos pedido. En memoria: tras reiniciar
    /// el daemon, un lote sigue con las dependencias que ya tiene, sin el ajuste por cuota.
    pub(crate) batches: Mutex<HashMap<MilestoneId, usize>>,
    /// Una sola pasada de `fill_batches` a la vez.
    pub(crate) batch_fill: tokio::sync::Mutex<()>,
}

/// Persistencia del latido por defecto (producción).
pub const DEFAULT_HEARTBEAT_EVERY: Duration = Duration::from_secs(5);
/// Silencio tolerado por defecto antes de un `NO_HEARTBEAT` (producción).
/// Los CLIs pueden estar callados entre turnos; P10 afinará esto por agente.
pub const DEFAULT_STALE_AFTER: Duration = Duration::from_secs(900);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchdogTiming {
    pub heartbeat_every: Duration,
    pub stale_after: Duration,
}

impl Default for WatchdogTiming {
    fn default() -> Self {
        Self {
            heartbeat_every: DEFAULT_HEARTBEAT_EVERY,
            stale_after: DEFAULT_STALE_AFTER,
        }
    }
}

impl Runtime {
    pub fn new(
        home: &Path,
        writer: WriterHandle,
        reader: rusqlite::Connection,
        bus: EventBus,
        adapters: Vec<Arc<dyn ProviderAdapter>>,
        hook: Option<HookCommand>,
    ) -> Self {
        Self::new_with_watchdog(
            home,
            writer,
            reader,
            bus,
            adapters,
            hook,
            WatchdogTiming::default(),
        )
    }

    /// Como `new`, con el ritmo del watchdog explícito (tests de P06.S6).
    pub fn new_with_watchdog(
        home: &Path,
        writer: WriterHandle,
        reader: rusqlite::Connection,
        bus: EventBus,
        adapters: Vec<Arc<dyn ProviderAdapter>>,
        hook: Option<HookCommand>,
        watchdog: WatchdogTiming,
    ) -> Self {
        let rt = Self {
            inner: Arc::new(Inner {
                home: home.to_path_buf(),
                writer,
                reader: Mutex::new(reader),
                bus,
                adapters,
                hook,
                creating: tokio::sync::Mutex::new(()),
                pumps: TaskTracker::new(),
                live: Mutex::default(),
                heartbeat_every: watchdog.heartbeat_every,
                stale_after: watchdog.stale_after,
                activity: Mutex::default(),
                chat_switch_tokens: AtomicU64::new(0),
                default_profile: Mutex::new("@code".into()),
                watchers: crate::watcher::Watchers::default(),
                batches: Mutex::default(),
                batch_fill: tokio::sync::Mutex::new(()),
                scheduling: std::sync::OnceLock::new(),
                integration: tokio::sync::Mutex::new(()),
            }),
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(Runtime::watchdog(Arc::downgrade(&rt.inner)));
        }
        rt
    }

    /// El scheduler compartido con el daemon (validaciones y preparación de worktrees).
    pub fn set_scheduling(&self, scheduling: Arc<crate::scheduling::Scheduling>) {
        let _ = self.scheduling.set(scheduling);
    }

    /// Comandos de `[worktree] prepare` de `project.toml` (F8): corren en el worktree nuevo,
    /// uno tras otro y por el scheduler, antes de lanzar al agente.
    async fn prepare_worktree_commands(
        &self,
        project_root: &str,
        wt_path: &Path,
        agent: AgentId,
        number: i64,
    ) -> Result<(), String> {
        let Some(cfg) = symphony_core::load_project(Path::new(project_root))
            .ok()
            .flatten()
            .and_then(|p| p.worktree)
            .filter(|w| !w.prepare.is_empty())
        else {
            return Ok(());
        };
        let timeout = Duration::from_secs(cfg.prepare_timeout_secs.unwrap_or(1_800).max(1));
        for (i, command) in cfg.prepare.iter().enumerate() {
            let key = format!("prep:{}:{i}", wt_path.display());
            let (exec, _, _) = self
                .run_scheduled(
                    key,
                    agent,
                    format!("Agent #{number}"),
                    command,
                    wt_path,
                    timeout,
                    symphony_core::ToolCallId::new(),
                )
                .await
                .map_err(|e| format!("`{command}` no pudo correr: {e}"))?;
            if !exec.success {
                let tail: Vec<&str> = exec.output.lines().rev().take(5).collect();
                let tail: Vec<&str> = tail.into_iter().rev().collect();
                return Err(format!(
                    "la preparación del worktree falló en `{command}` ({}): {}",
                    if exec.timed_out {
                        "tiempo agotado".to_string()
                    } else {
                        format!(
                            "código {}",
                            exec.exit_code.map_or("?".into(), |c| c.to_string())
                        )
                    },
                    tail.join(" | ")
                ));
            }
        }
        Ok(())
    }

    /// Vigila los archivos del proyecto (idempotente; ver `watcher.rs`).
    pub fn watch_project(&self, project_id: &str, root: &Path) {
        let objects = symphony_object_store::ObjectStore::new(
            symphony_core::SymphonyHome::at(&self.home).objects_dir(),
        );
        self.watchers
            .ensure(project_id, root, self.writer.clone(), objects);
    }

    /// Al arrancar el daemon: vigila los proyectos que tienen agentes sin terminar.
    pub fn watch_active_projects(&self) {
        let projects: Vec<(String, String)> = self
            .reader
            .lock()
            .ok()
            .and_then(|c| {
                c.prepare(
                    "SELECT DISTINCT p.id, p.root_path FROM projects p JOIN agents a ON a.project_id = p.id
                     WHERE a.state NOT IN ('COMPLETED','FAILED','CANCELLED')",
                )
                .and_then(|mut s| {
                    s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                        .collect::<Result<Vec<_>, _>>()
                })
                .ok()
            })
            .unwrap_or_default();
        for (id, root) in projects {
            let root = PathBuf::from(root);
            if root.is_dir() {
                self.watch_project(&id, &root);
            }
        }
    }

    /// El profile por defecto (`routing.default_profile` de `config.toml`).
    pub fn set_default_profile(&self, profile: &str) {
        if let Ok(mut p) = self.default_profile.lock() {
            *p = profile.to_string();
        }
    }

    pub(crate) fn default_profile(&self) -> String {
        self.default_profile
            .lock()
            .map(|p| p.clone())
            .unwrap_or_else(|_| "@code".into())
    }

    /// Evalúa a todos los modelos con el router (sin guardar nada).
    pub(crate) fn evaluate(
        &self,
        conn: &rusqlite::Connection,
        choose: &crate::routing::Choose,
    ) -> Result<symphony_router::Decision, repo::RepoError> {
        let cfg = self.bus.health_config();
        let has_adapter = |p: &str| self.adapter(p).is_some();
        crate::routing::choose(conn, &has_adapter, &cfg, now_ms(), choose)
    }

    /// Activa (`Some`) o apaga (`None`) el cambio de proveedor del chat por uso de contexto.
    pub fn set_chat_switch_tokens(&self, tokens: Option<u64>) {
        self.chat_switch_tokens
            .store(tokens.unwrap_or(0), std::sync::atomic::Ordering::Relaxed);
    }

    /// Marca actividad de un run vivo (lo llama el pump por cada línea).
    pub(crate) fn beat(&self, run: RunId) {
        if let Ok(mut activity) = self.activity.lock() {
            activity.insert(run, Instant::now());
        }
    }

    /// Persiste el latido de los runs vivos y cierra como `NO_HEARTBEAT` los
    /// que llevan más de `stale_after` sin emitir nada (FLOW §16, IDEA §5.10).
    /// Débil: un clon fuerte mantendría vivo el runtime (y su base) para siempre.
    async fn watchdog(weak: std::sync::Weak<Inner>) {
        let Some(every) = weak.upgrade().map(|i| i.heartbeat_every) else {
            return;
        };
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let Some(inner) = weak.upgrade() else { return };
            let this = Runtime { inner };
            let live: Vec<(AgentId, RunId)> = this
                .live
                .lock()
                .map(|l| l.iter().map(|(a, r)| (*a, r.run_id)).collect())
                .unwrap_or_default();
            for (agent, run) in live {
                this.watchdog_step(agent, run).await;
            }
        }
    }

    /// Evalúa un `(agent, run)` observado por el watchdog: persiste su latido y, si superó
    /// `stale_after`, envía `NoHeartbeat` condicionado a que `agent` siga en ese `run`.
    pub(crate) async fn watchdog_step(&self, agent: AgentId, run: RunId) {
        let silent = self
            .activity
            .lock()
            .ok()
            .and_then(|a| a.get(&run).copied())
            .is_none_or(|t| t.elapsed() > self.stale_after);
        let now = now_ms();
        let _ = self
            .writer
            .write(Box::new(move |t| {
                repo::heartbeat(t, run, now)?;
                Ok(())
            }))
            .await;
        if silent
            && self
                .control_if(agent, Some(run), |done| Control::NoHeartbeat { done })
                .await
                .is_some()
        {
            tracing::warn!(
                agent = %agent,
                run = %run,
                "executor sin latido: se cierra como NO_HEARTBEAT y se abre recuperación"
            );
        }
    }

    fn read<T>(
        &self,
        f: impl FnOnce(&rusqlite::Connection) -> Result<T, repo::RepoError>,
    ) -> Result<T, CreateError> {
        let conn = self
            .reader
            .lock()
            .map_err(|_| CreateError::Store("lector de la base no disponible".into()))?;
        f(&conn).map_err(store_err)
    }

    pub(crate) fn adapter(&self, provider_id: &str) -> Option<Arc<dyn ProviderAdapter>> {
        self.adapters
            .iter()
            .find(|a| a.provider_id() == provider_id)
            .cloned()
    }

    /// Arma el prompt para que otro executor continúe al agente (P06.S4).
    pub async fn prepare_handoff(
        &self,
        agent: AgentId,
        reason: &str,
    ) -> Result<crate::handoff::PreparedHandoff, String> {
        let objects = symphony_object_store::ObjectStore::new(
            symphony_core::SymphonyHome::at(&self.home).objects_dir(),
        );
        let mut prepared =
            crate::handoff::prepare(&self.reader, &objects, &self.writer, agent, reason).await?;
        // Context transformers de plugin (ADR-0015): un fallo nunca bloquea el handoff.
        prepared.prompt = self.transform_handoff(agent, prepared.prompt).await;
        Ok(prepared)
    }

    /// Espera a que terminen los executors lanzados y sus checkpoints (apagado y tests).
    pub async fn wait_executors(&self) {
        self.pumps.close();
        self.pumps.wait().await;
        self.pumps.reopen();
        self.bus.checkpoints_idle().await;
    }

    /// FLOW §6: task + agente + worktree + checkpoint inicial → run con el modelo exacto.
    pub async fn create_agent(&self, req: CreateAgent) -> Result<Created, CreateError> {
        let title = req.title.trim().to_string();
        if title.is_empty() {
            return Err(CreateError::EmptyTask);
        }
        let task_text = title.clone();

        // 1. Executor elegible (antes de tocar nada).
        let mut pending: Option<crate::routing::Pending> = None;
        let executor = match &req.execution {
            Execution::Profile(profile) => {
                // FLOW §8.3: filtrar a los no utilizables, comparar, elegir y poder explicarlo.
                let known = self.read(|c| routing_profile_exists(c, profile))?;
                if !known {
                    return Err(CreateError::UnknownProfile {
                        profile: profile.clone(),
                        task: task_text,
                    });
                }
                let choose = crate::routing::Choose {
                    profile: profile.clone(),
                    ..Default::default()
                };
                let decision = self.read(|c| self.evaluate(c, &choose))?;
                let Some(model_id) = decision.selected.clone() else {
                    return Err(CreateError::NoEligibleModel {
                        profile: profile.clone(),
                        explanation: decision.explanation,
                        task: task_text,
                    });
                };
                let chosen =
                    self.read(|c| repo::eligible_model(c, &model_id))?
                        .map_err(|reason| CreateError::ExactModelUnavailable {
                            model: model_id.clone(),
                            reason,
                            task: task_text.clone(),
                        })?;
                let adapter = self.adapter(&chosen.provider_id).ok_or_else(|| {
                    CreateError::NoEligibleProvider {
                        task: task_text.clone(),
                    }
                })?;
                pending = Some(crate::routing::Pending {
                    explanation: decision.explanation.clone(),
                    selected: Some(model_id),
                    profile: Some(profile.clone()),
                    trigger: symphony_core::RoutingTrigger::Spawn,
                    decision,
                });
                Some((chosen, adapter))
            }
            Execution::Exact(model) => {
                let found = self.read(|c| repo::eligible_model(c, model))?;
                let eligible = found
                    .and_then(|m| match self.adapter(&m.provider_id) {
                        Some(a) => Ok((m, a)),
                        None => Err(format!(
                            "esta versión no tiene adapter para `{}`",
                            m.provider_id
                        )),
                    })
                    .map_err(|reason| CreateError::ExactModelUnavailable {
                        model: model.clone(),
                        reason,
                        task: task_text.clone(),
                    })?;
                // El modelo exacto se obedece y no se sustituye; igual se deja constancia de cómo
                // estaban los demás para que `/explain-route` lo cuente.
                let choose = crate::routing::Choose {
                    profile: self.default_profile(),
                    allow_reserve: true,
                    ..Default::default()
                };
                if let Ok(decision) = self.read(|c| self.evaluate(c, &choose)) {
                    pending = Some(crate::routing::Pending {
                        explanation: crate::routing::exact_explanation(
                            model,
                            &decision.explanation,
                        ),
                        selected: Some(model.clone()),
                        profile: None,
                        trigger: symphony_core::RoutingTrigger::Spawn,
                        decision,
                    });
                }
                Some(eligible)
            }
            Execution::DecideLater => {
                let ready = self.read(repo::ready_providers)?;
                if !ready.iter().any(|p| self.adapter(p).is_some()) {
                    return Err(CreateError::NoEligibleProvider { task: task_text });
                }
                None
            }
        };

        // 2. Repo del proyecto.
        let root = req.project_root.clone();
        let (repo_root, base, branch_name) = tokio::task::spawn_blocking(move || {
            let r = Repo::discover(&root)?;
            Ok::<_, GitError>((
                r.root().to_path_buf(),
                r.head_commit()?,
                r.current_branch()?,
            ))
        })
        .await
        .map_err(store_err)?
        .map_err(|source| CreateError::NotARepo {
            path: req.project_root.clone(),
            source,
        })?;
        let root_text = repo_root.display().to_string();

        let _one_at_a_time = self.creating.lock().await;
        let pid = std::process::id();
        let now = now_ms();

        // 3. Identidad del agente.
        let (project, session, number, task_code, existing) = self.read(|c| {
            let project = repo::project_by_root(c, &root_text)?;
            let (session, number, code) = match &project {
                Some(p) => (
                    repo::active_session(c, p.id, pid)?,
                    repo::next_agent_number(c, p.id)?,
                    repo::next_task_code(c, p.id)?,
                ),
                None => (None, 1, "T-1".to_string()),
            };
            let existing = match &project {
                Some(p) if req.chat => repo::chat_agent(c, p.id)?,
                _ => None,
            };
            Ok((project, session, number, code, existing))
        })?;
        // Dependencias: se resuelven antes de crear el worktree (un `T-99` inexistente no deja nada).
        let mut dep_ids: Vec<TaskId> = Vec::new();
        let mut unmet = Vec::new();
        if !req.depends_on.is_empty() && existing.is_none() {
            let project_id = project.as_ref().map(|p| p.id);
            let refs = req.depends_on.clone();
            let resolved = self.read(|c| {
                let mut out = Vec::new();
                for r in &refs {
                    let found = match (r.parse::<TaskId>(), project_id) {
                        (Ok(id), _) => repo::get_task(c, id).ok(),
                        (Err(_), Some(p)) => repo::task_by_code(c, p, r)?,
                        (Err(_), None) => None,
                    };
                    match found {
                        Some(t) if Some(t.project_id) == project_id => out.push(t),
                        _ => return Ok(Err(r.clone())),
                    }
                }
                Ok(Ok(out))
            })?;
            match resolved {
                Ok(tasks) => {
                    for t in tasks {
                        if t.status != TaskStatus::Done {
                            unmet.push(symphony_store::deps::DepRow {
                                task_id: t.id,
                                code: t.code.clone(),
                                title: t.title.clone(),
                                status: t.status,
                            });
                        }
                        dep_ids.push(t.id);
                    }
                }
                Err(reference) => {
                    return Err(CreateError::UnknownDependency {
                        reference,
                        task: task_text,
                    });
                }
            }
        }
        if let Some(agent) = existing {
            let (wt, task_code) = self.read(|c| {
                let wt = agent.worktree_id.ok_or_else(|| repo::RepoError::NotFound {
                    entity: "worktree",
                    id: agent.id.to_string(),
                })?;
                Ok((
                    repo::get_worktree(c, wt)?,
                    repo::get_task(c, agent.task_id)?.code,
                ))
            })?;
            return Ok(Created {
                agent_id: agent.id,
                task_id: agent.task_id,
                number: agent.number,
                task_code,
                worktree: PathBuf::from(wt.path),
                branch: wt.branch,
                state: agent.state,
                state_reason: agent.state_reason,
                run: None,
            });
        }
        let task_code = if req.chat {
            repo::CHAT_TASK_CODE.to_string()
        } else {
            task_code
        };
        let project_id = project.as_ref().map_or_else(ProjectId::new, |p| p.id);
        let session_id = session.unwrap_or_default();
        let agent_id = AgentId::new();
        let task_id = TaskId::new();
        let worktree_id = WorktreeId::new();
        let short_session = session_id.to_string().to_lowercase();
        let (branch, dir, wt_base) = if req.chat {
            let main = project
                .as_ref()
                .map(|p| p.default_branch.clone())
                .or_else(|| branch_name.clone())
                .unwrap_or_else(|| "main".into());
            (
                symphony_git::CHAT_BRANCH.to_string(),
                "chat".to_string(),
                main,
            )
        } else {
            (
                symphony_git::agent_branch(
                    &short_session[short_session.len().saturating_sub(8)..],
                    u32::try_from(number).unwrap_or(u32::MAX),
                ),
                format!("agent-{number:03}"),
                base.clone(),
            )
        };
        let wt_path = self
            .home
            .join("worktrees")
            .join(project_id.to_string())
            .join(dir);

        // 4. Worktree. Si falla, no se creó nada en la base (y `prepare_worktree` ya revirtió
        // solo lo que llegó a crear este intento, sin tocar recursos preexistentes).
        let plan = {
            let (repo_root, wt_path, branch, wt_base) = (
                repo_root.clone(),
                wt_path.clone(),
                branch.clone(),
                wt_base.clone(),
            );
            tokio::task::spawn_blocking(move || {
                prepare_worktree(&repo_root, &wt_path, &branch, &wt_base)
            })
            .await
            .map_err(store_err)?
        }
        .map_err(|detail| CreateError::Workspace {
            detail,
            task: task_text.clone(),
        })?;

        // 4b. Preparación del worktree (`[worktree] prepare`): si falla, no se creó el agente.
        if let Err(detail) = self
            .prepare_worktree_commands(&root_text, &wt_path, agent_id, number)
            .await
        {
            self.undo_worktree(&repo_root, &wt_path, &branch).await;
            return Err(CreateError::Workspace {
                detail,
                task: task_text,
            });
        }

        // 5. Una transacción con todo.
        // Con dependencias sin cumplir el agente espera y no arranca; el modelo exacto o el
        // profile quedan guardados para cuando se libere (`dependencies.rs`).
        let waiting_phrase =
            (!unmet.is_empty()).then(|| symphony_store::deps::waiting_phrase(&unmet));
        let (state, task_status) = if waiting_phrase.is_some() {
            (AgentState::WaitingDependency, TaskStatus::Waiting)
        } else if executor.is_some() {
            (AgentState::Running, TaskStatus::Running)
        } else {
            (AgentState::Ready, TaskStatus::Ready)
        };
        let launch_now = executor.is_some() && waiting_phrase.is_none();
        let run_id = launch_now.then(RunId::new);
        let objective = match &req.description {
            Some(d) if !d.trim().is_empty() => format!("{title}\n\n{}", d.trim()),
            _ => title.clone(),
        };
        let requested_profile = match &req.execution {
            Execution::Profile(p) => Some(p.clone()),
            _ => None,
        };
        let execution_mode = match req.execution {
            Execution::Exact(_) => ExecutionMode::Exact,
            Execution::Profile(_) => ExecutionMode::Profile,
            Execution::DecideLater => ExecutionMode::DecideLater,
        };
        let tx_data = TxData {
            project: project.is_none().then(|| repo::Project {
                id: project_id,
                name: repo_root
                    .file_name()
                    .map_or_else(|| root_text.clone(), |n| n.to_string_lossy().into_owned()),
                root_path: root_text.clone(),
                default_branch: branch_name.unwrap_or_else(|| "main".into()),
                created_at: now,
            }),
            new_session: session.is_none().then_some((session_id, pid)),
            task: repo::Task {
                id: task_id,
                project_id,
                code: task_code.clone(),
                title: title.clone(),
                description: req.description.clone(),
                status: task_status,
                status_reason: waiting_phrase.clone(),
                priority: req.priority,
            },
            worktree: repo::Worktree {
                id: worktree_id,
                project_id,
                path: wt_path.display().to_string(),
                branch: branch.clone(),
                base_ref: wt_base,
                deps_strategy: plan.strategy.as_str().into(),
                status: "READY".into(),
            },
            agent: repo::Agent {
                id: agent_id,
                project_id,
                session_id,
                task_id,
                worktree_id: Some(worktree_id),
                number,
                state,
                state_reason: waiting_phrase.clone(),
                execution_mode,
                // Un profile deja el modelo en el run, no en el agente (AGENT ≠ MODEL).
                requested_model_id: executor
                    .as_ref()
                    .filter(|_| requested_profile.is_none())
                    .map(|(m, _)| m.model_id.clone()),
                requested_profile_id: requested_profile,
                failover_policy: req.failover,
                context_mode: req.context_mode,
                priority: req.priority,
            },
            checkpoint: repo::NewCheckpoint {
                id: CheckpointId::new(),
                agent_id,
                run_id,
                objective: objective.clone(),
                plan_tail: None,
                current_step: None,
                next_step: Some("Empezar la tarea".into()),
                head_commit: Some(base.clone()),
                summary_json: Some(
                    serde_json::json!({"files_touched": [], "last_command": null}).to_string(),
                ),
                diff_object_id: None,
            },
            run: run_id.zip(
                executor
                    .as_ref()
                    .map(|(m, _)| (m.provider_id.clone(), m.model_id.clone())),
            ),
            handoff: run_id.map(|run| {
                let tokens = symphony_context::handoff::estimate_tokens(&objective);
                repo::NewHandoff {
                    id: symphony_core::HandoffId::new(),
                    agent_id,
                    checkpoint_id: None,
                    to_run_id: run,
                    mode: req.context_mode,
                    tokens_raw_estimate: tokens,
                    tokens_sent: tokens,
                    build_ms: 0,
                    items: Vec::new(),
                }
            }),
            decision: if launch_now { pending } else { None },
            deps: dep_ids,
            review: req.review,
            now,
        };
        if let Err(e) = self.writer.write(Box::new(move |t| tx_data.apply(t))).await {
            self.undo_worktree(&repo_root, &wt_path, &branch).await;
            return Err(store_err(e));
        }
        // Los archivos del proyecto quedan buscables (P09.S5): un watcher por proyecto.
        self.watch_project(&project_id.to_string(), &repo_root);
        // Nació esperando: si sus dependencias terminaron mientras se creaba, se libera ya.
        let mut state = state;
        let mut waiting_phrase = waiting_phrase;
        if state == AgentState::WaitingDependency {
            crate::dependencies::recheck_new(self, task_id).await;
            if let Ok(a) = self.read(|c| repo::get_agent(c, agent_id))
                && a.state != AgentState::WaitingDependency
            {
                state = a.state;
                waiting_phrase = a.state_reason;
            }
        }

        let mut created = Created {
            agent_id,
            task_id,
            number,
            task_code,
            worktree: wt_path.clone(),
            branch,
            state,
            state_reason: waiting_phrase,
            run: None,
        };

        // 6. Lanzar el executor con el modelo exacto.
        if let (Some(run_id), Some((model, adapter))) = (run_id, executor) {
            created.run = Some((run_id, model.model_id.clone()));
            let launch = Launch {
                adapter,
                project_id,
                agent_id,
                task_id,
                run_id,
                provider_id: model.provider_id.clone(),
                model_id: model.model_id.clone(),
                worktree: wt_path,
                cli_model: model.cli_model_id,
                prompt: self.planned_prompt(task_id, req.chat || req.review, &objective),
                resume_session: None,
            };
            if let Err(reason) = self.launch(launch).await {
                created.state = AgentState::Failed;
                created.state_reason = Some(reason);
            }
        }
        Ok(created)
    }

    /// Borra el worktree y la rama creados por este intento fallido.
    async fn undo_worktree(&self, repo_root: &Path, wt_path: &Path, branch: &str) {
        let (root, path, branch) = (
            repo_root.to_path_buf(),
            wt_path.to_path_buf(),
            branch.to_string(),
        );
        let _ =
            tokio::task::spawn_blocking(move || undo_worktree_sync(&root, &path, &branch)).await;
    }
}

fn undo_worktree_sync(repo_root: &Path, wt_path: &Path, branch: &str) {
    let repo = Repo::at(repo_root);
    if (wt_path.exists() || wt_path.symlink_metadata().is_ok())
        && let Err(e) = repo.worktree_remove(wt_path, true)
    {
        tracing::warn!(
            path = %wt_path.display(),
            error = %e,
            "no se pudo quitar el worktree con git al revertir la creación"
        );
        if (wt_path.exists() || wt_path.symlink_metadata().is_ok())
            && let Err(e) = std::fs::remove_dir_all(wt_path)
        {
            tracing::warn!(
                path = %wt_path.display(),
                error = %e,
                "no se pudo borrar la carpeta del worktree al revertir la creación"
            );
        }
    }
    if let Err(e) = repo.worktree_prune() {
        tracing::warn!(error = %e, "no se pudo podar worktrees al revertir la creación");
    }
    if repo.branch_exists(branch).unwrap_or(false)
        && let Err(e) = repo.delete_branch(branch, true)
    {
        tracing::warn!(
            branch,
            error = %e,
            "no se pudo borrar la rama al revertir la creación"
        );
    }
}

/// Crea el worktree y aplica la estrategia de dependencias que no necesita el scheduler.
/// Si hay colisión con una carpeta o rama preexistente, devuelve error sin borrar nada;
/// si falla después de crear el worktree/rama en este intento, revierte solo lo creado.
fn prepare_worktree(
    repo_root: &Path,
    wt_path: &Path,
    branch: &str,
    base: &str,
) -> Result<deps::DepsPlan, String> {
    if wt_path.exists() || wt_path.symlink_metadata().is_ok() {
        return Err(format!("{} ya existe", wt_path.display()));
    }
    let repo = Repo::at(repo_root);
    // El plan por fases es temporal: no entra en `git add -A` ni en el diff (exclusión local).
    let _ = repo.exclude_locally(crate::plan::PLAN_EXCLUDE);
    if let Some(parent) = wt_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    if repo.branch_exists(branch).map_err(|e| e.to_string())? {
        return Err(format!("la rama `{branch}` ya existe"));
    }
    if let Err(e) = repo.worktree_add(wt_path, branch, base) {
        undo_worktree_sync(repo_root, wt_path, branch);
        return Err(e.to_string());
    }
    let setup = (|| {
        let plan = deps::plan(wt_path, repo_root).map_err(|e| e.to_string())?;
        // ponytail: INSTALL y PNPM_STORE son operaciones pesadas; las corre el scheduler (P08).
        if plan.strategy == deps::DepsStrategy::Link {
            deps::link_node_modules(wt_path, repo_root).map_err(|e| e.to_string())?;
        }
        Ok(plan)
    })();
    if setup.is_err() {
        undo_worktree_sync(repo_root, wt_path, branch);
    }
    setup
}

/// Filas de la transacción de creación.
struct TxData {
    project: Option<repo::Project>,
    new_session: Option<(SessionId, u32)>,
    task: repo::Task,
    worktree: repo::Worktree,
    agent: repo::Agent,
    checkpoint: repo::NewCheckpoint,
    run: Option<(RunId, (String, String))>,
    /// Handoff del primer spawn: el prompt es el objetivo, sin checkpoint previo.
    handoff: Option<repo::NewHandoff>,
    /// Cómo se eligió el modelo del primer run.
    decision: Option<crate::routing::Pending>,
    /// Tareas de las que depende (P08.S5).
    deps: Vec<TaskId>,
    /// `kind = REVIEW` (P08.S7).
    review: bool,
    now: i64,
}

impl TxData {
    fn apply(self, t: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
        let now = self.now;
        if let Some(p) = &self.project {
            repo::insert_project(t, p)?;
        }
        if let Some((id, pid)) = self.new_session {
            repo::start_session(t, id, self.task.project_id, pid, now)?;
        }
        repo::insert_task_kind(
            t,
            &self.task,
            if self.review { "REVIEW" } else { "WORK" },
            now,
        )?;
        repo::insert_worktree(t, &self.worktree, now)?;
        repo::insert_agent(t, &self.agent, now)?;
        if let Some((run_id, (provider, model))) = &self.run {
            repo::open_run(t, *run_id, self.agent.id, provider, model, now)?;
            if let Some(d) = &self.decision {
                d.save(t, self.agent.id, Some(*run_id), now)?;
            }
        }
        if let Some(h) = &self.handoff {
            repo::insert_handoff(t, h, now)?;
        }
        repo::insert_checkpoint(t, &self.checkpoint, now)?;
        for dep in &self.deps {
            symphony_store::deps::add_dependency(t, self.task.id, *dep, now)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        }
        Ok(())
    }
}

/// ¿Existe el profile? (la tabla `profiles` la siembra la migración 002).
fn routing_profile_exists(
    conn: &rusqlite::Connection,
    profile: &str,
) -> Result<bool, repo::RepoError> {
    Ok(crate::routing::profile_weights(conn, profile)?.is_some())
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::executor::LiveRun;
    use symphony_object_store::ObjectStore;
    use symphony_store::Writer;
    use symphony_testkit::FakeAdapter;
    use tokio::sync::mpsc;

    /// Auditoría P13.S5 (`server.md`, hallazgo 8): si el watchdog observó que `run_a` estaba
    /// inactivo pero, mientras esperaba al escritor, el agente cambió a `run_b`, el envío de
    /// `NoHeartbeat` condicionado a `run_a` se descarta y no mata a `run_b`.
    #[tokio::test]
    async fn watchdog_discards_no_heartbeat_for_replaced_run() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let writer = Writer::start(&home.join("symphony.db")).unwrap();
        let bus = EventBus::new(writer.handle(), 16, ObjectStore::new(home.join("objects")));
        let rt = Runtime::new_with_watchdog(
            &home,
            writer.handle(),
            writer.reader().unwrap(),
            bus,
            Vec::new(),
            None,
            WatchdogTiming {
                heartbeat_every: Duration::from_secs(3600),
                stale_after: Duration::from_millis(50),
            },
        );
        let agent = AgentId::new();
        let run_a = RunId::new();
        let run_b = RunId::new();
        let (tx_b, mut rx_b) = mpsc::unbounded_channel();
        let adapter: Arc<dyn ProviderAdapter> =
            Arc::new(FakeAdapter::new(PathBuf::from("fake"), dir.path()));
        rt.live.lock().unwrap().insert(
            agent,
            LiveRun {
                run_id: run_b,
                adapter,
                project_id: ProjectId::new(),
                control: tx_b,
            },
        );

        // `run_a` ya no tiene actividad reciente (fue reemplazado por `run_b`): el paso del
        // watchdog con la instantánea vieja `(agent, run_a)` no debe enviar `NoHeartbeat` a `run_b`.
        rt.watchdog_step(agent, run_a).await;
        assert!(
            rx_b.try_recv().is_err(),
            "el watchdog no debe cerrar el run nuevo por la inactividad del run anterior"
        );

        // Cuando es el propio `run_b` el que supera `stale_after`, sí recibe `NoHeartbeat`.
        let responder = tokio::spawn(async move {
            match rx_b.recv().await {
                Some(Control::NoHeartbeat { done }) => {
                    let _ = done.send(());
                    true
                }
                _ => false,
            }
        });
        rt.watchdog_step(agent, run_b).await;
        assert!(responder.await.unwrap());
        writer.shutdown();
    }
}
