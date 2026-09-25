//! `fake-agent`: imita un CLI de proveedor para E2E sin gastar suscripciones.
//!
//!   fake-agent run --script <guion.toml> [--hook <programa> [--hook-arg <a>]...] [--transcript <archivo.jsonl>] [--model <m>] [--session-id <id>]
//!   fake-agent record-hook <archivo.jsonl>
//!
//! `run` emite un stream JSONL por stdout (al estilo de `claude -p --output-format
//! stream-json`), llama al hook como lo haría un CLI real (JSON por stdin) y
//! respeta un `deny` de `PreToolUse`.
//! `record-hook` sirve de hook en los tests: agrega el payload a un archivo y,
//! si `FAKE_HOOK_DENY` aparece en el comando, responde `deny`.

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use symphony_testkit::{Script, Step};

struct Agent {
    session_id: String,
    model: String,
    cwd: PathBuf,
    hook: Option<(String, Vec<String>)>,
    transcript: Option<PathBuf>,
    turns: u32,
}

fn emit(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

impl Agent {
    fn transcript(&self, entry: &Value) {
        if let Some(path) = &self.transcript
            && let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
        {
            let _ = writeln!(f, "{entry}");
        }
    }

    /// Llama al hook con el payload por stdin. Devuelve `true` si el hook dijo `deny`.
    fn hook(&self, event: &str, extra: Value) -> bool {
        let Some((program, args)) = &self.hook else {
            return false;
        };
        let mut payload = json!({
            "session_id": self.session_id,
            "transcript_path": self.transcript,
            "cwd": self.cwd,
            "hook_event_name": event,
            "model": self.model,
        });
        if let (Some(p), Some(e)) = (payload.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        let Ok(mut child) = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(payload.to_string().as_bytes());
        }
        let Ok(out) = child.wait_with_output() else {
            return false;
        };
        let reply: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
        out.status.code() == Some(2) || reply["hookSpecificOutput"]["permissionDecision"] == "deny"
    }

    fn say(&mut self, text: &str) {
        emit(&json!({"type": "assistant", "session_id": self.session_id, "text": text}));
        self.transcript(
            &json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}),
        );
    }

    fn tool(&mut self, tool: &str, input: Value, act: impl FnOnce() -> (bool, Value)) {
        let id = format!("toolu_{}", now_ms());
        emit(&json!({"type": "tool_use", "id": id, "name": tool, "input": input}));
        self.transcript(&json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": id, "name": tool, "input": input}]}}));
        if self.hook(
            "PreToolUse",
            json!({"tool_name": tool, "tool_input": input, "tool_use_id": id}),
        ) {
            emit(&json!({"type": "tool_result", "id": id, "name": tool, "denied": true}));
            return;
        }
        let (ok, response) = act();
        let event = if ok {
            "PostToolUse"
        } else {
            "PostToolUseFailure"
        };
        self.hook(event, json!({"tool_name": tool, "tool_input": input, "tool_use_id": id, "tool_response": response}));
        emit(&json!({"type": "tool_result", "id": id, "name": tool, "ok": ok, "output": response}));
    }

    fn fail(&mut self, error: &str, extra: Value) -> ExitCode {
        let mut ev = json!({"type": "error", "error": error, "session_id": self.session_id});
        if let (Some(e), Some(x)) = (ev.as_object_mut(), extra.as_object()) {
            e.extend(x.clone());
        }
        emit(&ev);
        self.hook("StopFailure", json!({"error": error}));
        ExitCode::FAILURE
    }

    fn run(&mut self, steps: &[Step]) -> ExitCode {
        emit(
            &json!({"type": "system", "subtype": "init", "session_id": self.session_id, "model": self.model, "cwd": self.cwd}),
        );
        self.hook("SessionStart", json!({"source": "startup"}));
        self.hook("UserPromptSubmit", json!({}));
        for step in steps {
            match step {
                Step::Say { text } => self.say(text),
                Step::Edit { path, content } => {
                    let full = self.cwd.join(path);
                    let input = json!({"file_path": full, "content": content});
                    self.tool("Write", input, || {
                        let r = full
                            .parent()
                            .map_or(Ok(()), std::fs::create_dir_all)
                            .and_then(|()| std::fs::write(&full, content));
                        (
                            r.is_ok(),
                            json!(
                                r.map(|()| "ok".to_string())
                                    .unwrap_or_else(|e| e.to_string())
                            ),
                        )
                    });
                }
                Step::Run { command } => {
                    let Some((program, args)) = command.split_first() else {
                        continue;
                    };
                    let cwd = self.cwd.clone();
                    self.tool(
                        "Bash",
                        json!({"command": command.join(" ")}),
                        || match Command::new(program)
                            .args(args)
                            .current_dir(&cwd)
                            .stdin(Stdio::null())
                            .output()
                        {
                            Ok(o) => (
                                o.status.success(),
                                json!({
                                    "stdout": String::from_utf8_lossy(&o.stdout),
                                    "stderr": String::from_utf8_lossy(&o.stderr),
                                    "exit_code": o.status.code()
                                }),
                            ),
                            Err(e) => (false, json!({"error": e.to_string()})),
                        },
                    );
                }
                Step::WaitInput => {
                    let mut line = String::new();
                    if std::io::stdin().lock().read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let text = line.trim_end().to_string();
                    emit(&json!({"type": "user", "text": text}));
                    self.transcript(&json!({"type": "user", "message": {"content": text}}));
                    self.turns += 1;
                }
                Step::Sleep { ms } => std::thread::sleep(Duration::from_millis(*ms)),
                Step::RateLimit { retry_after_ms } => {
                    emit(
                        &json!({"type": "system", "subtype": "api_retry", "error": "rate_limit", "error_status": 429, "retry_delay_ms": retry_after_ms}),
                    );
                    std::thread::sleep(Duration::from_millis(*retry_after_ms));
                }
                Step::QuotaExhausted { resets_at } => {
                    return self.fail("quota_exhausted", json!({"resets_at": resets_at}));
                }
                Step::AuthError => return self.fail("authentication_failed", json!({})),
                Step::Crash => std::process::abort(),
                Step::Hang => loop {
                    std::thread::sleep(Duration::from_secs(3600));
                },
            }
        }
        self.hook("Stop", json!({"last_assistant_message": null}));
        emit(
            &json!({"type": "result", "subtype": "success", "session_id": self.session_id, "num_turns": self.turns + 1}),
        );
        ExitCode::SUCCESS
    }
}

