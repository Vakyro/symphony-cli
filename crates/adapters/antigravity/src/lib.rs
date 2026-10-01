//! Adapter de Antigravity, el CLI `agy` de Google (`docs/research/cli-p11.md`, ADR-0008).
//!
//! - Turno headless por stdin: `agy --print= --input-format stream-json --output-format
//!   stream-json`, con un mensaje `{"event":"user","message":{"content":…}}` por línea. `--print`
//!   exige el prompt como valor, y el contrato prohíbe pasarlo como argumento.
//! - Eventos NDJSON: `init` (trae `conversation_id`), `step_update` (herramientas, uso por paso) y
//!   `result` (respuesta completa y estado). El texto se toma del `result`: los `text_delta` llegan
//!   troceados por paso y un parser sin estado no puede juntarlos.
//! - Permisos: en headless Antigravity **deniega solo** lo que pide permiso. Por defecto,
//!   `--mode accept-edits` (ediciones sí, shell no); `skip_permissions` pasa
//!   `--dangerously-skip-permissions` (todo, sin sandbox: `--sandbox` se cuelga en Windows).
//! - Resume: `--conversation <id>`; el id solo sale en `init` (no se puede fijar).
//! - Symphony no lee `~/.gemini`: el login es cosa del CLI.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use symphony_adapter_common::{
    AdapterError, AgentEvent, AuthStatus, Detection, ModelInfo, ProcessSpec, ProviderAdapter,
    ProviderError, ResumeRequest, SpawnRequest, ToolKind,
};
use symphony_core::FailureType;

#[derive(Debug, Clone, Default)]
pub struct AntigravityAdapter {
    pub binary: Option<PathBuf>,
    /// `true`: `--dangerously-skip-permissions` (el agente puede ejecutar cualquier comando).
    pub skip_permissions: bool,
}

fn find_agy() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(format!("agy{}", std::env::consts::EXE_SUFFIX)))
        .find(|p| p.is_file())
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn tool_kind(name: &str) -> ToolKind {
    match name {
        "run_command" => ToolKind::Command,
        "write_to_file"
        | "replace_file_content"
        | "multi_replace_file_content"
        | "sed_file"
        | "notebook_edit" => ToolKind::Edit,
        _ => ToolKind::Other,
    }
}

/// Identificador estable de un paso: `<conversación>:<índice>`.
fn step_id(step: &Value) -> Option<String> {
    Some(format!(
        "{}:{}",
        step.get("conversation_id").and_then(Value::as_str)?,
        step.get("step_index").and_then(Value::as_u64)?
    ))
}

fn param(step: &Value, keys: &[&str]) -> Option<String> {
    let p = &step["tool_info"]["parameters"];
    keys.iter().find_map(|k| s(p, k))
}

fn step_events(step: &Value) -> Vec<AgentEvent> {
    let state = step.get("state").and_then(Value::as_str).unwrap_or("");
    match step.get("step_type").and_then(Value::as_str) {
        Some("tool") => {
            let Some(tool) = s(step, "tool_name").filter(|t| !t.is_empty()) else {
                return Vec::new();
            };
            let kind = tool_kind(&tool);
            let tool_use_id = step_id(step);
            match state {
                "ACTIVE" => vec![AgentEvent::ToolRequested {
                    tool_use_id,
                    tool,
                    kind,
                    command: if kind == ToolKind::Command {
                        param(step, &["CommandLine"])
                    } else {
                        None
                    },
                }],
                "DONE" | "ERROR" => {
                    let ok = state == "DONE";
                    let mut out = vec![AgentEvent::ToolFinished {
                        tool_use_id,
                        tool: tool.clone(),
                        kind,
                        ok,
                        exit_code: None,
                    }];
                    if ok && kind == ToolKind::Edit {
                        out.push(AgentEvent::FileModified {
                            tool,
                            path: param(step, &["TargetFile", "AbsolutePath", "FilePath", "path"]),
                        });
                    }
                    out
                }
                _ => Vec::new(),
            }
        }
        // El último paso de respuesta deja el tamaño real del contexto (el `result` suma todos).
        Some("agent_response") if state == "DONE" => {
            let u = &step["usage"];
            let tokens = u["input_tokens"].as_u64().unwrap_or(0)
                + u["cache_read_tokens"].as_u64().unwrap_or(0);
            if tokens == 0 {
                Vec::new()
            } else {
                vec![AgentEvent::TurnUsage {
                    context_tokens: tokens,
                }]
            }
        }
        _ => Vec::new(),
    }
}

