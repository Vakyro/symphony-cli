//! Handoff assembler v1 (IDEA §5.6 a, ADR-0004): el prompt de arranque de un
//! executor nuevo, armado con una plantilla fija desde el último checkpoint más
//! el git vivo del worktree (H2: git manda). Sin LLM y sin E/S: el daemon junta
//! los datos y esta función solo los ordena y recorta según el modo.

use symphony_core::ContextMode;

/// Último comando que corrió el executor anterior.
#[derive(Debug, Clone, PartialEq)]
pub struct LastCommand {
    pub command: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
}

/// Archivo nuevo (sin commit). `content: None` si es binario o no se pudo leer.
#[derive(Debug, Clone, PartialEq)]
pub struct NewFile {
    pub path: String,
    pub content: Option<String>,
}

/// Todo lo que entra al handoff, ya redactado.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HandoffInput {
    pub objective: String,
    /// Por qué hay un executor nuevo, en una frase ("el anterior se quedó sin cuota").
    pub reason: String,
    pub plan_tail: Option<String>,
    pub current_step: Option<String>,
    pub next_step: Option<String>,
    pub last_command: Option<LastCommand>,
    pub failures: Vec<String>,
    pub files_touched: Vec<String>,
    /// `git status --porcelain` del worktree.
    pub git_status: String,
    /// Diff vivo contra la base.
    pub diff: String,
    /// Dónde queda el diff completo si se recorta (`ctx://…`).
    pub diff_uri: Option<String>,
    pub new_files: Vec<NewFile>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Handoff {
    pub prompt: String,
    pub tokens_sent: i64,
}

/// Estimación determinista de tokens (~4 caracteres por token).
pub fn estimate_tokens(text: &str) -> i64 {
    i64::try_from(text.chars().count().div_ceil(4)).unwrap_or(i64::MAX)
}

/// Límites en caracteres por modo: (diff, cada archivo nuevo, archivos nuevos en total).
/// `RAW` es la salida de emergencia: nada se recorta.
fn limits(mode: ContextMode) -> Option<(usize, usize, usize)> {
    match mode {
        ContextMode::Raw => None,
        ContextMode::Safe => Some((120_000, 40_000, 160_000)),
        ContextMode::Balanced => Some((60_000, 20_000, 80_000)),
        ContextMode::Aggressive => Some((20_000, 5_000, 25_000)),
    }
}

/// Recorta a `max` caracteres y avisa cuánto se omitió y dónde está el original.
fn clip(text: &str, max: Option<usize>, original: Option<&str>) -> String {
    let n = text.chars().count();
    match max {
        Some(max) if n > max => {
            let kept: String = text.chars().take(max).collect();
            let where_ = original
                .map(|u| format!("; el original completo está en {u}"))
                .unwrap_or_default();
            format!("{kept}\n[… {} caracteres omitidos{where_}]", n - max)
        }
        _ => text.to_string(),
    }
}

fn or_none(text: &str) -> &str {
    if text.trim().is_empty() {
        "(ninguno)"
    } else {
        text.trim_end()
    }
}

fn opt(text: Option<&str>) -> &str {
    text.map_or("(ninguno)", or_none)
}

