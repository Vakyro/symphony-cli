//! `JsonStructuralCompressor`: un JSON grande → su forma. Esquema, conteos, rangos de números,
//! unos pocos ejemplos y los campos que no están en todos los elementos (las anomalías suelen
//! ser lo interesante). Los textos largos se recortan.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::{Map, Value};

const MAX_DEPTH: usize = 4;
const MAX_KEYS: usize = 40;
const SAMPLES: usize = 2;
const MAX_STR: usize = 60;

fn clip(s: &str) -> String {
    let n = s.chars().count();
    if n <= MAX_STR {
        return s.to_string();
    }
    let head: String = s.chars().take(MAX_STR).collect();
    format!("{head}…({n} caracteres)")
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "número",
        Value::String(_) => "texto",
        Value::Array(_) => "lista",
        Value::Object(_) => "objeto",
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => format!("{:?}", clip(s)),
        other => other.to_string(),
    }
}

fn describe(v: &Value, depth: usize, indent: usize, out: &mut Vec<String>, label: &str) {
    let pad = " ".repeat(indent);
    match v {
        Value::Object(map) => describe_object(map, depth, indent, out, label),
        Value::Array(items) => describe_array(items, depth, indent, out, label),
        other => out.push(format!("{pad}{label}{}", scalar(other))),
    }
}

fn describe_object(
    map: &Map<String, Value>,
    depth: usize,
    indent: usize,
    out: &mut Vec<String>,
    label: &str,
) {
    let pad = " ".repeat(indent);
    out.push(format!("{pad}{label}objeto ({} claves)", map.len()));
    if depth >= MAX_DEPTH {
        out.push(format!("{pad}  […]"));
        return;
    }
    for (k, child) in map.iter().take(MAX_KEYS) {
        describe(child, depth + 1, indent + 2, out, &format!("{k}: "));
    }
    if map.len() > MAX_KEYS {
        out.push(format!("{pad}  [… {} claves más]", map.len() - MAX_KEYS));
    }
}

fn describe_array(
    items: &[Value],
    depth: usize,
    indent: usize,
    out: &mut Vec<String>,
    label: &str,
) {
    let pad = " ".repeat(indent);
    out.push(format!("{pad}{label}lista de {}", items.len()));
    if items.is_empty() {
        return;
    }
    if depth >= MAX_DEPTH {
        out.push(format!("{pad}  […]"));
        return;
    }
    let objects: Vec<&Map<String, Value>> = items.iter().filter_map(Value::as_object).collect();
    if objects.len() == items.len() {
        // Presencia de cada clave, tipos vistos y rango de los números.
        let mut seen: BTreeMap<&str, (usize, Vec<&'static str>)> = BTreeMap::new();
        let mut ranges: BTreeMap<&str, (f64, f64)> = BTreeMap::new();
        for o in &objects {
            for (k, v) in *o {
                let e = seen.entry(k).or_default();
                e.0 += 1;
                if !e.1.contains(&kind(v)) {
                    e.1.push(kind(v));
                }
                if let Some(n) = v.as_f64() {
                    let r = ranges.entry(k).or_insert((n, n));
                    r.0 = r.0.min(n);
                    r.1 = r.1.max(n);
                }
            }
        }
        out.push(format!("{pad}  elementos: objetos con"));
        for (k, (count, kinds)) in seen.iter().take(MAX_KEYS) {
            let mut line = format!("{pad}    {k}: {}", kinds.join("|"));
            if let Some((lo, hi)) = ranges.get(k) {
                let _ = write!(line, " [{lo} … {hi}]");
            }
            if *count < objects.len() {
                let _ = write!(line, "  ← solo en {count} de {}", objects.len());
            }
            out.push(line);
        }
        for (i, o) in objects.iter().take(SAMPLES).enumerate() {
            out.push(format!("{pad}  ejemplo {}:", i + 1));
            describe_object(o, depth + 1, indent + 4, out, "");
        }
    } else {
        let mut kinds: Vec<&str> = items.iter().map(kind).collect();
        kinds.sort_unstable();
        kinds.dedup();
        out.push(format!("{pad}  elementos: {}", kinds.join("|")));
        for item in items.iter().take(SAMPLES) {
            describe(item, depth + 1, indent + 2, out, "- ");
        }
    }
}

/// Describe la estructura de un JSON. `None` si el texto no es JSON.
pub fn summarize(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    let mut out = Vec::new();
    describe(&v, 0, 0, &mut out, "");
    Some(out.join("\n"))
}
