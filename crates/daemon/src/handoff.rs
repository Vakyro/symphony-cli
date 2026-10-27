//! Junta los datos del handoff (P06.S4): último checkpoint válido + git vivo del
//! worktree (ADR-0004, H2: git manda) y los pasa al assembler de `symphony-context`.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use serde_json::Value;
use symphony_context::handoff::{
    self, ChatMessage, HandoffInput, LastCommand, NewFile, ProjectFact, Speaker,
};
use symphony_core::{AgentId, CheckpointId, ContextMode, redact};
use symphony_git::{FileStatus, Repo};
use symphony_object_store::ObjectStore;
use symphony_store::repo;

/// Archivos nuevos más grandes que esto se nombran pero no se incluyen.
const NEW_FILE_MAX_BYTES: u64 = 1024 * 1024;
/// Tope agregado de bytes de archivos nuevos por handoff: pasado el tope se nombran sin
/// contenido, aunque cada uno quepa por separado (P13.S5).
const NEW_FILES_TOTAL_MAX: usize = 8 * 1024 * 1024;

/// Prompt listo para lanzar un executor nuevo, con lo que va a `handoffs`.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedHandoff {
    pub prompt: String,
    pub checkpoint_id: Option<CheckpointId>,
    /// `seq` y fecha del checkpoint usado (para el separador y `checkpoint_age_ms`).
    pub checkpoint_seq: i64,
    pub checkpoint_created_at: i64,
    /// Hubo actividad que amerita checkpoint después de este (FLOW §13.3): el estado del código
    /// se reconstruye del worktree vivo, pero el objetivo y el «qué seguía» pueden venir atrasados.
    pub stale: bool,
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

/// Lee un archivo nuevo si cabe en el tope por archivo y en el presupuesto de la tanda. Pasado
/// el tope total entra como «sin contenido»: no se lee ni se reserva (P13.S5).
fn read_new_file(worktree: &Path, path: &str, budget: &mut usize) -> Option<String> {
    let full = worktree.join(path);
    let meta = std::fs::metadata(&full).ok()?;
    let len = usize::try_from(meta.len()).ok()?;
    if !meta.is_file() || meta.len() > NEW_FILE_MAX_BYTES || len > *budget {
        return None;
    }
    let text = String::from_utf8(std::fs::read(&full).ok()?).ok()?;
    *budget -= len;
    Some(redact(&text).into_owned())
}

