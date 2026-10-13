//! El host: descubre plugins y los ejecuta como procesos aislados (ADR-0015).
//!
//! Un proceso por llamada, bajo el supervisor (`symphony-process`): grupo propio, tiempo máximo,
//! memoria y procesos limitados y salida acotada. Un plugin que muere, se cuelga o miente es un
//! error de esa llamada: nunca toca al daemon.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use symphony_process::{ExitStatus, Limits, OutputLine, ProcessSpec, spawn};

use crate::manifest::{Kind, Manifest, ManifestError};
use crate::protocol::{
    Description, MAX_RESPONSE_BYTES, ProtocolError, Request, ValidateInput, ValidateOutput,
    parse_response,
};

/// Memoria máxima del árbol de un plugin.
const MAX_MEMORY_BYTES: u64 = 512 * 1024 * 1024;
/// Procesos máximos del árbol de un plugin.
const MAX_PROCESSES: u32 = 16;
/// Cuánto de stderr se conserva para diagnosticar (el final).
const STDERR_KEEP: usize = 2048;
/// Espera para que un proceso que ya cerró su salida termine del todo.
const EXIT_GRACE: Duration = Duration::from_secs(5);
/// Tope del resumen y de cada hallazgo (caracteres).
const TEXT_KEEP: usize = 4000;
const FINDINGS_KEEP: usize = 50;

/// Un plugin instalado y válido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plugin {
    pub manifest: Manifest,
    pub dir: PathBuf,
}

/// Lo que dejó recorrer la carpeta de plugins.
#[derive(Debug, Default)]
pub struct Discovery {
    pub plugins: Vec<Plugin>,
    /// `(carpeta, por qué no sirve)`: un plugin roto no impide listar los demás.
    pub problems: Vec<(PathBuf, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("los plugins de tipo `{0}` todavía no se soportan en esta versión")]
    Unsupported(&'static str),
    #[error("el plugin es de tipo `{have}`, no `{want}`")]
    WrongKind {
        have: &'static str,
        want: &'static str,
    },
    #[error("no se pudo lanzar el plugin: {0}")]
    Spawn(String),
    #[error("el plugin superó su tiempo máximo ({0} s) y se detuvo")]
    Timeout(u64),
    #[error("el plugin terminó con error ({code}){stderr}")]
    Crashed { code: String, stderr: String },
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error("el plugin dice ser otro de lo que declara su manifiesto: {0}")]
    Mismatch(String),
    #[error("la respuesta del plugin no tiene la forma esperada: {0}")]
    Shape(String),
}

/// Recorre `<root>/<nombre>/plugin.toml`. El nombre de la carpeta tiene que ser el del manifiesto.
pub fn discover(root: &Path) -> Discovery {
    let mut out = Discovery::default();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let file = dir.join("plugin.toml");
        if !file.is_file() {
            continue;
        }
        let loaded = std::fs::read_to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|t| Manifest::parse(&t).map_err(|e: ManifestError| e.to_string()));
        match loaded {
            Ok(m) => {
                let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned());
                if folder.as_deref() == Some(m.name.as_str()) {
                    out.plugins.push(Plugin { manifest: m, dir });
                } else {
                    out.problems.push((
                        dir,
                        format!(
                            "la carpeta tiene que llamarse `{}` (el `name` del manifiesto)",
                            m.name
                        ),
                    ));
                }
            }
            Err(why) => out.problems.push((dir, why)),
        }
    }
    out
}

/// Conserva el final de `s` (como mucho `max` bytes, en un borde de carácter).
fn tail(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut from = s.len() - max;
    while !s.is_char_boundary(from) {
        from += 1;
    }
    &s[from..]
}

/// Recorta a `max` caracteres.
fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Lo que sale de un plugin hacia la base o un proveedor: sin secretos y acotado.
fn clean(s: &str, max: usize) -> String {
    clip(&symphony_core::redact(s), max)
}

