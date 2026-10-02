//! `LogCollapser`: logs largos → lo que importa. Quita secuencias ANSI y las líneas de progreso
//! (`\r`), colapsa repeticiones y recorta lo aburrido, **sin tocar nunca las líneas de error**:
//! toda línea que parezca un fallo aparece tal cual al menos una vez.

use std::sync::LazyLock;

use regex::Regex;
use symphony_core::{AnsiMode, sanitize};

/// Líneas que se conservan siempre (fallos, avisos, pilas de llamadas).
static IMPORTANT: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(error|errors|fail|failed|failure|failures|panic|panicked|exception|traceback|fatal|denied|cannot|unable|warning|warn|unhandled|segfault|abort(ed)?)\b|^\s+at\s|\bE[0-9]{4}\b|ERR!|✗|✖|FAILED",
    )
    .ok()
});

/// Números, ids hexadecimales y horas: dos líneas que solo difieren en eso son «la misma».
static VOLATILE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"[0-9a-fA-F]{7,}|\d+(?:[.:,-]\d+)*").ok());
// Las expresiones son constantes: si no compilaran, el test `patterns_compile` lo detecta.

/// Líneas a partir de las cuales se recorta lo del medio.
const KEEP_ALL_UP_TO: usize = 150;
const HEAD: usize = 25;
const TAIL: usize = 40;
const MAX_IMPORTANT: usize = 80;

pub fn is_important(line: &str) -> bool {
    IMPORTANT.as_ref().is_some_and(|r| r.is_match(line))
}

fn shape(line: &str) -> String {
    let line = line.trim();
    VOLATILE.as_ref().map_or_else(
        || line.to_string(),
        |r| r.replace_all(line, "#").into_owned(),
    )
}

/// Una línea de progreso reescrita con `\r`: solo cuenta la última parte.
fn last_segment(line: &str) -> &str {
    line.rsplit('\r')
        .find(|s| !s.trim().is_empty())
        .unwrap_or("")
}

/// Colapsa repeticiones consecutivas: idénticas (`×N`) y, solo si no son importantes, parecidas.
fn collapse_runs(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        let important = is_important(line);
        let key = shape(line);
        let mut j = i + 1;
        let mut identical = true;
        while j < lines.len() {
            let same_text = lines[j] == *line;
            let same_shape = !important && !is_important(&lines[j]) && shape(&lines[j]) == key;
            if !(same_text || same_shape) {
                break;
            }
            identical &= same_text;
            j += 1;
        }
        let run = j - i;
        out.push(line.clone());
        if run >= 3 {
            out.push(if identical {
                format!("  [… la línea anterior se repite {} veces más]", run - 1)
            } else {
                format!("  [… {} líneas parecidas omitidas]", run - 1)
            });
        } else {
            out.extend(lines[i + 1..j].iter().cloned());
        }
        i = j;
    }
    out
}

/// Recorta lo del medio: conserva el principio, el final y cada línea importante con su vecina.
fn trim_middle(lines: Vec<String>) -> Vec<String> {
    if lines.len() <= KEEP_ALL_UP_TO {
        return lines;
    }
    let n = lines.len();
    let mut keep = vec![false; n];
    keep.iter_mut().take(HEAD).for_each(|k| *k = true);
    keep.iter_mut()
        .skip(n.saturating_sub(TAIL))
        .for_each(|k| *k = true);
    let mut important = 0;
    for (i, line) in lines.iter().enumerate() {
        if is_important(line) {
            important += 1;
            if important > MAX_IMPORTANT {
                continue;
            }
            let (from, to) = (i.saturating_sub(1), (i + 2).min(n));
            keep[from..to].iter_mut().for_each(|k| *k = true);
        }
    }
    let mut out = Vec::new();
    let mut skipped = 0;
    for (line, keep) in lines.into_iter().zip(keep) {
        if keep {
            if skipped > 0 {
                out.push(format!("[… {skipped} líneas omitidas]"));
                skipped = 0;
            }
            out.push(line);
        } else {
            skipped += 1;
        }
    }
    if skipped > 0 {
        out.push(format!("[… {skipped} líneas omitidas]"));
    }
    out
}

/// Colapsa un log. Total: cualquier texto devuelve un texto.
pub fn collapse(text: &str) -> String {
    // Primero el progreso (`\r`), luego las secuencias de terminal.
    let lines = text.lines().map(|l| {
        sanitize(last_segment(l), AnsiMode::Plain)
            .trim_end()
            .to_string()
    });
    // Las líneas vacías seguidas valen una.
    let mut compact: Vec<String> = Vec::new();
    for l in lines {
        if l.is_empty() && compact.last().is_some_and(String::is_empty) {
            continue;
        }
        compact.push(l);
    }
    let collapsed = collapse_runs(&compact);
    trim_middle(collapsed).join("\n")
}
