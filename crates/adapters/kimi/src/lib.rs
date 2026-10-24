//! Adapter de Kimi Code (`docs/research/cli-p11.md`, ADR-0008).
//!
//! - Turno headless: `kimi --print --output-format stream-json`, prompt por stdin hasta EOF.
//!   `--print` ya auto-aprueba las herramientas (Kimi no tiene sandbox en este modo).
//! - Un JSON por mensaje: `assistant` (texto y `tool_calls`) y `tool` (resultado). No hay
//!   deltas, ni uso de tokens, ni evento de fin de turno: el turno termina al salir el proceso.
//! - `-S <id>` crea la sesión con ese id o la retoma si existe; sirve para `spawn` y `resume`.
//! - El id de sesión solo sale por **stderr** (`To resume this session: kimi -r <id>`): ver
//!   [`session_from_stderr`] y `parse_stderr_line` (ADR-0009).
//! - Symphony no lee `~/.kimi`: el login y la config de modelos son cosa del CLI.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use symphony_adapter_common::{
    AdapterError, AgentEvent, AuthStatus, Detection, ModelInfo, ProcessSpec, ProviderAdapter,
    ProviderError, ResumeRequest, SpawnRequest, ToolKind, find_on_path, has_token, json_str,
    looks_like_error, parse_retry_after_ms,
};
use symphony_core::FailureType;

/// Modelo que Kimi tiene configurado: no se pasa `-m` (los nombres válidos salen de su config).
const DEFAULT_MODEL: &str = "default";

#[derive(Debug, Clone, Default)]
pub struct KimiAdapter {
    pub binary: Option<PathBuf>,
}

/// Agente de Kimi sin herramientas propias: hereda el prompt de sistema del predeterminado.
const HARNESS_AGENT: &str = "version: 1\nagent:\n  extend: default\n  tools: []\n  subagents: {}\n";

