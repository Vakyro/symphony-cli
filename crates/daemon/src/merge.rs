//! Integración de los agentes a la rama del chat, conflictos y review agent (P08.S7,
//! FLOW §14–§15, ADR-0006: `main ← symphony/chat ← agentes`).
//!
//! Reglas: Symphony nunca escribe en `main`; integrar a la rama del chat es una cola
//! serializada que exige un preflight verde y, por defecto, la aprobación del usuario; un
//! review agent solo recomienda.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use symphony_core::{
    AgentId, AgentState, ConflictResolution, MergeStatus, ReviewRecommendation, ReviewStatus,
    TaskStatus, ValidationTrigger,
};
use symphony_git::{CHAT_BRANCH, MergeOutcome, MergePreflight, Repo};
use symphony_store::merge as store;
use symphony_store::repo;

use crate::runtime::{CreateAgent, Execution, Runtime, now_ms};

/// Una comprobación del preflight (FLOW §14.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: &'static str,
    /// `PASS`, `WARN` o `FAIL`.
    pub status: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preflight {
    pub merge_request: String,
    pub target_branch: String,
    pub checks: Vec<Check>,
    pub conflicts: Vec<String>,
    pub requires_approval: bool,
    pub approved: bool,
}

impl Preflight {
    /// Sin ningún `FAIL`: puede entrar a la cola de integración.
    pub fn ready(&self) -> bool {
        self.checks.iter().all(|c| c.status != "FAIL")
    }

    pub fn to_json(&self) -> Value {
        json!({
            "merge_request": self.merge_request,
            "target_branch": self.target_branch,
            "ready": self.ready(),
            "requires_approval": self.requires_approval,
            "approved": self.approved,
            "conflicts": self.conflicts,
            "checks": self.checks.iter().map(|c| json!({
                "name": c.name, "status": c.status, "detail": c.detail,
            })).collect::<Vec<_>>(),
        })
    }
}

/// Lo que se necesita saber de un agente para integrarlo.
struct Ctx {
    agent: repo::Agent,
    task: repo::Task,
    root: String,
    branch: String,
    base_ref: String,
}

impl Runtime {
    fn merge_ctx(&self, agent_id: AgentId) -> Result<Ctx, String> {
        let c = self
            .reader
            .lock()
            .map_err(|_| "lector de la base no disponible".to_string())?;
        let agent = repo::get_agent(&c, agent_id).map_err(|e| e.to_string())?;
        let task = repo::get_task(&c, agent.task_id).map_err(|e| e.to_string())?;
        let root = repo::get_project(&c, agent.project_id)
            .map_err(|e| e.to_string())?
            .root_path;
        let wt = agent
            .worktree_id
            .ok_or_else(|| "el agente no tiene worktree".to_string())?;
        let wt = repo::get_worktree(&c, wt).map_err(|e| e.to_string())?;
        Ok(Ctx {
            agent,
            task,
            root,
            branch: wt.branch,
            base_ref: wt.base_ref,
        })
    }

    /// Worktree del chat del proyecto, si existe (ahí se hace el merge: la rama del chat está
    /// extraída en él).
    fn chat_worktree(&self, project: symphony_core::ProjectId) -> Option<(AgentId, String)> {
        let c = self.reader.lock().ok()?;
        let chat = repo::chat_agent(&c, project).ok()??;
        let wt = repo::get_worktree(&c, chat.worktree_id?).ok()?;
        Some((chat.id, wt.path))
    }

    fn require_approval(&self, root: &str) -> bool {
        symphony_core::load_project(Path::new(root))
            .ok()
            .flatten()
            .and_then(|p| p.merge)
            .unwrap_or_default()
            .require_approval
    }

