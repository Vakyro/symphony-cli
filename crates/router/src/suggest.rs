//! Sugerencias de cambio de modelo por reglas (P14.S2, IDEA §5.7, FLOW §8.5). Puro y
//! determinista: sin E/S ni LLM. Una sugerencia **nunca** se aplica sola.
//!
//! Dos preguntas, ambas con reglas visibles:
//! 1. ¿Cambió el tipo de tarea? (documentación → refactor de varios archivos, por los archivos
//!    que lleva tocados.)
//! 2. ¿Hay otro modelo claramente mejor para ese tipo de tarea? (margen mínimo sobre el actual.)

use crate::Evaluated;

/// Qué tipo de trabajo parece que es la tarea, según los archivos que toca.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    /// Solo documentación (`.md`, `docs/`…).
    Docs,
    /// Solo configuración o documentación + configuración (`.toml`, `.json`, `.yml`…).
    Config,
    /// Mayoría de pruebas.
    Tests,
    /// Código en pocos archivos.
    Code,
    /// Código en varios archivos y carpetas (un refactor, una funcionalidad transversal).
    MultiFile,
}

/// Archivos de código mínimos y carpetas distintas para considerarlo `MultiFile`.
pub const MULTI_FILE_MIN_FILES: usize = 4;
pub const MULTI_FILE_MIN_DIRS: usize = 3;
/// Ventaja mínima de puntaje (0–1) sobre el modelo actual para sugerir otro.
pub const MIN_GAIN: f64 = 0.10;

fn ext(path: &str) -> &str {
    path.rsplit_once('.').map_or("", |(_, e)| e)
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn is_docs(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    matches!(ext(&p), "md" | "mdx" | "rst" | "adoc" | "txt")
        || p.starts_with("docs/")
        || p.starts_with("doc/")
}

fn is_config(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    matches!(
        ext(&p),
        "toml" | "json" | "yaml" | "yml" | "lock" | "ini" | "env" | "cfg"
    ) || matches!(
        file_name(&p),
        "dockerfile" | ".gitignore" | ".editorconfig" | "makefile"
    )
}

fn is_test(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.contains("test") || p.contains("spec") || p.contains("__tests__")
}

/// Carpeta de un archivo (vacía en la raíz).
fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// El tipo de tarea según los archivos que lleva tocados; `None` sin archivos.
pub fn classify(files: &[String]) -> Option<TaskKind> {
    if files.is_empty() {
        return None;
    }
    // Las rutas de Windows llegan con `\`.
    let files: Vec<String> = files.iter().map(|f| f.replace('\\', "/")).collect();
    let files = files.as_slice();
    let code: Vec<&String> = files
        .iter()
        .filter(|f| !is_docs(f) && !is_config(f))
        .collect();
    if code.is_empty() {
        let all_docs = files.iter().all(|f| is_docs(f));
        return Some(if all_docs {
            TaskKind::Docs
        } else {
            TaskKind::Config
        });
    }
    let tests = code.iter().filter(|f| is_test(f)).count();
    if tests * 2 > code.len() {
        return Some(TaskKind::Tests);
    }
    let dirs: std::collections::HashSet<&str> = code.iter().map(|f| dir_of(f)).collect();
    if code.len() >= MULTI_FILE_MIN_FILES && dirs.len() >= MULTI_FILE_MIN_DIRS {
        Some(TaskKind::MultiFile)
    } else {
        Some(TaskKind::Code)
    }
}

/// ¿Es un cambio de tipo que amerita preguntar? Devuelve la razón (la que ve el usuario).
/// Solo de lo liviano a lo pesado: lo contrario nunca justifica cambiar de modelo.
pub fn change_reason(start: TaskKind, now: TaskKind) -> Option<&'static str> {
    match (start, now) {
        (TaskKind::Docs, TaskKind::MultiFile) => {
            Some("la tarea pasó de documentación a un refactor de varios archivos")
        }
        (TaskKind::Config, TaskKind::MultiFile) => {
            Some("la tarea pasó de configuración a un cambio de código en varios archivos")
        }
        _ => None,
    }
}

