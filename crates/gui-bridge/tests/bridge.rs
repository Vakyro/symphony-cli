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
    // La ruta la emite cargo: con `build.build-dir` (caché compartido del harness)
    // los `deps/` no cuelgan del ejecutable de este test.
    symphony_testkit::build_and_pin(
        "symphonyd",
        &["-p", "symphony-daemon", "--bin", "symphonyd"],
    )
    .expect("cargo build symphonyd")
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
