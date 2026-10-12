//! Integración de la rama del chat a `main` (P11.5.S6, ADR-0006 §3 y §5).
//!
//! `main ← symphony/chat`: es el único camino por el que Symphony escribe en `main`, y solo
//! con acción explícita del usuario y después de un review agent. Reglas:
//! - el repo principal tiene que tener `main` extraída y limpia (no se cambia de rama ni se
//!   toca un trabajo a medias del usuario);
//! - el chat no está en un turno y su worktree está limpio;
//! - nada de conflictos (el merge se prueba antes con `merge-tree`);
//! - la revisión recomienda `APPROVE`. Si el revisor falló o no devolvió un veredicto, el usuario
//!   puede integrar igual, pero lo dice (`skip_review`); si pidió cambios, no se integra salvo
//!   que el usuario lo diga igual de explícito.

use std::path::Path;

use serde_json::{Value, json};
use symphony_core::{AgentId, MergeStatus, ReviewRecommendation, ReviewStatus, ValidationTrigger};
use symphony_git::{CHAT_BRANCH, MergeOutcome, MergePreflight, Repo};
use symphony_store::merge as store;
use symphony_store::repo;

use crate::merge::Check;
use crate::runtime::{Runtime, now_ms};

/// Resultado del preflight chat → `main`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMerge {
    pub source: String,
    pub target: String,
    /// Commits que la rama del chat tiene y `main` no.
    pub ahead: u32,
    pub checks: Vec<Check>,
    pub conflicts: Vec<String>,
    /// La última revisión del chat (`status`, `recommendation`, `findings`), si hay.
    pub review: Option<Value>,
}

impl ChatMerge {
    /// Ningún `FAIL`.
    pub fn ready(&self) -> bool {
        self.checks.iter().all(|c| c.status != "FAIL")
    }

    pub fn to_json(&self) -> Value {
        json!({
            "source": self.source,
            "target": self.target,
            "ahead": self.ahead,
            "ready": self.ready(),
            "conflicts": self.conflicts,
            "review": self.review,
            "checks": self.checks.iter().map(|c| json!({
                "name": c.name, "status": c.status, "detail": c.detail,
            })).collect::<Vec<_>>(),
        })
    }
}

/// Lo que hace falta saber del chat de un proyecto.
struct ChatCtx {
    project: repo::Project,
    chat: repo::Agent,
    task: repo::Task,
    worktree: String,
}

fn check(name: &'static str, status: &'static str, detail: impl Into<String>) -> Check {
    Check {
        name,
        status,
        detail: detail.into(),
    }
}

