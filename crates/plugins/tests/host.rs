//! P12.S2: el host contra un plugin falso (`fake-plugin`): lo que funciona y, sobre todo, lo que
//! sale mal sin llegar nunca al llamador como pánico ni como proceso huérfano.
// Código de test: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use symphony_plugins::protocol::ProtocolError;
use symphony_plugins::{Kind, Manifest, Plugin, PluginError, ValidateInput, discover};

/// `fake-plugin` al día (paquete testkit). Vive en el mismo `target/<perfil>/` que este test.
fn fake_plugin() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let status = Command::new(env!("CARGO"))
                .args([
                    "build",
                    "-q",
                    "-p",
                    "symphony-testkit",
                    "--bin",
                    "fake-plugin",
                ])
                .status()
                .expect("cargo build fake-plugin");
            assert!(status.success());
            let exe = std::env::current_exe().unwrap();
            let profile_dir = exe.parent().unwrap().parent().unwrap();
            symphony_testkit::pinned_bin(
                &profile_dir.join(format!("fake-plugin{}", std::env::consts::EXE_SUFFIX)),
            )
        })
        .clone()
}

/// Un plugin cuyo ejecutable es el `fake-plugin` en el modo dado.
fn plugin(dir: &Path, mode: &str, kind: &str, timeout: u64) -> Plugin {
    let program = fake_plugin().display().to_string().replace('\\', "\\\\");
    let text = format!(
        "name = \"fake-plugin\"\nversion = \"0.1.0\"\nplugin_api = 1\nkind = \"{kind}\"\n\
         exec = [\"{program}\", \"{mode}\", \"fake-plugin\", \"{kind}\"]\ntimeout_secs = {timeout}\n"
    );
    // Un `exec[0]` absoluto es válido para el test: el manifiesto real lo rechaza (otro test).
    let manifest = match Manifest::parse(&text) {
        Ok(m) => m,
        Err(_) => {
            let mut m = Manifest::parse(&text.replace(&program, "fake-plugin")).unwrap();
            m.exec[0] = fake_plugin().display().to_string();
            m
        }
    };
    Plugin {
        manifest,
        dir: dir.to_path_buf(),
    }
}