fn record_hook(file: &Path) -> ExitCode {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return ExitCode::FAILURE;
    }
    let payload: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
    {
        let _ = writeln!(f, "{payload}");
    }
    if let Ok(deny) = std::env::var("FAKE_HOOK_DENY")
        && payload["hook_event_name"] == "PreToolUse"
        && payload["tool_input"]["command"]
            .as_str()
            .is_some_and(|c| c.contains(&deny))
    {
        println!(
            "{}",
            json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny"}})
        );
    }
    ExitCode::SUCCESS
}

fn usage() -> ExitCode {
    eprintln!(
        "uso: fake-agent run --script <guion.toml> [--hook <prog> [--hook-arg <a>]...] [--transcript <f>]\n     fake-agent record-hook <archivo>"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("record-hook") => args
            .get(1)
            .map_or_else(usage, |f| record_hook(Path::new(f))),
        Some("run") => {
            let (mut script, mut hook, mut hook_args, mut transcript) =
                (None, None, Vec::new(), None);
            let (mut model, mut session): (Option<String>, Option<String>) = (None, None);
            let mut it = args.iter().skip(1);
            while let Some(a) = it.next() {
                match (a.as_str(), it.next()) {
                    ("--script", Some(v)) => script = Some(PathBuf::from(v)),
                    ("--hook", Some(v)) => hook = Some(v.clone()),
                    ("--hook-arg", Some(v)) => hook_args.push(v.clone()),
                    ("--transcript", Some(v)) => transcript = Some(PathBuf::from(v)),
                    ("--model", Some(v)) => model = Some(v.clone()),
                    ("--session-id", Some(v)) => session = Some(v.clone()),
                    _ => return usage(),
                }
            }
            let Some(script_path) = script else {
                return usage();
            };
            let parsed = std::fs::read_to_string(&script_path)
                .map_err(|e| e.to_string())
                .and_then(|t| Script::parse(&t).map_err(|e| e.to_string()));
            let script = match parsed {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("fake-agent: guion inválido {}: {e}", script_path.display());
                    return ExitCode::from(2);
                }
            };
            let mut agent = Agent {
                session_id: session
                    .or_else(|| script.session_id.clone())
                    .unwrap_or_else(|| format!("fake-{}", now_ms())),
                model: model.unwrap_or_else(|| script.model.clone()),
                cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                hook: hook.map(|h| (h, hook_args)),
                transcript,
                turns: 0,
            };
            agent.run(&script.steps)
        }
        _ => usage(),
    }
}
