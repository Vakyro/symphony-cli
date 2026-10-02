//! Handoff assembler v2 (IDEA §5.6 a, ADR-0004, P09.S6): el prompt de arranque de un
//! executor nuevo, armado con una plantilla fija desde el último checkpoint más
//! el git vivo del worktree (H2: git manda). Sin LLM y sin E/S: el daemon junta
//! los datos y esta función solo los ordena, comprime y recorta según el modo.
//!
//! Además del prompt devuelve **qué entró y con qué fidelidad** ([`Item`]): la tabla
//! `handoff_items` y `symphony context inspect` salen de ahí.

use symphony_core::ContextMode;

use crate::compress::{Compressor, Hint, compress};

/// Qué tan fiel al original es una parte del prompt (`handoff_items.fidelity`, 0–5).
pub mod fidelity {
    /// No entró: solo se dice que existe.
    pub const OMITTED: u8 = 0;
    /// Solo una referencia `ctx://` al original.
    pub const REFERENCE: u8 = 1;
    /// Esqueleto estructural (AST, P09.S4).
    pub const SKELETON: u8 = 2;
    /// Resumen determinista (compresor); el original sigue en su `ctx://`.
    pub const COMPRESSED: u8 = 3;
    /// El original recortado por tamaño.
    pub const CLIPPED: u8 = 4;
    /// El original completo.
    pub const FULL: u8 = 5;
}

/// Sección del prompt (`handoff_items.section`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Objective,
    Plan,
    Decisions,
    Failures,
    Code,
    Diff,
    References,
}

impl Section {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Objective => "OBJECTIVE",
            Self::Plan => "PLAN",
            Self::Decisions => "DECISIONS",
            Self::Failures => "FAILURES",
            Self::Code => "CODE",
            Self::Diff => "DIFF",
            Self::References => "REFERENCES",
        }
    }
}

/// Una parte del prompt y cómo quedó.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub section: Section,
    /// Archivo o `ctx://` al que se refiere, si aplica.
    pub path: Option<String>,
    pub fidelity: u8,
    pub tokens: i64,
}

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

/// Quién dijo un mensaje de la conversación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    User,
    Assistant,
}

