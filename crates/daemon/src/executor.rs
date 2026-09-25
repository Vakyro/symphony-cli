//! Executors vivos (P06.S1, S5): lanzar el CLI, leer su salida, cerrarlo y
//! reemplazarlo. AGENT ≠ MODEL: el agente, su task, su worktree y su rama no
//! cambian; cambia el run abierto.
//!
//! Cada executor vivo tiene un canal de control (parar, escribir en stdin,
//! suspender, reanudar). La tarea que lee su salida es la única dueña del proceso.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use symphony_adapter_common::{AgentEvent, ProviderAdapter, ProviderError, SpawnRequest};
use symphony_core::{
    AgentId, AgentState, ExecutorChangeId, FailoverPolicy, FailureType, HandoffId, MessageId,
    ProjectId, ProviderFailureId, RunEndReason, RunId, RunStatus, TaskId, TaskStatus,
};
use symphony_process::{ExitStatus, OutputLine, Supervised};
use symphony_store::repo;
use tokio::sync::{mpsc, oneshot};

use crate::bus::{BusEvent, EventSource};
use crate::runtime::{Runtime, now_ms};

/// Lo que hace falta para lanzar un executor.
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
}

/// Órdenes para el executor vivo de un agente.
pub(crate) enum Control {
    /// Mata el árbol de procesos y cierra el run así. El estado del agente lo decide quien pide.
    Stop {
        status: RunStatus,
        end: RunEndReason,
        done: oneshot::Sender<()>,
    },
    Send {
        bytes: Vec<u8>,
        done: oneshot::Sender<Result<(), String>>,
    },
    Suspend(oneshot::Sender<Result<(), String>>),
    Resume(oneshot::Sender<Result<(), String>>),
}

pub struct LiveRun {
    pub(crate) run_id: RunId,
    pub(crate) adapter: Arc<dyn ProviderAdapter>,
    pub(crate) project_id: ProjectId,
    pub(crate) control: mpsc::UnboundedSender<Control>,
}

