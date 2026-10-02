//! Junta los datos del handoff (P06.S4): último checkpoint válido + git vivo del
//! worktree (ADR-0004, H2: git manda) y los pasa al assembler de `symphony-context`.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use serde_json::Value;
use symphony_context::handoff::{self, ChatMessage, HandoffInput, LastCommand, NewFile, Speaker};
use symphony_core::{AgentId, CheckpointId, ContextMode, redact};
use symphony_git::{FileStatus, Repo};
use symphony_object_store::ObjectStore;
use symphony_store::repo;

/// Archivos nuevos más grandes que esto se nombran pero no se incluyen.
const NEW_FILE_MAX_BYTES: u64 = 1024 * 1024;

/// Prompt listo para lanzar un executor nuevo, con lo que va a `handoffs`.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedHandoff {
    pub prompt: String,
    pub checkpoint_id: Option<CheckpointId>,
    /// `seq` y fecha del checkpoint usado (para el separador y `checkpoint_age_ms`).
    pub checkpoint_seq: i64,
    pub checkpoint_created_at: i64,
    pub mode: ContextMode,
    pub tokens_sent: i64,
    pub tokens_raw_estimate: i64,
    pub build_ms: i64,
    pub items: Vec<repo::NewHandoffItem>,
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

/// Texto de un mensaje: en línea o, si era largo, en el object store.
pub(crate) fn message_text(
    conn: &rusqlite::Connection,
    objects: &ObjectStore,
    m: &repo::MessageRecord,
) -> String {
    if let Some(c) = &m.content {
        return c.clone();
    }
    let Some(object) = m.content_object_id else {
        return String::new();
    };
    conn.query_row(
        "SELECT blob_hash FROM context_objects WHERE id = ?1",
        [object.to_string()],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|hash| objects.get(&hash).ok())
    .map_or_else(
        || "(objeto no disponible)".into(),
        |b| String::from_utf8_lossy(&b).into_owned(),
    )
}

/// Lo dicho hasta ahora: mensajes del usuario y del asistente, en orden. No entran las
/// herramientas, los separadores de cambio de executor ni los prompts de handoff (el
/// primer mensaje de un run que arrancó desde un checkpoint), que ya contienen esta misma
/// conversación. Los fragmentos seguidos del asistente se juntan en un mensaje.
fn conversation(
    conn: &rusqlite::Connection,
    objects: &ObjectStore,
    agent: AgentId,
) -> Result<Vec<ChatMessage>, String> {
    let handoff_runs: HashSet<_> = repo::handoff_run_ids(conn, agent)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    let mut seen_first_user = HashSet::new();
    let mut out: Vec<ChatMessage> = Vec::new();
    for m in repo::agent_messages(conn, agent, None).map_err(|e| e.to_string())? {
        let speaker = match m.role.as_str() {
            "USER" => Speaker::User,
            "ASSISTANT" => Speaker::Assistant,
            _ => continue,
        };
        if speaker == Speaker::User
            && let Some(run) = m.run_id
            && handoff_runs.contains(&run)
            && seen_first_user.insert(run)
        {
            continue;
        }
        let text = redact(&message_text(conn, objects, &m)).into_owned();
        if text.trim().is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if speaker == Speaker::Assistant && last.speaker == Speaker::Assistant => {
                last.text.push_str("\n\n");
                last.text.push_str(&text);
            }
            _ => out.push(ChatMessage { speaker, text }),
        }
    }
    Ok(out)
}

/// Arma el handoff del agente. `reason`: por qué entra un executor nuevo, en una frase.
pub async fn prepare(
    reader: &Mutex<rusqlite::Connection>,
    objects: &ObjectStore,
    agent: AgentId,
    reason: &str,
) -> Result<PreparedHandoff, String> {
    let started = Instant::now();
    let (a, wt, checkpoint, conversation) = {
        let conn = reader
            .lock()
            .map_err(|_| "lector de la base no disponible")?;
        let a = repo::get_agent(&conn, agent).map_err(|e| e.to_string())?;
        let wt_id = a.worktree_id.ok_or("el agente no tiene worktree")?;
        let wt = repo::get_worktree(&conn, wt_id).map_err(|e| e.to_string())?;
        let checkpoint = repo::latest_checkpoint(&conn, agent)
            .map_err(|e| e.to_string())?
            .ok_or("el agente no tiene un checkpoint válido")?;
        let conversation = conversation(&conn, objects, agent)?;
        (a, wt, checkpoint, conversation)
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

    let (checkpoint_seq, checkpoint_created_at) = (checkpoint.seq, checkpoint.created_at);
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
        conversation,
    };
    let built = handoff::assemble(&input, a.context_mode);
    let raw = handoff::assemble(&input, ContextMode::Raw).tokens_sent;
    Ok(PreparedHandoff {
        prompt: built.prompt,
        checkpoint_id: Some(checkpoint.id),
        checkpoint_seq,
        checkpoint_created_at,
        mode: a.context_mode,
        tokens_sent: built.tokens_sent,
        tokens_raw_estimate: raw,
        build_ms: i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
        items: built
            .items
            .into_iter()
            .map(|i| repo::NewHandoffItem {
                section: i.section.as_str(),
                path: i.path,
                fidelity: i.fidelity,
                tokens: i.tokens,
            })
            .collect(),
    })
}
