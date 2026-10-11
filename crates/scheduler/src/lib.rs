//! Scheduler de Symphony (IDEA §5.4, STACK §8). Sin E/S: reglas puras y deterministas.
//!
//! Clasificador de operaciones (P08.S2) y cola con slots pesados, equidad y explicación
//! de la espera (P08.S3), más el tope de agentes según la cuota (F4).

mod classify;
mod concurrency;
mod queue;

pub use classify::{OpClass, classify};
pub use concurrency::agent_concurrency;
pub use queue::{
    Admission, Grant, Holder, Limits, OsLimits, Request, Scheduler, Snapshot, WaitReason, explain,
};
