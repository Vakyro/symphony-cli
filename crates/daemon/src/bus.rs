//! Event bus del daemon (STACK §6.3, IDEA §5.3). Tres consumidores:
//! - persistencia en `events` por el writer único (mpsc acotado, en lotes): nunca pierde;
//! - TUI y otros efímeros por `broadcast`: un suscriptor lento se atrasa (`Lagged`)
//!   pero nunca frena al productor;
//! - estado actual por `watch` (último evento de cada agente).
//!
//! Primero se persiste y después se difunde: lo que ve la TUI ya está guardado.

use std::collections::HashMap;
use std::sync::Arc;

use symphony_adapter_common::AgentEvent;
use symphony_store::{NewEvent, WriterClosed, WriterHandle};
use tokio::sync::{broadcast, watch};

/// Valores de `events.source` (DB §3.F).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSource {
    Hook,
    JsonStream,
    Stdout,
    Process,
    System,
    User,
}

impl EventSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hook => "HOOK",
            Self::JsonStream => "JSON_STREAM",
            Self::Stdout => "STDOUT",
            Self::Process => "PROCESS",
            Self::System => "SYSTEM",
            Self::User => "USER",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BusEvent {
    pub project_id: String,
    pub agent_id: Option<String>,
    pub run_id: Option<String>,
    pub source: EventSource,
    pub event: AgentEvent,
    pub occurred_at: i64,
}

/// Lo último que se sabe de cada agente (para Home y el scheduler).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentSnapshot {
    pub last_event: &'static str,
    pub last_event_at: i64,
    pub events: u64,
}

pub type BusState = HashMap<String, AgentSnapshot>;

#[derive(Clone)]
pub struct EventBus {
    writer: WriterHandle,
    tui: broadcast::Sender<Arc<BusEvent>>,
    state: Arc<watch::Sender<BusState>>,
}

impl EventBus {
    /// `tui_capacity`: cuántos eventos puede atrasarse un suscriptor antes de perder los más viejos.
    pub fn new(writer: WriterHandle, tui_capacity: usize) -> Self {
        let (tui, _) = broadcast::channel(tui_capacity.max(1));
        let (state, _) = watch::channel(BusState::new());
        Self {
            writer,
            tui,
            state: Arc::new(state),
        }
    }

    pub async fn publish(&self, ev: BusEvent) -> Result<(), WriterClosed> {
        let payload = serde_json::to_string(&ev.event).ok();
        self.writer
            .event(NewEvent {
                project_id: ev.project_id.clone(),
                agent_id: ev.agent_id.clone(),
                run_id: ev.run_id.clone(),
                event_type: ev.event.type_name().to_string(),
                source: ev.source.as_str().to_string(),
                payload_json: payload,
                occurred_at: ev.occurred_at,
            })
            .await?;
        if let Some(agent) = &ev.agent_id {
            let (name, at) = (ev.event.type_name(), ev.occurred_at);
            self.state.send_modify(|s| {
                let snap = s.entry(agent.clone()).or_default();
                snap.last_event = name;
                snap.last_event_at = at;
                snap.events += 1;
            });
        }
        // Sin suscriptores, send falla: no es un error.
        let _ = self.tui.send(Arc::new(ev));
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<BusEvent>> {
        self.tui.subscribe()
    }

    pub fn state(&self) -> watch::Receiver<BusState> {
        self.state.subscribe()
    }
}
