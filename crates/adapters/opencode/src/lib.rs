//! Adapter de OpenCode (`docs/research/cli-opencode.md`, ADR-0012).
//!
//! - Turno headless: `opencode run --format json [-m proveedor/modelo]`, prompt por stdin hasta
//!   EOF. En `run` las herramientas se ejecutan sin pedir permiso y no hay sandbox (como Kimi).
//! - Un JSON por evento con `sessionID` en todos: `step_start`, `text`, `tool_use` (ya terminada,
//!   con `state.status` `completed` o `error`) y `step_finish` (con tokens). Un turno son varios
//!   pasos; termina el que cierra con `reason: "stop"` y el proceso sale solo.
//! - `-s <id>` continúa la sesión: OpenCode guarda el historial, así que el contexto persiste
//!   entre turnos sin que Symphony reenvíe nada. OpenCode asigna él mismo el id (`ses_…`): no se
//!   puede fijar uno, y se lee del primer evento.
//! - Modelos: lo que lista `opencode models` (`proveedor/modelo`). El id canónico de Symphony no
//!   puede llevar dos `/`, así que `opencode/big-pickle` queda igual y el resto es
//!   `opencode/<proveedor>:<modelo>` (ver [`canonical_id`]).
//! - Servidores MCP por invocación con `OPENCODE_CONFIG_CONTENT` (JSON que se suma a su config):
//!   el broker de contexto de Symphony y los de `mcp.toml`.
//! - Symphony no lee `~/.local/share/opencode`: el login y la config son cosa del CLI.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};
use symphony_adapter_common::{
    AdapterError, AgentEvent, AuthStatus, Detection, ModelInfo, ProcessSpec, ProviderAdapter,
    ProviderError, ResumeRequest, SpawnRequest, ToolKind, find_on_path, has_token, json_str,
    looks_like_error, parse_retry_after_ms,
};
use symphony_core::FailureType;

/// Sin `-m`: OpenCode usa el modelo que tenga configurado o el último que usó.
const DEFAULT_MODEL: &str = "default";
/// El proveedor propio de OpenCode (Zen): sus modelos conservan el id corto.
const OWN_PROVIDER: &str = "opencode";
/// Tope de modelos registrados: con muchas credenciales `opencode models` lista miles.
const MODELS_MAX: usize = 200;
/// Cuánto vale la lista guardada antes de pedirla de nuevo.
const MODELS_TTL: Duration = Duration::from_secs(24 * 3600);
/// Tras este tiempo una consulta lanzada ya terminó (`opencode models` tarda ~7 s).
const MODELS_SETTLE: Duration = Duration::from_secs(30);
/// Una consulta que sigue sin terminar tras esto se da por colgada.
const MODELS_STALE: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Default)]
pub struct OpenCodeAdapter {
    pub binary: Option<PathBuf>,
    /// Dónde se guarda lo que lista `opencode models`; `None` = `~/.symphony/cache/…`.
    pub models_cache: Option<PathBuf>,
}

/// `opencode/big-pickle` → `opencode/big-pickle`; `github-copilot/gpt-5` → `opencode/github-copilot:gpt-5`.
pub fn canonical_id(cli_model: &str) -> String {
    match cli_model.split_once('/') {
        Some((OWN_PROVIDER, model)) => format!("{OWN_PROVIDER}/{model}"),
        Some((provider, model)) => format!("{OWN_PROVIDER}/{provider}:{model}"),
        None => format!("{OWN_PROVIDER}/{cli_model}"),
    }
}

/// La salida de `opencode models`: una línea `proveedor/modelo` cada una.
pub fn parse_models(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim) {
        let ok = line.split_once('/').is_some_and(|(p, m)| {
            !p.is_empty() && !m.is_empty() && !line.contains(char::is_whitespace)
        });
        if ok && !out.iter().any(|m| m == line) {
            out.push(line.to_string());
        }
        if out.len() == MODELS_MAX {
            break;
        }
    }
    out
}

fn tool_kind(name: &str) -> ToolKind {
    match name {
        "bash" => ToolKind::Command,
        "write" | "edit" | "multiedit" | "patch" | "apply_patch" => ToolKind::Edit,
        _ => ToolKind::Other,
    }
}

