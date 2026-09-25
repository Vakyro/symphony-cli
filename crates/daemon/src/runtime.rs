//! Runtime de agentes (P06): crea agentes (FLOW §6) y lanza su executor.
//!
//! Orden de creación: primero el worktree (git, fuera de la base) y después una
//! sola transacción con project, session, task, worktree, agente, checkpoint
//! inicial y run. Si algo falla antes de confirmar, se deshace el worktree: no
//! queda un agente "medio roto". Un fallo al lanzar el CLI sí deja al agente,
//! en `FAILED` con su razón y un item en el Recovery Center (su workspace sirve).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use symphony_adapter_common::{HookCommand, ProviderAdapter};
use symphony_core::{
    AgentId, AgentState, CheckpointId, ContextMode, ExecutionMode, FailoverPolicy, ProjectId,
    RunId, SessionId, TaskId, TaskStatus, WorktreeId,
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
    #[error("los profiles llegan con el routing (P10); usa un modelo exacto o «decidir después»")]
    ProfilesNotSupported { task: String },
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
    #[error("base de datos: {0}")]
    Store(String),
}

impl CreateError {
    /// Código estable para el protocolo IPC.
    pub fn code(&self) -> &'static str {
        match self {
            Self::EmptyTask => "empty_task",
            Self::NotARepo { .. } => "not_a_repo",
            Self::ProfilesNotSupported { .. } => "profiles_not_supported",
            Self::NoEligibleProvider { .. } => "no_eligible_provider",
            Self::ExactModelUnavailable { .. } => "exact_model_unavailable",
            Self::Workspace { .. } => "workspace_failed",
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
            }),
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(rt.clone().watchdog());
        }
        rt
    }

    /// Marca actividad de un run vivo (lo llama el pump por cada línea).
    pub(crate) fn beat(&self, run: RunId) {
        if let Ok(mut activity) = self.activity.lock() {
            activity.insert(run, Instant::now());
        }
    }

    /// Persiste el latido de los runs vivos y cierra como `NO_HEARTBEAT` los
    /// que llevan más de `stale_after` sin emitir nada (FLOW §16, IDEA §5.10).
    async fn watchdog(self) {
        let mut tick = tokio::time::interval(self.heartbeat_every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let live: Vec<(AgentId, RunId)> = self
                .live
                .lock()
                .map(|l| l.iter().map(|(a, r)| (*a, r.run_id)).collect())
                .unwrap_or_default();
            for (agent, run) in live {
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
                if silent {
                    tracing::warn!(
                        agent = %agent,
                        run = %run,
                        "executor sin latido: se cierra como NO_HEARTBEAT y se abre recuperación"
                    );
                    let _ = self
                        .control(agent, |done| Control::NoHeartbeat { done })
                        .await;
                }
            }
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
        crate::handoff::prepare(&self.reader, agent, reason).await
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
        let executor = match &req.execution {
            Execution::Profile(_) => {
                return Err(CreateError::ProfilesNotSupported { task: task_text });
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
        let (project, session, number, task_code) = self.read(|c| {
            let project = repo::project_by_root(c, &root_text)?;
            let (session, number, code) = match &project {
                Some(p) => (
                    repo::active_session(c, p.id, pid)?,
                    repo::next_agent_number(c, p.id)?,
                    repo::next_task_code(c, p.id)?,
                ),
                None => (None, 1, "T-1".to_string()),
            };
            Ok((project, session, number, code))
        })?;
        let project_id = project.as_ref().map_or_else(ProjectId::new, |p| p.id);
        let session_id = session.unwrap_or_default();
        let agent_id = AgentId::new();
        let task_id = TaskId::new();
        let worktree_id = WorktreeId::new();
        let short_session = session_id.to_string().to_lowercase();
        let branch = symphony_git::agent_branch(
            &short_session[short_session.len().saturating_sub(8)..],
            u32::try_from(number).unwrap_or(u32::MAX),
        );
        let wt_path = self
            .home
            .join("worktrees")
            .join(project_id.to_string())
            .join(format!("agent-{number:03}"));

        // 4. Worktree. Si falla, no se creó nada en la base.
        let plan = {
            let (repo_root, wt_path, branch, base) = (
                repo_root.clone(),
                wt_path.clone(),
                branch.clone(),
                base.clone(),
            );
            tokio::task::spawn_blocking(move || {
                prepare_worktree(&repo_root, &wt_path, &branch, &base)
            })
            .await
            .map_err(store_err)?
        };
        let plan = match plan {
            Ok(plan) => plan,
            Err(detail) => {
                self.undo_worktree(&repo_root, &wt_path, &branch).await;
                return Err(CreateError::Workspace {
                    detail,
                    task: task_text,
                });
            }
        };

        // 5. Una transacción con todo.
        let (state, task_status) = if executor.is_some() {
            (AgentState::Running, TaskStatus::Running)
        } else {
            (AgentState::Ready, TaskStatus::Ready)
        };
        let run_id = executor.as_ref().map(|_| RunId::new());
        let objective = match &req.description {
            Some(d) if !d.trim().is_empty() => format!("{title}\n\n{}", d.trim()),
            _ => title.clone(),
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
                status_reason: None,
                priority: req.priority,
            },
            worktree: repo::Worktree {
                id: worktree_id,
                project_id,
                path: wt_path.display().to_string(),
                branch: branch.clone(),
                base_ref: base.clone(),
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
                state_reason: None,
                execution_mode,
                requested_model_id: executor.as_ref().map(|(m, _)| m.model_id.clone()),
                requested_profile_id: None,
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
                }
            }),
            now,
        };
        if let Err(e) = self.writer.write(Box::new(move |t| tx_data.apply(t))).await {
            self.undo_worktree(&repo_root, &wt_path, &branch).await;
            return Err(store_err(e));
        }

        let mut created = Created {
            agent_id,
            task_id,
            number,
            task_code,
            worktree: wt_path.clone(),
            branch,
            state,
            state_reason: None,
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
                prompt: objective,
            };
            if let Err(reason) = self.launch(launch).await {
                created.state = AgentState::Failed;
                created.state_reason = Some(reason);
            }
        }
        Ok(created)
    }

    /// Borra el worktree y la rama de un intento fallido (best effort).
    async fn undo_worktree(&self, repo_root: &Path, wt_path: &Path, branch: &str) {
        let (root, path, branch) = (
            repo_root.to_path_buf(),
            wt_path.to_path_buf(),
            branch.to_string(),
        );
        let _ = tokio::task::spawn_blocking(move || {
            let repo = Repo::at(&root);
            if path.exists() {
                let _ = repo.worktree_remove(&path, true);
                let _ = std::fs::remove_dir_all(&path);
            }
            let _ = repo.worktree_prune();
            let _ = repo.delete_branch(&branch, true);
        })
        .await;
    }
}

