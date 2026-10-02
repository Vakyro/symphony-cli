// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use proptest::prelude::*;

use super::*;

fn vitest_output(passed: usize, failed: usize) -> String {
    let mut s = String::from("\u{1b}[32m RUN \u{1b}[39m v2.1.0\n");
    for i in 0..passed {
        s.push_str(&format!(" ✓ src/mod{i}.test.ts (12 tests) {i}ms\n"));
    }
    for i in 0..failed {
        s.push_str(&format!(
            " FAIL src/auth{i}.test.ts > refresh > rota el token\nAssertionError: expected 401 to be 200\n"
        ));
    }
    s.push_str(&format!(
        " Tests:  {failed} failed, {passed} passed, {} total\n",
        passed + failed
    ));
    s
}

#[test]
fn test_output_becomes_one_line_plus_the_failures() {
    let out = compress(&vitest_output(895, 1), Hint::Auto);
    assert_eq!(out.compressor, Compressor::TestSummary);
    assert!(
        out.text.starts_with("896 pruebas: 895 pasaron, 1 falló"),
        "{}",
        out.text
    );
    assert!(out.text.contains("rota el token"), "{}", out.text);
    assert!(out.tokens_compressed * 20 < out.tokens_original, "{out:?}");
}

#[test]
fn cargo_and_pytest_summaries() {
    let cargo = "running 3 tests\ntest a ... ok\ntest b::c ... FAILED\ntest d ... ok\n\nfailures:\n    b::c\n\ntest result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured\n";
    let s = tests_summary::summarize(cargo).unwrap();
    assert!(s.starts_with("3 pruebas: 2 pasaron, 1 falló"), "{s}");
    assert!(s.contains("b::c"), "{s}");

    let py = "FAILED tests/test_x.py::test_y - AssertionError\n===== 1 failed, 40 passed, 2 skipped in 3.20s =====\n";
    let s = tests_summary::summarize(py).unwrap();
    assert!(
        s.starts_with("43 pruebas: 40 pasaron, 1 falló, 2 omitidas"),
        "{s}"
    );
    assert!(s.contains("test_y"), "{s}");

    assert!(tests_summary::summarize("hola\nmundo\n").is_none());
}

#[test]
fn logs_keep_every_error_and_collapse_the_noise() {
    let mut log = String::new();
    for i in 0..500 {
        log.push_str(&format!("downloading crate-{i} v1.{i}.0\n"));
    }
    log.push_str("error[E0308]: mismatched types\n  --> src/lib.rs:10:5\n");
    for i in 0..200 {
        log.push_str(&format!("   Compiling dep{i} v0.1.{i}\n"));
    }
    log.push_str("progreso 10%\rprogreso 50%\rprogreso 100%\n");
    log.push_str("panicked at 'boom', src/main.rs:3\n");
    let out = compress(&log, Hint::Log);
    assert!(out.text.contains("error[E0308]: mismatched types"));
    assert!(out.text.contains("panicked at 'boom'"));
    assert!(out.text.contains("progreso 100%"));
    assert!(!out.text.contains("progreso 10%"));
    assert!(out.text.contains("líneas parecidas omitidas") || out.text.contains("omitidas"));
    assert!(
        out.text.len() * 5 < log.len(),
        "{} vs {}",
        out.text.len(),
        log.len()
    );
}

