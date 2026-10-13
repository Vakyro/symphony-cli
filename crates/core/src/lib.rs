//! Modelo de dominio de Symphony: IDs, enums de DB y transiciones de estado.
//! AGENT ≠ MODEL: el agente no guarda el modelo actual; eso vive en su run abierto.

mod ansi;
mod config;
mod enums;
mod health;
mod ids;
pub mod learning;
mod redact;
mod transitions;

pub use ansi::{AnsiMode, sanitize};
pub use config::{
    ChatConfig, Config, ConfigError, ContextConfig, DEFAULT_CONFIG, LoggingConfig,
    PerformanceConfig, PhaseStart, PlanConfig, PluginsConfig, ProjectConfig, ProjectInit,
    ProjectSection, ProviderLimits, ProviderPermissions, ProvidersConfig, RoutingConfig,
    SymphonyHome, init_project, load_or_create, load_project, project_config_path, set_value,
};
pub use config::{MergeConfig, ValidationConfig, WorktreeConfig};
pub use enums::{
    AccountAuthStatus, AgentState, CheckName, CheckStatus, ConflictResolution, ContextMode,
    ExecutionMode, FailoverPolicy, FailureType, InvalidValue, MergeStatus, MilestoneStatus,
    PerformanceProfile, ProviderState, QuotaCertainty, RejectReason, ReviewRecommendation,
    ReviewStatus, RoutingTrigger, RunEndReason, RunStatus, SpeedClass, SuggestionStatus,
    TaskStatus, UsageSource, ValidationStatus, ValidationTrigger,
};
pub use health::{
    DEGRADED_COOLDOWN_MS, EXHAUSTED_PROBE_MS, Health, HealthEvent, NETWORK_COOLDOWN_MS,
    OFFLINE_AFTER, RATE_LIMIT_COOLDOWN_MS, THROTTLE_AFTER, THROTTLED_COOLDOWN_MS,
};
pub use ids::{
    AgentId, CheckpointId, ContextObjectId, ExecutorChangeId, FactId, HandoffId, HandoffItemId,
    InvalidId, McpServerId, MessageId, MilestoneId, ProcessId, ProjectId, ProviderFailureId,
    ProviderHealthId, RecoveryItemId, RoutingDecisionId, RunId, SessionId, SkillId, TaskId,
    ToolCallId, UsageRecordId, WorktreeId,
};
pub use redact::{REDACTED, redact};
pub use transitions::InvalidTransition;
