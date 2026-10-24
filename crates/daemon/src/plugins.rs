//! Plugins en el daemon (P12, ADR-0015): cuáles activa cada proyecto y cómo se usan.
//!
//! El daemon es quien decide cuándo llamar a un plugin y con qué datos; el plugin corre como un
//! proceso aislado (`symphony-plugins`), no abre la base ni pide nada al daemon. Un plugin que
//! falla degrada su propio resultado, nunca el de Symphony.

use std::path::Path;
use std::time::Instant;

use serde_json::{Value, json};
use symphony_core::{AgentId, CheckName, CheckStatus, SymphonyHome};
use symphony_git::Repo;
use symphony_plugins::{Kind, Plugin, ValidateInput, discover};
use symphony_store::repo;
use symphony_store::validation::{self as store, NewCheck};

use crate::runtime::Runtime;
use crate::validation::{CheckResult, ulid_string};

/// Hallazgos que se copian al resumen de un check.
const FINDINGS_IN_SUMMARY: usize = 8;
/// Caracteres del diff que ve un plugin.
const DIFF_KEEP: usize = 512 * 1024;
/// Líneas de cada archivo nuevo sin seguimiento que entran al diff del plugin.
const UNTRACKED_LINES: usize = 2000;
/// Tiempo total que pueden tomar los context transformers de un handoff.
const TRANSFORM_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

/// Lo que un proyecto activa, resuelto contra lo instalado. `Err` = por qué no sirve.
#[derive(Debug, Clone, Default)]
pub struct Active {
    pub validators: Vec<Result<Plugin, Unavailable>>,
    pub transformers: Vec<Result<Plugin, Unavailable>>,
}

/// Un plugin activado que no se puede usar: cuál es y por qué.
#[derive(Debug, Clone)]
pub struct Unavailable {
    pub name: String,
    pub why: String,
}

impl Runtime {
    /// Los plugins que activa `project.toml` (`[plugins]`), resueltos contra `~/.symphony/plugins`.
    pub(crate) fn active_plugins(&self, project_root: &str) -> Active {
        let cfg = symphony_core::load_project(Path::new(project_root))
            .ok()
            .flatten()
            .and_then(|p| p.plugins)
            .unwrap_or_default();
        if cfg.validators.is_empty() && cfg.context_transformers.is_empty() {
            return Active::default();
        }
        let installed = discover(&SymphonyHome::user_for(&self.home).plugins_dir());
        let resolve = |names: &[String], want: Kind| -> Vec<Result<Plugin, Unavailable>> {
            names
                .iter()
                .map(|n| {
                    let unavailable = |why: String| Unavailable {
                        name: n.clone(),
                        why,
                    };
                    match installed.plugins.iter().find(|p| &p.manifest.name == n) {
                        Some(p) if p.manifest.kind == want => Ok(p.clone()),
                        Some(p) => Err(unavailable(format!(
                            "`{n}` es de tipo `{}`, no `{}`",
                            p.manifest.kind.as_str(),
                            want.as_str()
                        ))),
                        None => Err(unavailable(format!(
                            "`{n}` no está instalado en ~/.symphony/plugins"
                        ))),
                    }
                })
                .collect()
        };
        Active {
            validators: resolve(&cfg.validators, Kind::Validator),
            transformers: resolve(&cfg.context_transformers, Kind::ContextTransformer),
        }
    }

    /// Corre los validators de plugin sobre lo que cambió la tarea y guarda un check por cada uno.
    /// Un plugin que no pudo correr deja un check `SKIPPED` (no se da por bueno).
    pub(crate) async fn run_plugin_checks(
        &self,
        agent: &repo::Agent,
        vrun: &str,
        worktree: &Path,
        base_ref: &str,
        plugins: &[Result<Plugin, Unavailable>],
    ) -> Result<Vec<CheckResult>, String> {
        let task = self
            .reader
            .lock()
            .map_err(|_| "lector de la base no disponible".to_string())
            .and_then(|c| repo::get_task(&c, agent.task_id).map_err(|e| e.to_string()))?;
        let (wt, base) = (worktree.to_path_buf(), base_ref.to_string());
        let (diff, files) = tokio::task::spawn_blocking(move || {
            let r = Repo::at(&wt);
            let mut diff = r.diff(&base).unwrap_or_default();
            let mut files: Vec<String> = r
                .diff_numstat(&base)
                .map(|v| v.into_iter().map(|n| n.path).collect())
                .unwrap_or_default();
            // Los archivos nuevos todavía sin seguimiento (el gate corre antes del commit) no
            // salen en `git diff`: entran como adiciones, con tope por archivo.
            for entry in r.status().unwrap_or_default() {
                if let symphony_git::FileStatus::Untracked { path } = entry
                    && let Ok(bytes) = std::fs::read(wt.join(&path))
                    && !bytes.contains(&0)
                {
                    let text = String::from_utf8_lossy(&bytes);
                    diff.push_str(&format!("\ndiff --git a/{path} b/{path}\n+++ b/{path}\n"));
                    for line in text.lines().take(UNTRACKED_LINES) {
                        diff.push('+');
                        diff.push_str(line);
                        diff.push('\n');
                    }
                    files.push(path);
                }
            }
            (diff, files)
        })
        .await
        .map_err(|e| e.to_string())?;
        // Lo que sale hacia un plugin tampoco lleva secretos, y no pasa de un tope.
        let diff: String = symphony_core::redact(&diff)
            .chars()
            .take(DIFF_KEEP)
            .collect();
        let input = ValidateInput {
            task_code: task.code.clone(),
            task_title: task.title.clone(),
            diff,
            files,
        };
        let mut out = Vec::new();
        for resolved in plugins {
            let started = Instant::now();
            let (name, status, failed, summary) = match resolved {
                Err(u) => (
                    u.name.clone(),
                    CheckStatus::Skipped,
                    None,
                    format!("no se ejecutó: {}", u.why),
                ),
                Ok(p) => match p.validate(&input).await {
                    Ok(v) => {
                        let mut text = v.summary.clone();
                        for f in v.findings.iter().take(FINDINGS_IN_SUMMARY) {
                            text.push_str(&format!("\n- [{}] {}: {}", f.severity, f.file, f.text));
                        }
                        let n = u32::try_from(v.findings.len()).unwrap_or(u32::MAX);
                        if v.passed {
                            (p.manifest.name.clone(), CheckStatus::Pass, None, text)
                        } else {
                            (p.manifest.name.clone(), CheckStatus::Fail, Some(n), text)
                        }
                    }
                    Err(e) => (
                        p.manifest.name.clone(),
                        CheckStatus::Skipped,
                        None,
                        format!("no se ejecutó: {e}"),
                    ),
                },
            };
            let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let new = NewCheck {
                id: ulid_string(),
                validation_run_id: vrun.to_string(),
                name: CheckName::Plugin,
                status,
                passed: None,
                failed,
                summary: (!summary.is_empty()).then(|| summary.clone()),
                output_object_id: None,
            };
            self.writer
                .write(Box::new(move |t| {
                    store::insert_check(t, &new)?;
                    Ok(())
                }))
                .await
                .map_err(|e| e.to_string())?;
            out.push(CheckResult {
                command: format!("plugin:{name}"),
                name: CheckName::Plugin,
                status,
                passed: None,
                failed,
                summary,
                exit_code: None,
                duration_ms,
            });
        }
        Ok(out)
    }

