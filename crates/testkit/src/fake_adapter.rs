//! `FakeAdapter`: implementa `ProviderAdapter` sobre `fake-agent` (P05.S1).
//! Sirve para probar el core de punta a punta sin gastar suscripciones.

use std::path::PathBuf;

use serde_json::Value;
use symphony_adapter_common::hooks::{classify_error_category, parse_standard_hook, tool_kind};
use symphony_adapter_common::{
    AdapterError, AgentEvent, AuthStatus, Detection, ModelInfo, ProcessSpec, ProviderAdapter,
    ProviderError, ResumeRequest, SpawnRequest,
};
use symphony_core::FailureType;

#[derive(Debug, Clone)]
pub struct FakeAdapter {
    /// Binario `fake-agent`.
    pub binary: PathBuf,
    /// Guion que ejecuta cada spawn.
    pub script: PathBuf,
}

impl FakeAdapter {
    pub fn new(binary: impl Into<PathBuf>, script: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            script: script.into(),
        }
    }
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

impl ProviderAdapter for FakeAdapter {
    fn provider_id(&self) -> &'static str {
        "fake"
    }

    fn display_name(&self) -> &'static str {
        "Fake"
    }

    fn detect(&self) -> Result<Detection, AdapterError> {
        if self.binary.is_file() {
            Ok(Detection {
                cli_path: self.binary.clone(),
                version: env!("CARGO_PKG_VERSION").into(),
            })
        } else {
            Err(AdapterError::NotInstalled(
                self.binary.display().to_string(),
            ))
        }
    }

    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Ok
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        ["fast", "smart"]
            .iter()
            .map(|m| ModelInfo {
                id: format!("fake/{m}"),
                cli_model_id: (*m).into(),
                display_name: format!("Fake {m}"),
            })
            .collect()
    }

    fn supports_hooks(&self) -> bool {
        true
    }

    fn spawn_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError> {
        let mut spec = ProcessSpec::new(&self.binary)
            .arg("run")
            .arg("--script")
            .arg(&self.script)
            .args(["--model", &req.model])
            .cwd(&req.worktree)
            .with_stdin();
        if let Some(id) = &req.session_id {
            spec = spec.args(["--session-id", id]);
        }
        if let Some(hook) = &req.hook {
            spec = spec.arg("--hook").arg(&hook.program);
            for a in &hook.args {
                spec = spec.args(["--hook-arg", a]);
            }
        }
        for (k, v) in &req.env {
            spec = spec.env(k, v);
        }
        Ok(spec)
    }

    fn resume_spec(&self, req: &ResumeRequest) -> Result<ProcessSpec, AdapterError> {
        let mut spawn = req.spawn.clone();
        spawn.session_id = Some(req.cli_session_id.clone());
        self.spawn_spec(&spawn)
    }

    fn encode_prompt(&self, prompt: &str) -> Vec<u8> {
        format!("{prompt}\n").into_bytes()
    }

    fn encode_user_message(&self, text: &str) -> Option<Vec<u8>> {
        Some(format!("{text}\n").into_bytes())
    }

    fn parse_stream_line(&self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        let tool = s(&v, "name").unwrap_or_default();
        match (
            v.get("type").and_then(Value::as_str),
            v.get("subtype").and_then(Value::as_str),
        ) {
            (Some("system"), Some("init")) => vec![AgentEvent::SessionStarted {
                cli_session_id: s(&v, "session_id"),
                model: s(&v, "model"),
            }],
            (Some("system"), Some("api_retry")) => {
                let category = s(&v, "error").unwrap_or_else(|| "unknown".into());
                vec![AgentEvent::ProviderError(ProviderError {
                    failure_type: classify_error_category(&category)
                        .unwrap_or(FailureType::Unknown),
                    raw_code: v.get("error_status").map(|c| c.to_string()),
                    message: String::new(),
                    retry_after_ms: v.get("retry_delay_ms").and_then(Value::as_u64),
                    resets_at: None,
                    transient: true,
                })]
            }
            (Some("assistant"), _) => s(&v, "text")
                .map(|text| vec![AgentEvent::AssistantText { text }])
                .unwrap_or_default(),
            (Some("user"), _) => s(&v, "text")
                .map(|text| vec![AgentEvent::UserMessage { text }])
                .unwrap_or_default(),
            (Some("tool_use"), _) if !tool.is_empty() => {
                let command = v
                    .get("input")
                    .and_then(|i| i.get("command"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                vec![AgentEvent::ToolRequested {
                    tool_use_id: s(&v, "id"),
                    kind: tool_kind(&tool),
                    tool,
                    command,
                }]
            }
            (Some("tool_result"), _) if !tool.is_empty() => {
                let exit_code = v
                    .get("output")
                    .and_then(|o| o.get("exit_code"))
                    .and_then(Value::as_i64)
                    .and_then(|c| i32::try_from(c).ok());
                let ok = v.get("ok").and_then(Value::as_bool).unwrap_or(false);
                vec![AgentEvent::ToolFinished {
                    tool_use_id: s(&v, "id"),
                    kind: tool_kind(&tool),
                    tool,
                    ok,
                    exit_code,
                }]
            }
            (Some("error"), _) => s(&v, "error")
                .and_then(|e| self.parse_error(&e))
                .map(|mut e| {
                    e.resets_at = v.get("resets_at").and_then(Value::as_i64);
                    vec![AgentEvent::ProviderError(e)]
                })
                .unwrap_or_default(),
            (Some("result"), _) => vec![AgentEvent::TurnFinished { last_message: None }],
            _ => Vec::new(),
        }
    }

    fn parse_hook(&self, payload: &Value) -> Vec<AgentEvent> {
        parse_standard_hook(payload)
    }

    fn parse_error(&self, text: &str) -> Option<ProviderError> {
        let (failure_type, code) = if text.contains("quota_exhausted") {
            (FailureType::DailyQuota, "quota_exhausted")
        } else if text.contains("authentication_failed") {
            (FailureType::Auth, "authentication_failed")
        } else if text.contains("rate_limit") {
            (FailureType::TempRateLimit, "rate_limit")
        } else {
            return None;
        };
        Some(ProviderError {
            failure_type,
            raw_code: Some(code.into()),
            message: symphony_core::redact(text).into_owned(),
            retry_after_ms: None,
            resets_at: None,
            transient: failure_type == FailureType::TempRateLimit,
        })
    }
}
