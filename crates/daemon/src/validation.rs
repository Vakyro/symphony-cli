//! Motor de validación (P08.S6, STACK §23, FLOW §15): corre los comandos de los tiers de
//! `project.toml` en el worktree del agente, a través del scheduler, guarda lo que pasó y,
//! como gate de la tarea, le devuelve los fallos al mismo agente.
//!
//! Symphony no entiende ESLint, Vitest ni Cargo: ejecuta, captura, resume y reporta.

use std::path::Path;
use std::time::{Duration, Instant};

use symphony_core::{
    AgentId, AgentState, CheckName, CheckStatus, ContextObjectId, RunEndReason, RunId, RunStatus,
    TaskStatus, ToolCallId, ValidationConfig, ValidationStatus, ValidationTrigger,
};
use symphony_process::{OutputLine, ProcessSpec};
use symphony_scheduler::{OpClass, Request, check_name, classify, summarize};
use symphony_store::repo;
use symphony_store::validation::{self as store, NewCheck, NewValidationRun};

use crate::executor::Launch;
use crate::runtime::{Runtime, now_ms};
use crate::scheduling::{Submitted, Waited};

/// Lo que se conserva de la salida de un comando (la cola: ahí está el fallo).
const OUTPUT_KEEP: usize = 1024 * 1024;
/// Cuánto se espera un turno del scheduler para un comando de validación.
const SLOT_WAIT: Duration = Duration::from_secs(30 * 60);

/// Un comando ya ejecutado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub command: String,
    pub name: CheckName,
    pub status: CheckStatus,
    pub passed: Option<u32>,
    pub failed: Option<u32>,
    pub summary: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
}

/// Una pasada de un tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierResult {
    pub tier: u8,
    pub status: ValidationStatus,
    pub checks: Vec<CheckResult>,
}

/// El resultado de validar hasta un tier.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Validation {
    pub tiers: Vec<TierResult>,
}

impl Validation {
    pub fn passed(&self) -> bool {
        self.tiers
            .iter()
            .all(|t| t.status != ValidationStatus::Fail)
    }

    /// El texto que recibe el agente cuando falla (FLOW §15.2).
    pub fn feedback(&self) -> String {
        let mut out = String::from(
            "Symphony ejecutó la validación de tu tarea y no pasó. Corrígelo y termina de nuevo.\n",
        );
        for t in &self.tiers {
            for c in t.checks.iter().filter(|c| c.status == CheckStatus::Fail) {
                out.push_str(&format!(
                    "\n[tier {} · {}] `{}` → código {}\n",
                    t.tier,
                    c.name.as_str(),
                    c.command,
                    c.exit_code.map_or("?".into(), |e| e.to_string())
                ));
                if let (Some(p), Some(f)) = (c.passed, c.failed) {
                    out.push_str(&format!("{f} fallaron, {p} pasaron\n"));
                }
                if !c.summary.is_empty() {
                    out.push_str(&c.summary);
                    out.push('\n');
                }
            }
        }
        out
    }

    /// Una frase corta para la razón del agente y el recovery item.
    pub fn headline(&self) -> String {
        self.tiers
            .iter()
            .flat_map(|t| t.checks.iter().map(move |c| (t.tier, c)))
            .find(|(_, c)| c.status == CheckStatus::Fail)
            .map_or_else(
                || "la validación falló".into(),
                |(tier, c)| {
                    let first = c.summary.lines().next().unwrap_or("").trim();
                    format!("tier {tier} · {}: `{}` {first}", c.name.as_str(), c.command)
                },
            )
    }
}

/// Un comando ejecutado en el shell de la plataforma.
pub(crate) struct Exec {
    pub(crate) success: bool,
    pub(crate) exit_code: Option<i32>,
    pub(crate) output: String,
    pub(crate) timed_out: bool,
}

fn shell_spec(command: &str) -> ProcessSpec {
    if cfg!(windows) {
        ProcessSpec::new("cmd").args(["/C", command])
    } else {
        ProcessSpec::new("sh").args(["-c", command])
    }
}

