//! Checkpoints incrementales (P06.S3, IDEA §5.5). Nunca se generan al fallar:
//! se actualizan después de cada evento significativo (archivo modificado,
//! herramienta terminada, fin de turno).
//!
//! En el camino del bus solo se acumula en memoria (barato, no toca git); un
//! worker toma la foto de git (HEAD, diff, archivos) y escribe el checkpoint.
//! Varios eventos seguidos del mismo agente se agrupan en un solo checkpoint.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use symphony_adapter_common::{AgentEvent, ToolKind};
use symphony_core::{AgentId, CheckpointId, RunId, redact};
use symphony_git::{FileStatus, Repo};
use symphony_object_store::ObjectStore;
use symphony_store::{WriterHandle, repo};
use tokio::sync::{mpsc, oneshot};

use crate::bus::BusEvent;

/// Checkpoints que se conservan por agente, además de los usados en handoffs.
pub const KEEP: usize = 20;
/// Largo máximo de `plan_tail` (se guarda el final del mensaje).
const PLAN_TAIL_MAX: usize = 2_000;
const FAILURES_MAX: usize = 5;

/// Lo que se sabe de un agente desde su último checkpoint.
#[derive(Debug, Clone, Default)]
struct Progress {
    run: Option<RunId>,
    last_assistant: Option<String>,
    current_step: Option<String>,
    next_step: Option<String>,
    pending_command: Option<String>,
    last_command: Option<Value>,
    commands_run: u32,
    failures: VecDeque<String>,
    /// Hay un checkpoint pedido que el worker todavía no tomó.
    dirty: bool,
}

impl Progress {
    fn fail(&mut self, what: String) {
        if self.failures.len() == FAILURES_MAX {
            self.failures.pop_front();
        }
        self.failures.push_back(what);
    }

    /// Actualiza el estado; `true` si el evento amerita checkpoint.
    fn apply(&mut self, event: &AgentEvent) -> bool {
        match event {
            AgentEvent::AssistantText { text } => {
                self.note_assistant(text);
                false
            }
            AgentEvent::TurnFinished { last_message } => {
                if let Some(text) = last_message {
                    self.note_assistant(text);
                }
                true
            }
            AgentEvent::ToolRequested {
                tool,
                kind,
                command,
                ..
            } => {
                let command = command.as_deref().map(|c| redact(c).into_owned());
                self.current_step = Some(match &command {
                    Some(c) => format!("{tool}: {c}"),
                    None => tool.clone(),
                });
                if *kind == ToolKind::Command {
                    self.pending_command = command;
                }
                false
            }
            AgentEvent::ToolFinished {
                tool,
                kind,
                ok,
                exit_code,
                ..
            } => {
                if *kind == ToolKind::Command {
                    let command = self.pending_command.take().unwrap_or_else(|| tool.clone());
                    self.commands_run += 1;
                    if !ok {
                        let code = exit_code
                            .map(|c| format!(" (código {c})"))
                            .unwrap_or_default();
                        self.fail(format!("`{command}` falló{code}"));
                    }
                    self.last_command =
                        Some(json!({"command": command, "ok": ok, "exit_code": exit_code}));
                } else if !ok {
                    self.fail(format!("{tool} falló"));
                }
                true
            }
            AgentEvent::FileModified { .. } => true,
            AgentEvent::ProviderError(e) => {
                self.fail(format!("error del proveedor: {}", e.failure_type.as_str()));
                false
            }
            _ => false,
        }
    }

    fn note_assistant(&mut self, text: &str) {
        if let Some(next) = next_step_in(text) {
            self.next_step = Some(next);
        }
        self.last_assistant = Some(text.to_string());
    }
}

/// "Qué sigue" explícito en un mensaje: el primer TODO sin marcar o una línea
/// `Next:`/`Siguiente:`. Determinista: sin LLM en el camino crítico.
fn next_step_in(text: &str) -> Option<String> {
    const PREFIXES: [&str; 6] = [
        "next step:",
        "next:",
        "siguiente paso:",
        "siguiente:",
        "próximo paso:",
        "proximo paso:",
    ];
    text.lines().find_map(|line| {
        let line = line.trim();
        let lower = line.to_lowercase();
        let rest = ["- [ ]", "* [ ]"]
            .iter()
            .find_map(|p| line.strip_prefix(p))
            .or_else(|| {
                PREFIXES
                    .iter()
                    .find(|p| lower.starts_with(**p))
                    .and_then(|p| line.get(p.len()..))
            })?;
        let rest = rest.trim();
        (!rest.is_empty()).then(|| rest.chars().take(200).collect())
    })
}

