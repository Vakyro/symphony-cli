//! Troceado de texto para buscar y recuperar solo lo necesario (`context.search`, `context.lines`).
//!
//! Determinista y total: cualquier texto produce chunks que, juntos, contienen todas sus líneas
//! (nada se pierde) y cada uno conoce su rango de líneas (1-indexado, inclusivo).

/// Un fragmento de un objeto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub seq: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub text: String,
}

/// Tamaño objetivo de un chunk (en caracteres) y máximo de líneas.
pub const TARGET_CHARS: usize = 1_500;
pub const MAX_LINES: usize = 40;

/// Parte `text` en chunks de ~`TARGET_CHARS` caracteres y hasta `MAX_LINES` líneas. Una línea
/// más larga que el objetivo se divide en trozos (cada trozo conserva su número de línea).
pub fn chunk_text(text: &str) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();
    let mut buf = String::new();
    let (mut start, mut count, mut chars) = (1usize, 0usize, 0usize);

    let flush = |out: &mut Vec<Chunk>, buf: &mut String, start: usize, count: usize| {
        if !buf.is_empty() {
            out.push(Chunk {
                seq: out.len(),
                start_line: start,
                end_line: start + count.saturating_sub(1),
                text: std::mem::take(buf),
            });
        }
    };

    for (i, line) in text.lines().enumerate() {
        let lineno = i + 1;
        // Una línea enorme se divide en pedazos que comparten el mismo número de línea.
        let pieces: Vec<&str> = if line.chars().count() > TARGET_CHARS {
            let mut v = Vec::new();
            let mut rest = line;
            while !rest.is_empty() {
                let cut = rest
                    .char_indices()
                    .nth(TARGET_CHARS)
                    .map_or(rest.len(), |(b, _)| b);
                v.push(&rest[..cut]);
                rest = &rest[cut..];
            }
            v
        } else {
            vec![line]
        };
        for piece in pieces {
            let len = piece.chars().count() + 1;
            if count > 0 && (chars + len > TARGET_CHARS || count >= MAX_LINES) {
                flush(&mut out, &mut buf, start, count);
                (count, chars) = (0, 0);
            }
            if count == 0 {
                start = lineno;
            }
            buf.push_str(piece);
            buf.push('\n');
            count += 1;
            chars += len;
        }
    }
    flush(&mut out, &mut buf, start, count);
    out
}

/// Prepara una búsqueda del usuario para FTS5: cada palabra va entre comillas (así los
/// operadores `AND`, `NOT`, `*`, `:` o `"` no se interpretan) y se buscan con `OR`, para que
/// BM25 ordene por relevancia. `None` si no queda ninguna palabra.
pub fn fts_query(user: &str) -> Option<String> {
    let terms: Vec<String> = user
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .take(24)
        .map(|t| format!("\"{t}\""))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}

/// Líneas `from..=to` (1-indexadas, inclusivas) de `text`; el rango se recorta al texto.
pub fn lines(text: &str, from: usize, to: usize) -> String {
    let from = from.max(1);
    if to < from {
        return String::new();
    }
    text.lines()
        .enumerate()
        .skip(from - 1)
        .take(to - from + 1)
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn chunks_cover_every_line_with_their_ranges() {
        let text: String = (1..=100).map(|i| format!("línea {i}\n")).collect();
        let chunks = chunk_text(&text);
        assert!(chunks.len() >= 3, "{}", chunks.len());
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks.last().unwrap().end_line, 100);
        for pair in chunks.windows(2) {
            assert_eq!(pair[1].start_line, pair[0].end_line + 1);
        }
        assert!(chunks.iter().all(|c| c.text.lines().count() <= MAX_LINES));
    }

    #[test]
    fn empty_and_tiny_texts() {
        assert!(chunk_text("").is_empty());
        let one = chunk_text("hola");
        assert_eq!(one.len(), 1);
        assert_eq!((one[0].start_line, one[0].end_line), (1, 1));
    }

    #[test]
    fn a_huge_single_line_is_split_without_losing_characters() {
        let line = "x".repeat(TARGET_CHARS * 3 + 17);
        let chunks = chunk_text(&line);
        assert!(chunks.len() >= 4);
        let joined: String = chunks
            .iter()
            .map(|c| c.text.trim_end_matches('\n'))
            .collect();
        assert_eq!(joined, line);
        assert!(chunks.iter().all(|c| c.start_line == 1 && c.end_line == 1));
    }

    #[test]
    fn fts_queries_neutralise_operators() {
        assert_eq!(
            fts_query("refresh 401").as_deref(),
            Some("\"refresh\" OR \"401\"")
        );
        assert_eq!(
            fts_query("a AND b NOT \"c\" d* e:f").as_deref(),
            Some("\"a\" OR \"AND\" OR \"b\" OR \"NOT\" OR \"c\" OR \"d\" OR \"e\" OR \"f\"")
        );
        assert_eq!(fts_query("   ***  "), None);
        assert_eq!(fts_query(""), None);
    }

    #[test]
    fn line_ranges_are_clamped() {
        let t = "a\nb\nc\nd\n";
        assert_eq!(lines(t, 2, 3), "b\nc");
        assert_eq!(lines(t, 0, 2), "a\nb");
        assert_eq!(lines(t, 3, 99), "c\nd");
        assert_eq!(lines(t, 5, 9), "");
        assert_eq!(lines(t, 3, 2), "");
    }

    proptest! {
        /// Cualquier texto: los chunks lo contienen entero, en orden, con rangos consecutivos.
        #[test]
        fn chunking_loses_nothing(text in "(\\PC|\\n){0,4000}") {
            let chunks = chunk_text(&text);
            let joined: String = chunks.iter().map(|c| c.text.as_str()).collect();
            let expected: String = text.lines().map(|l| format!("{l}\n")).collect();
            prop_assert_eq!(joined.replace('\n', ""), expected.replace('\n', ""));
            for (i, c) in chunks.iter().enumerate() {
                prop_assert_eq!(c.seq, i);
                prop_assert!(c.start_line >= 1 && c.end_line >= c.start_line);
            }
            for pair in chunks.windows(2) {
                prop_assert!(pair[1].start_line >= pair[0].end_line);
            }
        }

        /// La búsqueda saneada siempre es una expresión FTS5 válida (nunca produce sintaxis suelta).
        #[test]
        fn fts_queries_are_quoted_terms(user in "\\PC{0,80}") {
            if let Some(q) = fts_query(&user) {
                for term in q.split(" OR ") {
                    prop_assert!(term.starts_with('"') && term.ends_with('"') && term.len() > 2);
                    prop_assert!(!term[1..term.len() - 1].contains('"'));
                }
            }
        }
    }
}
