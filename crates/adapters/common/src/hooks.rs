//! Hooks con el esquema común de Claude Code y Codex (P01 Test B,
//! `docs/research/cli-*.md`): `hook_event_name`, `session_id`, `tool_name`,
//! `tool_input`, `tool_use_id`, `tool_response`, `last_assistant_message`, `error`.
//! Los adapters lo reutilizan; lo que no es común lo agregan ellos.

use serde_json::Value;
use symphony_core::FailureType;

use crate::{AgentEvent, ProviderError, ToolKind};

/// `Bash`/`exec_command` → comando; `Write`/`Edit`/`MultiEdit`/`apply_patch` → edición.
/// Tiempos de un hook que puede retener una operación (ADR-0003). Cada nivel tiene que ser
/// menor que el siguiente: el daemon retiene, `hook emit` espera un poco más y el CLI
/// mata el hook un poco después (Codex no mata a los vencidos: por eso el cliente corta antes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldTimes {
    /// Cuánto retiene el daemon antes de rechazar con una razón.
    pub hold_secs: u64,
    /// Cuánto espera `symphony hook emit` la respuesta del daemon.
    pub client_secs: u64,
    /// `timeout` que se escribe en el hook del CLI.
    pub hook_secs: u64,
}

impl HoldTimes {
    pub const CLAUDE: Self = Self {
        hold_secs: 100,
        client_secs: 110,
        hook_secs: 120,
    };
    pub const CODEX: Self = Self {
        hold_secs: 40,
        client_secs: 50,
        hook_secs: 60,
    };

    /// Claude Code aguanta esperas largas con un `timeout` explícito; Codex hasta ~60 s
    /// (medido en P01, Test C).
    pub fn for_provider(provider: Option<&str>) -> Self {
        match provider {
            Some("openai") => Self::CODEX,
            _ => Self::CLAUDE,
        }
    }
}

/// El nombre de una herramienta sin el prefijo del broker del harness (ADR-0019): Claude la ve
/// como `mcp__symphony-context__shell` y OpenCode como `symphony-context_shell`.
pub fn harness_tool_name(tool: &str) -> &str {
    tool.strip_prefix("mcp__symphony-context__")
        .or_else(|| tool.strip_prefix("symphony-context_"))
        .unwrap_or(tool)
}

pub fn tool_kind(tool: &str) -> ToolKind {
    match harness_tool_name(tool) {
        "Bash" | "exec_command" | "shell" => ToolKind::Command,
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" | "apply_patch" | "write" | "edit" => {
            ToolKind::Edit
        }
        _ => ToolKind::Other,
    }
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Categoría de error tipada de Claude Code (`StopFailure.error`, `api_retry.error`).
pub fn classify_error_category(category: &str) -> Option<FailureType> {
    Some(match category {
        "rate_limit" => FailureType::TempRateLimit,
        "authentication_failed" | "oauth_org_not_allowed" | "account_on_hold" => FailureType::Auth,
        "billing_error" => FailureType::AccountLimit,
        "overloaded" | "server_error" => FailureType::ProviderError,
        "model_not_found" => FailureType::ModelUnavailable,
        "max_output_tokens" => FailureType::ModelLimit,
        "invalid_request" | "unknown" => FailureType::Unknown,
        _ => return None,
    })
}