/// Un modelo mejor que `current` para este tipo de trabajo: el elegible de mayor puntaje cuya
/// ventaja sobre el actual alcanza [`MIN_GAIN`]. Devuelve `(modelo, ventaja, confianza)`.
pub fn better_model(current: &str, evaluated: &[Evaluated]) -> Option<(String, f64, f64)> {
    // Si el actual no es elegible ahora (cooldown, cuota, offline) es un asunto de disponibilidad:
    // lo resuelve el failover, no una sugerencia de «mejor modelo».
    let current_score = evaluated
        .iter()
        .find(|e| e.model_id == current)
        .and_then(|e| e.score)?;
    let best = evaluated
        .iter()
        .filter(|e| e.eligible && e.model_id != current)
        .filter_map(|e| e.score.map(|s| (e, s)))
        .max_by(|a, b| {
            a.1.total_cmp(&b.1)
                .then_with(|| b.0.model_id.cmp(&a.0.model_id))
        })?;
    let gain = best.1 - current_score;
    if gain < MIN_GAIN {
        return None;
    }
    // La confianza crece con la ventaja y nunca llega a 1: es una heurística, no una certeza.
    let confidence = (0.5 + gain).clamp(0.0, 0.95);
    Some((best.0.model_id.clone(), gain, confidence))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    fn ev(model: &str, score: Option<f64>) -> Evaluated {
        Evaluated {
            model_id: model.into(),
            provider_id: model.split('/').next().unwrap_or("").into(),
            eligible: score.is_some(),
            reject: None,
            score,
            factors: Vec::new(),
        }
    }

    #[test]
    fn classification_follows_the_files() {
        assert_eq!(classify(&[]), None);
        assert_eq!(
            classify(&files(&["README.md", "docs/a.md"])),
            Some(TaskKind::Docs)
        );
        assert_eq!(
            classify(&files(&["README.md", "Cargo.toml"])),
            Some(TaskKind::Config)
        );
        assert_eq!(classify(&files(&["src/a.rs"])), Some(TaskKind::Code));
        assert_eq!(
            classify(&files(&["tests/a_test.rs", "tests/b_test.rs", "src/a.rs"])),
            Some(TaskKind::Tests)
        );
        assert_eq!(
            classify(&files(&["src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs"])),
            Some(TaskKind::Code),
            "cuatro archivos pero una sola carpeta"
        );
        assert_eq!(
            classify(&files(&["a/x.rs", "b/y.rs", "c/z.rs", "c/w.rs"])),
            Some(TaskKind::MultiFile)
        );
    }

    #[test]
    fn only_light_to_heavy_changes_are_worth_a_question() {
        assert!(change_reason(TaskKind::Docs, TaskKind::MultiFile).is_some());
        assert!(change_reason(TaskKind::Config, TaskKind::MultiFile).is_some());
        assert!(change_reason(TaskKind::MultiFile, TaskKind::Docs).is_none());
        assert!(change_reason(TaskKind::Docs, TaskKind::Code).is_none());
        assert!(change_reason(TaskKind::Code, TaskKind::MultiFile).is_none());
        assert!(change_reason(TaskKind::Docs, TaskKind::Docs).is_none());
    }

    #[test]
    fn a_better_model_needs_a_clear_margin_and_to_be_eligible() {
        let e = [
            ev("g/flash", Some(0.50)),
            ev("c/sonnet", Some(0.72)),
            ev("c/opus", None),
        ];
        let (m, gain, conf) = better_model("g/flash", &e).unwrap();
        assert_eq!(m, "c/sonnet");
        assert!((gain - 0.22).abs() < 1e-9);
        assert!(conf > 0.5 && conf <= 0.95);
        // Sin margen suficiente, nada.
        let close = [ev("g/flash", Some(0.50)), ev("c/sonnet", Some(0.55))];
        assert!(better_model("g/flash", &close).is_none());
        // El actual ya es el mejor.
        assert!(better_model("c/sonnet", &e).is_none());
        // El actual no es elegible ahora: eso es del failover, no una sugerencia.
        assert!(better_model("c/opus", &e).is_none());
        assert!(better_model("no/existe", &e).is_none());
        // Un modelo no elegible nunca se sugiere.
        let only_ineligible = [ev("g/flash", Some(0.1)), ev("c/opus", None)];
        assert!(better_model("g/flash", &only_ineligible).is_none());
    }

    #[test]
    fn ties_between_candidates_resolve_by_id() {
        let e = [
            ev("g/flash", Some(0.2)),
            ev("b/x", Some(0.8)),
            ev("a/y", Some(0.8)),
        ];
        assert_eq!(better_model("g/flash", &e).unwrap().0, "a/y");
    }
}
