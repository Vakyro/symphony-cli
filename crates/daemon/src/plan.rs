//! Plan por fases y tareas de cada agente (ADR-0011, ADR-0013 D1–D2, P08.S5).
//!
//! El agente escribe `.symphony/plan/<task_id>.md` en su worktree y trabaja una fase por
//! sesión. Cuando el CLI sale con la fase terminada, el daemon cierra el run con
//! `PHASE_DONE` y abre uno nuevo con el mismo modelo y un prompt acotado a la fase. Al
//! terminar la tarea el archivo se borra (es temporal); antes, lo hecho y lo que falló
//! quedan en la conversación del agente.

use std::path::{Path, PathBuf};

use symphony_core::{PhaseStart, PlanConfig, TaskId};
use symphony_scheduler::{Plan, Progress, TaskMark};

/// Dónde vive el plan dentro del worktree.
pub const PLAN_DIR: &str = ".symphony/plan";

/// Patrón que se excluye de git (de forma local) para que el plan no entre en commits ni
/// en el diff del agente.
pub const PLAN_EXCLUDE: &str = ".symphony/plan/";

pub fn plan_rel_path(task: TaskId) -> String {
    format!("{PLAN_DIR}/{task}.md")
}

pub fn plan_file(worktree: &Path, task: TaskId) -> PathBuf {
    worktree.join(PLAN_DIR).join(format!("{task}.md"))
}

/// Lee y valida el plan del worktree. `None` si no existe (el agente no lo escribió).
pub fn read_plan(worktree: &Path, task: TaskId, min_tasks: usize) -> Option<Plan> {
    let text = std::fs::read_to_string(plan_file(worktree, task)).ok()?;
    let plan = Plan::parse(&text).normalized(min_tasks);
    // Un plan sin fases o con dependencias imposibles no guía nada: se trata como ausente.
    plan.validate().ok().map(|()| plan)
}

/// Lo que separa la tarea de las reglas del plan en el primer prompt.
const INSTRUCTIONS_MARK: &str = "\n\n---\nInstrucciones de Symphony (plan por fases):\n";

/// La tarea del usuario sin las reglas del plan: un handoff lleva lo que el usuario pidió,
/// no el boilerplate que Symphony le agregó al prompt.
pub fn without_instructions(text: &str) -> &str {
    text.split_once(INSTRUCTIONS_MARK)
        .map_or(text, |(task, _)| task)
}

/// El prompt de la primera sesión: la tarea y las reglas del plan por fases.
pub fn planning_prompt(objective: &str, task: TaskId, min_tasks: u32) -> String {
    let path = plan_rel_path(task);
    format!(
        "{objective}{INSTRUCTIONS_MARK}\
         1. Antes de tocar código, escribe el plan en `{path}` con este formato:\n\
         ```\n\
         # Plan: <título corto>\n\
         ## Fase 1: <objetivo verificable>\n\
         Verifica: <comando que prueba la fase>\n\
         - [ ] <tarea concreta>\n\
         - [ ] <otra tarea>\n\
         ## Fase 2: ...\n\
         ```\n\
         Cada fase agrupa al menos {min_tasks} tareas relacionadas. Si el proyecto tiene \
         `AGENTS.md`, `CLAUDE.md` o `docs/`, léelos y síguelos; si no existen, no inventes \
         convenciones.\n\
         2. Ejecuta SOLO la fase 1 en esta sesión. Al terminar cada tarea márcala `- [x]`. \
         Si una tarea falla dos veces, márcala `- [!]`, escribe debajo una nota `  > por qué \
         falló` y detente.\n\
         3. Al terminar la fase deja el plan actualizado y termina tu turno: Symphony abrirá \
         una sesión nueva con la fase siguiente. No la empieces tú.\n\
         4. Si la tarea es pequeña, el plan puede tener una sola fase: hazla y termina.\n\
         5. `{path}` es temporal y no se commitea; no lo borres, lo borra Symphony al terminar."
    )
}

