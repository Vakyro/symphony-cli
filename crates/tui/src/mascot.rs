//! Mascota del chat (P17.S4): un mini piano de una octava cuyas teclas se presionan solas
//! mientras el agente trabaja, con frases con personalidad. Solo se dibuja en el chat
//! principal y avanza con el tick de 1 s que ya existe (sin timer nuevo).
//!
//! Se apaga con la variable de entorno `SYMPHONY_NO_ANIM`, con `NO_COLOR`, con una
//! terminal sin animación (`TERM=dumb`) o con `[ui] animations = false` en la config
//! (`~/.symphony/config.toml`, pisible por `.symphony/project.toml`; la TUI lo pregunta
//! al daemon con `config.ui`).

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;

/// Frases rotativas con personalidad (mientras «piensa»).
pub const PHRASES: [&str; 8] = [
    "afinando ideas…",
    "ensayando el compás…",
    "buscando la melodía…",
    "componiendo la respuesta…",
    "puliendo cada nota…",
    "leyendo la partitura del repo…",
    "dando tempo al razonamiento…",
    "acordando los detalles…",
];

/// Nombres de las teclas blancas (una octava).
const WHITE_NOTES: [&str; 7] = ["Do", "Re", "Mi", "Fa", "Sol", "La", "Si"];

/// Tras cada tecla blanca hay tecla negra, salvo tras Mi (índice 2) y Si (índice 6).
const HAS_BLACK: [bool; 7] = [true, true, false, true, true, true, false];

const KEY_W: usize = 4; // ancho de una tecla blanca: `│   `

/// ¿El entorno permite animar? Pura, para poder probarla sin tocar el proceso.
/// `no_anim`: `SYMPHONY_NO_ANIM` definida y no vacía. `no_color`: `NO_COLOR` idem.
/// `dumb_term`: `TERM=dumb` (terminal sin animación).
pub fn env_allows(no_anim: bool, no_color: bool, dumb_term: bool) -> bool {
    !(no_anim || no_color || dumb_term)
}

/// ¿Se dibuja la mascota? Config (`[ui] animations`) y entorno, las dos a la vez.
pub fn allowed(config_animations: bool, env_ok: bool) -> bool {
    config_animations && env_ok
}

/// Lee el entorno (una sola vez, al arrancar la app).
pub fn env_ok() -> bool {
    let set = |var: &str| std::env::var_os(var).is_some_and(|v| !v.is_empty());
    env_allows(
        set("SYMPHONY_NO_ANIM"),
        set("NO_COLOR"),
        std::env::var_os("TERM").is_some_and(|t| t == "dumb"),
    )
}

/// Qué teclas suenan en este tick: una blanca siempre y, la mitad de los ticks, una negra
/// (la negra número 0–4 entre las blancas que tienen). Determinista: mismo tick → mismas
/// teclas, para que los snapshots sean estables.
fn pressed(tick: u64) -> (usize, Option<usize>) {
    let h = tick.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let white = ((h >> 8) % WHITE_NOTES.len() as u64) as usize;
    let black = if h & 0x1000 == 0 {
        Some(((h >> 24) % 5) as usize)
    } else {
        None
    };
    (white, black)
}

/// La frase de este tick: rota por las de [`PHRASES`].
pub fn phrase(tick: u64) -> &'static str {
    PHRASES[(tick as usize) % PHRASES.len()]
}

fn brand_style(theme: &Theme) -> Style {
    theme.brand.map_or_else(
        || Style::default().add_modifier(Modifier::BOLD),
        |c| Style::default().fg(c).add_modifier(Modifier::BOLD),
    )
}