    /// Preflight de integración (FLOW §14.2). Deja la solicitud en `READY` o `BLOCKED` con el
    /// resultado de cada comprobación y los archivos en conflicto.
    pub async fn merge_prepare(
        &self,
        agent_id: AgentId,
        run_validation: bool,
    ) -> Result<Preflight, String> {
        let ctx = self.merge_ctx(agent_id)?;
        let mut checks: Vec<Check> = Vec::new();
        let mut conflicts: Vec<String> = Vec::new();
        let root = ctx.root.clone();
        let repo_root = Repo::at(&root);

        // Destino: la rama del chat (nunca `main`).
        let target_ok = {
            let (r, b) = (Repo::at(&root), CHAT_BRANCH.to_string());
            tokio::task::spawn_blocking(move || r.branch_exists(&b).unwrap_or(false))
                .await
                .map_err(|e| e.to_string())?
        };
        if target_ok {
            checks.push(Check {
                name: "target",
                status: "PASS",
                detail: format!("se integra a `{CHAT_BRANCH}`"),
            });
        } else {
            checks.push(Check {
                name: "target",
                status: "FAIL",
                detail: format!(
                    "la rama `{CHAT_BRANCH}` no existe todavía: abre `symphony` para crear el chat"
                ),
            });
        }

        // La tarea terminó.
        let done = ctx.task.status == TaskStatus::Done || ctx.agent.state == AgentState::Completed;
        checks.push(Check {
            name: "task",
            status: if done { "PASS" } else { "FAIL" },
            detail: if done {
                format!("{} completa", ctx.task.code)
            } else {
                format!(
                    "{} sigue {}: espera a que termine",
                    ctx.task.code,
                    ctx.agent.state.as_str()
                )
            },
        });

        // Dependencias.
        let unmet = self
            .reader
            .lock()
            .ok()
            .and_then(|c| symphony_store::deps::unmet_dependencies(&c, ctx.task.id).ok())
            .unwrap_or_default();
        checks.push(Check {
            name: "dependencies",
            status: if unmet.is_empty() { "PASS" } else { "FAIL" },
            detail: if unmet.is_empty() {
                "todas cumplidas".into()
            } else {
                symphony_store::deps::waiting_phrase(&unmet)
            },
        });

        // Validación requerida (hasta el tier 3: el merge es el gate de los tiers pesados).
        let has_validation = symphony_core::load_project(Path::new(&root))
            .ok()
            .flatten()
            .and_then(|p| p.effective_validation())
            .is_some();
        if !has_validation {
            checks.push(Check {
                name: "validation",
                status: "PASS",
                detail: "el proyecto no define comandos de validación".into(),
            });
        } else if run_validation && done {
            match self
                .validate(agent_id, 3, ValidationTrigger::MergeGate)
                .await
            {
                Ok(v) if v.passed() && v.has_skipped() => checks.push(Check {
                    name: "validation",
                    status: "WARN",
                    detail: "pasó, pero un check no pudo correr (¿un plugin caído?): revisa la vista de validación".into(),
                }),
                Ok(v) if v.passed() => checks.push(Check {
                    name: "validation",
                    status: "PASS",
                    detail: format!("{} tier(s) pasaron", v.tiers.len()),
                }),
                Ok(v) => checks.push(Check {
                    name: "validation",
                    status: "FAIL",
                    detail: v.headline(),
                }),
                Err(e) => checks.push(Check {
                    name: "validation",
                    status: "WARN",
                    detail: format!("no se pudo validar: {e}"),
                }),
            }
        } else {
            checks.push(Check {
                name: "validation",
                status: "WARN",
                detail: "no se corrió la validación del merge".into(),
            });
        }

        // Conflictos con la rama del chat.
        if target_ok {
            let (r, ours, theirs) = (repo_root, CHAT_BRANCH.to_string(), ctx.branch.clone());
            let pre = tokio::task::spawn_blocking(move || r.merge_preflight(&ours, &theirs))
                .await
                .map_err(|e| e.to_string())?;
            match pre {
                Ok(MergePreflight::Clean { .. }) => checks.push(Check {
                    name: "conflicts",
                    status: "PASS",
                    detail: "ninguno".into(),
                }),
                Ok(MergePreflight::Conflicts { paths }) => {
                    checks.push(Check {
                        name: "conflicts",
                        status: "FAIL",
                        detail: format!("choca en {}", paths.join(", ")),
                    });
                    conflicts = paths;
                }
                Err(e) => checks.push(Check {
                    name: "conflicts",
                    status: "FAIL",
                    detail: format!("no se pudo comprobar: {e}"),
                }),
            }
        }

        // Política del usuario.
        let requires = self.require_approval(&root);
        let (mr_id, approved) = self
            .persist_preflight(&ctx, &checks, &conflicts, requires)
            .await?;
        checks.push(Check {
            name: "policy",
            status: if !requires || approved {
                "PASS"
            } else {
                "WARN"
            },
            detail: if !requires {
                "integración automática permitida".into()
            } else if approved {
                "aprobado por el usuario".into()
            } else {
                "requiere la aprobación del usuario".into()
            },
        });
        Ok(Preflight {
            merge_request: mr_id,
            target_branch: CHAT_BRANCH.to_string(),
            checks,
            conflicts,
            requires_approval: requires,
            approved,
        })
    }