/// El prompt de una fase nueva: solo lo que esa sesión necesita (ADR-0011 §3).
pub fn phase_prompt(objective: &str, plan: &Plan, index: usize, task: TaskId) -> String {
    use std::fmt::Write as _;
    let total = plan.phases.len();
    let phase = &plan.phases[index];
    let mut out = String::new();
    let first_line = objective.lines().next().unwrap_or(objective).trim();
    let _ = writeln!(
        out,
        "Continúas la tarea «{first_line}» en una sesión nueva: fase {} de {total}: {}.",
        index + 1,
        phase.title
    );
    let _ = writeln!(
        out,
        "\nEstado del plan (`{}`; lo hecho ya está en el worktree):\n",
        plan_rel_path(task)
    );
    out.push_str(&plan.render());
    let _ = writeln!(out, "Pendiente en esta fase:");
    for t in &phase.tasks {
        if t.mark != TaskMark::Done {
            let _ = writeln!(out, "- {}", t.text);
        }
    }
    if let Some(v) = &phase.verify {
        let _ = writeln!(out, "\nVerifica la fase con: {v}");
    }
    let _ = writeln!(
        out,
        "\nEjecuta SOLO esta fase. Marca cada tarea `- [x]` al terminarla; si una falla dos \
         veces, márcala `- [!]` con una nota debajo y detente. Al terminar la fase, deja el \
         plan actualizado y termina tu turno."
    );
    out
}

/// Qué sigue tras una sesión que terminó bien.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// No hay plan (el CLI no lo escribió): la tarea termina como siempre.
    NoPlan,
    /// Todo el plan está hecho.
    Complete(Plan),
    /// Quedan fases: toca la `index` (0-based).
    NextPhase { index: usize, plan: Plan },
    /// Una tarea del plan quedó marcada como fallida.
    Failed { index: usize, plan: Plan },
}

pub fn next_after_run(worktree: &Path, task: TaskId, cfg: &PlanConfig) -> Next {
    let min = cfg.min_tasks_per_phase as usize;
    match read_plan(worktree, task, min) {
        None => Next::NoPlan,
        Some(plan) => match plan.progress() {
            Progress::Done => Next::Complete(plan),
            Progress::Phase { index } => Next::NextPhase { index, plan },
            Progress::Failed { index } => Next::Failed { index, plan },
        },
    }
}

/// Texto de lo hecho y lo que falló, para dejarlo en la conversación del agente antes de
/// borrar el archivo.
pub fn summary(plan: &Plan) -> String {
    let (done, total) = plan.counts();
    let mut out = format!("Plan «{}»: {done}/{total} tareas hechas.\n\n", plan.title);
    out.push_str(&plan.render());
    out
}

/// «Falló la tarea X de la fase N» para la razón del agente.
pub fn failure_reason(plan: &Plan, index: usize) -> String {
    let phase = &plan.phases[index];
    let failed: Vec<String> = phase
        .tasks
        .iter()
        .filter(|t| t.mark == TaskMark::Failed)
        .map(|t| match t.notes.first() {
            Some(n) => format!("{} ({n})", t.text),
            None => t.text.clone(),
        })
        .collect();
    format!(
        "el plan se detuvo en la fase {} «{}»: {}",
        index + 1,
        phase.title,
        failed.join("; ")
    )
}

/// ¿El plan de esta config pide confirmar cada fase?
pub fn asks_before_phase(cfg: &PlanConfig) -> bool {
    cfg.phase_start == PhaseStart::Ask
}

// --- el driver: el daemon avanza el plan --------------------------------------------

use symphony_core::{AgentId, AgentState, MessageId, RunEndReason, RunStatus, TaskStatus};
use symphony_store::repo;

use crate::executor::Launch;
use crate::runtime::{Runtime, now_ms};

/// Por qué un agente no pudo pasar a la fase siguiente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseError(pub String);

impl Runtime {
    /// `[plan]` de `config.toml`; se relee en cada uso (editarlo aplica a la fase siguiente).
    pub(crate) fn plan_config(&self) -> PlanConfig {
        symphony_core::load_or_create(&symphony_core::SymphonyHome::user_for(&self.home))
            .map(|c| c.plan)
            .unwrap_or_default()
    }

    /// Primer prompt de un agente: con las reglas del plan por fases, salvo en el chat o con
    /// el plan apagado.
    pub(crate) fn planned_prompt(&self, task: TaskId, is_chat: bool, objective: &str) -> String {
        let cfg = self.plan_config();
        if is_chat || !cfg.enabled {
            return objective.to_string();
        }
        planning_prompt(objective, task, cfg.min_tasks_per_phase)
    }

