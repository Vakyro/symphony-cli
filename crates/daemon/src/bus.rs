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

use crate::checkpoint::Checkpointer;
use crate::recorder::Recorder;
use symphony_object_store::ObjectStore;

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

/// Los cambios de estado salen del punto único de escritura (`set_agent_state`) y
/// llegan a los suscriptores sin pasar por `events`: el estado ya vive en `agents`.
fn forward_state_changes(
    mut changes: broadcast::Receiver<symphony_store::AgentStateChange>,
    tui: &broadcast::Sender<Arc<BusEvent>>,
) {
    use broadcast::error::RecvError;
    // Débil: el reenviador no mantiene abierto el canal cuando el bus se suelta.
    let tui = tui.downgrade();
    tokio::spawn(async move {
        loop {
            match changes.recv().await {
                Ok(c) => {
                    let Some(tui) = tui.upgrade() else { return };
                    let _ = tui.send(Arc::new(BusEvent {
                        project_id: c.project_id,
                        agent_id: Some(c.agent_id),
                        run_id: None,
                        source: EventSource::System,
                        event: AgentEvent::StateChanged {
                            from: c.from.as_str().into(),
                            to: c.to.as_str().into(),
                            reason: c.reason,
                        },
                        occurred_at: crate::runtime::now_ms(),
                    }));
                }
                // Los suscriptores releen el estado al ver `bus.lagged` o el siguiente evento.
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            }
        }
    });
}

#[derive(Clone)]
pub struct EventBus {
    writer: WriterHandle,
    recorder: Arc<Recorder>,
    checkpoints: Checkpointer,
    tui: broadcast::Sender<Arc<BusEvent>>,
    state: Arc<watch::Sender<BusState>>,
}

impl EventBus {
    /// `tui_capacity`: cuántos eventos puede atrasarse un suscriptor antes de perder los más viejos.
    pub fn new(writer: WriterHandle, tui_capacity: usize, objects: ObjectStore) -> Self {
        let (tui, _) = broadcast::channel(tui_capacity.max(1));
        let (state, _) = watch::channel(BusState::new());
        let checkpoints =
            Checkpointer::start(writer.clone(), objects.clone(), crate::checkpoint::KEEP);
        forward_state_changes(writer.state_changes(), &tui);
        Self {
            writer,
            recorder: Arc::new(Recorder::new(objects)),
            checkpoints,
            tui,
            state: Arc::new(state),
        }
    }

    /// La reserva de cuota por proveedor de `config.toml`.
    pub fn set_health_config(&self, cfg: crate::health::HealthConfig) {
        self.recorder.set_health_config(cfg);
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
        // Mensajes y tool calls derivados (P06.S2), en el mismo orden que el evento.
        if let Some(write) = self.recorder.plan(&ev) {
            match self.writer.write(write).await {
                Ok(()) => {}
                Err(symphony_store::StoreError::WriterClosed) => return Err(WriterClosed),
                Err(e) => {
                    tracing::warn!(error = %e, "no se pudo registrar el mensaje o la tool call")
                }
            }
        }
        self.checkpoints.observe(&ev);
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

    /// Espera a que se tomen los checkpoints pedidos hasta ahora (apagado y tests).
    pub async fn checkpoints_idle(&self) {
        self.checkpoints.idle().await;
    }

    /// El run terminó: suelta su estado de deduplicación.
    pub fn run_ended(&self, run: symphony_core::RunId) {
        self.recorder.run_ended(run);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<BusEvent>> {
        self.tui.subscribe()
    }

    pub fn state(&self) -> watch::Receiver<BusState> {
        self.state.subscribe()
    }
}
