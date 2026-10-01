//! Adapter de GitHub Copilot CLI (`docs/research/cli-p11.md`, ADR-0008, ADR-0009).
//!
//! - Turno headless: `copilot --output-format json --no-ask-user`, con el prompt por **stdin**
//!   (sin `-p`: con stdin entubado Copilot corre no interactivo y lee el prompt hasta EOF).
//! - JSONL de eventos con `type`: `assistant.message` (texto y `toolRequests`),
//!   `tool.execution_start` / `tool.execution_complete` y `result`. No hay tokens de uso.
//! - El id de sesión solo aparece en el `result` final (`sessionId`); `--session-id <uuid>` permite
//!   fijarlo de antemano y `--resume=<id>` retoma.
//! - Permisos: en headless Copilot deniega solo lo que pide permiso. `--allow-tool=write` permite
//!   editar y deja el shell en solo lectura (como `acceptEdits` de Claude); `allow_all_tools`
//!   pasa `--allow-all-tools`.
//! - `--model` solo acepta lo que la cuenta tiene habilitado (en la cuenta de las pruebas, solo
//!   `auto`); el error sale por **stderr** (ADR-0009).
//! - Symphony no lee `~/.copilot`: el login es cosa del CLI.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use symphony_adapter_common::{
    AdapterError, AgentEvent, AuthStatus, Detection, ModelInfo, ProcessSpec, ProviderAdapter,
    ProviderError, ResumeRequest, SpawnRequest, ToolKind, find_on_path, has_token, json_str,
    looks_like_error,
};
use symphony_core::FailureType;

/// Deja que Copilot elija; es lo único que aceptó `--model` en la cuenta de las pruebas.
const AUTO_MODEL: &str = "auto";

#[derive(Debug, Clone, Default)]
pub struct CopilotAdapter {
    pub binary: Option<PathBuf>,
    /// `true`: `--allow-all-tools` (el agente puede ejecutar cualquier comando).
    pub allow_all_tools: bool,
}

fn tool_kind(name: &str) -> ToolKind {
    match name {
        "powershell" | "bash" | "shell" => ToolKind::Command,
        "create" | "edit" | "str_replace" | "apply_patch" | "write" | "str_replace_editor" => {
            ToolKind::Edit
        }
        _ => ToolKind::Other,
    }
}

/// La ruta editada: `path` del argumento o la primera cabecera de un parche (`*** Add File: x`).
fn edited_path(args: &Value) -> Option<String> {
    json_str(args, "path").or_else(|| {
        args.as_str()
            .or_else(|| args["input"].as_str())
            .and_then(|patch| {
                patch.lines().find_map(|l| {
                    ["Add File: ", "Update File: ", "Delete File: "]
                        .iter()
                        .find_map(|p| l.strip_prefix("*** ")?.strip_prefix(p))
                        .map(|p| p.trim().to_string())
                })
            })
    })
}

fn message_events(data: &Value) -> Vec<AgentEvent> {
    let text = json_str(data, "content").unwrap_or_default();
    if text.trim().is_empty() {
        Vec::new()
    } else {
        vec![AgentEvent::AssistantText { text }]
    }
}

fn tool_start_events(data: &Value) -> Vec<AgentEvent> {
    let Some(tool) = json_str(data, "toolName").filter(|t| !t.is_empty()) else {
        return Vec::new();
    };
    let kind = tool_kind(&tool);
    let args = &data["arguments"];
    let mut out = vec![AgentEvent::ToolRequested {
        tool_use_id: json_str(data, "toolCallId"),
        tool: tool.clone(),
        kind,
        command: if kind == ToolKind::Command {
            json_str(args, "command")
        } else {
            None
        },
    }];
    // El `complete` no trae la ruta: se anota al pedir la edición (un git status corrige el resto).
    if kind == ToolKind::Edit {
        out.push(AgentEvent::FileModified {
            tool,
            path: edited_path(args),
        });
    }
    out
}

fn tool_complete_events(data: &Value) -> Vec<AgentEvent> {
    let ok = data["success"].as_bool().unwrap_or(false);
    let text = data["result"]["content"].as_str().unwrap_or("");
    // «<shellId: 0 completed with exit code 0>»: solo los comandos de shell lo traen.
    let exit_code = text.split("exit code ").nth(1).and_then(|r| {
        r.chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect::<String>()
            .parse()
            .ok()
    });
    vec![AgentEvent::ToolFinished {
        tool_use_id: json_str(data, "toolCallId"),
        tool: "tool".into(),
        kind: if exit_code.is_some() {
            ToolKind::Command
        } else {
            ToolKind::Other
        },
        ok,
        exit_code,
    }]
}

impl CopilotAdapter {
    fn binary(&self) -> Result<PathBuf, AdapterError> {
        self.binary
            .clone()
            .or_else(|| find_on_path("copilot"))
            .ok_or_else(|| AdapterError::NotInstalled("copilot".into()))
    }

