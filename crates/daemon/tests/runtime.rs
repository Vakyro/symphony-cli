//! P06.S1: creación de agentes (FLOW §6) — flujo feliz y cada rama de error.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};

use symphony_adapter_common::ProviderAdapter;
use symphony_core::{AgentState, ContextMode, FailoverPolicy};
use symphony_daemon::bus::EventBus;
use symphony_daemon::providers;
use symphony_daemon::runtime::{CreateAgent, CreateError, Execution, Runtime};
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
            PathBuf::from(env!("CARGO_BIN_EXE_symphonyd"))
                .with_file_name(format!("fake-agent{}", std::env::consts::EXE_SUFFIX))
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
    runtime: Runtime,
}

/// Home + repo con un commit + proveedor `fake` detectado con el guion dado.
/// `binary` reemplaza al fake-agent (para simular un CLI que no arranca).
async fn env(script: &str, binary: Option<PathBuf>) -> Env {
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
    let script_path = dir.path().join("script.toml");
    std::fs::write(&script_path, script).unwrap();

    std::fs::create_dir_all(&home).unwrap();
    let db = home.join("symphony.db");
    let writer = Writer::start(&db).unwrap();
    let real = FakeAdapter::new(fake_agent(), &script_path);
    let detected: Vec<Box<dyn ProviderAdapter>> = vec![Box::new(real.clone())];
    providers::save(&writer.handle(), providers::detect_all(&detected), 1)
        .await
        .unwrap();
    let used = FakeAdapter::new(binary.unwrap_or_else(fake_agent), &script_path);
    let runtime = Runtime::new(
        &home,
        writer.handle(),
        writer.reader().unwrap(),
        EventBus::new(writer.handle(), 64),
        vec![Arc::new(used)],
        None,
    );
    Env {
        _dir: dir,
        home,
        repo,
        db,
        writer,
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
            "SELECT seq || '|' || objective || '|' || head_commit FROM checkpoints WHERE agent_id='{a}'"
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
        .create_agent(req(&e, "Con profile", Execution::Profile("@code".into())))
        .await
        .unwrap_err();
    assert!(
        matches!(err, CreateError::ProfilesNotSupported { .. }),
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