#[test]
fn json_keeps_the_shape_and_flags_optional_fields() {
    let items: Vec<String> = (0..300)
        .map(|i| {
            if i == 7 {
                format!(r#"{{"id":{i},"name":"n{i}","score":{i}.5,"error":"timeout"}}"#)
            } else {
                format!(r#"{{"id":{i},"name":"n{i}","score":{i}.5}}"#)
            }
        })
        .collect();
    let json = format!("[{}]", items.join(","));
    let out = compress(&json, Hint::Auto);
    assert_eq!(out.compressor, Compressor::JsonStruct);
    assert!(out.text.contains("lista de 300"), "{}", out.text);
    assert!(
        out.text.contains("error: texto  ← solo en 1 de 300"),
        "{}",
        out.text
    );
    assert!(out.text.contains("id: número [0 … 299]"), "{}", out.text);
    assert!(out.text.len() * 10 < json.len());
}

#[test]
fn diff_lists_files_and_drops_lockfiles() {
    let mut d = String::from(
        "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n-viejo\n+nuevo\n contexto\n",
    );
    d.push_str("diff --git a/Cargo.lock b/Cargo.lock\nindex 1..2 100644\n--- a/Cargo.lock\n+++ b/Cargo.lock\n@@ -1,1 +1,400 @@\n");
    for i in 0..400 {
        d.push_str(&format!("+name = \"dep{i}\"\n"));
    }
    d.push_str(
        "diff --git a/src/b.rs b/src/b.rs\n--- a/src/b.rs\n+++ b/src/b.rs\n@@ -1,1 +1,200 @@\n",
    );
    for i in 0..200 {
        d.push_str(&format!("+let x{i} = {i};\n"));
    }
    let out = compress(&d, Hint::Auto);
    assert_eq!(out.compressor, Compressor::GitDiff);
    assert!(
        out.text.starts_with("3 archivos cambiados, +601 −1:"),
        "{}",
        out.text
    );
    assert!(out.text.contains("Cargo.lock — generado o lockfile"));
    assert!(!out.text.contains("dep399"));
    assert!(out.text.contains("+nuevo"));
    assert!(out.text.contains("líneas de este hunk omitidas"));
}

#[test]
fn dedup_points_back_to_the_first_copy() {
    let block = "Error: la conexión con el servidor falló\n  at connect (net.js:10)\n  at retry (net.js:42)";
    let text = format!("{block}\n\ntexto intermedio distinto\n\n{block}\n\n{block}");
    let out = dedup::dedup_blocks(&text);
    assert_eq!(out.matches("at retry").count(), 1, "{out}");
    assert!(out.contains("igual al de la línea 1"), "{out}");
}

#[test]
fn small_or_hostile_inputs_come_back_unchanged() {
    for text in ["", "ok", "una línea corta", "{\"a\":1}"] {
        let out = compress(text, Hint::Auto);
        assert_eq!(out.text, text);
        assert_eq!(out.compressor, Compressor::None);
    }
    // Binario / controles / diff roto: no entra en pánico.
    let weird = "\u{0}\u{1b}[\u{1b}]52;c;AAAA\u{7}\r\r\ndiff --git\n@@ -\n";
    let _ = compress(weird, Hint::Auto);
    assert!(json::summarize("{no es json").is_none());
}

#[test]
fn labels_fit_the_database_check() {
    for c in [
        Compressor::LogCollapse,
        Compressor::TestSummary,
        Compressor::JsonStruct,
        Compressor::Dedup,
        Compressor::GitDiff,
        Compressor::None,
    ] {
        assert!(
            ["LOG_COLLAPSE", "JSON_STRUCT", "AST", "DEDUP", "NONE"].contains(&c.db_label()),
            "{c:?}"
        );
    }
}

#[test]
fn patterns_compile() {
    assert!(log::is_important("error: algo"));
    assert!(!log::is_important("todo bien"));
    assert!(tests_summary::summarize("test result: ok. 1 passed; 0 failed; 0 ignored").is_some());
}

proptest! {
    #[test]
    fn never_panics_and_never_inflates(text in "\\PC{0,2000}", hint in 0u8..5) {
        let hint = [Hint::Auto, Hint::Log, Hint::Json, Hint::Diff, Hint::TestOutput][hint as usize];
        let out = compress(&text, hint);
        prop_assert!(out.text.len() <= text.len());
        prop_assert!(out.tokens_compressed <= out.tokens_original);
    }

    #[test]
    fn error_lines_survive_log_collapse(
        noise in prop::collection::vec("[a-z ]{5,40}", 0..400),
        pos in 0usize..400,
        code in 1000u32..9999,
    ) {
        let error = format!("error[E{code}]: algo se rompió");
        let mut lines = noise;
        let at = pos.min(lines.len());
        lines.insert(at, error.clone());
        let log = lines.join("\n");
        let out = log::collapse(&log);
        prop_assert!(out.contains(&error), "falta la línea de error");
    }
}
