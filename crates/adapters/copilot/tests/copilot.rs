//! P11.S4: adapter de Copilot contra fixtures reales (L1, P11.S1/S4) y la suite de contrato.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;
use symphony_adapter_common::contract::{self, Fixtures};
use symphony_adapter_common::{
    AgentEvent, ProcessSpec, ProviderAdapter, ResumeRequest, SpawnRequest, ToolKind,
};
use symphony_adapter_copilot::{CopilotAdapter, fixtures_dir};
use symphony_core::FailureType;

fn adapter() -> CopilotAdapter {
    CopilotAdapter {
        binary: Some(std::env::current_exe().unwrap()),
        ..CopilotAdapter::default()
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

fn request(model: &str, session_id: Option<&str>) -> SpawnRequest {
    SpawnRequest {
        worktree: std::env::temp_dir(),
        model: model.into(),
        prompt: "hola".into(),
        session_id: session_id.map(str::to_string),
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
    let d = &v["data"];
    match v["type"].as_str().unwrap() {
        "assistant.message" if d["content"].as_str().is_some_and(|c| !c.trim().is_empty()) => {
            vec!["AssistantText"]
        }
        "tool.execution_start" => match d["toolName"].as_str().unwrap() {
            "powershell" | "bash" => vec!["CommandRequested"],
            "create" | "apply_patch" => vec!["ToolRequested", "FileModified"],
            _ => vec!["ToolRequested"],
        },
        "tool.execution_complete" => {
            if d["result"]["content"]
                .as_str()
                .is_some_and(|c| c.contains("exit code "))
            {
                vec!["CommandFinished"]
            } else {
                vec!["ToolFinished"]
            }
        }
        "result" => vec!["AgentStarted"],
        _ => vec![],
    }
}

fn fixtures() -> Fixtures {
    let mut stream = Vec::new();
    for file in [
        "stream.jsonl",
        "stdin.jsonl",
        "tools-denied.jsonl",
        "tools-write.jsonl",
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
                lines("error-modelo.txt").remove(0),
                FailureType::ModelUnavailable,
            ),
            // Sintéticos: no se pudo provocar cuota ni auth en vivo (cli-p11.md).
            (
                "You have exceeded your premium request quota".into(),
                FailureType::DailyQuota,
            ),
            (
                "429 Too Many Requests (rate limited)".into(),
                FailureType::TempRateLimit,
            ),
            (
                "401 Unauthorized: run copilot login (Authorization: Bearer ghp_AAAAAAAAAAAAAAAAAAAAAAAAAAAA)".into(),
                FailureType::Auth,
            ),
        ],
    }
}

#[test]
fn passes_the_contract_suite_with_real_fixtures() {
    let f = fixtures();
    assert!(
        f.stream.len() >= 60,
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
fn the_session_id_comes_with_the_final_result() {
    let a = adapter();
    let last = lines("stream.jsonl").pop().unwrap();
    assert!(matches!(
        &a.parse_stream_line(&last)[..],
        [AgentEvent::SessionStarted { cli_session_id: Some(id), .. }]
            if id == "d0d71fbc-a822-4baf-8dc8-6ac8fed3e1a5"
    ));
}

#[test]
fn tool_events_carry_command_path_and_result() {
    let a = adapter();
    let ev: Vec<AgentEvent> = lines("tools-write.jsonl")
        .iter()
        .flat_map(|l| a.parse_stream_line(l))
        .collect();
    // El parche de `apply_patch` trae la ruta en su cabecera.
    assert!(ev.iter().any(|e| matches!(
        e,
        AgentEvent::FileModified { path: Some(p), .. } if p == "hola.txt"
    )));
    assert!(ev.iter().any(|e| matches!(
        e,
        AgentEvent::ToolRequested { command: Some(c), kind: ToolKind::Command, .. }
            if c.contains("git --version")
    )));
    // Un permiso denegado es un fallo de la herramienta.
    assert!(
        ev.iter()
            .any(|e| matches!(e, AgentEvent::ToolFinished { ok: false, .. }))
    );
    assert!(
        ev.iter()
            .any(|e| matches!(e, AgentEvent::ToolFinished { ok: true, .. }))
    );
}

#[test]
fn a_shell_result_carries_its_exit_code() {
    let a = adapter();
    let codes: Vec<i32> = lines("tools-denied.jsonl")
        .iter()
        .flat_map(|l| a.parse_stream_line(l))
        .filter_map(|e| match e {
            AgentEvent::ToolFinished {
                exit_code: Some(c), ..
            } => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(codes, [0]);
}

#[test]
fn stderr_gives_the_unavailable_model_error() {
    let a = adapter();
    let ev = a.parse_stderr_line(&lines("error-modelo.txt").remove(0));
    assert!(matches!(
        &ev[..],
        [AgentEvent::ProviderError(e)] if e.failure_type == FailureType::ModelUnavailable
    ));
    assert!(a.parse_stderr_line("").is_empty());
    assert!(a.parse_stderr_line("ruido cualquiera").is_empty());
}

#[test]
fn service_errors_in_the_stream_are_classified() {
    let line = r#"{"type":"session.error","data":{"message":"429 Too Many Requests"}}"#;
    assert!(matches!(
        &adapter().parse_stream_line(line)[..],
        [AgentEvent::ProviderError(e)] if e.failure_type == FailureType::TempRateLimit
    ));
}

#[test]
fn specs_use_json_stdin_and_allow_only_writes_by_default() {
    let a = adapter();
    let spawn = args(&a.spawn_spec(&request("auto", Some("sess-9"))).unwrap());
    assert_eq!(
        spawn,
        [
            "--output-format",
            "json",
            "--no-ask-user",
            "--no-auto-update",
            "--model",
            "auto",
            "--allow-tool=write",
            "--session-id",
            "sess-9"
        ]
    );
    assert!(!spawn.contains(&"-p".to_string()));
    let resume = args(
        &a.resume_spec(&ResumeRequest {
            spawn: request("auto", None),
            cli_session_id: "sess-1".into(),
        })
        .unwrap(),
    );
    assert_eq!(resume.last().unwrap(), "--resume=sess-1");
}

#[test]
fn allow_all_tools_is_opt_in() {
    let a = CopilotAdapter {
        allow_all_tools: true,
        ..adapter()
    };
    let spawn = args(&a.spawn_spec(&request("auto", None)).unwrap());
    assert!(spawn.contains(&"--allow-all-tools".to_string()));
    assert!(!spawn.contains(&"--allow-tool=write".to_string()));
}

#[test]
fn lists_only_auto() {
    let models = adapter().list_models();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "github/auto");
}
