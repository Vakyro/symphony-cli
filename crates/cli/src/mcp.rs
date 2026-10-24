//! `symphony mcp serve`: el broker MCP del contexto (P09.S8, STACK §15).
//!
//! Un proceso mínimo por CLI, hablando MCP por stdio. No guarda nada: cada herramienta se
//! reenvía al daemon por IPC (`context.search|retrieve|lines`), que es el dueño del índice y
//! registra el acceso en `context_retrievals`. Los nombres usan `_` y no `.`: varios CLIs solo
//! aceptan `[A-Za-z0-9_-]` en el nombre de una herramienta.

use std::path::PathBuf;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, ServiceExt, schemars, tool, tool_handler, tool_router, transport::stdio};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{client, harness};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchArgs {
    /// Palabras a buscar en el proyecto (se ordenan por relevancia).
    query: String,
    /// Limitar la búsqueda a un objeto, p. ej. `ctx://file/src/auth.ts`.
    uri: Option<String>,
    /// Cuántos fragmentos devolver (1–20, por defecto 5).
    limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RetrieveArgs {
    /// Dirección `ctx://…` de lo que el resumen dejó fuera.
    uri: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct LinesArgs {
    /// Dirección `ctx://…`.
    uri: String,
    /// Primera línea (desde 1).
    from: i64,
    /// Última línea, inclusive (se recorta a 400 líneas por llamada).
    to: i64,
}

struct Broker {
    tool_router: ToolRouter<Self>,
    /// Directorio del agente (su worktree): de ahí cuelgan las rutas relativas del harness.
    root: PathBuf,
    home: PathBuf,
    /// Identidad del run que pregunta; sin ella el daemon no registra el acceso.
    agent: Option<String>,
    run: Option<String>,
    project: Option<String>,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn fail(msg: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(msg.to_string(), None)
}

impl Broker {
    async fn ask(&self, method: &str, mut params: Value) -> Result<Value, ErrorData> {
        params["agent_id"] = json!(self.agent);
        params["run_id"] = json!(self.run);
        match &self.project {
            Some(p) => params["project_id"] = json!(p),
            None => {
                // Fuera de un agente de Symphony: el proyecto es el del directorio actual.
                let cwd = std::env::current_dir().map_err(fail)?;
                params["project_root"] = json!(cwd.display().to_string());
            }
        }
        let mut conn = client::connect_or_start(&self.home).await.map_err(fail)?;
        client::call(&mut conn, method, params).await.map_err(fail)
    }
}

fn text_of(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_string()
}

#[tool_router(router = context_router)]
impl Broker {
    #[tool(
        description = "Busca en el contexto indexado del proyecto (archivos, diffs, salidas de herramientas). Devuelve fragmentos con su dirección ctx://, líneas y relevancia."
    )]
    async fn context_search(
        &self,
        Parameters(a): Parameters<SearchArgs>,
    ) -> Result<String, ErrorData> {
        let v = self
            .ask(
                "context.search",
                json!({ "query": a.query, "uri": a.uri, "limit": a.limit }),
            )
            .await?;
        let hits = v["hits"].as_array().cloned().unwrap_or_default();
        if hits.is_empty() {
            return Ok("Sin resultados.".into());
        }
        let lines: Vec<String> = hits
            .iter()
            .map(|h| {
                let range = match (h["start_line"].as_i64(), h["end_line"].as_i64()) {
                    (Some(a), Some(b)) => format!(" (líneas {a}-{b})"),
                    _ => String::new(),
                };
                format!("{}{range}\n  {}", text_of(h, "uri"), text_of(h, "snippet"))
            })
            .collect();
        Ok(lines.join("\n"))
    }

    #[tool(
        description = "Devuelve el original completo de un objeto ctx:// (hasta 64 KB). Úsalo cuando un resumen del contexto dejó fuera lo que necesitas; con `context_lines` pides solo un rango."
    )]
    async fn context_retrieve(
        &self,
        Parameters(a): Parameters<RetrieveArgs>,
    ) -> Result<String, ErrorData> {
        let v = self
            .ask("context.retrieve", json!({ "uri": a.uri }))
            .await?;
        let note = if v["truncated"].as_bool() == Some(true) {
            format!(
                "\n[recortado: {} bytes en total, {} líneas; pide un rango con context_lines]",
                v["bytes"], v["total_lines"]
            )
        } else {
            String::new()
        };
        Ok(format!("{}{note}", text_of(&v, "text")))
    }

    #[tool(
        description = "Devuelve las líneas from..=to (desde 1) del original de un objeto ctx://, hasta 400 por llamada."
    )]
    async fn context_lines(
        &self,
        Parameters(a): Parameters<LinesArgs>,
    ) -> Result<String, ErrorData> {
        let v = self
            .ask(
                "context.lines",
                json!({ "uri": a.uri, "from": a.from, "to": a.to }),
            )
            .await?;
        let text = text_of(&v, "text");
        if text.is_empty() {
            return Ok(format!(
                "Sin líneas en ese rango: el objeto tiene {} líneas.",
                v["total_lines"]
            ));
        }
        Ok(text)
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReadArgs {
    /// File path (absolute, or relative to the working directory).
    path: String,
    /// First line to return (1-based). Default 1.
    offset: Option<usize>,
    /// Maximum number of lines. Default 2000.
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct WriteArgs {
    /// File path (absolute, or relative to the working directory). Parent folders are created.
    path: String,
    /// Complete new content of the file.
    content: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EditArgs {
    /// File path (absolute, or relative to the working directory).
    path: String,
    /// Exact text to replace, copied from the file (including whitespace).
    old: String,
    /// Replacement text.
    new: String,
    /// Replace every occurrence instead of requiring a unique one. Default false.
    replace_all: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GlobArgs {
    /// Glob pattern such as "**/*.rs" or "src/*.ts".
    pattern: String,
    /// Folder to search in. Default: the working directory.
    path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GrepArgs {
    /// Regular expression (Rust regex syntax).
    pattern: String,
    /// File or folder to search in. Default: the working directory.
    path: Option<String>,
    /// Only search files matching this glob, e.g. "*.rs".
    glob: Option<String>,
    /// Case-insensitive search. Default false.
    ignore_case: Option<bool>,
    /// Maximum number of matching lines. Default 100.
    max_results: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ShellArgs {
    /// Command line to run with the machine's shell.
    command: String,
    /// Working directory. Default: the agent's working directory.
    cwd: Option<String>,
    /// Time limit in seconds. Default 120, maximum 600.
    timeout_secs: Option<u64>,
}

// Las herramientas del harness de Symphony (ADR-0019). Descripciones en inglés: es lo que leen
// los modelos y rinden mejor así. Solo existen si el agente corre en el harness
// (`SYMPHONY_HARNESS`); si no, no se suman al router al arrancar.
#[tool_router(router = harness_router)]
impl Broker {
    #[tool(
        description = "Read a text file. Returns the lines numbered; use offset and limit to page through long files."
    )]
    async fn read(&self, Parameters(a): Parameters<ReadArgs>) -> Result<String, ErrorData> {
        harness::read(&self.root, &a.path, a.offset, a.limit).map_err(fail)
    }

    #[tool(
        description = "Create a file or replace its whole content. Missing parent folders are created. To change part of an existing file use edit."
    )]
    async fn write(&self, Parameters(a): Parameters<WriteArgs>) -> Result<String, ErrorData> {
        harness::write(&self.root, &a.path, &a.content).map_err(fail)
    }

    #[tool(
        description = "Replace exact text in a file. old must match the file exactly and be unique unless replace_all is true; read the file first."
    )]
    async fn edit(&self, Parameters(a): Parameters<EditArgs>) -> Result<String, ErrorData> {
        harness::edit(
            &self.root,
            &a.path,
            &a.old,
            &a.new,
            a.replace_all.unwrap_or(false),
        )
        .map_err(fail)
    }

    #[tool(
        description = "Find files by glob pattern (e.g. **/*.rs). Respects .gitignore. Returns paths relative to the searched folder."
    )]
    async fn glob(&self, Parameters(a): Parameters<GlobArgs>) -> Result<String, ErrorData> {
        harness::glob(&self.root, &a.pattern, a.path.as_deref()).map_err(fail)
    }

    #[tool(
        description = "Search file contents with a regular expression. Respects .gitignore. Returns path:line:text for each match."
    )]
    async fn grep(&self, Parameters(a): Parameters<GrepArgs>) -> Result<String, ErrorData> {
        harness::grep(
            &self.root,
            &a.pattern,
            a.path.as_deref(),
            a.glob.as_deref(),
            a.ignore_case.unwrap_or(false),
            a.max_results,
        )
        .map_err(fail)
    }

    #[tool(
        description = "Run a shell command and return its exit code, stdout and stderr. Commands that exceed the time limit are killed."
    )]
    async fn shell(&self, Parameters(a): Parameters<ShellArgs>) -> Result<String, ErrorData> {
        harness::shell(
            &self.root,
            &a.command,
            a.cwd.as_deref(),
            a.timeout_secs.map(std::time::Duration::from_secs),
        )
        .await
        .map_err(fail)
    }
}

#[tool_handler(router = self.tool_router)]
impl rmcp::ServerHandler for Broker {}

/// Sirve MCP por stdio hasta que el CLI cierra la conexión.
pub async fn serve(home: PathBuf) -> miette::Result<()> {
    use miette::IntoDiagnostic;
    // Con el harness (ADR-0019) el modelo no tiene otras herramientas: se suman las de archivos y shell.
    let mut tool_router = Broker::context_router();
    if env("SYMPHONY_HARNESS").is_some() {
        tool_router += Broker::harness_router();
    }
    let broker = Broker {
        tool_router,
        // El worktree del agente lo pone el daemon; el directorio actual es solo el respaldo.
        root: match env("SYMPHONY_WORKTREE") {
            Some(w) => PathBuf::from(w),
            None => std::env::current_dir().into_diagnostic()?,
        },
        home,
        agent: env("SYMPHONY_AGENT_ID"),
        run: env("SYMPHONY_RUN_ID"),
        project: env("SYMPHONY_PROJECT_ID"),
    };
    let service = broker.serve(stdio()).await.into_diagnostic()?;
    service.waiting().await.into_diagnostic()?;
    Ok(())
}
