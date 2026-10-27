//! P06.S1: creación de agentes (FLOW §6) — flujo feliz y cada rama de error.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use symphony_adapter_common::{AgentEvent, ProviderAdapter};
use symphony_core::{AgentState, ContextMode, FailoverPolicy};
use symphony_daemon::bus::EventBus;
use symphony_daemon::providers;
use symphony_daemon::runtime::{CreateAgent, CreateError, Execution, Runtime};
use symphony_object_store::ObjectStore;
use symphony_store::Writer;
use symphony_testkit::FakeAdapter;

fn fake_agent() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let ok = Command::new(env!("CARGO"))
                .args([
                    "build",
                    "-q",
                    "-p",
                    "symphony-testkit",
                    "--bin",
                    "fake-agent",
                ])
                .status()
                .unwrap()
                .success();
            assert!(ok, "no se pudo construir fake-agent");
            symphony_testkit::pinned_bin(
                &PathBuf::from(env!("CARGO_BIN_EXE_symphonyd"))
                    .with_file_name(format!("fake-agent{}", std::env::consts::EXE_SUFFIX)),
            )
        })
        .clone()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct Env {
    _dir: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
    db: PathBuf,
    writer: Writer,
    bus: EventBus,
    runtime: Runtime,
}

/// Home + repo con un commit + proveedor `fake` detectado con el guion dado.
/// `binary` reemplaza al fake-agent (para simular un CLI que no arranca).
async fn env(script: &str, binary: Option<PathBuf>) -> Env {
    env_with(&[("fake", script)], binary).await
}

