//! Test D (P01.S6): checkpoint incremental por agente, actualizado en cada
//! evento (CHECKPOINT BEFORE FAILURE), y armado del prompt de handoff.
//!
//! El checkpoint no le pide nada al modelo que muere: sale de los payloads de
//! hooks, de la cola del transcript y de git.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const TRANSCRIPT_TAIL: u64 = 512 * 1024;

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…(recortado)", &s[..end])
}

fn git(cwd: &str, args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Último texto del asistente en la cola del transcript. Entiende el JSONL de
/// Claude Code (`type: assistant`) y el rollout de Codex (`response_item`
/// con `role: assistant`, o `event_msg` de tipo `agent_message`).
fn last_assistant_text(path: &str) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(TRANSCRIPT_TAIL))).ok()?;
    let mut buf = String::new();
    f.read_to_string(&mut buf).ok()?;
    let mut last = None;
    for line in buf.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let content = if v["type"] == "assistant" {
            &v["message"]["content"]
        } else if v["type"] == "response_item" && v["payload"]["role"] == "assistant" {
            &v["payload"]["content"]
        } else if v["type"] == "event_msg" && v["payload"]["type"] == "agent_message" {
            if let Some(t) = v["payload"]["message"].as_str() {
                last = Some(t.to_string());
            }
            continue;
        } else {
            continue;
        };
        for c in content.as_array().into_iter().flatten() {
            if let Some(t) = c["text"].as_str().filter(|t| !t.trim().is_empty()) {
                last = Some(t.to_string());
            }
        }
    }
    last
}

fn path_for(dir: &Path, agent: &str) -> PathBuf {
    dir.join(format!("{agent}.json"))
}

/// Aplica un evento normalizado al checkpoint de su agente.
pub fn update(dir: &Path, ev: &Value) -> std::io::Result<()> {
    let Some(agent) = ev["agent_id"].as_str() else { return Ok(()) };
    let path = path_for(dir, agent);
    let mut cp: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({"agent_id": agent, "events": 0}));
    let raw = &ev["raw"];
    let set = |cp: &mut Value, k: &str, v: Value| {
        if !v.is_null() {
            cp[k] = v;
        }
    };
    set(&mut cp, "provider", ev["provider"].clone());
    set(&mut cp, "session_id", raw["session_id"].clone());
    set(&mut cp, "cwd", raw["cwd"].clone());
    set(&mut cp, "transcript_path", raw["transcript_path"].clone());
    set(&mut cp, "model", raw["model"].clone());
    set(&mut cp, "updated_ms", ev["ts_ms"].clone());
    cp["events"] = json!(cp["events"].as_u64().unwrap_or(0) + 1);

    let tool = ev["tool"].as_str().unwrap_or("");
    let cli_event = ev["cli_event"].as_str().unwrap_or("");
    // Plan / TODOs estructurados, si el CLI los usa (H1: a menudo no).
    if cli_event == "PreToolUse" && tool == "TodoWrite" {
        set(&mut cp, "plan", raw["tool_input"]["todos"].clone());
    }
    if cli_event == "PreToolUse" && tool == "update_plan" {
        set(&mut cp, "plan", raw["tool_input"]["plan"].clone());
    }
    if tool == "Bash" && matches!(cli_event, "PostToolUse" | "PostToolUseFailure") {
        cp["last_command"] = raw["tool_input"]["command"].clone();
        let resp = raw.get("tool_response").or_else(|| raw.get("tool_output")).cloned().unwrap_or(Value::Null);
        let text = match &resp {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        cp["last_command_result"] = json!(clip(&text, 2500));
        cp["last_command_failed"] = json!(cli_event == "PostToolUseFailure");
    }
    if let Some(m) = raw["last_assistant_message"].as_str() {
        cp["last_assistant_message"] = json!(m);
    } else if let Some(t) = raw["transcript_path"].as_str().and_then(last_assistant_text) {
        cp["last_assistant_message"] = json!(t);
    }
    if matches!(cli_event, "PostToolUse" | "PostToolUseFailure" | "Stop")
        && let Some(cwd) = cp["cwd"].as_str().map(str::to_owned)
    {
        cp["git_status"] = json!(git(&cwd, &["status", "--porcelain"]));
        cp["git_diff_stat"] = json!(git(&cwd, &["diff", "--stat"]));
    }

    std::fs::write(&path, serde_json::to_string_pretty(&cp)?)
}

/// Prompt del sucesor: objetivo + checkpoint + git vivo del worktree (H2: git manda).
pub fn handoff(checkpoint: &Path, goal: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let cp: Value = serde_json::from_str(&std::fs::read_to_string(checkpoint)?)?;
    let goal = std::fs::read_to_string(goal)?;
    let cwd = cp["cwd"].as_str().ok_or("checkpoint sin cwd")?;
    let status = git(cwd, &["status", "--porcelain"]);
    let diff = git(cwd, &["diff"]);
    let untracked = git(cwd, &["ls-files", "--others", "--exclude-standard"]);
    let new_files: Vec<String> = untracked
        .lines()
        .filter(|p| !p.is_empty() && !p.ends_with("pnpm-lock.yaml"))
        .map(|p| {
            let body = std::fs::read_to_string(Path::new(cwd).join(p)).unwrap_or_default();
            format!("--- {p} (archivo nuevo)\n{}", clip(&body, 20_000))
        })
        .collect();
    let plan = match cp["plan"].as_array() {
        Some(items) => items
            .iter()
            .map(|t| {
                let text = t["content"].as_str().or(t["step"].as_str()).unwrap_or("?");
                let mark = match t["status"].as_str() {
                    Some("completed") => "x",
                    Some("in_progress") => "~",
                    _ => " ",
                };
                format!("- [{mark}] {text}")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        None => "(el agente no dejó una lista estructurada)".into(),
    };
    let s = |k: &str| cp[k].as_str().unwrap_or("(ninguno)").to_string();
    Ok(format!(
        "Retomas una tarea que otro agente de código dejó a medias (su proceso murió sin aviso). \
Ya estás en su worktree. No empieces de cero: revisa lo que ya existe y continúa desde donde quedó.

## Objetivo original
{goal}

## Plan del agente anterior ([x] hecho, [~] en curso, [ ] pendiente)
{plan}

## Último mensaje del agente anterior
{msg}

## Último comando que corrió y su resultado{failed}
$ {cmd}
{out}

## Estado de git
{status}

## Diff de archivos modificados
{diff}

## Archivos nuevos (todavía sin commit)
{new}

Termina la tarea. Al final, todos los tests (`npm test`) deben pasar. No hagas commit.
",
        goal = goal.trim(),
        msg = clip(&s("last_assistant_message"), 1500),
        failed = if cp["last_command_failed"] == true { " (falló)" } else { "" },
        cmd = s("last_command"),
        out = s("last_command_result"),
        status = if status.is_empty() { "(limpio)".into() } else { status },
        diff = if diff.is_empty() { "(ninguno)".into() } else { diff },
        new = if new_files.is_empty() { "(ninguno)".into() } else { new_files.join("\n\n") },
    ))
}