/// El `part` de un `tool_use` ya terminado → pedido, archivo tocado y resultado.
fn tool_events(part: &Value) -> Vec<AgentEvent> {
    let state = &part["state"];
    let status = state["status"].as_str().unwrap_or("");
    if status != "completed" && status != "error" {
        return Vec::new();
    }
    let Some(tool) = json_str(part, "tool").filter(|t| !t.is_empty()) else {
        return Vec::new();
    };
    let kind = tool_kind(&tool);
    let input = &state["input"];
    let id = json_str(part, "callID");
    let exit_code = state["metadata"]["exit"]
        .as_i64()
        .and_then(|c| i32::try_from(c).ok());
    let mut out = vec![AgentEvent::ToolRequested {
        tool_use_id: id.clone(),
        tool: tool.clone(),
        kind,
        command: (kind == ToolKind::Command)
            .then(|| json_str(input, "command"))
            .flatten(),
    }];
    // `bash` termina como `completed` aunque el comando falle: el éxito sale del código.
    let ok = status == "completed" && exit_code.unwrap_or(0) == 0;
    if kind == ToolKind::Edit && ok {
        out.push(AgentEvent::FileModified {
            tool: tool.clone(),
            path: json_str(input, "filePath").or_else(|| json_str(input, "path")),
        });
    }
    out.push(AgentEvent::ToolFinished {
        tool_use_id: id,
        tool,
        kind,
        ok,
        exit_code,
    });
    out
}

/// Tokens que el modelo leyó en el paso: entrada más caché (lo que ocupa el contexto).
fn context_tokens(tokens: &Value) -> Option<u64> {
    let input = tokens["input"].as_u64()?;
    let cache = |k: &str| tokens["cache"][k].as_u64().unwrap_or(0);
    Some(input + cache("read") + cache("write"))
}

/// Permisos de OpenCode que corresponden a sus herramientas propias.
const OPENCODE_NATIVE_TOOLS: [&str; 8] = [
    "bash", "edit", "read", "glob", "grep", "list", "webfetch", "task",
];

/// El JSON de `OPENCODE_CONFIG_CONTENT` de un spawn: el permiso para el worktree y, si hay, los
/// servidores MCP (broker de Symphony y `mcp.toml`).
///
/// OpenCode toma como raíz del proyecto el repo principal; el worktree del agente queda como
/// «directorio externo», y en headless un permiso que se pregunta se rechaza solo (verificado en
/// vivo: el `write` con ruta absoluta al worktree falla). Se permite **solo** ese subárbol.
/// Con la ruta larga de Windows: una corta (`LATITU~1`) no coincide con el patrón.
pub fn run_config(req: &SpawnRequest) -> Value {
    let wt = req.worktree.to_string_lossy().into_owned();
    let fwd = wt.replace('\\', "/");
    let mut external = serde_json::Map::new();
    external.insert(format!("{}/**", fwd.trim_end_matches('/')), json!("allow"));
    external.insert(format!("{}\\**", wt.trim_end_matches('\\')), json!("allow"));
    let mut cfg = json!({ "permission": { "external_directory": external } });
    // Harness de Symphony (ADR-0019): se niegan las herramientas propias de OpenCode y el modelo
    // trabaja con las del broker (verificado en vivo con opencode 2.1.4).
    if symphony_adapter_common::harness_on(req) && symphony_adapter_common::broker(req).is_some() {
        for tool in OPENCODE_NATIVE_TOOLS {
            cfg["permission"][tool] = json!("deny");
        }
    }
    if let Some(mcp) = mcp_config(req) {
        cfg["mcp"] = mcp["mcp"].clone();
    }
    cfg
}

/// El bloque `mcp` de la config: el broker de Symphony y los servidores de `mcp.toml`;
/// `None` si no hay ninguno que registrar.
pub fn mcp_config(req: &SpawnRequest) -> Option<Value> {
    let mut servers = serde_json::Map::new();
    if let Some((cmd, env)) = symphony_adapter_common::broker(req) {
        let mut command = vec![cmd.program.to_string_lossy().into_owned()];
        command.extend(cmd.args);
        servers.insert(
            symphony_adapter_common::MCP_SERVER_NAME.into(),
            json!({
                "type": "local",
                "command": command,
                "environment": env.into_iter().collect::<std::collections::BTreeMap<_, _>>(),
                "enabled": true,
            }),
        );
    }
    for m in symphony_adapter_common::extra_mcp(req) {
        let def = if m.http {
            json!({ "type": "remote", "url": m.url, "enabled": true })
        } else {
            let mut command = vec![m.command.clone().unwrap_or_default()];
            command.extend(m.args.iter().cloned());
            json!({
                "type": "local",
                "command": command,
                "environment": m.env.iter().cloned().collect::<std::collections::BTreeMap<_, _>>(),
                "enabled": true,
            })
        };
        servers.insert(m.name, def);
    }
    (!servers.is_empty()).then(|| json!({ "mcp": servers }))
}