    /// Pasa el handoff por los context transformers del proyecto, en orden. Si uno falla se
    /// conserva el texto anterior y se avisa en el log.
    pub(crate) async fn transform_handoff(&self, agent: AgentId, mut text: String) -> String {
        let root = self.reader.lock().ok().and_then(|c| {
            let a = repo::get_agent(&c, agent).ok()?;
            repo::get_project(&c, a.project_id)
                .ok()
                .map(|p| p.root_path)
        });
        let Some(root) = root else { return text };
        let deadline = Instant::now() + TRANSFORM_BUDGET;
        for resolved in self.active_plugins(&root).transformers {
            if Instant::now() >= deadline {
                tracing::warn!(
                    "context transformers: se acabó el presupuesto de tiempo del handoff"
                );
                break;
            }
            match resolved {
                Err(u) => {
                    tracing::warn!(plugin = %u.name, why = %u.why, "context transformer no disponible")
                }
                Ok(p) => match tokio::time::timeout(
                    deadline.saturating_duration_since(Instant::now()),
                    p.transform(&text),
                )
                .await
                .unwrap_or(Err(symphony_plugins::PluginError::Timeout(30)))
                {
                    Ok(next) => text = next,
                    Err(e) => tracing::warn!(
                        plugin = %p.manifest.name,
                        error = %e,
                        "context transformer falló; el handoff sigue sin su cambio"
                    ),
                },
            }
        }
        text
    }

    /// Lo instalado y lo que activa el proyecto, para `plugin.list`.
    pub fn plugin_list(&self, project_root: Option<&str>) -> Value {
        let installed = discover(&SymphonyHome::user_for(&self.home).plugins_dir());
        let enabled: Vec<String> = project_root
            .and_then(|r| symphony_core::load_project(Path::new(r)).ok().flatten())
            .and_then(|p| p.plugins)
            .map(|p| {
                p.validators
                    .into_iter()
                    .chain(p.context_transformers)
                    .collect()
            })
            .unwrap_or_default();
        json!({
            "plugins": installed.plugins.iter().map(|p| json!({
                "name": p.manifest.name,
                "version": p.manifest.version,
                "kind": p.manifest.kind.as_str(),
                "description": p.manifest.description,
                "supported": p.manifest.kind.supported(),
                "enabled": enabled.contains(&p.manifest.name),
                "timeout_secs": p.manifest.timeout_secs,
            })).collect::<Vec<_>>(),
            "problems": installed.problems.iter().map(|(dir, why)| json!({
                "dir": dir.display().to_string(), "problem": why,
            })).collect::<Vec<_>>(),
            "enabled_but_missing": enabled
                .iter()
                .filter(|n| !installed.plugins.iter().any(|p| &p.manifest.name == *n))
                .collect::<Vec<_>>(),
            "installed_dir": SymphonyHome::user_for(&self.home).plugins_dir().display().to_string(),
        })
    }

    /// `plugin.check`: lanza el plugin y le pide `describe` (comprueba que arranca y que es quien
    /// dice ser). Devuelve el resultado, nunca falla el daemon.
    pub async fn plugin_check(&self, name: &str) -> Result<Value, String> {
        let installed = discover(&SymphonyHome::user_for(&self.home).plugins_dir());
        let Some(p) = installed.plugins.iter().find(|p| p.manifest.name == name) else {
            return Err(format!("no hay un plugin `{name}` instalado"));
        };
        let t = Instant::now();
        let result = p.describe().await;
        let ms = u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(match result {
            Ok(d) => json!({
                "name": name, "ok": true, "elapsed_ms": ms,
                "version": d.version, "kind": d.kind, "plugin_api": d.plugin_api,
            }),
            Err(e) => {
                json!({ "name": name, "ok": false, "elapsed_ms": ms, "error": e.to_string() })
            }
        })
    }
}
