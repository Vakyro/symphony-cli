//! P11.S2: adapter de Kimi contra fixtures reales (L1, P11.S1) y la suite de contrato.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use symphony_adapter_common::contract::{self, Fixtures};
use symphony_adapter_common::{AgentEvent, ProviderAdapter, ResumeRequest, SpawnRequest, ToolKind};
use symphony_adapter_kimi::{KimiAdapter, fixtures_dir, session_from_stderr};
use symphony_core::FailureType;

fn adapter() -> KimiAdapter {
    KimiAdapter {
        binary: Some(std::env::current_exe().unwrap()),
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

fn args(spec: &symphony_adapter_common::ProcessSpec) -> Vec<String> {
    spec.args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

fn names(events: &[AgentEvent]) -> Vec<&'static str> {
    events.iter().map(AgentEvent::type_name).collect()
}

fn fixtures() -> Fixtures {
    let mut stream = Vec::new();
    // Un turno sin herramientas: una sola respuesta.
    for l in lines("stream.jsonl") {
        stream.push((l, vec!["AssistantText"]));
    }
    // WriteFile + Shell, sus resultados y la respuesta final.
    let want = [
        vec!["ToolRequested", "FileModified", "CommandRequested"],
        vec!["ToolFinished"],
        vec!["CommandFinished"],
        vec!["AssistantText"],
    ];
    for (l, w) in lines("tools.jsonl").into_iter().zip(want) {
        stream.push((l, w));
    }
    // ReadFile + Shell que fallan.
    let want = [
        vec!["ToolRequested", "CommandRequested"],
        vec!["ToolFinished"],
        vec!["CommandFinished"],
        vec!["AssistantText"],
    ];
    for (l, w) in lines("tools-error.jsonl").into_iter().zip(want) {
        stream.push((l, w));
    }
    // `LLM not set` sale como texto plano por stdout.
    stream.push((lines("error-modelo.txt").remove(0), vec!["ProviderError"]));
    Fixtures {
        stream,
        hooks: vec![],
        errors: vec![
            ("LLM not set".into(), FailureType::ModelUnavailable),
            // Sintéticos: no se pudo provocar cuota ni auth en vivo (cli-p11.md).
            ("Error 429: too many requests".into(), FailureType::TempRateLimit),
            ("insufficient balance, please recharge".into(), FailureType::DailyQuota),
            ("401 Unauthorized: run kimi login (Authorization: Bearer sk-AAAAAAAAAAAAAAAAAAAAAAAA)".into(), FailureType::Auth),
        ],
    }
}

#[test]
fn passes_the_contract_suite_with_real_fixtures() {
    let f = fixtures();
    assert!(
        f.stream.len() >= 9,
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
fn tool_events_carry_command_path_and_result() {
    let a = adapter();
    let ok = lines("tools.jsonl");
    let ev = a.parse_stream_line(&ok[0]);
    assert!(matches!(
        &ev[0],
        AgentEvent::ToolRequested { tool, kind: ToolKind::Edit, .. } if tool == "WriteFile"
    ));
    assert!(matches!(
        &ev[1],
        AgentEvent::FileModified { path: Some(p), .. } if p == "hola.txt"
    ));
    assert!(matches!(
        &ev[2],
        AgentEvent::ToolRequested { command: Some(c), .. } if c == "echo listo"
    ));
    assert!(matches!(
        a.parse_stream_line(&ok[2])[0],
        AgentEvent::ToolFinished {
            ok: true,
            exit_code: Some(0),
            ..
        }
    ));

    let bad = lines("tools-error.jsonl");
    assert!(matches!(
        a.parse_stream_line(&bad[1])[0],
        AgentEvent::ToolFinished { ok: false, .. }
    ));
    assert!(matches!(
        a.parse_stream_line(&bad[2])[0],
        AgentEvent::ToolFinished {
            ok: false,
            exit_code: Some(3),
            ..
        }
    ));
}

#[test]
fn thinking_alone_is_not_assistant_text() {
    let line =
        r#"{"role":"assistant","content":[{"type":"think","think":"mmm","encrypted":null}]}"#;
    assert!(names(&adapter().parse_stream_line(line)).is_empty());
}

#[test]
fn session_id_comes_from_stderr_only() {
    let stderr = lines("stderr-session.txt").remove(0);
    assert_eq!(
        session_from_stderr(&stderr).as_deref(),
        Some("0ef4ce57-f034-48b3-ba6d-8abd10c6b84a")
    );
    assert_eq!(session_from_stderr("To resume this session: kimi -r"), None);
    assert_eq!(session_from_stderr("otra cosa"), None);
}

#[test]
fn spawn_and_resume_specs_use_print_mode_and_the_session_flag() {
    let a = adapter();
    let spawn = args(&a.spawn_spec(&request("kimi-x", Some("abc"))).unwrap());
    assert_eq!(
        spawn,
        [
            "--print",
            "--output-format",
            "stream-json",
            "-m",
            "kimi-x",
            "-S",
            "abc"
        ]
    );
    let resume = args(
        &a.resume_spec(&ResumeRequest {
            spawn: request("kimi-x", None),
            cli_session_id: "sess-1".into(),
        })
        .unwrap(),
    );
    assert_eq!(resume[resume.len() - 2..], ["-S", "sess-1"]);
}

#[test]
fn default_model_does_not_pass_dash_m() {
    let spec = adapter().spawn_spec(&request("default", None)).unwrap();
    assert!(!args(&spec).contains(&"-m".to_string()));
    let attach = adapter()
        .attach_spec("sess-1", "default", &std::env::temp_dir())
        .unwrap();
    assert_eq!(args(&attach), ["-S", "sess-1"]);
}

#[test]
fn lists_one_provider_scoped_model() {
    let models = adapter().list_models();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "moonshot/default");
}

#[test]
fn stderr_lines_give_the_session_id_and_errors() {
    let a = adapter();
    let stderr = lines("stderr-session.txt").remove(0);
    assert!(matches!(
        &a.parse_stderr_line(&stderr)[..],
        [AgentEvent::SessionStarted { cli_session_id: Some(id), .. }]
            if id == "0ef4ce57-f034-48b3-ba6d-8abd10c6b84a"
    ));
    assert_eq!(
        names(&a.parse_stderr_line("Error 429: too many requests")),
        ["ProviderError"]
    );
    assert!(a.parse_stderr_line("").is_empty());
    assert!(a.parse_stderr_line("ruido cualquiera").is_empty());
}