    /// Guarda el resultado del preflight: estado de la solicitud, conflictos y recovery item.
    async fn persist_preflight(
        &self,
        ctx: &Ctx,
        checks: &[Check],
        conflicts: &[String],
        requires: bool,
    ) -> Result<(String, bool), String> {
        // Qué otro agente tocó cada archivo en conflicto (para el Conflict Review).
        let others = self.agents_touching(ctx, conflicts).await;
        let failed = checks.iter().any(|c| c.status == "FAIL");
        let status = if failed {
            MergeStatus::Blocked
        } else {
            MergeStatus::Ready
        };
        let json = json!({
            "checks": checks.iter().map(|c| json!({
                "name": c.name, "status": c.status, "detail": c.detail,
            })).collect::<Vec<_>>(),
            "conflicts": conflicts,
        })
        .to_string();
        let slot: Arc<Mutex<Option<(String, bool)>>> = Arc::default();
        let out = slot.clone();
        let (agent, task, project) = (ctx.agent.id, ctx.task.id, ctx.agent.project_id);
        let conflict_rows: Vec<(String, Option<AgentId>)> = conflicts
            .iter()
            .map(|p| (p.clone(), others.get(p).copied()))
            .collect();
        let detail = format!("conflicto de merge en {}", conflicts.join(", "));
        self.writer
            .write(Box::new(move |t| {
                let now = now_ms();
                let mr = store::open_or_reuse(t, agent, task, CHAT_BRANCH, requires, now)?;
                store::set_status(t, &mr.id, status, Some(&json))?;
                store::replace_conflicts(t, &mr.id, &conflict_rows)?;
                if !conflict_rows.is_empty() {
                    let open: i64 = t.query_row(
                        "SELECT COUNT(*) FROM recovery_items WHERE agent_id = ?1
                         AND kind = 'MERGE_CONFLICT' AND status = 'OPEN'",
                        [agent.to_string()],
                        |r| r.get(0),
                    )?;
                    if open == 0 {
                        repo::open_recovery_item(
                            t,
                            project,
                            Some(agent),
                            None,
                            "MERGE_CONFLICT",
                            &detail,
                            now,
                        )?;
                    }
                }
                if let Ok(mut s) = out.lock() {
                    *s = Some((mr.id.clone(), mr.approved_at.is_some()));
                }
                Ok(())
            }))
            .await
            .map_err(|e| e.to_string())?;
        slot.lock()
            .map_err(|_| "estado interno envenenado".to_string())?
            .take()
            .ok_or_else(|| "la escritura no devolvió resultado".into())
    }

    /// Para cada archivo, otro agente del proyecto cuya rama también lo cambió.
    async fn agents_touching(
        &self,
        ctx: &Ctx,
        paths: &[String],
    ) -> std::collections::HashMap<String, AgentId> {
        let mut found = std::collections::HashMap::new();
        if paths.is_empty() {
            return found;
        }
        let others: Vec<(AgentId, String)> = self
            .reader
            .lock()
            .ok()
            .and_then(|c| {
                let mut stmt = c
                    .prepare(
                        "SELECT a.id, w.branch FROM agents a JOIN worktrees w ON w.id = a.worktree_id
                         WHERE a.project_id = ?1 AND a.id <> ?2 AND a.state <> 'CANCELLED'
                           AND a.task_id NOT IN (SELECT id FROM tasks WHERE project_id = ?1 AND code = 'CHAT')",
                    )
                    .ok()?;
                let rows = stmt
                    .query_map(
                        [ctx.agent.project_id.to_string(), ctx.agent.id.to_string()],
                        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                    )
                    .ok()?;
                Some(
                    rows.flatten()
                        .filter_map(|(id, b)| Some((id.parse().ok()?, b)))
                        .collect(),
                )
            })
            .unwrap_or_default();
        let (root, base) = (ctx.root.clone(), ctx.base_ref.clone());
        let wanted: Vec<String> = paths.to_vec();
        let result = tokio::task::spawn_blocking(move || {
            let repo = Repo::at(root);
            let mut out = std::collections::HashMap::new();
            for (id, branch) in others {
                if let Ok(changed) = repo.changed_paths(&base, &branch) {
                    for p in &wanted {
                        if changed.contains(p) {
                            out.entry(p.clone()).or_insert(id);
                        }
                    }
                }
            }
            out
        })
        .await
        .unwrap_or_default();
        found.extend(result);
        found
    }

