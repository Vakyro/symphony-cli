//! Adapter de Claude Code (`docs/research/cli-claude-code.md`, ADR-0003, ADR-0005).
//!
//! - Lanza `claude -p --input-format stream-json --output-format stream-json --verbose`
//!   (headless; los mensajes a media tarea entran por stdin).
//! - Hooks por invocación con `--settings '<json>'`: nada se escribe en el worktree.
//! - **Nunca `--bare`**: no usa el login de la suscripción.
//! - Fuentes: los hooks mandan en herramientas y turnos; el stream aporta la sesión,
//!   el texto, los reintentos de API y la cuota. Así nada se cuenta dos veces.
//! - Symphony no lee credenciales: el login es cosa del CLI.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use symphony_adapter_common::hooks::{classify_error_category, parse_standard_hook};
use symphony_adapter_common::{
    AdapterError, AgentEvent, AuthStatus, Detection, HookCommand, ModelInfo, ProcessSpec,
    ProviderAdapter, ProviderError, QuotaSnapshot, ResumeRequest, SpawnRequest,
};
use symphony_core::FailureType;

/// Eventos de hook que Symphony escucha.
const HOOK_EVENTS: [&str; 8] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "StopFailure",
    "SessionEnd",
];
/// `timeout` explícito del hook (ADR-0003): mayor que la espera máxima de `symphony hook emit`.
const HOOK_TIMEOUT_SECS: u32 = 30;

#[derive(Debug, Clone)]
pub struct ClaudeAdapter {
    /// Ejecutable a usar; `None` = detectarlo.
    pub binary: Option<PathBuf>,
    /// `--permission-mode` para headless (sin diálogos). Lo decide la config del usuario.
    pub permission_mode: String,
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self {
            binary: None,
            permission_mode: "acceptEdits".into(),
        }
    }
}

/// Busca `claude`: el binario nativo del paquete npm (evita el shim `.cmd`,
/// LEARNINGS Test A/B) o lo que haya en el PATH.
fn find_claude() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if cfg!(windows) && dir.join("claude.cmd").is_file() {
            let native = dir
                .join("node_modules")
                .join("@anthropic-ai")
                .join("claude-code")
                .join("bin")
                .join("claude.exe");
            if native.is_file() {
                return Some(native);
            }
        }
        let exe = dir.join(format!("claude{}", std::env::consts::EXE_SUFFIX));
        if exe.is_file() {
            return Some(exe);
        }
    }
    None
}