    fn base_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError> {
        let mut spec = ProcessSpec::new(self.binary()?)
            .args([
                "--output-format",
                "json",
                "--no-ask-user",
                "--no-auto-update",
            ])
            .args(["--model", &req.model])
            .cwd(&req.worktree)
            .with_stdin();
        spec = if self.allow_all_tools {
            spec.arg("--allow-all-tools")
        } else {
            spec.arg("--allow-tool=write")
        };
        for (k, v) in &req.env {
            spec = spec.env(k, v);
        }
        Ok(spec)
    }
}

impl ProviderAdapter for CopilotAdapter {
    fn provider_id(&self) -> &'static str {
        "github"
    }

    fn display_name(&self) -> &'static str {
        "Copilot"
    }

    fn cli_name(&self) -> &'static str {
        "copilot"
    }

    fn detect(&self) -> Result<Detection, AdapterError> {
        let cli_path = self.binary()?;
        let out = Command::new(&cli_path)
            .arg("--version")
            .stdin(Stdio::null())
            .output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        // "GitHub Copilot CLI 1.0.60.\nRun 'copilot update' to check for updates."
        let version = text
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().last())
            .map(|v| v.trim_end_matches('.').to_string())
            .unwrap_or_default();
        if !out.status.success() || version.is_empty() {
            return Err(AdapterError::Invalid(format!(
                "`copilot --version` respondió: {}",
                text.trim()
            )));
        }
        Ok(Detection { cli_path, version })
    }

    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Unknown
    }

    /// Solo `auto`: es lo único que `--model` aceptó en la cuenta de las pruebas (la ayuda de
    /// Copilot documenta 17 ids que dependen del plan de cada usuario).
    fn list_models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: format!("github/{AUTO_MODEL}"),
            cli_model_id: AUTO_MODEL.into(),
            display_name: "Copilot (automático)".into(),
        }]
    }

    /// Sin verificar (docs/research/cli-p11.md); el chat no los necesita.
    fn supports_hooks(&self) -> bool {
        false
    }

    fn hooks_can_hold(&self) -> Option<bool> {
        None
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
            .arg(format!("--resume={}", req.cli_session_id)))
    }

    fn attach_spec(
        &self,
        cli_session_id: &str,
        model: &str,
        worktree: &Path,
    ) -> Result<ProcessSpec, AdapterError> {
        Ok(ProcessSpec::new(self.binary()?)
            .arg(format!("--resume={cli_session_id}"))
            .args(["--model", model])
            .cwd(worktree))
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
            return Vec::new();
        };
        let data = &v["data"];
        match v.get("type").and_then(Value::as_str) {
            Some("assistant.message") => message_events(data),
            Some("tool.execution_start") => tool_start_events(data),
            Some("tool.execution_complete") => tool_complete_events(data),
            // El id de sesión solo llega aquí, al final del turno.
            Some("result") => vec![AgentEvent::SessionStarted {
                cli_session_id: json_str(&v, "sessionId").filter(|id| !id.is_empty()),
                model: None,
            }],
            // Forma esperada de un fallo del servicio; sin verificar en vivo.
            Some("session.error") | Some("error") => json_str(data, "message")
                .or_else(|| json_str(&v, "message"))
                .and_then(|m| self.parse_error(&m))
                .map(|e| vec![AgentEvent::ProviderError(e)])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Los errores de arranque (`Error: Model "x" from --model flag is not available.`).
    fn parse_stderr_line(&self, line: &str) -> Vec<AgentEvent> {
        if !looks_like_error(line) {
            return Vec::new();
        }
        self.parse_error(line)
            .map(|e| vec![AgentEvent::ProviderError(e)])
            .unwrap_or_default()
    }

    fn parse_hook(&self, _payload: &Value) -> Vec<AgentEvent> {
        Vec::new()
    }

    /// Solo «modelo no disponible» viene de Copilot real (P11.S1); cuota, límite y auth son
    /// textos genéricos sin verificar en vivo.
    fn parse_error(&self, text: &str) -> Option<ProviderError> {
        let lower = text.to_lowercase();
        let (failure_type, code, transient) =
            if lower.contains("model") && lower.contains("is not available") {
                (FailureType::ModelUnavailable, "model_unavailable", false)
            } else if lower.contains("quota") || lower.contains("premium request") {
                (FailureType::DailyQuota, "quota", false)
            } else if lower.contains("rate limit")
                || lower.contains("rate-limit")
                || has_token(&lower, "429")
                || lower.contains("too many requests")
            {
                (FailureType::TempRateLimit, "rate_limit", true)
            } else if has_token(&lower, "401")
                || lower.contains("unauthorized")
                || lower.contains("copilot login")
                || lower.contains("not logged in")
                || lower.contains("not authenticated")
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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/providers/copilot")
}
