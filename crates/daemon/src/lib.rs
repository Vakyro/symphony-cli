//! Daemon de Symphony (`symphonyd`): dueño del estado del proyecto.

pub mod bus;
pub mod checkpoint;
pub mod executor;
pub mod handoff;
pub mod logging;
pub mod providers;
pub mod recorder;
pub mod runtime;
pub mod server;
pub mod views;
