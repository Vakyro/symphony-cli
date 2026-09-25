//! P05.S5: adapter de Codex contra fixtures reales (L1) y la suite de contrato.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;
use symphony_adapter_codex::{
    CodexAdapter, fixtures_dir, hook_command_line, hook_overrides, quota_from_rollout,
};
use symphony_adapter_common::contract::{self, Fixtures};
use symphony_adapter_common::{
    AgentEvent, HookCommand, ProviderAdapter, ResumeRequest, SpawnRequest,
};
use symphony_core::FailureType;

fn adapter() -> CodexAdapter {
    CodexAdapter {
        binary: Some(std::env::current_exe().unwrap()),
        ..CodexAdapter::default()
    }
}

fn lines(file: &str) -> Vec<String> {
    std::fs::read_to_string(fixtures_dir().join(file))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

fn expected_stream(line: &str) -> Vec<&'static str> {
    let v: Value = serde_json::from_str(line).unwrap();
    match (v["type"].as_str().unwrap(), v["item"]["type"].as_str()) {
        ("thread.started", _) => vec!["AgentStarted"],
        ("item.completed", Some("agent_message")) => vec!["AssistantText"],
        // "Reconnecting… (stream disconnected…)": red, transitoria.
        ("error", _) => vec!["ProviderError"],
        _ => vec![],
    }
}

fn expected_hook(v: &Value) -> Vec<&'static str> {
    match (
        v["hook_event_name"].as_str().unwrap(),
        v["tool_name"].as_str().unwrap_or(""),
    ) {
        ("SessionStart", _) => vec!["AgentStarted"],
        ("UserPromptSubmit", _) => vec!["TurnStarted"],
        ("PreToolUse", "Bash") => vec!["CommandRequested"],
        ("PreToolUse", _) => vec!["ToolRequested"],
        ("PostToolUse", "Bash") => vec!["CommandFinished"],
        ("PostToolUse", "apply_patch") => vec!["ToolFinished", "FileModified"],
        ("PostToolUse", _) => vec!["ToolFinished"],
        ("Stop", _) => vec!["TurnFinished"],
        ("SessionEnd", _) => vec!["AgentStopped"],
        _ => vec![],
    }
}

