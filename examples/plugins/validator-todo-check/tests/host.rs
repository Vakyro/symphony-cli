//! El plugin de ejemplo, ejecutado por el host real de Symphony (el mismo camino que usa el
//! daemon): `describe` y `validate` sobre el binario compilado.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use symphony_plugins::{Manifest, Plugin, ValidateInput};

/// Instala el binario y su `plugin.toml` en una carpeta como lo haría el usuario.
fn installed() -> (tempfile::TempDir, Plugin) {
    let dir = tempfile::tempdir().unwrap();
    let name = format!("validator-todo-check{}", std::env::consts::EXE_SUFFIX);
    std::fs::copy(
        PathBuf::from(env!("CARGO_BIN_EXE_validator-todo-check")),
        dir.path().join(&name),
    )
    .unwrap();
    let manifest_text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/plugin.toml")).unwrap();
    std::fs::write(dir.path().join("plugin.toml"), &manifest_text).unwrap();
    let manifest = Manifest::parse(&manifest_text).expect("el plugin.toml del ejemplo es válido");
    let plugin = Plugin {
        manifest,
        dir: dir.path().to_path_buf(),
    };
    (dir, plugin)
}

fn input(diff: &str) -> ValidateInput {
    ValidateInput {
        task_code: "T-1".into(),
        task_title: "algo".into(),
        diff: diff.into(),
        files: vec!["src/a.rs".into()],
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_example_describes_itself_and_judges_a_diff() {
    let (_dir, p) = installed();
    let d = p.describe().await.unwrap();
    assert_eq!(
        (d.name.as_str(), d.kind.as_str()),
        ("validator-todo-check", "validator")
    );

    let bad = p
        .validate(&input("+++ b/src/a.rs\n+// TODO arreglar\n"))
        .await
        .unwrap();
    assert!(!bad.passed);
    assert_eq!(bad.findings.len(), 1);
    assert_eq!(bad.findings[0].file, "src/a.rs");

    let good = p
        .validate(&input("+++ b/src/a.rs\n+// TODO(#9) arreglar\n"))
        .await
        .unwrap();
    assert!(good.passed);
}