/// Los últimos `max` caracteres (la "cola" del plan).
fn tail(text: &str, max: usize) -> String {
    let n = text.chars().count();
    text.chars().skip(n.saturating_sub(max)).collect()
}

enum Job {
    Agent(AgentId),
    Idle(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct Checkpointer {
    progress: Arc<Mutex<HashMap<AgentId, Progress>>>,
    jobs: mpsc::UnboundedSender<Job>,
}

impl Checkpointer {
    /// Arranca el worker (necesita un runtime de tokio).
    pub fn start(writer: WriterHandle, objects: ObjectStore, keep: usize) -> Self {
        let progress: Arc<Mutex<HashMap<AgentId, Progress>>> = Arc::default();
        // Sin límite pero acotada: hay a lo sumo un trabajo pendiente por agente (`dirty`).
        let (jobs, rx) = mpsc::unbounded_channel();
        tokio::spawn(worker(writer, objects, keep, progress.clone(), rx));
        Self { progress, jobs }
    }

    /// Anota el evento; si es significativo, pide un checkpoint.
    pub fn observe(&self, ev: &BusEvent) {
        let Some(agent) = ev
            .agent_id
            .as_deref()
            .and_then(|a| a.parse::<AgentId>().ok())
        else {
            return;
        };
        let Ok(mut all) = self.progress.lock() else {
            return;
        };
        let p = all.entry(agent).or_default();
        if let Some(run) = ev.run_id.as_deref().and_then(|r| r.parse().ok()) {
            p.run = Some(run);
        }
        if p.apply(&ev.event) && !p.dirty {
            p.dirty = true;
            let _ = self.jobs.send(Job::Agent(agent));
        }
    }

    /// Espera a que el worker tome los checkpoints pedidos hasta ahora.
    pub async fn idle(&self) {
        let (tx, rx) = oneshot::channel();
        if self.jobs.send(Job::Idle(tx)).is_ok() {
            let _ = rx.await;
        }
    }
}

async fn worker(
    writer: WriterHandle,
    objects: ObjectStore,
    keep: usize,
    progress: Arc<Mutex<HashMap<AgentId, Progress>>>,
    mut jobs: mpsc::UnboundedReceiver<Job>,
) {
    while let Some(job) = jobs.recv().await {
        match job {
            Job::Idle(done) => {
                let _ = done.send(());
            }
            Job::Agent(agent) => {
                if let Err(e) = take(&writer, &objects, keep, &progress, agent).await {
                    tracing::warn!(agent = %agent, error = %e, "no se pudo tomar el checkpoint");
                }
            }
        }
    }
}

/// Foto de git del worktree.
struct GitSnapshot {
    head: String,
    diff: String,
    files: Vec<Value>,
}

fn snapshot(worktree: PathBuf, base: &str) -> Result<GitSnapshot, symphony_git::GitError> {
    let repo = Repo::at(worktree);
    let mut files: Vec<Value> = repo
        .diff_numstat(base)?
        .into_iter()
        .map(|s| json!({"path": s.path, "added": s.added, "deleted": s.deleted}))
        .collect();
    files.extend(repo.status()?.into_iter().filter_map(|s| match s {
        FileStatus::Untracked { path } => Some(json!({"path": path, "untracked": true})),
        _ => None,
    }));
    Ok(GitSnapshot {
        head: repo.head_commit()?,
        // El diff puede viajar a otro proveedor en un handoff: nunca con secretos.
        diff: redact(&repo.diff(base)?).into_owned(),
        files,
    })
}

async fn take(
    writer: &WriterHandle,
    objects: &ObjectStore,
    keep: usize,
    progress: &Mutex<HashMap<AgentId, Progress>>,
    agent: AgentId,
) -> Result<(), String> {
    let now_state = {
        let mut all = progress
            .lock()
            .map_err(|_| "estado envenenado".to_string())?;
        let Some(p) = all.get_mut(&agent) else {
            return Ok(());
        };
        p.dirty = false;
        p.clone()
    };

    let (tx, rx) = oneshot::channel();
    writer
        .write(Box::new(move |t| {
            let _ = tx.send(repo::checkpoint_base(t, agent)?);
            Ok(())
        }))
        .await
        .map_err(|e| e.to_string())?;
    let Some(base) = rx.await.map_err(|e| e.to_string())? else {
        return Ok(());
    };

    let (path, base_ref) = (PathBuf::from(&base.worktree_path), base.base_ref.clone());
    let git = tokio::task::spawn_blocking(move || snapshot(path, &base_ref))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    // El archivo del blob se escribe acá, fuera del hilo del writer.
    let stored = if git.diff.is_empty() {
        None
    } else {
        Some(
            objects
                .write_object(git.diff.as_bytes())
                .map_err(|e| e.to_string())?,
        )
    };

    let summary = json!({
        "files_touched": git.files,
        "last_command": now_state.last_command,
        "commands_run": now_state.commands_run,
        "failures": now_state.failures,
    });
    let checkpoint = repo::NewCheckpoint {
        id: CheckpointId::new(),
        agent_id: agent,
        run_id: now_state.run.or(base.open_run),
        objective: base.objective,
        plan_tail: now_state
            .last_assistant
            .as_deref()
            .map(|t| tail(&redact(t), PLAN_TAIL_MAX)),
        current_step: now_state.current_step,
        next_step: now_state.next_step.or(base.next_step),
        head_commit: Some(git.head),
        diff_object_id: None,
        summary_json: Some(summary.to_string()),
    };
    let (objects, project, diff) = (objects.clone(), base.project_id, git.diff);
    writer
        .write(Box::new(move |t| {
            let sql = |e: symphony_object_store::ObjectError| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(e))
            };
            let now = now_ms();
            let mut checkpoint = checkpoint;
            if stored.is_some() {
                // `put` ve que el archivo ya existe y solo registra la fila del blob.
                let blob = objects
                    .put(t, diff.as_bytes(), Some("text/x-diff"))
                    .map_err(sql)?;
                let (id, created) =
                    repo::diff_object(t, project, agent, checkpoint.run_id, &blob.hash, now)?;
                if created {
                    objects.add_ref(t, &blob.hash).map_err(sql)?;
                }
                checkpoint.diff_object_id = Some(id);
            }
            repo::insert_checkpoint(t, &checkpoint, now)?;
            for hash in repo::prune_checkpoints(t, agent, keep)? {
                objects.release(t, &hash).map_err(sql)?;
            }
            Ok(())
        }))
        .await
        .map_err(|e| e.to_string())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_step_comes_from_explicit_markers_only() {
        assert_eq!(
            next_step_in("Hecho X.\n- [x] a\n- [ ] correr los tests de auth\n- [ ] b"),
            Some("correr los tests de auth".into())
        );
        assert_eq!(
            next_step_in("Listo.\nSiguiente: revisar el middleware"),
            Some("revisar el middleware".into())
        );
        assert_eq!(
            next_step_in("NEXT STEP: fix refresh"),
            Some("fix refresh".into())
        );
        assert_eq!(next_step_in("Arreglé el bug, todo pasa."), None);
        assert_eq!(next_step_in("- [ ]   "), None);
    }

    #[test]
    fn tail_keeps_the_end_on_char_boundaries() {
        assert_eq!(tail("ñandú", 3), "ndú");
        assert_eq!(tail("ab", 10), "ab");
    }

    #[test]
    fn failures_keep_the_last_five() {
        let mut p = Progress::default();
        for i in 0..8 {
            p.apply(&AgentEvent::ToolRequested {
                tool_use_id: None,
                tool: "Bash".into(),
                kind: ToolKind::Command,
                command: Some(format!("cmd {i}")),
            });
            assert!(p.apply(&AgentEvent::ToolFinished {
                tool_use_id: None,
                tool: "Bash".into(),
                kind: ToolKind::Command,
                ok: false,
                exit_code: Some(1),
            }));
        }
        assert_eq!(p.commands_run, 8);
        assert_eq!(p.failures.len(), FAILURES_MAX);
        assert_eq!(p.failures[0], "`cmd 3` falló (código 1)");
        assert_eq!(p.current_step.as_deref(), Some("Bash: cmd 7"));
    }
}
