use std::process::Command;

#[test]
fn version_flag_prints_package_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_symphony"))
        .arg("--version")
        .output()
        .expect("run symphony");
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("symphony {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unknown_args_exit_with_usage_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_symphony"))
        .output()
        .expect("run symphony");
    assert_eq!(out.status.code(), Some(2));
}
