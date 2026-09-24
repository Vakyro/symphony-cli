//! Modelo de dominio de Symphony: IDs, enums de DB y transiciones de estado.
//! AGENT ≠ MODEL: el agente no guarda el modelo actual; eso vive en su run abierto.

mod config;
mod enums;
mod ids;
mod redact;
mod transitions;

pub use config::{
    Config, ConfigError, ContextConfig, DEFAULT_CONFIG, LoggingConfig, PerformanceConfig,
    ProjectConfig, ProjectSection, ProvidersConfig, RoutingConfig, SymphonyHome, create_project,
    load_or_create, load_project, project_config_path, set_value,
};
pub use enums::{
    AgentState, ContextMode, ExecutionMode, FailoverPolicy, FailureType, InvalidValue,
    PerformanceProfile, ProviderState, QuotaCertainty, RunEndReason, RunStatus, TaskStatus,
};
pub use ids::{AgentId, CheckpointId, InvalidId, ProjectId, RunId, SessionId, TaskId, WorktreeId};
pub use redact::{REDACTED, redact};
pub use transitions::InvalidTransition;