/// Ejecuta `command` en `cwd` con los límites del SO y un tope de tiempo; captura la cola de
/// la salida (stdout y stderr juntos, en orden de llegada).
async fn exec_shell(
    command: &str,
    cwd: &Path,
    timeout: Duration,
    limits: symphony_process::Limits,
) -> Result<Exec, String> {
    let spec = shell_spec(command).cwd(cwd).limits(limits);
    let mut proc = symphony_process::spawn(spec)
        .await
        .map_err(|e| e.to_string())?;
    let mut output = String::new();
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, proc.next_output()).await {
            Err(_) => {
                timed_out = true;
                let _ = proc.terminate_tree();
                break;
            }
            Ok(None) => break,
            Ok(Some(OutputLine::Stdout(l) | OutputLine::Stderr(l))) => {
                output.push_str(&l);
                output.push('\n');
                if output.len() > OUTPUT_KEEP * 2 {
                    // Se conserva la cola: ahí está el fallo.
                    let cut = output.len() - OUTPUT_KEEP;
                    let at = (cut..output.len())
                        .find(|i| output.is_char_boundary(*i))
                        .unwrap_or(cut);
                    output.drain(..at);
                }
            }
        }
    }
    let status = proc.wait().await;
    Ok(Exec {
        success: !timed_out && status.success(),
        exit_code: match status {
            symphony_process::ExitStatus::Exited(c) => Some(c),
            _ => None,
        },
        output,
        timed_out,
    })
}

impl Runtime {
    /// Configuración de validación del proyecto (`<raíz>/.symphony/project.toml`).
    fn project_validation(&self, project_root: &str) -> Option<ValidationConfig> {
        symphony_core::load_project(Path::new(project_root))
            .ok()
            .flatten()
            .and_then(|p| p.validation)
            .filter(|v| !v.is_empty())
    }

    fn project_root_of(&self, project: symphony_core::ProjectId) -> Option<String> {
        self.reader
            .lock()
            .ok()
            .and_then(|c| repo::get_project(&c, project).ok())
            .map(|p| p.root_path)
    }

    /// Corre un comando pasando por el scheduler si es pesado. `label` nombra al agente en
    /// las esperas; `key` identifica la operación. `Err` si no hubo turno.
    pub(crate) async fn run_scheduled(
        &self,
        key: String,
        label: String,
        command: &str,
        cwd: &Path,
        timeout: Duration,
        tag: ToolCallId,
    ) -> Result<(Exec, OpClass), String> {
        let class = classify("Bash", Some(command));
        let held = match self.scheduling.get() {
            Some(s) if class.is_heavy() => {
                let req = Request {
                    id: key.clone(),
                    tag: tag.to_string(),
                    agent: label.clone(),
                    agent_label: label,
                    class,
                    priority: 0,
                    command: Some(command.to_string()),
                    now_ms: now_ms(),
                };
                match s.submit(req) {
                    Submitted::Run { .. } => Some(s.clone()),
                    Submitted::Queued(q) => match s.wait(q, SLOT_WAIT).await {
                        Waited::Granted { .. } => Some(s.clone()),
                        Waited::TimedOut { reason } => {
                            return Err(symphony_scheduler::explain(&reason));
                        }
                    },
                }
            }
            _ => None,
        };
        let limits = crate::executor::os_limits(&self.home);
        let result = exec_shell(command, cwd, timeout, limits).await;
        if let Some(s) = held {
            s.release(&key);
        }
        result.map(|e| (e, class))
    }