/// Arma el prompt con la plantilla fija de v1.
pub fn assemble(input: &HandoffInput, mode: ContextMode) -> Handoff {
    let (diff_max, file_max, files_total) = match limits(mode) {
        Some((d, f, t)) => (Some(d), Some(f), Some(t)),
        None => (None, None, None),
    };

    let last_command = match &input.last_command {
        None => "(ninguno)".to_string(),
        Some(c) => {
            let result = match (c.ok, c.exit_code) {
                (true, _) => "terminó bien".to_string(),
                (false, Some(code)) => format!("falló con código {code}"),
                (false, None) => "falló".to_string(),
            };
            format!("$ {}\n({result})", c.command)
        }
    };
    let bullets = |items: &[String]| {
        if items.is_empty() {
            "(ninguno)".to_string()
        } else {
            items
                .iter()
                .map(|i| format!("- {i}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };

    let mut budget = files_total;
    let new_files: Vec<String> = input
        .new_files
        .iter()
        .map(|f| match &f.content {
            None => format!(
                "--- {} (archivo nuevo, binario o ilegible: no se incluye)",
                f.path
            ),
            Some(body) => {
                let room = match (file_max, budget) {
                    (Some(each), Some(left)) => Some(each.min(left)),
                    _ => None,
                };
                let shown = clip(body, room, None);
                if let (Some(left), Some(room)) = (budget.as_mut(), room) {
                    *left -= room.min(body.chars().count());
                }
                format!("--- {} (archivo nuevo)\n{}", f.path, shown.trim_end())
            }
        })
        .collect();

    let prompt = format!(
        "Retomas una tarea de código que otro executor dejó a medias. {reason}
Ya estás en su worktree, con su rama y sus cambios. No empieces de cero: revisa lo que ya existe y continúa desde donde quedó.

## Objetivo
{objective}

## Último plan del executor anterior
{plan}

## En qué estaba
{current}

## Qué seguía
{next}

## Último comando y su resultado
{last_command}

## Fallos recientes
{failures}

## Archivos tocados
{files}

## Estado de git
{status}

## Diff contra la base
{diff}

## Archivos nuevos (sin commit)
{new_files}

Continúa hasta terminar el objetivo. Si algo del estado de arriba contradice lo que ves en el worktree, manda el worktree.
",
        reason = input.reason.trim(),
        objective = input.objective.trim(),
        plan = opt(input.plan_tail.as_deref()),
        current = opt(input.current_step.as_deref()),
        next = opt(input.next_step.as_deref()),
        failures = bullets(&input.failures),
        files = bullets(&input.files_touched),
        status = if input.git_status.trim().is_empty() {
            "(limpio)"
        } else {
            input.git_status.trim_end()
        },
        diff = or_none(&clip(&input.diff, diff_max, input.diff_uri.as_deref())),
        new_files = if new_files.is_empty() {
            "(ninguno)".to_string()
        } else {
            new_files.join("\n\n")
        },
    );
    Handoff {
        tokens_sent: estimate_tokens(&prompt),
        prompt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed() -> HandoffInput {
        HandoffInput {
            objective: "Arreglar la rotación de refresh tokens y agregar tests de regresión".into(),
            reason: "El executor anterior (anthropic/claude-sonnet) se quedó sin cuota.".into(),
            plan_tail: Some(
                "Ya cambié `rotate()` para invalidar el token viejo.\n- [x] rotate\n- [ ] test de regresión para el 401"
                    .into(),
            ),
            current_step: Some("Bash: npm test -- auth".into()),
            next_step: Some("test de regresión para el 401".into()),
            last_command: Some(LastCommand {
                command: "npm test -- auth".into(),
                ok: false,
                exit_code: Some(1),
            }),
            failures: vec!["`npm test -- auth` falló (código 1)".into()],
            files_touched: vec!["src/auth.ts".into(), "test/auth.test.ts".into()],
            git_status: " M src/auth.ts\n?? test/auth.test.ts\n".into(),
            diff: "diff --git a/src/auth.ts b/src/auth.ts\n--- a/src/auth.ts\n+++ b/src/auth.ts\n@@ -1,3 +1,4 @@\n export function rotate(t) {\n+  revoke(t);\n   return issue();\n }\n".into(),
            diff_uri: Some("ctx://diff/AGENT/abc".into()),
            new_files: vec![
                NewFile {
                    path: "test/auth.test.ts".into(),
                    content: Some("test('401 tras rotar', () => {});\n".into()),
                },
                NewFile {
                    path: "fixtures/logo.png".into(),
                    content: None,
                },
            ],
        }
    }

    #[test]
    fn prompt_for_a_fixed_checkpoint() {
        let h = assemble(&fixed(), ContextMode::Balanced);
        insta::assert_snapshot!(h.prompt);
        assert_eq!(h.tokens_sent, estimate_tokens(&h.prompt));
    }

    #[test]
    fn first_checkpoint_without_progress_says_so() {
        let input = HandoffInput {
            objective: "Hacer X".into(),
            reason: "El executor anterior terminó de forma inesperada.".into(),
            ..HandoffInput::default()
        };
        insta::assert_snapshot!(assemble(&input, ContextMode::Balanced).prompt);
    }

    #[test]
    fn modes_clip_the_diff_except_raw() {
        let mut input = fixed();
        input.diff = "x".repeat(70_000);
        let balanced = assemble(&input, ContextMode::Balanced).prompt;
        assert!(balanced.contains(
            "[… 10000 caracteres omitidos; el original completo está en ctx://diff/AGENT/abc]"
        ));
        let aggressive = assemble(&input, ContextMode::Aggressive);
        assert!(aggressive.prompt.contains("[… 50000 caracteres omitidos"));
        let raw = assemble(&input, ContextMode::Raw);
        assert!(raw.prompt.contains(&"x".repeat(70_000)));
        assert!(!raw.prompt.contains("omitidos"));
        assert!(aggressive.tokens_sent < raw.tokens_sent);
    }

    #[test]
    fn new_files_share_a_total_budget() {
        let mut input = fixed();
        input.new_files = (0..10)
            .map(|i| NewFile {
                path: format!("f{i}.txt"),
                content: Some("y".repeat(10_000)),
            })
            .collect();
        // AGGRESSIVE: 5k por archivo, 25k en total → 5 archivos completos de 5k y el resto vacío.
        let p = assemble(&input, ContextMode::Aggressive).prompt;
        assert_eq!(p.matches("[… 5000 caracteres omitidos]").count(), 5, "{p}");
        assert_eq!(p.matches("[… 10000 caracteres omitidos]").count(), 5);
    }

    #[test]
    fn tokens_are_estimated_by_characters() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("ñandú"), 2);
    }
}