fn input() -> ValidateInput {
    ValidateInput {
        task_code: "T-1".into(),
        task_title: "algo".into(),
        diff: "+// TODO arreglar\n".into(),
        files: vec!["src/a.rs".into()],
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_well_behaved_validator_answers_and_describes_itself() {
    let dir = tempfile::tempdir().unwrap();
    let p = plugin(dir.path(), "validate-fail", "validator", 10);
    let d = p.describe().await.unwrap();
    assert_eq!((d.name.as_str(), d.plugin_api), ("fake-plugin", 1));
    let out = p.validate(&input()).await.unwrap();
    assert!(!out.passed);
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].file, "src/a.rs");

    let ok = plugin(dir.path(), "validate-ok", "validator", 10);
    assert!(ok.validate(&input()).await.unwrap().passed);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crashing_plugin_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let p = plugin(dir.path(), "crash", "validator", 10);
    let err = p.validate(&input()).await.unwrap_err();
    match err {
        PluginError::Crashed { code, stderr } => {
            assert_eq!(code, "134");
            assert!(stderr.contains("me muero"), "{stderr}");
        }
        other => panic!("{other:?}"),
    }
    // El host sigue sirviendo después del fallo.
    let ok = plugin(dir.path(), "validate-ok", "validator", 10);
    assert!(ok.validate(&input()).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hanging_plugin_is_killed_at_its_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let p = plugin(dir.path(), "hang", "validator", 1);
    let t = Instant::now();
    let err = p.validate(&input()).await.unwrap_err();
    assert!(matches!(err, PluginError::Timeout(1)), "{err:?}");
    assert!(
        t.elapsed() < Duration::from_secs(8),
        "tardó {:?}",
        t.elapsed()
    );
    // Sin huérfanos: ningún `fake-plugin hang` sigue vivo.
    let mut left = usize::MAX;
    for _ in 0..30 {
        left = alive("hang");
        if left == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(left, 0, "quedó un plugin colgado");
}

/// Procesos vivos de `fake-plugin` con `marker` entre sus argumentos.
fn alive(marker: &str) -> usize {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    sys.processes()
        .values()
        .filter(|p| p.status() != sysinfo::ProcessStatus::Zombie)
        .filter(|p| {
            let cmd: Vec<String> = p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            cmd.iter().any(|a| a.contains("fake-plugin")) && cmd.iter().any(|a| a == marker)
        })
        .count()
}

#[tokio::test(flavor = "multi_thread")]
async fn garbage_lies_and_oversized_answers_are_protocol_errors() {
    let dir = tempfile::tempdir().unwrap();
    for (mode, check) in [
        ("garbage", "json"),
        ("wrong-api", "api"),
        ("huge", "grande"),
        ("silent", "vacía"),
        ("error", "remoto"),
    ] {
        let p = plugin(dir.path(), mode, "validator", 10);
        let err = p.validate(&input()).await.unwrap_err();
        let ok = match (&err, check) {
            (PluginError::Protocol(ProtocolError::Json(_)), "json") => true,
            (PluginError::Protocol(ProtocolError::Api(Some(2))), "api") => true,
            (PluginError::Protocol(ProtocolError::TooLarge), "grande") => true,
            (PluginError::Protocol(ProtocolError::Empty), "vacía") => true,
            (PluginError::Protocol(ProtocolError::Remote(m)), "remoto") => m.contains("no pude"),
            _ => false,
        };
        assert!(ok, "{mode}: {err:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn what_a_plugin_returns_is_redacted_before_it_leaves_the_host() {
    let dir = tempfile::tempdir().unwrap();
    let p = plugin(dir.path(), "secret", "validator", 10);
    let out = p.validate(&input()).await.unwrap();
    assert!(!out.summary.contains("sk-ant-api03"), "{}", out.summary);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_that_claims_to_be_another_one_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let p = plugin(dir.path(), "wrong-name", "validator", 10);
    assert!(matches!(
        p.describe().await.unwrap_err(),
        PluginError::Mismatch(_)
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn kinds_are_enforced_and_unsupported_kinds_are_not_run() {
    let dir = tempfile::tempdir().unwrap();
    // Un validator no transforma.
    let v = plugin(dir.path(), "validate-ok", "validator", 10);
    assert!(matches!(
        v.transform("hola").await.unwrap_err(),
        PluginError::WrongKind { .. }
    ));
    // `tool` está en el manifiesto pero el host no lo ejecuta (ADR-0015).
    let t = plugin(dir.path(), "validate-ok", "tool", 10);
    assert_eq!(t.manifest.kind, Kind::Tool);
    assert!(matches!(
        t.describe().await.unwrap_err(),
        PluginError::Unsupported("tool")
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_context_transformer_rewrites_within_its_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let up = plugin(dir.path(), "transform-upper", "context-transformer", 10);
    assert_eq!(up.transform("hola mundo").await.unwrap(), "HOLA MUNDO");
    // Una salida que crece sin medida no se acepta.
    let huge = plugin(dir.path(), "transform-huge", "context-transformer", 10);
    let err = huge.transform(&"x".repeat(2000)).await.unwrap_err();
    assert!(matches!(err, PluginError::Shape(_)), "{err:?}");
}

#[test]
fn discovery_lists_valid_plugins_and_reports_the_broken_ones() {
    let root = tempfile::tempdir().unwrap();
    let write = |folder: &str, text: &str| {
        let d = root.path().join(folder);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("plugin.toml"), text).unwrap();
    };
    let good = |name: &str| {
        format!(
            "name = '{name}'\nversion = '1'\nplugin_api = 1\nkind = 'validator'\nexec = ['./x']\n"
        )
    };
    write("uno", &good("uno"));
    write("dos", &good("otro-nombre")); // carpeta ≠ name
    write("tres", "name = 'tres'\nplugin_api = 9\n");
    std::fs::create_dir_all(root.path().join("sin-manifiesto")).unwrap();

    let found = discover(root.path());
    assert_eq!(
        found
            .plugins
            .iter()
            .map(|p| p.manifest.name.as_str())
            .collect::<Vec<_>>(),
        ["uno"]
    );
    assert_eq!(found.problems.len(), 2, "{:?}", found.problems);
    assert!(
        discover(Path::new("/no/existe/de/verdad"))
            .plugins
            .is_empty()
    );
}
