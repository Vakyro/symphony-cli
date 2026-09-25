//! Espejo de la conversación (`messages`) y registro de `tool_calls` a partir de
//! los eventos canónicos (P06.S2). Hooks y stream pueden informar lo mismo, así
//! que se deduplica: tool calls por `tool_use_id`, mensajes del usuario por texto
//! (el prompt lo publica el runtime y puede volver por el hook `UserPromptSubmit`).

use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use symphony_adapter_common::{AgentEvent, ToolKind};
use symphony_core::{AgentId, ContextObjectId, MessageId, ProjectId, RunId, ToolCallId};
use symphony_object_store::ObjectStore;
use symphony_store::{WriteFn, repo};

use crate::bus::BusEvent;

/// Mensajes más largos que esto van al object store (DB §3.C `messages`).
pub const INLINE_MAX: usize = 4 * 1024;

pub struct Recorder {
    objects: ObjectStore,
    open: Mutex<Open>,
}

#[derive(Default)]
struct Open {
    /// (run, tool_use_id) → (tool call, ¿ya terminó?).
    by_use_id: HashMap<(RunId, String), (ToolCallId, bool)>,
    /// Tool calls sin `tool_use_id`, por (run, herramienta), en orden de llegada.
    unkeyed: HashMap<(RunId, String), VecDeque<ToolCallId>>,
    /// Último mensaje del usuario de cada run.
    last_user: HashMap<RunId, String>,
}

impl Open {
    fn forget(&mut self, run: RunId) {
        self.by_use_id.retain(|(r, _), _| *r != run);
        self.unkeyed.retain(|(r, _), _| *r != run);
        self.last_user.remove(&run);
    }
}

/// Clase de operación provisional hasta el clasificador del scheduler (P08).
fn op_class(kind: ToolKind) -> i64 {
    // ponytail: todo comando es "medio"; P08 clasifica por comando real (test, build, install…).
    match kind {
        ToolKind::Command => 2,
        ToolKind::Edit | ToolKind::Other => 1,
    }
}

fn sql_err(e: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}

impl Recorder {
    pub fn new(objects: ObjectStore) -> Self {
        Self {
            objects,
            open: Mutex::new(Open::default()),
        }
    }

    /// Suelta el estado en memoria de un run que terminó.
    pub fn run_ended(&self, run: RunId) {
        if let Ok(mut open) = self.open.lock() {
            open.forget(run);
        }
    }

    /// Escritura derivada del evento (mensaje o tool call), si corresponde.
    pub fn plan(&self, ev: &BusEvent) -> Option<WriteFn> {
        let agent: AgentId = ev.agent_id.as_deref()?.parse().ok()?;
        let project: ProjectId = ev.project_id.parse().ok()?;
        let run: Option<RunId> = ev.run_id.as_deref().and_then(|r| r.parse().ok());
        let now = ev.occurred_at;
        let mut open = self.open.lock().ok()?;
        match &ev.event {
            AgentEvent::AssistantText { text } => {
                Some(self.message(project, agent, run, repo::MessageRole::Assistant, text, now))
            }
            AgentEvent::UserMessage { text } => {
                if let Some(run) = run {
                    if open.last_user.get(&run) == Some(text) {
                        return None;
                    }
                    open.last_user.insert(run, text.clone());
                }
                Some(self.message(project, agent, run, repo::MessageRole::User, text, now))
            }
            AgentEvent::ToolRequested {
                tool_use_id,
                tool,
                kind,
                command,
            } => {
                let run = run?;
                let id = ToolCallId::new();
                match tool_use_id {
                    Some(use_id) => {
                        if open.by_use_id.contains_key(&(run, use_id.clone())) {
                            return None;
                        }
                        open.by_use_id.insert((run, use_id.clone()), (id, false));
                    }
                    None => open
                        .unkeyed
                        .entry((run, tool.clone()))
                        .or_default()
                        .push_back(id),
                }
                let call = repo::NewToolCall {
                    id,
                    agent_id: agent,
                    run_id: run,
                    tool_name: tool.clone(),
                    command: command
                        .as_deref()
                        .map(|c| symphony_core::redact(c).into_owned()),
                    op_class: op_class(*kind),
                };
                Some(Box::new(move |t| {
                    Ok(repo::insert_tool_call(t, &call, now)?)
                }))
            }
            AgentEvent::ToolFinished {
                tool_use_id,
                tool,
                kind,
                ok,
                exit_code,
            } => {
                let run = run?;
                // (tool call, ¿ya estaba registrada?)
                let (id, requested) = match tool_use_id {
                    Some(use_id) => match open.by_use_id.entry((run, use_id.clone())) {
                        Entry::Occupied(mut e) => {
                            if e.get().1 {
                                return None;
                            }
                            e.get_mut().1 = true;
                            (e.get().0, true)
                        }
                        Entry::Vacant(e) => (e.insert((ToolCallId::new(), true)).0, false),
                    },
                    None => match open
                        .unkeyed
                        .get_mut(&(run, tool.clone()))
                        .and_then(VecDeque::pop_front)
                    {
                        Some(id) => (id, true),
                        None => (ToolCallId::new(), false),
                    },
                };
                // Solo vimos el final (el pedido no llegó): se registra entera.
                let call = (!requested).then(|| repo::NewToolCall {
                    id,
                    agent_id: agent,
                    run_id: run,
                    tool_name: tool.clone(),
                    command: None,
                    op_class: op_class(*kind),
                });
                let (ok, exit_code) = (*ok, *exit_code);
                Some(Box::new(move |t| {
                    if let Some(call) = &call {
                        repo::insert_tool_call(t, call, now)?;
                    }
                    Ok(repo::finish_tool_call(t, id, ok, exit_code, now)?)
                }))
            }
            _ => None,
        }
    }

    fn message(
        &self,
        project: ProjectId,
        agent: AgentId,
        run: Option<RunId>,
        role: repo::MessageRole,
        text: &str,
        now: i64,
    ) -> WriteFn {
        let objects = self.objects.clone();
        let text = text.to_string();
        Box::new(move |t| {
            let id = MessageId::new();
            let (content, object) = if text.len() > INLINE_MAX {
                // ponytail: el archivo del blob se escribe en el hilo del writer; mensajes largos son pocos.
                let stored = objects
                    .put(t, text.as_bytes(), Some("text/plain"))
                    .map_err(sql_err)?;
                objects.add_ref(t, &stored.hash).map_err(sql_err)?;
                let object = ContextObjectId::new();
                repo::insert_context_object(
                    t,
                    object,
                    &format!("ctx://message/{id}"),
                    project,
                    Some(agent),
                    run,
                    "CONVERSATION",
                    &stored.hash,
                    now,
                )?;
                (None, Some(object))
            } else {
                (Some(text), None)
            };
            let m = repo::NewMessage {
                id,
                agent_id: agent,
                run_id: run,
                role,
                content,
                content_object_id: object,
            };
            Ok(repo::insert_message(t, &m, now)?)
        })
    }
}