fn live_git(worktree: &Path, base: &str) -> Result<LiveGit, symphony_git::GitError> {
    let repo = Repo::at(worktree);
    let entries = repo.status()?;
    let mut status = String::new();
    let mut new_files = Vec::new();
    let mut budget = NEW_FILES_TOTAL_MAX;
    for e in &entries {
        match e {
            FileStatus::Changed { xy, path, .. } | FileStatus::Unmerged { xy, path } => {
                status.push_str(&format!("{} {path}\n", xy.replace('.', " ")));
            }
            FileStatus::Untracked { path } => {
                status.push_str(&format!("?? {path}\n"));
                let content = read_new_file(worktree, path, &mut budget);
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

/// De dónde sale el texto de un mensaje.
enum SourceText {
    /// El mensaje corto ya trae su texto en la fila.
    Inline(String),
    /// El mensaje largo vive en el object store: se lee después, sin el cerrojo del lector.
    Blob(String),
}

/// Un mensaje ya filtrado, pendiente de resolver su texto.
struct MessageSource {
    speaker: Speaker,
    run: Option<String>,
    text: SourceText,
}

/// Bajo el cerrojo del lector: solo filas y hashes, ninguna lectura del object store (P13.S5).
fn message_sources(
    conn: &rusqlite::Connection,
    agent: AgentId,
) -> Result<(Vec<MessageSource>, HashSet<String>), String> {
    let handoff_runs: HashSet<String> = repo::handoff_run_ids(conn, agent)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|run| run.to_string())
        .collect();
    let mut stmt = conn
        .prepare(
            "SELECT m.run_id, m.role, m.content, co.blob_hash
             FROM messages m
             LEFT JOIN context_objects co ON co.id = m.content_object_id
             WHERE m.agent_id = ?1
             ORDER BY m.created_at ASC, m.id ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([agent.to_string()], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (run, role, content, hash) in rows {
        let speaker = match role.as_str() {
            "USER" => Speaker::User,
            "ASSISTANT" => Speaker::Assistant,
            _ => continue,
        };
        let text = match (content, hash) {
            (Some(content), _) => SourceText::Inline(content),
            (None, Some(hash)) => SourceText::Blob(hash),
            (None, None) => continue,
        };
        out.push(MessageSource { speaker, run, text });
    }
    Ok((out, handoff_runs))
}

/// Lo dicho hasta ahora: mensajes del usuario y del asistente, en orden. No entran las
/// herramientas, los separadores de cambio de executor ni los prompts de handoff (el
/// primer mensaje de un run que arrancó desde un checkpoint), que ya contienen esta misma
/// conversación. Los fragmentos seguidos del asistente se juntan en un mensaje.
fn conversation(
    sources: Vec<MessageSource>,
    handoff_runs: HashSet<String>,
    objects: &ObjectStore,
) -> Vec<ChatMessage> {
    let mut seen_first_user = HashSet::new();
    let mut out: Vec<ChatMessage> = Vec::new();
    for source in sources {
        if source.speaker == Speaker::User
            && let Some(run) = source.run
            && handoff_runs.contains(&run)
            && seen_first_user.insert(run)
        {
            continue;
        }
        let raw = match source.text {
            SourceText::Inline(text) => text,
            SourceText::Blob(hash) => objects.get(&hash).map_or_else(
                |_| "(objeto no disponible)".into(),
                |b| String::from_utf8_lossy(&b).into_owned(),
            ),
        };
        let text = redact(&raw).into_owned();
        if text.trim().is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last)
                if source.speaker == Speaker::Assistant && last.speaker == Speaker::Assistant =>
            {
                last.text.push_str("\n\n");
                last.text.push_str(&text);
            }
            _ => out.push(ChatMessage {
                speaker: source.speaker,
                text,
            }),
        }
    }
    out
}

/// Descarta checkpoints ilegibles antes de usarlos; errores de E/S transitorios se propagan.
pub(crate) async fn validated_checkpoint(
    reader: &Mutex<rusqlite::Connection>,
    writer: &symphony_store::WriterHandle,
    objects: &ObjectStore,
    agent: AgentId,
) -> Result<Option<repo::LatestCheckpoint>, String> {
    loop {
        let checkpoint = {
            let conn = reader
                .lock()
                .map_err(|_| "lector de la base no disponible")?;
            repo::latest_checkpoint(&conn, agent).map_err(|e| e.to_string())?
        };
        let Some(checkpoint) = checkpoint else {
            return Ok(None);
        };
        let objects = objects.clone();
        let candidate = checkpoint.clone();
        let invalid = tokio::task::spawn_blocking(move || {
            if let Some(summary) = &candidate.summary_json
                && serde_json::from_str::<Value>(summary).is_err()
            {
                return Ok(Some("JSON de resumen ilegible".to_string()));
            }
            if candidate.diff_object_id.is_some() {
                let Some(hash) = &candidate.diff_blob_hash else {
                    return Ok(Some("referencia de diff sin blob".to_string()));
                };
                match objects.get(hash) {
                    Ok(_) => {}
                    Err(
                        e @ (symphony_object_store::ObjectError::NotFound(_)
                        | symphony_object_store::ObjectError::Corrupt { .. }
                        | symphony_object_store::ObjectError::InvalidHash(_)),
                    ) => {
                        return Ok(Some(e.to_string()));
                    }
                    Err(e) => return Err(e.to_string()),
                }
            }
            Ok(None)
        })
        .await
        .map_err(|e| e.to_string())??;
        let Some(detail) = invalid else {
            return Ok(Some(checkpoint));
        };
        let id = checkpoint.id;
        let detail = format!("Checkpoint {id}: {detail}");
        writer
            .write(Box::new(move |tx| {
                repo::invalidate_checkpoint(tx, id, &detail, crate::runtime::now_ms())
            }))
            .await
            .map_err(|e| e.to_string())?;
    }
}

/// Arma el handoff del agente. `reason`: por qué entra un executor nuevo, en una frase.
pub async fn prepare(
    reader: &Mutex<rusqlite::Connection>,
    objects: &ObjectStore,
    writer: &symphony_store::WriterHandle,
    agent: AgentId,
    reason: &str,
) -> Result<PreparedHandoff, String> {
    let started = Instant::now();
    let checkpoint = validated_checkpoint(reader, writer, objects, agent).await?;
    let (a, wt, objective, sources, handoff_runs, facts, stale) = {
        let conn = reader
            .lock()
            .map_err(|_| "lector de la base no disponible")?;
        let a = repo::get_agent(&conn, agent).map_err(|e| e.to_string())?;
        let wt_id = a.worktree_id.ok_or("el agente no tiene worktree")?;
        let wt = repo::get_worktree(&conn, wt_id).map_err(|e| e.to_string())?;
        let task = repo::get_task(&conn, a.task_id).map_err(|e| e.to_string())?;
        let objective = match task.description {
            Some(d) if !d.trim().is_empty() => format!("{}\n\n{}", task.title, d.trim()),
            _ => task.title,
        };
        // Los eventos que piden checkpoint (ver `checkpoint::Progress::apply`): si hay uno más
        // nuevo que el checkpoint, el checkpoint no lo alcanzó.
        let stale: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM events WHERE agent_id = ?1 AND occurred_at > ?2
                   AND type IN ('TurnFinished','ToolFinished','CommandFinished','FileModified'))",
                rusqlite::params![
                    agent.to_string(),
                    checkpoint.as_ref().map_or(0, |c| c.created_at)
                ],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let (sources, handoff_runs) = message_sources(&conn, agent)?;
        let facts = symphony_store::context::get_current_facts(&conn, &a.project_id.to_string())
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|f| ProjectFact {
                key: f.key,
                value: f.value,
                kind: f.kind,
            })
            .collect();
        (a, wt, objective, sources, handoff_runs, facts, stale)
    };
    // Ya sin el cerrojo del lector: los blobs del object store y la redacción (P13.S5).
    let conversation = conversation(sources, handoff_runs, objects);
    let (path, base) = (wt.path.clone(), wt.base_ref.clone());
    let git = tokio::task::spawn_blocking(move || live_git(Path::new(&path), &base))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("no se pudo leer el worktree: {e}"))?;

    let summary: Value = checkpoint
        .as_ref()
        .and_then(|c| c.summary_json.as_deref())
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

    let checkpoint_id = checkpoint.as_ref().map(|c| c.id);
    let (checkpoint_seq, checkpoint_created_at) = checkpoint
        .as_ref()
        .map_or((0, 0), |c| (c.seq, c.created_at));
    let input = HandoffInput {
        objective: checkpoint
            .as_ref()
            .map_or(objective, |c| c.objective.clone()),
        reason: reason.to_string(),
        plan_tail: checkpoint.as_ref().and_then(|c| c.plan_tail.clone()),
        current_step: checkpoint.as_ref().and_then(|c| c.current_step.clone()),
        next_step: checkpoint.as_ref().and_then(|c| c.next_step.clone()),
        last_command,
        failures,
        files_touched: git.files,
        git_status: git.status,
        diff: git.diff,
        // ponytail: el diff vivo no se guarda aparte; si se recorta, se apunta al del checkpoint.
        diff_uri: checkpoint.as_ref().and_then(|c| c.diff_uri.clone()),
        new_files: git.new_files,
        conversation,
        facts,
    };
    let built = handoff::assemble(&input, a.context_mode);
    let raw = handoff::assemble(&input, ContextMode::Raw).tokens_sent;
    Ok(PreparedHandoff {
        prompt: built.prompt,
        checkpoint_id,
        checkpoint_seq,
        checkpoint_created_at,
        stale,
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn untracked_files_share_one_total_budget() {
        // Dos archivos que caben por separado pero no juntos: el segundo se nombra sin
        // contenido, sin leerlo ni reservarlo (P13.S5).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("uno.txt"), "abcdef").unwrap();
        std::fs::write(root.join("dos.txt"), "ghijkl").unwrap();
        let mut budget = 8;
        assert_eq!(
            read_new_file(root, "uno.txt", &mut budget).as_deref(),
            Some("abcdef")
        );
        assert_eq!(budget, 2);
        assert_eq!(read_new_file(root, "dos.txt", &mut budget), None);
        assert_eq!(budget, 2, "no se descuenta lo que no se leyó");
        // Un archivo que no existe tampoco toca el presupuesto.
        assert!(read_new_file(root, "no-existe.txt", &mut budget).is_none());
        assert_eq!(budget, 2);
    }

    #[test]
    fn a_huge_new_file_never_enters_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("grande.bin"), vec![b'x'; 2 * 1024 * 1024]).unwrap();
        let mut budget = NEW_FILES_TOTAL_MAX;
        assert!(read_new_file(root, "grande.bin", &mut budget).is_none());
        assert_eq!(budget, NEW_FILES_TOTAL_MAX);
    }
}
