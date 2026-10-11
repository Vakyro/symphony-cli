//! Daemon de Symphony (`symphonyd`): dueño del estado del proyecto.

pub mod attach;
pub mod bus;
pub mod checkpoint;
pub mod context_index;
pub mod context_mcp;
pub mod context_views;
pub mod executor;
pub mod export;
pub mod handoff;
pub mod health;
pub mod logging;
pub mod providers;
pub mod recorder;
pub mod resources;
pub mod routing;
pub mod runtime;
pub mod server;
pub mod shared;
pub mod skills;
pub mod views;
pub mod watcher;
