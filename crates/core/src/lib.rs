//! Modelo de dominio de Symphony: IDs, enums de DB y transiciones de estado.
//! AGENT ≠ MODEL: el agente no guarda el modelo actual; eso vive en su run abierto.

mod ansi;
mod config;
mod enums;
mod health;
mod ids;
mod redact;
mod transitions;

pub use ansi::{AnsiMode, sanitize};
pub use config::{
    ChatConfig, Config, ConfigError, ContextConfig, DEFAULT_CONFIG, LoggingConfig,
    PerformanceConfig, ProjectConfig, ProjectInit, ProjectSection, ProviderLimits, ProvidersConfig,
    RoutingConfig, SymphonyHome, init_project, load_or_create, load_project, project_config_path,
    set_value,
};
pub use enums::{
    AccountAuthStatus, AgentState, ContextMode, ExecutionMode, FailoverPolicy, FailureType,
    InvalidValue, PerformanceProfile, ProviderState, QuotaCertainty, RejectReason, RoutingTrigger,
    RunEndReason, RunStatus, SpeedClass, TaskStatus, UsageSource,
};
pub use health::{
    DEGRADED_COOLDOWN_MS, EXHAUSTED_PROBE_MS, Health, HealthEvent, NETWORK_COOLDOWN_MS,
    OFFLINE_AFTER, RATE_LIMIT_COOLDOWN_MS, THROTTLE_AFTER, THROTTLED_COOLDOWN_MS,
};
pub use ids::{
    AgentId, CheckpointId, ContextObjectId, ExecutorChangeId, HandoffId, HandoffItemId, InvalidId,
    MessageId, ProjectId, ProviderFailureId, ProviderHealthId, RecoveryItemId, RoutingDecisionId,
    RunId, SessionId, TaskId, ToolCallId, UsageRecordId, WorktreeId,
};
pub use redact::{REDACTED, redact};
pub use transitions::InvalidTransition;
