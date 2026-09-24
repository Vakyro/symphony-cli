//! `symphonyd` real con un `SYMPHONY_HOME` temporal.
// Código de test: CONSTRAINTS C3 permite unwrap/expect (clippy.toml solo lo cubre dentro de #[test]).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use symphony_protocol::transport;
use symphony_protocol::{Message, Outcome, Request};

fn spawn_daemon(home: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_symphonyd"))
        .env("SYMPHONY_HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("arrancar symphonyd")
}

async fn call(home: &Path, method: &str, params: Value) -> Outcome {
    let mut conn = transport::connect(home).await.expect("conectar");
    conn.send(&Message::Request(Request::new("t-1", method, params)))
        .await
        .unwrap();
    match conn.recv().await.unwrap() {
        Some(Message::Response(r)) => {
            assert_eq!(r.id, "t-1");
            r.outcome
        }
        other => panic!("respuesta inesperada: {other:?}"),
    }
}

async fn wait_ready(home: &Path, daemon: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while transport::connect(home).await.is_err() {
        let exited = daemon.try_wait().unwrap().is_some();
        if exited || Instant::now() > deadline {
            let _ = daemon.kill();
            let mut stderr = String::new();
            if let Some(mut e) = daemon.stderr.take() {
                std::io::Read::read_to_string(&mut e, &mut stderr).unwrap();
            }
            panic!(
                "el daemon no abrió el socket (terminó: {exited}). stderr:
{stderr}"
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn wait_exit(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "el daemon no terminó a tiempo");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[tokio::test]
async fn lifecycle_single_instance_and_methods() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("symphony-home");
    let mut first = spawn_daemon(&home);
    wait_ready(&home, &mut first).await;

    assert_eq!(
        call(&home, "ping", json!({})).await,
        Outcome::Ok(json!({"pong": true}))
    );

    let Outcome::Ok(status) = call(&home, "status", Value::Null).await else {
        panic!("status falló")
    };
    assert_eq!(status["pid"], json!(first.id()));
    assert_eq!(status["protocol"], json!(1));
    assert_eq!(status["version"], json!(env!("CARGO_PKG_VERSION")));

    let Outcome::Error(e) = call(&home, "agent.teleport", json!({})).await else {
        panic!("debió fallar")
    };
    assert_eq!(e.code, "unknown_method");
    let Outcome::Error(e) = call(&home, "ping", json!([1, 2])).await else {
        panic!("debió fallar")
    };
    assert_eq!(e.code, "invalid_params");

    // Un segundo daemon en el mismo home sale con un error claro que nombra al primero.
    let second = spawn_daemon(&home).wait_with_output().unwrap();
    assert!(!second.status.success());
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("ya hay un daemon de Symphony corriendo"),
        "{stderr}"
    );
    assert!(stderr.contains(&format!("pid {}", first.id())), "{stderr}");
    // El primero sigue sano.
    assert_eq!(
        call(&home, "ping", json!({})).await,
        Outcome::Ok(json!({"pong": true}))
    );

    assert_eq!(
        call(&home, "shutdown", json!({})).await,
        Outcome::Ok(json!({"stopping": true}))
    );
    assert!(wait_exit(&mut first).success());

    // El lock se liberó: otro daemon arranca en el mismo home.
    let mut third = spawn_daemon(&home);
    wait_ready(&home, &mut third).await;
    call(&home, "shutdown", json!({})).await;
    assert!(wait_exit(&mut third).success());

    // Hubo log en ~/.symphony/logs y la config se creó.
    let logs: Vec<_> = std::fs::read_dir(home.join("logs"))
        .unwrap()
        .flatten()
        .collect();
    assert!(!logs.is_empty());
    assert!(home.join("config.toml").exists());
}

#[tokio::test]
async fn bad_frames_close_the_connection_without_killing_the_daemon() {
    use tokio::io::AsyncWriteExt;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("h");
    let mut daemon = spawn_daemon(&home);
    wait_ready(&home, &mut daemon).await;

    // Cabecera que anuncia un frame gigante.
    let conn = transport::connect(&home).await.unwrap();
    let mut raw = conn.into_inner();
    raw.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
    drop(raw);
    // Versión de protocolo desconocida.
    let mut conn = transport::connect(&home).await.unwrap();
    let frame = br#"{"type":"request","protocol":99,"id":"x","method":"ping","params":{}}"#;
    let mut raw = conn.into_inner();
    raw.write_all(&symphony_protocol::encode_frame(frame).unwrap())
        .await
        .unwrap();
    conn = symphony_protocol::Connection::new(raw);
    let Some(Message::Response(r)) = conn.recv().await.unwrap() else {
        panic!("sin respuesta")
    };
    let Outcome::Error(e) = r.outcome else {
        panic!("debió fallar")
    };
    assert_eq!(e.code, "bad_request");
    assert!(
        e.message.contains("versión de protocolo 99"),
        "{}",
        e.message
    );

    assert_eq!(
        call(&home, "ping", json!({})).await,
        Outcome::Ok(json!({"pong": true}))
    );
    call(&home, "shutdown", json!({})).await;
    assert!(wait_exit(&mut daemon).success());
}
