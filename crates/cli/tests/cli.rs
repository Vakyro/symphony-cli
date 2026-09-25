//! Tests `trycmd` del binario `symphony` (STACK §24.2).
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::symphonyd;

#[test]
fn cli_commands() {
    let home = tempfile::tempdir().unwrap();
    trycmd::TestCases::new()
        .env(
            "SYMPHONY_HOME",
            home.path().join("home").display().to_string(),
        )
        .env("SYMPHONYD", symphonyd().display().to_string())
        .case("tests/cmd/*.toml")
        .case("tests/cmd/*.md");
}
