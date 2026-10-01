//! Adapter de Kimi Code (`docs/research/cli-p11.md`, ADR-0008).
//!
//! - Turno headless: `kimi --print --output-format stream-json`, prompt por stdin hasta EOF.
//!   `--print` ya auto-aprueba las herramientas (Kimi no tiene sandbox en este modo).
//! - Un JSON por mensaje: `assistant` (texto y `tool_calls`) y `tool` (resultado). No hay
//!   deltas, ni uso de tokens, ni evento de fin de turno: el turno termina al salir el proceso.
//! - `-S <id>` crea la sesión con ese id o la retoma si existe; sirve para `spawn` y `resume`.
//! - El id de sesión solo sale por **stderr** (`To resume this session: kimi -r <id>`): ver
//!   [`session_from_stderr`].
//! - Symphony no lee `~/.kimi`: el login y la config de modelos son cosa del CLI.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use symphony_adapter_common::{
    AdapterError, AgentEvent, AuthStatus, Detection, ModelInfo, ProcessSpec, ProviderAdapter,
    ProviderError, ResumeRequest, SpawnRequest, ToolKind,
};
use symphony_core::FailureType;

/// Modelo que Kimi tiene configurado: no se pasa `-m` (los nombres válidos salen de su config).
const DEFAULT_MODEL: &str = "default";

#[derive(Debug, Clone, Default)]
pub struct KimiAdapter {
    pub binary: Option<PathBuf>,
}

fn find_kimi() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(format!("kimi{}", std::env::consts::EXE_SUFFIX)))
        .find(|p| p.is_file())
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

