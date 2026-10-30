//! Marcadores explícitos del ADR-0023; nunca modifica el transcript.

use crate::theme::Role;

const OPEN: &str = "[[symphony:";
const CLOSE: &str = "[[/symphony:";

fn role(name: &str) -> Option<Role> {
    match name {
        "title" => Some(Role::Brand),
        "bullet" => Some(Role::Ok),
        "highlight" => Some(Role::Accent),
        "warning" => Some(Role::Warn),
        "error" => Some(Role::Error),
        "table" => Some(Role::Muted),
        _ => None,
    }
}

/// Fragmentos prestados: una región inválida se conserva completa y sin color.
pub fn parse(text: &str) -> Vec<(&str, Option<Role>)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        if start > 0 {
            out.push((&rest[..start], None));
        }
        rest = &rest[start..];
        let Some(end) = rest.find("]]") else { break };
        let name = &rest[OPEN.len()..end];
        let body = end + 2;
        let mut cursor = body;
        let mut depth = 1usize;
        let mut valid = role(name).is_some();
        let mut closed = None;
        while cursor < rest.len() {
            let next_open = rest[cursor..].find(OPEN).map(|n| cursor + n);
            let next_close = rest[cursor..].find(CLOSE).map(|n| cursor + n);
            let (pos, opening) = match (next_open, next_close) {
                (Some(a), Some(b)) => (a.min(b), a < b),
                (Some(a), None) => (a, true),
                (None, Some(b)) => (b, false),
                (None, None) => break,
            };
            let Some(end) = rest[pos..].find("]]").map(|n| pos + n) else {
                break;
            };
            if opening {
                depth += 1;
                valid = false;
            } else {
                depth -= 1;
                if depth == 0 {
                    valid &= &rest[pos + CLOSE.len()..end] == name;
                    closed = Some((pos, end + 2));
                    break;
                }
            }
            cursor = end + 2;
        }
        let Some((close, after)) = closed else { break };
        if valid {
            out.push((&rest[body..close], role(name)));
        } else {
            out.push((&rest[..after], None));
        }
        rest = &rest[after..];
    }
    if !rest.is_empty() {
        out.push((rest, None));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tipos_y_unicode() {
        for name in ["title", "bullet", "highlight", "warning", "error", "table"] {
            let text = format!("antes[[symphony:{name}]]á🎹\nfin[[/symphony:{name}]]después");
            assert_eq!(
                parse(&text),
                vec![("antes", None), ("á🎹\nfin", role(name)), ("después", None)]
            );
        }
        assert!(parse("").is_empty());
        assert_eq!(
            parse("[[symphony:title]][[/symphony:title]]"),
            vec![("", role("title"))]
        );
    }

    #[test]
    fn invalidos_literales_sin_panico() {
        for text in [
            "🎹[[",
            "[[symphony:",
            "[[/symphony:title]]",
            "**markdown**",
            "[[symphony:title]]sin cierre",
            "[[symphony:title]]x[[/symphony:error]]",
            "[[symphony:otro]]x[[/symphony:otro]]",
            "[[symphony:title]]a[[symphony:bullet]]b[[/symphony:bullet]]c[[/symphony:title]]",
            "[[symphony:title]]a[[symphony:bullet]]b[[/symphony:title]]",
        ] {
            let parsed = parse(text);
            assert!(parsed.iter().all(|(_, r)| r.is_none()), "{text}");
            assert_eq!(parsed.iter().map(|(s, _)| *s).collect::<String>(), text);
        }
    }

    #[test]
    fn recupera_despues_de_cierre_incorrecto() {
        assert_eq!(
            parse(
                "[[symphony:error]]x[[/symphony:title]][[symphony:bullet]]sí[[/symphony:bullet]]"
            ),
            vec![
                ("[[symphony:error]]x[[/symphony:title]]", None),
                ("sí", role("bullet"))
            ]
        );
    }
}
