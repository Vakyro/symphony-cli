//! P05.S1: el adapter `fake` pasa la suite de contrato y funciona de punta a punta.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use serde_json::json;
use symphony_adapter_common::contract::{self, Fixtures};
use symphony_adapter_common::{AgentEvent, HookCommand, ProviderAdapter, SpawnRequest};
use symphony_core::FailureType;
use symphony_process::{OutputLine, spawn};
use symphony_testkit::FakeAdapter;

const BIN: &str = env!("CARGO_BIN_EXE_fake-agent");

fn fixtures() -> Fixtures {
    Fixtures {
        stream: vec![
            (r#"{"type":"system","subtype":"init","session_id":"s","model":"fast"}"#.into(), vec!["AgentStarted"]),
            (r#"{"type":"assistant","text":"hola"}"#.into(), vec!["AssistantText"]),
            (r#"{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"npm test"}}"#.into(), vec!["CommandRequested"]),
            (r#"{"type":"tool_result","id":"t1","name":"Bash","ok":true,"output":{"exit_code":0}}"#.into(), vec!["CommandFinished"]),
            (r#"{"type":"tool_use","id":"t2","name":"Write","input":{}}"#.into(), vec!["ToolRequested"]),
            (r#"{"type":"system","subtype":"api_retry","error":"rate_limit","error_status":429,"retry_delay_ms":100}"#.into(), vec!["ProviderError"]),
            (r#"{"type":"error","error":"quota_exhausted","resets_at":1790300000}"#.into(), vec!["ProviderError"]),
            (r#"{"type":"result","subtype":"success"}"#.into(), vec!["TurnFinished"]),
        ],
        hooks: vec![
            (json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "ls"}}), vec!["CommandRequested"]),
            (json!({"hook_event_name": "PostToolUse", "tool_name": "Write", "tool_input": {"file_path": "a"}}), vec!["ToolFinished", "FileModified"]),
        ],
        errors: vec![
            ("error: quota_exhausted".into(), FailureType::DailyQuota),
            ("authentication_failed (Authorization: Bearer sk-ant-api03-XXXXXXXXXXXXXXXX)".into(), FailureType::Auth),
            ("rate_limit 429".into(), FailureType::TempRateLimit),
        ],
    }
}

#[test]
fn fake_adapter_passes_the_contract_suite() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = FakeAdapter::new(BIN, dir.path().join("s.toml"));
    let failures = contract::check(&adapter, &fixtures());
    assert!(
        failures.is_empty(),
        "fallas del contrato:\n{}",
        failures.join("\n")
    );
    assert!(adapter.detect().is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn fake_adapter_end_to_end_stream_and_hooks() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("s.toml");
    std::fs::write(
        &script,
        "[[step]]\nkind = \"edit\"\npath = \"a.js\"\ncontent = \"1\"\n[[step]]\nkind = \"rate_limit\"\nretry_after_ms = 10\n[[step]]\nkind = \"say\"\ntext = \"listo\"\n",
    )
    .unwrap();
    let hooks = dir.path().join("hooks.jsonl");
    let adapter = FakeAdapter::new(BIN, &script);
    let req = SpawnRequest {
        worktree: dir.path().to_path_buf(),
        model: "smart".into(),
        prompt: "creá a.js".into(),
        session_id: Some("sess-1".into()),
        hook: Some(HookCommand {
            program: BIN.into(),
            args: vec!["record-hook".into(), hooks.display().to_string()],
        }),
        env: vec![("SYMPHONY_AGENT_ID".into(), "agent-1".into())],
    };
    let mut proc = spawn(adapter.spawn_spec(&req).unwrap()).await.unwrap();
    proc.write_stdin(&adapter.encode_prompt(&req.prompt))
        .await
        .unwrap();

    let mut stream = Vec::new();
    while let Some(line) = tokio::time::timeout(Duration::from_secs(20), proc.next_output())
        .await
        .unwrap()
    {
        if let OutputLine::Stdout(l) = line {
            let events = adapter.parse_stream_line(&l);
            // Como Claude, el CLI espera otro mensaje hasta que le cierren stdin.
            if events
                .iter()
                .any(|e| matches!(e, AgentEvent::TurnFinished { .. }))
            {
                proc.close_stdin().await.unwrap();
            }
            stream.extend(events);
        }
    }
    assert!(proc.wait().await.success());
    let names: Vec<_> = stream.iter().map(AgentEvent::type_name).collect();
    assert_eq!(
        names,
        [
            "AgentStarted",
            "ToolRequested",
            "ToolFinished",
            "ProviderError",
            "AssistantText",
            "TurnFinished"
        ]
    );
    assert_eq!(
        stream[0],
        AgentEvent::SessionStarted {
            cli_session_id: Some("sess-1".into()),
            model: Some("smart".into())
        }
    );
    let AgentEvent::ProviderError(e) = &stream[3] else {
        unreachable!()
    };
    assert!(e.transient && e.failure_type == FailureType::TempRateLimit);

    let from_hooks: Vec<_> = std::fs::read_to_string(&hooks)
        .unwrap()
        .lines()
        .flat_map(|l| adapter.parse_hook(&serde_json::from_str(l).unwrap()))
        .map(|e| e.type_name())
        .collect();
    assert_eq!(
        from_hooks,
        [
            "AgentStarted",
            "TurnStarted",
            "ToolRequested",
            "ToolFinished",
            "FileModified",
            "TurnFinished"
        ]
    );
}