    /// Valida hasta el tier `up_to` en el worktree del agente. Cada tier corre solo si el
    /// anterior pasó («Tier 3: not required yet»).
    pub async fn validate(
        &self,
        agent_id: AgentId,
        up_to: u8,
        trigger: ValidationTrigger,
    ) -> Result<Validation, String> {
        let (agent, worktree, root, run) = {
            let c = self
                .reader
                .lock()
                .map_err(|_| "lector de la base no disponible".to_string())?;
            let agent = repo::get_agent(&c, agent_id).map_err(|e| e.to_string())?;
            let wt = agent
                .worktree_id
                .ok_or_else(|| "el agente no tiene worktree".to_string())?;
            let worktree = repo::get_worktree(&c, wt).map_err(|e| e.to_string())?.path;
            let root = repo::get_project(&c, agent.project_id)
                .map_err(|e| e.to_string())?
                .root_path;
            let run = repo::runs_of(&c, agent_id)
                .map_err(|e| e.to_string())?
                .pop()
                .map(|r| r.id);
            (agent, worktree, root, run)
        };
        let Some(cfg) = self.project_validation(&root) else {
            return Err(
                "el proyecto no define comandos de validación (`[validation]` en project.toml)"
                    .into(),
            );
        };
        let up_to = up_to.clamp(1, 3);
        let timeout = Duration::from_secs(cfg.timeout_secs.max(1));
        let label = format!("Agent #{}", agent.number);
        let mut result = Validation::default();
        for tier in 1..=up_to {
            let commands = cfg.tier(tier);
            if commands.is_empty() {
                continue;
            }
            let vrun = ulid_string();
            let new = NewValidationRun {
                id: vrun.clone(),
                agent_id,
                run_id: run,
                tier,
                trigger,
            };
            self.writer
                .write(Box::new(move |t| {
                    store::insert_run(t, &new, now_ms())?;
                    Ok(())
                }))
                .await
                .map_err(|e| e.to_string())?;
            let mut checks: Vec<CheckResult> = Vec::new();
            for (i, command) in commands.iter().enumerate() {
                let check = self
                    .run_check(
                        &agent,
                        &vrun,
                        i,
                        tier,
                        command,
                        Path::new(&worktree),
                        timeout,
                        &label,
                        run,
                    )
                    .await?;
                checks.push(check);
            }
            let status = if checks.iter().any(|c| c.status == CheckStatus::Fail) {
                ValidationStatus::Fail
            } else {
                ValidationStatus::Pass
            };
            let id = vrun.clone();
            self.writer
                .write(Box::new(move |t| {
                    store::finish_run(t, &id, status, now_ms())?;
                    Ok(())
                }))
                .await
                .map_err(|e| e.to_string())?;
            result.tiers.push(TierResult {
                tier,
                status,
                checks,
            });
            if status == ValidationStatus::Fail {
                break;
            }
        }
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_check(
        &self,
        agent: &repo::Agent,
        vrun: &str,
        index: usize,
        tier: u8,
        command: &str,
        cwd: &Path,
        timeout: Duration,
        label: &str,
        run: Option<RunId>,
    ) -> Result<CheckResult, String> {
        let name = check_name(command, tier);
        let tool_call = ToolCallId::new();
        let class = classify("Bash", Some(command));
        let redacted = symphony_core::redact(command).into_owned();
        // La operación queda en `tool_calls` como cualquier comando pesado (DB §3.H).
        if let Some(run) = run {
            let call = repo::NewToolCall {
                id: tool_call,
                agent_id: agent.id,
                run_id: run,
                tool_name: format!("validation:tier{tier}"),
                command: Some(redacted.clone()),
                op_class: class.as_i64(),
            };
            let _ = self
                .writer
                .write(Box::new(move |t| {
                    repo::insert_scheduled_tool_call(t, &call, None, now_ms())?;
                    Ok(())
                }))
                .await;
        }
        let started = Instant::now();
        let outcome = self
            .run_scheduled(
                format!("val:{vrun}:{index}"),
                label.to_string(),
                command,
                cwd,
                timeout,
                tool_call,
            )
            .await;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let (status, passed, failed, summary, output, exit_code, ok) = match outcome {
            Ok((exec, _)) => {
                let out = symphony_core::redact(&exec.output).into_owned();
                let s = summarize(&out, exec.success);
                let mut text = s.text;
                if exec.timed_out {
                    text = format!(
                        "superó el tiempo máximo ({} s) y se detuvo\n{text}",
                        timeout.as_secs()
                    );
                }
                let status = if exec.success {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                };
                (
                    status,
                    s.passed,
                    s.failed,
                    text,
                    out,
                    exec.exit_code,
                    exec.success,
                )
            }
            Err(why) => (
                // Sin turno o sin arrancar: no se pudo comprobar, no se dio por bueno.
                CheckStatus::Skipped,
                None,
                None,
                format!("no se ejecutó: {why}"),
                String::new(),
                None,
                false,
            ),
        };
        let objects = symphony_object_store::ObjectStore::new(
            symphony_core::SymphonyHome::at(&self.home).objects_dir(),
        );
        let (project, agent_id) = (agent.project_id, agent.id);
        let (check_id, vrun_id) = (ulid_string(), vrun.to_string());
        let (summary_db, output_db) = (summary.clone(), output);
        let check = NewCheck {
            id: check_id.clone(),
            validation_run_id: vrun_id,
            name,
            status,
            passed,
            failed,
            summary: (!summary_db.is_empty()).then_some(summary_db),
            output_object_id: None,
        };
        self.writer
            .write(Box::new(move |t| {
                let now = now_ms();
                let mut check = check;
                if !output_db.trim().is_empty() {
                    let stored = objects
                        .put(t, output_db.as_bytes(), Some("text/plain"))
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    objects
                        .add_ref(t, &stored.hash)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let object = ContextObjectId::new();
                    repo::insert_context_object(
                        t,
                        object,
                        &format!("ctx://validation/{check_id}"),
                        project,
                        Some(agent_id),
                        run,
                        "TEST_OUTPUT",
                        &stored.hash,
                        now,
                    )?;
                    check.output_object_id = Some(object);
                }
                store::insert_check(t, &check)?;
                if run.is_some() {
                    repo::finish_tool_call(t, tool_call, ok, exit_code, now)?;
                }
                Ok(())
            }))
            .await
            .map_err(|e| e.to_string())?;
        Ok(CheckResult {
            command: redacted,
            name,
            status,
            passed,
            failed,
            summary,
            exit_code,
            duration_ms,
        })
    }

    /// El gate de la tarea (FLOW §15.2): antes de completar un agente se valida hasta el tier
    /// configurado. Devuelve `true` si ya se ocupó del agente (lo devolvió al CLI con el
    /// fallo, o lo bloqueó).
    pub(crate) async fn run_gate(&self, l: &Launch) -> bool {
        let Some(root) = self.project_root_of(l.project_id) else {
            return false;
        };
        let Some(cfg) = self.project_validation(&root) else {
            return false;
        };
        if cfg.gate == 0 {
            return false;
        }
        let (agent, task, run) = (l.agent_id, l.task_id, l.run_id);
        // TESTING mientras valida: la vista dice por qué el agente no avanza.
        let _ = self
            .writer
            .write(Box::new(move |t| {
                if repo::get_agent(t, agent).is_ok_and(|a| a.state == AgentState::Running) {
                    repo::set_agent_state(t, agent, AgentState::Testing, None, now_ms())?;
                }
                Ok(())
            }))
            .await;
        let validation = match self
            .validate(agent, cfg.gate, ValidationTrigger::Auto)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(agent = %agent, error = %e, "no se pudo validar; la tarea continúa");
                self.leave_testing(agent).await;
                return false;
            }
        };
        if validation.passed() {
            self.leave_testing(agent).await;
            return false;
        }
        // Falló: ¿cuántas van? (esta incluida)
        let attempts = self
            .reader
            .lock()
            .ok()
            .and_then(|c| store::failed_runs(&c, agent).ok())
            .unwrap_or(1);
        let max = i64::from(cfg.max_attempts.max(1));
        let feedback = validation.feedback();
        let headline = validation.headline();
        let exhausted = attempts >= max;
        let (project, provider, model) = (l.project_id, l.provider_id.clone(), l.model_id.clone());
        let health_cfg = self.bus.health_config();
        let (feedback_msg, why) = (feedback.clone(), headline.clone());
        let blocked_reason = format!("la validación falló {attempts} veces: {headline}");
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
                crate::health::on_success(t, &provider, Some(&model), now)?;
                crate::health::on_run_finished(t, run, &health_cfg, now)?;
                repo::insert_message(
                    t,
                    &repo::NewMessage {
                        id: symphony_core::MessageId::new(),
                        agent_id: agent,
                        run_id: Some(run),
                        role: repo::MessageRole::System,
                        content: Some(feedback_msg),
                        content_object_id: None,
                    },
                    now,
                )?;
                if exhausted {
                    repo::set_agent_state(
                        t,
                        agent,
                        AgentState::Blocked,
                        Some(&blocked_reason),
                        now,
                    )?;
                    repo::set_task_status(
                        t,
                        task,
                        TaskStatus::Blocked,
                        Some(&blocked_reason),
                        now,
                    )?;
                    repo::open_recovery_item(
                        t,
                        project,
                        Some(agent),
                        Some(run),
                        "VALIDATION_FAILED",
                        &blocked_reason,
                        now,
                    )?;
                } else {
                    // TESTING → RUNNING → READY: `continue_session` lo reabre con el fallo.
                    repo::set_agent_state(t, agent, AgentState::Running, None, now)?;
                    repo::set_agent_state(t, agent, AgentState::Ready, Some(&why), now)?;
                }
                Ok(())
            }))
            .await;
        if let Err(e) = result {
            tracing::error!(agent = %agent, error = %e, "no se pudo registrar la validación fallida");
            return true;
        }
        if exhausted {
            return true;
        }
        if let Err(e) = self.continue_session(agent, &feedback).await {
            tracing::warn!(agent = %agent, error = %e.0, "no se pudo devolver el fallo por la sesión; se abre un run nuevo");
            if let Err(e2) = self.restart_with_feedback(agent, &feedback).await {
                tracing::error!(agent = %agent, error = %e2.0, "no se pudo devolver el fallo de validación al agente");
            }
        }
        true
    }

    async fn leave_testing(&self, agent: AgentId) {
        let _ = self
            .writer
            .write(Box::new(move |t| {
                if repo::get_agent(t, agent).is_ok_and(|a| a.state == AgentState::Testing) {
                    repo::set_agent_state(t, agent, AgentState::Running, None, now_ms())?;
                }
                Ok(())
            }))
            .await;
    }

    /// Plan B del gate: sin sesión que continuar, un run nuevo con el mismo modelo y el fallo.
    async fn restart_with_feedback(
        &self,
        agent_id: AgentId,
        feedback: &str,
    ) -> Result<RunId, crate::executor::AgentOpError> {
        let (agent, last) = {
            let c = self.reader.lock().map_err(|_| {
                crate::executor::AgentOpError("lector de la base no disponible".into())
            })?;
            let agent = repo::get_agent(&c, agent_id)
                .map_err(|e| crate::executor::AgentOpError(e.to_string()))?;
            let last = repo::runs_of(&c, agent_id)
                .map_err(|e| crate::executor::AgentOpError(e.to_string()))?
                .pop()
                .ok_or_else(|| crate::executor::AgentOpError("el agente no tiene runs".into()))?;
            (agent, last)
        };
        let model = self
            .reader
            .lock()
            .ok()
            .and_then(|c| repo::eligible_model(&c, &last.model_id).ok())
            .ok_or_else(|| crate::executor::AgentOpError("lector no disponible".into()))?
            .map_err(|why| crate::executor::AgentOpError(format!("modelo no elegible: {why}")))?;
        self.start_successor(
            &agent,
            Some(last.id),
            model,
            "La validación de la tarea falló; el fallo vuelve al mismo agente.",
            "RESTART",
            "validation failed",
            None,
            Some(feedback),
            None,
            None,
        )
        .await
    }
}

/// Id textual (ULID) para filas que no tienen tipo propio en `core`.
fn ulid_string() -> String {
    ToolCallId::new().to_string()
}
