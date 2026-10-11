//! Validación de una tarea (P08.S6, STACK §23): qué es cada comando de un tier y cómo se
//! resume su salida. Texto puro: ejecutar los comandos es del daemon.

use symphony_core::CheckName;

/// Qué comprueba un comando de validación, por su texto; si no se reconoce, por el tier
/// (1 = lint, 2 = pruebas dirigidas, 3 = suite completa).
pub fn check_name(command: &str, tier: u8) -> CheckName {
    let c = command.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| c.contains(w));
    if has(&["audit", "deny", "snyk", "semgrep", "gitleaks"]) {
        CheckName::Security
    } else if has(&["build", "compile", "bundle"]) {
        CheckName::Build
    } else if has(&["typecheck", "tsc", "mypy", "pyright", "cargo check"]) {
        CheckName::Typecheck
    } else if has(&["fmt", "format", "prettier", "black", "gofmt"]) {
        CheckName::Format
    } else if has(&["lint", "clippy", "eslint", "ruff", "flake8", "biome"]) {
        CheckName::Lint
    } else if has(&["syntax", "py_compile", "node --check"]) {
        CheckName::Syntax
    } else if has(&["test", "pytest", "jest", "vitest", "spec"]) {
        if tier >= 3 {
            CheckName::FullTests
        } else {
            CheckName::TargetedTests
        }
    } else {
        match tier {
            0 | 1 => CheckName::Lint,
            2 => CheckName::TargetedTests,
            _ => CheckName::FullTests,
        }
    }
}

/// Resumen de la salida de un comando.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OutputSummary {
    pub passed: Option<u32>,
    pub failed: Option<u32>,
    /// Las líneas que importan, para el usuario y para devolvérselas al agente.
    pub text: String,
}

const MAX_LINES: usize = 6;
const MAX_LINE: usize = 220;

/// Colapsa la salida (FLOW §15.2): conteos de pruebas si se reconocen y las primeras líneas
/// que parecen fallos, sin repetir. Con éxito y sin nada que contar, queda vacío.
pub fn summarize(output: &str, success: bool) -> OutputSummary {
    let mut passed = None;
    let mut failed = None;
    for line in output.lines() {
        let l = line.trim();
        let (p, f) = (number_before(l, "passed"), number_before(l, "failed"));
        if p.is_some() || f.is_some() {
            // La última línea de resumen manda (los runners imprimen el total al final).
            passed = p.or(passed);
            failed = f.or(failed);
        }
    }
    let mut lines: Vec<String> = Vec::new();
    if !success {
        for line in output.lines() {
            let l = line.trim();
            if l.is_empty() || !looks_like_failure(l) {
                continue;
            }
            let short: String = l.chars().take(MAX_LINE).collect();
            if !lines.contains(&short) {
                lines.push(short);
            }
            if lines.len() == MAX_LINES {
                break;
            }
        }
        if lines.is_empty() {
            // Sin patrón reconocible: el final de la salida es lo más probable que explique.
            let tail: Vec<&str> = output
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            let from = tail.len().saturating_sub(3);
            lines = tail[from..]
                .iter()
                .map(|l| l.chars().take(MAX_LINE).collect())
                .collect();
        }
    }
    OutputSummary {
        passed,
        failed,
        text: lines.join("\n"),
    }
}

fn looks_like_failure(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    [
        "fail",
        "error",
        "panicked",
        "expected",
        "assert",
        "not ok",
        "✗",
        "×",
        "exception",
    ]
    .iter()
    .any(|k| l.contains(k))
}

/// El número que precede a `word` en `line` («3 passed», «1 failed;», «Tests: 2 failed»).
/// Prueba cada aparición: en «test result: FAILED. 3 passed; 1 failed» la primera no lleva número.
fn number_before(line: &str, word: &str) -> Option<u32> {
    let lower = line.to_ascii_lowercase();
    lower.match_indices(word).find_map(|(at, _)| {
        // Una palabra entera, no «passed_checks».
        let after = lower[at + word.len()..].chars().next();
        if after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        let digits: String = lower[..at]
            .trim_end()
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        digits.parse().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_names_come_from_the_command_then_from_the_tier() {
        let cases: &[(&str, u8, CheckName)] = &[
            ("npm run lint", 1, CheckName::Lint),
            ("cargo clippy -- -D warnings", 1, CheckName::Lint),
            ("cargo fmt --check", 1, CheckName::Format),
            ("npx prettier --check .", 1, CheckName::Format),
            ("npm run typecheck", 2, CheckName::Typecheck),
            ("tsc --noEmit", 2, CheckName::Typecheck),
            ("npm test -- --changed", 2, CheckName::TargetedTests),
            ("npm test", 3, CheckName::FullTests),
            ("cargo nextest run", 3, CheckName::FullTests),
            ("npm run build", 3, CheckName::Build),
            ("cargo deny check", 3, CheckName::Security),
            ("node --check src/a.js", 1, CheckName::Syntax),
            ("./scripts/verify.sh", 1, CheckName::Lint),
            ("./scripts/verify.sh", 2, CheckName::TargetedTests),
            ("./scripts/verify.sh", 3, CheckName::FullTests),
        ];
        for (cmd, tier, want) in cases {
            assert_eq!(check_name(cmd, *tier), *want, "{cmd} (tier {tier})");
        }
    }

    #[test]
    fn rust_jest_and_pytest_summaries_give_counts() {
        let rust = "running 4 tests\ntest a ... ok\ntest b ... FAILED\n\nfailures:\n    b\n\ntest result: FAILED. 3 passed; 1 failed; 0 ignored";
        let s = summarize(rust, false);
        assert_eq!((s.passed, s.failed), (Some(3), Some(1)));
        assert!(s.text.contains("FAILED"));

        let jest = "FAIL src/auth.test.ts\n  ● refresh › returns 200\n    expected 200, received 401\nTests:       1 failed, 3 passed, 4 total";
        let s = summarize(jest, false);
        assert_eq!((s.passed, s.failed), (Some(3), Some(1)));
        assert!(s.text.contains("expected 200, received 401"), "{}", s.text);

        let py = "=== 1 failed, 5 passed in 0.31s ===";
        let s = summarize(py, false);
        assert_eq!((s.passed, s.failed), (Some(5), Some(1)));
    }

    #[test]
    fn a_passing_run_keeps_only_the_counts() {
        let s = summarize("test result: ok. 12 passed; 0 failed; 0 ignored", true);
        assert_eq!((s.passed, s.failed), (Some(12), Some(0)));
        assert!(s.text.is_empty());
    }

    #[test]
    fn without_a_known_pattern_the_tail_explains() {
        let s = summarize("paso 1\npaso 2\nboom\nse acabó\ndescansa", false);
        assert_eq!(s.text, "boom\nse acabó\ndescansa");
        assert_eq!(s.passed, None);
    }

    #[test]
    fn repeated_lines_collapse_and_long_ones_are_cut() {
        let long = format!("error: {}", "x".repeat(500));
        let out = format!("{long}\n{long}\nerror: otra\n");
        let s = summarize(&out, false);
        assert_eq!(s.text.lines().count(), 2);
        assert!(s.text.lines().next().unwrap().chars().count() <= MAX_LINE);
    }

    #[test]
    fn arbitrary_text_never_panics() {
        for t in [
            "",
            "\n\n",
            "passed",
            "5 passed 3",
            "ñ ✗ ×",
            &"a".repeat(10_000),
        ] {
            let _ = summarize(t, false);
            let _ = summarize(t, true);
        }
    }
}
