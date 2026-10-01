//! P11.S3: adapter de Antigravity contra fixtures reales (L1, P11.S1/S3) y la suite de contrato.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;
use symphony_adapter_antigravity::{AntigravityAdapter, fixtures_dir};
use symphony_adapter_common::contract::{self, Fixtures};
use symphony_adapter_common::{
    AgentEvent, ProcessSpec, ProviderAdapter, ResumeRequest, SpawnRequest, ToolKind,
};
use symphony_core::FailureType;

fn adapter() -> AntigravityAdapter {
    AntigravityAdapter {
        binary: Some(std::env::current_exe().unwrap()),
        ..AntigravityAdapter::default()
    }
}

fn lines(file: &str) -> Vec<String> {
    std::fs::read_to_string(fixtures_dir().join(file))
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

fn request(model: &str) -> SpawnRequest {
    SpawnRequest {
        worktree: std::env::temp_dir(),
        model: model.into(),
        prompt: "hola".into(),
        session_id: None,
        hook: None,
        env: vec![],
    }
}

fn args(spec: &ProcessSpec) -> Vec<String> {
    spec.args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

/// Lo que cada línea de las fixtures tiene que producir, según su tipo.
fn expected(line: &str) -> Vec<&'static str> {
    let v: Value = serde_json::from_str(line).unwrap();
    match v["event"].as_str().unwrap() {
        "init" => vec!["AgentStarted"],
        "step_update" => {
            let s = &v["step_update"];
            match (
                s["step_type"].as_str().unwrap(),
                s["state"].as_str().unwrap(),
            ) {
                ("tool", "ACTIVE") if s["tool_name"] == "run_command" => vec!["CommandRequested"],
                ("tool", "ACTIVE") => vec!["ToolRequested"],
                ("tool", "DONE") if s["tool_name"] == "run_command" => vec!["CommandFinished"],
                ("tool", "DONE")
                    if ["write_to_file", "replace_file_content"]
                        .contains(&s["tool_name"].as_str().unwrap()) =>
                {
                    vec!["ToolFinished", "FileModified"]
                }
                ("tool", _) => vec!["ToolFinished"],
                ("agent_response", "DONE")
                    if s["usage"]["input_tokens"].as_u64().unwrap_or(0) > 0 =>
                {
                    vec!["TurnUsage"]
                }
                _ => vec![],
            }
        }
        "result" => match v["result"]["status"].as_str().unwrap() {
            "ERROR" => vec!["ProviderError"],
            _ => vec!["AssistantText"],
        },
        _ => vec![],
    }
}

fn fixtures() -> Fixtures {
    let mut stream = Vec::new();
    for file in [
        "stream.jsonl",
        "resume.jsonl",
        "tools.jsonl",
        "tools-denied.jsonl",
        "error-modelo.jsonl",
    ] {
        for l in lines(file) {
            let want = expected(&l);
            stream.push((l, want));
        }
    }
    Fixtures {
        stream,
        hooks: vec![],
        errors: vec![
            (
                "invalid model selection (--model \"x\"): model x is not recognized as a known model".into(),
                FailureType::ModelUnavailable,
            ),
            // Sintéticos: no se pudo provocar cuota ni auth en vivo (cli-p11.md).
            ("429 RESOURCE_EXHAUSTED: quota exceeded".into(), FailureType::DailyQuota),
            ("Too many requests, rate limit".into(), FailureType::TempRateLimit),
            (
                "401 Unauthenticated: sign in (Authorization: Bearer sk-AAAAAAAAAAAAAAAAAAAAAAAA)".into(),
                FailureType::Auth,
            ),
        ],
    }
}

#[test]
fn passes_the_contract_suite_with_real_fixtures() {
    let f = fixtures();
    assert!(
        f.stream.len() >= 30,
        "fixtures incompletos: {}",
        f.stream.len()
    );
    let failures = contract::check(&adapter(), &f);
    assert!(
        failures.is_empty(),
        "fallas del contrato:\n{}",
        failures.join("\n")
    );
}

#[test]
fn init_gives_the_conversation_id() {
    let init = &lines("stream.jsonl")[0];
    assert!(matches!(
        &adapter().parse_stream_line(init)[..],
        [AgentEvent::SessionStarted { cli_session_id: Some(id), .. }]
            if id == "32978495-66fd-4be1-9814-1eaacb875116"
    ));
}

#[test]
fn tool_events_carry_command_path_and_result() {
    let a = adapter();
    let ev: Vec<AgentEvent> = lines("tools.jsonl")
        .iter()
        .flat_map(|l| a.parse_stream_line(l))
        .collect();
    assert!(ev.iter().any(|e| matches!(
        e,
        AgentEvent::ToolRequested { tool, kind: ToolKind::Edit, .. } if tool == "write_to_file"
    )));
    assert!(ev.iter().any(|e| matches!(
        e,
        AgentEvent::FileModified { path: Some(p), .. } if p.ends_with("hola.txt")
    )));
    assert!(ev.iter().any(|e| matches!(
        e,
        AgentEvent::ToolRequested { command: Some(c), kind: ToolKind::Command, .. } if c == "echo listo"
    )));
    assert!(matches!(
        ev.last(),
        Some(AgentEvent::AssistantText { text }) if text.contains("hecho")
    ));
}

#[test]
fn only_the_last_response_step_decides_the_context_size() {
    let a = adapter();
    let tokens: Vec<u64> = lines("tools.jsonl")
        .iter()
        .flat_map(|l| a.parse_stream_line(l))
        .filter_map(|e| match e {
            AgentEvent::TurnUsage { context_tokens } => Some(context_tokens),
            _ => None,
        })
        .collect();
    assert!(tokens.len() >= 2, "{tokens:?}");
    assert!(tokens.iter().all(|t| *t > 0));
}

#[test]
fn a_denied_turn_says_what_was_denied() {
    let a = adapter();
    let last = lines("tools-denied.jsonl").pop().unwrap();
    assert!(matches!(
        &a.parse_stream_line(&last)[..],
        [AgentEvent::AssistantText { text }] if text.contains("command") && text.contains("denegó")
    ));
}

#[test]
fn invalid_model_error_is_classified() {
    let last = lines("error-modelo.jsonl").pop().unwrap();
    assert!(matches!(
        &adapter().parse_stream_line(&last)[..],
        [AgentEvent::ProviderError(e)] if e.failure_type == FailureType::ModelUnavailable
    ));
}

#[test]
fn prompt_goes_to_stdin_as_a_user_event() {
    let bytes = adapter().encode_prompt("línea 1\nlínea \"2\"");
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.ends_with('\n'));
    let v: Value = serde_json::from_str(text.trim_end()).unwrap();
    assert_eq!(v["event"], "user");
    assert_eq!(v["message"]["content"], "línea 1\nlínea \"2\"");
}

#[test]
fn specs_use_stream_json_stdin_and_accept_edits_by_default() {
    let a = adapter();
    let spawn = args(&a.spawn_spec(&request("gemini-x")).unwrap());
    assert_eq!(
        spawn,
        [
            "--print=",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--model",
            "gemini-x",
            "--mode",
            "accept-edits"
        ]
    );
    let resume = args(
        &a.resume_spec(&ResumeRequest {
            spawn: request("gemini-x"),
            cli_session_id: "conv-1".into(),
        })
        .unwrap(),
    );
    assert_eq!(resume[resume.len() - 2..], ["--conversation", "conv-1"]);
}

#[test]
fn skip_permissions_is_opt_in() {
    let a = AntigravityAdapter {
        skip_permissions: true,
        ..adapter()
    };
    let spawn = args(&a.spawn_spec(&request("gemini-x")).unwrap());
    assert!(spawn.contains(&"--dangerously-skip-permissions".to_string()));
    assert!(!spawn.contains(&"--mode".to_string()));
}

#[test]
fn lists_only_google_models() {
    let models = adapter().list_models();
    assert!(models.len() >= 3);
    assert!(models.iter().all(|m| m.id.starts_with("google/gemini-")));
}
