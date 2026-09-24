//! Test B/C (P01.S4–S5): hooks de Claude Code y Codex -> IPC local -> events.jsonl.
//!
//!   spike-hook collect <events.jsonl>   escucha y agrega eventos normalizados
//!   spike-hook hook <claude|codex>      lo invoca el CLI; lee el payload de stdin
//!
//! El hook nunca rompe al CLI: sin `SYMPHONY_AGENT_ID`, o si el collector no
//! está, sale con 0 sin decidir nada (IDEA §5.3, aislamiento de hooks).
//! Test C: con `SPIKE_HOLD_SECS` y `SPIKE_HOLD_MATCH`, un `PreToolUse` cuyo
//! comando contenga el texto espera esos segundos y luego responde `allow`.

mod checkpoint;

use std::io::{BufRead, BufReader, Read, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, prelude::*};
use serde_json::{Value, json};

const SOCKET: &str = "symphony-spike-bus.sock";

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

/// Nombres de IDEA §5.3.
fn normalize(event: &str, tool: &str) -> &'static str {
    let is_cmd = tool == "Bash";
    let is_edit = matches!(tool, "Write" | "Edit" | "MultiEdit" | "NotebookEdit" | "apply_patch");
    match event {
        "SessionStart" => "AgentStarted",
        "UserPromptSubmit" => "TurnStarted",
        "PreToolUse" if is_cmd => "CommandRequested",
        "PreToolUse" => "ToolRequested",
        "PostToolUse" | "PostToolUseFailure" if is_cmd => "CommandFinished",
        "PostToolUse" | "PostToolUseFailure" if is_edit => "FileModified",
        "PostToolUse" | "PostToolUseFailure" => "ToolFinished",
        "Stop" => "TurnFinished",
        "StopFailure" => "ProviderError",
        "SessionEnd" => "AgentStopped",
        _ => "Unknown",
    }
}

fn send(line: &Value) {
    let Ok(name) = SOCKET.to_ns_name::<GenericNamespaced>() else { return };
    if let Ok(mut conn) = Stream::connect(name) {
        let _ = writeln!(conn, "{line}");
    }
}

fn hook(provider: &str) {
    let Ok(agent) = std::env::var("SYMPHONY_AGENT_ID") else { return };
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return;
    }
    let raw: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    let event = raw["hook_event_name"].as_str().unwrap_or("?");
    let tool = raw["tool_name"].as_str().unwrap_or("");
    let command = raw["tool_input"]["command"].as_str().unwrap_or("");
    let base = |kind: &str| {
        json!({
            "ts_ms": now_ms(), "agent_id": agent, "provider": provider, "event": kind,
            "cli_event": event, "tool": tool, "session_id": raw["session_id"],
            "model": raw["model"], "command": command, "raw": raw,
        })
    };
    send(&base(normalize(event, tool)));

    let hold = std::env::var("SPIKE_HOLD_SECS").ok().and_then(|s| s.parse::<u64>().ok());
    let pattern = std::env::var("SPIKE_HOLD_MATCH").unwrap_or_default();
    if let Some(secs) = hold
        && event == "PreToolUse"
        && !pattern.is_empty()
        && command.contains(&pattern)
    {
        send(&base("HoldStarted"));
        std::thread::sleep(Duration::from_secs(secs));
        send(&base("HoldReleased"));
        println!(
            "{}",
            json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow"}})
        );
    }
}

fn collect(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let name = SOCKET.to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new().name(name).create_sync()?;
    let mut out = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    let checkpoints = std::path::Path::new(path).with_file_name("checkpoints");
    std::fs::create_dir_all(&checkpoints)?;
    eprintln!("collector escuchando en {SOCKET} -> {path}");
    for conn in listener.incoming() {
        let Ok(conn) = conn else { continue };
        for line in BufReader::new(conn).lines().map_while(Result::ok) {
            writeln!(out, "{line}")?;
            out.flush()?;
            if let Ok(ev) = serde_json::from_str::<Value>(&line)
                && let Err(e) = checkpoint::update(&checkpoints, &ev)
            {
                eprintln!("checkpoint: {e}");
            }
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["collect", path] => collect(path),
        ["handoff", cp, goal] => {
            print!("{}", checkpoint::handoff(cp.as_ref(), goal.as_ref())?);
            Ok(())
        }
        ["hook", provider] => {
            hook(provider);
            Ok(())
        }
        _ => Err("uso: spike-hook collect <events.jsonl> | spike-hook hook <claude|codex> | spike-hook handoff <checkpoint.json> <goal.md>".into()),
    }
}
