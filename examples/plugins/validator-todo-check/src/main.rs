//! Plugin de ejemplo (P12.S3, ADR-0015): un `validator` que falla si el diff de la tarea agrega
//! un `TODO` sin referencia a un issue.
//!
//! Es lo mínimo que hace falta para escribir uno: leer UNA línea JSON de stdin, contestar UNA
//! línea JSON por stdout (los registros van a stderr) y salir.
//!
//!   {"plugin_api":1,"kind":"validator","method":"describe","params":{}}
//!   {"plugin_api":1,"kind":"validator","method":"validate","params":{"diff":"…","files":[…],…}}

use std::io::{BufRead, Write};

use serde_json::{Value, json};

const NAME: &str = "validator-todo-check";
const VERSION: &str = "0.1.0";

/// Un hallazgo: dónde y qué línea.
#[derive(Debug, PartialEq, Eq)]
struct Finding {
    file: String,
    line: String,
}

/// ¿Esta línea agregada tiene un TODO sin issue? Un issue es `#123` o `ABC-12` justo después del
/// TODO: `TODO(#123)`, `TODO #123`, `TODO(ABC-12)`.
fn todo_without_issue(line: &str) -> bool {
    let mut rest = line;
    while let Some(i) = rest.find("TODO") {
        let after = &rest[i + 4..];
        if !has_issue(after) {
            return true;
        }
        rest = after;
    }
    false
}

fn has_issue(after_todo: &str) -> bool {
    let s = after_todo.trim_start_matches(['(', ' ', ':']);
    if let Some(num) = s.strip_prefix('#') {
        return num.chars().next().is_some_and(|c| c.is_ascii_digit());
    }
    // ABC-12
    let key: String = s.chars().take_while(char::is_ascii_uppercase).collect();
    let tail = &s[key.len()..];
    !key.is_empty()
        && tail
            .strip_prefix('-')
            .and_then(|t| t.chars().next())
            .is_some_and(|c| c.is_ascii_digit())
}

/// Recorre un diff unificado y devuelve cada línea **agregada** con un TODO sin issue.
fn check(diff: &str) -> Vec<Finding> {
    let mut file = String::new();
    let mut out = Vec::new();
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            file = path.strip_prefix("b/").unwrap_or(path).to_string();
        } else if let Some(added) = line.strip_prefix('+')
            && todo_without_issue(added)
        {
            out.push(Finding {
                file: file.clone(),
                line: added.trim().to_string(),
            });
        }
    }
    out
}

fn respond(result: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(
        out,
        "{}",
        json!({"plugin_api": 1, "ok": true, "result": result})
    );
}

fn fail(message: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(
        out,
        "{}",
        json!({"plugin_api": 1, "ok": false, "error": message})
    );
}

fn main() {
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        fail("no se pudo leer la petición");
        return;
    }
    let request: Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(e) => {
            fail(&format!("la petición no es JSON: {e}"));
            return;
        }
    };
    match request["method"].as_str() {
        Some("describe") => respond(&json!({
            "name": NAME, "version": VERSION, "plugin_api": 1, "kind": "validator"
        })),
        Some("validate") => {
            let findings = check(request["params"]["diff"].as_str().unwrap_or(""));
            let summary = if findings.is_empty() {
                "ningún TODO nuevo sin issue".to_string()
            } else {
                format!("{} TODO nuevo(s) sin issue", findings.len())
            };
            eprintln!("{NAME}: {summary}");
            respond(&json!({
                "passed": findings.is_empty(),
                "summary": summary,
                "findings": findings.iter().map(|f| json!({
                    "severity": "medium",
                    "file": f.file,
                    "text": format!("TODO sin issue: {}", f.line),
                })).collect::<Vec<_>>(),
            }));
        }
        other => fail(&format!("método no soportado: {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,5 @@
 fn a() {}
+// TODO arreglar esto
+// TODO(#42): con issue
+// TODO #43 también con issue
+// TODO(ABC-7): con clave de Jira
-// TODO viejo que se borra
";

    #[test]
    fn only_added_todos_without_an_issue_are_findings() {
        let f = check(DIFF);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].file, "src/a.rs");
        assert!(f[0].line.contains("arreglar esto"));
    }

    #[test]
    fn issue_forms() {
        for ok in ["TODO(#1)", "TODO #12", "TODO: #7 revisar", "TODO(PROJ-123)"] {
            assert!(!todo_without_issue(ok), "{ok}");
        }
        for bad in [
            "TODO",
            "TODO: después",
            "TODO(alguien)",
            "TODO #",
            "TODO(abc-1)",
        ] {
            assert!(todo_without_issue(bad), "{bad}");
        }
        // Dos TODO en la misma línea: basta que uno no tenga issue.
        assert!(todo_without_issue("TODO(#1) y TODO"));
    }

    #[test]
    fn an_empty_or_clean_diff_has_no_findings() {
        assert!(check("").is_empty());
        assert!(check("+++ b/x\n+let todo = 1;\n").is_empty());
    }
}
