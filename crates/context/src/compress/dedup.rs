//! `Deduplicator`: lo que aparece igual varias veces se dice una vez y después se apunta a la
//! primera. Actúa sobre bloques (separados por líneas en blanco) y sobre líneas largas repetidas.

use std::collections::HashMap;

/// Un bloque se considera repetible si tiene al menos esto de contenido.
const MIN_BLOCK_CHARS: usize = 60;

/// Reemplaza los bloques y las líneas largas repetidos por una referencia a su primera aparición.
pub fn dedup_blocks(text: &str) -> String {
    let mut seen_blocks: HashMap<String, usize> = HashMap::new();
    let mut seen_lines: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<String> = Vec::new();
    let mut block: Vec<&str> = Vec::new();
    let mut block_start = 1usize;

    let mut flush = |block: &mut Vec<&str>, start: usize, out: &mut Vec<String>| {
        if block.is_empty() {
            return;
        }
        let joined = block.join("\n");
        if joined.chars().count() >= MIN_BLOCK_CHARS && block.len() >= 2 {
            if let Some(first) = seen_blocks.get(&joined) {
                out.push(format!(
                    "[↑ bloque repetido de {} líneas: igual al de la línea {first}]",
                    block.len()
                ));
                block.clear();
                return;
            }
            seen_blocks.insert(joined, start);
        }
        for (i, line) in block.iter().enumerate() {
            let lineno = start + i;
            if line.chars().count() >= MIN_BLOCK_CHARS {
                if let Some(first) = seen_lines.get(*line) {
                    out.push(format!("[↑ igual a la línea {first}]"));
                    continue;
                }
                seen_lines.insert((*line).to_string(), lineno);
            }
            out.push((*line).to_string());
        }
        block.clear();
    };

    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            flush(&mut block, block_start, &mut out);
            out.push(String::new());
            block_start = i + 2;
        } else {
            if block.is_empty() {
                block_start = i + 1;
            }
            block.push(line);
        }
    }
    flush(&mut block, block_start, &mut out);
    out.join("\n")
}
