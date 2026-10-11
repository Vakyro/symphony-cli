//! `TestSummaryCompressor`: la salida de un runner de pruebas → «896 pruebas: 895 pasaron,
//! 1 falló» y, de los fallos, su nombre y su mensaje. Entiende cargo test, vitest/jest y pytest;
//! con cualquier otra cosa devuelve `None` y el llamador usa el colapsador de logs.

use std::fmt::Write as _;
use std::sync::LazyLock;

use regex::Regex;
use symphony_core::{AnsiMode, sanitize};

use super::log::is_important;

/// Cuántos fallos se nombran.
const MAX_FAILURES: usize = 20;
/// Líneas de detalle que se conservan.
const MAX_DETAIL: usize = 30;

#[derive(Default)]
struct Counts {
    passed: u64,
    failed: u64,
    skipped: u64,
}

fn re(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

static CARGO: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored"));
static PYTEST: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"^=+ (.*\b(?:passed|failed|error|errors)\b.*) in [\d.]+s"));
static JEST: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"^\s*Tests:?\s+(.*\b(?:passed|failed)\b.*)$"));
static NUM_WORD: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"(\d+) (passed|failed|skipped|todo|errors?|xfailed|xpassed)"));

fn add_words(counts: &mut Counts, line: &str) -> bool {
    let Some(rx) = NUM_WORD.as_ref() else {
        return false;
    };
    let mut any = false;
    for c in rx.captures_iter(line) {
        let n: u64 = c[1].parse().unwrap_or(0);
        match &c[2] {
            "passed" | "xpassed" => counts.passed += n,
            "failed" | "error" | "errors" | "xfailed" => counts.failed += n,
            _ => counts.skipped += n,
        }
        any = true;
    }
    any
}

fn is_failure_name(line: &str) -> bool {
    let t = line.trim_start();
    (t.starts_with("test ") && t.ends_with("FAILED"))
        || t.starts_with("FAILED ")
        || t.starts_with("ERROR ")
        || t.starts_with("FAIL ")
        || t.starts_with("● ")
        || t.starts_with("× ")
        || t.starts_with("✗ ")
        || t.starts_with("✖ ")
}

/// Resume la salida de un runner de pruebas, o `None` si no parece una.
pub fn summarize(text: &str) -> Option<String> {
    let text = sanitize(text, AnsiMode::Plain);
    let mut counts = Counts::default();
    let mut found = false;
    let mut failures: Vec<&str> = Vec::new();
    let mut detail: Vec<&str> = Vec::new();

    for line in text.lines() {
        if let Some(c) = CARGO.as_ref().and_then(|r| r.captures(line)) {
            counts.passed += c[1].parse().unwrap_or(0);
            counts.failed += c[2].parse().unwrap_or(0);
            counts.skipped += c[3].parse().unwrap_or(0);
            found = true;
        } else if let Some(c) = PYTEST.as_ref().and_then(|r| r.captures(line)) {
            found |= add_words(&mut counts, &c[1]);
        } else if let Some(c) = JEST.as_ref().and_then(|r| r.captures(line)) {
            found |= add_words(&mut counts, &c[1]);
        } else if is_failure_name(line) {
            failures.push(line.trim());
        } else if is_important(line) && detail.len() < MAX_DETAIL {
            detail.push(line.trim());
        }
    }
    if !found {
        return None;
    }

    let total = counts.passed + counts.failed + counts.skipped;
    let mut out = format!("{total} pruebas: {} pasaron", counts.passed);
    if counts.failed > 0 {
        let verb = if counts.failed == 1 {
            "falló"
        } else {
            "fallaron"
        };
        let _ = write!(out, ", {} {verb}", counts.failed);
    }
    if counts.skipped > 0 {
        let _ = write!(out, ", {} omitidas", counts.skipped);
    }
    failures.dedup();
    for f in failures.iter().take(MAX_FAILURES) {
        let _ = write!(out, "\n  ✗ {f}");
    }
    if failures.len() > MAX_FAILURES {
        let _ = write!(out, "\n  [… {} fallos más]", failures.len() - MAX_FAILURES);
    }
    if counts.failed > 0 && !detail.is_empty() {
        out.push_str("\nDetalle:");
        for d in detail {
            let _ = write!(out, "\n  {d}");
        }
    }
    Some(out)
}
