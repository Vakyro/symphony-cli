//! Plugins de Symphony (P12, ADR-0015): procesos aislados con un protocolo JSON de una vuelta.
//!
//! - [`manifest`]: `plugin.toml` validado.
//! - [`protocol`]: petición y respuesta v1 (`describe`, `validate`, `transform`).
//! - [`host`]: descubrimiento y ejecución bajo el supervisor de procesos.

pub mod host;
pub mod manifest;
pub mod protocol;

pub use host::{Discovery, Plugin, PluginError, discover};
pub use manifest::{Kind, Manifest, ManifestError};
pub use protocol::{Finding, ValidateInput, ValidateOutput};