impl OpenCodeAdapter {
    fn binary(&self) -> Result<PathBuf, AdapterError> {
        self.binary
            .clone()
            .or_else(find_opencode)
            .ok_or_else(|| AdapterError::NotInstalled("opencode".into()))
    }

    /// Un `error` del stream o una línea de stderr → error clasificado. Si no se reconoce, queda
    /// como `PROVIDER_ERROR` (no pide failover) para que la falla se vea en vez de perderse.
    fn error_event(&self, name: &str, text: &str) -> Vec<AgentEvent> {
        let full = format!("{name}: {text}");
        let e = self.parse_error(&full).unwrap_or_else(|| ProviderError {
            failure_type: FailureType::ProviderError,
            raw_code: Some(name.to_string()).filter(|n| !n.is_empty()),
            message: symphony_core::redact(&full).chars().take(500).collect(),
            retry_after_ms: None,
            resets_at: None,
            transient: false,
        });
        vec![AgentEvent::ProviderError(e)]
    }

    fn base_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError> {
        let mut spec = ProcessSpec::new(self.binary()?)
            .args(["run", "--format", "json"])
            .cwd(&req.worktree)
            .with_stdin();
        if req.model != DEFAULT_MODEL {
            spec = spec.args(["-m", &req.model]);
        }
        for (k, v) in &req.env {
            spec = spec.env(k, v);
        }
        spec = spec.env("OPENCODE_CONFIG_CONTENT", run_config(req).to_string());
        Ok(spec)
    }
}

/// El `opencode.exe` nativo del paquete npm (evita el shim `.cmd`) o lo que haya en el PATH.
fn find_opencode() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if cfg!(windows) && dir.join("opencode.cmd").is_file() {
            // El paquete se llamaba `opencode-ai`; desde 2026-10 es `@opencode/cli`.
            for pkg in ["@opencode/cli", "opencode-ai"] {
                let native = dir.join("node_modules").join(pkg).join("bin/opencode.exe");
                if native.is_file() {
                    return Some(native);
                }
            }
        }
    }
    find_on_path("opencode")
}

/// La versión del paquete npm (`.../@opencode/cli/bin/opencode.exe` → `../package.json`).
pub fn version_from_package(binary: &Path) -> Option<String> {
    let pkg = binary.parent()?.parent()?.join("package.json");
    let json: Value = serde_json::from_str(&std::fs::read_to_string(pkg).ok()?).ok()?;
    (json["name"] == "opencode-ai" || json["name"] == "@opencode/cli")
        .then(|| json["version"].as_str().map(str::to_string))
        .flatten()
        .filter(|v| !v.is_empty())
}

fn default_models_cache() -> Option<PathBuf> {
    std::env::home_dir().map(|h| h.join(".symphony/cache/opencode-models.txt"))
}