/// Crea el worktree y aplica la estrategia de dependencias que no necesita el scheduler.
fn prepare_worktree(
    repo_root: &Path,
    wt_path: &Path,
    branch: &str,
    base: &str,
) -> Result<deps::DepsPlan, String> {
    if wt_path.exists() {
        return Err(format!("{} ya existe", wt_path.display()));
    }
    if let Some(parent) = wt_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    Repo::at(repo_root)
        .worktree_add(wt_path, branch, base)
        .map_err(|e| e.to_string())?;
    let plan = deps::plan(wt_path, repo_root).map_err(|e| e.to_string())?;
    // ponytail: INSTALL y PNPM_STORE son operaciones pesadas; las corre el scheduler (P08).
    if plan.strategy == deps::DepsStrategy::Link {
        deps::link_node_modules(wt_path, repo_root).map_err(|e| e.to_string())?;
    }
    Ok(plan)
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
        repo::insert_task(t, &self.task, now)?;
        repo::insert_worktree(t, &self.worktree, now)?;
        repo::insert_agent(t, &self.agent, now)?;
        if let Some((run_id, (provider, model))) = &self.run {
            repo::open_run(t, *run_id, self.agent.id, provider, model, now)?;
        }
        if let Some(h) = &self.handoff {
            repo::insert_handoff(t, h, now)?;
        }
        repo::insert_checkpoint(t, &self.checkpoint, now)?;
        Ok(())
    }
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