/// Fallos que justifican cambiar de executor (P06.S5): cuota agotada o login.
fn needs_failover(e: &ProviderError) -> bool {
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

/// Error de las operaciones sobre un agente existente, con frase legible.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct AgentOpError(pub String);

fn op_err(e: impl std::fmt::Display) -> AgentOpError {
    AgentOpError(e.to_string())
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

    async fn launch_inner(&self, l: Launch) -> Result<(), String> {
        let env = vec![
            ("SYMPHONY_AGENT_ID".into(), l.agent_id.to_string()),
            ("SYMPHONY_PROJECT_ID".into(), l.project_id.to_string()),
            ("SYMPHONY_RUN_ID".into(), l.run_id.to_string()),
            ("SYMPHONY_HOME".into(), self.home.display().to_string()),
        ];
        let spawn_req = SpawnRequest {
            worktree: l.worktree.clone(),
            model: l.cli_model.clone(),
            prompt: l.prompt.clone(),
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
            event: AgentEvent::UserMessage {
                text: l.prompt.clone(),
            },
            occurred_at: now_ms(),
        };
        if self.bus.publish(prompt_ev).await.is_err() {
            return Err("la base de datos no acepta escrituras".into());
        }
        let started = async {
            let spec = l
                .adapter
                .spawn_spec(&spawn_req)
                .map_err(|e| e.to_string())?;
            let mut proc = symphony_process::spawn(spec)
                .await
                .map_err(|e| e.to_string())?;
            proc.write_stdin(&l.adapter.encode_prompt(&l.prompt))
                .await
                .map_err(|e| e.to_string())?;
            if l.adapter.close_stdin_after_prompt() {
                proc.close_stdin().await.map_err(|e| e.to_string())?;
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
        let _ = self
            .writer
            .write(Box::new(move |t| {
                Ok(repo::set_run_process(t, run_id, pid, None, None)?)
            }))
            .await;
        let (control, rx) = mpsc::unbounded_channel();
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
            Box::pin(self.clone().pump(l, proc, rx));
        self.pumps.spawn(task);
        Ok(())
    }

    /// Lee la salida del CLI y atiende el control hasta que el proceso termina.
    async fn pump(self, l: Launch, mut proc: Supervised, mut rx: mpsc::UnboundedReceiver<Control>) {
        let mut fatal: Option<ProviderError> = None;
        let mut control_open = true;
        let stopped = loop {
            tokio::select! {
                line = proc.next_output() => match line {
                    None => break None,
                    Some(OutputLine::Stdout(line)) => {
                        if !self.handle_line(&l, &proc, &line, &mut fatal).await {
                            break None;
                        }
                    }
                    Some(OutputLine::Stderr(_)) => {}
                },
                ctl = rx.recv(), if control_open => match ctl {
                    None => control_open = false,
                    Some(Control::Stop { status, end, done }) => {
                        let _ = proc.terminate_tree();
                        break Some((status, end, done));
                    }
                    Some(Control::Send { bytes, done }) => {
                        let _ = done.send(proc.write_stdin(&bytes).await.map_err(|e| e.to_string()));
                    }
                    Some(Control::Suspend(done)) => {
                        let _ = done.send(proc.suspend().map_err(|e| e.to_string()));
                    }
                    Some(Control::Resume(done)) => {
                        let _ = done.send(proc.resume().map_err(|e| e.to_string()));
                    }
                },
            }
        };
        let exit = proc.wait().await;
        if let Ok(mut live) = self.live.lock()
            && live.get(&l.agent_id).is_some_and(|r| r.run_id == l.run_id)
        {
            live.remove(&l.agent_id);
        }
        let code = match exit {
            ExitStatus::Exited(c) => Some(c),
            _ => None,
        };
        match (stopped, fatal) {
            (Some((status, end, done)), _) => {
                let run = l.run_id;
                let _ = self
                    .writer
                    .write(Box::new(move |t| {
                        repo::close_run(t, run, status, end, code, now_ms())?;
                        if status == RunStatus::HandedOff {
                            repo::set_handoff_outcome(t, run, "CONTINUED")?;
                        }
                        Ok(())
                    }))
                    .await;
                let _ = done.send(());
            }
            (None, Some(err)) => self.failover(&l, &err, code).await,
            (None, None) => self.finish_natural(&l, exit).await,
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
    ) -> bool {
        for event in l.adapter.parse_stream_line(line) {
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
                AgentEvent::ProviderError(e) if needs_failover(e) => *fatal = Some(e.clone()),
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
    async fn finish_natural(&self, l: &Launch, exit: ExitStatus) {
        // ponytail: exit 0 = tarea terminada; el heartbeat (P06.S6) cubre los cuelgues.
        if exit == ExitStatus::Exited(0) {
            let (agent, task, run) = (l.agent_id, l.task_id, l.run_id);
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
                    repo::set_agent_state(t, agent, AgentState::Completed, None, now)?;
                    repo::set_task_status(t, task, TaskStatus::Done, None, now)?;
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
        self.finish_failed(l, RunStatus::Failed, code, &reason)
            .await;
    }

    /// Run `FAILED/CRASH`, agente `FAILED` con razón y recovery item `EXECUTOR_EXITED`.
    async fn finish_failed(&self, l: &Launch, status: RunStatus, code: Option<i32>, reason: &str) {
        let (project, agent, run, reason) =
            (l.project_id, l.agent_id, l.run_id, reason.to_string());
        let result = self
            .writer
            .write(Box::new(move |t| {
                let now = now_ms();
                repo::close_run(t, run, status, RunEndReason::Crash, code, now)?;
                repo::set_handoff_outcome(t, run, "FAILED_TO_CONTINUE")?;
                repo::set_agent_state(t, agent, AgentState::Failed, Some(&reason), now)?;
                repo::open_recovery_item(
                    t,
                    project,
                    Some(agent),
                    Some(run),
                    "EXECUTOR_EXITED",
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

    /// Cuota agotada o login rechazado (P06.S5, IDEA §5.7): otro executor si la
    /// política lo permite; si no, `WAITING_PROVIDER` + recovery item.
    async fn failover(&self, l: &Launch, err: &ProviderError, code: Option<i32>) {
        let auth = err.failure_type == FailureType::Auth;
        let end = if auth {
            RunEndReason::AuthError
        } else {
            RunEndReason::QuotaExhausted
        };
        let who = l.adapter.display_name();
        let (why, why_en) = if auth {
            (format!("{who} rechazó el login"), "login rejected")
        } else {
            (format!("{who} se quedó sin cuota"), "quota exhausted")
        };
        let failure = repo::NewProviderFailure {
            id: ProviderFailureId::new(),
            provider_id: l.provider_id.clone(),
            model_id: Some(l.model_id.clone()),
            run_id: Some(l.run_id),
            failure_type: err.failure_type,
            raw_code: err.raw_code.clone(),
            message: symphony_core::redact(&err.message).into_owned(),
            reset_at: err.resets_at,
        };
        let failure_id = failure.id;

        let next = self.read_op(|c| {
            let agent = repo::get_agent(c, l.agent_id)?;
            let has_adapter = |p: &str| self.adapter(p).is_some();
            let next = repo::next_executor(
                c,
                l.agent_id,
                agent.failover_policy,
                &l.provider_id,
                &l.model_id,
                auth,
                &has_adapter,
            )?;
            Ok((agent, next))
        });
        let (agent, next) = match next {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(agent = %l.agent_id, error = %e, "failover: no se pudo leer el agente");
                return;
            }
        };
        let status = if next.is_some() {
            RunStatus::HandedOff
        } else {
            RunStatus::Failed
        };
        let run = l.run_id;
        let closed = self
            .writer
            .write(Box::new(move |t| {
                let now = now_ms();
                repo::close_run(t, run, status, end, code, now)?;
                repo::set_handoff_outcome(t, run, "CONTINUED")?;
                repo::insert_provider_failure(t, &failure, now)?;
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
                why_en,
                Some(failure_id),
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
        reason_en: &str,
        failure_id: Option<ProviderFailureId>,
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
                let old_name = self
                    .adapter(&old.provider_id)
                    .map_or_else(|| old.provider_id.clone(), |a| a.display_name().to_string());
                let separator = format!(
                    "── Executor changed: {old_name} / {} → {} / {}\nReason: {reason_en}\nAgent #{}, task and workspace unchanged\nHandoff restored from checkpoint #{}",
                    old.model_id,
                    adapter.display_name(),
                    model.model_id,
                    agent.number,
                    h.checkpoint_seq
                );
                let change_row = (from, h.checkpoint_id, now_ms() - h.checkpoint_created_at);
                let handoff = (
                    h.checkpoint_id,
                    h.mode,
                    h.tokens_raw_estimate,
                    h.tokens_sent,
                    h.build_ms,
                );
                (h.prompt, handoff, Some(change_row), Some(separator))
            }
            None => {
                let objective = self
                    .read_op(|c| repo::latest_checkpoint(c, agent.id))?
                    .map(|c| c.objective)
                    .ok_or_else(|| AgentOpError("el agente no tiene checkpoint".into()))?;
                let tokens = symphony_context::handoff::estimate_tokens(&objective);
                (
                    objective,
                    (None, agent.context_mode, tokens, tokens, 0),
                    None,
                    None,
                )
            }
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
                let (checkpoint_id, mode, raw, sent, build_ms) = handoff;
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
        };
        // Si no arranca, `launch` ya dejó al agente `FAILED` con razón y recovery item.
        let _ = self.launch(launch).await;
        Ok(run)
    }

    /// Manda una orden al executor vivo del agente y espera la respuesta.
    pub(crate) async fn control<T>(
        &self,
        agent: AgentId,
        make: impl FnOnce(oneshot::Sender<T>) -> Control,
    ) -> Option<T> {
        let (tx, rx) = oneshot::channel();
        let sent = self
            .live
            .lock()
            .ok()
            .and_then(|live| live.get(&agent).map(|r| r.control.send(make(tx)).is_ok()))
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
        self.start_successor(
            &agent,
            from_run.map(|r| r.id),
            model,
            &reason,
            "USER_SWITCH",
            "user switch",
            None,
        )
        .await
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

    /// Reanuda un executor pausado. El agente vuelve a `RUNNING`.
    pub async fn resume(&self, agent: AgentId) -> Result<(), AgentOpError> {
        self.live_parts(agent)?;
        let from = self.read_op(|c| repo::get_agent(c, agent))?.state;
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
}
