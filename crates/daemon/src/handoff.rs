//! Junta los datos del handoff (P06.S4): último checkpoint válido + git vivo del
//! worktree (ADR-0004, H2: git manda) y los pasa al assembler de `symphony-context`.

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use serde_json::Value;
use symphony_context::handoff::{self, HandoffInput, LastCommand, NewFile};
use symphony_core::{AgentId, CheckpointId, ContextMode, redact};
use symphony_git::{FileStatus, Repo};
use symphony_store::repo;

/// Archivos nuevos más grandes que esto se nombran pero no se incluyen.
const NEW_FILE_MAX_BYTES: u64 = 1024 * 1024;

/// Prompt listo para lanzar un executor nuevo, con lo que va a `handoffs`.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedHandoff {
    pub prompt: String,
    pub checkpoint_id: Option<CheckpointId>,
    pub mode: ContextMode,
    pub tokens_sent: i64,
    pub tokens_raw_estimate: i64,
    pub build_ms: i64,
}

/// Git vivo del worktree.
struct LiveGit {
    status: String,
    diff: String,
    files: Vec<String>,
    new_files: Vec<NewFile>,
}

fn live_git(worktree: &Path, base: &str) -> Result<LiveGit, symphony_git::GitError> {
    let repo = Repo::at(worktree);
    let entries = repo.status()?;
    let mut status = String::new();
    let mut new_files = Vec::new();
    for e in &entries {
        match e {
            FileStatus::Changed { xy, path, .. } | FileStatus::Unmerged { xy, path } => {
                status.push_str(&format!("{} {path}\n", xy.replace('.', " ")));
            }
            FileStatus::Untracked { path } => {
                status.push_str(&format!("?? {path}\n"));
                let full = worktree.join(path);
                let content = std::fs::metadata(&full)
                    .ok()
                    .filter(|m| m.len() <= NEW_FILE_MAX_BYTES)
                    .and_then(|_| std::fs::read(&full).ok())
                    .and_then(|b| String::from_utf8(b).ok())
                    .map(|s| redact(&s).into_owned());
                new_files.push(NewFile {
                    path: path.clone(),
                    content,
                });
            }
            FileStatus::Ignored { .. } => {}
        }
    }
    let mut files: Vec<String> = repo
        .diff_numstat(base)?
        .into_iter()
        .map(|s| s.path)
        .collect();
    files.extend(new_files.iter().map(|f| f.path.clone()));
    Ok(LiveGit {
        status,
        diff: redact(&repo.diff(base)?).into_owned(),
        files,
        new_files,
    })
}

/// Arma el handoff del agente. `reason`: por qué entra un executor nuevo, en una frase.
pub async fn prepare(
    reader: &Mutex<rusqlite::Connection>,
    agent: AgentId,
    reason: &str,
) -> Result<PreparedHandoff, String> {
    let started = Instant::now();
    let (a, wt, checkpoint, history) = {
        let conn = reader
            .lock()
            .map_err(|_| "lector de la base no disponible")?;
        let a = repo::get_agent(&conn, agent).map_err(|e| e.to_string())?;
        let wt_id = a.worktree_id.ok_or("el agente no tiene worktree")?;
        let wt = repo::get_worktree(&conn, wt_id).map_err(|e| e.to_string())?;
        let checkpoint = repo::latest_checkpoint(&conn, agent)
            .map_err(|e| e.to_string())?
            .ok_or("el agente no tiene un checkpoint válido")?;
        let history = repo::conversation_chars(&conn, agent).map_err(|e| e.to_string())?;
        (a, wt, checkpoint, history)
    };
    let (path, base) = (wt.path.clone(), wt.base_ref.clone());
    let git = tokio::task::spawn_blocking(move || live_git(Path::new(&path), &base))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("no se pudo leer el worktree: {e}"))?;

    let summary: Value = checkpoint
        .summary_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null);
    let last_command = summary["last_command"]["command"]
        .as_str()
        .map(|command| LastCommand {
            command: command.to_string(),
            ok: summary["last_command"]["ok"].as_bool().unwrap_or(false),
            exit_code: summary["last_command"]["exit_code"]
                .as_i64()
                .and_then(|c| i32::try_from(c).ok()),
        });
    let failures = summary["failures"]
        .as_array()
        .map(|f| {
            f.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let input = HandoffInput {
        objective: checkpoint.objective,
        reason: reason.to_string(),
        plan_tail: checkpoint.plan_tail,
        current_step: checkpoint.current_step,
        next_step: checkpoint.next_step,
        last_command,
        failures,
        files_touched: git.files,
        git_status: git.status,
        diff: git.diff,
        // ponytail: el diff vivo no se guarda aparte; si se recorta, se apunta al del checkpoint.
        diff_uri: checkpoint.diff_uri,
        new_files: git.new_files,
    };
    let built = handoff::assemble(&input, a.context_mode);
    let raw = handoff::assemble(&input, ContextMode::Raw).tokens_sent + (history + 3) / 4;
    Ok(PreparedHandoff {
        prompt: built.prompt,
        checkpoint_id: Some(checkpoint.id),
        mode: a.context_mode,
        tokens_sent: built.tokens_sent,
        tokens_raw_estimate: raw,
        build_ms: i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
    })
}