impl Runtime {
    fn chat_merge_ctx(&self, root: &Path) -> Result<ChatCtx, String> {
        let c = self
            .reader
            .lock()
            .map_err(|_| "lector de la base no disponible".to_string())?;
        // El proyecto se registra con la raíz que ve git (como `create_agent`): sirve cualquier
        // carpeta dentro del repo.
        let top = Repo::discover(root)
            .map(|r| r.root().display().to_string())
            .unwrap_or_else(|_| root.display().to_string());
        let project = repo::project_by_root(&c, &top)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "el proyecto no está registrado en Symphony".to_string())?;
        let chat = repo::chat_agent(&c, project.id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "el proyecto no tiene chat: abre `symphony` para crearlo".to_string())?;
        let task = repo::get_task(&c, chat.task_id).map_err(|e| e.to_string())?;
        let wt = chat
            .worktree_id
            .ok_or_else(|| "el chat no tiene worktree".to_string())?;
        let worktree = repo::get_worktree(&c, wt).map_err(|e| e.to_string())?.path;
        Ok(ChatCtx {
            project,
            chat,
            task,
            worktree,
        })
    }

    /// La última revisión del chat y qué dice de la rama de ahora.
    fn chat_review_check(
        &self,
        ctx: &ChatCtx,
        head_time_ms: Option<i64>,
    ) -> (Check, Option<Value>) {
        let latest = self
            .reader
            .lock()
            .ok()
            .and_then(|c| store::reviews_of_target(&c, ctx.chat.id).ok())
            .and_then(|r| r.into_iter().next());
        let Some(r) = latest else {
            return (
                check(
                    "review",
                    "WARN",
                    "sin revisión: pídela con `symphony chat merge --review`",
                ),
                None,
            );
        };
        let findings: Value = r
            .findings_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or(Value::Null);
        let json = json!({
            "reviewer_agent_id": r.reviewer_agent_id.to_string(),
            "status": r.status.as_str(),
            "recommendation": r.recommendation.map(|x| x.as_str()),
            "findings": findings,
            "finished_at": r.finished_at,
        });
        // Una revisión anterior al último commit no vale para lo que se va a integrar.
        let stale = match (r.finished_at, head_time_ms) {
            (Some(done), Some(head)) => head > done,
            _ => false,
        };
        let c = match (r.status, r.recommendation) {
            (ReviewStatus::Running, _) => check("review", "WARN", "la revisión sigue en curso"),
            (ReviewStatus::Failed, _) => {
                let why = findings["error"].as_str().unwrap_or("sin detalle");
                check(
                    "review",
                    "WARN",
                    format!("la revisión falló ({why}): puedes integrar con `--skip-review`"),
                )
            }
            (_, _) if stale => check(
                "review",
                "WARN",
                "la revisión es anterior a los últimos cambios del chat: pídela de nuevo",
            ),
            (ReviewStatus::Done, Some(ReviewRecommendation::Approve)) => {
                check("review", "PASS", "el revisor recomienda integrar")
            }
            (ReviewStatus::Done, Some(rec)) => {
                let n = findings["findings"].as_array().map_or(0, Vec::len);
                check(
                    "review",
                    "FAIL",
                    format!("el revisor recomienda {} ({n} hallazgos)", rec.as_str()),
                )
            }
            (ReviewStatus::Done, None) => check("review", "WARN", "la revisión no trae veredicto"),
        };
        (c, Some(json))
    }

    /// Preflight chat → `main` (FLOW §14.2 aplicado al chat). No cambia nada.
    pub async fn chat_merge_prepare(
        &self,
        root: &Path,
        run_validation: bool,
    ) -> Result<ChatMerge, String> {
        let ctx = self.chat_merge_ctx(root)?;
        let (target, project_root) = (
            ctx.project.default_branch.clone(),
            ctx.project.root_path.clone(),
        );
        if target.starts_with('-') || target.contains("..") {
            return Err(format!("la rama principal `{target}` no es válida"));
        }
        let mut checks = Vec::new();
        let mut conflicts = Vec::new();

        // El chat no está en un turno y su worktree está limpio.
        let busy = self.is_live(ctx.chat.id);
        checks.push(if busy {
            check(
                "chat",
                "FAIL",
                "el chat está en un turno: espera a que termine",
            )
        } else {
            check("chat", "PASS", "sin turno en curso")
        });

        // Git: todo en un solo hilo bloqueante.
        let (pr, wt, tgt) = (project_root.clone(), ctx.worktree.clone(), target.clone());
        let git = tokio::task::spawn_blocking(move || {
            let main_repo = Repo::at(&pr);
            let chat_repo = Repo::at(&wt);
            let on = main_repo.current_branch().ok().flatten();
            let main_clean = main_repo.is_clean().unwrap_or(false);
            let chat_clean = chat_repo.is_clean().unwrap_or(false);
            let ahead = main_repo.commits_ahead(&tgt, CHAT_BRANCH).unwrap_or(0);
            let pre = main_repo.merge_preflight(&tgt, CHAT_BRANCH);
            // Hora (s) del último commit del chat, para saber si la revisión quedó vieja.
            let head_time = main_repo.commit_time(CHAT_BRANCH).ok();
            (on, main_clean, chat_clean, ahead, pre, head_time)
        })
        .await
        .map_err(|e| e.to_string())?;
        let (on, main_clean, chat_clean, ahead, pre, head_time) = git;

        checks.push(if on.as_deref() == Some(target.as_str()) {
            check("main_checked_out", "PASS", format!("`{target}` está extraída en el repo principal"))
        } else {
            check(
                "main_checked_out",
                "FAIL",
                format!(
                    "el repo principal está en {}: cambia a `{target}` para integrar (Symphony no cambia tu rama)",
                    on.as_deref().map_or("HEAD suelto".to_string(), |b| format!("`{b}`"))
                ),
            )
        });
        checks.push(if main_clean {
            check("main_clean", "PASS", "sin cambios sin commitear en tu repo")
        } else {
            check(
                "main_clean",
                "FAIL",
                "tu repo principal tiene cambios sin commitear: confírmalos o guárdalos antes",
            )
        });
        checks.push(if chat_clean {
            check("chat_clean", "PASS", "el worktree del chat está limpio")
        } else {
            check(
                "chat_clean",
                "FAIL",
                "el worktree del chat tiene cambios sin commitear",
            )
        });
        checks.push(if ahead > 0 {
            check(
                "ahead",
                "PASS",
                format!("`{CHAT_BRANCH}` tiene {ahead} commit(s) que `{target}` no tiene"),
            )
        } else {
            check(
                "ahead",
                "FAIL",
                format!("no hay nada que integrar: `{target}` ya incluye al chat"),
            )
        });
        match pre {
            Ok(MergePreflight::Clean { .. }) => {
                checks.push(check("conflicts", "PASS", "ninguno"));
            }
            Ok(MergePreflight::Conflicts { paths }) => {
                checks.push(check(
                    "conflicts",
                    "FAIL",
                    format!("choca en {}", paths.join(", ")),
                ));
                conflicts = paths;
            }
            Err(e) => checks.push(check(
                "conflicts",
                "FAIL",
                format!("no se pudo comprobar: {e}"),
            )),
        }

        // Validación del proyecto (tiers hasta el 3) sobre el worktree del chat.
        let has_validation = symphony_core::load_project(Path::new(&project_root))
            .ok()
            .flatten()
            .and_then(|p| p.validation)
            .is_some_and(|v| !v.is_empty());
        if !has_validation {
            checks.push(check(
                "validation",
                "PASS",
                "el proyecto no define comandos de validación",
            ));
        } else if run_validation && !busy {
            match self
                .validate(ctx.chat.id, 3, ValidationTrigger::MergeGate)
                .await
            {
                Ok(v) if v.passed() => checks.push(check(
                    "validation",
                    "PASS",
                    format!("{} tier(s) pasaron", v.tiers.len()),
                )),
                Ok(v) => checks.push(check("validation", "FAIL", v.headline())),
                Err(e) => checks.push(check(
                    "validation",
                    "WARN",
                    format!("no se pudo validar: {e}"),
                )),
            }
        } else {
            checks.push(check("validation", "WARN", "no se corrió la validación"));
        }

        let (review_check, review) =
            self.chat_review_check(&ctx, head_time.map(|s| s.saturating_mul(1000)));
        checks.push(review_check);
        Ok(ChatMerge {
            source: CHAT_BRANCH.to_string(),
            target,
            ahead,
            checks,
            conflicts,
            review,
        })
    }

    /// Pide la revisión del chat antes de integrarlo a `main` (el revisor solo recomienda).
    pub async fn chat_merge_review(
        &self,
        root: &Path,
        model: Option<String>,
        profile: Option<String>,
    ) -> Result<AgentId, String> {
        let ctx = self.chat_merge_ctx(root)?;
        if self.is_live(ctx.chat.id) {
            return Err("el chat está en un turno: espera a que termine para revisarlo".into());
        }
        self.review_start(ctx.chat.id, model, profile).await
    }

    /// Integra el chat en `main` con un commit de merge. Exige `confirm` (el usuario lo dijo),
    /// un preflight sin `FAIL` y, salvo `skip_review`, una revisión que recomiende `APPROVE`.
    /// Devuelve el commit del merge.
    pub async fn chat_merge_integrate(
        &self,
        root: &Path,
        confirm: bool,
        skip_review: bool,
    ) -> Result<String, String> {
        if !confirm {
            return Err(
                "integrar el chat en `main` necesita tu confirmación explícita (`--yes`)".into(),
            );
        }
        let _turn = self.integration.lock().await;
        let pre = self.chat_merge_prepare(root, true).await?;
        let failing: Vec<String> = pre
            .checks
            .iter()
            .filter(|c| c.status == "FAIL" && !(skip_review && c.name == "review"))
            .map(|c| format!("{}: {}", c.name, c.detail))
            .collect();
        if !failing.is_empty() {
            return Err(format!("no se puede integrar: {}", failing.join("; ")));
        }
        if !skip_review
            && let Some(c) = pre.checks.iter().find(|c| c.name == "review")
            && c.status != "PASS"
        {
            return Err(format!(
                "falta una revisión que recomiende integrar ({}); pídela con `--review` o integra \
                 con `--skip-review` si decides hacerlo sin ella",
                c.detail
            ));
        }
        let ctx = self.chat_merge_ctx(root)?;
        let (pr, target) = (ctx.project.root_path.clone(), pre.target.clone());
        let message = format!("merge: {CHAT_BRANCH} → {target} (Symphony)");
        let outcome = tokio::task::spawn_blocking(move || {
            let r = Repo::at(&pr);
            // Otra vez, dentro del hilo: entre el preflight y el merge pudo cambiar algo.
            if r.current_branch().ok().flatten().as_deref() != Some(target.as_str()) {
                return Err(symphony_git::GitError::Failed {
                    args: "merge".into(),
                    code: "branch".into(),
                    stderr: format!("el repo principal ya no está en `{target}`"),
                });
            }
            if !r.is_clean()? {
                return Err(symphony_git::GitError::Failed {
                    args: "merge".into(),
                    code: "dirty".into(),
                    stderr: "tu repo principal tiene cambios sin commitear".into(),
                });
            }
            r.merge_no_ff(CHAT_BRANCH, &message)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        match outcome {
            MergeOutcome::Merged { commit } => {
                self.record_chat_merge(&ctx, &pre.target, &commit).await?;
                Ok(commit)
            }
            MergeOutcome::UpToDate => Err(format!("`{}` ya incluye al chat", pre.target)),
            MergeOutcome::Conflicts { paths } => Err(format!(
                "conflicto al integrar en {} (`{}` quedó como estaba)",
                paths.join(", "),
                pre.target
            )),
        }
    }

    /// Deja constancia: la solicitud de integración (historial) y una nota en el chat.
    async fn record_chat_merge(
        &self,
        ctx: &ChatCtx,
        target: &str,
        commit: &str,
    ) -> Result<(), String> {
        let (chat, task, target_s, commit_s) = (
            ctx.chat.id,
            ctx.task.id,
            target.to_string(),
            commit.to_string(),
        );
        let note = format!(
            "Integrado `{CHAT_BRANCH}` en `{target}` ({}).",
            &commit[..commit.len().min(8)]
        );
        self.writer
            .write(Box::new(move |t| {
                let now = now_ms();
                let mr = store::open_or_reuse(t, chat, task, &target_s, true, now)?;
                store::approve(t, &mr.id, now)?;
                store::set_status(t, &mr.id, MergeStatus::Ready, None)?;
                store::mark_merged(t, &mr.id, &commit_s, now)?;
                repo::insert_message(
                    t,
                    &repo::NewMessage {
                        id: symphony_core::MessageId::new(),
                        agent_id: chat,
                        run_id: None,
                        role: repo::MessageRole::System,
                        content: Some(note),
                        content_object_id: None,
                    },
                    now,
                )?;
                Ok(())
            }))
            .await
            .map_err(|e| e.to_string())
    }
}
