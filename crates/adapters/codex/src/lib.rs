//! Adapter de Codex (`docs/research/cli-codex.md`, ADR-0003, ADR-0005).
//!
//! - Lanza `codex exec --json … -`: el prompt entra por stdin hasta EOF (nunca como
//!   argumento: los `.cmd` rompen las comillas, P01 Test B).
//! - Hooks por invocación con `-c hooks.<Evento>=[…]` + `--dangerously-bypass-hook-trust`:
//!   Codex no carga `<worktree>/.codex/hooks.json` (Test B). En Windows el comando del
//!   hook corre en **PowerShell**: `& 'ruta' 'arg'`.
//! - Mensajes a media tarea: solo entre turnos, con `exec resume` (ADR-0005).
//! - La cuota KNOWN está en el rollout de disco, no en el stream (Test B): ver [`quota_from_rollout`].
//! - Symphony no lee `auth.json`: el login es cosa del CLI.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use symphony_adapter_common::hooks::parse_standard_hook;
use symphony_adapter_common::{
    AdapterError, AgentEvent, AuthStatus, Detection, HookCommand, ModelInfo, ProcessSpec,
    ProviderAdapter, ProviderError, QuotaSnapshot, ResumeRequest, SpawnRequest,
};
use symphony_core::FailureType;

/// Eventos de hook de Codex que Symphony escucha.
const HOOK_EVENTS: [&str; 6] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "SessionEnd",
];
const HOOK_TIMEOUT_SECS: u32 = 30;

#[derive(Debug, Clone)]
pub struct CodexAdapter {
    pub binary: Option<PathBuf>,
    /// `read-only` · `workspace-write` · `danger-full-access`. Lo decide la config del usuario.
    pub sandbox: String,
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self {
            binary: None,
            sandbox: "workspace-write".into(),
        }
    }
}

/// El `codex.exe` nativo del paquete npm (evita el shim `.cmd`) o lo que haya en el PATH.
fn find_codex() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if cfg!(windows) && dir.join("codex.cmd").is_file() {
            let native = dir
                .join("node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe");
            if native.is_file() {
                return Some(native);
            }
        }
        let exe = dir.join(format!("codex{}", std::env::consts::EXE_SUFFIX));
        if exe.is_file() {
            return Some(exe);
        }
    }
    None
}

/// Comando del hook para el shell en que Codex lo corre: PowerShell en Windows, sh en Unix.
pub fn hook_command_line(hook: &HookCommand) -> String {
    let parts = std::iter::once(hook.program.to_string_lossy().into_owned())
        .chain(hook.args.iter().cloned());
    if cfg!(windows) {
        // PowerShell: comillas simples literales ('' escapa una comilla) y `&` para invocar.
        let quoted: Vec<String> = parts
            .map(|p| format!("'{}'", p.replace('\'', "''")))
            .collect();
        format!("& {}", quoted.join(" "))
    } else {
        let quoted: Vec<String> = parts
            .map(|p| format!("'{}'", p.replace('\'', "'\\''")))
            .collect();
        quoted.join(" ")
    }
}