    /// El usuario aprueba integrar a la rama del chat.
    pub async fn merge_approve(&self, mr_id: &str) -> Result<(), String> {
        let id = mr_id.to_string();
        let exists = self
            .reader
            .lock()
            .ok()
            .and_then(|c| store::get_mr(&c, &id).ok());
        let Some(mr) = exists else {
            return Err(format!("no existe la solicitud de integración `{mr_id}`"));
        };
        if matches!(mr.status, MergeStatus::Merged | MergeStatus::Aborted) {
            return Err(format!("la solicitud ya está {}", mr.status.as_str()));
        }
        self.writer
            .write(Box::new(move |t| {
                store::approve(t, &id, now_ms())?;
                Ok(())
            }))
            .await
            .map_err(|e| e.to_string())
    }

    /// Abandona la integración (FLOW §14.3: «abandon merge»).
    pub async fn merge_abort(&self, mr_id: &str) -> Result<(), String> {
        let id = mr_id.to_string();
        self.writer
            .write(Box::new(move |t| {
                let now = now_ms();
                store::set_status(t, &id, MergeStatus::Aborted, None)?;
                store::resolve_conflicts(t, &id, ConflictResolution::Aborted, now)?;
                Ok(())
            }))
            .await
            .map_err(|e| e.to_string())
    }

    /// El usuario marca los conflictos como resueltos a mano (en el worktree del agente) o
    /// asignados a un revisor; la siguiente `merge_prepare` los vuelve a comprobar.
    pub async fn merge_mark_resolved(
        &self,
        mr_id: &str,
        resolution: ConflictResolution,
    ) -> Result<usize, String> {
        let id = mr_id.to_string();
        let slot: Arc<Mutex<usize>> = Arc::default();
        let out = slot.clone();
        self.writer
            .write(Box::new(move |t| {
                let n = store::resolve_conflicts(t, &id, resolution, now_ms())?;
                if let Ok(mut s) = out.lock() {
                    *s = n;
                }
                Ok(())
            }))
            .await
            .map_err(|e| e.to_string())?;
        Ok(slot.lock().map(|n| *n).unwrap_or(0))
    }

