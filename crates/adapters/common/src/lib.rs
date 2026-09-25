//! Contrato de los provider adapters (STACK §18.1, §20; IDEA §5.2–§5.3).
//!
//! Un adapter traduce un CLI oficial a términos de Symphony: cómo lanzarlo,
//! cómo inyectarle hooks sin tocar el repo, y cómo leer su stream, sus hooks y
//! sus errores como [`AgentEvent`] canónicos. **El core nunca hace
//! `if provider == "claude"`**: todo lo específico vive detrás de este trait.
//!
//! Forma concreta del trait de STACK §18.1: `spawn`/`resume` devuelven un
//! [`ProcessSpec`] que lanza el supervisor genérico (`symphony-process`);
//! `stop` es genérico (`terminate_tree`); la salud sale de los errores parseados.
//! Todos los métodos son síncronos y puros, así el trait se usa como `dyn`.

pub mod contract;
pub mod hooks;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use symphony_core::FailureType;
pub use symphony_process::ProcessSpec;

/// Evento canónico (IDEA §5.3). Venga del stream o de un hook, se ve igual.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Arrancó o se reanudó la sesión del CLI (`AgentStarted`).
    SessionStarted {
        cli_session_id: Option<String>,
        model: Option<String>,
    },
    TurnStarted,
    /// Terminó el turno. `last_message`: fuente del "qué seguía" del checkpoint (H1).
    TurnFinished {
        last_message: Option<String>,
    },
    AssistantText {
        text: String,
    },
    UserMessage {
        text: String,
    },
    ToolRequested {
        tool_use_id: Option<String>,
        tool: String,
        kind: ToolKind,
        command: Option<String>,
    },
    ToolFinished {
        tool_use_id: Option<String>,
        tool: String,
        kind: ToolKind,
        ok: bool,
        exit_code: Option<i32>,
    },
    FileModified {
        tool: String,
        path: Option<String>,
    },
    ProviderError(ProviderError),
    /// Cuota conocida que reportó el CLI (Claude: `rate_limit_event`; Codex: rollout).
    Quota(QuotaSnapshot),
    AgentStopped {
        reason: Option<String>,
    },
}

impl AgentEvent {
    /// Valor para `events.type` (DB §3.F), con los nombres de IDEA §5.3.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::SessionStarted { .. } => "AgentStarted",
            Self::TurnStarted => "TurnStarted",
            Self::TurnFinished { .. } => "TurnFinished",
            Self::AssistantText { .. } => "AssistantText",
            Self::UserMessage { .. } => "UserMessage",
            Self::ToolRequested {
                kind: ToolKind::Command,
                ..
            } => "CommandRequested",
            Self::ToolRequested { .. } => "ToolRequested",
            Self::ToolFinished {
                kind: ToolKind::Command,
                ..
            } => "CommandFinished",
            Self::ToolFinished { .. } => "ToolFinished",
            Self::FileModified { .. } => "FileModified",
            Self::ProviderError(_) => "ProviderError",
            Self::Quota(_) => "QuotaUpdated",
            Self::AgentStopped { .. } => "AgentStopped",
        }
    }
}

/// Qué clase de herramienta es, para el scheduler y los checkpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    /// Shell (`Bash`, `exec_command`): candidato a retención del scheduler.
    Command,
    /// Escribe archivos (`Write`, `Edit`, `apply_patch`).
    Edit,
    Other,
}

/// Error ya clasificado (DB `provider_failures`). Un 429 no es una cuota agotada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderError {
    pub failure_type: FailureType,
    /// Código crudo: `429`, `rate_limit`, exit code, texto clave.
    pub raw_code: Option<String>,
    /// Mensaje ya redactado (sin secretos).
    pub message: String,
    pub retry_after_ms: Option<u64>,
    /// Epoch en segundos, si el CLI lo informa.
    pub resets_at: Option<i64>,
    /// `true` si el CLI mismo va a reintentar (el run sigue vivo).
    pub transient: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaSnapshot {
    /// `five_hour`, `seven_day`, …
    pub window: String,
    /// 0–1.
    pub used_fraction: f64,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub cli_path: PathBuf,
    pub version: String,
}

/// Estado de login según lo que reporta el CLI. Symphony nunca lee credenciales.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatus {
    Ok,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    /// Canónico: `claude/sonnet`.
    pub id: String,
    /// Lo que espera el CLI en `--model`.
    pub cli_model_id: String,
    pub display_name: String,
}

/// Comando que el CLI tiene que ejecutar como hook (`symphony hook emit …`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpawnRequest {
    pub worktree: PathBuf,
    /// `cli_model_id` del modelo elegido.
    pub model: String,
    /// Prompt inicial (objetivo o handoff). Va por stdin, nunca como argumento (LEARNINGS Test B).
    pub prompt: String,
    /// ID que Symphony fija para la sesión del CLI, si el CLI lo permite.
    pub session_id: Option<String>,
    pub hook: Option<HookCommand>,
    /// Variables que heredan el CLI y sus hooks (p. ej. `SYMPHONY_AGENT_ID`).
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResumeRequest {
    pub spawn: SpawnRequest,
    /// Sesión del CLI a continuar (`agent_runs.cli_session_id`).
    pub cli_session_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("no se encontró el CLI `{0}`")]
    NotInstalled(String),
    #[error("{0}")]
    Invalid(String),
    #[error("E/S: {0}")]
    Io(#[from] std::io::Error),
}

pub trait ProviderAdapter: Send + Sync {
    /// Slug estable del proveedor (`providers.id`): `anthropic`, `openai`, …
    fn provider_id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;

    fn detect(&self) -> Result<Detection, AdapterError>;
    fn auth_status(&self) -> AuthStatus;
    fn list_models(&self) -> Vec<ModelInfo>;
    fn supports_hooks(&self) -> bool;

    /// Comando para lanzar un turno nuevo en el worktree, con hooks inyectados por invocación.
    fn spawn_spec(&self, req: &SpawnRequest) -> Result<ProcessSpec, AdapterError>;
    /// Comando para continuar una sesión existente (`resume`).
    fn resume_spec(&self, req: &ResumeRequest) -> Result<ProcessSpec, AdapterError>;
    /// Bytes a escribir en stdin para el prompt inicial.
    fn encode_prompt(&self, prompt: &str) -> Vec<u8>;
    /// `true` si el CLI lee el prompt hasta EOF (Codex `exec -`): el runtime cierra stdin
    /// después de escribirlo. `false` si stdin queda abierto para más mensajes (Claude).
    fn close_stdin_after_prompt(&self) -> bool;
    /// Bytes para un mensaje del usuario a media tarea, o `None` si el CLI solo los
    /// acepta entre turnos (Codex, ADR-0005).
    fn encode_user_message(&self, text: &str) -> Option<Vec<u8>>;

    /// Una línea de stdout del CLI → eventos. Nunca falla: lo que no entiende lo ignora.
    fn parse_stream_line(&self, line: &str) -> Vec<AgentEvent>;
    /// El payload JSON de un hook → eventos.
    fn parse_hook(&self, payload: &Value) -> Vec<AgentEvent>;
    /// Texto de error (stderr, mensaje de fallo) → error clasificado, si lo reconoce.
    fn parse_error(&self, text: &str) -> Option<ProviderError>;
}