fn result_events(result: &Value, adapter: &AntigravityAdapter) -> Vec<AgentEvent> {
    if result.get("status").and_then(Value::as_str) == Some("ERROR") {
        return s(result, "error")
            .and_then(|e| adapter.parse_error(&e))
            .map(|e| vec![AgentEvent::ProviderError(e)])
            .unwrap_or_default();
    }
    let text = s(result, "response").unwrap_or_default();
    if !text.trim().is_empty() {
        return vec![AgentEvent::AssistantText {
            text: text.trim_end().to_string(),
        }];
    }
    // `SUCCESS` sin respuesta y con acciones denegadas: el agente no pudo hacer lo pedido.
    let denied: Vec<String> = result["denied_actions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| s(d, "action"))
        .collect();
    if denied.is_empty() {
        return Vec::new();
    }
    vec![AgentEvent::AssistantText {
        text: format!(
            "Antigravity no pudo continuar: denegó acciones sin permiso en modo headless ({}).",
            denied.join(", ")
        ),
    }]
}

impl AntigravityAdapter {
    fn binary(&self) -> Result<PathBuf, AdapterError> {
        self.binary
            .clone()
            .or_else(find_agy)
            .ok_or_else(|| AdapterError::NotInstalled("agy".into()))
    }

    fn base_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError> {
        let mut spec = ProcessSpec::new(self.binary()?)
            .args(["--print=", "--input-format", "stream-json"])
            .args(["--output-format", "stream-json"])
            .args(["--model", &req.model])
            .cwd(&req.worktree)
            .with_stdin();
        spec = if self.skip_permissions {
            spec.arg("--dangerously-skip-permissions")
        } else {
            spec.args(["--mode", "accept-edits"])
        };
        for (k, v) in &req.env {
            spec = spec.env(k, v);
        }
        Ok(spec)
    }
}

impl ProviderAdapter for AntigravityAdapter {
    fn provider_id(&self) -> &'static str {
        "google"
    }

    fn display_name(&self) -> &'static str {
        "Antigravity"
    }

    fn cli_name(&self) -> &'static str {
        "agy"
    }

    fn detect(&self) -> Result<Detection, AdapterError> {
        let cli_path = self.binary()?;
        let out = Command::new(&cli_path)
            .arg("--version")
            .stdin(Stdio::null())
            .output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        // "1.2.14"
        let version = text.split_whitespace().last().unwrap_or("").to_string();
        if !out.status.success() || version.is_empty() {
            return Err(AdapterError::Invalid(format!(
                "`agy --version` respondió: {}",
                text.trim()
            )));
        }
        Ok(Detection { cli_path, version })
    }

    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Unknown
    }

    /// Los de Google que lista `agy models` (P11.S1). Los de Anthropic y OpenAI que también
    /// ofrece quedan fuera hasta revisar los ToS (docs/research/cli-p11.md).
    fn list_models(&self) -> Vec<ModelInfo> {
        [
            ("gemini-3.8-flash-high", "Gemini 3.8 Flash (High)"),
            ("gemini-3.8-flash-medium", "Gemini 3.8 Flash (Medium)"),
            ("gemini-3.8-flash-low", "Gemini 3.8 Flash (Low)"),
            ("gemini-3.1-pro-high", "Gemini 3.1 Pro (High)"),
            ("gemini-3.1-pro-low", "Gemini 3.1 Pro (Low)"),
        ]
        .iter()
        .map(|(slug, name)| ModelInfo {
            id: format!("google/{slug}"),
            cli_model_id: (*slug).into(),
            display_name: (*name).into(),
        })
        .collect()
    }

    /// Sin verificar (docs/research/cli-p11.md); el chat no los necesita.
    fn supports_hooks(&self) -> bool {
        false
    }

    fn hooks_can_hold(&self) -> Option<bool> {
        None
    }

    fn spawn_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError> {
        self.base_spec(req)
    }

    fn resume_spec(&self, req: &ResumeRequest) -> Result<ProcessSpec, AdapterError> {
        Ok(self
            .base_spec(&req.spawn)?
            .args(["--conversation", &req.cli_session_id]))
    }

    fn attach_spec(
        &self,
        cli_session_id: &str,
        model: &str,
        worktree: &Path,
    ) -> Result<ProcessSpec, AdapterError> {
        Ok(ProcessSpec::new(self.binary()?)
            .args(["--conversation", cli_session_id, "--model", model])
            .cwd(worktree))
    }

    fn encode_prompt(&self, prompt: &str) -> Vec<u8> {
        let mut line = json!({"event": "user", "message": {"content": prompt}}).to_string();
        line.push('\n');
        line.into_bytes()
    }

    /// Cada línea de stdin corre un turno; al cerrar stdin el proceso termina.
    fn close_stdin_after_prompt(&self) -> bool {
        true
    }

    /// Como Codex: los mensajes van en el próximo turno, con `resume`.
    fn encode_user_message(&self, _text: &str) -> Option<Vec<u8>> {
        None
    }

    fn parse_stream_line(&self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        match v.get("event").and_then(Value::as_str) {
            Some("init") => vec![AgentEvent::SessionStarted {
                cli_session_id: s(&v, "conversation_id").filter(|id| !id.is_empty()),
                model: None,
            }],
            Some("step_update") => step_events(&v["step_update"]),
            Some("result") => result_events(&v["result"], self),
            _ => Vec::new(),
        }
    }

    fn parse_hook(&self, _payload: &Value) -> Vec<AgentEvent> {
        Vec::new()
    }

    /// Solo «modelo no reconocido» viene de `agy` real (P11.S1); cuota, límite y auth son
    /// textos genéricos sin verificar en vivo.
    fn parse_error(&self, text: &str) -> Option<ProviderError> {
        let lower = text.to_lowercase();
        let (failure_type, code, transient) = if lower.contains("invalid model selection")
            || lower.contains("not recognized as a known model")
        {
            (FailureType::ModelUnavailable, "invalid_model", false)
        } else if lower.contains("resource_exhausted") || lower.contains("quota") {
            (FailureType::DailyQuota, "quota", false)
        } else if lower.contains("rate limit")
            || lower.contains("429")
            || lower.contains("too many requests")
        {
            (FailureType::TempRateLimit, "rate_limit", true)
        } else if lower.contains("401")
            || lower.contains("unauthenticated")
            || lower.contains("unauthorized")
            || lower.contains("not logged in")
            || lower.contains("sign in")
        {
            (FailureType::Auth, "unauthorized", false)
        } else {
            return None;
        };
        Some(ProviderError {
            failure_type,
            raw_code: Some(code.into()),
            message: symphony_core::redact(text).chars().take(500).collect(),
            retry_after_ms: None,
            resets_at: None,
            transient,
        })
    }
}

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/providers/antigravity")
}