/// Un override `-c` por evento, en TOML inline.
pub fn hook_overrides(hook: &HookCommand) -> Vec<String> {
    let cmd = hook_command_line(hook);
    // Cadena TOML básica: escapar `\` y `"`.
    let toml_str = format!("\"{}\"", cmd.replace('\\', "\\\\").replace('"', "\\\""));
    HOOK_EVENTS
        .iter()
        .map(|e| format!("hooks.{e}=[{{hooks=[{{type=\"command\",command={toml_str},timeout={HOOK_TIMEOUT_SECS}}}]}}]"))
        .collect()
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

impl CodexAdapter {
    fn binary(&self) -> Result<PathBuf, AdapterError> {
        self.binary
            .clone()
            .or_else(find_codex)
            .ok_or_else(|| AdapterError::NotInstalled("codex".into()))
    }

    /// Flags comunes a `exec` y `exec resume`.
    fn common_flags(&self, spec: ProcessSpec, req: &SpawnRequest) -> ProcessSpec {
        let mut spec = spec
            .args(["--json", "-m", &req.model])
            .cwd(&req.worktree)
            .with_stdin();
        if let Some(hook) = &req.hook {
            spec = spec.arg("--dangerously-bypass-hook-trust");
            for o in hook_overrides(hook) {
                spec = spec.arg("-c").arg(o);
            }
        }
        for (k, v) in &req.env {
            spec = spec.env(k, v);
        }
        spec
    }
}

impl ProviderAdapter for CodexAdapter {
    fn provider_id(&self) -> &'static str {
        "openai"
    }

    fn display_name(&self) -> &'static str {
        "Codex"
    }

    fn detect(&self) -> Result<Detection, AdapterError> {
        let cli_path = self.binary()?;
        let out = Command::new(&cli_path)
            .arg("--version")
            .stdin(Stdio::null())
            .output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        // "codex-cli 0.154.0"
        let version = text.split_whitespace().nth(1).unwrap_or("").to_string();
        if !out.status.success() || version.is_empty() {
            return Err(AdapterError::Invalid(format!(
                "`codex --version` respondió: {}",
                text.trim()
            )));
        }
        Ok(Detection { cli_path, version })
    }

    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Unknown
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        // Modelos que Codex ofrece a una cuenta ChatGPT (P01 S2). `-m` acepta cualquier slug.
        [
            ("gpt-5.6-sol", "GPT-5.6 Sol"),
            ("gpt-5.6-terra", "GPT-5.6 Terra"),
            ("gpt-5.6-luna", "GPT-5.6 Luna"),
            ("gpt-5.5", "GPT-5.5"),
        ]
        .iter()
        .map(|(slug, name)| ModelInfo {
            id: format!("openai/{slug}"),
            cli_model_id: (*slug).into(),
            display_name: (*name).into(),
        })
        .collect()
    }

    fn supports_hooks(&self) -> bool {
        true
    }

    fn spawn_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError> {
        let spec = ProcessSpec::new(self.binary()?)
            .arg("exec")
            .args(["-s", &self.sandbox]);
        Ok(self.common_flags(spec, req).arg("-"))
    }

    fn resume_spec(&self, req: &ResumeRequest) -> Result<ProcessSpec, AdapterError> {
        let spec = ProcessSpec::new(self.binary()?)
            .args(["exec", "resume"])
            .arg("-c")
            .arg(format!("sandbox_mode=\"{}\"", self.sandbox));
        Ok(self
            .common_flags(spec, &req.spawn)
            .arg(&req.cli_session_id)
            .arg("-"))
    }

    fn encode_prompt(&self, prompt: &str) -> Vec<u8> {
        prompt.as_bytes().to_vec()
    }

    fn close_stdin_after_prompt(&self) -> bool {
        true
    }

    /// Codex no consume mensajes a media tarea en `exec` (ni `codex queue`, P01): van en el próximo turno.
    fn encode_user_message(&self, _text: &str) -> Option<Vec<u8>> {
        None
    }

    fn parse_stream_line(&self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        let item = &v["item"];
        match v.get("type").and_then(Value::as_str) {
            Some("thread.started") => vec![AgentEvent::SessionStarted {
                cli_session_id: s(&v, "thread_id"),
                model: None,
            }],
            Some("item.completed") if item["type"] == "agent_message" => s(item, "text")
                .filter(|t| !t.trim().is_empty())
                .map(|text| vec![AgentEvent::AssistantText { text }])
                .unwrap_or_default(),
            // `item.completed` de tipo error: avisos (p. ej. el de bypass-hook-trust), no fallas.
            Some("error") | Some("turn.failed") => {
                let msg = s(&v, "message")
                    .or_else(|| v["error"]["message"].as_str().map(str::to_string))
                    .unwrap_or_default();
                self.parse_error(&msg)
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
        let (failure_type, code, transient) =
            if lower.contains("usage limit") || lower.contains("hit your limit") {
                (FailureType::DailyQuota, "usage_limit", false)
            } else if lower.contains("rate limit")
                || lower.contains("429")
                || lower.contains("too many requests")
            {
                (FailureType::TempRateLimit, "rate_limit", true)
            } else if lower.contains("401")
                || lower.contains("unauthorized")
                || lower.contains("codex login")
                || lower.contains("not logged in")
            {
                (FailureType::Auth, "unauthorized", false)
            } else if lower.contains("reconnecting")
                || lower.contains("stream disconnected")
                || lower.contains("waiting for network")
                || lower.contains("error sending request")
            {
                // P01 Test D: Codex reintenta solo y espera la red sin fallar.
                (FailureType::Network, "network", true)
            } else if lower.contains("model")
                && (lower.contains("not supported")
                    || lower.contains("does not exist")
                    || lower.contains("not found"))
            {
                (FailureType::ModelUnavailable, "model_unavailable", false)
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

/// Cuota KNOWN desde el rollout de disco (`transcript_path` del hook): el último
/// `event_msg`/`token_count` con `rate_limits` (P01 S2: `primary` 5 h, `secondary` 7 días).
pub fn quota_from_rollout(rollout: &Path) -> Vec<QuotaSnapshot> {
    let Ok(text) = std::fs::read_to_string(rollout) else {
        return Vec::new();
    };
    let Some(limits) = text
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find_map(|v| {
            v["payload"]["rate_limits"]
                .as_object()
                .cloned()
                .or_else(|| v["rate_limits"].as_object().cloned())
        })
    else {
        return Vec::new();
    };
    ["primary", "secondary"]
        .iter()
        .filter_map(|k| limits.get(*k))
        .filter_map(|w| {
            let minutes = w.get("window_minutes")?.as_u64()?;
            let window = match minutes {
                300 => "five_hour".to_string(),
                10080 => "seven_day".to_string(),
                m => format!("{m}m"),
            };
            Some(QuotaSnapshot {
                window,
                used_fraction: w.get("used_percent")?.as_f64()? / 100.0,
                resets_at: w.get("resets_at").and_then(Value::as_i64),
            })
        })
        .collect()
}

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/providers/codex")
}
