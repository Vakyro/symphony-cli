//! `GitDiffReducer`: un diff largo → qué archivos cambiaron y cuánto, con los cambios que
//! importan. Los lockfiles y los archivos generados se resumen en una línea; un hunk muy largo
//! conserva su principio y su final.

/// Líneas de un hunk que se conservan al principio y al final cuando es muy largo.
const HUNK_HEAD: usize = 30;
const HUNK_TAIL: usize = 10;
const HUNK_MAX: usize = HUNK_HEAD + HUNK_TAIL + 5;

fn is_generated(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(
        name,
        "package-lock.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "Cargo.lock"
            | "poetry.lock"
            | "Gemfile.lock"
            | "composer.lock"
            | "go.sum"
    ) || name.ends_with(".min.js")
        || name.ends_with(".min.css")
        || name.ends_with(".snap")
        || name.ends_with(".map")
        || path.contains("/dist/")
        || path.contains("/node_modules/")
}

#[derive(Default)]
struct FileDiff {
    path: String,
    header: Vec<String>,
    body: Vec<String>,
    added: usize,
    removed: usize,
    binary: bool,
}

fn path_of(header_line: &str) -> String {
    // `diff --git a/x b/x` → `x`
    header_line
        .strip_prefix("diff --git ")
        .and_then(|r| r.split(" b/").nth(1))
        .map_or_else(|| header_line.to_string(), str::to_string)
}

fn parse(text: &str) -> (Vec<String>, Vec<FileDiff>) {
    let mut preamble = Vec::new();
    let mut files: Vec<FileDiff> = Vec::new();
    for line in text.lines() {
        if line.starts_with("diff --git ") {
            files.push(FileDiff {
                path: path_of(line),
                header: vec![line.to_string()],
                ..FileDiff::default()
            });
            continue;
        }
        let Some(f) = files.last_mut() else {
            preamble.push(line.to_string());
            continue;
        };
        if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
            f.binary = true;
            f.header.push(line.to_string());
        } else if f.body.is_empty() && !line.starts_with("@@") {
            f.header.push(line.to_string()); // index, ---, +++, rename…
        } else {
            if line.starts_with('+') && !line.starts_with("+++") {
                f.added += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                f.removed += 1;
            }
            f.body.push(line.to_string());
        }
    }
    (preamble, files)
}

/// Líneas de contexto que se conservan junto a cada cambio (como `git diff -U1`).
const CONTEXT: usize = 1;

fn is_change(line: &str) -> bool {
    line.starts_with('+') || line.starts_with('-')
}

/// Deja `CONTEXT` líneas sin cambios alrededor de cada cambio; las demás se cuentan en un aviso
/// (una sola línea suelta se queda: el aviso no sería más corto).
fn trim_context<'a>(hunk: &[&'a String]) -> Vec<std::borrow::Cow<'a, str>> {
    let n = hunk.len();
    let keep: Vec<bool> = (0..n)
        .map(|i| {
            !hunk[i].starts_with(' ')
                || hunk[i.saturating_sub(CONTEXT)..(i + CONTEXT + 1).min(n)]
                    .iter()
                    .any(|l| is_change(l))
        })
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if keep[i] {
            out.push(hunk[i].as_str().into());
            i += 1;
            continue;
        }
        let run = keep[i..].iter().take_while(|k| !**k).count();
        if run == 1 {
            out.push(hunk[i].as_str().into());
        } else {
            out.push(format!(" [… {run} líneas sin cambios]").into());
        }
        i += run;
    }
    out
}

/// Recorta el contexto de cada hunk y cada hunk largo a su principio y su final.
fn trim_hunks(body: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut hunk: Vec<&String> = Vec::new();
    let flush = |hunk: &[&String], out: &mut Vec<String>| {
        let trimmed = trim_context(hunk);
        let hunk: Vec<&str> = trimmed.iter().map(|s| s.as_ref()).collect();
        if hunk.len() > HUNK_MAX {
            out.extend(hunk[..HUNK_HEAD].iter().map(|s| s.to_string()));
            out.push(format!(
                "[… {} líneas de este hunk omitidas]",
                hunk.len() - HUNK_HEAD - HUNK_TAIL
            ));
            out.extend(hunk[hunk.len() - HUNK_TAIL..].iter().map(|s| s.to_string()));
        } else {
            out.extend(hunk.iter().map(|s| s.to_string()));
        }
    };
    for line in body {
        if line.starts_with("@@") {
            flush(&hunk, &mut out);
            hunk.clear();
        }
        hunk.push(line);
    }
    flush(&hunk, &mut out);
    out
}

/// Reduce un diff unificado. Total: un texto que no es un diff se devuelve sin cambios.
pub fn reduce(text: &str) -> String {
    let (preamble, files) = parse(text);
    if files.is_empty() {
        return text.to_string();
    }
    let total_add: usize = files.iter().map(|f| f.added).sum();
    let total_del: usize = files.iter().map(|f| f.removed).sum();
    let mut out: Vec<String> = preamble;
    out.push(format!(
        "{} archivos cambiados, +{total_add} −{total_del}:",
        files.len()
    ));
    for f in &files {
        let what = if f.binary {
            "binario".to_string()
        } else {
            format!("+{} −{}", f.added, f.removed)
        };
        out.push(format!("  {} ({what})", f.path));
    }
    out.push(String::new());
    for f in &files {
        if is_generated(&f.path) {
            out.push(format!(
                "# {} — generado o lockfile: +{} −{} líneas, omitido",
                f.path, f.added, f.removed
            ));
            continue;
        }
        // `index` y los `---`/`+++` que repiten la ruta de `diff --git` no dicen nada nuevo.
        let (minus, plus) = (format!("--- a/{}", f.path), format!("+++ b/{}", f.path));
        out.extend(
            f.header
                .iter()
                .filter(|h| !h.starts_with("index ") && **h != minus && **h != plus)
                .cloned(),
        );
        if !f.binary {
            out.extend(trim_hunks(&f.body));
        }
    }
    out.join("\n")
}
