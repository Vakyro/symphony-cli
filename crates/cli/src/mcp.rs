//! `symphony mcp serve`: el broker MCP del contexto (P09.S8, STACK §15).
//!
//! Un proceso mínimo por CLI, hablando MCP por stdio. No guarda nada: cada herramienta se
//! reenvía al daemon por IPC (`context.search|retrieve|lines`), que es el dueño del índice y
//! registra el acceso en `context_retrievals`. Los nombres usan `_` y no `.`: varios CLIs solo
//! aceptan `[A-Za-z0-9_-]` en el nombre de una herramienta.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ErrorData, ServiceExt, schemars, tool, tool_router, transport::stdio};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::client;

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

#[tool_router(server_handler)]
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

/// Sirve MCP por stdio hasta que el CLI cierra la conexión.
pub async fn serve(home: PathBuf) -> miette::Result<()> {
    use miette::IntoDiagnostic;
    let broker = Broker {
        home,
        agent: env("SYMPHONY_AGENT_ID"),
        run: env("SYMPHONY_RUN_ID"),
        project: env("SYMPHONY_PROJECT_ID"),
    };
    let service = broker.serve(stdio()).await.into_diagnostic()?;
    service.waiting().await.into_diagnostic()?;
    Ok(())
}