/// El archivo de [`HARNESS_AGENT`] (Kimi solo acepta un archivo). Se crea una vez por versión del
/// texto, en el directorio temporal; `None` si no se pudo escribir.
fn harness_agent_file() -> Option<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "symphony-kimi-harness-{:08x}.yaml",
        HARNESS_AGENT
            .bytes()
            .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)))
    ));
    // Nunca se confía en un archivo que ya estaba (otro usuario de la máquina pudo crearlo con
    // ese nombre): si su contenido no es el esperado se escribe uno propio de este proceso.
    let is_ours = |p: &Path| std::fs::read_to_string(p).is_ok_and(|t| t == HARNESS_AGENT);
    if is_ours(&path) {
        return Some(path);
    }
    let own = path.with_extension(format!("{}.yaml", std::process::id()));
    let _ = std::fs::remove_file(&own);
    std::fs::write(&own, HARNESS_AGENT).ok()?;
    is_ours(&own).then_some(own)
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
        let Some(tool) = json_str(f, "name").filter(|n| !n.is_empty()) else {
            continue;
        };
        let args: Value = f["arguments"]
            .as_str()
            .and_then(|a| serde_json::from_str(a).ok())
            .unwrap_or(Value::Null);
        let kind = tool_kind(&tool);
        out.push(AgentEvent::ToolRequested {
            tool_use_id: json_str(call, "id"),
            tool: tool.clone(),
            kind,
            command: if kind == ToolKind::Command {
                json_str(&args, "command")
            } else {
                None
            },
        });
        // El resultado (`role: tool`) no trae la ruta: se anota al pedir la edición.
        if kind == ToolKind::Edit {
            out.push(AgentEvent::FileModified {
                tool,
                path: json_str(&args, "path"),
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
        tool_use_id: json_str(v, "tool_call_id"),
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
            .or_else(|| find_on_path("kimi"))
            .ok_or_else(|| AdapterError::NotInstalled("kimi".into()))
    }

    /// Una línea de texto plano → error clasificado, solo si parece un error (el resto es ruido).
    fn plain_error(&self, line: &str) -> Vec<AgentEvent> {
        if !(looks_like_error(line) || line.to_lowercase().contains("llm not set")) {
            return Vec::new();
        }
        self.parse_error(line)
            .map(|e| vec![AgentEvent::ProviderError(e)])
            .unwrap_or_default()
    }

    fn base_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError> {
        // Kimi es Python: en Windows, con la salida en un pipe, escribe en cp1252 y se cae con el
        // primer carácter que no cabe (p. ej. «⛔» leído de un archivo). Dogfooding 2026-10-24.
        let mut spec = ProcessSpec::new(self.binary()?)
            .args(["--print", "--output-format", "stream-json"])
            .cwd(&req.worktree)
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .with_stdin();
        if req.model != DEFAULT_MODEL {
            spec = spec.args(["-m", &req.model]);
        }
        // Harness de Symphony (ADR-0019): un agente sin herramientas propias y el broker como
        // única fuente (verificado en vivo con kimi 1.44).
        if symphony_adapter_common::harness_on(req)
            && let Some((cmd, env)) = symphony_adapter_common::broker(req)
            && let Some(agent) = harness_agent_file()
        {
            let servers = json!({ "mcpServers": { symphony_adapter_common::MCP_SERVER_NAME: {
                "command": cmd.program.to_string_lossy(),
                "args": cmd.args,
                "env": env.into_iter().collect::<std::collections::BTreeMap<_, _>>(),
            } } });
            spec = spec
                .arg("--agent-file")
                .arg(agent)
                .arg("--mcp-config")
                .arg(servers.to_string());
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
        // `default` es el modelo de `~/.kimi/config.toml`; el resto son las entradas `[models.*]`
        // que trae esa config (kimi 1.44, 2026-10-23) y que `-m` acepta por su clave.
        // ponytail: lista fija; si Kimi cambia sus modelos, leer ese TOML en vez de copiarlo.
        let mut out = vec![ModelInfo {
            id: format!("moonshot/{DEFAULT_MODEL}"),
            cli_model_id: DEFAULT_MODEL.into(),
            display_name: "Kimi (modelo configurado)".into(),
        }];
        out.extend(
            [
                ("kimi-for-coding", "Kimi for Coding"),
                (
                    "kimi-for-coding-highspeed",
                    "Kimi for Coding (alta velocidad)",
                ),
                ("k3", "Kimi K3"),
                ("k3-256k", "Kimi K3 (256k)"),
                ("k3.1-pro", "Kimi K3.1 Pro"),
                ("k3.1-pro-highspeed", "Kimi K3.1 Pro (alta velocidad)"),
            ]
            .iter()
            .map(|(slug, name)| ModelInfo {
                id: format!("moonshot/{slug}"),
                cli_model_id: format!("kimi-code/{slug}"),
                display_name: (*name).into(),
            }),
        );
        out
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
            return self.plain_error(line);
        };
        match v.get("role").and_then(Value::as_str) {
            Some("assistant") => assistant_events(&v),
            Some("tool") => tool_finished(&v),
            _ => Vec::new(),
        }
    }

    /// El id de sesión (`To resume this session: kimi -r <id>`) y los errores de stderr.
    fn parse_stderr_line(&self, line: &str) -> Vec<AgentEvent> {
        if let Some(id) = session_from_stderr(line) {
            return vec![AgentEvent::SessionStarted {
                cli_session_id: Some(id),
                model: None,
            }];
        }
        self.plain_error(line)
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
        } else if lower.contains("subscription does not") || lower.contains("higher-tier") {
            // 403 de un modelo fuera del plan (`kimi-for-coding-highspeed`); llega partido en varias
            // líneas por stdout y la primera ya lo dice (dogfooding 2026-10-24).
            (FailureType::ModelUnavailable, "plan_without_model", false)
        } else if lower.contains("insufficient balance")
            || lower.contains("quota")
            || lower.contains("usage limit")
        {
            (FailureType::DailyQuota, "quota", false)
        } else if lower.contains("rate limit")
            || has_token(&lower, "429")
            || lower.contains("too many requests")
        {
            (FailureType::TempRateLimit, "rate_limit", true)
        } else if has_token(&lower, "401")
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
            retry_after_ms: parse_retry_after_ms(text),
            resets_at: None,
            transient,
        })
    }
}

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/providers/kimi")
}
