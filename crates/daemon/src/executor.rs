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
            let spec = match &l.resume_session {
                Some(id) => l.adapter.resume_spec(&ResumeRequest {
                    spawn: spawn_req.clone(),
                    cli_session_id: id.clone(),
                }),
                None => l.adapter.spawn_spec(&spawn_req),
            }
            .map_err(|e| e.to_string())?;
            let mut proc = symphony_process::spawn(spec)
                .await
                .map_err(|e| e.to_string())?;
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
            exited_early(proc.write_stdin(&l.adapter.encode_prompt(&l.prompt)).await)?;
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
                        self.beat(l.run_id);
                        let mut end_turn = false;
                        if !self.handle_line(&l, &proc, &line, &mut fatal, &mut end_turn).await {
                            break None;
                        }
                        if end_turn {
                            let _ = proc.close_stdin().await;
                        }
                    }
                    Some(OutputLine::Stderr(line)) => {
                        self.beat(l.run_id);
                        let mut end_turn = false;
                        let events = l.adapter.parse_stderr_line(&line);
                        if !self.apply_events(&l, &proc, events, &mut fatal, &mut end_turn).await {
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
                                let reason = "el executor dejó de responder; el workspace y \
                                                el último checkpoint quedan intactos"
                                    .to_string();
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
                                    "NO_HEARTBEAT",
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
    async fn finish_natural(&self, l: &Launch, exit: ExitStatus) {
        // ponytail: exit 0 = tarea terminada; el heartbeat (P06.S6) cubre los cuelgues.
        if exit == ExitStatus::Exited(0) {
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
        self.finish_failed(l, RunStatus::Failed, code, &reason)
            .await;
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
                repo::close_run(t, run, status, RunEndReason::Crash, code, now)?;
                crate::health::on_run_finished(t, run, &health_cfg, now)?;
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

    /// El mejor reemplazo según la política del agente (P10.S5, IDEA §5.7): el router elige entre
    /// los modelos utilizables, sin tocar lo que este agente ya agotó ni la reserva de cuota.
    /// `rotate`: rotación a propósito (umbral de contexto): se aparta el proveedor actual.
    fn pick_replacement(
        &self,
        c: &rusqlite::Connection,
        agent: &repo::Agent,
        current_provider: &str,
        current_model: &str,
        auth: bool,
        rotate: bool,
    ) -> Result<(Option<repo::EligibleModel>, Option<crate::routing::Pending>), repo::RepoError>
    {
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
        let (why, why_en) = if auth {
            (format!("{who} rechazó el login"), "login rejected")
        } else {
            (format!("{who} se quedó sin cuota"), "quota exhausted")
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

        // 2. El router elige el reemplazo.
        let picked = self.read_op(|c| {
            let agent = repo::get_agent(c, l.agent_id)?;
            let (next, pending) =
                self.pick_replacement(c, &agent, &l.provider_id, &l.model_id, auth, false)?;
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
                why_en,
                Some(failure_id),
                None,
                pending,
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
        message: Option<&str>,
        route: Option<crate::routing::Pending>,
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
                    h.items,
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
        let decision = self.read_op(|c| {
            if crate::routing::profile_weights(c, &profile_name)?.is_none() {
                return Ok(None);
            }
            let choose = crate::routing::Choose {
                profile: profile_name.clone(),
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
            "user switch",
            None,
            message,
            route,
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
    fn rotation_target(
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
        self.read_op(|c| {
            if repo::get_task(c, agent.task_id)?.code != repo::CHAT_TASK_CODE
                || repo::last_turn_tokens(c, last.id)? < limit
            {
                return Ok(None);
            }
            let (next, pending) =
                self.pick_replacement(c, agent, &last.provider_id, &last.model_id, false, true)?;
            Ok(next.map(|n| (n, pending)))
        })
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
        if let Some((next, route)) = self.rotation_target(&agent, &last)? {
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
                    "context threshold",
                    None,
                    Some(text),
                    route,
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
        reason_en: &str,
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
                reason_en,
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
            "user restart",
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
            "reclaim after lost heartbeat",
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
            .map_err(op_err)
    }

    /// Termina forzosamente el executor y el agente (FLOW §7).
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
        Ok(serde_json::json!({
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