/// Comando del hook con comillas de shell tipo bash (Claude corre los hooks así, P01 Test B).
fn hook_command_line(hook: &HookCommand) -> String {
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "/").replace('"', "\\\""));
    std::iter::once(quote(&hook.program.to_string_lossy()))
        .chain(hook.args.iter().map(|a| quote(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// JSON para `--settings`: cada evento llama al hook de Symphony.
pub fn hook_settings(hook: &HookCommand) -> Value {
    let handler = json!([{ "hooks": [{ "type": "command", "command": hook_command_line(hook), "timeout": HOOK_TIMEOUT_SECS }] }]);
    let hooks: serde_json::Map<String, Value> = HOOK_EVENTS
        .iter()
        .map(|e| ((*e).to_string(), handler.clone()))
        .collect();
    json!({ "hooks": hooks })
}

fn user_message(text: &str) -> Vec<u8> {
    let mut line =
        json!({ "type": "user", "message": { "role": "user", "content": text } }).to_string();
    line.push('\n');
    line.into_bytes()
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

impl ClaudeAdapter {
    fn binary(&self) -> Result<PathBuf, AdapterError> {
        self.binary
            .clone()
            .or_else(find_claude)
            .ok_or_else(|| AdapterError::NotInstalled("claude".into()))
    }

    fn base_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError> {
        let mut spec = ProcessSpec::new(self.binary()?)
            .args([
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
            ])
            .args(["--model", &req.model])
            .args(["--permission-mode", &self.permission_mode])
            .cwd(&req.worktree)
            .with_stdin();
        if let Some(hook) = &req.hook {
            spec = spec.arg("--settings").arg(hook_settings(hook).to_string());
        }
        for (k, v) in &req.env {
            spec = spec.env(k, v);
        }
        Ok(spec)
    }
}

impl ProviderAdapter for ClaudeAdapter {
    fn provider_id(&self) -> &'static str {
        "anthropic"
    }

    fn display_name(&self) -> &'static str {
        "Claude"
    }

    fn detect(&self) -> Result<Detection, AdapterError> {
        let cli_path = self.binary()?;
        let out = Command::new(&cli_path)
            .arg("--version")
            .stdin(Stdio::null())
            .output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        // "2.1.281 (Claude Code)"
        let version = text.split_whitespace().next().unwrap_or("").to_string();
        if !out.status.success() || version.is_empty() {
            return Err(AdapterError::Invalid(format!(
                "`claude --version` respondió: {}",
                text.trim()
            )));
        }
        Ok(Detection { cli_path, version })
    }

    /// Sin leer credenciales no hay forma confiable de saberlo antes de correr:
    /// el primer `StopFailure(authentication_failed)` lo dice.
    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Unknown
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        // Alias estables de Claude Code (model-config); el modelo real sale de `system/init`.
        [
            ("opus", "Opus"),
            ("sonnet", "Sonnet"),
            ("haiku", "Haiku"),
            ("fable", "Fable"),
        ]
        .iter()
        .map(|(alias, name)| ModelInfo {
            id: format!("claude/{alias}"),
            cli_model_id: (*alias).into(),
            display_name: format!("Claude {name}"),
        })
        .collect()
    }

    fn supports_hooks(&self) -> bool {
        true
    }

    fn spawn_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError> {
        let spec = self.base_spec(req)?;
        Ok(match &req.session_id {
            Some(id) => spec.args(["--session-id", id]),
            None => spec,
        })
    }

    fn resume_spec(&self, req: &ResumeRequest) -> Result<ProcessSpec, AdapterError> {
        Ok(self
            .base_spec(&req.spawn)?
            .args(["--resume", &req.cli_session_id]))
    }

    fn encode_prompt(&self, prompt: &str) -> Vec<u8> {
        user_message(prompt)
    }

    fn encode_user_message(&self, text: &str) -> Option<Vec<u8>> {
        Some(user_message(text))
    }

    fn parse_stream_line(&self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        match (
            v.get("type").and_then(Value::as_str),
            v.get("subtype").and_then(Value::as_str),
        ) {
            (Some("system"), Some("init")) => {
                vec![AgentEvent::SessionStarted {
                    cli_session_id: s(&v, "session_id"),
                    model: s(&v, "model"),
                }]
            }
            (Some("system"), Some("api_retry")) => {
                let category = s(&v, "error").unwrap_or_else(|| "unknown".into());
                vec![AgentEvent::ProviderError(ProviderError {
                    failure_type: classify_error_category(&category)
                        .unwrap_or(FailureType::Unknown),
                    raw_code: v
                        .get("error_status")
                        .filter(|c| !c.is_null())
                        .map(|c| c.to_string())
                        .or(Some(category)),
                    message: String::new(),
                    retry_after_ms: v.get("retry_delay_ms").and_then(Value::as_u64),
                    resets_at: None,
                    transient: true,
                })]
            }
            (Some("assistant"), _) => v["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|c| c["type"] == "text")
                .filter_map(|c| c["text"].as_str())
                .filter(|t| !t.trim().is_empty())
                .map(|t| AgentEvent::AssistantText {
                    text: t.to_string(),
                })
                .collect(),
            // Eco de un mensaje del usuario (`--replay-user-messages`); los tool_result van por hooks.
            (Some("user"), _) if v.get("isReplay").and_then(Value::as_bool) == Some(true) => {
                match &v["message"]["content"] {
                    Value::String(t) => vec![AgentEvent::UserMessage { text: t.clone() }],
                    _ => Vec::new(),
                }
            }
            (Some("rate_limit_event"), _) => v["rate_limit_info"]["unifiedWindows"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(window, w)| {
                    Some(AgentEvent::Quota(QuotaSnapshot {
                        window: window.clone(),
                        used_fraction: w.get("utilization")?.as_f64()?,
                        resets_at: w.get("resetsAt").and_then(Value::as_i64),
                    }))
                })
                .collect(),
            (Some("result"), _) if v.get("is_error").and_then(Value::as_bool) == Some(true) => {
                let text = s(&v, "result").unwrap_or_default();
                self.parse_error(&text)
                    .map(|e| vec![AgentEvent::ProviderError(e)])
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    fn parse_hook(&self, payload: &Value) -> Vec<AgentEvent> {
        parse_standard_hook(payload)
    }

    fn parse_error(&self, text: &str) -> Option<ProviderError> {
        let lower = text.to_lowercase();
        let (failure_type, code) = if lower.contains("usage limit")
            || lower.contains("limit will reset")
            || lower.contains("weekly limit")
        {
            (
                if lower.contains("weekly") {
                    FailureType::WeeklyQuota
                } else {
                    FailureType::DailyQuota
                },
                "usage_limit",
            )
        } else if lower.contains("rate_limit")
            || lower.contains("rate limit")
            || lower.contains("429")
        {
            (FailureType::TempRateLimit, "rate_limit")
        } else if lower.contains("authentication")
            || lower.contains("/login")
            || lower.contains("not logged in")
            || lower.contains("401")
        {
            (FailureType::Auth, "authentication_failed")
        } else if lower.contains("overloaded")
            || lower.contains("529")
            || lower.contains("server_error")
        {
            (FailureType::ProviderError, "overloaded")
        } else if lower.contains("model_not_found") || lower.contains("model not found") {
            (FailureType::ModelUnavailable, "model_not_found")
        } else if lower.contains("connection error")
            || lower.contains("network")
            || lower.contains("enotfound")
            || lower.contains("econnreset")
        {
            (FailureType::Network, "network")
        } else {
            return None;
        };
        Some(ProviderError {
            failure_type,
            raw_code: Some(code.into()),
            message: symphony_core::redact(text).chars().take(500).collect(),
            retry_after_ms: None,
            resets_at: None,
            transient: failure_type == FailureType::TempRateLimit,
        })
    }
}

/// Fixtures L1 reales, sanitizados (`fixtures/providers/claude-code/`).
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/providers/claude-code")
}