/// Traduce un payload de hook a eventos canónicos. Lo desconocido se ignora.
pub fn parse_standard_hook(payload: &Value) -> Vec<AgentEvent> {
    let Some(event) = payload.get("hook_event_name").and_then(Value::as_str) else {
        return Vec::new();
    };
    let tool = str_field(payload, "tool_name").unwrap_or_default();
    let kind = tool_kind(&tool);
    let tool_use_id = str_field(payload, "tool_use_id");
    let input = payload.get("tool_input");
    // Los comandos van a `events` y a `tool_calls.command`: siempre redactados (DB §3.F).
    let command = input
        .and_then(|i| i.get("command"))
        .and_then(Value::as_str)
        .map(|c| symphony_core::redact(c).into_owned());
    let path = input
        .and_then(|i| i.get("file_path").or_else(|| i.get("path")))
        .and_then(Value::as_str)
        .map(str::to_string);
    match event {
        "SessionStart" => vec![AgentEvent::SessionStarted {
            cli_session_id: str_field(payload, "session_id"),
            model: str_field(payload, "model"),
        }],
        "UserPromptSubmit" => vec![AgentEvent::TurnStarted],
        "PreToolUse" if !tool.is_empty() => vec![AgentEvent::ToolRequested {
            tool_use_id,
            tool,
            kind,
            command,
        }],
        "PostToolUse" | "PostToolUseFailure" if !tool.is_empty() => {
            let ok = event == "PostToolUse";
            let exit_code = payload
                .get("tool_response")
                .and_then(|r| r.get("exit_code").or_else(|| r.get("exitCode")))
                .and_then(Value::as_i64)
                .and_then(|c| i32::try_from(c).ok());
            let mut out = vec![AgentEvent::ToolFinished {
                tool_use_id,
                tool: tool.clone(),
                kind,
                ok,
                exit_code,
            }];
            if kind == ToolKind::Command
                && let Some(text) = payload
                    .get("tool_response")
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .filter(|s| !s.is_empty())
            {
                out.push(AgentEvent::ToolOutput {
                    tool_use_id: str_field(payload, "tool_use_id"),
                    tool: tool.clone(),
                    text,
                });
            }
            if ok && kind == ToolKind::Edit {
                out.push(AgentEvent::FileModified { tool, path });
            }
            out
        }
        "Stop" => vec![AgentEvent::TurnFinished {
            last_message: str_field(payload, "last_assistant_message"),
        }],
        "StopFailure" => {
            let category = str_field(payload, "error").unwrap_or_else(|| "unknown".into());
            let failure_type = classify_error_category(&category).unwrap_or(FailureType::Unknown);
            vec![AgentEvent::ProviderError(ProviderError {
                failure_type,
                raw_code: Some(category),
                message: str_field(payload, "error_details")
                    .map(|d| symphony_core::redact(&d).into_owned())
                    .unwrap_or_default(),
                retry_after_ms: None,
                resets_at: None,
                transient: false,
            })]
        }
        "SessionEnd" => vec![AgentEvent::AgentStopped {
            reason: str_field(payload, "reason"),
        }],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn names(v: &Value) -> Vec<&'static str> {
        parse_standard_hook(v)
            .iter()
            .map(AgentEvent::type_name)
            .collect()
    }

    #[test]
    fn maps_the_common_schema() {
        assert_eq!(
            names(&json!({"hook_event_name": "SessionStart", "session_id": "s"})),
            ["AgentStarted"]
        );
        assert_eq!(
            names(&json!({"hook_event_name": "UserPromptSubmit"})),
            ["TurnStarted"]
        );
        assert_eq!(
            names(
                &json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "npm test"}})
            ),
            ["CommandRequested"]
        );
        assert_eq!(
            names(&json!({"hook_event_name": "PreToolUse", "tool_name": "apply_patch"})),
            ["ToolRequested"]
        );
        assert_eq!(
            names(
                &json!({"hook_event_name": "PostToolUse", "tool_name": "Write", "tool_input": {"file_path": "a.js"}})
            ),
            ["ToolFinished", "FileModified"]
        );
        assert_eq!(
            names(&json!({"hook_event_name": "PostToolUseFailure", "tool_name": "Bash"})),
            ["CommandFinished"]
        );
        let output = parse_standard_hook(
            &json!({"hook_event_name":"PostToolUse","tool_name":"Bash","tool_use_id":"u1","tool_response":{"stdout":"hello","exit_code":0}}),
        );
        assert!(
            matches!(output.last(), Some(AgentEvent::ToolOutput { tool_use_id: Some(id), text, .. }) if id == "u1" && text.contains("hello"))
        );
        assert_eq!(
            names(&json!({"hook_event_name": "Stop", "last_assistant_message": "listo"})),
            ["TurnFinished"]
        );
        assert_eq!(
            names(&json!({"hook_event_name": "SessionEnd"})),
            ["AgentStopped"]
        );
        assert!(names(&json!({"hook_event_name": "Notification"})).is_empty());
        assert!(names(&json!({"sin": "evento"})).is_empty());
    }

    #[test]
    fn stop_failure_is_typed_and_redacted() {
        let ev = parse_standard_hook(&json!({
            "hook_event_name": "StopFailure",
            "error": "rate_limit",
            "error_details": "429 Authorization: Bearer sk-ant-api03-SECRETSECRETSECRET"
        }));
        let [AgentEvent::ProviderError(e)] = ev.as_slice() else {
            panic!("{ev:?}")
        };
        assert_eq!(e.failure_type, FailureType::TempRateLimit);
        assert!(!e.message.contains("SECRET"), "{}", e.message);
        assert_eq!(
            classify_error_category("authentication_failed"),
            Some(FailureType::Auth)
        );
        assert_eq!(classify_error_category("inventado"), None);
    }
}

#[cfg(test)]
mod redaction_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_commands_are_redacted_before_storage() {
        let ev = parse_standard_hook(&json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "export GITHUB_TOKEN=ghp_AbCdEfGhIjKlMnOpQrStUvWxYz0123 && npm publish"}
        }));
        let [
            AgentEvent::ToolRequested {
                command: Some(c), ..
            },
        ] = ev.as_slice()
        else {
            panic!("{ev:?}")
        };
        assert!(!c.contains("ghp_"), "{c}");
        assert!(c.contains("npm publish"));
    }
}