fn fixtures() -> Fixtures {
    let mut stream: Vec<(String, Vec<&'static str>)> = lines("exec-stream.jsonl")
        .into_iter()
        .map(|l| {
            let want = expected_stream(&l);
            (l, want)
        })
        .collect();
    stream.push((r#"{"type":"turn.failed","error":{"message":"You've hit your usage limit. Try again at 5 PM."}}"#.into(), vec!["ProviderError"]));
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
            ("You've hit your usage limit. Upgrade to Pro or try again at 5 PM.".into(), FailureType::DailyQuota),
            ("unexpected status 429 Too Many Requests".into(), FailureType::TempRateLimit),
            ("401 Unauthorized: run `codex login` (Authorization: Bearer sk-proj-XXXXXXXXXXXXXXXXXXXXXXXX)".into(), FailureType::Auth),
            ("Reconnecting... 2/5 (stream disconnected before completion: Host desconocido. (os error 11001))".into(), FailureType::Network),
            ("The model `gpt-9` does not exist or you do not have access to it.".into(), FailureType::ModelUnavailable),
        ],
    }
}

#[test]
fn passes_the_contract_suite_with_real_fixtures() {
    let f = fixtures();
    assert!(
        f.stream.len() >= 10 && f.hooks.len() >= 6,
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
fn network_reconnects_are_transient_warnings_are_ignored() {
    let a = adapter();
    let warn = lines("exec-stream.jsonl")
        .into_iter()
        .find(|l| l.contains("bypass-hook-trust"))
        .unwrap();
    assert!(
        a.parse_stream_line(&warn).is_empty(),
        "el aviso de bypass-hook-trust no es una falla"
    );
    let events = a.parse_stream_line(
        r#"{"type":"error","message":"Reconnecting... 5/5 (waiting for network)"}"#,
    );
    let [AgentEvent::ProviderError(e)] = events.as_slice() else {
        panic!("{events:?}")
    };
    assert!(e.transient && e.failure_type == FailureType::Network);
}

#[test]
fn spawn_reads_prompt_from_stdin_and_injects_hooks_with_c() {
    let a = adapter();
    let hook = HookCommand {
        program: "C:\\Program Files\\Symphony\\symphony.exe".into(),
        args: vec!["hook".into(), "emit".into()],
    };
    let req = SpawnRequest {
        worktree: std::env::temp_dir(),
        model: "gpt-5.6-luna".into(),
        prompt: "hola".into(),
        session_id: None,
        hook: Some(hook.clone()),
        env: vec![("SYMPHONY_AGENT_ID".into(), "a1".into())],
    };
    let spec = a.spawn_spec(&req).unwrap();
    let args: Vec<String> = spec
        .args
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert_eq!(&args[..3], ["exec", "-s", "workspace-write"]);
    assert_eq!(
        args.last().map(String::as_str),
        Some("-"),
        "el prompt va por stdin"
    );
    assert!(
        args.contains(&"--json".to_string())
            && args.contains(&"--dangerously-bypass-hook-trust".to_string())
    );
    let overrides: Vec<&String> = args
        .iter()
        .zip(args.iter().skip(1))
        .filter(|(k, _)| *k == "-c")
        .map(|(_, v)| v)
        .collect();
    assert_eq!(overrides.len(), 6);
    assert!(
        overrides
            .iter()
            .any(|o| o.starts_with("hooks.PreToolUse=[{hooks=[{type=\"command\",command="))
    );
    assert!(a.close_stdin_after_prompt());
    assert!(
        a.encode_user_message("x").is_none(),
        "Codex: mensajes solo entre turnos (ADR-0005)"
    );

    let resume = a
        .resume_spec(&ResumeRequest {
            spawn: req,
            cli_session_id: "01a0d481".into(),
        })
        .unwrap();
    let rargs: Vec<String> = resume
        .args
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert_eq!(&rargs[..2], ["exec", "resume"]);
    assert_eq!(&rargs[rargs.len() - 2..], ["01a0d481", "-"]);
    assert!(rargs.contains(&"sandbox_mode=\"workspace-write\"".to_string()));
}

#[test]
fn hook_command_uses_the_shell_codex_runs() {
    let hook = HookCommand {
        program: "C:\\Users\\Leo O'Brien\\symphony.exe".into(),
        args: vec!["hook".into(), "emit".into()],
    };
    let line = hook_command_line(&hook);
    if cfg!(windows) {
        // PowerShell: `&` + comillas simples, '' escapa la comilla (P01 Test B).
        assert_eq!(
            line,
            r"& 'C:\Users\Leo O''Brien\symphony.exe' 'hook' 'emit'"
        );
    } else {
        assert_eq!(
            line,
            r"'C:\Users\Leo O'\''Brien\symphony.exe' 'hook' 'emit'"
        );
    }
    // El override es TOML válido.
    let o = &hook_overrides(&hook)[0];
    let table: toml_edit::DocumentMut = o.parse().unwrap();
    let cmd = table["hooks"]["SessionStart"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert_eq!(cmd, line);
}

#[test]
fn quota_comes_from_the_rollout_file() {
    let dir = tempfile::tempdir().unwrap();
    let rollout = dir.path().join("rollout.jsonl");
    // Forma real del rollout (docs/research/cli-codex.md §7).
    std::fs::write(
        &rollout,
        concat!(
            r#"{"type":"session_meta","payload":{}}"#, "\n",
            r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":6.0,"window_minutes":300,"resets_at":1790275539},"secondary":{"used_percent":32.0,"window_minutes":10080,"resets_at":1790479002}}}}"#, "\n",
        ),
    )
    .unwrap();
    let q = quota_from_rollout(&rollout);
    assert_eq!(q.len(), 2);
    assert_eq!(
        (q[0].window.as_str(), q[0].used_fraction, q[0].resets_at),
        ("five_hour", 0.06, Some(1790275539))
    );
    assert_eq!(
        (q[1].window.as_str(), q[1].used_fraction),
        ("seven_day", 0.32)
    );
    assert!(quota_from_rollout(&dir.path().join("no-existe")).is_empty());
}