/// Un mensaje de texto de la conversación (sin herramientas ni separadores).
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub speaker: Speaker,
    pub text: String,
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
    /// Lo dicho hasta ahora, en orden cronológico. Vacío = sin sección de conversación.
    pub conversation: Vec<ChatMessage>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Handoff {
    pub prompt: String,
    pub tokens_sent: i64,
    pub items: Vec<Item>,
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

/// Límites de la conversación por modo: (caracteres en total, caracteres por mensaje).
/// Se conserva lo más reciente; `RAW` no omite nada.
fn conversation_limits(mode: ContextMode) -> Option<(usize, usize)> {
    match mode {
        ContextMode::Raw => None,
        ContextMode::Safe => Some((120_000, 12_000)),
        ContextMode::Balanced => Some((40_000, 4_000)),
        ContextMode::Aggressive => Some((10_000, 1_500)),
    }
}

/// La conversación como texto: los mensajes más recientes que caben en el presupuesto
/// (siempre al menos el último), con aviso de cuántos anteriores se omitieron.
fn render_conversation(messages: &[ChatMessage], mode: ContextMode) -> (String, u8) {
    let (total, each) = match conversation_limits(mode) {
        Some((t, e)) => (Some(t), Some(e)),
        None => (None, None),
    };
    let mut kept: Vec<String> = Vec::new();
    let mut used = 0usize;
    let mut clipped = false;
    for m in messages.iter().rev() {
        let who = match m.speaker {
            Speaker::User => "Usuario",
            Speaker::Assistant => "Asistente",
        };
        clipped |= is_clipped(m.text.trim(), each);
        let line = format!("**{who}:** {}", clip(m.text.trim(), each, None));
        let len = line.chars().count();
        if total.is_some_and(|t| !kept.is_empty() && used + len > t) {
            break;
        }
        used += len;
        kept.push(line);
    }
    let omitted = messages.len() - kept.len();
    kept.reverse();
    let mut out = String::new();
    if omitted > 0 {
        out.push_str(&format!("[… {omitted} mensajes anteriores omitidos]\n\n"));
    }
    out.push_str(&kept.join("\n\n"));
    let f = if omitted > 0 || clipped {
        fidelity::CLIPPED
    } else {
        fidelity::FULL
    };
    (out, f)
}

fn is_clipped(text: &str, max: Option<usize>) -> bool {
    max.is_some_and(|m| text.chars().count() > m)
}

/// El diff para el prompt: en `BALANCED`/`AGGRESSIVE` primero se reduce (archivos, +/−, lockfiles
/// fuera, hunks largos recortados) si eso es más corto que el original con su aviso; después se
/// recorta al límite del modo.
fn render_diff(diff: &str, max: Option<usize>, uri: Option<&str>, reduce: bool) -> (String, u8) {
    let mut text = diff.to_string();
    let mut f = fidelity::FULL;
    if reduce {
        let c = compress(diff, Hint::Diff);
        if c.compressor == Compressor::GitDiff {
            let note = match uri {
                Some(u) => {
                    format!("[resumen determinista del diff; el original completo está en {u}]")
                }
                None => "[resumen determinista del diff]".to_string(),
            };
            let candidate = format!("{}\n{note}", c.text);
            if candidate.len() < diff.len() {
                text = candidate;
                f = fidelity::COMPRESSED;
            }
        }
    }
    if is_clipped(&text, max) {
        f = f.min(fidelity::CLIPPED);
    }
    (clip(&text, max, uri), f)
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

    let mut items: Vec<Item> = Vec::new();
    let mut budget = files_total;
    let new_files: Vec<String> = input
        .new_files
        .iter()
        .map(|f| {
            let (text, fid) = match &f.content {
                None => (
                    format!(
                        "--- {} (archivo nuevo, binario o ilegible: no se incluye)",
                        f.path
                    ),
                    fidelity::OMITTED,
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
                    let fid = if is_clipped(body, room) {
                        fidelity::CLIPPED
                    } else {
                        fidelity::FULL
                    };
                    (
                        format!("--- {} (archivo nuevo)\n{}", f.path, shown.trim_end()),
                        fid,
                    )
                }
            };
            items.push(Item {
                section: Section::Code,
                path: Some(f.path.clone()),
                fidelity: fid,
                tokens: estimate_tokens(&text),
            });
            text
        })
        .collect();

    let conversation = if input.conversation.is_empty() {
        String::new()
    } else {
        let (rendered, fid) = render_conversation(&input.conversation, mode);
        items.push(Item {
            section: Section::Decisions,
            path: None,
            fidelity: fid,
            tokens: estimate_tokens(&rendered),
        });
        format!(
            "## Conversación hasta ahora (lo más reciente al final)
{}
El último mensaje del usuario puede estar sin responder o a medias: revisa el worktree y continúa desde ahí.

",
            rendered
        )
    };

    let (diff, diff_fidelity) = render_diff(
        &input.diff,
        diff_max,
        input.diff_uri.as_deref(),
        !matches!(mode, ContextMode::Raw | ContextMode::Safe),
    );

    let prompt = format!(
        "Retomas una tarea de código que otro executor dejó a medias. {reason}
Ya estás en su worktree, con su rama y sus cambios. No empieces de cero: revisa lo que ya existe y continúa desde donde quedó.

## Objetivo
{objective}

{conversation}## Último plan del executor anterior
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
        conversation = conversation,
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
        diff = or_none(&diff),
        new_files = if new_files.is_empty() {
            "(ninguno)".to_string()
        } else {
            new_files.join("\n\n")
        },
    );
    let tokens = |text: &str| estimate_tokens(text);
    let mut head = vec![
        Item {
            section: Section::Objective,
            path: None,
            fidelity: fidelity::FULL,
            tokens: tokens(&input.objective),
        },
        Item {
            section: Section::Plan,
            path: None,
            fidelity: fidelity::FULL,
            tokens: tokens(&format!(
                "{}{}{}",
                opt(input.plan_tail.as_deref()),
                opt(input.current_step.as_deref()),
                opt(input.next_step.as_deref())
            )),
        },
        Item {
            section: Section::Failures,
            path: None,
            fidelity: fidelity::FULL,
            tokens: tokens(&format!("{last_command}{}", bullets(&input.failures))),
        },
        Item {
            section: Section::Diff,
            path: None,
            fidelity: diff_fidelity,
            tokens: tokens(&diff),
        },
    ];
    if diff_fidelity < fidelity::FULL
        && let Some(uri) = &input.diff_uri
    {
        head.push(Item {
            section: Section::References,
            path: Some(uri.clone()),
            fidelity: fidelity::REFERENCE,
            tokens: tokens(uri),
        });
    }
    head.append(&mut items);
    Handoff {
        tokens_sent: estimate_tokens(&prompt),
        prompt,
        items: head,
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
            conversation: Vec::new(),
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

    fn chat(n: usize, text_len: usize) -> Vec<ChatMessage> {
        (0..n)
            .map(|i| ChatMessage {
                speaker: if i % 2 == 0 {
                    Speaker::User
                } else {
                    Speaker::Assistant
                },
                text: format!("m{i} {}", "z".repeat(text_len)),
            })
            .collect()
    }

    #[test]
    fn conversation_section_only_appears_with_messages() {
        let p = assemble(&fixed(), ContextMode::Balanced).prompt;
        assert!(!p.contains("Conversación hasta ahora"));
        let mut input = fixed();
        input.conversation = chat(2, 5);
        let p = assemble(&input, ContextMode::Balanced).prompt;
        assert!(p.contains("## Conversación hasta ahora"));
        assert!(p.contains("**Usuario:** m0 zzzzz"));
        assert!(p.contains("**Asistente:** m1 zzzzz"));
        assert!(!p.contains("anteriores omitidos"));
    }

    #[test]
    fn conversation_keeps_the_most_recent_messages_within_the_budget() {
        let mut input = fixed();
        // 30 mensajes de 1.000 caracteres: 30.000 en total.
        input.conversation = chat(30, 1_000);
        let aggressive = assemble(&input, ContextMode::Aggressive).prompt;
        // AGGRESSIVE: 10.000 caracteres → los últimos ~9 mensajes; el más reciente siempre está.
        assert!(aggressive.contains("m29 "), "{aggressive}");
        assert!(!aggressive.contains("m0 "), "{aggressive}");
        assert!(aggressive.contains("mensajes anteriores omitidos]"));
        let balanced = assemble(&input, ContextMode::Balanced).prompt;
        assert!(
            balanced.contains("m0 "),
            "30.000 caben en BALANCED (40.000)"
        );
        assert!(!balanced.contains("anteriores omitidos"));
    }

    #[test]
    fn a_huge_last_message_is_clipped_but_never_dropped() {
        let mut input = fixed();
        input.conversation = chat(3, 50_000);
        let p = assemble(&input, ContextMode::Aggressive).prompt;
        assert!(p.contains("m2 "), "el último mensaje siempre entra");
        assert!(p.contains("caracteres omitidos]"), "y se recorta a 1.500");
    }

    #[test]
    fn conversation_tokens_never_grow_from_raw_to_aggressive() {
        let mut input = fixed();
        input.conversation = chat(80, 2_000);
        let t = |m| assemble(&input, m).tokens_sent;
        let (raw, safe, balanced, aggressive) = (
            t(ContextMode::Raw),
            t(ContextMode::Safe),
            t(ContextMode::Balanced),
            t(ContextMode::Aggressive),
        );
        assert!(raw >= safe && safe >= balanced && balanced >= aggressive);
        assert!(raw > aggressive);
        let raw_prompt = assemble(&input, ContextMode::Raw).prompt;
        for i in 0..80 {
            assert!(raw_prompt.contains(&format!("m{i} ")), "RAW omitió m{i}");
        }
        assert!(!raw_prompt.contains("omitidos"));
    }

    /// Medición de P07.5.S9: `cargo nextest run -p symphony-context --run-ignored only --no-capture`.
    #[test]
    #[ignore = "imprime la tabla de coste del handoff; no verifica nada nuevo"]
    fn handoff_cost_table() {
        println!(
            "mensajes | raw | safe | balanced | aggressive  (tokens que lee el proveedor nuevo)"
        );
        for n in [10, 40, 100, 200] {
            let mut input = fixed();
            input.conversation = chat(n, 800);
            let t = |m| assemble(&input, m).tokens_sent;
            println!(
                "{n:>8} | {} | {} | {} | {}",
                t(ContextMode::Raw),
                t(ContextMode::Safe),
                t(ContextMode::Balanced),
                t(ContextMode::Aggressive)
            );
        }
    }

    /// Un diff realista: un cambio de verdad, un lockfile enorme y un archivo generado largo.
    fn big_diff() -> String {
        let mut d = String::from(
            "diff --git a/src/auth.ts b/src/auth.ts\n--- a/src/auth.ts\n+++ b/src/auth.ts\n@@ -1,3 +1,4 @@\n export function rotate(t) {\n+  revoke(t);\n   return issue();\n }\n",
        );
        d.push_str("diff --git a/package-lock.json b/package-lock.json\n--- a/package-lock.json\n+++ b/package-lock.json\n@@ -1,1 +1,3000 @@\n");
        for i in 0..3000 {
            d.push_str(&format!("+    \"dep{i}\": \"1.0.{i}\",\n"));
        }
        d
    }

    #[test]
    fn balanced_reduces_the_diff_and_points_to_the_original() {
        let mut input = fixed();
        input.diff = big_diff();
        let safe = assemble(&input, ContextMode::Safe);
        let balanced = assemble(&input, ContextMode::Balanced);
        assert!(
            balanced.prompt.contains("+  revoke(t);"),
            "el cambio real sigue"
        );
        assert!(
            balanced
                .prompt
                .contains("package-lock.json — generado o lockfile")
        );
        assert!(!balanced.prompt.contains("dep2999"));
        assert!(balanced.prompt.contains(
            "[resumen determinista del diff; el original completo está en ctx://diff/AGENT/abc]"
        ));
        assert!(balanced.tokens_sent * 3 < safe.tokens_sent);
        let diff_item = |h: &Handoff| {
            h.items
                .iter()
                .find(|i| i.section == Section::Diff)
                .unwrap()
                .fidelity
        };
        assert_eq!(diff_item(&balanced), fidelity::COMPRESSED);
        assert_eq!(diff_item(&safe), fidelity::FULL);
        assert!(
            balanced
                .items
                .iter()
                .any(|i| i.section == Section::References
                    && i.path.as_deref() == Some("ctx://diff/AGENT/abc"))
        );
        assert!(!safe.items.iter().any(|i| i.section == Section::References));
    }

    #[test]
    fn raw_omits_nothing_and_modes_are_monotonic_with_a_realistic_checkpoint() {
        let mut input = fixed();
        input.diff = big_diff();
        input.conversation = chat(60, 1_500);
        input.new_files = (0..6)
            .map(|i| NewFile {
                path: format!("src/n{i}.ts"),
                content: Some(format!("export const n{i} = {};\n", "1".repeat(30_000))),
            })
            .collect();
        let t = |m| assemble(&input, m);
        let (raw, safe, balanced, aggressive) = (
            t(ContextMode::Raw),
            t(ContextMode::Safe),
            t(ContextMode::Balanced),
            t(ContextMode::Aggressive),
        );
        assert!(raw.tokens_sent >= safe.tokens_sent);
        assert!(safe.tokens_sent >= balanced.tokens_sent);
        assert!(balanced.tokens_sent >= aggressive.tokens_sent);
        assert!(raw.tokens_sent > aggressive.tokens_sent);
        // RAW: todo entra completo, nada se resume ni se recorta.
        assert!(raw.prompt.contains("dep2999"));
        assert!(!raw.prompt.contains("omitidos") && !raw.prompt.contains("resumen determinista"));
        assert!(
            raw.items.iter().all(|i| i.fidelity == fidelity::FULL),
            "{:?}",
            raw.items
        );
        // Los demás nunca dicen más fidelidad que RAW y siempre listan las mismas secciones.
        for h in [&safe, &balanced, &aggressive] {
            assert!(h.items.iter().all(|i| i.fidelity <= fidelity::FULL));
            assert!(h.items.iter().any(|i| i.fidelity < fidelity::FULL));
        }
        // La suma de los items no pasa lo enviado (la plantilla fija es lo que falta).
        for h in [&raw, &safe, &balanced, &aggressive] {
            assert!(h.items.iter().map(|i| i.tokens).sum::<i64>() <= h.tokens_sent);
        }
    }

    #[test]
    fn items_describe_the_sections_of_a_small_checkpoint() {
        let h = assemble(&fixed(), ContextMode::Balanced);
        let sections: Vec<&str> = h.items.iter().map(|i| i.section.as_str()).collect();
        assert_eq!(
            sections,
            ["OBJECTIVE", "PLAN", "FAILURES", "DIFF", "CODE", "CODE"]
        );
        let png = h.items.last().unwrap();
        assert_eq!(png.path.as_deref(), Some("fixtures/logo.png"));
        assert_eq!(png.fidelity, fidelity::OMITTED);
        assert!(h.items[..5].iter().all(|i| i.fidelity == fidelity::FULL));
    }

    #[test]
    fn tokens_are_estimated_by_characters() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("ñandú"), 2);
    }
}
