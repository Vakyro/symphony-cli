//! P04.S5: cada escenario del fake-agent tiene un test que valida su salida.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_fake-agent");

struct Run {
    dir: tempfile::TempDir,
    hooks: PathBuf,
    transcript: PathBuf,
}

impl Run {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let hooks = dir.path().join("hooks.jsonl");
        let transcript = dir.path().join("transcript.jsonl");
        Self {
            dir,
            hooks,
            transcript,
        }
    }

    fn command(&self, script: &str) -> Command {
        let path = self.dir.path().join("script.toml");
        std::fs::write(&path, script).unwrap();
        let mut cmd = Command::new(BIN);
        cmd.args(["run", "--script"])
            .arg(&path)
            .args(["--hook", BIN, "--hook-arg", "record-hook", "--hook-arg"])
            .arg(&self.hooks)
            .arg("--transcript")
            .arg(&self.transcript)
            .current_dir(self.dir.path());
        cmd
    }

    fn run(&self, script: &str) -> Output {
        self.command(script).stdin(Stdio::null()).output().unwrap()
    }

    fn hook_events(&self) -> Vec<(String, String)> {
        jsonl(&self.hooks)
            .into_iter()
            .map(|h| {
                (
                    h["hook_event_name"].as_str().unwrap_or("").to_string(),
                    h["tool_name"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect()
    }
}

fn jsonl(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn events(out: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn types(evs: &[Value]) -> Vec<String> {
    evs.iter()
        .map(|e| e["type"].as_str().unwrap_or("").to_string())
        .collect()
}

#[test]
fn normal_turn_streams_edits_runs_and_calls_hooks() {
    let r = Run::new();
    let out = r.run(
        r#"
model = "fake/sonnet"
session_id = "s-123"
[[step]]
kind = "say"
text = "Voy a crear el archivo"
[[step]]
kind = "edit"
path = "src/a.js"
content = "export const a = 1;\n"
[[step]]
kind = "run"
command = ["git", "--version"]
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let evs = events(&out);
    assert_eq!(
        types(&evs),
        [
            "system",
            "assistant",
            "tool_use",
            "tool_result",
            "tool_use",
            "tool_result",
            "result"
        ]
    );
    assert_eq!(evs[0]["model"], "fake/sonnet");
    assert_eq!(evs[0]["session_id"], "s-123");
    assert!(
        evs[5]["output"]["stdout"]
            .as_str()
            .unwrap()
            .starts_with("git version")
    );
    assert_eq!(
        std::fs::read_to_string(r.dir.path().join("src/a.js")).unwrap(),
        "export const a = 1;\n"
    );

    let hooks = r.hook_events();
    let expected: Vec<(String, String)> = [
        ("SessionStart", ""),
        ("UserPromptSubmit", ""),
        ("PreToolUse", "Write"),
        ("PostToolUse", "Write"),
        ("PreToolUse", "Bash"),
        ("PostToolUse", "Bash"),
        ("Stop", ""),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    assert_eq!(hooks, expected);
    // Cada payload trae los campos comunes de un hook real.
    let first = &jsonl(&r.hooks)[0];
    assert_eq!(first["session_id"], "s-123");
    assert_eq!(first["model"], "fake/sonnet");
    assert!(first["transcript_path"].is_string() && first["cwd"].is_string());

    // Transcript al estilo de Claude Code.
    let t = jsonl(&r.transcript);
    assert_eq!(
        t[0]["message"]["content"][0]["text"],
        "Voy a crear el archivo"
    );
    assert!(
        t.iter()
            .any(|e| e["message"]["content"][0]["name"] == "Bash")
    );
}

#[test]
fn pre_tool_use_deny_blocks_the_command() {
    let r = Run::new();
    let out = r
        .command("[[step]]\nkind = \"run\"\ncommand = [\"git\", \"init\", \"-q\", \"no-deberia-existir\"]\n")
        .env("FAKE_HOOK_DENY", "git init")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());
    let evs = events(&out);
    assert_eq!(evs[2]["denied"], true);
    assert!(!r.dir.path().join("no-deberia-existir").exists());
    assert!(!r.hook_events().iter().any(|(e, _)| e == "PostToolUse"));
}

#[test]
fn temporary_rate_limit_emits_retry_and_continues() {
    let r = Run::new();
    let out = r.run("[[step]]\nkind = \"rate_limit\"\nretry_after_ms = 50\n[[step]]\nkind = \"say\"\ntext = \"sigo\"\n");
    assert!(out.status.success());
    let evs = events(&out);
    let retry = evs.iter().find(|e| e["subtype"] == "api_retry").unwrap();
    assert_eq!(
        (retry["error"].as_str(), retry["error_status"].as_i64()),
        (Some("rate_limit"), Some(429))
    );
    assert_eq!(types(&evs).last().map(String::as_str), Some("result"));
}

#[test]
fn quota_exhausted_and_auth_error_fail_with_typed_errors() {
    for (step, error) in [
        (
            "kind = \"quota_exhausted\"\nresets_at = 1790300000",
            "quota_exhausted",
        ),
        ("kind = \"auth_error\"", "authentication_failed"),
    ] {
        let r = Run::new();
        let out = r.run(&format!(
            "[[step]]\n{step}\n[[step]]\nkind = \"say\"\ntext = \"no llega\"\n"
        ));
        assert_eq!(out.status.code(), Some(1), "{error}");
        let evs = events(&out);
        let last = evs.last().unwrap();
        assert_eq!(
            (last["type"].as_str(), last["error"].as_str()),
            (Some("error"), Some(error))
        );
        assert!(!types(&evs).contains(&"assistant".to_string()));
        assert!(r.hook_events().iter().any(|(e, _)| e == "StopFailure"));
    }
}

#[test]
fn crash_dies_without_result() {
    let r = Run::new();
    let out = r.run("[[step]]\nkind = \"say\"\ntext = \"antes\"\n[[step]]\nkind = \"crash\"\n");
    assert!(!out.status.success());
    let evs = events(&out);
    assert!(!types(&evs).contains(&"result".to_string()));
    assert!(
        !r.hook_events().iter().any(|(e, _)| e == "Stop"),
        "un crash no llama Stop"
    );
}

#[test]
fn hang_keeps_running_until_killed() {
    let r = Run::new();
    let mut child = r
        .command("[[step]]\nkind = \"hang\"\n")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        child.try_wait().unwrap().is_none(),
        "debería seguir colgado"
    );
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn wait_input_reads_a_user_message_from_stdin() {
    let r = Run::new();
    let mut child = r
        .command("[[step]]\nkind = \"wait_input\"\n[[step]]\nkind = \"say\"\ntext = \"recibido\"\n")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all("agregá tests también\n".as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let evs = events(&out);
    let user = evs.iter().find(|e| e["type"] == "user").unwrap();
    assert_eq!(user["text"], "agregá tests también");
    assert_eq!(evs.last().unwrap()["num_turns"], 2);
}

#[test]
fn invalid_script_is_rejected() {
    let r = Run::new();
    let out = r.run("[[step]]\nkind = \"teleport\"\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("guion inválido"));
}