impl Plugin {
    /// Una llamada: lanza el proceso, le manda la petición y devuelve su `result`.
    pub async fn call(&self, request: Request) -> Result<Value, PluginError> {
        let kind = self.manifest.kind;
        if !kind.supported() {
            return Err(PluginError::Unsupported(kind.as_str()));
        }
        let spec = ProcessSpec::new(self.manifest.program(&self.dir))
            .args(self.manifest.exec.iter().skip(1))
            .cwd(&self.dir)
            .env("SYMPHONY_PLUGIN_API", "1")
            .with_stdin()
            .limits(Limits {
                max_memory_bytes: Some(MAX_MEMORY_BYTES),
                cpu_cores: None,
                max_processes: Some(MAX_PROCESSES),
            });
        let mut proc = spawn(spec)
            .await
            .map_err(|e| PluginError::Spawn(e.to_string()))?;
        // Si el plugin ya salió, escribirle falla: el estado de salida lo explica mejor.
        let _ = proc.write_stdin(request.to_line().as_bytes()).await;
        let _ = proc.close_stdin().await;

        let secs = self.manifest.timeout_secs;
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut overflow = false;
        let read = async {
            while let Some(line) = proc.next_output().await {
                match line {
                    OutputLine::Stdout(l) => {
                        // Se sigue drenando aunque se pase del tope, para no trabar al plugin.
                        if stdout.len() + l.len() > MAX_RESPONSE_BYTES {
                            overflow = true;
                        } else {
                            stdout.push_str(&l);
                            stdout.push('\n');
                        }
                    }
                    OutputLine::Stderr(l) => {
                        stderr.push_str(&l);
                        stderr.push('\n');
                        if stderr.len() > STDERR_KEEP * 4 {
                            stderr = tail(&stderr, STDERR_KEEP).to_string();
                        }
                    }
                }
            }
        };
        if tokio::time::timeout(Duration::from_secs(secs), read)
            .await
            .is_err()
        {
            let _ = proc.terminate_tree();
            return Err(PluginError::Timeout(secs));
        }
        let status = match proc.wait_timeout(EXIT_GRACE).await {
            Some(s) => s,
            None => {
                let _ = proc.terminate_tree();
                return Err(PluginError::Timeout(secs));
            }
        };
        if !status.success() {
            let code = match status {
                ExitStatus::Exited(c) => c.to_string(),
                ExitStatus::Killed(_) => "terminado".into(),
                ExitStatus::Unknown => "desconocido".into(),
            };
            let err = tail(stderr.trim(), STDERR_KEEP);
            return Err(PluginError::Crashed {
                code,
                stderr: if err.is_empty() {
                    String::new()
                } else {
                    format!(": {}", clean(err, STDERR_KEEP))
                },
            });
        }
        if overflow {
            return Err(ProtocolError::TooLarge.into());
        }
        Ok(parse_response(&stdout)?)
    }

    /// `describe`: el plugin dice quién es y se comprueba contra su manifiesto.
    pub async fn describe(&self) -> Result<Description, PluginError> {
        let v = self.call(Request::describe(self.manifest.kind)).await?;
        let d: Description =
            serde_json::from_value(v).map_err(|e| PluginError::Shape(e.to_string()))?;
        if d.name != self.manifest.name || d.kind != self.manifest.kind.as_str() {
            return Err(PluginError::Mismatch(format!(
                "manifiesto: {} ({}); plugin: {} ({})",
                self.manifest.name,
                self.manifest.kind.as_str(),
                d.name,
                d.kind
            )));
        }
        Ok(d)
    }

    fn require(&self, want: Kind) -> Result<(), PluginError> {
        if self.manifest.kind == want {
            Ok(())
        } else {
            Err(PluginError::WrongKind {
                have: self.manifest.kind.as_str(),
                want: want.as_str(),
            })
        }
    }

    /// Un validator revisa lo que cambió una tarea.
    pub async fn validate(&self, input: &ValidateInput) -> Result<ValidateOutput, PluginError> {
        self.require(Kind::Validator)?;
        let params = serde_json::to_value(input).map_err(|e| PluginError::Shape(e.to_string()))?;
        let v = self
            .call(Request::new(Kind::Validator, "validate", params))
            .await?;
        let mut out: ValidateOutput =
            serde_json::from_value(v).map_err(|e| PluginError::Shape(e.to_string()))?;
        out.summary = clean(&out.summary, TEXT_KEEP);
        out.findings.truncate(FINDINGS_KEEP);
        for f in &mut out.findings {
            f.severity = clean(&f.severity, 16);
            f.file = clean(&f.file, 300);
            f.text = clean(&f.text, TEXT_KEEP);
        }
        Ok(out)
    }

    /// Un context transformer reescribe un texto (el handoff). La salida pasa por `redact` y no
    /// puede crecer más de 4× (ni vaciarse).
    pub async fn transform(&self, text: &str) -> Result<String, PluginError> {
        self.require(Kind::ContextTransformer)?;
        let v = self
            .call(Request::new(
                Kind::ContextTransformer,
                "transform",
                json!({ "stage": "handoff", "text": text }),
            ))
            .await?;
        let out = v["text"]
            .as_str()
            .ok_or_else(|| PluginError::Shape("falta `text`".into()))?;
        if out.trim().is_empty() {
            return Err(PluginError::Shape("`text` vacío".into()));
        }
        let max = text.len().saturating_mul(4).max(4096);
        if out.len() > max {
            return Err(PluginError::Shape(format!(
                "`text` mide {} bytes (el máximo es {max})",
                out.len()
            )));
        }
        Ok(symphony_core::redact(out).into_owned())
    }
}