    /// El chat y los review agents no llevan plan por fases ni gate de validación.
    pub(crate) fn task_is_chat(&self, task: TaskId) -> bool {
        self.reader.lock().ok().is_some_and(|c| {
            repo::get_task(&c, task).is_ok_and(|t| t.code == repo::CHAT_TASK_CODE)
                || repo::task_kind(&c, task).is_ok_and(|k| k == "REVIEW")
        })
    }

    /// El CLI salió bien: ¿el plan pide otra fase, se detuvo o terminó? Devuelve `true` si
    /// ya se ocupó del agente (no hay que completarlo).
    pub(crate) async fn advance_plan(&self, l: &Launch) -> bool {
        let cfg = self.plan_config();
        if !cfg.enabled || self.task_is_chat(l.task_id) {
            return false;
        }
        match next_after_run(&l.worktree, l.task_id, &cfg) {
            Next::NoPlan => false,
            Next::Complete(plan) => {
                self.archive_plan(l, &plan).await;
                false
            }
            Next::Failed { index, plan } => {
                self.stop_on_failed_plan(l, index, &plan).await;
                true
            }
            Next::NextPhase { index, plan } => {
                match self.close_phase_and_continue(l, index, &plan, &cfg).await {
                    Ok(()) => true,
                    Err(e) => {
                        tracing::warn!(agent = %l.agent_id, error = %e.0, "no se pudo pasar a la fase siguiente");
                        false
                    }
                }
            }
        }
    }

    /// Deja lo hecho y lo que falló en la conversación del agente y borra el plan (es
    /// temporal, ADR-0013 D1).
    async fn archive_plan(&self, l: &Launch, plan: &Plan) {
        self.record_plan_summary(l.agent_id, Some(l.run_id), plan)
            .await;
        let file = plan_file(&l.worktree, l.task_id);
        let _ = std::fs::remove_file(&file);
        if let Some(dir) = file.parent() {
            // Solo si quedó vacío: `.symphony/` puede tener otras cosas del usuario.
            let _ = std::fs::remove_dir(dir);
            if let Some(up) = dir.parent() {
                let _ = std::fs::remove_dir(up);
            }
        }
    }

    async fn record_plan_summary(
        &self,
        agent: AgentId,
        run: Option<symphony_core::RunId>,
        plan: &Plan,
    ) {
        let text = summary(plan);
        let _ = self
            .writer
            .write(Box::new(move |t| {
                repo::insert_message(
                    t,
                    &repo::NewMessage {
                        id: MessageId::new(),
                        agent_id: agent,
                        run_id: run,
                        role: repo::MessageRole::System,
                        content: Some(text),
                        content_object_id: None,
                    },
                    now_ms(),
                )?;
                Ok(())
            }))
            .await;
    }

