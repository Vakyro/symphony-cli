//! Modelo de dominio de Symphony: IDs, enums de DB y transiciones de estado.
//! AGENT ≠ MODEL: el agente no guarda el modelo actual; eso vive en su run abierto.

mod enums;
mod ids;
mod transitions;

pub use enums::{
    AgentState, ContextMode, ExecutionMode, FailoverPolicy, FailureType, InvalidValue,
    ProviderState, QuotaCertainty, RunEndReason, RunStatus, TaskStatus,
};
pub use ids::{AgentId, CheckpointId, InvalidId, ProjectId, RunId, SessionId, TaskId, WorktreeId};
pub use transitions::InvalidTransition;