    /// Integra el agente a la rama del chat: una a la vez, solo con preflight verde y, si la
    /// política lo pide, aprobada. Devuelve el commit del merge.
    pub async fn merge_integrate(&self, mr_id: &str) -> Result<String, String> {
        let _turn = self.integration.lock().await;
        let id = mr_id.to_string();
        let mr = {
            let conn = self
                .reader
                .lock()
                .map_err(|_| "lector de la base no disponible".to_string())?;
            store::get_mr(&conn, &id).map_err(|e| e.to_string())?
        };
        if !matches!(mr.status, MergeStatus::Ready | MergeStatus::Queued) {
            return Err(format!(
                "la solicitud está {}: corre el preflight (`merge.prepare`) hasta que quede READY",
                mr.status.as_str()
            ));
        }
        if mr.requires_approval && mr.approved_at.is_none() {
            return Err(
                "la integración necesita tu aprobación (`merge.approve`): Symphony no integra sin política explícita"
                    .into(),
            );
        }
        let ctx = self.merge_ctx(mr.agent_id)?;
        let Some((chat_agent, chat_wt)) = self.chat_worktree(ctx.agent.project_id) else {
            return Err("el proyecto no tiene chat: abre `symphony` para crearlo".into());
        };
        if self.is_live(chat_agent) {
            return Err(
                "el chat está en un turno: reintenta cuando termine para no mezclar cambios".into(),
            );
        }
        let message = format!("merge: {} {}", ctx.task.code, ctx.task.title);
        let (wt, branch) = (chat_wt.clone(), ctx.branch.clone());
        let outcome = tokio::task::spawn_blocking(move || {
            let r = Repo::at(wt);
            if !r.is_clean()? {
                return Err(symphony_git::GitError::Failed {
                    args: "merge".into(),
                    code: "dirty".into(),
                    stderr: "el worktree del chat tiene cambios sin commitear".into(),
                });
            }
            r.merge_no_ff(&branch, &message)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        match outcome {
            MergeOutcome::Merged { commit } => {
                self.record_integration(&mr.id, &ctx, chat_agent, &commit)
                    .await?;
                Ok(commit)
            }
            MergeOutcome::UpToDate => {
                let head = tokio::task::spawn_blocking(move || Repo::at(chat_wt).head_commit())
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string())?;
                self.record_integration(&mr.id, &ctx, chat_agent, &head)
                    .await?;
                Ok(head)
            }
            MergeOutcome::Conflicts { paths } => {
                // Apareció un conflicto entre el preflight y la integración (otra integración).
                let others = self.agents_touching(&ctx, &paths).await;
                let rows: Vec<(String, Option<AgentId>)> = paths
                    .iter()
                    .map(|p| (p.clone(), others.get(p).copied()))
                    .collect();
                let (mrid, agent, project) = (mr.id.clone(), ctx.agent.id, ctx.agent.project_id);
                let detail = format!("conflicto de merge en {}", paths.join(", "));
                let _ = self
                    .writer
                    .write(Box::new(move |t| {
                        let now = now_ms();
                        store::set_status(t, &mrid, MergeStatus::Blocked, None)?;
                        store::replace_conflicts(t, &mrid, &rows)?;
                        repo::open_recovery_item(
                            t,
                            project,
                            Some(agent),
                            None,
                            "MERGE_CONFLICT",
                            &detail,
                            now,
                        )?;
                        Ok(())
                    }))
                    .await;
                Err(format!("conflicto al integrar en {}", paths.join(", ")))
            }
        }
    }

    async fn record_integration(
        &self,
        mr: &str,
        ctx: &Ctx,
        chat_agent: AgentId,
        commit: &str,
    ) -> Result<(), String> {
        let (mr, commit_s) = (mr.to_string(), commit.to_string());
        let note = format!(
            "Integrado {} «{}» en `{CHAT_BRANCH}` ({}).",
            ctx.task.code,
            ctx.task.title,
            &commit[..commit.len().min(8)]
        );
        let (agent, project, task) = (ctx.agent.id, ctx.agent.project_id, ctx.task.id);
        self.writer
            .write(Box::new(move |t| {
                let now = now_ms();
                store::mark_merged(t, &mr, &commit_s, now)?;
                repo::resolve_recovery_items(t, agent, "MANUAL", now)?;
                repo::insert_message(
                    t,
                    &repo::NewMessage {
                        id: symphony_core::MessageId::new(),
                        agent_id: chat_agent,
                        run_id: None,
                        role: repo::MessageRole::System,
                        content: Some(note),
                        content_object_id: None,
                    },
                    now,
                )?;
                let _ = (project, task);
                Ok(())
            }))
            .await
            .map_err(|e| e.to_string())
    }

    // --- review agent ---------------------------------------------------------------

    /// Crea un review agent para `target` (FLOW §15.3): recibe la tarea, el diff, el
    /// resultado de la validación y las restricciones; devuelve findings y una
    /// recomendación, nunca mergea.
    pub async fn review_start(
        &self,
        target: AgentId,
        model: Option<String>,
        profile: Option<String>,
    ) -> Result<AgentId, String> {
        let ctx = self.merge_ctx(target)?;
        let (root, base, branch) = (ctx.root.clone(), ctx.base_ref.clone(), ctx.branch.clone());
        let diff = tokio::task::spawn_blocking(move || {
            Repo::at(root)
                .diff_range(&base, &branch)
                .unwrap_or_default()
        })
        .await
        .map_err(|e| e.to_string())?;
        let validation = self
            .reader
            .lock()
            .ok()
            .and_then(|c| symphony_store::validation::runs_of(&c, target, 3).ok())
            .map(|runs| {
                runs.iter()
                    .map(|r| {
                        let checks: Vec<String> = r
                            .checks
                            .iter()
                            .map(|k| {
                                format!(
                                    "{} {}{}",
                                    k.name,
                                    k.status,
                                    k.summary
                                        .as_deref()
                                        .map(|s| format!(" — {}", s.lines().next().unwrap_or("")))
                                        .unwrap_or_default()
                                )
                            })
                            .collect();
                        format!("tier {}: {} ({})", r.tier, r.status, checks.join("; "))
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "(sin validación registrada)".into());
        let constraints = std::fs::read_to_string(Path::new(&ctx.root).join("CONSTRAINTS.md"))
            .map(|t| t.chars().take(4_000).collect::<String>())
            .unwrap_or_default();
        let prompt = review_prompt(&ctx.task, &diff, &validation, &constraints);
        let execution = match (model, profile) {
            (Some(m), _) => Execution::Exact(m),
            (None, Some(p)) => Execution::Profile(p),
            (None, None) => Execution::Profile("@review".into()),
        };
        let req = CreateAgent {
            project_root: Path::new(&ctx.root).to_path_buf(),
            title: format!("Review de {}: {}", ctx.task.code, ctx.task.title),
            description: Some(prompt),
            execution,
            failover: symphony_core::FailoverPolicy::Any,
            context_mode: symphony_core::ContextMode::Balanced,
            priority: 0,
            chat: false,
            depends_on: Vec::new(),
            review: true,
        };
        let created = self.create_agent(req).await.map_err(|e| e.to_string())?;
        let reviewer = created.agent_id;
        let mr = self
            .reader
            .lock()
            .ok()
            .and_then(|c| store::latest_mr(&c, target).ok().flatten())
            .map(|m| m.id);
        self.writer
            .write(Box::new(move |t| {
                store::insert_review(t, target, reviewer, mr.as_deref(), now_ms())?;
                Ok(())
            }))
            .await
            .map_err(|e| e.to_string())?;
        // Un revisor rápido puede haber terminado antes de que existiera su fila.
        let finished = self
            .reader
            .lock()
            .ok()
            .and_then(|c| repo::get_agent(&c, reviewer).ok())
            .map(|a| a.state);
        match finished {
            Some(AgentState::Completed) => self.on_agent_finished_review(reviewer, false).await,
            Some(AgentState::Failed | AgentState::Cancelled) => {
                self.on_agent_finished_review(reviewer, true).await;
            }
            _ => {}
        }
        Ok(reviewer)
    }

    /// Un agente terminó: si era un revisor, se lee su veredicto.
    pub(crate) async fn on_agent_finished_review(&self, agent: AgentId, failed: bool) {
        let review = self
            .reader
            .lock()
            .ok()
            .and_then(|c| store::review_by_reviewer(&c, agent).ok().flatten());
        let Some(review) = review else { return };
        if review.status != ReviewStatus::Running {
            return;
        }
        let objects = symphony_object_store::ObjectStore::new(
            symphony_core::SymphonyHome::at(&self.home).objects_dir(),
        );
        let text = self
            .reader
            .lock()
            .ok()
            .and_then(|c| {
                let msgs = repo::agent_messages(&c, agent, None).ok()?;
                let last: Vec<String> = msgs
                    .iter()
                    .filter(|m| m.role == "ASSISTANT")
                    .map(|m| crate::handoff::message_text(&c, &objects, m))
                    .collect();
                Some(last.join("\n\n"))
            })
            .unwrap_or_default();
        let (status, rec, findings) = if failed {
            (
                ReviewStatus::Failed,
                None,
                Some(json!({"error": "el revisor terminó con error"}).to_string()),
            )
        } else {
            match parse_review(&text) {
                Some((rec, findings)) => (ReviewStatus::Done, Some(rec), Some(findings)),
                None => (
                    ReviewStatus::Failed,
                    None,
                    Some(
                        json!({"error": "el revisor no devolvió un bloque ```review válido"})
                            .to_string(),
                    ),
                ),
            }
        };
        let id = review.id;
        let _ = self
            .writer
            .write(Box::new(move |t| {
                store::finish_review(t, &id, status, rec, findings.as_deref(), now_ms())?;
                Ok(())
            }))
            .await;
    }
}

/// El prompt del revisor: solo tarea, diff, validación y restricciones (FLOW §15.3).
fn review_prompt(task: &repo::Task, diff: &str, validation: &str, constraints: &str) -> String {
    const MAX_DIFF: usize = 60_000;
    let (diff_text, cut) = if diff.chars().count() > MAX_DIFF {
        (diff.chars().take(MAX_DIFF).collect::<String>(), true)
    } else {
        (diff.to_string(), false)
    };
    let mut out = format!(
        "Eres el revisor de la tarea {} «{}». No modifiques archivos ni integres nada: solo \
         revisa y recomienda.\n\n\
         Responde al final con UN bloque con este formato exacto:\n\
         ```review\n\
         {{\"recommendation\":\"APPROVE|REQUEST_CHANGES|REJECT\",\"findings\":[{{\"severity\":\"high|medium|low\",\"file\":\"ruta\",\"text\":\"qué y por qué\"}}]}}\n\
         ```\n\n\
         ## Tarea\n{}\n\n## Validación\n{validation}\n",
        task.code,
        task.title,
        task.description.as_deref().unwrap_or(&task.title),
    );
    if !constraints.trim().is_empty() {
        out.push_str(&format!("\n## Restricciones del proyecto\n{constraints}\n"));
    }
    out.push_str("\n## Diff\n");
    out.push_str(&diff_text);
    if cut {
        out.push_str("\n… (diff recortado)\n");
    }
    out
}

/// Lee el bloque ```review del texto del revisor.
fn parse_review(text: &str) -> Option<(ReviewRecommendation, String)> {
    let body = {
        let start = text.rfind("```review")?;
        let rest = &text[start + "```review".len()..];
        let end = rest.find("```")?;
        rest[..end].trim().to_string()
    };
    let v: Value = serde_json::from_str(&body).ok()?;
    let rec: ReviewRecommendation = v
        .get("recommendation")?
        .as_str()?
        .trim()
        .to_ascii_uppercase()
        .parse()
        .ok()?;
    let findings = v.get("findings").cloned().unwrap_or_else(|| json!([]));
    Some((rec, findings.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_review_block_is_read_and_only_the_last_one_counts() {
        let text = "Mi análisis.\n```review\n{\"recommendation\":\"approve\",\"findings\":[]}\n```\n\
                    Corrijo:\n```review\n{\"recommendation\":\"REQUEST_CHANGES\",\"findings\":[{\"severity\":\"high\",\"file\":\"a.js\",\"text\":\"falta el caso 401\"}]}\n```\n";
        let (rec, findings) = parse_review(text).unwrap();
        assert_eq!(rec, ReviewRecommendation::RequestChanges);
        assert!(findings.contains("falta el caso 401"));
        assert!(parse_review("sin bloque").is_none());
        assert!(parse_review("```review\nno es json\n```").is_none());
        assert!(
            parse_review("```review\n{\"recommendation\":\"MERGE_IT\"}\n```").is_none(),
            "solo recomienda: nada de merge"
        );
    }

    #[test]
    fn the_reviewer_prompt_carries_task_diff_validation_and_limits_the_diff() {
        let t = repo::Task {
            id: symphony_core::TaskId::new(),
            project_id: symphony_core::ProjectId::new(),
            code: "T-4".into(),
            title: "Arreglar 401".into(),
            description: Some("detalle".into()),
            status: TaskStatus::Done,
            status_reason: None,
            priority: 0,
        };
        let big = "+x\n".repeat(30_000);
        let p = review_prompt(&t, &big, "tier 1: PASS", "no tocar src/legacy");
        assert!(
            p.contains("T-4") && p.contains("tier 1: PASS") && p.contains("no tocar src/legacy")
        );
        assert!(p.contains("```review"));
        assert!(p.contains("diff recortado"));
        assert!(p.chars().count() < 70_000);
    }
}