/// Las líneas del piano para el `tick` dado: fila de negras, fila de blancas con la frase
/// a la derecha y el borde inferior. Las teclas pulsadas van en color de marca.
pub fn piano_lines(tick: u64, theme: &Theme) -> Vec<Line<'static>> {
    let (white, black) = pressed(tick);
    let mut lines = Vec::with_capacity(3);

    // Negras: dos columnas centradas en el borde entre dos blancas (x = k * KEY_W + 2),
    // en el orden en que aparecen. La que suena se pinta; el resto queda en gris.
    let black_pos: Vec<usize> = HAS_BLACK
        .iter()
        .enumerate()
        .filter(|(_, has)| **has)
        .map(|(k, _)| k * KEY_W + 2)
        .collect();
    let mut blacks: Vec<Span<'static>> = Vec::with_capacity(WHITE_NOTES.len() * KEY_W + 1);
    for i in 0..=WHITE_NOTES.len() * KEY_W {
        let key = black_pos.iter().position(|p| *p == i || *p + 1 == i);
        match key {
            Some(k) => {
                let style = if black == Some(k) {
                    brand_style(theme)
                } else {
                    Style::default()
                };
                blacks.push(Span::styled("█", style));
            }
            None => blacks.push(Span::styled(" ", Style::default())),
        }
    }
    lines.push(Line::from(blacks));

    // Blancas: `│   ` por tecla; la pulsada se pinta y la frase va a su derecha.
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(WHITE_NOTES.len() * 2 + 4);
    for i in 0..WHITE_NOTES.len() {
        spans.push(Span::styled("│", Style::default()));
        if i == white {
            spans.push(Span::styled(" ▓ ", brand_style(theme)));
        } else {
            spans.push(Span::styled("   ", Style::default()));
        }
    }
    spans.push(Span::styled("│", Style::default()));
    spans.push(Span::styled(
        format!(" ♪ {} · {}", WHITE_NOTES[white], phrase(tick)),
        Style::default(),
    ));
    lines.push(Line::from(spans));

    let bottom: String = std::iter::once('└')
        .chain((0..WHITE_NOTES.len()).flat_map(|_| "───┴".chars()))
        .chain(std::iter::once('┘'))
        .collect();
    lines.push(Line::from(Span::styled(bottom, Style::default())));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_allows_blocks_each_kill_switch() {
        assert!(env_allows(false, false, false));
        assert!(!env_allows(true, false, false));
        assert!(!env_allows(false, true, false));
        assert!(!env_allows(false, false, true));
    }

    #[test]
    fn allowed_needs_config_and_env() {
        assert!(allowed(true, true));
        assert!(!allowed(false, true));
        assert!(!allowed(true, false));
        assert!(!allowed(false, false));
    }

    #[test]
    fn piano_has_an_octave_and_a_phrase() {
        let theme = Theme::default();
        let lines = piano_lines(0, &theme);
        assert_eq!(lines.len(), 3);
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        // 7 blancas: 8 bordes verticales; 5 negras de 2 bloques: 10.
        assert_eq!(text.matches('│').count(), 8);
        assert_eq!(text.matches('█').count(), 10);
        assert!(text.contains(phrase(0)), "{text}");
        assert!(text.contains('└') && text.contains('┘'), "{text}");
    }

    #[test]
    fn piano_is_deterministic_per_tick() {
        let theme = Theme::default();
        let render = |t: u64| {
            piano_lines(t, &theme)
                .iter()
                .map(|l| l.to_string())
                .collect::<String>()
        };
        assert_eq!(render(42), render(42));
    }

    #[test]
    fn pressed_key_and_phrase_rotate() {
        // En algún tick cambia la tecla pulsada y en alguno suena una negra.
        let keys: std::collections::HashSet<_> = (0..64).map(|t| pressed(t).0).collect();
        assert!(keys.len() > 1, "la tecla blanca debería rotar");
        assert!((0..64).any(|t| pressed(t).1.is_some()));
        // La frase rota entre al menos dos distintas.
        let phrases: std::collections::HashSet<_> = (0..16).map(phrase).collect();
        assert!(phrases.len() > 1);
    }
}
