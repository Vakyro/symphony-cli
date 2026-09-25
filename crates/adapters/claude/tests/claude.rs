//! P05.S4: adapter de Claude Code contra fixtures reales (L1) y la suite de contrato.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;
use symphony_adapter_claude::{ClaudeAdapter, fixtures_dir, hook_settings};
use symphony_adapter_common::contract::{self, Fixtures};
use symphony_adapter_common::{
    AgentEvent, HookCommand, ProviderAdapter, ResumeRequest, SpawnRequest,
};
use symphony_core::FailureType;

fn adapter() -> ClaudeAdapter {
    // Un binario cualquiera que exista: los tests no ejecutan Claude.
    ClaudeAdapter {
        binary: Some(std::env::current_exe().unwrap()),
        ..ClaudeAdapter::default()
    }
}

fn lines(file: &str) -> Vec<String> {
    std::fs::read_to_string(fixtures_dir().join(file))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

/// Eventos esperados para cada línea real del stream, según su tipo.
fn expected_stream(line: &str) -> Vec<&'static str> {
    let v: Value = serde_json::from_str(line).unwrap();
    match (v["type"].as_str().unwrap(), v["subtype"].as_str()) {
        ("system", Some("init")) => vec!["AgentStarted"],
        ("rate_limit_event", _) => v["rate_limit_info"]["unifiedWindows"]
            .as_object()
            .unwrap()
            .iter()
            .map(|_| "QuotaUpdated")
            .collect(),
        ("assistant", _) => v["message"]["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["type"] == "text")
            .map(|_| "AssistantText")
            .collect(),
        // Fin del turno: el runtime cierra stdin para que el CLI salga (ADR-0005, adenda P07).
        ("result", _) => vec!["TurnFinished"],
        // hooks, tool_result, thinking, tareas: vienen por hooks o no importan.
        _ => vec![],
    }
}

fn expected_hook(v: &Value) -> Vec<&'static str> {
    let tool = v["tool_name"].as_str().unwrap_or("");
    match (v["hook_event_name"].as_str().unwrap(), tool) {
        ("SessionStart", _) => vec!["AgentStarted"],
        ("UserPromptSubmit", _) => vec!["TurnStarted"],
        ("PreToolUse", "Bash") => vec!["CommandRequested"],
        ("PreToolUse", _) => vec!["ToolRequested"],
        ("PostToolUse", "Bash") | ("PostToolUseFailure", "Bash") => vec!["CommandFinished"],
        ("PostToolUse", "Write" | "Edit") => vec!["ToolFinished", "FileModified"],
        ("PostToolUse" | "PostToolUseFailure", _) => vec!["ToolFinished"],
        ("Stop", _) => vec!["TurnFinished"],
        ("StopFailure", _) => vec!["ProviderError"],
        ("SessionEnd", _) => vec!["AgentStopped"],
        _ => vec![],
    }
}

