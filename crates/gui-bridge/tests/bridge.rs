//! P15.S1: el puente contra un daemon real, solo por IPC.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::json;
use symphony_gui_bridge::{Bridge, BridgeError};

struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn build_daemon() -> PathBuf {
    let status = Command::new(env!("CARGO"))
        .args(["build", "-q", "-p", "symphony-daemon", "--bin", "symphonyd"])
        .status()
        .unwrap();
    assert!(status.success(), "no se pudo construir symphonyd");
    let exe = std::env::current_exe().unwrap();
    let target = exe.parent().unwrap().parent().unwrap();
    symphony_testkit::pinned_bin(&target.join(format!("symphonyd{}", std::env::consts::EXE_SUFFIX)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_bridge_calls_a_real_daemon_and_reports_when_it_is_down() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let bridge = Bridge::new(&home);

    // Sin daemon: error claro, no un cuelgue.
    assert!(matches!(
        bridge.call("ping", json!({})).await,
        Err(BridgeError::Unavailable(_))
    ));

    let bin = build_daemon();
    let _daemon = Daemon(
        Command::new(bin)
            .env("SYMPHONY_HOME", &home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut pong = None;
    for _ in 0..300 {
        if let Ok(v) = bridge.call("ping", json!({})).await {
            pong = Some(v);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(pong.unwrap()["pong"], true);
    assert!(bridge.call("status", json!({})).await.is_ok());

    // Un error del daemon llega con su código.
    match bridge.call("no.existe", json!({})).await {
        Err(BridgeError::Remote { code, .. }) => assert!(!code.is_empty()),
        other => panic!("{other:?}"),
    }
    // `ensure_daemon` con el daemon vivo no hace nada.
    bridge.ensure_daemon().await.unwrap();
}
