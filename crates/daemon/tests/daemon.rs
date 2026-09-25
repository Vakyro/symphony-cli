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

#[tokio::test]
async fn startup_recovers_sessions_left_active_by_a_dead_daemon() {
    use symphony_core::{ProjectId, SessionId, SymphonyHome};
    use symphony_store::repo;

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("h");
    // Estado que dejaría un daemon muerto a mitad de sesión.
    let db = SymphonyHome::at(&home).db_path();
    let session = SessionId::new();
    {
        let conn = symphony_store::open(&db).unwrap();
        let project = ProjectId::new();
        repo::insert_project(
            &conn,
            &repo::Project {
                id: project,
                name: "demo".into(),
                root_path: "/repo".into(),
                default_branch: "main".into(),
                created_at: 1,
            },
        )
        .unwrap();
        repo::start_session(&conn, session, project, 999_999, 1).unwrap();
    }

    let mut daemon = spawn_daemon(&home);
    wait_ready(&home, &mut daemon).await;
    let Outcome::Ok(status) = call(&home, "status", json!({})).await else {
        panic!("status falló")
    };
    assert_eq!(status["recovery_open"], json!(1));
    call(&home, "shutdown", json!({})).await;
    assert!(wait_exit(&mut daemon).success());

    let conn = symphony_store::open_reader(&db).unwrap();
    let (st, ended): (String, Option<i64>) = conn
        .query_row(
            "SELECT status, ended_at FROM sessions WHERE id=?1",
            [session.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(st, "INTERRUPTED");
    assert!(ended.is_some());
    let (kind, detail): (String, String) = conn
        .query_row("SELECT kind, detail FROM recovery_items", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(kind, "SESSION_INTERRUPTED");
    assert!(detail.contains("999999"), "{detail}");
    drop(conn);

    // Un segundo arranque no duplica el recovery item.
    let mut again = spawn_daemon(&home);
    wait_ready(&home, &mut again).await;
    let Outcome::Ok(status) = call(&home, "status", json!({})).await else {
        panic!("status falló")
    };
    assert_eq!(status["recovery_open"], json!(1));
    call(&home, "shutdown", json!({})).await;
    assert!(wait_exit(&mut again).success());
}

#[tokio::test]
async fn client_refuses_a_socket_served_by_another_process() {
    if cfg!(target_os = "macos") {
        // macOS no informa el pid del par: ahí se verifica el uid (mismo usuario), que
        // no se puede falsear sin una segunda cuenta. El 0700 del directorio sigue valiendo.
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("h");
    let mut daemon = spawn_daemon(&home);
    wait_ready(&home, &mut daemon).await;
    let pid_file = home.join("run").join("symphonyd.pid");
    let real = std::fs::read_to_string(&pid_file).unwrap();

    // Simula que el socket lo atiende un proceso que no es el daemon registrado.
    std::fs::write(&pid_file, "1\n").unwrap();
    let Err(err) = transport::connect(&home).await else {
        panic!("debió rechazar la conexión")
    };
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied, "{err}");

    std::fs::write(&pid_file, real).unwrap();
    assert_eq!(
        call(&home, "ping", json!({})).await,
        Outcome::Ok(json!({"pong": true}))
    );
    call(&home, "shutdown", json!({})).await;
    assert!(wait_exit(&mut daemon).success());
}