/// Como `env`, con varios proveedores fake (`provider`, guion), en ese orden.
async fn env_with(providers_: &[(&'static str, &str)], binary: Option<PathBuf>) -> Env {
    env_with_watchdog(
        providers_,
        binary,
        Duration::from_secs(5),
        Duration::from_secs(600),
    )
    .await
}

async fn env_with_watchdog(
    providers_: &[(&'static str, &str)],
    binary: Option<PathBuf>,
    heartbeat_every: Duration,
    stale_after: Duration,
) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("README.md"), "demo\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);

    std::fs::create_dir_all(&home).unwrap();
    let db = home.join("symphony.db");
    let writer = Writer::start(&db).unwrap();
    let mut detected: Vec<Box<dyn ProviderAdapter>> = Vec::new();
    let mut used: Vec<Arc<dyn ProviderAdapter>> = Vec::new();
    for (provider, script) in providers_ {
        let script_path = dir.path().join(format!("{provider}.toml"));
        std::fs::write(&script_path, script).unwrap();
        detected.push(Box::new(
            FakeAdapter::new(fake_agent(), &script_path).with_provider(provider),
        ));
        used.push(Arc::new(
            FakeAdapter::new(binary.clone().unwrap_or_else(fake_agent), &script_path)
                .with_provider(provider),
        ));
    }
    providers::save(&writer.handle(), providers::detect_all(&detected), 1)
        .await
        .unwrap();
    let bus = EventBus::new(writer.handle(), 64, ObjectStore::new(home.join("objects")));
    let runtime = Runtime::new_with_watchdog(
        &home,
        writer.handle(),
        writer.reader().unwrap(),
        bus.clone(),
        used,
        None,
        symphony_daemon::runtime::WatchdogTiming {
            heartbeat_every,
            stale_after,
        },
    );
    Env {
        _dir: dir,
        home,
        repo,
        db,
        writer,
        bus,
        runtime,
    }
}

fn req(e: &Env, title: &str, execution: Execution) -> CreateAgent {
    CreateAgent {
        project_root: e.repo.clone(),
        title: title.into(),
        description: None,
        execution,
        failover: FailoverPolicy::Any,
        context_mode: ContextMode::Balanced,
        priority: 0,
        chat: false,
        depends_on: Vec::new(),
        review: false,
    }
}

fn count(e: &Env, table: &str) -> i64 {
    symphony_store::open_reader(&e.db)
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn one<T: rusqlite::types::FromSql>(e: &Env, sql: &str) -> T {
    symphony_store::open_reader(&e.db)
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}

/// Nada quedó a medias: ni filas, ni worktrees, ni ramas de agentes.
fn assert_nothing_created(e: &Env) {
    for t in ["tasks", "agents", "worktrees", "agent_runs", "checkpoints"] {
        assert_eq!(count(e, t), 0, "{t} no está vacía");
    }
    assert!(!e.home.join("worktrees").exists() || dir_is_empty_tree(&e.home.join("worktrees")));
    assert_eq!(git(&e.repo, &["branch", "--list", "symphony/*"]), "");
    assert_eq!(git(&e.repo, &["worktree", "list"]).lines().count(), 1);
}

fn dir_is_empty_tree(p: &Path) -> bool {
    std::fs::read_dir(p)
        .unwrap()
        .flatten()
        .all(|d| d.path().is_dir() && dir_is_empty_tree(&d.path()))
}

const WORK: &str = r#"
[[step]]
kind = "say"
text = "Arreglando"

[[step]]
kind = "edit"
path = "fix.txt"
content = "arreglado\n"
"#;

#[tokio::test(flavor = "multi_thread")]
async fn exact_model_creates_everything_and_runs_the_executor() {
    let e = env(WORK, None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Arreglar la rotación de tokens",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    assert_eq!((created.number, created.task_code.as_str()), (1, "T-1"));
    assert_eq!(created.state, AgentState::Running);
    let (run_id, model) = created.run.clone().unwrap();
    assert_eq!(model, "fake/fast");
    assert!(created.worktree.join("README.md").is_file());
    assert!(created.worktree.starts_with(e.home.join("worktrees")));

    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    // El executor trabajó en el worktree, no en el repo base.
    assert!(created.worktree.join("fix.txt").is_file());
    assert!(!e.repo.join("fix.txt").exists());
    assert_eq!(
        git(&created.worktree, &["branch", "--show-current"]),
        created.branch
    );
    let base = git(&e.repo, &["rev-parse", "HEAD"]);

    let id = created.agent_id.to_string();
    let q = |sql: &str| one::<String>(&e, &sql.replace("{a}", &id));
    assert_eq!(q("SELECT state FROM agents WHERE id='{a}'"), "COMPLETED");
    assert_eq!(
        q("SELECT execution_mode FROM agents WHERE id='{a}'"),
        "EXACT"
    );
    assert_eq!(
        q("SELECT requested_model_id FROM agents WHERE id='{a}'"),
        "fake/fast"
    );
    assert_eq!(
        q("SELECT t.status FROM tasks t JOIN agents a ON a.task_id=t.id WHERE a.id='{a}'"),
        "DONE"
    );
    // El modelo vive en el run (AGENT ≠ MODEL).
    assert_eq!(
        q(&format!(
            "SELECT model_id || '|' || status || '|' || end_reason || '|' || (cli_session_id IS NOT NULL) FROM agent_runs WHERE id='{run_id}'"
        )),
        "fake/fast|EXITED|COMPLETED|1"
    );
    // Checkpoint inicial: objetivo y commit base.
    assert_eq!(
        q(
            "SELECT seq || '|' || objective || '|' || head_commit FROM checkpoints WHERE agent_id='{a}' AND seq = 1"
        ),
        format!("1|Arreglar la rotación de tokens|{base}")
    );
    assert_eq!(q("SELECT status FROM worktrees"), "READY");
    assert_eq!(q("SELECT status FROM sessions"), "ACTIVE");
    // Los eventos del stream llegaron con el run.
    let started: i64 = one(
        &e,
        &format!(
            "SELECT COUNT(*) FROM events WHERE agent_id='{id}' AND run_id='{run_id}' AND type='AgentStarted'"
        ),
    );
    assert_eq!(started, 1);
    assert_eq!(count(&e, "recovery_items"), 0);

    // Un segundo agente del mismo proyecto: misma sesión, número y código siguientes.
    let second = e
        .runtime
        .create_agent(req(&e, "Otra cosa", Execution::DecideLater))
        .await
        .unwrap();
    assert_eq!((second.number, second.task_code.as_str()), (2, "T-2"));
    assert_eq!(count(&e, "sessions"), 1);
    assert_eq!(count(&e, "projects"), 1);
    assert_ne!(second.worktree, created.worktree);
    e.writer.shutdown();
}

/// Un agente sin harness tampoco puede pasar inadvertido si toca el repo principal.
#[tokio::test(flavor = "multi_thread")]
async fn main_repo_changes_during_a_run_open_workspace_damaged_recovery() {
    let script = r#"
[[step]]
kind = "run"
command = ["git", "-C", "../../../../repo", "commit", "--allow-empty", "-q", "-m", "daño simulado"]

[[step]]
kind = "edit"
path = "../../../../repo/dano.txt"
content = "cambio sin seguimiento\n"
"#;
    let e = env(script, None).await;
    let initial_head = git(&e.repo, &["rev-parse", "HEAD"]);
    let _created = e
        .runtime
        .create_agent(req(
            &e,
            "Simular una fuga al repo principal",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_ne!(git(&e.repo, &["rev-parse", "HEAD"]), initial_head);
    assert_eq!(count(&e, "recovery_items"), 1);
    let recovery = one::<String>(&e, "SELECT kind || '|' || detail FROM recovery_items");
    assert!(recovery.starts_with("WORKSPACE_DAMAGED|"), "{recovery}");
    assert!(
        recovery.contains("HEAD antes:") && recovery.contains("HEAD después:"),
        "{recovery}"
    );
    assert!(recovery.contains("?? dano.txt"), "{recovery}");
    assert_eq!(
        one::<String>(&e, "SELECT state FROM agents"),
        "COMPLETED",
        "la guardia registra el daño sin borrar el resultado del agente"
    );
    e.writer.shutdown();
}

/// P08.S4: el árbol de procesos del run queda en `processes` y se cierra al terminar.
#[tokio::test(flavor = "multi_thread")]
async fn the_process_tree_of_a_run_is_recorded_and_closed() {
    // El CLI vive más que la primera foto (que se toma al arrancar el run).
    let script = r#"
[[step]]
kind = "say"
text = "Pensando"

[[step]]
kind = "sleep"
ms = 1500
"#;
    let e = env(script, None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "Proceso", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    let (run_id, _) = created.run.clone().unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let pid: i64 = one(
        &e,
        &format!("SELECT pid FROM agent_runs WHERE id='{run_id}'"),
    );
    let root: String = one(
        &e,
        &format!(
            "SELECT pid || '|' || (ended_at IS NOT NULL) || '|' || (command IS NOT NULL) FROM processes
             WHERE run_id='{run_id}' AND parent_process_id IS NULL ORDER BY started_at LIMIT 1"
        ),
    );
    assert_eq!(
        root,
        format!("{pid}|1|1"),
        "la raíz es el CLI y quedó cerrada"
    );
    let open: i64 = one(
        &e,
        &format!("SELECT COUNT(*) FROM processes WHERE run_id='{run_id}' AND ended_at IS NULL"),
    );
    assert_eq!(open, 0, "al terminar el run no queda nada abierto");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn decide_later_creates_a_ready_agent_without_executor() {
    let e = env(WORK, None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "Revisar el login", Execution::DecideLater))
        .await
        .unwrap();
    assert_eq!(created.state, AgentState::Ready);
    assert!(created.run.is_none());
    assert!(created.worktree.join("README.md").is_file());
    assert_eq!(count(&e, "agent_runs"), 0);
    assert_eq!(count(&e, "checkpoints"), 1);
    assert_eq!(one::<String>(&e, "SELECT status FROM tasks"), "READY");
    e.writer.shutdown();
}

/// Disco (2026-10-24): cancelar un agente borra su worktree limpio y deja la rama; uno con
/// cambios sin commitear se queda.
#[tokio::test(flavor = "multi_thread")]
async fn stopping_an_agent_frees_its_clean_worktree_and_keeps_the_branch() {
    let e = env(WORK, None).await;
    let clean = e
        .runtime
        .create_agent(req(&e, "Limpio", Execution::DecideLater))
        .await
        .unwrap();
    let dirty = e
        .runtime
        .create_agent(req(&e, "Con cambios", Execution::DecideLater))
        .await
        .unwrap();
    // Revisión de código de P13: un archivo nuevo sin seguimiento también es trabajo.
    let new_file = e
        .runtime
        .create_agent(req(&e, "Archivo nuevo", Execution::DecideLater))
        .await
        .unwrap();
    std::fs::write(dirty.worktree.join("README.md"), "a medias\n").unwrap();
    std::fs::write(new_file.worktree.join("nuevo.txt"), "sin commitear\n").unwrap();
    e.runtime.stop(clean.agent_id).await.unwrap();
    e.runtime.stop(dirty.agent_id).await.unwrap();
    e.runtime.stop(new_file.agent_id).await.unwrap();
    e.writer.handle().flush().await.unwrap();

    assert!(!clean.worktree.exists());
    assert!(git(&e.repo, &["branch", "--list", &clean.branch]).contains(&clean.branch));
    assert!(dirty.worktree.join("README.md").is_file());
    assert!(
        new_file.worktree.join("nuevo.txt").is_file(),
        "un archivo nuevo sin commitear no se borra"
    );
    let status = |id: &symphony_core::AgentId| {
        one::<String>(
            &e,
            &format!(
                "SELECT w.status FROM worktrees w JOIN agents a ON a.worktree_id = w.id WHERE a.id = '{id}'"
            ),
        )
    };
    assert_eq!(status(&clean.agent_id), "REMOVED");
    assert_ne!(status(&dirty.agent_id), "REMOVED");
    e.writer.shutdown();
}

/// Disco: un agente completado que nadie integró libera su worktree a los 7 días, no antes.
#[tokio::test(flavor = "multi_thread")]
async fn a_completed_agent_keeps_its_worktree_for_a_week() {
    let e = env(WORK, None).await;
    let c = e
        .runtime
        .create_agent(req(&e, "Hecho", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    // El fake-agent deja `fix.txt` sin commitear; aquí el trabajo ya está en la rama.
    if !git(&c.worktree, &["status", "--porcelain"]).is_empty() {
        git(&c.worktree, &["add", "-A"]);
        git(&c.worktree, &["commit", "-qm", "hecho"]);
    }
    e.runtime.reclaim_disk(false).await;
    assert!(c.worktree.exists(), "recién completado: se queda");
    let conn = rusqlite::Connection::open(&e.db).unwrap();
    conn.execute(
        "UPDATE agents SET updated_at = 0 WHERE id = ?1",
        [c.agent_id.to_string()],
    )
    .unwrap();
    e.runtime.reclaim_disk(false).await;
    assert!(!c.worktree.exists(), "una semana después se borra");
    e.writer.shutdown();
}

/// Disco: los agentes de un proyecto Rust comparten el `build-dir` de Cargo (y cada uno guarda
/// sus binarios en su `target/`); el repo del usuario no.
#[test]
fn cargo_builds_of_agents_share_one_build_dir() {
    let user_chose = std::env::var_os("CARGO_TARGET_DIR").is_some()
        || std::env::var_os("CARGO_BUILD_BUILD_DIR").is_some();
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join(".symphony");
    let wt = home.join("worktrees").join("p").join("agent-001");
    std::fs::create_dir_all(&wt).unwrap();
    assert!(
        symphony_daemon::gc::cargo_env(&home, &wt).is_empty(),
        "sin Cargo.toml"
    );
    std::fs::write(wt.join("Cargo.toml"), "").unwrap();
    std::fs::write(d.path().join("Cargo.toml"), "").unwrap();
    let env = symphony_daemon::gc::cargo_env(&home, &wt);
    if !user_chose {
        assert_eq!(
            env,
            [(
                "CARGO_BUILD_BUILD_DIR".to_string(),
                home.join("cache")
                    .join("cargo-build")
                    .join("1")
                    .display()
                    .to_string()
            )]
        );
        // El agente siguiente cae en otra carpeta: no esperan el mismo candado de Cargo.
        let next = home.join("worktrees").join("p").join("agent-002");
        std::fs::create_dir_all(&next).unwrap();
        std::fs::write(next.join("Cargo.toml"), "").unwrap();
        assert_ne!(symphony_daemon::gc::cargo_env(&home, &next), env);
    }
    assert!(
        symphony_daemon::gc::cargo_env(&home, d.path()).is_empty(),
        "el repo del usuario"
    );
    if cfg!(windows) && !user_chose {
        let upper = PathBuf::from(wt.to_string_lossy().to_uppercase());
        assert_eq!(
            symphony_daemon::gc::cargo_env(&home, &upper).len(),
            1,
            "mayúsculas"
        );
    }
}

/// P07.5.S4: crear el chat dos veces devuelve el mismo agente, en `symphony/chat`.
#[tokio::test(flavor = "multi_thread")]
async fn chat_is_created_once_per_project_on_its_own_branch() {
    let e = env(WORK, None).await;
    let mut r = req(&e, "Chat", Execution::DecideLater);
    r.chat = true;
    let first = e.runtime.create_agent(r.clone()).await.unwrap();
    let again = e.runtime.create_agent(r).await.unwrap();
    assert_eq!(again.agent_id, first.agent_id);
    assert_eq!(again.worktree, first.worktree);
    assert_eq!(first.branch, "symphony/chat");
    assert_eq!(first.task_code, "CHAT");
    assert_eq!(count(&e, "agents"), 1);
    assert_eq!(count(&e, "worktrees"), 1);
    assert_eq!(count(&e, "tasks"), 1);
    assert_eq!(
        git(&e.repo, &["rev-parse", "symphony/chat"]),
        git(&e.repo, &["rev-parse", "HEAD"])
    );
    e.writer.shutdown();
}

/// P07.5.S4: el turno del chat termina en `READY` («esperando mensaje»), no en `COMPLETED`.
#[tokio::test(flavor = "multi_thread")]
async fn chat_waits_for_a_message_after_its_turn() {
    let e = env(WORK, None).await;
    let mut r = req(&e, "Hola", Execution::Exact("fake/fast".into()));
    r.chat = true;
    e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "READY");
    assert_eq!(
        one::<String>(&e, "SELECT state_reason FROM agents"),
        "esperando mensaje"
    );
    assert_eq!(one::<String>(&e, "SELECT status FROM tasks"), "READY");
    e.writer.shutdown();
}

/// P07.5.S5: un turno con cambios deja un commit y el worktree limpio; uno sin cambios, ninguno.
#[tokio::test(flavor = "multi_thread")]
async fn chat_commits_a_turn_only_when_it_changed_files() {
    let e = env(WORK, None).await;
    let mut r = req(&e, "Hola", Execution::Exact("fake/fast".into()));
    r.chat = true;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let subjects = |n: &str| git(&e.repo, &["log", "--format=%s", n]);
    assert_eq!(subjects("symphony/chat"), "chat: turno 1 (fake/fast)\ninit");
    assert_eq!(git(&created.worktree, &["status", "--porcelain"]), "");
    assert_eq!(
        git(&created.worktree, &["show", "HEAD:fix.txt"]),
        "arreglado"
    );

    // El segundo turno reescribe el mismo contenido: no hay cambios, no hay commit.
    e.runtime
        .continue_session(created.agent_id, "otra vez")
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(subjects("symphony/chat"), "chat: turno 1 (fake/fast)\ninit");
    assert_eq!(subjects("main"), "init");
    e.writer.shutdown();
}

/// P07.5.S5: el bus recibe los cambios de estado desde el punto único de escritura.
#[tokio::test(flavor = "multi_thread")]
async fn bus_announces_agent_state_changes() {
    let e = env(WORK, None).await;
    let mut rx = e.bus.subscribe();
    let mut r = req(&e, "Hola", Execution::Exact("fake/fast".into()));
    r.chat = true;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;

    let seen = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let ev = rx.recv().await.unwrap();
            if let AgentEvent::StateChanged { from, to, reason } = &ev.event {
                assert_eq!(ev.agent_id, Some(created.agent_id.to_string()));
                return (from.clone(), to.clone(), reason.clone());
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        seen,
        (
            "RUNNING".to_string(),
            "READY".to_string(),
            Some("esperando mensaje".to_string())
        )
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn unavailable_exact_model_asks_the_user_and_creates_nothing() {
    let e = env(WORK, None).await;
    let err = e
        .runtime
        .create_agent(req(
            &e,
            "Tarea importante",
            Execution::Exact("fake/ultra".into()),
        ))
        .await
        .unwrap_err();
    let CreateError::ExactModelUnavailable {
        model,
        reason,
        task,
    } = &err
    else {
        panic!("{err:?}")
    };
    assert_eq!(
        (model.as_str(), task.as_str()),
        ("fake/ultra", "Tarea importante")
    );
    assert!(reason.contains("no existe"), "{reason}");
    for option in ["esperar", "escoger otro modelo", "profile", "cancelar"] {
        assert!(err.to_string().contains(option), "{err}");
    }
    assert_nothing_created(&e);

    // Proveedor deshabilitado por el usuario: mismo camino.
    symphony_store::open(&e.db)
        .unwrap()
        .execute("UPDATE providers SET enabled = 0", [])
        .unwrap();
    let err = e
        .runtime
        .create_agent(req(
            &e,
            "Tarea importante",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CreateError::ExactModelUnavailable { reason, .. } if reason.contains("deshabilitado")),
        "{err:?}"
    );
    assert_nothing_created(&e);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn no_ready_provider_keeps_the_task_text() {
    let e = env(WORK, None).await;
    symphony_store::open(&e.db)
        .unwrap()
        .execute("UPDATE providers SET setup_state = 'NOT_FOUND'", [])
        .unwrap();
    let err = e
        .runtime
        .create_agent(req(&e, "No perder esto", Execution::DecideLater))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CreateError::NoEligibleProvider { task } if task == "No perder esto"),
        "{err:?}"
    );
    assert_eq!(err.code(), "no_eligible_provider");
    // Con modelo exacto, el motivo nombra al proveedor.
    let err = e
        .runtime
        .create_agent(req(
            &e,
            "No perder esto",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CreateError::ExactModelUnavailable { reason, .. } if reason.contains("no está listo (NOT_FOUND)")),
        "{err:?}"
    );
    assert_nothing_created(&e);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_requests_are_rejected_before_touching_anything() {
    let e = env(WORK, None).await;
    let err = e
        .runtime
        .create_agent(req(&e, "   ", Execution::DecideLater))
        .await
        .unwrap_err();
    assert!(matches!(err, CreateError::EmptyTask), "{err:?}");

    let err = e
        .runtime
        .create_agent(req(
            &e,
            "Con profile",
            Execution::Profile("@inexistente".into()),
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CreateError::UnknownProfile { profile, .. } if profile == "@inexistente"),
        "{err:?}"
    );

    let mut not_repo = req(&e, "Fuera de git", Execution::DecideLater);
    not_repo.project_root = e.home.clone();
    let err = e.runtime.create_agent(not_repo).await.unwrap_err();
    assert!(matches!(err, CreateError::NotARepo { .. }), "{err:?}");
    assert_nothing_created(&e);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn workspace_failure_leaves_no_half_built_agent() {
    let e = env(WORK, None).await;
    // La rama del agente ya existe: `git worktree add -b` falla.
    let clash = e
        .runtime
        .create_agent(req(&e, "Primero", Execution::DecideLater))
        .await
        .unwrap();
    std::fs::remove_dir_all(e.home.join("worktrees")).unwrap();
    git(&e.repo, &["worktree", "prune"]);
    symphony_store::open(&e.db)
        .unwrap()
        .execute_batch("DELETE FROM checkpoints; DELETE FROM agents; DELETE FROM worktrees; DELETE FROM tasks;")
        .unwrap();
    // Queda la rama `…/agent-001` del intento anterior, sin agente.
    assert_eq!(
        git(&e.repo, &["branch", "--list", "symphony/*"]).trim_start_matches(['*', '+', ' ']),
        clash.branch
    );

    let err = e
        .runtime
        .create_agent(req(&e, "Segundo", Execution::DecideLater))
        .await
        .unwrap_err();
    let CreateError::Workspace { detail, task } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(task, "Segundo");
    assert!(
        detail.contains("already exists") || detail.contains("ya existe"),
        "{detail}"
    );
    assert!(err.to_string().contains("reintentar o cancelar"), "{err}");
    for t in ["tasks", "agents", "worktrees", "checkpoints"] {
        assert_eq!(count(&e, t), 0, "{t}");
    }
    assert!(dir_is_empty_tree(&e.home.join("worktrees")));
    assert_eq!(
        git(&e.repo, &["branch", "--list", "symphony/*"]).trim_start_matches(['*', '+', ' ']),
        clash.branch,
        "la rama preexistente no se borra ante una colisión"
    );
    e.writer.shutdown();
}

/// Auditoría P13.S5 (`server.md`, hallazgo 1): una colisión con un worktree o rama preexistentes
/// devuelve error sin borrar el directorio (con trabajo sin commitear) ni la rama anterior.
#[tokio::test(flavor = "multi_thread")]
async fn workspace_collision_preserves_preexisting_worktree_and_branch() {
    let e = env(WORK, None).await;
    let clash = e
        .runtime
        .create_agent(req(&e, "Primero", Execution::DecideLater))
        .await
        .unwrap();
    let uncommitted = clash.worktree.join("sin-commitear.txt");
    std::fs::write(&uncommitted, "trabajo valioso\n").unwrap();
    symphony_store::open(&e.db)
        .unwrap()
        .execute_batch(
            "DELETE FROM checkpoints; DELETE FROM agents; DELETE FROM worktrees; DELETE FROM tasks;",
        )
        .unwrap();

    // 1. Colisión de carpeta (`agent-001` sigue en disco con cambios sin commit) y de rama.
    let err = e
        .runtime
        .create_agent(req(&e, "Segundo", Execution::DecideLater))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CreateError::Workspace { detail, .. } if detail.contains("ya existe")),
        "{err:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&uncommitted).unwrap(),
        "trabajo valioso\n",
        "la carpeta preexistente y sus archivos sin commitear se conservan"
    );
    assert_eq!(
        git(&e.repo, &["branch", "--list", "symphony/*"]).trim_start_matches(['*', '+', ' ']),
        clash.branch,
        "la rama preexistente se conserva"
    );

    // 2. Quitada la carpeta, queda solo la rama preexistente con un commit propio: tampoco se borra.
    git(&clash.worktree, &["add", "-A"]);
    git(&clash.worktree, &["commit", "-q", "-m", "commit previo"]);
    let branch_tip = git(&e.repo, &["rev-parse", &clash.branch]);
    git(
        &e.repo,
        &[
            "worktree",
            "remove",
            "--force",
            &clash.worktree.display().to_string(),
        ],
    );
    let err = e
        .runtime
        .create_agent(req(&e, "Tercero", Execution::DecideLater))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CreateError::Workspace { detail, .. } if detail.contains("ya existe")),
        "{err:?}"
    );
    assert_eq!(
        git(&e.repo, &["rev-parse", &clash.branch]),
        branch_tip,
        "el commit de la rama preexistente sigue intacto"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn store_failure_after_the_worktree_rolls_it_back() {
    let e = env(WORK, None).await;
    symphony_store::open(&e.db)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER boom BEFORE INSERT ON agents BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    let err = e
        .runtime
        .create_agent(req(
            &e,
            "Se cae la base",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CreateError::Store(m) if m.contains("boom")),
        "{err:?}"
    );
    // La transacción se deshizo entera y el worktree/rama también.
    assert_eq!(count(&e, "projects"), 0);
    assert_eq!(count(&e, "sessions"), 0);
    assert_nothing_created(&e);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn executor_that_cannot_start_leaves_a_failed_agent_with_a_reason() {
    let missing = std::env::temp_dir().join("symphony-no-such-cli-xyz");
    let e = env(WORK, Some(missing)).await;
    let created = e
        .runtime
        .create_agent(req(&e, "Lanzar", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    assert_eq!(created.state, AgentState::Failed);
    let reason = created.state_reason.unwrap();
    assert!(reason.contains("no se pudo lanzar fake-agent"), "{reason}");
    e.writer.handle().flush().await.unwrap();
    let id = created.agent_id.to_string();
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT state || '|' || state_reason FROM agents WHERE id='{id}'")
        ),
        format!("FAILED|{reason}")
    );
    assert_eq!(
        one::<String>(&e, "SELECT status || '|' || end_reason FROM agent_runs"),
        "FAILED|CRASH"
    );
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT kind FROM recovery_items WHERE agent_id='{id}'")
        ),
        "EXECUTOR_EXITED"
    );
    // El workspace y el checkpoint quedan para reintentar.
    assert!(created.worktree.join("README.md").is_file());
    assert_eq!(count(&e, "checkpoints"), 1);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn executor_crash_fails_the_agent_and_opens_recovery() {
    let e = env(
        "[[step]]\nkind = \"say\"\ntext = \"hola\"\n\n[[step]]\nkind = \"crash\"\n",
        None,
    )
    .await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Se va a caer",
            Execution::Exact("fake/smart".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let id = created.agent_id.to_string();
    let state: String = one(
        &e,
        &format!("SELECT state || '|' || state_reason FROM agents WHERE id='{id}'"),
    );
    assert!(state.starts_with("FAILED|fake-agent terminó"), "{state}");
    // La causa que dejó el CLI en stderr queda con el agente.
    assert!(
        state.contains("stderr: fake-agent: error fatal simulado"),
        "{state}"
    );
    assert_eq!(
        one::<String>(&e, "SELECT status || '|' || end_reason FROM agent_runs"),
        "FAILED|CRASH"
    );
    assert_eq!(count(&e, "recovery_items"), 1);
    // Ningún agente con dos runs abiertos.
    assert_eq!(
        one::<i64>(&e, "SELECT COUNT(*) FROM agent_runs WHERE ended_at IS NULL"),
        0
    );
    e.writer.shutdown();
}

/// P06.S2: una sesión deja la conversación y las tool calls coherentes.
#[tokio::test(flavor = "multi_thread")]
async fn session_mirrors_conversation_and_tool_calls() {
    let long = "x".repeat(symphony_daemon::recorder::INLINE_MAX + 100);
    let script = format!(
        r#"
[[step]]
kind = "say"
text = "Primero miro el estado"

[[step]]
kind = "run"
command = ["git", "status", "--short"]

[[step]]
kind = "edit"
path = "src/fix.txt"
content = "ok\n"

[[step]]
kind = "run"
command = ["git", "no-such-subcommand"]

[[step]]
kind = "say"
text = "{long}"
"#
    );
    let e = env(&script, None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Arreglar el build",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let run_id = created.run.unwrap().0.to_string();
    let conn = symphony_store::open_reader(&e.db).unwrap();

    // Conversación: prompt, texto corto y texto largo (al object store), en orden y con su run.
    type Msg = (String, Option<String>, Option<String>, Option<String>);
    let msgs: Vec<Msg> = conn
        .prepare(
            "SELECT role, content, content_object_id, run_id FROM messages ORDER BY created_at, rowid",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let roles: Vec<&str> = msgs.iter().map(|m| m.0.as_str()).collect();
    assert_eq!(roles, ["USER", "ASSISTANT", "ASSISTANT"], "{msgs:?}");
    assert_eq!(msgs[0].1.as_deref(), Some("Arreglar el build"));
    assert_eq!(msgs[1].1.as_deref(), Some("Primero miro el estado"));
    assert!(msgs.iter().all(|m| m.3.as_deref() == Some(run_id.as_str())));
    let (_, content, object, _) = &msgs[2];
    assert!(content.is_none());
    let (kind, hash, refs): (String, String, i64) = conn
        .query_row(
            "SELECT o.kind, o.blob_hash, b.ref_count FROM context_objects o
             JOIN blobs b ON b.hash = o.blob_hash WHERE o.id = ?1",
            [object.as_deref().unwrap()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((kind.as_str(), refs), ("CONVERSATION", 1));
    let stored = ObjectStore::new(e.home.join("objects")).get(&hash).unwrap();
    assert_eq!(stored, long.as_bytes());

    // Tool calls: una por herramienta, todas cerradas, con resultado y comando.
    type Call = (String, Option<String>, String, Option<i32>, i64, bool);
    let calls: Vec<Call> = conn
        .prepare(
            "SELECT tool_name, command, status, exit_code, op_class, finished_at >= started_at
             FROM tool_calls WHERE run_id = ?1 ORDER BY requested_at, rowid",
        )
        .unwrap()
        .query_map([&run_id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let summary: Vec<(&str, Option<&str>, &str)> = calls
        .iter()
        .map(|c| (c.0.as_str(), c.1.as_deref(), c.2.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            ("Bash", Some("git status --short"), "DONE"),
            ("Write", None, "DONE"),
            ("Bash", Some("git no-such-subcommand"), "FAILED"),
        ]
    );
    assert_eq!(calls[0].3, Some(0));
    assert!(calls[2].3.is_some_and(|c| c != 0), "{calls:?}");
    assert!(
        calls.iter().all(|c| c.5),
        "toda tool call cerrada: {calls:?}"
    );
    // P08.S2: `git status` es lectura (clase 1), igual que escribir un archivo.
    assert_eq!((calls[0].4, calls[1].4), (1, 1));
    e.writer.shutdown();
}

// --- P06.S3: checkpoints incrementales --------------------------------------

type CkRow = (
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    String,
);

/// (seq, plan_tail, current_step, next_step, head_commit, diff_object_id, summary_json), por seq.
fn checkpoints_of(e: &Env, agent: &str) -> Vec<CkRow> {
    symphony_store::open_reader(&e.db)
        .unwrap()
        .prepare(
            "SELECT seq, plan_tail, current_step, next_step, head_commit, diff_object_id, summary_json
             FROM checkpoints WHERE agent_id = ?1 ORDER BY seq",
        )
        .unwrap()
        .query_map([agent], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn diff_text(e: &Env, object: &str) -> String {
    let hash: String = one(
        e,
        &format!("SELECT blob_hash FROM context_objects WHERE id = '{object}'"),
    );
    let bytes = ObjectStore::new(e.home.join("objects")).get(&hash).unwrap();
    String::from_utf8(bytes).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn session_leaves_an_up_to_date_checkpoint() {
    let script = r#"
[[step]]
kind = "say"
text = "Plan:\n- [x] leer\n- [ ] correr los tests de auth\nToken de prueba: Bearer sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123"

[[step]]
kind = "edit"
path = "README.md"
content = "demo\narreglado\n"

[[step]]
kind = "edit"
path = "src/nuevo.txt"
content = "nuevo\n"

[[step]]
kind = "run"
command = ["git", "no-such-subcommand"]
"#;
    let e = env(script, None).await;
    // Este test mira los checkpoints del trabajo vivo: sin commit al completar.
    std::fs::create_dir_all(e.repo.join(".symphony")).unwrap();
    std::fs::write(
        e.repo.join(".symphony/project.toml"),
        "[project]\nname = \"demo\"\ndefault_branch = \"main\"\n\n[merge]\ncommit_on_complete = false\n",
    )
    .unwrap();
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Arreglar auth",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let id = created.agent_id.to_string();
    let rows = checkpoints_of(&e, &id);
    assert!(rows.len() >= 2, "hubo checkpoints incrementales: {rows:?}");
    let seqs: Vec<i64> = rows.iter().map(|r| r.0).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");

    let (_, plan_tail, current, next, head, diff, summary) = rows.last().unwrap().clone();
    let base = git(&e.repo, &["rev-parse", "HEAD"]);
    assert_eq!(head, base);
    assert_eq!(next.as_deref(), Some("correr los tests de auth"));
    let plan_tail = plan_tail.unwrap();
    assert!(
        plan_tail.contains("correr los tests de auth"),
        "{plan_tail}"
    );
    assert!(
        !plan_tail.contains("sk-ant-api03"),
        "plan sin redactar: {plan_tail}"
    );
    assert_eq!(current.as_deref(), Some("Bash: git no-such-subcommand"));
    let diff = diff_text(&e, &diff.unwrap());
    assert!(diff.contains("+arreglado"), "{diff}");
    let summary: serde_json::Value = serde_json::from_str(&summary).unwrap();
    let files: Vec<&str> = summary["files_touched"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(files, ["README.md", "src/nuevo.txt"], "{summary}");
    assert_eq!(summary["files_touched"][1]["untracked"], true);
    assert_eq!(summary["commands_run"], 1);
    assert_eq!(summary["last_command"]["ok"], false);
    let failure = summary["failures"][0].as_str().unwrap();
    assert!(
        failure.starts_with("`git no-such-subcommand` falló"),
        "{failure}"
    );
    e.writer.shutdown();
}

#[derive(Debug, Clone)]
enum Op {
    /// Escribe `content` en uno de 3 archivos (el 0 es el README versionado) y avisa la edición.
    Edit {
        file: usize,
        content: u8,
    },
    Say {
        next: bool,
    },
    Command {
        ok: bool,
    },
    Turn,
    /// Evento que no amerita checkpoint.
    Noise,
}

fn op() -> impl proptest::strategy::Strategy<Value = Op> {
    use proptest::prelude::*;
    prop_oneof![
        3 => (0..3usize, 0..3u8).prop_map(|(file, content)| Op::Edit { file, content }),
        1 => any::<bool>().prop_map(|next| Op::Say { next }),
        2 => any::<bool>().prop_map(|ok| Op::Command { ok }),
        1 => Just(Op::Turn),
        1 => Just(Op::Noise),
    ]
}

/// 50 eventos → checkpoints monótonos por `seq`, ninguno referencia objetos
/// inexistentes, refs de blobs coherentes y retención de los usados en handoffs.
async fn run_checkpoint_case(ops: Vec<Op>) {
    use symphony_adapter_common::{AgentEvent, ToolKind};
    use symphony_daemon::bus::{BusEvent, EventSource};
    use symphony_daemon::checkpoint::KEEP;

    let e = env(WORK, None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "Propiedad", Execution::DecideLater))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    let run = symphony_core::RunId::new().to_string();
    // Un run y un handoff que usa el checkpoint inicial: no se puede podar.
    symphony_store::open(&e.db)
        .unwrap()
        .execute_batch(&format!(
            "INSERT INTO agent_runs (id, agent_id, seq, provider_id, model_id, status, started_at)
                 VALUES ('{run}','{agent}',1,'fake','fake/fast','RUNNING',0);
             INSERT INTO handoffs (id, agent_id, checkpoint_id, to_run_id, mode, created_at)
                 SELECT 'h1', agent_id, id, '{run}', 'BALANCED', 0 FROM checkpoints WHERE agent_id='{agent}';"
        ))
        .unwrap();
    let project: String = one(&e, "SELECT id FROM projects");
    let files = ["README.md", "a.txt", "dir/b.txt"];
    let publish = |event| {
        e.bus.publish(BusEvent {
            project_id: project.clone(),
            agent_id: Some(agent.clone()),
            run_id: Some(run.clone()),
            source: EventSource::Hook,
            event,
            occurred_at: 1,
        })
    };
    let mut significant = 0;
    for op in &ops {
        let events = match op {
            Op::Edit { file, content } => {
                let path = created.worktree.join(files[*file]);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                // content 0 en el README = contenido original (diff vacío si no hay más cambios).
                let text = if *content == 0 && *file == 0 {
                    "demo\n".to_string()
                } else {
                    format!("v{content}\n")
                };
                std::fs::write(&path, text).unwrap();
                vec![AgentEvent::FileModified {
                    tool: "Write".into(),
                    path: Some(files[*file].into()),
                }]
            }
            Op::Say { next } => vec![AgentEvent::AssistantText {
                text: if *next {
                    "Siguiente: seguir".into()
                } else {
                    "pensando".into()
                },
            }],
            Op::Command { ok } => vec![
                AgentEvent::ToolRequested {
                    tool_use_id: None,
                    tool: "Bash".into(),
                    kind: ToolKind::Command,
                    command: Some("npm test".into()),
                },
                AgentEvent::ToolFinished {
                    tool_use_id: None,
                    tool: "Bash".into(),
                    kind: ToolKind::Command,
                    ok: *ok,
                    exit_code: Some(i32::from(!ok)),
                },
            ],
            Op::Turn => vec![AgentEvent::TurnFinished { last_message: None }],
            Op::Noise => vec![AgentEvent::TurnStarted],
        };
        for event in events {
            if matches!(
                event,
                AgentEvent::FileModified { .. }
                    | AgentEvent::ToolFinished { .. }
                    | AgentEvent::TurnFinished { .. }
            ) {
                significant += 1;
            }
            publish(event).await.unwrap();
            // Sin agrupar: un checkpoint por evento significativo, para contar exacto.
            e.bus.checkpoints_idle().await;
        }
    }
    e.writer.handle().flush().await.unwrap();

    let rows = checkpoints_of(&e, &agent);
    let seqs: Vec<i64> = rows.iter().map(|r| r.0).collect();
    let created_total = 1 + significant;
    assert_eq!(*seqs.last().unwrap(), created_total, "{seqs:?}");
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
    assert_eq!(seqs[0], 1, "el checkpoint del handoff se conserva");
    let expected = if created_total <= KEEP as i64 {
        created_total
    } else {
        KEEP as i64 + 1
    };
    assert_eq!(rows.len() as i64, expected, "{seqs:?}");

    // Ningún checkpoint referencia objetos inexistentes; refs de blobs coherentes.
    let conn = symphony_store::open_reader(&e.db).unwrap();
    let dangling: i64 = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM checkpoints c WHERE c.diff_object_id IS NOT NULL
                       AND NOT EXISTS (SELECT 1 FROM context_objects o WHERE o.id = c.diff_object_id))
                  + (SELECT COUNT(*) FROM checkpoint_refs r
                       WHERE NOT EXISTS (SELECT 1 FROM context_objects o WHERE o.id = r.object_id))
                  + (SELECT COUNT(*) FROM context_objects o
                       WHERE NOT EXISTS (SELECT 1 FROM blobs b WHERE b.hash = o.blob_hash))",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dangling, 0);
    let orphans: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM context_objects o WHERE o.kind = 'GIT_DIFF'
             AND NOT EXISTS (SELECT 1 FROM checkpoints c WHERE c.diff_object_id = o.id)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orphans, 0, "diffs sin checkpoint quedaron sin podar");
    let bad_refs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM blobs b WHERE b.ref_count <>
                 (SELECT COUNT(*) FROM context_objects o WHERE o.blob_hash = b.hash)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bad_refs, 0, "ref_count de blobs incoherente");
    let store = ObjectStore::new(e.home.join("objects"));
    for (_, _, _, _, _, diff, _) in &rows {
        if let Some(object) = diff {
            let hash: String = one(
                &e,
                &format!("SELECT blob_hash FROM context_objects WHERE id = '{object}'"),
            );
            store.get(&hash).unwrap();
        }
    }
    // El último checkpoint refleja el diff real del worktree.
    let real = git(
        &created.worktree,
        &[
            "diff",
            "--no-ext-diff",
            "--no-color",
            &git(&e.repo, &["rev-parse", "HEAD"]),
        ],
    );
    let last = rows.last().unwrap();
    if created_total > 1 {
        match &last.5 {
            Some(object) => assert_eq!(diff_text(&e, object).trim_end(), real),
            None => assert_eq!(real, ""),
        }
    }
    e.writer.shutdown();
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 4, ..Default::default() })]
    #[test]
    fn checkpoints_stay_monotonic_and_consistent(ops in proptest::collection::vec(op(), 50)) {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run_checkpoint_case(ops));
    }
}

// --- P13.S3: recuperación de checkpoints ------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn handoff_skips_missing_checkpoint_blobs_and_records_recovery_once() {
    let e = env(
        "[[step]]\nkind = \"edit\"\npath = \"README.md\"\ncontent = \"demo\\narreglado\\n\"\n",
        None,
    )
    .await;
    std::fs::create_dir_all(e.repo.join(".symphony")).unwrap();
    std::fs::write(
        e.repo.join(".symphony/project.toml"),
        "[project]\nname = \"demo\"\ndefault_branch = \"main\"\n\n[merge]\ncommit_on_complete = false\n",
    )
    .unwrap();
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Recuperar auth",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let hash: String = one(
        &e,
        "SELECT o.blob_hash FROM checkpoints c JOIN context_objects o ON o.id = c.diff_object_id
         ORDER BY c.seq DESC LIMIT 1",
    );
    let previous: String = one(
        &e,
        &format!(
            "SELECT c.id FROM checkpoints c LEFT JOIN context_objects o ON o.id = c.diff_object_id
             WHERE o.blob_hash IS NULL OR o.blob_hash != '{hash}' ORDER BY c.seq DESC LIMIT 1"
        ),
    );
    let broken: i64 = one(
        &e,
        &format!(
            "SELECT COUNT(*) FROM checkpoints c JOIN context_objects o ON o.id = c.diff_object_id
             WHERE o.blob_hash = '{hash}'"
        ),
    );
    assert!(broken > 0);
    std::fs::remove_file(e.home.join("objects").join(&hash[..2]).join(&hash)).unwrap();

    for _ in 0..2 {
        let h = e
            .runtime
            .prepare_handoff(created.agent_id, "Recuperación")
            .await
            .unwrap();
        assert_eq!(
            h.checkpoint_id.map(|id| id.to_string()),
            Some(previous.clone())
        );
        assert!(h.prompt.contains("+arreglado"), "{}", h.prompt);
        let invalid: i64 = one(&e, "SELECT COUNT(*) FROM checkpoints WHERE is_valid = 0");
        assert_eq!(invalid, broken);
        let items: i64 = one(
            &e,
            "SELECT COUNT(*) FROM recovery_items WHERE kind = 'CHECKPOINT_INVALID'
             AND checkpoint_valid = 0 AND status = 'OPEN'",
        );
        assert_eq!(
            items, broken,
            "la segunda preparación no duplica incidencias"
        );
    }
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_uses_live_worktree_and_task_when_every_checkpoint_has_invalid_json() {
    let e = env(
        "[[step]]\nkind = \"say\"\ntext = \"Trabajo conservado\"\n",
        None,
    )
    .await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Objetivo conservado",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let checkpoints = count(&e, "checkpoints");
    assert!(checkpoints > 0);
    // Simula corrupción persistida: el escritor normal rechaza JSON ilegible.
    symphony_store::open(&e.db)
        .unwrap()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             UPDATE checkpoints SET summary_json = '{incompleto';
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    std::fs::write(
        created.worktree.join("rescatado.txt"),
        "Trabajo sin checkpoint\n",
    )
    .unwrap();

    for _ in 0..2 {
        let h = e
            .runtime
            .prepare_handoff(created.agent_id, "Recuperación")
            .await
            .unwrap();
        assert_eq!(h.checkpoint_id, None);
        assert!(
            h.prompt.contains("## Objetivo\nObjetivo conservado"),
            "{}",
            h.prompt
        );
        assert!(h.prompt.contains("Trabajo sin checkpoint"), "{}", h.prompt);
        assert!(h.prompt.contains("Trabajo conservado"), "{}", h.prompt);
        let invalid: i64 = one(&e, "SELECT COUNT(*) FROM checkpoints WHERE is_valid = 0");
        assert_eq!(invalid, checkpoints);
        let items: i64 = one(
            &e,
            "SELECT COUNT(*) FROM recovery_items WHERE kind = 'CHECKPOINT_INVALID'
             AND checkpoint_valid = 0 AND status = 'OPEN'",
        );
        assert_eq!(items, checkpoints);
    }
    e.writer.shutdown();
}

// --- P06.S4: handoff v1 ------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn handoff_combines_the_checkpoint_with_live_git() {
    let script = r#"
[[step]]
kind = "say"
text = "Voy por partes.\n- [x] leer\n- [ ] correr los tests de auth"

[[step]]
kind = "edit"
path = "README.md"
content = "demo\narreglado\n"

[[step]]
kind = "edit"
path = "src/nuevo.txt"
content = "contenido nuevo\n"

[[step]]
kind = "run"
command = ["git", "no-such-subcommand"]
"#;
    let e = env(script, None).await;
    // Este test mira el estado vivo de git tras el trabajo del agente: sin commit al completar.
    std::fs::create_dir_all(e.repo.join(".symphony")).unwrap();
    std::fs::write(
        e.repo.join(".symphony/project.toml"),
        "[project]\nname = \"demo\"\ndefault_branch = \"main\"\n\n[merge]\ncommit_on_complete = false\n",
    )
    .unwrap();
    let objective = "Arreglar auth";
    let created = e
        .runtime
        .create_agent(req(&e, objective, Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let (run, _) = created.run.clone().unwrap();

    // El primer spawn quedó registrado como handoff sin checkpoint.
    let first: String = one(
        &e,
        &format!(
            "SELECT COALESCE(checkpoint_id,'-') || '|' || mode || '|' || tokens_sent || '|' || tokens_raw_estimate
             FROM handoffs WHERE to_run_id = '{run}'"
        ),
    );
    let tokens = symphony_context::handoff::estimate_tokens(objective);
    assert_eq!(first, format!("-|BALANCED|{tokens}|{tokens}"));

    // Algo que pasó después del último checkpoint: git manda.
    std::fs::write(created.worktree.join("tardio.txt"), "escrito tarde\n").unwrap();

    let h = e
        .runtime
        .prepare_handoff(created.agent_id, "El executor anterior se quedó sin cuota.")
        .await
        .unwrap();
    let latest: String = one(
        &e,
        &format!(
            "SELECT id FROM checkpoints WHERE agent_id = '{}' ORDER BY seq DESC LIMIT 1",
            created.agent_id
        ),
    );
    assert_eq!(h.checkpoint_id.map(|c| c.to_string()), Some(latest));
    assert_eq!(h.mode, ContextMode::Balanced);
    let p = &h.prompt;
    for want in [
        "El executor anterior se quedó sin cuota.",
        "## Objetivo\nArreglar auth",
        "## Qué seguía\ncorrer los tests de auth",
        "$ git no-such-subcommand\n(falló con código",
        "- `git no-such-subcommand` falló",
        " M README.md",
        "?? src/nuevo.txt",
        "+arreglado",
        "--- src/nuevo.txt (archivo nuevo)\ncontenido nuevo",
        "--- tardio.txt (archivo nuevo)\nescrito tarde",
        "## Conversación hasta ahora",
        "**Usuario:** Arreglar auth",
        "**Asistente:** Voy por partes.",
    ] {
        assert!(p.contains(want), "falta {want:?} en:\n{p}");
    }
    assert_eq!(h.tokens_sent, symphony_context::handoff::estimate_tokens(p));
    let sections: Vec<&str> = h.items.iter().map(|i| i.section).collect();
    for want in ["OBJECTIVE", "PLAN", "FAILURES", "DIFF", "CODE", "DECISIONS"] {
        assert!(
            sections.contains(&want),
            "falta la sección {want}: {sections:?}"
        );
    }
    // Nada se recortó: lo que costaría en RAW es lo que se envió.
    assert!(h.tokens_raw_estimate >= h.tokens_sent, "{h:?}");
    e.writer.shutdown();
}

/// P07.5.S3 (criterio de PLAN): con una conversación de 10 turnos, el handoff en modo
/// `raw` lleva todos, en orden, sin omitir nada.
#[tokio::test(flavor = "multi_thread")]
async fn handoff_of_a_ten_turn_conversation_keeps_every_turn_in_raw() {
    let script = "[[step]]\nkind = \"say\"\ntext = \"ok\"\n";
    let e = env(script, None).await;
    let mut r = req(&e, "Charla larga", Execution::Exact("fake/fast".into()));
    r.context_mode = ContextMode::Raw;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    for i in 1..10 {
        e.writer.handle().flush().await.unwrap();
        e.runtime
            .continue_session(created.agent_id, &format!("mensaje {i}"))
            .await
            .unwrap();
        e.runtime.wait_executors().await;
    }
    e.writer.handle().flush().await.unwrap();

    let h = e
        .runtime
        .prepare_handoff(created.agent_id, "cambio")
        .await
        .unwrap();
    let p = &h.prompt;
    assert_eq!(h.mode, ContextMode::Raw);
    assert!(p.contains("**Usuario:** Charla larga"), "{p}");
    let mut at = 0;
    for i in 1..10 {
        let want = format!("**Usuario:** mensaje {i}");
        let found = p[at..]
            .find(&want)
            .unwrap_or_else(|| panic!("falta {want:?} o está fuera de orden en:\n{p}"));
        at += found + want.len();
    }
    assert_eq!(p.matches("**Usuario:**").count(), 10);
    assert_eq!(p.matches("**Asistente:** ok").count(), 10);
    assert!(!p.contains("omitidos"));
    e.writer.shutdown();
}

/// P07.5.S3: el handoff lleva la conversación (usuario y asistente) y no anida el
/// prompt de un handoff anterior, que ya trae esa misma conversación.
#[tokio::test(flavor = "multi_thread")]
async fn handoff_carries_the_conversation_without_nesting_earlier_handoffs() {
    let first = "[[step]]\nkind = \"say\"\ntext = \"primero\"\n[[step]]\nkind = \"hang\"\n";
    let second = "[[step]]\nkind = \"say\"\ntext = \"segundo\"\n";
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Arreglar auth",
            Execution::Exact("alpha/fast".into()),
        ))
        .await
        .unwrap();
    // Esperar a que el primer executor haya dicho algo antes de cambiarlo.
    let started = std::time::Instant::now();
    loop {
        e.writer.handle().flush().await.unwrap();
        if count(&e, "messages WHERE role = 'ASSISTANT'") > 0 {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(15), "sin respuesta");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    e.runtime
        .switch(created.agent_id, "beta/smart")
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let h = e
        .runtime
        .prepare_handoff(created.agent_id, "otro cambio")
        .await
        .unwrap();
    let p = &h.prompt;
    let order: Vec<usize> = [
        "**Usuario:** Arreglar auth",
        "**Asistente:** primero",
        // Sin nada del usuario entre medias, las dos respuestas quedan juntas.
        "

segundo",
    ]
    .iter()
    .map(|want| {
        p.find(want)
            .unwrap_or_else(|| panic!("falta {want:?} en:\n{p}"))
    })
    .collect();
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "fuera de orden:\n{p}"
    );
    assert_eq!(
        p.matches("Retomas una tarea").count(),
        1,
        "el prompt del handoff anterior no debe entrar a la conversación:\n{p}"
    );
    // El cambio de executor dejó su handoff con el detalle de lo que entró y su fidelidad.
    assert!(
        count(
            &e,
            "handoff_items WHERE section = 'OBJECTIVE' AND fidelity = 5"
        ) >= 1
    );
    assert!(count(&e, "handoff_items WHERE section = 'DECISIONS'") >= 1);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn quota_exhausted_hands_the_same_agent_to_the_next_provider() {
    let first = r#"
[[step]]
kind = "edit"
path = "from-alpha.txt"
content = "kept\n"
[[step]]
kind = "say"
text = "Next: continue with beta"
[[step]]
kind = "quota_exhausted"
resets_at = 1790300000
"#;
    let second = r#"
[[step]]
kind = "edit"
path = "from-beta.txt"
content = "continued\n"
"#;
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "continuar tras cuota",
            Execution::Exact("alpha/fast".into()),
        ))
        .await
        .unwrap();

    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let conn = symphony_store::open_reader(&e.db).unwrap();
    let runs: Vec<(String, String, String, Option<String>)> = conn
        .prepare("SELECT provider_id, model_id, status, end_reason FROM agent_runs ORDER BY seq")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        runs,
        vec![
            (
                "alpha".into(),
                "alpha/fast".into(),
                "HANDED_OFF".into(),
                Some("QUOTA_EXHAUSTED".into()),
            ),
            (
                "beta".into(),
                "beta/fast".into(),
                "EXITED".into(),
                Some("COMPLETED".into()),
            ),
        ]
    );
    assert_eq!(count(&e, "agents"), 1);
    assert_eq!(count(&e, "tasks"), 1);
    assert_eq!(count(&e, "worktrees"), 1);
    assert_eq!(count(&e, "provider_failures"), 1);
    assert_eq!(count(&e, "executor_changes"), 1);
    assert_eq!(count(&e, "handoffs"), 2);
    assert_eq!(
        one::<i64>(
            &e,
            "SELECT COUNT(*) FROM handoffs WHERE outcome = 'CONTINUED'"
        ),
        2
    );
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "COMPLETED");
    assert_eq!(one::<String>(&e, "SELECT status FROM tasks"), "DONE");
    assert_eq!(
        one::<String>(&e, "SELECT reason FROM executor_changes"),
        "FAILOVER"
    );
    let separator: String = one(
        &e,
        "SELECT content FROM messages WHERE role = 'EXECUTOR_CHANGE'",
    );
    assert!(separator.contains("alpha/fast → beta/fast"));
    assert!(separator.contains("Agente #1, tarea y workspace sin cambios"));
    assert!(created.worktree.join("from-alpha.txt").is_file());
    assert!(created.worktree.join("from-beta.txt").is_file());
    assert_eq!(
        git(&created.worktree, &["branch", "--show-current"]),
        created.branch
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn disabled_failover_waits_for_a_provider_and_opens_recovery() {
    let script = "[[step]]\nkind = \"quota_exhausted\"\n";
    let e = env(script, None).await;
    let mut request = req(
        &e,
        "esperar tras cuota",
        Execution::Exact("fake/fast".into()),
    );
    request.failover = FailoverPolicy::None;
    e.runtime.create_agent(request).await.unwrap();

    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(count(&e, "agent_runs"), 1);
    assert_eq!(count(&e, "provider_failures"), 1);
    assert_eq!(count(&e, "executor_changes"), 1);
    assert_eq!(count(&e, "recovery_items"), 1);
    assert_eq!(
        one::<String>(&e, "SELECT state FROM agents"),
        "WAITING_PROVIDER"
    );
    assert_eq!(
        one::<String>(&e, "SELECT kind FROM recovery_items"),
        "RATE_LIMITED"
    );
    assert_eq!(
        one::<i64>(
            &e,
            "SELECT COUNT(*) FROM executor_changes WHERE to_run_id IS NULL"
        ),
        1
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn manual_switch_replaces_only_the_executor_and_remembers_the_model() {
    let first = "[[step]]\nkind = \"hang\"\n";
    let second = "[[step]]\nkind = \"edit\"\npath = \"switched.txt\"\ncontent = \"ok\\n\"\n";
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "cambio manual",
            Execution::Exact("alpha/fast".into()),
        ))
        .await
        .unwrap();
    let new_run = e
        .runtime
        .switch(created.agent_id, "beta/smart")
        .await
        .unwrap();

    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(count(&e, "agents"), 1);
    assert_eq!(count(&e, "tasks"), 1);
    assert_eq!(count(&e, "worktrees"), 1);
    assert_eq!(count(&e, "agent_runs"), 2);
    assert_eq!(count(&e, "executor_changes"), 1);
    assert_eq!(
        one::<i64>(
            &e,
            "SELECT COUNT(*) FROM handoffs WHERE outcome = 'CONTINUED'"
        ),
        2
    );
    let conn = symphony_store::open_reader(&e.db).unwrap();
    let changed: (String, String, String) = conn
        .query_row(
            "SELECT reason, from_run_id, to_run_id FROM executor_changes",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(changed.0, "USER_SWITCH");
    assert_eq!(changed.2, new_run.to_string());
    assert_ne!(changed.1, changed.2);
    assert_eq!(
        one::<String>(&e, "SELECT requested_model_id FROM agents"),
        "beta/smart"
    );
    assert!(created.worktree.join("switched.txt").is_file());
    assert_eq!(
        git(&created.worktree, &["branch", "--show-current"]),
        created.branch
    );
    e.writer.shutdown();
}

/// P07.5.S6, Journey de chat: empieza en un proveedor, cambia a otro a mitad con un mensaje
/// nuevo y el segundo recibe la conversación sin que el usuario repita nada.
#[tokio::test(flavor = "multi_thread")]
async fn chat_switches_provider_mid_conversation_with_a_new_message() {
    let first = "[[step]]\nkind = \"edit\"\npath = \"alpha.txt\"\ncontent = \"a\\n\"\n";
    let second = "[[step]]\nkind = \"edit\"\npath = \"beta.txt\"\ncontent = \"b\\n\"\n";
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let mut r = req(
        &e,
        "Empieza el arreglo",
        Execution::Exact("alpha/fast".into()),
    );
    r.chat = true;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "READY");

    e.runtime
        .switch_with_message(created.agent_id, "beta/fast", Some("ahora sigue tú"))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "READY");
    assert_eq!(one::<i64>(&e, "SELECT COUNT(*) FROM agents"), 1);
    assert_eq!(
        one::<String>(&e, "SELECT reason FROM executor_changes"),
        "USER_SWITCH"
    );
    assert_eq!(
        git(&e.repo, &["log", "--format=%s", "symphony/chat"]),
        "chat: turno 2 (beta/fast)\nchat: turno 1 (alpha/fast)\ninit"
    );
    // El mensaje nuevo llegó al modelo (va en su prompt) y quedó como `USER` aparte.
    // Dos mensajes `USER` del run (el prompt con el handoff y el mensaje nuevo, aparte) pueden
    // crearse en el mismo milisegundo: el orden por id no es estable (falló en macOS), así que
    // se mira el conjunto.
    let sent: String = one(
        &e,
        "SELECT group_concat(content, char(10)) FROM messages WHERE role = 'USER' AND run_id =
            (SELECT id FROM agent_runs WHERE provider_id = 'beta')",
    );
    assert!(sent.contains("ahora sigue tú"), "{sent}");
    assert!(sent.contains("Empieza el arreglo"), "{sent}");
    let h = e
        .runtime
        .prepare_handoff(created.agent_id, "otro cambio")
        .await
        .unwrap();
    assert!(
        h.prompt.contains("**Usuario:** Empieza el arreglo"),
        "{}",
        h.prompt
    );
    assert!(
        h.prompt.contains("**Usuario:** ahora sigue tú"),
        "{}",
        h.prompt
    );
    assert_eq!(h.prompt.matches("**Usuario:**").count(), 2, "{}", h.prompt);
    e.writer.shutdown();
}

/// P07.5.S6: política por umbral. Apagada, el chat sigue en su proveedor; encendida y con el
/// último turno por encima del umbral, el siguiente mensaje pasa al otro proveedor.
#[tokio::test(flavor = "multi_thread")]
async fn chat_rotates_provider_only_when_the_token_threshold_is_on_and_reached() {
    let first = "[[step]]\nkind = \"usage\"\ntokens = 5000\n";
    let second = "[[step]]\nkind = \"edit\"\npath = \"beta.txt\"\ncontent = \"b\\n\"\n";
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let mut r = req(&e, "Hola", Execution::Exact("alpha/fast".into()));
    r.chat = true;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let providers = |e: &Env| -> String {
        one(
            e,
            "SELECT GROUP_CONCAT(provider_id, ',') FROM (SELECT provider_id FROM agent_runs ORDER BY seq)",
        )
    };

    // Apagada (por defecto): se reanuda la sesión de alpha.
    e.runtime
        .continue_session(created.agent_id, "sigue")
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(providers(&e), "alpha,alpha");

    // Encendida: el último turno de alpha usó 5000 ≥ 1000, el mensaje va a beta.
    e.runtime.set_chat_switch_tokens(Some(1000));
    e.runtime
        .continue_session(created.agent_id, "y ahora")
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(providers(&e), "alpha,alpha,beta");
    assert_eq!(
        one::<String>(&e, "SELECT reason FROM executor_changes"),
        "FAILOVER"
    );
    assert_eq!(one::<i64>(&e, "SELECT COUNT(*) FROM provider_failures"), 0);
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "READY");

    // Beta no reportó uso: el siguiente mensaje se queda en beta (no rebota).
    e.runtime
        .continue_session(created.agent_id, "otra más")
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(providers(&e), "alpha,alpha,beta,beta");
    e.writer.shutdown();
}

/// P07.5.S6: cuota agotada en el chat → failover con los parsers existentes y, al terminar,
/// el chat sigue abierto (`READY`) y con su turno commiteado.
#[tokio::test(flavor = "multi_thread")]
async fn chat_fails_over_on_quota_and_keeps_waiting_for_messages() {
    let first = "[[step]]\nkind = \"say\"\ntext = \"voy\"\n[[step]]\nkind = \"quota_exhausted\"\nresets_at = 1790300000\n";
    let second = "[[step]]\nkind = \"edit\"\npath = \"beta.txt\"\ncontent = \"b\\n\"\n";
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let mut r = req(
        &e,
        "Arregla el login",
        Execution::Exact("alpha/fast".into()),
    );
    r.chat = true;
    e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "READY");
    assert_eq!(one::<String>(&e, "SELECT status FROM tasks"), "READY");
    assert_eq!(
        one::<String>(&e, "SELECT reason FROM executor_changes"),
        "FAILOVER"
    );
    assert_eq!(one::<i64>(&e, "SELECT COUNT(*) FROM provider_failures"), 1);
    assert_eq!(
        git(&e.repo, &["log", "--format=%s", "-1", "symphony/chat"]),
        "chat: turno 1 (beta/fast)"
    );
    e.writer.shutdown();
}

/// Auditoría P13.S5, hallazgo 1: tras un hijo que gastó mucha CPU y terminó, un comando callado
/// que sigue también late (la comparación es contra la muestra anterior, no contra el máximo).
#[tokio::test(flavor = "multi_thread")]
async fn cpu_keeps_beating_after_a_busy_child_ends() {
    let dir = tempfile::tempdir().unwrap();
    let child = dir.path().join("hijo.toml");
    std::fs::write(&child, "[[step]]\nkind = \"busy\"\nms = 3000\n").unwrap();
    let script = format!(
        "[[step]]\nkind = \"run\"\ncommand = ['{}', 'run', '--script', '{}']\n\n[[step]]\nkind = \"busy\"\nms = 3000\n\n[[step]]\nkind = \"say\"\ntext = \"listo\"\n",
        fake_agent().display(),
        child.display()
    );
    let e = env_with_watchdog(
        &[("fake", &script)],
        None,
        Duration::from_millis(100),
        Duration::from_millis(1500),
    )
    .await;
    e.runtime
        .create_agent(req(
            &e,
            "compilar dos veces",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(
        one::<String>(&e, "SELECT end_reason FROM agent_runs"),
        "COMPLETED"
    );
    e.writer.shutdown();
}

/// Auditoría P13.S5, hallazgo 7: un agente pausado (árbol congelado, sin salida ni CPU) no es un
/// cuelgue: el watchdog no lo mata aunque pase más de `stale_after`, y `resume` sigue valiendo.
#[tokio::test(flavor = "multi_thread")]
async fn a_paused_executor_is_not_killed_by_the_watchdog() {
    let script = "[[step]]\nkind = \"say\"\ntext = \"arranca\"\n\n[[step]]\nkind = \"hang\"\n";
    let e = env_with_watchdog(
        &[("fake", script)],
        None,
        Duration::from_millis(100),
        Duration::from_millis(1000),
    )
    .await;
    let created = e
        .runtime
        .create_agent(req(&e, "pausar", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.writer.handle().flush().await.unwrap();
    e.runtime.pause(created.agent_id).await.unwrap();
    // Tres veces `stale_after` congelado: sin el arreglo, el watchdog ya lo habría cerrado.
    tokio::time::sleep(Duration::from_millis(3000)).await;
    assert!(
        e.runtime.is_live(created.agent_id),
        "el watchdog mató a un agente pausado"
    );
    e.runtime.resume(created.agent_id).await.unwrap();
    e.runtime.stop(created.agent_id).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.shutdown();
}

/// Auditoría P13.S5, hallazgo 5: si la base deja de aceptar escrituras, el pump mata el árbol y
/// termina en vez de esperar para siempre a un CLI que sigue vivo.
#[tokio::test(flavor = "multi_thread")]
async fn a_pump_that_cannot_publish_kills_the_cli_instead_of_hanging() {
    let script = "[[step]]\nkind = \"sleep\"\nms = 800\n\n[[step]]\nkind = \"say\"\ntext = \"tarde\"\n\n[[step]]\nkind = \"hang\"\n";
    let e = env(script, None).await;
    e.runtime
        .create_agent(req(&e, "sin base", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.writer.shutdown();
    tokio::time::timeout(Duration::from_secs(20), e.runtime.wait_executors())
        .await
        .expect("el pump quedó colgado esperando al CLI");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_silent_executor_that_burns_cpu_is_not_taken_for_hung() {
    let script = "[[step]]
kind = \"busy\"
ms = 4000

[[step]]
kind = \"say\"
text = \"listo\"
";
    let e = env_with_watchdog(
        &[("fake", script)],
        None,
        Duration::from_millis(100),
        Duration::from_millis(1500),
    )
    .await;
    e.runtime
        .create_agent(req(&e, "compilar", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(
        one::<String>(&e, "SELECT end_reason FROM agent_runs"),
        "COMPLETED",
        "4 s callado pero con CPU: trabaja, no está colgado"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn hung_executor_is_failed_and_reclaim_keeps_its_workspace() {
    let script = r#"
[[step]]
kind = "edit"
path = "before-hang.txt"
content = "safe\n"
[[step]]
kind = "hang"
"#;
    let e = env_with_watchdog(
        &[("fake", script)],
        None,
        Duration::from_millis(100),
        Duration::from_millis(1500),
    )
    .await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "recuperar cuelgue",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();

    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(
        one::<String>(&e, "SELECT end_reason FROM agent_runs"),
        "NO_HEARTBEAT"
    );
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "FAILED");
    assert_eq!(
        one::<String>(&e, "SELECT kind FROM recovery_items"),
        "NO_HEARTBEAT"
    );
    assert!(one::<Option<i64>>(&e, "SELECT last_heartbeat_at FROM agent_runs").is_some());
    assert!(created.worktree.join("before-hang.txt").is_file());

    e.runtime.reclaim(created.agent_id).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(count(&e, "agent_runs"), 2);
    assert_eq!(
        one::<String>(&e, "SELECT reason FROM executor_changes"),
        "RECLAIM"
    );
    assert_eq!(
        one::<i64>(
            &e,
            "SELECT COUNT(*) FROM recovery_items WHERE status = 'RESOLVED' AND resolution = 'RECLAIM'"
        ),
        1
    );
    assert!(created.worktree.join("before-hang.txt").is_file());
    e.writer.shutdown();
}

/// F5 (`docs/research/uso-signalia.md`): un run que se queda mudo poco después de un rate limit
/// murió por el rate limit, no «sin latido»; el Recovery Center lo dice así.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_that_goes_silent_after_a_rate_limit_is_closed_with_that_cause() {
    let script = r#"
[[step]]
kind = "say"
text = "trabajando"
[[step]]
kind = "rate_limit"
retry_after_ms = 60000
[[step]]
kind = "hang"
"#;
    let e = env_with_watchdog(
        &[("fake", script)],
        None,
        Duration::from_millis(100),
        Duration::from_millis(1500),
    )
    .await;
    let created = e
        .runtime
        .create_agent(req(&e, "limitado", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(
        one::<String>(&e, "SELECT end_reason FROM agent_runs"),
        "RATE_LIMITED"
    );
    assert_eq!(one::<String>(&e, "SELECT status FROM agent_runs"), "FAILED");
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "FAILED");
    assert_eq!(
        one::<String>(&e, "SELECT kind FROM recovery_items"),
        "RATE_LIMITED"
    );
    let detail = one::<String>(&e, "SELECT detail FROM recovery_items");
    assert!(detail.contains("rate limit"), "{detail}");
    assert!(created.worktree.is_dir());

    // F6: `resume` lo reabre con el mismo modelo, sin `switch`.
    e.runtime.resume(created.agent_id).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(count(&e, "agent_runs"), 2);
    assert_eq!(
        one::<String>(&e, "SELECT model_id FROM agent_runs WHERE seq = 2"),
        "fake/fast"
    );
    e.writer.shutdown();
}

/// F6: `resume` también reabre un agente que salió con error.
#[tokio::test(flavor = "multi_thread")]
async fn resume_reopens_a_crashed_agent_with_the_same_model() {
    let e = env("[[step]]\nkind = \"crash\"\n", None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "crash", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "FAILED");
    assert_eq!(
        one::<String>(&e, "SELECT end_reason FROM agent_runs"),
        "CRASH"
    );

    e.runtime.resume(created.agent_id).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(count(&e, "agent_runs"), 2);
    assert_eq!(
        one::<String>(&e, "SELECT reason FROM executor_changes"),
        "RESTART"
    );
    // El segundo run también se cae (es el mismo guion): el agente queda otra vez `FAILED`.
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "FAILED");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn crashed_executor_can_restart_from_its_checkpoint() {
    let e = env("[[step]]\nkind = \"crash\"\n", None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "reiniciar crash",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;

    e.runtime.restart(created.agent_id).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(count(&e, "agent_runs"), 2);
    assert_eq!(
        one::<String>(&e, "SELECT reason FROM executor_changes"),
        "RESTART"
    );
    assert_eq!(
        one::<i64>(
            &e,
            "SELECT COUNT(*) FROM recovery_items WHERE status = 'RESOLVED' AND resolution = 'RESTART'"
        ),
        1
    );
    assert_eq!(
        git(&created.worktree, &["branch", "--show-current"]),
        created.branch
    );
    e.writer.shutdown();
}

/// «Abrir en el CLI» (ADR-0005): nunca con el executor vivo ni sin sesión;
/// terminado el turno, la sesión del último run en su worktree.
#[tokio::test(flavor = "multi_thread")]
async fn attach_opens_the_last_cli_session_only_when_idle() {
    let script = r#"
[[step]]
kind = "say"
text = "trabajando"
[[step]]
kind = "sleep"
ms = 1500
"#;
    let e = env(script, None).await;
    let waiting = e
        .runtime
        .create_agent(req(&e, "sin modelo", Execution::DecideLater))
        .await
        .unwrap();
    let err = e.runtime.attach_spec(waiting.agent_id).unwrap_err().0;
    assert!(err.contains("todavía no tiene una sesión"), "{err}");

    let created = e
        .runtime
        .create_agent(req(&e, "con modelo", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    let err = e.runtime.attach_spec(created.agent_id).unwrap_err().0;
    assert!(err.contains("está trabajando"), "{err}");

    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let session: String = one(
        &e,
        &format!(
            "SELECT cli_session_id FROM agent_runs WHERE agent_id = '{}'",
            created.agent_id
        ),
    );
    let (spec, cli) = e.runtime.attach_spec(created.agent_id).unwrap();
    assert_eq!(spec.program, fake_agent());
    assert_eq!(spec.args[0], "attach");
    assert_eq!(spec.args[1].to_string_lossy(), session);
    assert_eq!(spec.cwd.as_deref(), Some(created.worktree.as_path()));
    assert!(!cli.is_empty());

    e.runtime
        .note_attached(created.agent_id, cli)
        .await
        .unwrap();
    e.writer.handle().flush().await.unwrap();
    let note: String = one(&e, "SELECT content FROM messages WHERE role = 'SYSTEM'");
    assert!(note.contains("Sesión abierta en"), "{note}");
    e.writer.shutdown();
}

/// ADR-0009: un CLI que da su id de sesión solo por stderr (Kimi) lo deja guardado y
/// permite retomar la sesión con ese mismo id.
#[tokio::test(flavor = "multi_thread")]
async fn session_id_from_stderr_is_stored_and_resumed() {
    let script = "[[step]]
kind = \"say\"
text = \"listo\"
";
    let e = env_with(&[("fake-stderr", script)], None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "id por stderr",
            Execution::Exact("fake-stderr/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id;
    let first: String = one(
        &e,
        &format!("SELECT cli_session_id FROM agent_runs WHERE agent_id = '{agent}'"),
    );
    assert!(
        first.starts_with("fake-"),
        "id de stderr sin guardar: {first}"
    );

    let run = e.runtime.continue_session(agent, "sigue").await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let second: String = one(
        &e,
        &format!("SELECT cli_session_id FROM agent_runs WHERE id = '{run}'"),
    );
    assert_eq!(second, first, "el run nuevo debe retomar la misma sesión");
    e.writer.shutdown();
}

/// P07.5.S1: un mensaje después del turno retoma la sesión del CLI (mismo modelo y
/// mismo session id, sin handoff) y reabre al agente `COMPLETED`.
#[tokio::test(flavor = "multi_thread")]
async fn message_after_the_turn_resumes_the_cli_session() {
    let script = "[[step]]\nkind = \"say\"\ntext = \"listo\"\n";
    let e = env(script, None).await;
    let waiting = e
        .runtime
        .create_agent(req(&e, "sin modelo", Execution::DecideLater))
        .await
        .unwrap();
    let err = e
        .runtime
        .continue_session(waiting.agent_id, "hola")
        .await
        .unwrap_err()
        .0;
    assert!(err.contains("todavía no tiene una sesión"), "{err}");

    let created = e
        .runtime
        .create_agent(req(&e, "con modelo", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id;
    let state =
        |e: &Env| one::<String>(e, &format!("SELECT state FROM agents WHERE id = '{agent}'"));
    assert_eq!(state(&e), "COMPLETED");
    let first: String = one(
        &e,
        &format!("SELECT cli_session_id FROM agent_runs WHERE agent_id = '{agent}'"),
    );

    let run = e.runtime.continue_session(agent, "sigue").await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(
        one::<i64>(
            &e,
            &format!("SELECT COUNT(*) FROM agent_runs WHERE agent_id = '{agent}'")
        ),
        2
    );
    let second: String = one(
        &e,
        &format!("SELECT cli_session_id FROM agent_runs WHERE id = '{run}'"),
    );
    assert_eq!(second, first, "el run nuevo debe continuar la misma sesión");
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT model_id FROM agent_runs WHERE id = '{run}'")
        ),
        "fake/fast"
    );
    assert_eq!(state(&e), "COMPLETED");
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT status FROM tasks WHERE id = (SELECT task_id FROM agents WHERE id = '{agent}')"
            )
        ),
        "DONE"
    );
    // Sin handoff ni cambio de executor: la conversación sigue en el CLI.
    assert_eq!(count(&e, "executor_changes"), 0);
    assert_eq!(
        one::<i64>(
            &e,
            &format!("SELECT COUNT(*) FROM handoffs WHERE to_run_id = '{run}'")
        ),
        0
    );
    let said: i64 = one(
        &e,
        &format!(
            "SELECT COUNT(*) FROM messages WHERE agent_id = '{agent}' AND role = 'USER' AND content = 'sigue'"
        ),
    );
    assert_eq!(said, 1);
    e.writer.shutdown();
}

/// Un CLI lento en imprimir su primera línea no es un cuelgue: la inactividad
/// se mide desde el arranque (antes moría como NO_HEARTBEAT en el primer tick).
#[tokio::test(flavor = "multi_thread")]
async fn slow_starting_executor_is_not_taken_for_hung() {
    let script = r#"
startup_delay_ms = 600
[[step]]
kind = "edit"
path = "slow.txt"
content = "ok\n"
[[step]]
kind = "say"
text = "listo"
"#;
    let e = env_with_watchdog(
        &[("fake", script)],
        None,
        Duration::from_millis(100),
        Duration::from_millis(1500),
    )
    .await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "arranque lento",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(
        one::<String>(&e, "SELECT end_reason FROM agent_runs"),
        "COMPLETED"
    );
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "COMPLETED");
    assert!(created.worktree.join("slow.txt").is_file());
    e.writer.shutdown();
}

/// P06.S8: Prueba de aceptación forced kill (IDEA §8, Test D).
/// fake-agent A trabaja y se cuelga/muere sin cleanup;
/// fake-agent B continúa solo con el handoff estructurado;
/// se verifica que la tarea se completa y todos los artefactos y estados quedan íntegros.
#[tokio::test(flavor = "multi_thread")]
async fn forced_kill_test_d_acceptance_test() {
    let script_a = r#"
[[step]]
kind = "edit"
path = "email.js"
content = "export function validateEmail(e) { return e.includes('@'); }\n"
[[step]]
kind = "edit"
path = "password.js"
content = "export function hashPassword(p) { return 'hash:' + p; }\n"
[[step]]
kind = "say"
text = "Creados módulos email y password.\nNext: implementar UserStore en users.js y tests en users_test.js"
[[step]]
kind = "hang"
"#;
    let script_b = r#"
[[step]]
kind = "edit"
path = "users.js"
content = "import { validateEmail } from './email.js';\nexport class UserStore { constructor() { this.users = []; } }\n"
[[step]]
kind = "edit"
path = "users_test.js"
content = "import { UserStore } from './users.js';\n// tests passed\n"
[[step]]
kind = "say"
text = "Tarea completada con éxito: módulo users y tests agregados."
"#;

    let e = env_with_watchdog(
        &[("claude", script_a), ("codex", script_b)],
        None,
        Duration::from_millis(100),
        Duration::from_millis(1500),
    )
    .await;

    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Implementar sistema de usuarios con autenticación",
            Execution::Exact("claude/fast".into()),
        ))
        .await
        .unwrap();

    // 1. fake-agent A trabaja, crea archivos y se cuelga. El watchdog lo mata forzosamente.
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    // Comprobamos que el run 1 murió por NO_HEARTBEAT y el agente quedó en FAILED
    assert_eq!(
        one::<String>(&e, "SELECT end_reason FROM agent_runs WHERE seq = 1"),
        "NO_HEARTBEAT"
    );
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "FAILED");
    assert!(created.worktree.join("email.js").is_file());
    assert!(created.worktree.join("password.js").is_file());

    // 2. fake-agent B (Codex) retoma el trabajo mediante switch con handoff automático
    let _new_run = e
        .runtime
        .switch(created.agent_id, "codex/fast")
        .await
        .unwrap();

    // 3. fake-agent B corre con el handoff recibido y completa la tarea
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    // 4. Verificaciones completas de aceptación (Test D)
    assert_eq!(one::<String>(&e, "SELECT state FROM agents"), "COMPLETED");
    assert_eq!(one::<String>(&e, "SELECT status FROM tasks"), "DONE");
    assert_eq!(count(&e, "agent_runs"), 2);
    assert_eq!(count(&e, "executor_changes"), 1);

    let conn = symphony_store::open_reader(&e.db).unwrap();
    let runs: Vec<(String, String, String, Option<String>)> = conn
        .prepare("SELECT provider_id, model_id, status, end_reason FROM agent_runs ORDER BY seq")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        runs,
        vec![
            (
                "claude".into(),
                "claude/fast".into(),
                "FAILED".into(),
                Some("NO_HEARTBEAT".into()),
            ),
            (
                "codex".into(),
                "codex/fast".into(),
                "EXITED".into(),
                Some("COMPLETED".into()),
            ),
        ]
    );

    // Los handoffs registran el traspaso
    assert_eq!(count(&e, "handoffs"), 2);
    assert_eq!(
        one::<i64>(
            &e,
            "SELECT COUNT(*) FROM handoffs WHERE outcome = 'CONTINUED'"
        ),
        1
    );

    // Los mensajes registran los prompts enviados a cada run
    let user_messages: Vec<String> = conn
        .prepare("SELECT content FROM messages WHERE role = 'USER' ORDER BY created_at")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(user_messages.len(), 2);
    let handoff_prompt = &user_messages[1];
    assert!(
        handoff_prompt.contains("Implementar sistema de usuarios con autenticación"),
        "debe contener el objetivo"
    );
    assert!(
        handoff_prompt.contains("implementar UserStore en users.js y tests en users_test.js"),
        "debe contener el qué seguía extraído del último mensaje"
    );
    assert!(
        handoff_prompt.contains("email.js") && handoff_prompt.contains("password.js"),
        "debe reflejar los archivos creados por el agente A"
    );

    // Todos los archivos existen en el worktree final
    assert!(created.worktree.join("email.js").is_file());
    assert!(created.worktree.join("password.js").is_file());
    assert!(created.worktree.join("users.js").is_file());
    assert!(created.worktree.join("users_test.js").is_file());

    // La rama de git se mantuvo consistente
    assert_eq!(
        git(&created.worktree, &["branch", "--show-current"]),
        created.branch
    );

    e.writer.shutdown();
}

/// Los cinco proveedores de v0.1 + P11, con sus ids reales.
const MATRIX: [&str; 5] = ["anthropic", "openai", "moonshot", "google", "github"];

const MATRIX_A: &str = r#"
[[step]]
kind = "edit"
path = "email.js"
content = "export function validateEmail(e) { return e.includes('@'); }\n"
[[step]]
kind = "edit"
path = "password.js"
content = "export function hashPassword(p) { return 'hash:' + p; }\n"
[[step]]
kind = "say"
text = "Creados módulos email y password.\nNext: implementar UserStore en users.js y tests en users_test.js"
[[step]]
kind = "hang"
"#;

const MATRIX_B: &str = r#"
[[step]]
kind = "edit"
path = "users.js"
content = "import { validateEmail } from './email.js';\nexport class UserStore { constructor() { this.users = []; } }\n"
[[step]]
kind = "edit"
path = "users_test.js"
content = "import { UserStore } from './users.js';\n// tests passed\n"
[[step]]
kind = "say"
text = "Tarea completada con éxito: módulo users y tests agregados."
"#;

/// P11.S5: forced kill cruzado (Test D) del proveedor `from` hacia cada uno de los otros cuatro.
/// A trabaja y se cuelga; el watchdog lo mata sin cleanup; B continúa solo con el handoff.
async fn handoff_matrix_from(from: &'static str) {
    let targets: Vec<&'static str> = MATRIX.iter().copied().filter(|p| *p != from).collect();
    let mut providers: Vec<(&'static str, &str)> = vec![(from, MATRIX_A)];
    providers.extend(targets.iter().map(|t| (*t, MATRIX_B)));
    let e = env_with_watchdog(
        &providers,
        None,
        Duration::from_millis(100),
        Duration::from_millis(1500),
    )
    .await;

    for to in targets {
        let pair = format!("{from} -> {to}");
        let title = format!("Sistema de usuarios {from}-{to}");
        let created = e
            .runtime
            .create_agent(req(&e, &title, Execution::Exact(format!("{from}/fast"))))
            .await
            .unwrap();
        e.runtime.wait_executors().await;
        e.writer.handle().flush().await.unwrap();
        let agent = created.agent_id;
        let q = |sql: &str| sql.replace("{agent}", &agent.to_string());

        assert_eq!(
            one::<String>(
                &e,
                &q("SELECT end_reason FROM agent_runs WHERE agent_id = '{agent}' AND seq = 1")
            ),
            "NO_HEARTBEAT",
            "{pair}: el run de origen debe morir por NO_HEARTBEAT"
        );
        assert_eq!(
            one::<String>(&e, &q("SELECT state FROM agents WHERE id = '{agent}'")),
            "FAILED",
            "{pair}"
        );

        e.runtime
            .switch(agent, &format!("{to}/fast"))
            .await
            .unwrap_or_else(|err| panic!("{pair}: el cambio falló: {}", err.0));
        e.runtime.wait_executors().await;
        e.writer.handle().flush().await.unwrap();

        assert_eq!(
            one::<String>(&e, &q("SELECT state FROM agents WHERE id = '{agent}'")),
            "COMPLETED",
            "{pair}: B debe terminar la tarea"
        );
        let conn = symphony_store::open_reader(&e.db).unwrap();
        let runs: Vec<(String, String, Option<String>)> = conn
            .prepare(&q(
                "SELECT provider_id, status, end_reason FROM agent_runs WHERE agent_id = '{agent}' ORDER BY seq",
            ))
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            runs,
            vec![
                (from.into(), "FAILED".into(), Some("NO_HEARTBEAT".into())),
                (to.into(), "EXITED".into(), Some("COMPLETED".into())),
            ],
            "{pair}"
        );
        assert_eq!(
            one::<i64>(
                &e,
                &q(
                    "SELECT COUNT(*) FROM handoffs WHERE outcome = 'CONTINUED' AND agent_id = '{agent}'"
                )
            ),
            1,
            "{pair}: un handoff continuado"
        );

        // Lo que B recibió: solo el handoff, con objetivo, el qué seguía y los archivos de A.
        let prompts: Vec<String> = conn
            .prepare(&q(
                "SELECT content FROM messages WHERE role = 'USER' AND agent_id = '{agent}' ORDER BY created_at",
            ))
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(prompts.len(), 2, "{pair}");
        let handoff = &prompts[1];
        assert!(handoff.contains(&title), "{pair}: falta el objetivo");
        assert!(
            handoff.contains("implementar UserStore en users.js y tests en users_test.js"),
            "{pair}: falta el qué seguía"
        );
        assert!(
            handoff.contains("email.js") && handoff.contains("password.js"),
            "{pair}: faltan los archivos de A"
        );
        for f in ["email.js", "password.js", "users.js", "users_test.js"] {
            assert!(created.worktree.join(f).is_file(), "{pair}: falta {f}");
        }
        assert_eq!(
            git(&created.worktree, &["branch", "--show-current"]),
            created.branch,
            "{pair}"
        );
    }
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_matrix_from_anthropic() {
    handoff_matrix_from("anthropic").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_matrix_from_openai() {
    handoff_matrix_from("openai").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_matrix_from_moonshot() {
    handoff_matrix_from("moonshot").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_matrix_from_google() {
    handoff_matrix_from("google").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_matrix_from_github() {
    handoff_matrix_from("github").await;
}

/// P10.S2: la salud del proveedor sigue a lo que pasa en sus runs.
fn health_of(e: &Env, provider: &str) -> (String, String, Option<f64>, Option<i64>, Option<i64>) {
    symphony_store::open_reader(&e.db)
        .unwrap()
        .query_row(
            "SELECT state, quota_certainty, quota_remaining, retry_after_at, reset_at
             FROM provider_health WHERE provider_id = ?1 AND model_id IS NULL",
            [provider],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[tokio::test(flavor = "multi_thread")]
async fn a_429_rate_limits_the_provider_and_never_exhausts_it() {
    // Un 429 y el CLI se cuelga: el watchdog lo mata y no hay éxito que limpie el estado.
    let script =
        "[[step]]\nkind = \"rate_limit\"\nretry_after_ms = 30\n[[step]]\nkind = \"hang\"\n";
    let e = env_with_watchdog(
        &[("alpha", script)],
        None,
        Duration::from_millis(100),
        Duration::from_millis(1500),
    )
    .await;
    e.runtime
        .create_agent(req(&e, "limitado", Execution::Exact("alpha/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let (state, certainty, remaining, retry_after, reset) = health_of(&e, "alpha");
    assert_eq!(state, "RATE_LIMITED", "un 429 no es «agotado»");
    assert_eq!(
        (certainty.as_str(), remaining, reset),
        ("UNKNOWN", None, None)
    );
    assert!(retry_after.is_some(), "con la espera que pidió el CLI");
    // Queda anotado como fallo temporal, con la cuenta `default`.
    assert_eq!(
        one::<String>(
            &e,
            "SELECT failure_type || '|' || account_id FROM provider_failures"
        ),
        "TEMP_RATE_LIMIT|acct-alpha"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn exhausted_quota_marks_the_provider_and_the_replacement_stays_healthy() {
    let reset_secs = now_secs() + 3600;
    let first = format!("[[step]]\nkind = \"quota_exhausted\"\nresets_at = {reset_secs}\n");
    let second = "[[step]]\nkind = \"say\"\ntext = \"listo\"\n";
    let e = env_with(&[("alpha", first.as_str()), ("beta", second)], None).await;
    e.runtime
        .create_agent(req(&e, "agotado", Execution::Exact("alpha/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let (state, _, _, _, reset) = health_of(&e, "alpha");
    assert_eq!(state, "EXHAUSTED");
    assert_eq!(
        reset,
        Some(reset_secs * 1000),
        "la base guarda milisegundos"
    );
    assert_eq!(
        health_of(&e, "beta").0,
        "HEALTHY",
        "el reemplazo terminó bien"
    );
    assert_eq!(
        one::<String>(&e, "SELECT failure_type FROM provider_failures"),
        "DAILY_QUOTA"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_known_quota_report_below_the_reserve_marks_quota_low_with_a_real_percentage() {
    let script = format!(
        "[[step]]\nkind = \"quota\"\nwindow = \"five_hour\"\nused_fraction = 0.3\nresets_at = {r}\n\
         [[step]]\nkind = \"quota\"\nwindow = \"seven_day\"\nused_fraction = 0.9\nresets_at = {r}\n\
         [[step]]\nkind = \"say\"\ntext = \"listo\"\n",
        r = now_secs() + 7200
    );
    let e = env_with(&[("alpha", script.as_str())], None).await;
    e.runtime
        .create_agent(req(&e, "cuota", Execution::Exact("alpha/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let (state, certainty, remaining, _, _) = health_of(&e, "alpha");
    // Manda la ventana más apretada (0.9 usado → 0.1 restante ≤ reserva 0.20); un éxito no la borra.
    assert_eq!(state, "QUOTA_LOW");
    assert_eq!(certainty, "KNOWN");
    assert!((remaining.unwrap() - 0.1).abs() < 1e-9, "{remaining:?}");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn usage_is_reported_when_the_cli_gives_it_and_estimated_when_it_does_not() {
    let reported =
        "[[step]]\nkind = \"usage\"\ntokens = 4321\n[[step]]\nkind = \"say\"\ntext = \"listo\"\n";
    let silent = "[[step]]\nkind = \"say\"\ntext = \"respuesta de unas cuantas palabras\"\n";
    let e = env_with(&[("alpha", reported), ("beta", silent)], None).await;
    for (title, model) in [("con uso", "alpha/fast"), ("sin uso", "beta/fast")] {
        e.runtime
            .create_agent(req(&e, title, Execution::Exact(model.into())))
            .await
            .unwrap();
        e.runtime.wait_executors().await;
    }
    e.writer.handle().flush().await.unwrap();

    let usage = |provider: &str| -> (String, i64) {
        symphony_store::open_reader(&e.db)
            .unwrap()
            .query_row(
                "SELECT source, COALESCE(tokens_in, 0) FROM usage_records WHERE provider_id = ?1",
                [provider],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    };
    assert_eq!(usage("alpha"), ("REPORTED".into(), 4321));
    let (source, tokens_in) = usage("beta");
    assert_eq!(source, "ESTIMATED");
    assert!(tokens_in > 0, "se estima a partir del prompt enviado");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_budget_in_the_config_gives_an_estimated_quota_never_a_percentage() {
    let heavy =
        "[[step]]\nkind = \"usage\"\ntokens = 900\n[[step]]\nkind = \"say\"\ntext = \"listo\"\n";
    let e = env_with(&[("alpha", heavy), ("beta", heavy)], None).await;
    // Solo alpha tiene presupuesto: 1000 tokens por ventana de una hora, reserva 0.20.
    let mut cfg = symphony_daemon::health::HealthConfig::default();
    cfg.budgets.insert("alpha".into(), (1, 1000));
    e.bus.set_health_config(cfg);
    for (title, model) in [
        ("con presupuesto", "alpha/fast"),
        ("sin presupuesto", "beta/fast"),
    ] {
        e.runtime
            .create_agent(req(&e, title, Execution::Exact(model.into())))
            .await
            .unwrap();
        e.runtime.wait_executors().await;
    }
    e.writer.handle().flush().await.unwrap();

    let (state, certainty, remaining, _, _) = health_of(&e, "alpha");
    assert_eq!(certainty, "ESTIMATED");
    assert_eq!(
        remaining, None,
        "una estimación nunca se muestra como porcentaje"
    );
    assert_eq!(state, "QUOTA_LOW", "900 de 1000 con reserva 0.20");
    let (state, certainty, remaining, _, _) = health_of(&e, "beta");
    assert_eq!(
        (state.as_str(), certainty.as_str(), remaining),
        ("HEALTHY", "UNKNOWN", None)
    );
    e.writer.shutdown();
}

/// P10.S5: routing por profiles y failover con el router.
async fn set_health(
    e: &Env,
    provider: &str,
    state: &str,
    certainty: &str,
    remaining: Option<f64>,
    reset_in_ms: Option<i64>,
) {
    let (provider, state, certainty) = (
        provider.to_string(),
        state.to_string(),
        certainty.to_string(),
    );
    e.writer
        .handle()
        .write(Box::new(move |t| {
            let now = now_secs() * 1000;
            let h = symphony_core::Health {
                state: state.parse().unwrap(),
                certainty: certainty.parse().unwrap(),
                remaining,
                retry_after_at: None,
                reset_at: reset_in_ms.map(|ms| now + ms),
                evidence: Some("test".into()),
                updated_at: now,
            };
            symphony_store::health::put_health(t, &provider, None, &h)?;
            Ok(())
        }))
        .await
        .unwrap();
}

const SAY: &str = "[[step]]\nkind = \"say\"\ntext = \"listo\"\n";
const QUOTA: &str = "[[step]]\nkind = \"quota_exhausted\"\n";

fn rejections(e: &Env, agent: &str) -> Vec<(String, Option<String>)> {
    let conn = symphony_store::open_reader(&e.db).unwrap();
    conn.prepare(
        "SELECT c.model_id, c.reject_reason FROM routing_candidates c
         JOIN routing_decisions d ON d.id = c.decision_id
         WHERE d.agent_id = ?1 AND d.id = (SELECT id FROM routing_decisions WHERE agent_id = ?1 ORDER BY decided_at DESC, id DESC LIMIT 1)
         ORDER BY c.model_id",
    )
    .unwrap()
    .query_map([agent], |r| Ok((r.get(0)?, r.get(1)?)))
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_spawn_skips_the_exhausted_provider_and_records_the_decision() {
    let e = env_with(&[("alpha", SAY), ("beta", SAY)], None).await;
    set_health(&e, "alpha", "EXHAUSTED", "UNKNOWN", None, Some(3_600_000)).await;
    let created = e
        .runtime
        .create_agent(req(&e, "con profile", Execution::Profile("@code".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id.to_string();

    // El modelo vive en el run; el agente solo recuerda el profile (AGENT ≠ MODEL).
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT execution_mode || '|' || requested_profile_id || '|' || COALESCE(requested_model_id, 'null') FROM agents WHERE id = '{agent}'"
            )
        ),
        "PROFILE|@code|null"
    );
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT provider_id FROM agent_runs WHERE agent_id = '{agent}'")
        ),
        "beta"
    );
    // La decisión queda guardada, atada al run, con el motivo de cada descarte.
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT d.trigger || '|' || d.profile_id FROM routing_decisions d JOIN agent_runs r ON r.routing_decision_id = d.id WHERE d.agent_id = '{agent}'"
            )
        ),
        "SPAWN|@code"
    );
    let rej = rejections(&e, &agent);
    assert!(
        rej.iter()
            .filter(|(m, _)| m.starts_with("alpha/"))
            .all(|(_, r)| r.as_deref() == Some("EXHAUSTED")),
        "{rej:?}"
    );
    assert!(
        rej.iter()
            .filter(|(m, _)| m.starts_with("beta/"))
            .any(|(_, r)| r.is_none()),
        "{rej:?}"
    );
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT account_id FROM agent_runs WHERE agent_id = '{agent}'")
        ),
        "acct-beta"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_with_no_eligible_model_explains_why_and_keeps_the_task() {
    let e = env_with(&[("alpha", SAY)], None).await;
    set_health(&e, "alpha", "AUTH_ERROR", "UNKNOWN", None, None).await;
    let err = e
        .runtime
        .create_agent(req(&e, "sin nadie", Execution::Profile("@fast".into())))
        .await
        .unwrap_err();
    match err {
        CreateError::NoEligibleModel {
            profile,
            explanation,
            task,
        } => {
            assert_eq!(profile, "@fast");
            assert!(
                explanation.contains("descartado: sesión inválida o vencida"),
                "{explanation}"
            );
            assert!(explanation.contains("Ningún modelo es elegible ahora."));
            assert_eq!(task, "sin nadie", "la tarea no se pierde");
        }
        other => panic!("{other:?}"),
    }
    assert_nothing_created(&e);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_exact_model_is_obeyed_even_with_low_quota_and_the_choice_is_recorded() {
    let e = env_with(&[("alpha", SAY), ("beta", SAY)], None).await;
    // Cuota informada dentro de la reserva: un profile automático no la gastaría; el usuario sí.
    set_health(&e, "alpha", "QUOTA_LOW", "KNOWN", Some(0.1), None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "exacto", Execution::Exact("alpha/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id.to_string();
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT provider_id FROM agent_runs WHERE agent_id = '{agent}'")
        ),
        "alpha"
    );
    let explanation: String = one(
        &e,
        &format!("SELECT explanation FROM routing_decisions WHERE agent_id = '{agent}'"),
    );
    assert!(
        explanation.starts_with("Modelo exacto elegido por el usuario: alpha/fast"),
        "{explanation}"
    );
    assert_eq!(
        one::<Option<String>>(
            &e,
            &format!("SELECT profile_id FROM routing_decisions WHERE agent_id = '{agent}'")
        ),
        None
    );
    e.writer.shutdown();
}

async fn failover_from(
    first: &'static str,
    policy: symphony_core::FailoverPolicy,
    others: &[(&'static str, &'static str)],
) -> (Env, String) {
    let mut providers = vec![(first, QUOTA)];
    providers.extend_from_slice(others);
    let e = env_with(&providers, None).await;
    let mut r = req(&e, "failover", Execution::Exact(format!("{first}/fast")));
    r.failover = policy;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    (e, created.agent_id.to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn failover_any_goes_to_the_best_other_provider_and_records_why() {
    let (e, agent) = failover_from(
        "alpha",
        symphony_core::FailoverPolicy::Any,
        &[("beta", SAY), ("gamma", SAY)],
    )
    .await;
    let providers: String = one(
        &e,
        &format!(
            "SELECT group_concat(provider_id, '>') FROM (SELECT provider_id FROM agent_runs WHERE agent_id = '{agent}' ORDER BY seq)"
        ),
    );
    assert_eq!(
        providers, "alpha>beta",
        "gana el primero por puntaje y, a igualdad, por id"
    );
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT state FROM agents WHERE id = '{agent}'")
        ),
        "COMPLETED"
    );
    // La decisión de failover queda atada al run nuevo, con todos los modelos evaluados.
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT d.trigger FROM routing_decisions d JOIN agent_runs r ON r.routing_decision_id = d.id WHERE r.agent_id = '{agent}' AND r.seq = 2"
            )
        ),
        "FAILOVER"
    );
    let rej = rejections(&e, &agent);
    // El modelo que se agotó queda descartado como agotado; el proveedor agotado, por salud.
    assert!(
        rej.iter()
            .any(|(m, r)| m == "alpha/fast" && r.as_deref() == Some("EXHAUSTED")),
        "{rej:?}"
    );
    assert!(
        rej.iter()
            .any(|(m, r)| m.starts_with("gamma/") && r.is_none()),
        "gamma sigue elegible: {rej:?}"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn failover_none_leaves_the_agent_waiting_and_asks_the_user() {
    let (e, agent) = failover_from(
        "alpha",
        symphony_core::FailoverPolicy::None,
        &[("beta", SAY)],
    )
    .await;
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT state FROM agents WHERE id = '{agent}'")
        ),
        "WAITING_PROVIDER"
    );
    assert!(
        one::<String>(
            &e,
            &format!("SELECT state_reason FROM agents WHERE id = '{agent}'")
        )
        .contains("el failover está desactivado")
    );
    assert_eq!(
        one::<i64>(
            &e,
            &format!("SELECT COUNT(*) FROM agent_runs WHERE agent_id = '{agent}'")
        ),
        1
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn failover_same_provider_does_not_leave_a_provider_that_is_exhausted() {
    // Quedarse en el proveedor no sirve si lo que se agotó fue su cuota: espera al usuario.
    let (e, agent) = failover_from(
        "alpha",
        symphony_core::FailoverPolicy::SameProvider,
        &[("beta", SAY)],
    )
    .await;
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT state FROM agents WHERE id = '{agent}'")
        ),
        "WAITING_PROVIDER"
    );
    assert_eq!(
        one::<i64>(
            &e,
            &format!("SELECT COUNT(*) FROM agent_runs WHERE agent_id = '{agent}'")
        ),
        1
    );
    // Aun así queda la decisión: todo lo de otros proveedores quedó fuera por la política.
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT d.trigger || '|' || COALESCE(d.selected_model_id, 'ninguno') FROM routing_decisions d WHERE d.agent_id = '{agent}' AND d.trigger = 'FAILOVER'"
            )
        ),
        "FAILOVER|ninguno"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_automatic_failover_never_spends_the_reserve_of_another_provider() {
    let e = env_with(&[("alpha", QUOTA), ("beta", SAY), ("gamma", SAY)], None).await;
    // beta (que ganaría por id) tiene la cuota informada dentro de la reserva: se salta.
    set_health(&e, "beta", "QUOTA_LOW", "KNOWN", Some(0.1), None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "reserva", Execution::Exact("alpha/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id.to_string();
    let providers: String = one(
        &e,
        &format!(
            "SELECT group_concat(provider_id, '>') FROM (SELECT provider_id FROM agent_runs WHERE agent_id = '{agent}' ORDER BY seq)"
        ),
    );
    assert_eq!(providers, "alpha>gamma", "se salta la reserva de beta");
    let rej = rejections(&e, &agent);
    assert!(
        rej.iter()
            .any(|(m, r)| m.starts_with("beta/") && r.as_deref() == Some("RESERVE")),
        "{rej:?}"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_manual_switch_is_recorded_as_an_exact_choice() {
    // alpha sigue trabajando cuando el usuario cambia de modelo.
    let slow = "[[step]]\nkind = \"say\"\ntext = \"voy\"\n[[step]]\nkind = \"sleep\"\nms = 30000\n";
    let e = env_with(&[("alpha", slow), ("beta", SAY)], None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "cambio", Execution::Exact("alpha/fast".into())))
        .await
        .unwrap();
    e.runtime
        .switch(created.agent_id, "beta/smart")
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id.to_string();
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT trigger || '|' || selected_model_id FROM routing_decisions WHERE agent_id = '{agent}' AND trigger = 'SWITCH'"
            )
        ),
        "SWITCH|beta/smart"
    );
    let explain = symphony_daemon::routing::explain(
        &symphony_store::open_reader(&e.db).unwrap(),
        created.agent_id,
        5,
    )
    .unwrap();
    let decisions = explain.as_array().unwrap();
    assert_eq!(decisions.len(), 2, "el spawn y el cambio");
    assert_eq!(decisions[0]["trigger"], "SWITCH");
    assert!(decisions[0]["candidates"].as_array().unwrap().len() >= 4);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_by_profile_lets_the_router_choose_among_the_usable_models() {
    let slow = "[[step]]\nkind = \"say\"\ntext = \"voy\"\n[[step]]\nkind = \"sleep\"\nms = 30000\n";
    let e = env_with(&[("alpha", slow), ("beta", SAY), ("gamma", SAY)], None).await;
    set_health(&e, "beta", "EXHAUSTED", "UNKNOWN", None, Some(3_600_000)).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "por profile",
            Execution::Exact("alpha/fast".into()),
        ))
        .await
        .unwrap();
    let (_, model) = e
        .runtime
        .switch_to_profile(created.agent_id, "@fast", None)
        .await
        .unwrap();
    assert!(model.starts_with("gamma/"), "beta está agotado: {model}");
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let agent = created.agent_id.to_string();
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT trigger || '|' || profile_id FROM routing_decisions WHERE agent_id = '{agent}' AND trigger = 'SWITCH'"
            )
        ),
        "SWITCH|@fast"
    );
    // Un profile que no existe se rechaza sin tocar al agente.
    let err = e
        .runtime
        .switch_to_profile(created.agent_id, "@nada", None)
        .await
        .unwrap_err();
    assert!(err.0.contains("no existe"), "{}", err.0);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn long_messages_and_diffs_become_searchable_context_objects() {
    // Un mensaje de más de 4 KB va al object store; un archivo nuevo deja un diff en el checkpoint.
    let long = format!(
        "{} palabraunicadelmensaje {}",
        "relleno ".repeat(700),
        "fin ".repeat(100)
    );
    let script = format!(
        "[[step]]\nkind = \"edit\"\npath = \"README.md\"\ncontent = \"identificadorraro actualizado\"\n[[step]]\nkind = \"say\"\ntext = \"{long}\"\n"
    );
    let e = env(&script, None).await;
    e.runtime
        .create_agent(req(&e, "indexar", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let project: String = one(&e, "SELECT id FROM projects");

    let conn = symphony_store::open_reader(&e.db).unwrap();
    let find = |word: &str| {
        let q = symphony_context::chunk::fts_query(word).unwrap();
        symphony_store::context::search(&conn, &project, &q, None, 10).unwrap()
    };
    let dbg: (i64, i64, String, i64) = (
        one(&e, "SELECT COUNT(*) FROM context_objects"),
        one(&e, "SELECT COUNT(*) FROM context_chunks"),
        one(&e, "SELECT state || COALESCE(state_reason, '') FROM agents"),
        one(&e, "SELECT MAX(LENGTH(content)) FROM messages"),
    );
    let msg = find("palabraunicadelmensaje");
    assert_eq!(msg.len(), 1, "{msg:?} objetos/chunks={dbg:?}");
    assert!(msg[0].uri.starts_with("ctx://message/"), "{}", msg[0].uri);
    // El original completo sigue recuperable por su dirección.
    let obj = symphony_store::context::object_by_uri(&conn, None, &msg[0].uri)
        .unwrap()
        .unwrap();
    let bytes = symphony_object_store::ObjectStore::new(e.home.join("objects"))
        .get(&obj.blob_hash)
        .unwrap();
    assert!(
        String::from_utf8(bytes)
            .unwrap()
            .contains("palabraunicadelmensaje")
    );
    let diff = find("identificadorraro");
    assert!(
        !diff.is_empty() && diff[0].uri.starts_with("ctx://diff/"),
        "{diff:?}"
    );
    e.writer.shutdown();
}

/// Un CLI que falla (login, cuota) antes de leer su prompt deja un `EPIPE` al escribirlo: no es un
/// fallo de arranque; su salida y su código de salida mandan y el failover sigue su curso.
#[tokio::test(flavor = "multi_thread")]
async fn cli_that_quits_before_reading_its_prompt_still_fails_over() {
    let first = "[[step]]\nkind = \"quota_exhausted\"\n";
    let second = "[[step]]\nkind = \"edit\"\npath = \"b.txt\"\ncontent = \"b\n\"\n";
    let e = env_with(&[("alpha", first), ("beta", second)], None).await;
    let mut r = req(
        &e,
        "tarea con prompt enorme",
        Execution::Exact("alpha/fast".into()),
    );
    // Mayor que el búfer de la tubería: la escritura sigue en curso cuando alpha ya salió.
    r.description = Some("x".repeat(1_000_000));
    e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(
        one::<String>(&e, "SELECT end_reason FROM agent_runs WHERE seq = 1"),
        "QUOTA_EXHAUSTED"
    );
    assert_eq!(count(&e, "provider_failures"), 1);
    assert_eq!(count(&e, "agent_runs"), 2);
    e.writer.shutdown();
}

/// P09.S10: evaluación del handoff. El forced kill de P06.S8 con un checkpoint pesado (un
/// lockfile de 3.000 líneas tocado, una conversación larga y un cambio real en README) y los
/// cuatro modos. Lo que se mide: tokens enviados, retrievals y misses (todavía sin MCP:
/// siempre 0) y si B terminó la tarea. `--nocapture` imprime la tabla de la bitácora.
#[tokio::test(flavor = "multi_thread")]
async fn handoff_evaluation_forced_kill_by_context_mode() {
    let lock: String = (0..3000)
        .map(|i| format!("    \"dep{i}\": \"1.0.{i}\"\n"))
        .collect();
    let long_say = "Revisé el módulo de sesiones y sus casos borde. ".repeat(60);
    let script_a = format!(
        "[[step]]\nkind = \"edit\"\npath = \"package-lock.json\"\ncontent = '''\n{lock}'''\n\
         [[step]]\nkind = \"edit\"\npath = \"README.md\"\ncontent = \"demo rotacion de tokens revisada\"\n\
         [[step]]\nkind = \"edit\"\npath = \"src_auth.js\"\ncontent = \"export const rotate = 1\"\n\
         [[step]]\nkind = \"say\"\ntext = \"{long_say}\"\n\
         [[step]]\nkind = \"say\"\ntext = \"{long_say}\"\n\
         [[step]]\nkind = \"say\"\ntext = \"Next: escribir las pruebas de rotación en rotate_test.js\"\n\
         [[step]]\nkind = \"hang\"\n"
    );
    let script_b = "[[step]]\nkind = \"edit\"\npath = \"rotate_test.js\"\ncontent = \"// pruebas\"\n[[step]]\nkind = \"say\"\ntext = \"Tarea completada con éxito.\"\n";
    let e = env_with_watchdog(
        &[("claude", &script_a), ("codex", script_b)],
        None,
        Duration::from_millis(100),
        Duration::from_millis(3000),
    )
    .await;
    std::fs::write(e.repo.join("package-lock.json"), "{\n}\n").unwrap();
    git(&e.repo, &["add", "."]);
    git(&e.repo, &["commit", "-q", "-m", "lock"]);

    println!("modo       | tokens enviados | tokens RAW | retrievals | misses | terminó");
    let mut sent = Vec::new();
    for mode in [
        ContextMode::Raw,
        ContextMode::Safe,
        ContextMode::Balanced,
        ContextMode::Aggressive,
    ] {
        let mut r = req(
            &e,
            &format!("Rotación de tokens {mode:?}"),
            Execution::Exact("claude/fast".into()),
        );
        r.context_mode = mode;
        let created = e.runtime.create_agent(r).await.unwrap();
        e.runtime.wait_executors().await;
        e.writer.handle().flush().await.unwrap();
        e.runtime
            .switch(created.agent_id, "codex/fast")
            .await
            .unwrap();
        e.runtime.wait_executors().await;
        e.writer.handle().flush().await.unwrap();

        let agent = created.agent_id.to_string();
        let state: String = one(
            &e,
            &format!("SELECT state FROM agents WHERE id = '{agent}'"),
        );
        let (tokens_sent, tokens_raw): (i64, i64) = {
            let conn = symphony_store::open_reader(&e.db).unwrap();
            conn.query_row(
                &format!(
                    "SELECT tokens_sent, tokens_raw_estimate FROM handoffs WHERE agent_id = '{agent}' AND checkpoint_id IS NOT NULL"
                ),
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        // Un mensaje largo vive en el object store; uno corto, en la fila.
        let (inline, object): (Option<String>, Option<String>) = {
            let conn = symphony_store::open_reader(&e.db).unwrap();
            conn.query_row(
                &format!("SELECT content, content_object_id FROM messages WHERE role = 'USER' AND agent_id = '{agent}' ORDER BY created_at DESC LIMIT 1"),
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        let prompt = inline.unwrap_or_else(|| diff_text(&e, &object.unwrap()));
        println!(
            "{:<10} | {:>15} | {:>10} | {:>10} | {:>6} | {}",
            format!("{mode:?}"),
            tokens_sent,
            tokens_raw,
            0,
            0,
            state == "COMPLETED"
        );
        assert_eq!(state, "COMPLETED", "{mode:?}: B debe terminar la tarea");
        // Lo esencial entra en todos los modos: objetivo, qué seguía, archivos y el cambio real.
        for want in [
            "Rotación de tokens",
            "escribir las pruebas de rotación en rotate_test.js",
            "src_auth.js",
            "package-lock.json",
            "+demo rotacion de tokens revisada",
        ] {
            assert!(prompt.contains(want), "{mode:?}: falta {want:?}");
        }
        sent.push(tokens_sent);
    }
    let [raw, safe, balanced, aggressive] = sent[..] else {
        panic!("cuatro modos")
    };
    // RAW y SAFE mandan lo mismo con un diff chico: cada modo corre con su propio agente y los
    // hechos del proyecto se consolidan en segundo plano, así que entre corridas puede variar
    // una veintena de tokens (visto en el CI de Windows: 21 492 frente a 21 512). El orden que
    // importa es el de los modos que recortan de verdad.
    const NOISE: i64 = 64;
    assert!(
        raw + NOISE >= safe && safe >= balanced && balanced >= aggressive,
        "{sent:?}"
    );
    assert!(
        balanced * 2 < raw,
        "BALANCED debería costar menos de la mitad de RAW: {sent:?}"
    );
    e.writer.shutdown();
}

/// P09.S5: el watcher del proyecto deja los archivos buscables y sigue sus cambios (con
/// debounce), sin indexar lo ignorado por `.gitignore`.
#[tokio::test(flavor = "multi_thread")]
async fn project_files_stay_searchable_as_they_change() {
    let e = env("[[step]]\nkind = \"say\"\ntext = \"ok\"\n", None).await;
    std::fs::write(e.repo.join(".gitignore"), "build/\n").unwrap();
    std::fs::create_dir_all(e.repo.join("src")).unwrap();
    std::fs::create_dir_all(e.repo.join("build")).unwrap();
    std::fs::write(
        e.repo.join("src/auth.ts"),
        "export function rotarviejo() {}\n",
    )
    .unwrap();
    std::fs::write(e.repo.join("build/out.js"), "palabraignorada\n").unwrap();
    e.runtime
        .create_agent(req(&e, "watch", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    let project: String = one(&e, "SELECT id FROM projects");
    let hits = |word: &str| {
        let conn = symphony_store::open_reader(&e.db).unwrap();
        let q = symphony_context::chunk::fts_query(word).unwrap();
        symphony_store::context::search(&conn, &project, &q, None, 5)
            .unwrap()
            .into_iter()
            .map(|h| h.uri)
            .collect::<Vec<_>>()
    };
    let wait_for = |word: &'static str, present: bool| {
        let started = std::time::Instant::now();
        loop {
            if hits(word).is_empty() != present {
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "{word}: esperaba presente = {present}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    };

    wait_for("rotarviejo", true);
    assert_eq!(hits("rotarviejo"), ["ctx://file/src/auth.ts"]);
    assert!(
        hits("palabraignorada").is_empty(),
        "build/ está en .gitignore"
    );

    std::fs::write(
        e.repo.join("src/auth.ts"),
        "export function rotarnuevo() {}\n",
    )
    .unwrap();
    wait_for("rotarnuevo", true);
    wait_for("rotarviejo", false);

    std::fs::remove_file(e.repo.join("src/auth.ts")).unwrap();
    wait_for("rotarnuevo", false);

    // Lo que entró antes de su regla en `.gitignore` sale al volver a arrancar: el recorrido
    // inicial limpia lo que ya no corresponde.
    std::fs::create_dir_all(e.repo.join("target")).unwrap();
    std::fs::write(e.repo.join("target/output"), "artefactoviejo\n").unwrap();
    wait_for("artefactoviejo", true);
    std::fs::write(e.repo.join(".gitignore"), "build/\ntarget/\n").unwrap();
    // El `Watchers` temporal se destruía al salir de `ensure` y cerraba su canal antes de que el
    // `rescan` podara el índice; retenerlo durante la espera (agente #53, codex/gpt-6.1-sol).
    let rescan = symphony_daemon::watcher::Watchers::default();
    rescan.ensure(
        &project,
        &e.repo,
        e.writer.handle(),
        ObjectStore::new(e.home.join("objects")),
    );
    wait_for("artefactoviejo", false);
    e.writer.shutdown();
}

/// P09.S8: lo que el broker MCP pide al daemon (`search`, `retrieve`, `lines`) sobre archivos
/// indexados: aciertos con su recuperación, recorte, misses y direcciones inválidas.
#[tokio::test(flavor = "multi_thread")]
async fn context_mcp_search_retrieve_and_lines() {
    use symphony_daemon::context_mcp as mcp;
    let e = env(
        "[[step]]
kind = \"say\"
text = \"ok\"
",
        None,
    )
    .await;
    std::fs::create_dir_all(e.repo.join("src")).unwrap();
    let body: String = (1..=30)
        .map(|i| {
            format!(
                "linea {i} rotarmcp
"
            )
        })
        .collect();
    std::fs::write(e.repo.join("src/big.ts"), &body).unwrap();
    // P11.5.S1: una ruta con `@` (paquete con scope) tiene dirección propia, con `@` escapado.
    std::fs::create_dir_all(e.repo.join("packages/@scope")).unwrap();
    std::fs::write(
        e.repo.join("packages/@scope/x.ts"),
        "export const palabraescopada = 1;\n",
    )
    .unwrap();
    std::fs::write(
        e.repo.join("src/secret.ts"),
        "const KEY = \"sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv\"; // claveindexada
",
    )
    .unwrap();
    e.runtime
        .create_agent(req(&e, "mcp", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    let project: String = one(&e, "SELECT id FROM projects");
    let conn = symphony_store::open_reader(&e.db).unwrap();
    let objects = symphony_object_store::ObjectStore::new(
        symphony_core::SymphonyHome::at(&e.home).objects_dir(),
    );
    let started = std::time::Instant::now();
    let found = loop {
        let a = mcp::search(&conn, &project, "rotarmcp", None, None).unwrap();
        if !a.value["hits"].as_array().unwrap().is_empty() {
            break a;
        }
        assert!(started.elapsed() < Duration::from_secs(20), "sin indexar");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(found.value["hits"][0]["uri"], "ctx://file/src/big.ts");
    assert!(
        found
            .retrievals
            .iter()
            .all(|r| r.found && r.operation == "SEARCH")
    );

    let uri = "ctx://file/src/big.ts";
    let all = mcp::retrieve(&conn, &objects, &project, uri).unwrap();
    assert_eq!(all.value["text"], body);
    assert_eq!(all.value["truncated"], false);
    assert_eq!(all.value["total_lines"], 30);

    let some = mcp::lines(&conn, &objects, &project, uri, 3, 4).unwrap();
    assert_eq!(
        some.value["text"],
        "linea 3 rotarmcp
linea 4 rotarmcp"
    );
    assert!(some.retrievals[0].found);

    // Más allá del final: miss registrable, no error.
    let past = mcp::lines(&conn, &objects, &project, uri, 99, 120).unwrap();
    assert_eq!(past.value["text"], "");
    assert!(!past.retrievals[0].found);
    // Búsqueda sin resultados dentro de un objeto conocido: miss atado a ese objeto.
    let none = mcp::search(&conn, &project, "inexistentexyz", Some(uri), None).unwrap();
    assert!(none.value["hits"].as_array().unwrap().is_empty());
    assert_eq!(none.retrievals.len(), 1);
    assert!(!none.retrievals[0].found);
    // El rango pedido se recorta a LINES_MAX.
    let wide = mcp::lines(&conn, &objects, &project, uri, 1, 100_000).unwrap();
    assert_eq!(wide.value["to"], 30);

    // Lo que sale hacia un proveedor pasa por el redactor: la clave no viaja ni en la búsqueda
    // ni en el original.
    let started = std::time::Instant::now();
    let secret = loop {
        let a = mcp::search(&conn, &project, "claveindexada", None, None).unwrap();
        if !a.value["hits"].as_array().unwrap().is_empty() {
            break a;
        }
        assert!(started.elapsed() < Duration::from_secs(20), "sin indexar");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(
        !secret.value.to_string().contains("sk-ant-api03"),
        "{}",
        secret.value
    );
    let orig = mcp::retrieve(&conn, &objects, &project, "ctx://file/src/secret.ts").unwrap();
    assert!(
        !orig.value["text"]
            .as_str()
            .unwrap()
            .contains("sk-ant-api03")
    );
    assert!(
        orig.value["text"]
            .as_str()
            .unwrap()
            .contains("claveindexada")
    );

    let started = std::time::Instant::now();
    let scoped = loop {
        let a = mcp::search(&conn, &project, "palabraescopada", None, None).unwrap();
        if !a.value["hits"].as_array().unwrap().is_empty() {
            break a;
        }
        assert!(started.elapsed() < Duration::from_secs(20), "sin indexar");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(
        scoped.value["hits"][0]["uri"],
        "ctx://file/packages/%40scope/x.ts"
    );
    let uri_scoped = "ctx://file/packages/%40scope/x.ts";
    let got = mcp::retrieve(&conn, &objects, &project, uri_scoped).unwrap();
    assert_eq!(got.value["text"], "export const palabraescopada = 1;\n");
    assert!(
        mcp::lines(&conn, &objects, &project, uri_scoped, 1, 1)
            .unwrap()
            .retrievals[0]
            .found
    );
    // El `@` sin escapar sigue siendo el separador de revisión (`packages/` + rev `scope/x.ts`):
    // la ruta queda con un segmento vacío y se rechaza.
    let err =
        mcp::retrieve(&conn, &objects, &project, "ctx://file/packages/@scope/x.ts").unwrap_err();
    assert_eq!(err.0, "invalid_params");

    for bad in [
        "ctx://file/../../etc/passwd",
        "ctx://file/packages/%2e%2e/x.ts",
        "no-es-una-uri",
    ] {
        let err = mcp::retrieve(&conn, &objects, &project, bad).unwrap_err();
        assert_eq!(err.0, "invalid_params", "{bad}");
    }
    let err = mcp::retrieve(&conn, &objects, &project, "ctx://file/nada.ts").unwrap_err();
    assert_eq!(err.0, "context_not_found");
    let err = mcp::search(&conn, &project, "!!! ???", None, None).unwrap_err();
    assert_eq!(err.0, "invalid_params");
    e.writer.shutdown();
}

/// P09.S7: `project_facts` se incluyen en el handoff y registran su item en Decisions.
#[tokio::test(flavor = "multi_thread")]
async fn project_facts_ipc_and_handoff_integration() {
    let script = r#"
[[step]]
kind = "say"
text = "primer paso listo"
"#;
    let e = env(script, None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "test facts", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;

    let project: String = one(&e, "SELECT id FROM projects");

    // Registrar un hecho vía store
    let proj_clone = project.clone();
    e.writer
        .handle()
        .write(Box::new(move |t| {
            symphony_store::context::set_fact(
                t,
                &proj_clone,
                "token_storage",
                "httpOnly cookies",
                "DECISION",
                None,
                None,
                100,
            )?;
            Ok(())
        }))
        .await
        .unwrap();

    let handoff = e
        .runtime
        .prepare_handoff(created.agent_id, "cambio de modelo")
        .await
        .unwrap();
    assert!(
        handoff.prompt.contains("## Hechos vigentes del proyecto"),
        "el prompt debe incluir los hechos vigentes: {}",
        handoff.prompt
    );
    assert!(
        handoff
            .prompt
            .contains("- **[DECISION] token_storage:** httpOnly cookies"),
        "{}",
        handoff.prompt
    );
    assert!(
        handoff.items.iter().any(|i| i.section == "DECISIONS"),
        "debe registrar un item en la sección DECISIONS: {:?}",
        handoff.items
    );

    e.writer.shutdown();
}

// --- P08.S5: dependencias entre tareas ---------------------------------------------

/// Espera (hasta 30 s) a que el agente llegue a `state`.
async fn wait_state(e: &Env, agent: &str, state: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let now: String = one(e, &format!("SELECT state FROM agents WHERE id='{agent}'"));
        if now == state {
            return;
        }
        let why: Option<String> = one(
            e,
            &format!("SELECT state_reason FROM agents WHERE id='{agent}'"),
        );
        assert!(
            std::time::Instant::now() < deadline,
            "el agente #{} quedó en {now} ({why:?}), se esperaba {state}",
            one::<i64>(e, &format!("SELECT number FROM agents WHERE id='{agent}'"))
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dependent_agent_waits_then_starts_when_its_dependency_is_done() {
    // El primero tarda: cuando se crea el segundo, todavía corre.
    let slow = r#"
[[step]]
kind = "say"
text = "Trabajando"

[[step]]
kind = "sleep"
ms = 1500

[[step]]
kind = "edit"
path = "fix.txt"
content = "ok"
"#;
    let e = env(slow, None).await;
    let reactor = symphony_daemon::dependencies::spawn(
        e.runtime.clone(),
        tokio_util::sync::CancellationToken::new(),
    );

    // B depende de T-1 antes de que exista el primer agente: es una dependencia desconocida.
    let mut unknown = req(&e, "Segunda", Execution::Exact("fake/fast".into()));
    unknown.depends_on = vec!["T-99".into()];
    let err = e.runtime.create_agent(unknown).await.unwrap_err();
    assert_eq!(err.code(), "unknown_dependency");
    assert_nothing_created(&e);

    // A corre; B se crea esperando a T-1 y no arranca todavía.
    let a = e
        .runtime
        .create_agent(req(&e, "Primera", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    let mut second = req(&e, "Segunda", Execution::Exact("fake/fast".into()));
    second.depends_on = vec![a.task_code.clone()];
    let b = e.runtime.create_agent(second).await.unwrap();
    assert!(b.run.is_none(), "no arranca mientras espera");

    let b_id = b.agent_id.to_string();
    assert_eq!(b.state, AgentState::WaitingDependency);
    assert_eq!(b.state_reason.as_deref(), Some("espera a T-1"));

    // A termina → el reactor libera a B y lo arranca con su modelo exacto.
    wait_state(&e, &a.agent_id.to_string(), "COMPLETED").await;
    wait_state(&e, &b_id, "COMPLETED").await;
    e.runtime.wait_executors().await;
    let runs: i64 = one(
        &e,
        &format!("SELECT COUNT(*) FROM agent_runs WHERE agent_id='{b_id}'"),
    );
    assert_eq!(runs, 1, "B corrió una vez, ya liberado");
    let b_started: i64 = one(
        &e,
        &format!("SELECT started_at FROM agent_runs WHERE agent_id='{b_id}'"),
    );
    let a_done: i64 = one(
        &e,
        &format!(
            "SELECT COALESCE(t.completed_at, 0) FROM tasks t WHERE t.id='{}'",
            a.task_id
        ),
    );
    assert!(b_started >= a_done, "B no arrancó antes de que A terminara");
    reactor.abort();
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_dependency_blocks_the_dependent_until_the_user_decides() {
    let script = "[[step]]\nkind = \"crash\"\n";
    let e = env(script, None).await;
    let reactor = symphony_daemon::dependencies::spawn(
        e.runtime.clone(),
        tokio_util::sync::CancellationToken::new(),
    );
    let a = e
        .runtime
        .create_agent(req(&e, "Primera", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    let mut second = req(&e, "Segunda", Execution::Exact("fake/fast".into()));
    second.depends_on = vec![a.task_code.clone()];
    let b = e.runtime.create_agent(second).await.unwrap();
    let b_id = b.agent_id.to_string();

    // A muere → B queda BLOCKED con la decisión pendiente y un recovery item.
    wait_state(&e, &a.agent_id.to_string(), "FAILED").await;
    wait_state(&e, &b_id, "BLOCKED").await;
    let reason: String = one(
        &e,
        &format!("SELECT state_reason FROM agents WHERE id='{b_id}'"),
    );
    assert!(
        reason.contains("falló") && reason.contains("ignorar la dependencia"),
        "{reason}"
    );
    let kind: String = one(
        &e,
        "SELECT kind FROM recovery_items WHERE kind='DEPENDENCY_FAILED'",
    );
    assert_eq!(kind, "DEPENDENCY_FAILED");
    let none: i64 = one(
        &e,
        &format!("SELECT COUNT(*) FROM agent_runs WHERE agent_id='{b_id}'"),
    );
    assert_eq!(none, 0, "no arrancó");

    // «Ignorar la dependencia»: se libera y arranca (con el mismo guion muere, pero corrió).
    let resolved = symphony_daemon::dependencies::resolve(
        &e.runtime,
        b.task_id,
        a.task_id,
        symphony_daemon::dependencies::Resolution::Ignore,
    )
    .await
    .unwrap();
    assert!(resolved.released);
    e.runtime.wait_executors().await;
    let runs: i64 = one(
        &e,
        &format!("SELECT COUNT(*) FROM agent_runs WHERE agent_id='{b_id}'"),
    );
    assert_eq!(runs, 1, "tras ignorar la dependencia B arrancó");
    let deps: i64 = one(&e, "SELECT COUNT(*) FROM task_dependencies");
    assert_eq!(deps, 0);

    // Cancelar es otra salida: una tercera tarea que depende de la fallida.
    let mut third = req(&e, "Tercera", Execution::Exact("fake/fast".into()));
    third.depends_on = vec![a.task_code.clone()];
    let c = e.runtime.create_agent(third).await.unwrap();
    wait_state(&e, &c.agent_id.to_string(), "BLOCKED").await;
    symphony_daemon::dependencies::resolve(
        &e.runtime,
        c.task_id,
        a.task_id,
        symphony_daemon::dependencies::Resolution::Cancel,
    )
    .await
    .unwrap();
    wait_state(&e, &c.agent_id.to_string(), "CANCELLED").await;
    reactor.abort();
    e.writer.shutdown();
}

// --- P08.S5: plan por fases ----------------------------------------------------------

const TWO_PHASES: &str = r##"
[[step]]
kind = "say"
text = "Trabajando la fase"

[[step]]
kind = "plan"
content = "# Plan: arreglar\n## Fase 1: preparar\nVerifica: cargo test\n- [ ] leer\n- [ ] escribir el test\n## Fase 2: arreglar\n- [ ] corregir\n- [ ] correr la suite\n"
"##;

fn run_rows(e: &Env, agent: &str) -> Vec<(i64, String, String, String)> {
    let conn =
        rusqlite::Connection::open_with_flags(&e.db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT seq, status, COALESCE(end_reason, ''), model_id FROM agent_runs
             WHERE agent_id = ?1 ORDER BY seq",
        )
        .unwrap();
    stmt.query_map([agent], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

fn first_user_message(e: &Env, agent: &str, seq: i64) -> String {
    one(
        e,
        &format!(
            "SELECT m.content FROM messages m JOIN agent_runs r ON r.id = m.run_id
             WHERE m.agent_id = '{agent}' AND r.seq = {seq} AND m.role = 'USER'
             ORDER BY m.created_at, m.rowid LIMIT 1"
        ),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plan_runs_one_phase_per_session_and_is_deleted_when_done() {
    let e = env(TWO_PHASES, None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Arreglar la rotación",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    wait_state(&e, &agent, "COMPLETED").await;
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    // Dos sesiones: la fase 1 cerró con PHASE_DONE y la fase 2 con COMPLETED, mismo modelo.
    let runs = run_rows(&e, &agent);
    assert_eq!(
        runs,
        vec![
            (
                1,
                "HANDED_OFF".into(),
                "PHASE_DONE".into(),
                "fake/fast".into()
            ),
            (2, "EXITED".into(), "COMPLETED".into(), "fake/fast".into()),
        ]
    );
    // La conversación guarda lo que pidió el usuario (las reglas del plan van al CLI, no a
    // la conversación); la segunda sesión recibe solo su fase.
    assert_eq!(first_user_message(&e, &agent, 1), "Arreglar la rotación");
    let second = first_user_message(&e, &agent, 2);
    assert!(second.contains("fase 2 de 2: arreglar"), "{second}");
    assert!(
        !second.contains("Instrucciones de Symphony"),
        "no repite las reglas"
    );
    // La tarea terminó y el plan temporal se borró; lo hecho quedó en la conversación.
    assert!(
        !created
            .worktree
            .join(".symphony/plan")
            .join(format!("{}.md", created.task_id))
            .exists()
    );
    let summaries: i64 = one(
        &e,
        &format!(
            "SELECT COUNT(*) FROM messages WHERE agent_id = '{agent}' AND role = 'SYSTEM' AND content LIKE 'Plan «%'"
        ),
    );
    assert_eq!(summaries, 2, "una por fase terminada y una al final");
    let changes: String = one(
        &e,
        &format!("SELECT reason FROM executor_changes WHERE agent_id = '{agent}'"),
    );
    assert_eq!(changes, "RESTART");
    let task: String = one(
        &e,
        &format!("SELECT status FROM tasks WHERE id = '{}'", created.task_id),
    );
    assert_eq!(task, "DONE");
    // Las dos sesiones son una sola tarea: ni recovery items ni un segundo agente.
    assert_eq!(count(&e, "recovery_items"), 0);
    assert_eq!(count(&e, "agents"), 1);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn asking_before_each_phase_leaves_the_agent_ready_until_confirmed() {
    let e = env(TWO_PHASES, None).await;
    std::fs::write(
        e.home.join("config.toml"),
        "[plan]\nenabled = true\nphase_start = \"ask\"\nmin_tasks_per_phase = 2\n",
    )
    .unwrap();
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Con confirmación",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    wait_state(&e, &agent, "READY").await;
    e.runtime.wait_executors().await;
    let why: String = one(
        &e,
        &format!("SELECT state_reason FROM agents WHERE id = '{agent}'"),
    );
    assert!(
        why.contains("fase 2 «arreglar» lista para arrancar"),
        "{why}"
    );
    assert_eq!(run_rows(&e, &agent).len(), 1, "no arrancó la fase 2 sola");

    // El usuario confirma: arranca la fase 2 y la tarea termina.
    let run = e.runtime.start_next_phase(created.agent_id).await.unwrap();
    assert_ne!(run, created.run.clone().unwrap().0);
    wait_state(&e, &agent, "COMPLETED").await;
    e.runtime.wait_executors().await;
    assert_eq!(run_rows(&e, &agent).len(), 2);
    // Sin plan pendiente, confirmar otra vez no hace nada raro.
    assert!(e.runtime.start_next_phase(created.agent_id).await.is_err());
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_task_in_the_plan_stops_the_agent_and_keeps_the_plan() {
    let script = r##"
[[step]]
kind = "plan"
content = "# Plan: x\n## Fase 1: a\n- [x] uno\n- [!] dos\n  > no compila\n"
"##;
    let e = env(script, None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Plan que falla",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    wait_state(&e, &agent, "FAILED").await;
    e.runtime.wait_executors().await;
    let why: String = one(
        &e,
        &format!("SELECT state_reason FROM agents WHERE id = '{agent}'"),
    );
    assert!(
        why.contains("fase 1") && why.contains("no compila"),
        "{why}"
    );
    let kind: String = one(&e, "SELECT kind FROM recovery_items");
    assert_eq!(kind, "EXECUTOR_EXITED");
    // El plan queda para quien reanude la tarea.
    assert!(
        created
            .worktree
            .join(".symphony/plan")
            .join(format!("{}.md", created.task_id))
            .is_file()
    );
    assert_eq!(run_rows(&e, &agent).len(), 1);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_plan_file_the_agent_completes_as_before() {
    let e = env(WORK, None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "Sin plan", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    wait_state(&e, &agent, "COMPLETED").await;
    assert_eq!(run_rows(&e, &agent).len(), 1);
    assert_eq!(first_user_message(&e, &agent, 1), "Sin plan");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_plan_file_never_reaches_git() {
    let e = env(TWO_PHASES, None).await;
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "No commitear",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    wait_state(&e, &created.agent_id.to_string(), "COMPLETED").await;
    e.runtime.wait_executors().await;
    let exclude = std::fs::read_to_string(e.repo.join(".git/info/exclude")).unwrap();
    assert!(
        exclude.lines().any(|l| l.trim() == ".symphony/plan/"),
        "{exclude}"
    );
    let status = git(&created.worktree, &["status", "--porcelain"]);
    assert!(!status.contains(".symphony"), "{status}");
    e.writer.shutdown();
}

// --- P08.S6: validación y preparación del worktree -----------------------------------

/// Archivos de validación en el repo base (los worktrees nacen de este commit).
fn seed_validation(e: &Env, project_toml: &str) {
    let dir = e.repo.join("scripts");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ok.js"), "process.exit(0)\n").unwrap();
    std::fs::write(
        dir.join("check.js"),
        "const fs = require('fs');\n\
         if (!fs.existsSync('fixed.txt')) {\n  \
           console.error('FAIL auth.refresh.test.ts expected 200, received 401');\n  \
           console.log('Tests: 1 failed, 3 passed, 4 total');\n  \
           process.exit(1);\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("fail.js"),
        "console.error('error: nunca pasa');\nprocess.exit(1);\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("prepare.js"),
        "require('fs').writeFileSync('prepared.txt', 'listo');\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("prepare_fails.js"),
        "console.error('pnpm: no se pudo instalar');\nprocess.exit(3);\n",
    )
    .unwrap();
    std::fs::create_dir_all(e.repo.join(".symphony")).unwrap();
    std::fs::write(e.repo.join(".symphony/project.toml"), project_toml).unwrap();
    git(&e.repo, &["add", "."]);
    git(&e.repo, &["commit", "-q", "-m", "validación"]);
}

const PROJECT_HEAD: &str = "[project]\nname = \"demo\"\ndefault_branch = \"main\"\n\n";

/// Primer run: deja una marca y no arregla nada; los siguientes (la sesión retomada) sí.
const FIX_ON_RETRY: &str = r##"
[[step]]
kind = "say"
text = "Intento"

[[step]]
kind = "run"
command = ["node", "-e", 'const fs=require("fs"); fs.writeFileSync(fs.existsSync(".attempt") ? "fixed.txt" : ".attempt", "x")']
"##;

fn validation_rows(e: &Env, agent: &str) -> Vec<(i64, String)> {
    let conn =
        rusqlite::Connection::open_with_flags(&e.db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT tier, status FROM validation_runs WHERE agent_id = ?1 ORDER BY started_at, rowid",
        )
        .unwrap();
    stmt.query_map([agent], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_gate_returns_to_the_same_agent_and_the_task_completes_when_it_passes() {
    let e = env(FIX_ON_RETRY, None).await;
    seed_validation(
        &e,
        &format!(
            "{PROJECT_HEAD}[validation]\ntier1 = [\"node scripts/ok.js\"]\ntier2 = [\"node scripts/check.js\"]\ngate = 2\n"
        ),
    );
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Arreglar el 401",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    wait_state(&e, &agent, "COMPLETED").await;
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    // Tier 1 pasa, tier 2 falla; el agente arregla en la misma sesión; vuelven a pasar.
    assert_eq!(
        validation_rows(&e, &agent),
        vec![
            (1, "PASS".to_string()),
            (2, "FAIL".to_string()),
            (1, "PASS".to_string()),
            (2, "PASS".to_string()),
        ]
    );
    // El fallo se le devolvió al agente con lo que importa, no con toda la salida.
    let feedback: String = one(
        &e,
        &format!(
            "SELECT content FROM messages WHERE agent_id = '{agent}' AND role = 'SYSTEM' AND content LIKE 'Symphony ejecutó la validación%'"
        ),
    );
    assert!(
        feedback.contains("FAIL auth.refresh.test.ts expected 200, received 401"),
        "{feedback}"
    );
    assert!(feedback.contains("1 fallaron, 3 pasaron"), "{feedback}");
    // El resultado quedó guardado con su salida completa como objeto de contexto.
    let (name, summary, object): (String, String, Option<String>) = {
        let conn = symphony_store::open_reader(&e.db).unwrap();
        conn.query_row(
            "SELECT name, summary, output_object_id FROM validation_checks
             WHERE status = 'FAIL' AND validation_run_id IN (SELECT id FROM validation_runs WHERE agent_id = ?1)",
            [&agent],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    };
    assert_eq!(name, "targeted_tests");
    assert!(summary.contains("expected 200, received 401"));
    assert!(object.is_some());
    // Dos runs del mismo agente (la sesión retomada), una sola tarea; nada que recuperar.
    assert_eq!(run_rows(&e, &agent).len(), 2);
    assert_eq!(count(&e, "agents"), 1);
    assert_eq!(count(&e, "recovery_items"), 0);
    let task: String = one(
        &e,
        &format!("SELECT status FROM tasks WHERE id = '{}'", created.task_id),
    );
    assert_eq!(task, "DONE");
    // Cada comando quedó en `tool_calls` (pasan por el scheduler como cualquier otro).
    let calls: i64 = one(
        &e,
        &format!(
            "SELECT COUNT(*) FROM tool_calls WHERE agent_id = '{agent}' AND tool_name LIKE 'validation:%'"
        ),
    );
    assert_eq!(calls, 4);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn repeated_validation_failures_block_the_task_with_a_decision() {
    let e = env(WORK, None).await;
    seed_validation(
        &e,
        &format!(
            "{PROJECT_HEAD}[validation]\ntier1 = [\"node scripts/fail.js\"]\ngate = 1\nmax_attempts = 2\n"
        ),
    );
    let created = e
        .runtime
        .create_agent(req(&e, "Nunca pasa", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    wait_state(&e, &agent, "BLOCKED").await;
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    assert_eq!(
        validation_rows(&e, &agent),
        vec![(1, "FAIL".to_string()), (1, "FAIL".to_string())]
    );
    let why: String = one(
        &e,
        &format!("SELECT state_reason FROM agents WHERE id = '{agent}'"),
    );
    assert!(
        why.contains("falló 2 veces") && why.contains("nunca pasa"),
        "{why}"
    );
    let kind: String = one(&e, "SELECT kind FROM recovery_items");
    assert_eq!(kind, "VALIDATION_FAILED");
    let task: String = one(
        &e,
        &format!("SELECT status FROM tasks WHERE id = '{}'", created.task_id),
    );
    assert_eq!(task, "BLOCKED");
    assert_eq!(run_rows(&e, &agent).len(), 2, "se le devolvió una vez");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_project_without_validation_completes_as_before_and_a_user_run_reports_it() {
    let e = env(WORK, None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "Sin validar", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    wait_state(&e, &agent, "COMPLETED").await;
    assert!(validation_rows(&e, &agent).is_empty());
    let err = e
        .runtime
        .validate(created.agent_id, 2, symphony_core::ValidationTrigger::User)
        .await
        .unwrap_err();
    assert!(err.contains("no define comandos de validación"), "{err}");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_user_validation_runs_tiers_in_order_and_stops_after_a_failure() {
    let e = env(WORK, None).await;
    seed_validation(
        &e,
        &format!(
            "{PROJECT_HEAD}[validation]\ngate = 0\ntier1 = [\"node scripts/ok.js\"]\ntier2 = [\"node scripts/fail.js\"]\ntier3 = [\"node scripts/ok.js\"]\n"
        ),
    );
    let created = e
        .runtime
        .create_agent(req(&e, "A mano", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    wait_state(&e, &created.agent_id.to_string(), "COMPLETED").await;
    // Con `gate = 0` la tarea no se validó sola; a pedido corre hasta el tier 3 y para en el 2.
    let v = e
        .runtime
        .validate(created.agent_id, 3, symphony_core::ValidationTrigger::User)
        .await
        .unwrap();
    assert!(!v.passed());
    let tiers: Vec<(u8, &str)> = v
        .tiers
        .iter()
        .map(|t| (t.tier, t.status.as_str()))
        .collect();
    assert_eq!(
        tiers,
        [(1, "PASS"), (2, "FAIL")],
        "el tier 3 no se pidió todavía"
    );
    assert!(v.headline().contains("nunca pasa"), "{}", v.headline());
    let trigger: String = one(&e, "SELECT DISTINCT trigger FROM validation_runs");
    assert_eq!(trigger, "USER");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_worktree_is_prepared_before_the_agent_starts_and_a_failure_creates_nothing() {
    let e = env(WORK, None).await;
    seed_validation(
        &e,
        &format!("{PROJECT_HEAD}[worktree]\nprepare = [\"node scripts/prepare.js\"]\n"),
    );
    let created = e
        .runtime
        .create_agent(req(&e, "Preparar", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(created.worktree.join("prepared.txt")).unwrap(),
        "listo"
    );
    wait_state(&e, &created.agent_id.to_string(), "COMPLETED").await;
    e.runtime.wait_executors().await;

    // Una preparación que falla no deja un agente a medias.
    let before = (count(&e, "agents"), count(&e, "worktrees"));
    std::fs::write(
        e.repo.join(".symphony/project.toml"),
        format!("{PROJECT_HEAD}[worktree]\nprepare = [\"node scripts/prepare_fails.js\"]\n"),
    )
    .unwrap();
    let err = e
        .runtime
        .create_agent(req(&e, "Falla", Execution::Exact("fake/fast".into())))
        .await
        .unwrap_err();
    assert_eq!(err.code(), "workspace_failed");
    let text = err.to_string();
    assert!(
        text.contains("node scripts/prepare_fails.js") && text.contains("código 3"),
        "{text}"
    );
    assert!(text.contains("no se pudo instalar"), "{text}");
    assert_eq!((count(&e, "agents"), count(&e, "worktrees")), before);
    assert_eq!(git(&e.repo, &["worktree", "list"]).lines().count(), 2);
    e.writer.shutdown();
}

// --- P08.S7: integración, conflictos, review agent y lotes ---------------------------

/// Como `writes`, pero el agente tarda: dura más que la creación de los siguientes.
fn slow_writes(path: &str, content: &str) -> String {
    format!(
        "[[step]]\nkind = \"say\"\ntext = \"Trabajando\"\n\n[[step]]\nkind = \"sleep\"\nms = 1500\n\n[[step]]\nkind = \"edit\"\npath = \"{path}\"\ncontent = \"{content}\\n\"\n"
    )
}

fn writes(path: &str, content: &str) -> String {
    format!(
        "[[step]]\nkind = \"say\"\ntext = \"Trabajando\"\n\n[[step]]\nkind = \"edit\"\npath = \"{path}\"\ncontent = \"{content}\\n\"\n"
    )
}

async fn make_chat(e: &Env) -> symphony_daemon::runtime::Created {
    let mut chat = req(e, "chat", Execution::DecideLater);
    chat.chat = true;
    e.runtime.create_agent(chat).await.unwrap()
}

async fn complete(
    e: &Env,
    title: &str,
    model: &str,
) -> (symphony_daemon::runtime::Created, String) {
    let created = e
        .runtime
        .create_agent(req(e, title, Execution::Exact(model.into())))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    wait_state(e, &agent, "COMPLETED").await;
    e.runtime.wait_executors().await;
    (created, agent)
}

/// P13.S5 (auditoría de merge): un `project.toml` ilegible no abre la puerta: el preflight
/// pide aprobación y no queda listo, en vez de usar la política por defecto.
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_project_toml_blocks_the_merge_instead_of_skipping_checks() {
    let e = env_with(&[("alpha", &writes("a.txt", "desde A"))], None).await;
    make_chat(&e).await;
    let (a, _) = complete(&e, "Agregar a.txt", "alpha/fast").await;
    std::fs::create_dir_all(e.repo.join(".symphony")).unwrap();
    std::fs::write(e.repo.join(".symphony/project.toml"), "esto [[[ no es toml").unwrap();

    let pre = e.runtime.merge_prepare(a.agent_id, false).await.unwrap();
    assert!(
        pre.requires_approval,
        "sin config legible se pide aprobación"
    );
    assert!(!pre.ready(), "{:?}", pre.checks);
    assert!(
        pre.checks
            .iter()
            .any(|c| c.name == "validation" && c.status == "FAIL"),
        "{:?}",
        pre.checks
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_finished_agent_integrates_into_the_chat_branch_only_with_approval_and_never_into_main() {
    let e = env_with(&[("alpha", &writes("a.txt", "desde A"))], None).await;
    let chat = make_chat(&e).await;
    let main_before = git(&e.repo, &["rev-parse", "main"]);
    let (a, _) = complete(&e, "Agregar a.txt", "alpha/fast").await;

    // El trabajo del agente quedó en un commit de su rama.
    assert!(
        git(&a.worktree, &["log", "--oneline", "-1"]).contains("T-"),
        "commit por tarea"
    );
    assert_eq!(git(&a.worktree, &["status", "--porcelain"]), "");

    let pre = e.runtime.merge_prepare(a.agent_id, false).await.unwrap();
    assert!(pre.ready(), "{:?}", pre.checks);
    assert!(pre.requires_approval && !pre.approved);
    assert_eq!(pre.target_branch, "symphony/chat");
    let names: Vec<_> = pre.checks.iter().map(|c| (c.name, c.status)).collect();
    assert!(
        names.contains(&("conflicts", "PASS")) && names.contains(&("policy", "WARN")),
        "{names:?}"
    );

    // Sin aprobación no se integra.
    let err = e
        .runtime
        .merge_integrate(&pre.merge_request)
        .await
        .unwrap_err();
    assert!(err.contains("aprobación"), "{err}");
    e.runtime.merge_approve(&pre.merge_request).await.unwrap();
    let commit = e.runtime.merge_integrate(&pre.merge_request).await.unwrap();

    // Entró a la rama del chat (su worktree la tiene), y `main` no se movió.
    assert!(chat.worktree.join("a.txt").is_file());
    assert_eq!(git(&chat.worktree, &["rev-parse", "HEAD"]), commit);
    assert_eq!(git(&e.repo, &["rev-parse", "main"]), main_before);
    assert!(!git(&e.repo, &["ls-tree", "-r", "--name-only", "main"]).contains("a.txt"));
    let status: String = one(
        &e,
        &format!(
            "SELECT status FROM merge_requests WHERE id = '{}'",
            pre.merge_request
        ),
    );
    assert_eq!(status, "MERGED");
    // El chat sabe lo que se integró.
    let note: i64 = one(
        &e,
        &format!(
            "SELECT COUNT(*) FROM messages WHERE agent_id = '{}' AND role = 'SYSTEM' AND content LIKE 'Integrado T-%'",
            chat.agent_id
        ),
    );
    assert_eq!(note, 1);
    // Una segunda vez no repite nada: ya está integrada.
    let again = e
        .runtime
        .merge_integrate(&pre.merge_request)
        .await
        .unwrap_err();
    assert!(again.contains("MERGED"), "{again}");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn two_agents_on_the_same_file_conflict_and_the_user_can_abort_or_resolve() {
    // Journey D: A y B crean `shared.txt` con contenido distinto.
    let e = env_with(
        &[
            ("alpha", &writes("shared.txt", "version de A")),
            ("beta", &writes("shared.txt", "version de B")),
        ],
        None,
    )
    .await;
    let chat = make_chat(&e).await;
    let (a, _) = complete(&e, "A toca shared", "alpha/fast").await;
    let (b, b_id) = complete(&e, "B toca shared", "beta/fast").await;

    let pre_a = e.runtime.merge_prepare(a.agent_id, false).await.unwrap();
    e.runtime.merge_approve(&pre_a.merge_request).await.unwrap();
    e.runtime
        .merge_integrate(&pre_a.merge_request)
        .await
        .unwrap();

    // B choca con lo que A ya integró: preflight bloqueado, conflicto y decisión pendiente.
    let pre_b = e.runtime.merge_prepare(b.agent_id, false).await.unwrap();
    assert!(!pre_b.ready());
    assert_eq!(pre_b.conflicts, vec!["shared.txt".to_string()]);
    let blocked: String = one(
        &e,
        &format!(
            "SELECT status FROM merge_requests WHERE id = '{}'",
            pre_b.merge_request
        ),
    );
    assert_eq!(blocked, "BLOCKED");
    let (path, other, resolution): (String, Option<String>, String) = {
        let conn = symphony_store::open_reader(&e.db).unwrap();
        conn.query_row(
            "SELECT path, other_agent_id, resolution FROM merge_conflicts WHERE merge_request_id = ?1",
            [&pre_b.merge_request],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    };
    assert_eq!(
        (path.as_str(), resolution.as_str()),
        ("shared.txt", "PENDING")
    );
    assert_eq!(
        other.as_deref(),
        Some(a.agent_id.to_string().as_str()),
        "choca con A"
    );
    let kind: String = one(
        &e,
        "SELECT kind FROM recovery_items WHERE kind = 'MERGE_CONFLICT'",
    );
    assert_eq!(kind, "MERGE_CONFLICT");
    let err = e
        .runtime
        .merge_integrate(&pre_b.merge_request)
        .await
        .unwrap_err();
    assert!(err.contains("BLOCKED"), "{err}");

    // Resolver a mano: el usuario integra la rama del chat en la de B y arregla el archivo.
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(&b.worktree)
        .args(["merge", "--no-edit", "symphony/chat"])
        .output()
        .unwrap();
    std::fs::write(b.worktree.join("shared.txt"), "version de A y de B\n").unwrap();
    git(&b.worktree, &["add", "."]);
    git(&b.worktree, &["commit", "-q", "-m", "resuelto a mano"]);
    let n = e
        .runtime
        .merge_mark_resolved(
            &pre_b.merge_request,
            symphony_core::ConflictResolution::Manual,
        )
        .await
        .unwrap();
    assert_eq!(n, 1);
    let again = e.runtime.merge_prepare(b.agent_id, false).await.unwrap();
    assert!(again.ready(), "{:?}", again.checks);
    assert_eq!(again.merge_request, pre_b.merge_request, "misma solicitud");
    e.runtime.merge_approve(&again.merge_request).await.unwrap();
    e.runtime
        .merge_integrate(&again.merge_request)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(chat.worktree.join("shared.txt"))
            .unwrap()
            .trim(),
        "version de A y de B"
    );
    let _ = b_id;
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn aborting_leaves_the_branch_untouched_and_closes_the_conflicts() {
    let e = env_with(
        &[
            ("alpha", &writes("shared.txt", "uno")),
            ("beta", &writes("shared.txt", "dos")),
        ],
        None,
    )
    .await;
    let chat = make_chat(&e).await;
    let (a, _) = complete(&e, "A", "alpha/fast").await;
    let (b, _) = complete(&e, "B", "beta/fast").await;
    let pre_a = e.runtime.merge_prepare(a.agent_id, false).await.unwrap();
    e.runtime.merge_approve(&pre_a.merge_request).await.unwrap();
    e.runtime
        .merge_integrate(&pre_a.merge_request)
        .await
        .unwrap();
    let head = git(&chat.worktree, &["rev-parse", "HEAD"]);

    let pre_b = e.runtime.merge_prepare(b.agent_id, false).await.unwrap();
    assert!(!pre_b.ready());
    e.runtime.merge_abort(&pre_b.merge_request).await.unwrap();
    let (status, pending): (String, i64) = {
        let conn = symphony_store::open_reader(&e.db).unwrap();
        let s = conn
            .query_row(
                "SELECT status FROM merge_requests WHERE id = ?1",
                [&pre_b.merge_request],
                |r| r.get(0),
            )
            .unwrap();
        let p = conn
            .query_row(
                "SELECT COUNT(*) FROM merge_conflicts WHERE merge_request_id = ?1 AND resolution = 'PENDING'",
                [&pre_b.merge_request],
                |r| r.get(0),
            )
            .unwrap();
        (s, p)
    };
    assert_eq!((status.as_str(), pending), ("ABORTED", 0));
    assert_eq!(git(&chat.worktree, &["rev-parse", "HEAD"]), head);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_review_agent_recommends_and_never_merges() {
    let reviewer_script = r#"
[[step]]
kind = "say"
text = '''Revisé el diff.
```review
{"recommendation":"REQUEST_CHANGES","findings":[{"severity":"high","file":"a.txt","text":"falta el caso de error"}]}
```'''
"#;
    let e = env_with(
        &[
            ("alpha", &writes("a.txt", "desde A")),
            ("rev", reviewer_script),
        ],
        None,
    )
    .await;
    let chat = make_chat(&e).await;
    let main_before = git(&e.repo, &["rev-parse", "main"]);
    let chat_before = git(&chat.worktree, &["rev-parse", "HEAD"]);
    let (a, _) = complete(&e, "Para revisar", "alpha/fast").await;
    // En el daemon real el reactor ve terminar al revisor y guarda su veredicto.
    let reactor = symphony_daemon::dependencies::spawn(
        e.runtime.clone(),
        tokio_util::sync::CancellationToken::new(),
    );

    let reviewer = e
        .runtime
        .review_start(a.agent_id, Some("rev/fast".into()), None)
        .await
        .unwrap();
    wait_state(&e, &reviewer.to_string(), "COMPLETED").await;
    e.runtime.wait_executors().await;
    // El veredicto se guarda al terminar el revisor.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let (status, rec, findings) = loop {
        let row: Option<(String, Option<String>, Option<String>)> = {
            let conn = symphony_store::open_reader(&e.db).unwrap();
            conn.query_row(
                "SELECT status, recommendation, findings_json FROM reviews WHERE reviewer_agent_id = ?1",
                [reviewer.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok()
        };
        if let Some(r) = row.filter(|r| r.0 != "RUNNING") {
            break r;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "el veredicto no llegó"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(status, "DONE");
    assert_eq!(rec.as_deref(), Some("REQUEST_CHANGES"));
    assert!(findings.unwrap().contains("falta el caso de error"));

    // Es una tarea de review, no de trabajo, y no integró nada.
    let kind: String = one(
        &e,
        &format!(
            "SELECT t.kind FROM tasks t JOIN agents a ON a.task_id = t.id WHERE a.id = '{reviewer}'"
        ),
    );
    assert_eq!(kind, "REVIEW");
    assert_eq!(git(&e.repo, &["rev-parse", "main"]), main_before);
    assert_eq!(git(&chat.worktree, &["rev-parse", "HEAD"]), chat_before);
    assert_eq!(count(&e, "merge_requests"), 0);
    reactor.abort();
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_creates_agents_with_dependencies_and_integrates_what_finished() {
    let e = env_with(&[("alpha", &slow_writes("fix.txt", "hecho"))], None).await;
    let chat = make_chat(&e).await;
    // El reactor ya existe cuando se crea el lote (como en el daemon): bajo carga, el primer
    // agente puede terminar mientras se crean los demás y esa transición no debe perderse.
    let reactor = symphony_daemon::dependencies::spawn(
        e.runtime.clone(),
        tokio_util::sync::CancellationToken::new(),
    );
    let plan = "# Plan: lote\n## Fase 1: base\n- [ ] [A] Primera\n- [ ] [B] Segunda (needs: A)\n- [ ] [C] Tercera\n";
    let batch = e
        .runtime
        .batch_create(symphony_daemon::batch::BatchRequest {
            project_root: e.repo.clone(),
            title: "Lote de prueba".into(),
            plan: plan.into(),
            execution: Execution::Exact("alpha/fast".into()),
            max_parallel: Some(1),
            failover: FailoverPolicy::Any,
            context_mode: ContextMode::Balanced,
        })
        .await
        .unwrap();
    assert_eq!(batch.parallel, 1);
    let by_id: std::collections::HashMap<_, _> =
        batch.agents.iter().map(|a| (a.id.as_str(), a)).collect();
    // B espera a A; con tope 1, C (misma ola que A) espera a A también.
    assert_eq!(by_id["B"].depends_on, vec![by_id["A"].task_code.clone()]);
    assert_eq!(by_id["C"].depends_on, vec![by_id["A"].task_code.clone()]);
    // Con la máquina cargada, A (1,5 s) puede terminar antes de que nazca B: entonces B nace
    // liberado. Si no, espera.
    if by_id["B"].state != AgentState::WaitingDependency {
        let a_state: String = one(
            &e,
            &format!(
                "SELECT state FROM agents WHERE id = '{}'",
                by_id["A"].agent_id
            ),
        );
        assert_eq!(
            a_state, "COMPLETED",
            "B no esperó a A: {:?}",
            by_id["B"].state
        );
    }
    assert_ne!(by_id["A"].state, AgentState::WaitingDependency);

    // El chat sabe lo que delegó.
    let note: i64 = one(
        &e,
        &format!(
            "SELECT COUNT(*) FROM messages WHERE agent_id = '{}' AND content LIKE 'Delegado «Lote de prueba»%'",
            chat.agent_id
        ),
    );
    assert_eq!(note, 1);

    // Con la dependencia en marcha, el lote termina solo (el reactor arranca a B y a C).
    for a in &batch.agents {
        wait_state(&e, &a.agent_id.to_string(), "COMPLETED").await;
    }
    e.runtime.wait_executors().await;
    reactor.abort();

    let status = e.runtime.batch_status(batch.milestone).unwrap();
    assert_eq!(status["done"], 3);
    assert_eq!(
        status["pending_integration"], 3,
        "terminó y no entró a la rama del chat"
    );
    let milestone: String = one(
        &e,
        &format!(
            "SELECT status FROM milestones WHERE id = '{}'",
            batch.milestone
        ),
    );
    assert_eq!(
        milestone, "APPROVED",
        "sin aprobación manual, el gate se cierra solo"
    );

    // Los tres escriben `fix.txt` con el mismo contenido: entran sin conflicto.
    let r = e
        .runtime
        .batch_integrate(batch.milestone, true)
        .await
        .unwrap();
    let results: Vec<&str> = r["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["result"].as_str().unwrap())
        .collect();
    assert_eq!(results, ["integrada", "integrada", "integrada"], "{r}");
    assert!(chat.worktree.join("fix.txt").is_file());
    let status = e.runtime.batch_status(batch.milestone).unwrap();
    assert_eq!(status["merged"], 3);
    assert_eq!(status["pending_integration"], 0);
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_with_an_unknown_need_or_a_cycle_is_rejected_before_creating_anything() {
    let e = env(WORK, None).await;
    for plan in [
        "# Plan: x\n## Fase 1: a\n- [ ] [A] uno (needs: Z)\n",
        "# Plan: x\n## Fase 1: a\n- [ ] [A] uno (needs: B)\n- [ ] [B] dos (needs: A)\n",
        "# Plan: x\n",
    ] {
        let err = e
            .runtime
            .batch_create(symphony_daemon::batch::BatchRequest {
                project_root: e.repo.clone(),
                title: "malo".into(),
                plan: plan.into(),
                execution: Execution::Exact("fake/fast".into()),
                max_parallel: None,
                failover: FailoverPolicy::Any,
                context_mode: ContextMode::Balanced,
            })
            .await
            .unwrap_err();
        assert!(!err.is_empty());
    }
    assert_eq!(count(&e, "agents"), 0);
    e.writer.shutdown();
}

/// P11.5.S3 (L2): un turno con texto en trozos. Los suscriptores ven cada trozo, pero en la base
/// queda un solo mensaje del asistente y ningún evento `AssistantDelta`.
#[tokio::test(flavor = "multi_thread")]
async fn streamed_text_is_broadcast_but_only_the_full_message_is_stored() {
    let e = env(
        "[[step]]
kind = \"say\"
text = \"respuesta con varios trozos de texto\"
deltas = true
",
        None,
    )
    .await;
    let mut rx = e.bus.subscribe();
    e.runtime
        .create_agent(req(&e, "deltas", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    let mut seen = String::new();
    while let Ok(ev) = rx.try_recv() {
        if let AgentEvent::AssistantDelta { text } = &ev.event {
            seen.push_str(text);
        }
    }
    assert_eq!(seen, "respuesta con varios trozos de texto");
    let deltas: i64 = one(
        &e,
        "SELECT COUNT(*) FROM events WHERE type = 'AssistantDelta'",
    );
    assert_eq!(deltas, 0);
    let said: i64 = one(&e, "SELECT COUNT(*) FROM messages WHERE role = 'ASSISTANT'");
    assert_eq!(said, 1);
    e.writer.shutdown();
}

/// P11.5.S4: el router recibe el tamaño del handoff; un modelo cuya ventana no lo cabe queda
/// descartado por `CONTEXT` y el failover va al siguiente.
#[tokio::test(flavor = "multi_thread")]
async fn failover_skips_a_model_whose_window_cannot_hold_the_handoff() {
    let e = env_with(&[("alpha", QUOTA), ("beta", SAY), ("gamma", SAY)], None).await;
    // Sin esto gana beta (a igualdad de puntaje, por id): su ventana es demasiado chica.
    e.writer
        .handle()
        .write(Box::new(|t| {
            t.execute(
                "UPDATE models SET context_window = 5 WHERE provider_id = 'beta'",
                [],
            )?;
            Ok(())
        }))
        .await
        .unwrap();
    let mut r = req(&e, "ventana", Execution::Exact("alpha/fast".into()));
    r.failover = symphony_core::FailoverPolicy::Any;
    let agent = e
        .runtime
        .create_agent(r)
        .await
        .unwrap()
        .agent_id
        .to_string();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let providers: String = one(
        &e,
        &format!(
            "SELECT group_concat(provider_id, '>') FROM (SELECT provider_id FROM agent_runs WHERE agent_id = '{agent}' ORDER BY seq)"
        ),
    );
    assert_eq!(providers, "alpha>gamma");
    let rej = rejections(&e, &agent);
    assert!(
        rej.iter()
            .any(|(m, r)| m.starts_with("beta/") && r.as_deref() == Some("CONTEXT")),
        "{rej:?}"
    );
    e.writer.shutdown();
}

/// P11.5.S4 (FLOW §13.3): si hubo actividad que pide checkpoint después del último, el handoff
/// lo marca como obsoleto (el código se rehace del worktree).
#[tokio::test(flavor = "multi_thread")]
async fn a_handoff_flags_a_checkpoint_older_than_the_last_activity() {
    use symphony_daemon::bus::{BusEvent, EventSource};
    let e = env(SAY, None).await;
    let created = e
        .runtime
        .create_agent(req(&e, "obsoleto", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let fresh = e
        .runtime
        .prepare_handoff(created.agent_id, "prueba")
        .await
        .unwrap();
    assert!(!fresh.stale, "un run terminado deja el checkpoint al día");

    let project: String = one(&e, "SELECT id FROM projects");
    e.bus
        .publish(BusEvent {
            project_id: project,
            agent_id: Some(created.agent_id.to_string()),
            run_id: None,
            source: EventSource::System,
            event: AgentEvent::FileModified {
                tool: "Write".into(),
                path: Some("a.txt".into()),
            },
            // Más nuevo que cualquier checkpoint que se tome ahora.
            occurred_at: now_secs() * 1000 + 3_600_000,
        })
        .await
        .unwrap();
    e.bus.checkpoints_idle().await;
    let stale = e
        .runtime
        .prepare_handoff(created.agent_id, "prueba")
        .await
        .unwrap();
    assert!(stale.stale);
    e.writer.shutdown();
}

/// P11.5.S5: el tope de simultáneos de un lote sigue a la cuota de ahora. Con la cuota conocida
/// en 30 % solo caben 2 de 3: el tercero espera `READY` y arranca cuando termina uno.
///
/// Cada agente espera una «puerta» (un archivo con el id de su tarea en una carpeta del test):
/// el orden de los hechos lo manda el test, no el reloj, así que no depende de la carga.
#[tokio::test(flavor = "multi_thread")]
async fn a_batch_cap_follows_the_quota_that_is_known_now() {
    let gates = tempfile::tempdir().unwrap();
    let wait_for_gate = r#"const fs = require("fs"); const f = process.argv[1] + "/" + process.env.SYMPHONY_TASK_ID; setInterval(() => { if (fs.existsSync(f)) process.exit(0); }, 50);"#;
    let script = format!(
        "[[step]]\nkind = \"say\"\ntext = \"Trabajando\"\n\n[[step]]\nkind = \"run\"\ncommand = [\"node\", \"-e\", '{wait_for_gate}', '{}']\n\n[[step]]\nkind = \"edit\"\npath = \"fix.txt\"\ncontent = \"hecho\\n\"\n",
        gates.path().display()
    );
    let e = env_with(&[("alpha", &script)], None).await;
    make_chat(&e).await;
    let reactor = symphony_daemon::dependencies::spawn(
        e.runtime.clone(),
        tokio_util::sync::CancellationToken::new(),
    );
    let plan = "# Plan: cuota\n## Fase 1: base\n- [ ] [A] Base\n- [ ] [B] Uno (needs: A)\n- [ ] [C] Dos (needs: A)\n- [ ] [D] Tres (needs: A)\n";
    let batch = e
        .runtime
        .batch_create(symphony_daemon::batch::BatchRequest {
            project_root: e.repo.clone(),
            title: "Lote por cuota".into(),
            plan: plan.into(),
            execution: Execution::Exact("alpha/fast".into()),
            max_parallel: Some(3),
            failover: FailoverPolicy::Any,
            context_mode: ContextMode::Balanced,
        })
        .await
        .unwrap();
    assert_eq!(batch.parallel, 3, "al crearlo no hay cuota conocida");
    // A sigue detenido en su puerta: la cuota conocida baja a 30 % → la mitad (2 de 3).
    set_health(&e, "alpha", "HEALTHY", "KNOWN", Some(0.30), None).await;
    let agent_of = |name: &str| {
        batch
            .agents
            .iter()
            .find(|a| a.id == name)
            .unwrap()
            .agent_id
            .to_string()
    };
    let open_gate = |agent: &str| {
        let task: String = one(
            &e,
            &format!("SELECT task_id FROM agents WHERE id = '{agent}'"),
        );
        std::fs::write(gates.path().join(task), "").unwrap();
    };
    let (a, b, c, d) = (agent_of("A"), agent_of("B"), agent_of("C"), agent_of("D"));
    open_gate(&a);

    // D queda esperando cupo con B y C en marcha (detenidos en su puerta).
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        let why: Option<String> = one(
            &e,
            &format!("SELECT state_reason FROM agents WHERE id = '{d}'"),
        );
        if why.as_deref().is_some_and(|w| w.contains("cupo")) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "D nunca quedó esperando cupo; agentes: {:?}",
            [&a, &b, &c, &d].map(|x| {
                let state: String = one(&e, &format!("SELECT state FROM agents WHERE id = '{x}'"));
                let why: Option<String> = one(
                    &e,
                    &format!("SELECT state_reason FROM agents WHERE id = '{x}'"),
                );
                (state, why)
            })
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let runs = |agent: &str| -> i64 {
        one(
            &e,
            &format!("SELECT COUNT(*) FROM agent_runs WHERE agent_id = '{agent}'"),
        )
    };
    assert_eq!((runs(&b), runs(&c), runs(&d)), (1, 1, 0));
    assert_eq!(
        one::<String>(&e, &format!("SELECT state FROM agents WHERE id = '{d}'")),
        "READY"
    );
    // Cuando uno termina hay cupo: D arranca y el lote completo termina.
    open_gate(&b);
    open_gate(&c);
    open_gate(&d);
    for agent in [&a, &b, &c, &d] {
        wait_state(&e, agent, "COMPLETED").await;
    }
    assert_eq!(runs(&d), 1);
    e.runtime.wait_executors().await;
    reactor.abort();
    e.writer.shutdown();
}

/// P11.5.S5: un agente que nació esperando a una tarea que terminó mientras se creaba (el
/// reactor ya había pasado) no se queda en `WAITING_DEPENDENCY` para siempre.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_born_waiting_for_a_task_that_just_finished_is_released() {
    let e = env_with(&[("alpha", &writes("a.txt", "hecho"))], None).await;
    let (a, _) = complete(&e, "Primera", "alpha/fast").await;
    let b = e
        .runtime
        .create_agent(req(&e, "Segunda", Execution::DecideLater))
        .await
        .unwrap();
    // Reproduce el resultado de la carrera: B quedó esperando a A, que ya está hecha.
    let (task_a, task_b, agent_b) = (
        a.task_id.to_string(),
        b.task_id.to_string(),
        b.agent_id.to_string(),
    );
    e.writer
        .handle()
        .write(Box::new(move |t| {
            t.execute(
                "INSERT INTO task_dependencies (task_id, depends_on_task_id, created_at) VALUES (?1, ?2, 0)",
                [&task_b, &task_a],
            )?;
            t.execute(
                "UPDATE agents SET state = 'WAITING_DEPENDENCY', state_reason = 'espera a T-1' WHERE id = ?1",
                [&agent_b],
            )?;
            t.execute("UPDATE tasks SET status = 'WAITING' WHERE id = ?1", [&task_b])?;
            Ok(())
        }))
        .await
        .unwrap();
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT state FROM agents WHERE id = '{}'", b.agent_id)
        ),
        "WAITING_DEPENDENCY"
    );
    symphony_daemon::dependencies::recheck_new(&e.runtime, b.task_id).await;
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT state FROM agents WHERE id = '{}'", b.agent_id)
        ),
        "READY"
    );
    e.writer.shutdown();
}

/// Espera a que la revisión de `reviewer` deje de estar en curso.
async fn wait_review(e: &Env, reviewer: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let status: Option<String> = symphony_store::open_reader(&e.db)
            .unwrap()
            .query_row(
                "SELECT status FROM reviews WHERE reviewer_agent_id = ?1",
                [reviewer],
                |r| r.get(0),
            )
            .ok();
        if status.as_deref().is_some_and(|s| s != "RUNNING") {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "el veredicto no llegó"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn review_script(recommendation: &str) -> String {
    format!(
        "[[step]]\nkind = \"say\"\ntext = '''Revisé el chat.\n```review\n{{\"recommendation\":\"{recommendation}\",\"findings\":[{{\"severity\":\"medium\",\"file\":\"notas.md\",\"text\":\"detalle\"}}]}}\n```'''\n"
    )
}

/// Un commit en la rama del chat (como el commit por turno).
fn chat_commit(chat: &symphony_daemon::runtime::Created, file: &str, content: &str) {
    std::fs::write(chat.worktree.join(file), content).unwrap();
    git(&chat.worktree, &["add", "-A"]);
    git(&chat.worktree, &["commit", "-q", "-m", "chat: cambios"]);
}

fn check_status(pre: &symphony_daemon::chat_merge::ChatMerge, name: &str) -> String {
    pre.checks
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.status.to_string())
        .unwrap_or_else(|| format!("sin check {name}"))
}

/// P11.5.S6 (ADR-0006 §5): el chat entra a `main` solo con confirmación explícita y una
/// revisión que recomiende integrar; el merge es un commit `--no-ff`.
#[tokio::test(flavor = "multi_thread")]
async fn the_chat_merges_into_main_only_with_confirmation_and_an_approving_review() {
    let e = env_with(&[("rev", &review_script("APPROVE"))], None).await;
    let chat = make_chat(&e).await;
    let reactor = symphony_daemon::dependencies::spawn(
        e.runtime.clone(),
        tokio_util::sync::CancellationToken::new(),
    );
    let main_before = git(&e.repo, &["rev-parse", "main"]);

    // Sin nada que integrar: el preflight lo dice y no hay merge.
    let pre = e.runtime.chat_merge_prepare(&e.repo, false).await.unwrap();
    assert!(!pre.ready());
    assert_eq!(check_status(&pre, "ahead"), "FAIL");

    chat_commit(&chat, "notas.md", "# notas\n");
    let pre = e.runtime.chat_merge_prepare(&e.repo, false).await.unwrap();
    assert_eq!(pre.ahead, 1);
    assert_eq!(pre.target, "main");
    assert!(pre.ready(), "{:?}", pre.checks);
    assert_eq!(check_status(&pre, "review"), "WARN");

    // Sin confirmación: no. Con confirmación pero sin revisión: tampoco.
    let err = e
        .runtime
        .chat_merge_integrate(&e.repo, false, false)
        .await
        .unwrap_err();
    assert!(err.contains("confirmación"), "{err}");
    let err = e
        .runtime
        .chat_merge_integrate(&e.repo, true, false)
        .await
        .unwrap_err();
    assert!(err.contains("revisión"), "{err}");
    assert_eq!(git(&e.repo, &["rev-parse", "main"]), main_before);

    // La revisión aprueba: ahora sí.
    let reviewer = e
        .runtime
        .chat_merge_review(&e.repo, Some("rev/fast".into()), None)
        .await
        .unwrap();
    wait_state(&e, &reviewer.to_string(), "COMPLETED").await;
    wait_review(&e, &reviewer.to_string()).await;
    let pre = e.runtime.chat_merge_prepare(&e.repo, false).await.unwrap();
    assert_eq!(check_status(&pre, "review"), "PASS", "{:?}", pre.checks);
    assert_eq!(pre.review.as_ref().unwrap()["recommendation"], "APPROVE");

    let commit = e
        .runtime
        .chat_merge_integrate(&e.repo, true, false)
        .await
        .unwrap();
    assert_eq!(git(&e.repo, &["rev-parse", "main"]), commit);
    assert_eq!(
        git(&e.repo, &["rev-list", "--merges", "--count", "main"]),
        "1",
        "un commit de merge --no-ff"
    );
    assert!(e.repo.join("notas.md").is_file());
    assert_eq!(count(&e, "merge_requests"), 1);
    let note: i64 = one(
        &e,
        &format!(
            "SELECT COUNT(*) FROM messages WHERE agent_id = '{}' AND content LIKE 'Integrado `symphony/chat` en `main`%'",
            chat.agent_id
        ),
    );
    assert_eq!(note, 1);
    // Ya no hay nada que integrar.
    let pre = e.runtime.chat_merge_prepare(&e.repo, false).await.unwrap();
    assert_eq!(check_status(&pre, "ahead"), "FAIL");
    reactor.abort();
    e.writer.shutdown();
}

/// P11.5.S6: un revisor que pide cambios bloquea la integración (salvo orden explícita), y un
/// repo principal sucio, en otra rama o con conflictos la bloquea siempre sin tocar `main`.
#[tokio::test(flavor = "multi_thread")]
async fn chat_to_main_is_blocked_by_requested_changes_a_dirty_repo_or_conflicts() {
    let e = env_with(&[("rev", &review_script("REQUEST_CHANGES"))], None).await;
    let chat = make_chat(&e).await;
    let reactor = symphony_daemon::dependencies::spawn(
        e.runtime.clone(),
        tokio_util::sync::CancellationToken::new(),
    );
    chat_commit(&chat, "README.md", "# del chat\n");
    let main_before = git(&e.repo, &["rev-parse", "main"]);

    let reviewer = e
        .runtime
        .chat_merge_review(&e.repo, Some("rev/fast".into()), None)
        .await
        .unwrap();
    wait_state(&e, &reviewer.to_string(), "COMPLETED").await;
    wait_review(&e, &reviewer.to_string()).await;
    let pre = e.runtime.chat_merge_prepare(&e.repo, false).await.unwrap();
    assert_eq!(check_status(&pre, "review"), "FAIL");
    let err = e
        .runtime
        .chat_merge_integrate(&e.repo, true, false)
        .await
        .unwrap_err();
    assert!(err.contains("REQUEST_CHANGES"), "{err}");
    assert_eq!(git(&e.repo, &["rev-parse", "main"]), main_before);

    // Cambios sin commitear en tu repo: se rechaza aunque pidas saltarte la revisión.
    let readme = std::fs::read_to_string(e.repo.join("README.md")).unwrap();
    std::fs::write(e.repo.join("README.md"), format!("{readme}\nlocal\n")).unwrap();
    let err = e
        .runtime
        .chat_merge_integrate(&e.repo, true, true)
        .await
        .unwrap_err();
    assert!(err.contains("main_clean"), "{err}");
    git(&e.repo, &["checkout", "--", "README.md"]);

    // `main` con un cambio propio en la misma línea: conflicto, `main` intacto.
    std::fs::write(e.repo.join("README.md"), "# de main\n").unwrap();
    git(&e.repo, &["commit", "-q", "-am", "main: README"]);
    let main_now = git(&e.repo, &["rev-parse", "main"]);
    let pre = e.runtime.chat_merge_prepare(&e.repo, false).await.unwrap();
    assert_eq!(check_status(&pre, "conflicts"), "FAIL");
    assert_eq!(pre.conflicts, ["README.md"]);
    let err = e
        .runtime
        .chat_merge_integrate(&e.repo, true, true)
        .await
        .unwrap_err();
    assert!(err.contains("conflicts"), "{err}");
    assert_eq!(git(&e.repo, &["rev-parse", "main"]), main_now);
    assert!(e.repo.join("README.md").is_file());
    assert_eq!(
        git(&e.repo, &["status", "--porcelain", "--untracked-files=no"]),
        "",
        "el repo no quedó a medias"
    );
    reactor.abort();
    e.writer.shutdown();
}

/// P11.5.S10: si el reactor se atrasa y pierde cambios de estado (`Lagged`), el barrido libera
/// a quien esperaba una tarea terminada y bloquea (con decisión) a quien esperaba una fallida.
#[tokio::test(flavor = "multi_thread")]
async fn a_sweep_after_a_lagged_reactor_releases_and_blocks_the_waiting_agents() {
    let e = env_with(&[("alpha", &writes("a.txt", "hecho"))], None).await;
    let (a, _) = complete(&e, "Primera", "alpha/fast").await;
    let waiting_on = |dep_task: String, number_title: &str| {
        let e = &e;
        let title = number_title.to_string();
        async move {
            let b = e
                .runtime
                .create_agent(req(e, &title, Execution::DecideLater))
                .await
                .unwrap();
            let (task_b, agent_b) = (b.task_id.to_string(), b.agent_id.to_string());
            e.writer
                .handle()
                .write(Box::new(move |t| {
                    t.execute(
                        "INSERT INTO task_dependencies (task_id, depends_on_task_id, created_at) VALUES (?1, ?2, 0)",
                        [&task_b, &dep_task],
                    )?;
                    t.execute(
                        "UPDATE agents SET state = 'WAITING_DEPENDENCY', state_reason = 'espera' WHERE id = ?1",
                        [&agent_b],
                    )?;
                    t.execute("UPDATE tasks SET status = 'WAITING' WHERE id = ?1", [&task_b])?;
                    Ok(())
                }))
                .await
                .unwrap();
            b.agent_id.to_string()
        }
    };
    let on_done = waiting_on(a.task_id.to_string(), "Espera a la hecha").await;
    // Otra tarea que ya falló.
    let (failed, _) = complete(&e, "Segunda", "alpha/fast").await;
    let failed_task = failed.task_id.to_string();
    let failed_agent = failed.agent_id.to_string();
    e.writer
        .handle()
        .write(Box::new(move |t| {
            t.execute(
                "UPDATE tasks SET status = 'FAILED' WHERE id = ?1",
                [&failed_task],
            )?;
            t.execute(
                "UPDATE agents SET state = 'FAILED' WHERE id = ?1",
                [&failed_agent],
            )?;
            Ok(())
        }))
        .await
        .unwrap();
    let on_failed = waiting_on(failed.task_id.to_string(), "Espera a la fallida").await;

    symphony_daemon::dependencies::recheck_waiting(&e.runtime).await;
    let state = |agent: &str| -> String {
        one(
            &e,
            &format!("SELECT state FROM agents WHERE id = '{agent}'"),
        )
    };
    assert_eq!(state(&on_done), "READY", "la dependencia terminó bien");
    assert_eq!(
        state(&on_failed),
        "BLOCKED",
        "la dependencia falló: decide el usuario"
    );
    e.writer.shutdown();
}

// --- P12: plugins en el gate de validación y en el handoff -----------------------------

fn fake_plugin() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let ok = Command::new(env!("CARGO"))
                .args([
                    "build",
                    "-q",
                    "-p",
                    "symphony-testkit",
                    "--bin",
                    "fake-plugin",
                ])
                .status()
                .unwrap()
                .success();
            assert!(ok, "no se pudo construir fake-plugin");
            symphony_testkit::pinned_bin(
                &PathBuf::from(env!("CARGO_BIN_EXE_symphonyd"))
                    .with_file_name(format!("fake-plugin{}", std::env::consts::EXE_SUFFIX)),
            )
        })
        .clone()
}

/// Instala `fake-plugin` como plugin `todo-check` (instalación global en `<home>/plugins`) y lo
/// activa en el proyecto.
fn install_plugin(e: &Env, mode: &str, kind: &str, section: &str) {
    let dir = e.home.join("plugins/todo-check");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = format!("fake-plugin{}", std::env::consts::EXE_SUFFIX);
    std::fs::copy(fake_plugin(), dir.join(&exe)).unwrap();
    std::fs::write(
        dir.join("plugin.toml"),
        format!(
            "name = \"todo-check\"\nversion = \"0.1.0\"\nplugin_api = 1\nkind = \"{kind}\"\n\
             exec = [\"./fake-plugin\", \"{mode}\", \"todo-check\", \"{kind}\"]\ntimeout_secs = 20\n"
        ),
    )
    .unwrap();
    std::fs::create_dir_all(e.repo.join(".symphony")).unwrap();
    std::fs::write(
        e.repo.join(".symphony/project.toml"),
        format!("{PROJECT_HEAD}{section}"),
    )
    .unwrap();
}

fn plugin_checks(e: &Env, agent: &str) -> Vec<(String, String, Option<String>)> {
    let conn = symphony_store::open_reader(&e.db).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT name, status, summary FROM validation_checks
             WHERE validation_run_id IN (SELECT id FROM validation_runs WHERE agent_id = ?1)
             ORDER BY rowid",
        )
        .unwrap();
    stmt.query_map([agent], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

const PLUGIN_GATE: &str =
    "[plugins]\nvalidators = [\"todo-check\"]\n\n[validation]\nmax_attempts = 1\n";

#[tokio::test(flavor = "multi_thread")]
async fn a_validator_plugin_that_finds_a_problem_fails_the_gate_with_its_findings() {
    let e = env(&writes("a.txt", "// TODO sin issue"), None).await;
    install_plugin(&e, "validate-fail", "validator", PLUGIN_GATE);
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Agregar a.txt",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();

    let checks = plugin_checks(&e, &agent);
    assert_eq!(checks.len(), 1, "{checks:?}");
    assert_eq!(
        (checks[0].0.as_str(), checks[0].1.as_str()),
        ("plugin", "FAIL")
    );
    let summary = checks[0].2.clone().unwrap();
    assert!(summary.contains("TODO sin issue"), "{summary}");
    assert_eq!(validation_rows(&e, &agent), vec![(2, "FAIL".to_string())]);
    // El daemon sigue vivo: otro agente se crea sin problema.
    assert!(
        e.runtime
            .create_agent(req(&e, "Otro", Execution::Exact("fake/fast".into())))
            .await
            .is_ok()
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_validator_plugin_that_passes_lets_the_task_complete() {
    let e = env(&writes("a.txt", "limpio"), None).await;
    install_plugin(&e, "validate-ok", "validator", PLUGIN_GATE);
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Agregar a.txt",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    wait_state(&e, &agent, "COMPLETED").await;
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let checks = plugin_checks(&e, &agent);
    assert_eq!(
        checks.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(),
        ["PASS"],
        "{checks:?}"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_validator_plugin_that_dies_or_is_missing_is_skipped_never_passed_off_as_ok() {
    let e = env(&writes("a.txt", "x"), None).await;
    install_plugin(&e, "crash", "validator", PLUGIN_GATE);
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Con plugin roto",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    wait_state(&e, &agent, "COMPLETED").await;
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let checks = plugin_checks(&e, &agent);
    assert_eq!(checks.len(), 1, "{checks:?}");
    assert_eq!(checks[0].1, "SKIPPED");
    assert!(
        checks[0].2.clone().unwrap().contains("no se ejecutó"),
        "{checks:?}"
    );

    // Un plugin activado que no está instalado: también SKIPPED, con el motivo.
    std::fs::write(
        e.repo.join(".symphony/project.toml"),
        format!("{PROJECT_HEAD}[plugins]\nvalidators = [\"no-esta\"]\n\n[validation]\nmax_attempts = 1\n"),
    )
    .unwrap();
    let second = e
        .runtime
        .create_agent(req(&e, "Sin plugin", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    let agent2 = second.agent_id.to_string();
    wait_state(&e, &agent2, "COMPLETED").await;
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let checks = plugin_checks(&e, &agent2);
    assert_eq!(checks[0].1, "SKIPPED");
    assert!(
        checks[0].2.clone().unwrap().contains("no está instalado"),
        "{checks:?}"
    );
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_list_and_check_report_what_is_installed_and_whether_it_runs() {
    let e = env(&writes("a.txt", "x"), None).await;
    install_plugin(&e, "validate-ok", "validator", PLUGIN_GATE);
    let root = e.repo.display().to_string();
    let list = e.runtime.plugin_list(Some(&root));
    let plugins = list["plugins"].as_array().unwrap();
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0]["name"], "todo-check");
    assert_eq!(plugins[0]["enabled"], true);
    assert_eq!(plugins[0]["supported"], true);
    assert_eq!(list["problems"].as_array().unwrap().len(), 0);

    let ok = e.runtime.plugin_check("todo-check").await.unwrap();
    assert_eq!(ok["ok"], true, "{ok}");
    assert!(e.runtime.plugin_check("nada").await.is_err());

    // Un plugin que no arranca bien se reporta, no tira nada.
    install_plugin(&e, "crash", "validator", PLUGIN_GATE);
    let bad = e.runtime.plugin_check("todo-check").await.unwrap();
    assert_eq!(bad["ok"], false);
    assert!(bad["error"].as_str().unwrap().contains("134"), "{bad}");
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_context_transformer_rewrites_the_handoff_and_a_broken_one_is_ignored() {
    let e = env_with(&[("alpha", &writes("a.txt", "hecho")), ("beta", SAY)], None).await;
    install_plugin(
        &e,
        "transform-upper",
        "context-transformer",
        "[plugins]\ncontext_transformers = [\"todo-check\"]\n",
    );
    let created = e
        .runtime
        .create_agent(req(&e, "Trabajo", Execution::Exact("alpha/fast".into())))
        .await
        .unwrap();
    wait_state(&e, &created.agent_id.to_string(), "COMPLETED").await;
    e.runtime.wait_executors().await;
    let h = e
        .runtime
        .prepare_handoff(created.agent_id, "prueba")
        .await
        .unwrap();
    // El transformer del plugin puso todo en mayúsculas.
    assert_eq!(h.prompt, h.prompt.to_uppercase(), "{}", h.prompt);
    assert!(h.prompt.contains("TRABAJO"), "{}", h.prompt);

    // Si el plugin muere, el handoff sale igual, sin transformar.
    install_plugin(
        &e,
        "crash",
        "context-transformer",
        "[plugins]\ncontext_transformers = [\"todo-check\"]\n",
    );
    let h = e
        .runtime
        .prepare_handoff(created.agent_id, "prueba")
        .await
        .unwrap();
    assert!(h.prompt.contains("Trabajo"), "{}", h.prompt);
    e.writer.shutdown();
}

/// P12 (revisión): el validator ve también los archivos nuevos que todavía no tienen
/// seguimiento (el gate corre antes del commit) y lo que ve va sin secretos.
#[tokio::test(flavor = "multi_thread")]
async fn a_validator_plugin_sees_untracked_new_files() {
    let e = env(&writes("nuevo.txt", "// TODO sin issue"), None).await;
    install_plugin(&e, "echo", "validator", PLUGIN_GATE);
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Archivo nuevo",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    wait_state(&e, &agent, "COMPLETED").await;
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    let checks = plugin_checks(&e, &agent);
    let summary = checks[0].2.clone().unwrap_or_default();
    assert!(summary.contains("nuevo.txt"), "{summary}");
    assert!(summary.contains("TODO sin issue"), "{summary}");
    e.writer.shutdown();
}

// --- P14: desempeño aprendido (ADR-0016) -------------------------------------------------

/// Cada run que termina con un profile aporta una muestra, y con las mínimas el ajuste sube el
/// puntaje con que compite el modelo (con menos, no influye).
#[tokio::test(flavor = "multi_thread")]
async fn learned_performance_counts_each_finished_run_and_raises_the_score_only_with_enough_samples()
 {
    let e = env_with(&[("alpha", SAY), ("beta", SAY)], None).await;
    // El puntaje de `beta/fast` en la decisión de un agente.
    let score_of = |agent: &str| -> f64 {
        one(
            &e,
            &format!(
                "SELECT c.score FROM routing_candidates c JOIN routing_decisions d ON d.id = c.decision_id
                 WHERE d.agent_id = '{agent}' AND c.model_id = 'beta/fast'"
            ),
        )
    };
    let spawn = |title: &'static str| {
        let e = &e;
        async move {
            let created = e
                .runtime
                .create_agent(req(e, title, Execution::Profile("@code".into())))
                .await
                .unwrap();
            e.runtime.wait_executors().await;
            e.writer.handle().flush().await.unwrap();
            created.agent_id.to_string()
        }
    };
    let set_beta = |n: i64| {
        let e = &e;
        async move {
            e.writer
                .handle()
                .write(Box::new(move |t| {
                    t.execute(
                        "UPDATE profile_models SET sample_count = ?1, outcome_score_sum = ?1
                         WHERE profile_id = '@code' AND model_id = 'beta/fast'",
                        [n],
                    )?;
                    Ok(())
                }))
                .await
                .unwrap();
        }
    };

    let first = spawn("uno").await;
    let baseline = score_of(&first);
    // El run terminado dejó una muestra para el modelo elegido (@code), todavía sin ajuste.
    let chosen: String = one(
        &e,
        &format!("SELECT model_id FROM agent_runs WHERE agent_id = '{first}' ORDER BY seq LIMIT 1"),
    );
    let (n, sum, adj): (i64, f64, Option<f64>) = {
        let conn = symphony_store::open_reader(&e.db).unwrap();
        conn.query_row(
            "SELECT sample_count, outcome_score_sum, learned_adjustment FROM profile_models
             WHERE profile_id = '@code' AND model_id = ?1",
            [&chosen],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    };
    assert_eq!(
        (n, sum, adj),
        (1, 1.0, None),
        "{chosen}: una muestra, sin ajuste todavía"
    );

    // Con 9 muestras limpias (menos del mínimo) no influye…
    set_beta(9).await;
    let nine = spawn("dos").await;
    assert!(
        (score_of(&nine) - baseline).abs() < 1e-9,
        "{} vs {baseline}",
        score_of(&nine)
    );
    // …y con 10 sube su puntaje.
    set_beta(10).await;
    let ten = spawn("tres").await;
    assert!(
        score_of(&ten) > baseline + 1e-6,
        "{} no supera a {baseline}",
        score_of(&ten)
    );
    e.writer.shutdown();
}

// --- P14.S2: sugerencias de modelo por reglas ----------------------------------------------

/// Una tarea que empieza en documentación y termina tocando código en varias carpetas.
fn docs_then_refactor() -> String {
    let edit = |path: &str| {
        format!("[[step]]\nkind = \"edit\"\npath = \"{path}\"\ncontent = \"x\\n\"\n\n")
    };
    // La pausa deja que el checkpoint de la documentación se tome antes del código (sin ella,
    // los cinco cambios caen en un solo checkpoint y no hay «cómo empezó»).
    let pause = "[[step]]
kind = \"sleep\"
ms = 1500

";
    format!(
        "{}{pause}{}{}{}{}",
        edit("docs/guia.md"),
        edit("src/a/uno.rs"),
        edit("src/b/dos.rs"),
        edit("src/c/tres.rs"),
        edit("src/c/cuatro.rs"),
    )
}

/// Espera (hasta 20 s) a que el agente tenga una sugerencia pendiente.
async fn pending_suggestion(e: &Env, agent: &str) -> Option<(String, String, String)> {
    pending_suggestion_for(e, agent, 20).await
}

async fn pending_suggestion_for(
    e: &Env,
    agent: &str,
    secs: u64,
) -> Option<(String, String, String)> {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        e.writer.handle().flush().await.unwrap();
        let row: Option<(String, String, String)> = symphony_store::open_reader(&e.db)
            .unwrap()
            .query_row(
                "SELECT id, suggested_model_id, reason FROM model_suggestions
                 WHERE agent_id = ?1 AND status = 'PENDING'",
                [agent],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        if row.is_some() {
            return row;
        }
        if std::time::Instant::now() > deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Fija el puntaje base de un modelo para `@code` (los de fábrica dependen de heurísticas).
async fn set_code_score(e: &Env, model: &str, score: f64) {
    let model = model.to_string();
    e.writer
        .handle()
        .write(Box::new(move |t| {
            t.execute(
                "UPDATE profile_models SET base_score = ?2 WHERE profile_id = '@code' AND model_id = ?1",
                rusqlite::params![model, score],
            )?;
            Ok(())
        }))
        .await
        .unwrap();
}

fn suggestion_env_script() -> String {
    format!(
        "[[step]]\nkind = \"say\"\ntext = \"Empiezo\"\n\n{}",
        docs_then_refactor()
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_docs_task_that_becomes_a_refactor_gets_a_suggestion_the_user_can_accept() {
    let e = env_with(&[("alpha", SAY), ("beta", &suggestion_env_script())], None).await;
    // `beta/smart` es el modelo más flojo para `@code`: hay uno claramente mejor.
    set_code_score(&e, "beta/smart", 0.2).await;
    set_code_score(&e, "alpha/fast", 0.9).await;
    // Como en el daemon: el evaluador escucha los checkpoints que se van tomando.
    let evaluator = symphony_daemon::suggestions::spawn(
        e.runtime.clone(),
        tokio_util::sync::CancellationToken::new(),
    );
    let mut r = req(
        &e,
        "Documentar y refactorizar",
        Execution::Exact("beta/smart".into()),
    );
    r.chat = true;
    let created = e.runtime.create_agent(r).await.unwrap();
    let agent = created.agent_id.to_string();
    e.runtime.wait_executors().await;

    let (id, suggested, reason) = pending_suggestion(&e, &agent)
        .await
        .expect("debería haber una sugerencia pendiente");
    assert_ne!(suggested, "beta/smart");
    assert!(
        reason.contains("documentación") && reason.contains("varios archivos"),
        "{reason}"
    );
    // Nada cambió por su cuenta: el run sigue siendo el de `beta/smart`.
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT model_id FROM agent_runs WHERE agent_id = '{agent}' ORDER BY seq DESC LIMIT 1"
            )
        ),
        "beta/smart"
    );
    // La sugerencia sale en `inspect`.
    let inspected = e.runtime.inspect(created.agent_id).await.unwrap();
    assert_eq!(inspected["suggestion"]["id"], id);

    // Aceptarla cambia al modelo sugerido (un run nuevo) y la deja `ACCEPTED`.
    let done = e.runtime.suggestion_resolve(&id, "switch").await.unwrap();
    assert_eq!(done["status"], "ACCEPTED");
    e.runtime.wait_executors().await;
    e.writer.handle().flush().await.unwrap();
    assert_eq!(
        one::<String>(
            &e,
            &format!(
                "SELECT model_id FROM agent_runs WHERE agent_id = '{agent}' ORDER BY seq DESC LIMIT 1"
            )
        ),
        suggested
    );
    assert_eq!(
        one::<String>(
            &e,
            &format!("SELECT status FROM model_suggestions WHERE id = '{id}'")
        ),
        "ACCEPTED"
    );
    // Ya resuelta: no se resuelve dos veces.
    assert!(e.runtime.suggestion_resolve(&id, "keep").await.is_err());
    evaluator.abort();
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn keeping_clears_the_suggestion_and_muting_stops_the_next_ones() {
    let e = env_with(&[("alpha", SAY), ("beta", &suggestion_env_script())], None).await;
    set_code_score(&e, "beta/smart", 0.2).await;
    set_code_score(&e, "alpha/fast", 0.9).await;
    // Como en el daemon: el evaluador escucha los checkpoints que se van tomando.
    let evaluator = symphony_daemon::suggestions::spawn(
        e.runtime.clone(),
        tokio_util::sync::CancellationToken::new(),
    );
    let mut r = req(
        &e,
        "Documentar y refactorizar",
        Execution::Exact("beta/smart".into()),
    );
    r.chat = true;
    let created = e.runtime.create_agent(r).await.unwrap();
    let agent = created.agent_id.to_string();
    e.runtime.wait_executors().await;
    let (id, _, _) = pending_suggestion(&e, &agent).await.unwrap();

    // «No sugerir más»: queda `MUTED` y, aunque el agente siga trabajando, no vuelven.
    let done = e.runtime.suggestion_resolve(&id, "mute").await.unwrap();
    assert_eq!(done["status"], "MUTED");
    assert!(
        !e.runtime
            .evaluate_suggestion(created.agent_id)
            .await
            .unwrap()
    );
    assert!(pending_suggestion_for(&e, &agent, 2).await.is_none());
    assert_eq!(
        one::<i64>(
            &e,
            &format!("SELECT COUNT(*) FROM model_suggestions WHERE agent_id = '{agent}'")
        ),
        1
    );
    assert!(e.runtime.suggestion_resolve("nada", "keep").await.is_err());
    evaluator.abort();
    e.writer.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn no_suggestion_when_the_task_does_not_change_type_or_the_model_is_already_the_best() {
    // Solo documentación de principio a fin: nada que sugerir.
    let docs = "[[step]]\nkind = \"edit\"\npath = \"docs/a.md\"\ncontent = \"x\\n\"\n\n[[step]]\nkind = \"edit\"\npath = \"docs/b.md\"\ncontent = \"y\\n\"\n";
    let e = env_with(&[("alpha", SAY), ("beta", docs)], None).await;
    let mut r = req(&e, "Solo docs", Execution::Exact("beta/smart".into()));
    r.chat = true;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    assert!(
        !e.runtime
            .evaluate_suggestion(created.agent_id)
            .await
            .unwrap()
    );
    assert_eq!(count(&e, "model_suggestions"), 0);
    e.writer.shutdown();

    // Docs → refactor, pero con el mejor modelo ya elegido: tampoco.
    let e = env_with(&[("alpha", &suggestion_env_script()), ("beta", SAY)], None).await;
    set_code_score(&e, "alpha/fast", 0.9).await;
    for m in ["alpha/smart", "beta/fast", "beta/smart"] {
        set_code_score(&e, m, 0.3).await;
    }
    let mut r = req(&e, "Ya con el mejor", Execution::Exact("alpha/fast".into()));
    r.chat = true;
    let created = e.runtime.create_agent(r).await.unwrap();
    e.runtime.wait_executors().await;
    assert!(
        !e.runtime
            .evaluate_suggestion(created.agent_id)
            .await
            .unwrap()
    );
    assert_eq!(count(&e, "model_suggestions"), 0);
    e.writer.shutdown();
}

fn pdf_script(content: &str) -> String {
    format!(
        "[[step]]\nkind = \"say\"\ntext = \"Escribo el informe\"\n\n[[step]]\nkind = \"edit\"\npath = \"informe.pdf\"\ncontent = \"{content}\"\n"
    )
}

fn files_checks(e: &Env, agent: &str) -> Vec<(String, String)> {
    let conn =
        rusqlite::Connection::open_with_flags(&e.db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT c.status, COALESCE(c.summary, '') FROM validation_checks c
             JOIN validation_runs r ON r.id = c.validation_run_id
             WHERE r.agent_id = ?1 AND c.name = 'files' ORDER BY r.started_at, r.rowid",
        )
        .unwrap();
    stmt.query_map([agent], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn document_project(e: &Env) {
    std::fs::create_dir_all(e.repo.join(".symphony")).unwrap();
    std::fs::write(
        e.repo.join(".symphony/project.toml"),
        PROJECT_HEAD.replace(
            "default_branch = \"main\"\n",
            "default_branch = \"main\"\nkind = \"document\"\n",
        ),
    )
    .unwrap();
}

/// P16.S0 (ADR-0021): en un proyecto de documentos sin `[validation]`, el gate comprueba que
/// lo que cambió el agente se abre: un PDF completo pasa.
#[tokio::test(flavor = "multi_thread")]
async fn a_document_project_gates_on_files_that_open() {
    let e = env(&pdf_script("%PDF-1.4 1 0 obj %%EOF"), None).await;
    document_project(&e);
    let created = e
        .runtime
        .create_agent(req(&e, "Informe", Execution::Exact("fake/fast".into())))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    wait_state(&e, &agent, "COMPLETED").await;
    let checks = files_checks(&e, &agent);
    assert_eq!(checks.len(), 1, "{checks:?}");
    assert_eq!(checks[0].0, "PASS", "{checks:?}");
    e.writer.shutdown();
}

/// P16.S0: un PDF truncado no pasa el gate; el agente recibe el motivo y, sin arreglarlo,
/// la tarea se bloquea como con cualquier validación.
#[tokio::test(flavor = "multi_thread")]
async fn a_truncated_pdf_fails_the_files_check() {
    let e = env(&pdf_script("%PDF-1.4 sin final"), None).await;
    document_project(&e);
    let created = e
        .runtime
        .create_agent(req(
            &e,
            "Informe roto",
            Execution::Exact("fake/fast".into()),
        ))
        .await
        .unwrap();
    let agent = created.agent_id.to_string();
    wait_state(&e, &agent, "BLOCKED").await;
    let checks = files_checks(&e, &agent);
    assert!(!checks.is_empty());
    assert!(
        checks
            .iter()
            .all(|(s, why)| s == "FAIL" && why.contains("informe.pdf") && why.contains("truncado")),
        "{checks:?}"
    );
    e.writer.shutdown();
}
