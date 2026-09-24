//! Guion del `fake-agent` (TOML). Ejemplo:
//!
//! ```toml
//! model = "fake/sonnet"
//!
//! [[step]]
//! kind = "say"
//! text = "Voy a crear el archivo"
//!
//! [[step]]
//! kind = "edit"
//! path = "src/a.js"
//! content = "export const a = 1;\n"
//!
//! [[step]]
//! kind = "run"
//! command = ["node", "-e", "console.log(42)"]
//!
//! [[step]]
//! kind = "rate_limit"
//! retry_after_ms = 1500
//! ```

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    #[serde(default = "default_model")]
    pub model: String,
    /// Si se omite, el fake-agent genera uno.
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default, rename = "step")]
    pub steps: Vec<Step>,
}

fn default_model() -> String {
    "fake/model".into()
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// Texto del asistente.
    Say {
        text: String,
    },
    /// Escribe un archivo (tool `Write`), con hooks Pre/PostToolUse.
    Edit {
        path: String,
        content: String,
    },
    /// Corre un comando de verdad (tool `Bash`), con hooks Pre/PostToolUse.
    Run {
        command: Vec<String>,
    },
    /// Lee una línea de stdin como mensaje del usuario (ADR-0005).
    WaitInput,
    Sleep {
        ms: u64,
    },
    /// 429 temporal: emite `api_retry` con `error = rate_limit` y sigue.
    RateLimit {
        retry_after_ms: u64,
    },
    /// Cuota agotada: error final y exit 1.
    QuotaExhausted {
        resets_at: Option<i64>,
    },
    /// Login inválido o vencido: error final y exit 1.
    AuthError,
    /// Muere sin cleanup (exit 134, como un abort).
    Crash,
    /// Se cuelga para siempre (para probar timeouts y kills).
    Hang,
}

impl Script {
    pub fn parse(text: &str) -> Result<Self, toml_edit::de::Error> {
        toml_edit::de::from_str(text)
    }
}
