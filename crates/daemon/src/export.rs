//! Exportar una conversación completa a Markdown (P07.5.S7b).
//!
//! Función pura: recibe lo que ya está en la base (mensajes con hora y herramientas) y devuelve
//! el documento. Escribirlo a disco es cosa de `server::agent_export`.

use serde_json::Value;

/// Prompt de un handoff: lo lee el modelo nuevo, no es parte de la conversación.
// ponytail: se reconoce por su frase inicial (crates/context/src/handoff.rs), igual que en la TUI.
const HANDOFF_PROMPT: &str = "Retomas una tarea de código";

pub struct Meta<'a> {
    pub title: &'a str,
    pub project: &'a str,
    /// «chat general» o «agente #N».
    pub agent: &'a str,
}

/// `(año, mes, día, hora, minuto, segundo)` en UTC de unos ms desde epoch.
// ponytail: UTC, sin zona horaria local (no hay `chrono`); suficiente para nombrar y fechar.
fn civil(ms: i64) -> (i64, u32, u32, i64, i64, i64) {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Días desde 1970-01-01 → fecha (algoritmo de Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

fn stamp(ms: i64) -> String {
    let (y, mo, d, h, mi, s) = civil(ms);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
}

fn clock(ms: i64) -> String {
    let (_, _, _, h, mi, s) = civil(ms);
    format!("{h:02}:{mi:02}:{s:02}")
}

/// Nombre de archivo: `<prefijo>-AAAAMMDD-HHMMSS.md`.
pub fn file_name(prefix: &str, ms: i64) -> String {
    let (y, mo, d, h, mi, s) = civil(ms);
    format!("{prefix}-{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}.md")
}

/// El documento. `messages`: `(rol, contenido, hora)` en orden; `tools`: filas de
/// `views::activity` (lo más nuevo primero, se ignora lo que no sea `kind == "tool"`).
pub fn markdown(
    meta: &Meta,
    now_ms: i64,
    messages: &[(String, String, i64)],
    tools: &[Value],
) -> String {
    let mut timeline: Vec<(i64, String)> = Vec::new();
    for (role, content, at) in messages {
        let block = match role.as_str() {
            "USER" if content.starts_with(HANDOFF_PROMPT) => continue,
            "USER" => format!("**Tú** · {}\n\n{}\n", clock(*at), content.trim_end()),
            "ASSISTANT" => format!("**Agente** · {}\n\n{}\n", clock(*at), content.trim_end()),
            "EXECUTOR_CHANGE" => {
                content
                    .trim_end()
                    .lines()
                    .map(|l| format!("> {l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n"
            }
            other => format!("**{other}** · {}\n\n{}\n", clock(*at), content.trim_end()),
        };
        timeline.push((*at, block));
    }
    for t in tools.iter().rev().filter(|t| t["kind"] == "tool") {
        let mark = match t["status"].as_str().unwrap_or("") {
            "DONE" => "✓",
            "FAILED" | "DENIED" => "✗",
            _ => "…",
        };
        let cmd = t["command"].as_str().unwrap_or("").replace('\n', " ");
        let cmd = if cmd.is_empty() {
            String::new()
        } else {
            format!(" — `{cmd}`")
        };
        timeline.push((
            t["at"].as_i64().unwrap_or(0),
            format!("> ⚙ {mark} `{}`{cmd}\n", t["tool"].as_str().unwrap_or("?")),
        ));
    }
    // Estable: a igual hora, mensajes antes que herramientas.
    timeline.sort_by_key(|(at, _)| *at);

    let mut out = format!(
        "# {}\n\n- Proyecto: {}\n- Agente: {}\n- Exportado: {}\n\n---\n\n",
        meta.title,
        meta.project,
        meta.agent,
        stamp(now_ms)
    );
    for (_, block) in timeline {
        out.push_str(&block);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dates_are_right_across_the_epoch_and_a_leap_day() {
        assert_eq!(stamp(0), "1970-01-01 00:00:00 UTC");
        // 2024-02-29 12:34:56 UTC
        assert_eq!(stamp(1_709_210_096_000), "2024-02-29 12:34:56 UTC");
        // 2026-09-29 15:30:00 UTC
        assert_eq!(
            file_name("chat", 1_790_695_800_000),
            "chat-20260929-153000.md"
        );
    }

    #[test]
    fn export_interleaves_tools_by_time_and_hides_the_handoff_prompt() {
        let messages = vec![
            ("USER".into(), "corre los tests".into(), 1_000),
            (
                "ASSISTANT".into(),
                "Voy a correrlos.\n\nListo.".into(),
                2_000,
            ),
            (
                "EXECUTOR_CHANGE".into(),
                "── Executor changed: A → B\nReason: user switch".into(),
                5_000,
            ),
            (
                "USER".into(),
                "Retomas una tarea de código que otro…".into(),
                5_001,
            ),
            ("USER".into(), "sigue".into(), 6_000),
        ];
        // `agent.activity` trae lo más nuevo primero y mezcla checkpoints.
        let tools = vec![
            json!({ "kind": "tool", "tool": "Bash", "command": "cargo test", "status": "FAILED", "at": 3_000 }),
            json!({ "kind": "checkpoint", "seq": 1, "at": 2_500 }),
            json!({ "kind": "tool", "tool": "Read", "command": null, "status": "DONE", "at": 1_500 }),
        ];
        let md = markdown(
            &Meta {
                title: "Chat",
                project: "demo",
                agent: "chat general",
            },
            0,
            &messages,
            &tools,
        );
        let at = |s: &str| {
            md.find(s)
                .unwrap_or_else(|| panic!("falta `{s}` en:\n{md}"))
        };
        assert!(md.starts_with("# Chat\n"));
        assert!(at("corre los tests") < at("`Read`"));
        assert!(at("`Read`") < at("Voy a correrlos"));
        assert!(at("Voy a correrlos") < at("✗ `Bash` — `cargo test`"));
        assert!(at("✗ `Bash`") < at("> ── Executor changed"));
        assert!(at("> Reason: user switch") < at("sigue"));
        assert!(!md.contains("Retomas una tarea"));
        assert!(!md.contains("checkpoint"));
    }
}