    /// Una tarea del plan quedó fallida: el agente se detiene y lo reporta (ADR-0011 §6).
    async fn stop_on_failed_plan(&self, l: &Launch, index: usize, plan: &Plan) {
        let reason = failure_reason(plan, index);
        self.record_plan_summary(l.agent_id, Some(l.run_id), plan)
            .await;
        let (project, agent, run) = (l.project_id, l.agent_id, l.run_id);
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
                crate::health::on_success(t, &provider, Some(&model), now)?;
                crate::health::on_run_finished(t, run, &health_cfg, now)?;
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
            tracing::error!(agent = %agent, error = %e, "no se pudo cerrar el run con el plan fallido");
        }
    }

    /// Cierra el run de la fase terminada (`PHASE_DONE`, `HANDED_OFF`) y abre el siguiente
    /// con el mismo proveedor y modelo, o deja al agente listo si se pidió confirmación.
    async fn close_phase_and_continue(
        &self,
        l: &Launch,
        index: usize,
        plan: &Plan,
        cfg: &PlanConfig,
    ) -> Result<(), PhaseError> {
        self.record_plan_summary(l.agent_id, Some(l.run_id), plan)
            .await;
        let (agent, task, run) = (l.agent_id, l.task_id, l.run_id);
        let (provider, model) = (l.provider_id.clone(), l.model_id.clone());
        let health_cfg = self.bus.health_config();
        let asks = asks_before_phase(cfg);
        let phase_title = plan.phases[index].title.clone();
        self.writer
            .write(Box::new(move |t| {
                let now = now_ms();
                repo::close_run(
                    t,
                    run,
                    RunStatus::HandedOff,
                    RunEndReason::PhaseDone,
                    Some(0),
                    now,
                )?;
                repo::set_handoff_outcome(t, run, "CONTINUED")?;
                crate::health::on_success(t, &provider, Some(&model), now)?;
                crate::health::on_run_finished(t, run, &health_cfg, now)?;
                if asks {
                    let why = format!(
                        "fase {} «{phase_title}» lista para arrancar: confirma",
                        index + 1
                    );
                    repo::set_agent_state(t, agent, AgentState::Ready, Some(&why), now)?;
                    repo::set_task_status(t, task, TaskStatus::Ready, None, now)?;
                }
                Ok(())
            }))
            .await
            .map_err(|e| PhaseError(e.to_string()))?;
        if asks {
            return Ok(());
        }
        self.start_phase_run(l.agent_id, Some(l.run_id), index, plan, &l.model_id)
            .await
    }

    /// Abre el run de la fase `index` con `model_id` y el prompt acotado a la fase.
    async fn start_phase_run(
        &self,
        agent_id: AgentId,
        from_run: Option<symphony_core::RunId>,
        index: usize,
        plan: &Plan,
        model_id: &str,
    ) -> Result<(), PhaseError> {
        let (agent, objective, model) = {
            let c = self
                .reader
                .lock()
                .map_err(|_| PhaseError("lector de la base no disponible".into()))?;
            let agent = repo::get_agent(&c, agent_id).map_err(|e| PhaseError(e.to_string()))?;
            let objective = repo::latest_checkpoint(&c, agent_id)
                .map_err(|e| PhaseError(e.to_string()))?
                .map(|c| c.objective)
                .unwrap_or_default();
            let model = repo::eligible_model(&c, model_id)
                .map_err(|e| PhaseError(e.to_string()))?
                .map_err(|reason| {
                    PhaseError(format!("`{model_id}` ya no es elegible: {reason}"))
                })?;
            (agent, objective, model)
        };
        let prompt = phase_prompt(&objective, plan, index, agent.task_id);
        let reason = format!(
            "Fase {} de {} terminada; sesión nueva con el mismo modelo.",
            index,
            plan.phases.len()
        );
        self.start_successor(
            &agent,
            from_run,
            model,
            &reason,
            "RESTART",
            None,
            None,
            None,
            Some(prompt),
        )
        .await
        .map(|_| ())
        .map_err(|e| PhaseError(e.0))
    }

    /// El usuario confirma la fase siguiente (`phase_start = "ask"`).
    pub async fn start_next_phase(
        &self,
        agent_id: AgentId,
    ) -> Result<symphony_core::RunId, PhaseError> {
        let (agent, last) = {
            let c = self
                .reader
                .lock()
                .map_err(|_| PhaseError("lector de la base no disponible".into()))?;
            let agent = repo::get_agent(&c, agent_id).map_err(|e| PhaseError(e.to_string()))?;
            let last = repo::runs_of(&c, agent_id)
                .map_err(|e| PhaseError(e.to_string()))?
                .pop();
            (agent, last)
        };
        if agent.state != AgentState::Ready {
            return Err(PhaseError(format!(
                "el agente #{} está {} y no espera confirmar una fase",
                agent.number,
                agent.state.as_str()
            )));
        }
        let worktree = {
            let c = self
                .reader
                .lock()
                .map_err(|_| PhaseError("lector de la base no disponible".into()))?;
            let wt = agent
                .worktree_id
                .ok_or_else(|| PhaseError("el agente no tiene worktree".into()))?;
            repo::get_worktree(&c, wt)
                .map_err(|e| PhaseError(e.to_string()))?
                .path
        };
        let cfg = self.plan_config();
        let Next::NextPhase { index, plan } =
            next_after_run(Path::new(&worktree), agent.task_id, &cfg)
        else {
            return Err(PhaseError("el plan no tiene una fase pendiente".into()));
        };
        let last = last.ok_or_else(|| PhaseError("el agente no tiene runs".into()))?;
        self.start_phase_run(agent_id, Some(last.id), index, &plan, &last.model_id)
            .await?;
        let run = self
            .reader
            .lock()
            .ok()
            .and_then(|c| repo::runs_of(&c, agent_id).ok())
            .and_then(|mut r| r.pop())
            .map(|r| r.id)
            .ok_or_else(|| PhaseError("no se abrió el run".into()))?;
        Ok(run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> TaskId {
        TaskId::new()
    }

    #[test]
    fn the_planning_prompt_names_the_file_and_the_rules() {
        let t = task();
        let p = planning_prompt("Arreglar tokens", t, 2);
        assert!(p.starts_with("Arreglar tokens"));
        assert!(p.contains(&plan_rel_path(t)));
        assert!(p.contains("SOLO la fase 1"));
        assert!(p.contains("al menos 2 tareas"));
    }

    #[test]
    fn handoffs_do_not_carry_the_plan_boilerplate() {
        let t = task();
        let wrapped = planning_prompt("Arreglar tokens\n\ncon detalle", t, 2);
        assert_eq!(
            without_instructions(&wrapped),
            "Arreglar tokens\n\ncon detalle"
        );
        assert_eq!(without_instructions("sin reglas"), "sin reglas");
    }

    #[test]
    fn the_phase_prompt_carries_only_the_phase_and_the_plan() {
        let t = task();
        let plan = Plan::parse(
            "# Plan: tokens\n## Fase 1: preparar\n- [x] uno\n- [x] dos\n## Fase 2: arreglar\nVerifica: cargo test\n- [ ] tres\n- [ ] cuatro\n",
        );
        let p = phase_prompt("Arreglar tokens\n\ndetalle largo", &plan, 1, t);
        assert!(p.contains("fase 2 de 2: arreglar"));
        assert!(
            p.contains("«Arreglar tokens»"),
            "solo la primera línea del objetivo"
        );
        assert!(!p.contains("detalle largo"));
        assert!(p.contains("- tres") && p.contains("- cuatro"));
        assert!(!p.contains("Pendiente en esta fase:\n- uno"));
        assert!(p.contains("Verifica la fase con: cargo test"));
    }

    #[test]
    fn next_after_run_reads_the_file_and_decides() {
        let dir = tempfile::tempdir().unwrap();
        let t = task();
        let cfg = PlanConfig::default();
        assert_eq!(next_after_run(dir.path(), t, &cfg), Next::NoPlan);

        let write = |text: &str| {
            let f = plan_file(dir.path(), t);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, text).unwrap();
        };
        write("# Plan: x\n## Fase 1: a\n- [x] 1\n- [x] 2\n## Fase 2: b\n- [ ] 3\n- [ ] 4\n");
        assert!(matches!(
            next_after_run(dir.path(), t, &cfg),
            Next::NextPhase { index: 1, .. }
        ));
        write("# Plan: x\n## Fase 1: a\n- [x] 1\n- [!] 2\n  > no compila\n");
        match next_after_run(dir.path(), t, &cfg) {
            Next::Failed { index: 0, plan } => {
                assert!(failure_reason(&plan, 0).contains("no compila"));
            }
            other => panic!("{other:?}"),
        }
        write("# Plan: x\n## Fase 1: a\n- [x] 1\n- [x] 2\n");
        assert!(matches!(
            next_after_run(dir.path(), t, &cfg),
            Next::Complete(_)
        ));
        // Un plan roto (dependencia que no existe) se trata como ausente.
        write("# Plan: x\n## Fase 1: a\n- [ ] [A] uno (needs: Z)\n- [ ] dos\n");
        assert_eq!(next_after_run(dir.path(), t, &cfg), Next::NoPlan);
    }

    #[test]
    fn single_task_phases_are_merged_with_the_next_before_deciding() {
        let dir = tempfile::tempdir().unwrap();
        let t = task();
        let f = plan_file(dir.path(), t);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        // Dos fases de una tarea: con el mínimo de 2 son una sola, y la primera ya está hecha.
        std::fs::write(
            &f,
            "# Plan: x\n## Fase 1: a\n- [x] 1\n## Fase 2: b\n- [ ] 2\n",
        )
        .unwrap();
        match next_after_run(dir.path(), t, &PlanConfig::default()) {
            Next::NextPhase { index: 0, plan } => assert_eq!(plan.phases.len(), 1),
            other => panic!("{other:?}"),
        }
    }
}
