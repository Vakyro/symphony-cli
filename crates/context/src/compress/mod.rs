//! Compresores deterministas (STACK §14.3): sin LLM, sin embeddings y siempre reversibles, porque
//! el original queda en el object store y la versión compacta es solo un resumen de él.
//!
//! Cada compresor es una función pura y total: con cualquier texto devuelve un resultado y nunca
//! entra en pánico. [`compress`] elige el compresor según lo que parece el texto y **nunca
//! infla**: si el resultado no es más corto que el original, devuelve el original.

pub mod dedup;
pub mod diff;
pub mod json;
pub mod log;
pub mod tests_summary;

use crate::handoff::estimate_tokens;

/// Qué compresor produjo un texto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compressor {
    LogCollapse,
    TestSummary,
    JsonStruct,
    Dedup,
    GitDiff,
    /// Se devuelve el original: ningún compresor lo hizo más corto.
    None,
}

impl Compressor {
    /// El valor de `context_objects.compressor` (DB §3.G). Los resúmenes de tests se guardan
    /// como `LOG_COLLAPSE` y el reductor de diffs como `DEDUP`: el CHECK de la base solo
    /// admite cinco etiquetas.
    pub fn db_label(self) -> &'static str {
        match self {
            Self::LogCollapse | Self::TestSummary => "LOG_COLLAPSE",
            Self::JsonStruct => "JSON_STRUCT",
            Self::Dedup | Self::GitDiff => "DEDUP",
            Self::None => "NONE",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::LogCollapse => "LogCollapser",
            Self::TestSummary => "TestSummaryCompressor",
            Self::JsonStruct => "JsonStructuralCompressor",
            Self::Dedup => "Deduplicator",
            Self::GitDiff => "GitDiffReducer",
            Self::None => "ninguno",
        }
    }
}

/// Qué es el texto, si el llamador lo sabe (el `kind` del objeto de contexto).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    Auto,
    Log,
    Json,
    Diff,
    TestOutput,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Compressed {
    pub text: String,
    pub compressor: Compressor,
    pub tokens_original: i64,
    pub tokens_compressed: i64,
}

impl Compressed {
    fn unchanged(text: &str) -> Self {
        let t = estimate_tokens(text);
        Self {
            text: text.to_string(),
            compressor: Compressor::None,
            tokens_original: t,
            tokens_compressed: t,
        }
    }
}

fn detect(text: &str) -> Hint {
    let head = text.trim_start();
    if (head.starts_with('{') || head.starts_with('['))
        && serde_json::from_str::<serde_json::Value>(text).is_ok()
    {
        return Hint::Json;
    }
    if text.contains("\ndiff --git ") || text.starts_with("diff --git ") || text.contains("\n@@ -")
    {
        return Hint::Diff;
    }
    if tests_summary::summarize(text).is_some() {
        return Hint::TestOutput;
    }
    Hint::Log
}

/// Comprime `text`. Nunca devuelve algo más largo que el original.
pub fn compress(text: &str, hint: Hint) -> Compressed {
    let hint = if hint == Hint::Auto {
        detect(text)
    } else {
        hint
    };
    let (out, compressor) = match hint {
        Hint::Json => match json::summarize(text) {
            Some(s) => (s, Compressor::JsonStruct),
            None => (log::collapse(text), Compressor::LogCollapse),
        },
        Hint::Diff => (diff::reduce(text), Compressor::GitDiff),
        Hint::TestOutput => match tests_summary::summarize(text) {
            Some(s) => (s, Compressor::TestSummary),
            None => (log::collapse(text), Compressor::LogCollapse),
        },
        Hint::Log | Hint::Auto => {
            let collapsed = log::collapse(text);
            // Lo que se repite en bloques lejanos también se reduce.
            let deduped = dedup::dedup_blocks(&collapsed);
            if deduped.len() < collapsed.len() {
                (deduped, Compressor::Dedup)
            } else {
                (collapsed, Compressor::LogCollapse)
            }
        }
    };
    if out.len() >= text.len() {
        return Compressed::unchanged(text);
    }
    Compressed {
        tokens_original: estimate_tokens(text),
        tokens_compressed: estimate_tokens(&out),
        text: out,
        compressor,
    }
}

#[cfg(test)]
mod tests;
