//! Protocolo v1 (ADR-0015): una petición y una respuesta, una línea JSON cada una.
//!
//! Lo que no se entiende es un error del plugin: versión distinta, JSON roto, `ok` ausente o
//! una línea de más de [`MAX_RESPONSE_BYTES`].

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::manifest::{Kind, SUPPORTED_API};

/// Tope de la línea de respuesta.
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Una petición al plugin.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Request {
    pub plugin_api: u32,
    pub kind: &'static str,
    pub method: &'static str,
    pub params: Value,
}

impl Request {
    pub fn new(kind: Kind, method: &'static str, params: Value) -> Self {
        Self {
            plugin_api: SUPPORTED_API,
            kind: kind.as_str(),
            method,
            params,
        }
    }

    pub fn describe(kind: Kind) -> Self {
        Self::new(kind, "describe", json!({}))
    }

    /// La línea que se escribe en stdin (con salto de línea final).
    pub fn to_line(&self) -> String {
        let mut line = serde_json::to_string(self).unwrap_or_default();
        line.push('\n');
        line
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct RawResponse {
    plugin_api: Option<u32>,
    ok: Option<bool>,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("el plugin no devolvió ninguna respuesta por stdout")]
    Empty,
    #[error("la respuesta del plugin pasa de {MAX_RESPONSE_BYTES} bytes")]
    TooLarge,
    #[error("la respuesta del plugin no es JSON válido: {0}")]
    Json(String),
    #[error("el plugin habla `plugin_api = {0:?}` y este Symphony entiende {SUPPORTED_API}")]
    Api(Option<u32>),
    #[error("la respuesta del plugin no dice `ok`")]
    MissingOk,
    #[error("el plugin respondió con un error: {0}")]
    Remote(String),
}

/// Lee la respuesta: la última línea no vacía de stdout.
pub fn parse_response(stdout: &str) -> Result<Value, ProtocolError> {
    if stdout.len() > MAX_RESPONSE_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let line = stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .ok_or(ProtocolError::Empty)?;
    let raw: RawResponse =
        serde_json::from_str(line).map_err(|e| ProtocolError::Json(e.to_string()))?;
    if raw.plugin_api != Some(SUPPORTED_API) {
        return Err(ProtocolError::Api(raw.plugin_api));
    }
    match raw.ok {
        Some(true) => Ok(raw.result),
        Some(false) => Err(ProtocolError::Remote(
            raw.error.unwrap_or_else(|| "sin detalle".into()),
        )),
        None => Err(ProtocolError::MissingOk),
    }
}

/// Entrada de `validate`: lo que ve un validator (nada más).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidateInput {
    pub task_code: String,
    pub task_title: String,
    /// Diff unificado de la tarea contra su base.
    pub diff: String,
    /// Archivos que toca.
    pub files: Vec<String>,
}

/// Un hallazgo de un validator.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Finding {
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub text: String,
}

/// Salida de `validate`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ValidateOutput {
    pub passed: bool,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<Finding>,
}

/// Salida de `describe`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Description {
    pub name: String,
    #[serde(default)]
    pub version: String,
    pub plugin_api: u32,
    pub kind: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_non_empty_line_is_the_response() {
        let out = "ruido\n\n{\"plugin_api\":1,\"ok\":true,\"result\":{\"a\":1}}\n\n";
        assert_eq!(parse_response(out).unwrap(), json!({"a": 1}));
    }

    #[test]
    fn anything_unexpected_is_an_error() {
        let p = parse_response;
        assert_eq!(p("").unwrap_err(), ProtocolError::Empty);
        assert!(matches!(p("no json").unwrap_err(), ProtocolError::Json(_)));
        assert_eq!(
            p("{\"plugin_api\":2,\"ok\":true}").unwrap_err(),
            ProtocolError::Api(Some(2))
        );
        assert_eq!(p("{\"ok\":true}").unwrap_err(), ProtocolError::Api(None));
        assert_eq!(
            p("{\"plugin_api\":1}").unwrap_err(),
            ProtocolError::MissingOk
        );
        assert_eq!(
            p("{\"plugin_api\":1,\"ok\":false,\"error\":\"boom\"}").unwrap_err(),
            ProtocolError::Remote("boom".into())
        );
        let big = format!(
            "{{\"plugin_api\":1,\"ok\":true,\"result\":\"{}\"}}",
            "x".repeat(MAX_RESPONSE_BYTES)
        );
        assert_eq!(p(&big).unwrap_err(), ProtocolError::TooLarge);
    }

    #[test]
    fn a_request_is_one_json_line() {
        let r = Request::new(Kind::Validator, "validate", json!({"diff": "+a\n"}));
        let line = r.to_line();
        assert!(
            line.ends_with('\n') && line.matches('\n').count() == 1,
            "{line:?}"
        );
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            (v["plugin_api"].clone(), v["kind"].clone()),
            (json!(1), json!("validator"))
        );
    }
}