/// El contenido de un mensaje: un texto, o una lista de partes (`think`, `text`).
fn text_of(content: &Value) -> String {
    match content {
        Value::String(t) => t.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|p| p["type"] == "text")
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn tool_kind(name: &str) -> ToolKind {
    match name {
        "Shell" => ToolKind::Command,
        "WriteFile" | "StrReplaceFile" => ToolKind::Edit,
        _ => ToolKind::Other,
    }
}

/// `To resume this session: kimi -r <id>` → el id de la sesión.
pub fn session_from_stderr(line: &str) -> Option<String> {
    let id = line
        .trim()
        .strip_prefix("To resume this session: kimi -r ")?;
    let id = id.trim();
    (!id.is_empty() && !id.contains(char::is_whitespace)).then(|| id.to_string())
}

fn assistant_events(v: &Value) -> Vec<AgentEvent> {
    let mut out = Vec::new();
    let text = text_of(&v["content"]);
    if !text.trim().is_empty() {
        out.push(AgentEvent::AssistantText { text });
    }
    for call in v["tool_calls"].as_array().into_iter().flatten() {
        let f = &call["function"];
        let Some(tool) = s(f, "name").filter(|n| !n.is_empty()) else {
            continue;
        };
        let args: Value = f["arguments"]
            .as_str()
            .and_then(|a| serde_json::from_str(a).ok())
            .unwrap_or(Value::Null);
        let kind = tool_kind(&tool);
        out.push(AgentEvent::ToolRequested {
            tool_use_id: s(call, "id"),
            tool: tool.clone(),
            kind,
            command: if kind == ToolKind::Command {
                s(&args, "command")
            } else {
                None
            },
        });
        // El resultado (`role: tool`) no trae la ruta: se anota al pedir la edición.
        if kind == ToolKind::Edit {
            out.push(AgentEvent::FileModified {
                tool,
                path: s(&args, "path"),
            });
        }
    }
    out
}

fn tool_finished(v: &Value) -> Vec<AgentEvent> {
    let text = text_of(&v["content"]);
    let ok = !text.contains("<system>ERROR");
    // «Command executed successfully.» / «ERROR: Command failed with exit code: 3.»
    let is_command = text.contains("Command executed") || text.contains("Command failed");
    let exit_code = text.split("exit code:").nth(1).and_then(|r| {
        r.trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect::<String>()
            .parse()
            .ok()
    });
    vec![AgentEvent::ToolFinished {
        tool_use_id: s(v, "tool_call_id"),
        tool: "tool".into(),
        kind: if is_command {
            ToolKind::Command
        } else {
            ToolKind::Other
        },
        ok,
        exit_code: exit_code.or(if is_command && ok { Some(0) } else { None }),
    }]
}

impl KimiAdapter {
    fn binary(&self) -> Result<PathBuf, AdapterError> {
        self.binary
            .clone()
            .or_else(find_kimi)
            .ok_or_else(|| AdapterError::NotInstalled("kimi".into()))
    }

    fn base_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError> {
        let mut spec = ProcessSpec::new(self.binary()?)
            .args(["--print", "--output-format", "stream-json"])
            .cwd(&req.worktree)
            .with_stdin();
        if req.model != DEFAULT_MODEL {
            spec = spec.args(["-m", &req.model]);
        }
        for (k, v) in &req.env {
            spec = spec.env(k, v);
        }
        Ok(spec)
    }
}

impl ProviderAdapter for KimiAdapter {
    fn provider_id(&self) -> &'static str {
        "moonshot"
    }

    fn display_name(&self) -> &'static str {
        "Kimi"
    }

    fn cli_name(&self) -> &'static str {
        "kimi"
    }

    fn detect(&self) -> Result<Detection, AdapterError> {
        let cli_path = self.binary()?;
        let out = Command::new(&cli_path)
            .arg("--version")
            .stdin(Stdio::null())
            .output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        // "kimi, version 1.44.0"
        let version = text.split_whitespace().last().unwrap_or("").to_string();
        if !out.status.success() || version.is_empty() {
            return Err(AdapterError::Invalid(format!(
                "`kimi --version` respondió: {}",
                text.trim()
            )));
        }
        Ok(Detection { cli_path, version })
    }

    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Unknown
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: format!("moonshot/{DEFAULT_MODEL}"),
            cli_model_id: DEFAULT_MODEL.into(),
            display_name: "Kimi (modelo configurado)".into(),
        }]
    }

    /// Kimi no mostró hooks en `--help`: sin verificar (docs/research/cli-p11.md).
    fn supports_hooks(&self) -> bool {
        false
    }

    fn hooks_can_hold(&self) -> Option<bool> {
        None
    }

    fn spawn_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError> {
        let spec = self.base_spec(req)?;
        Ok(match &req.session_id {
            Some(id) => spec.args(["-S", id]),
            None => spec,
        })
    }

    fn resume_spec(&self, req: &ResumeRequest) -> Result<ProcessSpec, AdapterError> {
        Ok(self
            .base_spec(&req.spawn)?
            .args(["-S", &req.cli_session_id]))
    }

    fn attach_spec(
        &self,
        cli_session_id: &str,
        model: &str,
        worktree: &Path,
    ) -> Result<ProcessSpec, AdapterError> {
        let mut spec = ProcessSpec::new(self.binary()?).args(["-S", cli_session_id]);
        if model != DEFAULT_MODEL {
            spec = spec.args(["-m", model]);
        }
        Ok(spec.cwd(worktree))
    }

    fn encode_prompt(&self, prompt: &str) -> Vec<u8> {
        prompt.as_bytes().to_vec()
    }

    fn close_stdin_after_prompt(&self) -> bool {
        true
    }

    /// Como Codex: los mensajes van en el próximo turno, con `resume`.
    fn encode_user_message(&self, _text: &str) -> Option<Vec<u8>> {
        None
    }

    fn parse_stream_line(&self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            // Los errores de configuración salen como texto plano por stdout (`LLM not set`).
            return self
                .parse_error(line)
                .map(|e| vec![AgentEvent::ProviderError(e)])
                .unwrap_or_default();
        };
        match v.get("role").and_then(Value::as_str) {
            Some("assistant") => assistant_events(&v),
            Some("tool") => tool_finished(&v),
            _ => Vec::new(),
        }
    }

    fn parse_hook(&self, _payload: &Value) -> Vec<AgentEvent> {
        Vec::new()
    }

    /// Solo `LLM not set` viene de Kimi real (P11.S1); el resto son textos genéricos de
    /// cuota, límite y auth, sin verificar en vivo.
    fn parse_error(&self, text: &str) -> Option<ProviderError> {
        let lower = text.to_lowercase();
        let (failure_type, code, transient) = if lower.contains("llm not set") {
            (FailureType::ModelUnavailable, "llm_not_set", false)
        } else if lower.contains("insufficient balance")
            || lower.contains("quota")
            || lower.contains("usage limit")
        {
            (FailureType::DailyQuota, "quota", false)
        } else if lower.contains("rate limit")
            || lower.contains("429")
            || lower.contains("too many requests")
        {
            (FailureType::TempRateLimit, "rate_limit", true)
        } else if lower.contains("401")
            || lower.contains("unauthorized")
            || lower.contains("kimi login")
            || lower.contains("not logged in")
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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/providers/kimi")
}