/// Los modelos guardados. Si la lista falta o es más vieja que [`MODELS_TTL`], pide una nueva
/// **sin esperarla**: lanza `opencode models` suelto, con la salida a `<cache>.tmp`, y la próxima
/// llamada (otro arranque o `providers refresh`) la promueve a la lista vigente. Así el proceso
/// que la pidió puede morir sin perderla, y un solo lanzamiento a la vez (el `.tmp` se crea con
/// `create_new`).
fn cached_models(binary: &Path, cache: Option<&Path>) -> Vec<String> {
    let Some(path) = cache.map(Path::to_path_buf).or_else(default_models_cache) else {
        return Vec::new();
    };
    let tmp = path.with_extension("tmp");
    let age = |p: &Path| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok())
    };
    // Una consulta anterior ya terminó (tarda ~7 s): se promueve si salió bien, y se descarta si no.
    if age(&tmp).is_some_and(|a| a > MODELS_SETTLE) {
        let done = std::fs::read_to_string(&tmp).is_ok_and(|t| !parse_models(&t).is_empty());
        if done {
            let _ = std::fs::rename(&tmp, &path);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    if age(&path).is_none_or(|a| a > MODELS_TTL) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // `create_new` hace de cerrojo entre procesos. Un `.tmp` colgado (> 10 min) se limpia.
        if age(&tmp).is_some_and(|a| a > MODELS_STALE) {
            let _ = std::fs::remove_file(&tmp);
        }
        if let Ok(out) = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            let started = Command::new(binary)
                .arg("models")
                .stdin(Stdio::null())
                .stdout(Stdio::from(out))
                .stderr(Stdio::null())
                .spawn();
            if started.is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
    std::fs::read_to_string(&path)
        .map(|t| parse_models(&t))
        .unwrap_or_default()
}

impl ProviderAdapter for OpenCodeAdapter {
    fn provider_id(&self) -> &'static str {
        "opencode"
    }

    fn display_name(&self) -> &'static str {
        "OpenCode"
    }

    fn cli_name(&self) -> &'static str {
        "opencode"
    }

    fn detect(&self) -> Result<Detection, AdapterError> {
        let cli_path = self.binary()?;
        // `opencode --version` tarda ~2,5 s (arranca todo el runtime) y `detect` corre en cada
        // arranque del daemon: si el paquete npm lo dice en su `package.json`, se lee de ahí.
        if let Some(version) = version_from_package(&cli_path) {
            return Ok(Detection { cli_path, version });
        }
        let out = Command::new(&cli_path)
            .arg("--version")
            .stdin(Stdio::null())
            .output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        // "1.18.34"
        let version = text.split_whitespace().last().unwrap_or("").to_string();
        if !out.status.success() || version.is_empty() {
            return Err(AdapterError::Invalid(format!(
                "`opencode --version` respondió: {}",
                text.trim()
            )));
        }
        Ok(Detection { cli_path, version })
    }

    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Unknown
    }

    /// `default` siempre; el resto sale de `opencode models` (depende de las credenciales de Leo).
    fn list_models(&self) -> Vec<ModelInfo> {
        let mut out = vec![ModelInfo {
            id: format!("{OWN_PROVIDER}/{DEFAULT_MODEL}"),
            cli_model_id: DEFAULT_MODEL.into(),
            display_name: "OpenCode (modelo configurado)".into(),
        }];
        // `opencode models` tarda ~6,5 s: no se espera aquí. Se lee la lista guardada y, si
        // falta o venció, se pide en segundo plano (aparece en el próximo `providers refresh`).
        let listed = self
            .binary()
            .ok()
            .map(|b| cached_models(&b, self.models_cache.as_deref()))
            .unwrap_or_default();
        out.extend(listed.into_iter().map(|m| ModelInfo {
            id: canonical_id(&m),
            display_name: format!("OpenCode · {m}"),
            cli_model_id: m,
        }));
        out
    }

    /// OpenCode tiene plugins y no hooks por invocación: sin verificar.
    fn supports_hooks(&self) -> bool {
        false
    }

    fn hooks_can_hold(&self) -> Option<bool> {
        None
    }

    /// OpenCode asigna el id de sesión: `req.session_id` no se usa y el id sale del primer evento.
    fn spawn_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError> {
        self.base_spec(req)
    }

    fn resume_spec(&self, req: &ResumeRequest) -> Result<ProcessSpec, AdapterError> {
        Ok(self
            .base_spec(&req.spawn)?
            .args(["-s", &req.cli_session_id]))
    }

    fn attach_spec(
        &self,
        cli_session_id: &str,
        model: &str,
        worktree: &Path,
    ) -> Result<ProcessSpec, AdapterError> {
        let mut spec = ProcessSpec::new(self.binary()?).args(["-s", cli_session_id]);
        if model != DEFAULT_MODEL {
            spec = spec.args(["-m", model]);
        }
        Ok(spec.cwd(worktree))
    }

    /// OpenCode toma como raíz del proyecto el repo principal (el `--git-common-dir`), no el
    /// worktree del agente: sin esto, un modelo que arma rutas absolutas escribe fuera de su
    /// aislamiento (verificado en vivo, ADR-0012). El directorio de trabajo sí es el worktree.
    fn frame_prompt(&self, prompt: &str, worktree: &Path) -> String {
        format!(
            "Tu directorio de trabajo es `{wt}`. Trabaja SOLO dentro de él: usa rutas relativas              a ese directorio o rutas absolutas que empiecen por él. No leas ni escribas en ningún              otro directorio del proyecto, aunque aparezca como raíz del workspace: otros agentes              trabajan allí.

{prompt}",
            wt = worktree.display()
        )
    }

    fn encode_prompt(&self, prompt: &str) -> Vec<u8> {
        prompt.as_bytes().to_vec()
    }

    fn close_stdin_after_prompt(&self) -> bool {
        true
    }

    /// Como Codex y Kimi: los mensajes van en el próximo turno, con `resume`.
    fn encode_user_message(&self, _text: &str) -> Option<Vec<u8>> {
        None
    }

    fn parse_stream_line(&self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        let part = &v["part"];
        match v.get("type").and_then(Value::as_str) {
            // Se repite en cada paso del turno: el daemon guarda el mismo id de nuevo.
            Some("step_start") => json_str(&v, "sessionID")
                .map(|id| {
                    vec![AgentEvent::SessionStarted {
                        cli_session_id: Some(id),
                        model: None,
                    }]
                })
                .unwrap_or_default(),
            Some("text") => json_str(part, "text")
                .filter(|t| !t.trim().is_empty())
                .map(|text| vec![AgentEvent::AssistantText { text }])
                .unwrap_or_default(),
            Some("tool_use") => tool_events(part),
            Some("step_finish") => {
                let mut out = Vec::new();
                if let Some(context_tokens) = context_tokens(&part["tokens"]) {
                    out.push(AgentEvent::TurnUsage { context_tokens });
                }
                if part["reason"] == "stop" {
                    out.push(AgentEvent::TurnFinished { last_message: None });
                }
                out
            }
            Some("error") => {
                let e = &v["error"];
                // 1.x: `{name, data: {message, statusCode, responseBody}}`; 2.x: `{type, message,
                // status, retryAfterMs}` (visto con opencode 2.1.4 al agotarse el uso gratuito).
                let mut text = e["data"]["message"]
                    .as_str()
                    .or_else(|| e["message"].as_str())
                    .unwrap_or("")
                    .to_string();
                for (key, x) in [
                    ("statusCode", &e["data"]["statusCode"]),
                    ("responseBody", &e["data"]["responseBody"]),
                    ("status", &e["status"]),
                ] {
                    if !x.is_null() {
                        text.push_str(&format!(" {key}={}", x.to_string().trim_matches('"')));
                    }
                }
                let name = e["name"]
                    .as_str()
                    .or_else(|| e["type"].as_str())
                    .unwrap_or("");
                let mut events = self.error_event(name, &text);
                if let (Some(AgentEvent::ProviderError(pe)), Some(ms)) =
                    (events.first_mut(), e["retryAfterMs"].as_u64())
                {
                    pe.retry_after_ms = pe.retry_after_ms.or(Some(ms));
                }
                events
            }
            _ => Vec::new(),
        }
    }

    /// Sin `--print-logs` stderr va vacío; si algo sale, solo cuenta lo que parece un error.
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

    /// Sin verificar en vivo: con los modelos gratuitos de OpenCode no se pudo provocar cuota ni
    /// auth. La forma real del evento (`error.name` + `error.data`) sí salió de OpenCode 1.18.34.
    fn parse_error(&self, text: &str) -> Option<ProviderError> {
        let lower = text.to_lowercase();
        let (failure_type, code, transient) =
            if lower.contains("modelnotfound") || lower.contains("model not found") {
                (FailureType::ModelUnavailable, "model_not_found", false)
            } else if lower.contains("quota exceeded")
                || lower.contains("usage limit")
                || lower.contains("insufficient")
                || lower.contains("credit balance")
            {
                (FailureType::DailyQuota, "quota", false)
            } else if lower.contains("rate limit")
                || lower.contains("rate_limit")
                || has_token(&lower, "429")
                || lower.contains("too many requests")
            {
                (FailureType::TempRateLimit, "rate_limit", true)
            } else if lower.contains("quota") {
                (FailureType::DailyQuota, "quota", false)
            } else if has_token(&lower, "401")
                || lower.contains("providerautherror")
                || lower.contains("unauthorized")
                || lower.contains("invalid api key")
                || lower.contains("opencode auth login")
                || lower.contains("opencode providers login")
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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/providers/opencode")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_with_quota_text_is_classified_as_transient() {
        let adapter = OpenCodeAdapter::default();
        let err = adapter
            .parse_error("rate limit: too many requests, quota per minute")
            .expect("debe clasificar el error");
        assert_eq!(err.failure_type, FailureType::TempRateLimit);
        assert!(err.transient);
    }
}