fn fixtures() -> Fixtures {
    let mut stream: Vec<(String, Vec<&'static str>)> = lines("stream.jsonl")
        .into_iter()
        .map(|l| {
            let want = expected_stream(&l);
            (l, want)
        })
        .collect();
    // Sintéticos con las formas documentadas (docs/research/cli-claude-code.md) que no aparecieron en vivo.
    stream.extend([
        (r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Listo."},{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#.to_string(), vec!["AssistantText"]),
        (r#"{"type":"system","subtype":"api_retry","attempt":1,"max_retries":10,"retry_delay_ms":2000,"error_status":429,"error":"rate_limit"}"#.to_string(), vec!["ProviderError"]),
        (r#"{"type":"user","isReplay":true,"message":{"role":"user","content":"agregá tests"}}"#.to_string(), vec!["UserMessage"]),
        (r#"{"type":"result","subtype":"error","is_error":true,"result":"Claude AI usage limit reached|1790300000"}"#.to_string(), vec!["ProviderError"]),
    ]);
    let hooks = lines("hooks.jsonl")
        .into_iter()
        .map(|l| {
            let v: Value = serde_json::from_str(&l).unwrap();
            let want = expected_hook(&v);
            (v, want)
        })
        .collect();
    Fixtures {
        stream,
        hooks,
        errors: vec![
            ("Claude AI usage limit reached. Your limit will reset at 5pm".into(), FailureType::DailyQuota),
            ("You've hit your weekly limit".into(), FailureType::WeeklyQuota),
            ("API Error: 429 rate_limit_error".into(), FailureType::TempRateLimit),
            ("Invalid API key · Please run /login (Authorization: Bearer sk-ant-api03-XXXXXXXXXXXXXXXXXXXX)".into(), FailureType::Auth),
            ("API Error: 529 overloaded_error".into(), FailureType::ProviderError),
            ("Connection error: getaddrinfo ENOTFOUND api.anthropic.com".into(), FailureType::Network),
        ],
    }
}

#[test]
fn passes_the_contract_suite_with_real_fixtures() {
    let f = fixtures();
    assert!(
        f.stream.len() >= 10 && f.hooks.len() >= 8,
        "fixtures incompletos"
    );
    let failures = contract::check(&adapter(), &f);
    assert!(
        failures.is_empty(),
        "fallas del contrato:\n{}",
        failures.join("\n")
    );
}

#[test]
fn quota_from_rate_limit_event_is_known() {
    let line = lines("stream.jsonl")
        .into_iter()
        .find(|l| l.contains("rate_limit_event"))
        .unwrap();
    let ev = adapter().parse_stream_line(&line);
    let windows: Vec<_> = ev
        .iter()
        .map(|e| match e {
            AgentEvent::Quota(q) => (
                q.window.clone(),
                (0.0..=1.0).contains(&q.used_fraction),
                q.resets_at.is_some(),
            ),
            other => panic!("{other:?}"),
        })
        .collect();
    assert!(
        windows.contains(&("five_hour".into(), true, true)),
        "{windows:?}"
    );
    assert!(
        windows.contains(&("seven_day".into(), true, true)),
        "{windows:?}"
    );
}

#[test]
fn spawn_uses_headless_stream_json_and_injects_hooks_per_invocation() {
    let a = adapter();
    let hook = HookCommand {
        program: "C:\\Program Files\\Symphony\\symphony.exe".into(),
        args: vec!["hook".into(), "emit".into()],
    };
    let req = SpawnRequest {
        worktree: std::env::temp_dir(),
        model: "sonnet".into(),
        prompt: "hola".into(),
        session_id: Some("7f0c7b3e-2c7e-4a58-9d9e-8e1b1c4f0a11".into()),
        hook: Some(hook.clone()),
        env: vec![("SYMPHONY_AGENT_ID".into(), "a1".into())],
    };
    let spec = a.spawn_spec(&req).unwrap();
    let args: Vec<String> = spec
        .args
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    for want in [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "--verbose",
        "--settings",
        "--session-id",
    ] {
        assert!(args.iter().any(|a| a == want), "falta {want}: {args:?}");
    }
    assert!(
        !args.iter().any(|a| a == "--bare"),
        "--bare no usa la suscripción"
    );
    assert_eq!(
        args[args.iter().position(|a| a == "--permission-mode").unwrap() + 1],
        "acceptEdits"
    );

    // Los 8 hooks llaman a `symphony hook emit`, con comillas por el espacio en la ruta.
    let settings: Value =
        serde_json::from_str(&args[args.iter().position(|a| a == "--settings").unwrap() + 1])
            .unwrap();
    assert_eq!(settings, hook_settings(&hook));
    let hooks = settings["hooks"].as_object().unwrap();
    assert_eq!(hooks.len(), 8);
    let cmd = hooks["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert_eq!(
        cmd,
        r#"'C:/Program Files/Symphony/symphony.exe' 'hook' 'emit'"#
    );
    assert_eq!(hooks["PreToolUse"][0]["hooks"][0]["timeout"], 30);

    let resume = a
        .resume_spec(&ResumeRequest {
            spawn: req,
            cli_session_id: "prev-session".into(),
        })
        .unwrap();
    let rargs: Vec<String> = resume
        .args
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        rargs[rargs.iter().position(|a| a == "--resume").unwrap() + 1],
        "prev-session"
    );
    assert!(!rargs.iter().any(|a| a == "--session-id"));
}

#[test]
fn prompts_and_mid_task_messages_are_stream_json_user_lines() {
    let a = adapter();
    let bytes = a.encode_prompt("creá \"a.js\"\ncon ñ");
    let text = String::from_utf8(bytes).unwrap();
    assert!(
        text.ends_with('\n') && text.matches('\n').count() == 1,
        "una sola línea JSON"
    );
    let v: Value = serde_json::from_str(text.trim_end()).unwrap();
    assert_eq!(
        (v["type"].as_str(), v["message"]["content"].as_str()),
        (Some("user"), Some("creá \"a.js\"\ncon ñ"))
    );
    assert!(
        a.encode_user_message("otra cosa").is_some(),
        "Claude acepta mensajes a media tarea (ADR-0005)"
    );
}

/// L3: corre Claude de verdad (gasta cuota). Solo con `SYMPHONY_LIVE=1` y permiso de Leo.
#[test]
fn live_detect_and_short_turn() {
    if std::env::var("SYMPHONY_LIVE").as_deref() != Ok("1") {
        eprintln!("omitido: SYMPHONY_LIVE != 1");
        return;
    }
    let a = ClaudeAdapter::default();
    let det = a.detect().unwrap();
    eprintln!("claude {} en {}", det.version, det.cli_path.display());
    let dir = tempfile::tempdir().unwrap();
    let req = SpawnRequest {
        worktree: dir.path().to_path_buf(),
        model: "haiku".into(),
        prompt: "Reply with the single word: ok".into(),
        session_id: None,
        hook: None,
        env: vec![],
    };
    let spec = a.spawn_spec(&req).unwrap();
    let mut child = std::process::Command::new(&spec.program)
        .args(&spec.args)
        .current_dir(dir.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&a.encode_prompt(&req.prompt)).unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let events: Vec<_> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .flat_map(|l| a.parse_stream_line(l))
        .collect();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::SessionStarted { .. })),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::AssistantText { .. })),
        "{events:?}"
    );
}

#[test]
fn hook_command_cannot_inject_through_the_path() {
    // Una carpeta con `$(…)` y comillas no puede ejecutar nada: todo va entre comillas simples.
    let hook = HookCommand {
        program: r"C:\Users\a$(rm -rf ~)`x`'b\symphony.exe".into(),
        args: vec!["hook".into(), "emit".into()],
    };
    let settings = hook_settings(&hook);
    let cmd = settings["hooks"]["Stop"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert_eq!(
        cmd,
        r#"'C:/Users/a$(rm -rf ~)`x`'\''b/symphony.exe' 'hook' 'emit'"#
    );
}
