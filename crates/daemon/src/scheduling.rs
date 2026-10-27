//! Capa asíncrona del scheduler (P08.S3): la cola síncrona de `symphony-scheduler`, con
//! esperas por `oneshot`, un latido de 1 s, arrendamientos (un slot que nadie libera
//! caduca) y liberación al cambiar de estado un agente.
//!
//! La política vive en el crate `scheduler`; aquí solo se conectan relojes y canales.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use symphony_core::AgentState;
use symphony_scheduler::{
    Admission, Grant, Limits, Request, Scheduler, Snapshot, WaitReason, explain,
};
use symphony_store::WriterHandle;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

/// Un slot que nadie libera (hook perdido, CLI colgado) se recupera pasado este tiempo.
const LEASE_MS: i64 = 30 * 60 * 1000;
const HEARTBEAT: Duration = Duration::from_secs(1);

use crate::runtime::now_ms;

struct Inner {
    core: Scheduler,
    waiters: HashMap<String, oneshot::Sender<Grant>>,
    /// Cuándo caduca el slot de cada operación que corre.
    leases: HashMap<String, i64>,
    /// `max_wait_ms` del perfil: se restaura al liberarse el disco.
    max_wait_ms: i64,
    /// Última lectura de `(cpu_pct, ram_pct)` de la máquina.
    pressure: (f64, f64),
    /// El disco del proyecto tiene menos de [`crate::resources::DISK_LOW`] libres.
    disk_low: bool,
}

impl Inner {
    /// Aplica la presión vigente al scheduler. Con el disco casi lleno se fuerza el 100 %
    /// de presión y `max_wait_ms = i64::MAX` para que las operaciones pesadas (clases 3–4)
    /// esperen hasta que se libere espacio.
    fn apply_pressure(&mut self, now: i64) {
        let hold = self.disk_low || (self.pressure.0 >= 100.0 && self.pressure.1 >= 100.0);
        let mut limits = self.core.limits().clone();
        limits.max_wait_ms = if hold { i64::MAX } else { self.max_wait_ms };
        let mut grants = if hold {
            let mut g = self.core.set_limits(limits, 0);
            g.extend(self.core.set_pressure(100.0, 100.0, 0));
            g
        } else {
            let (cpu, ram) = self.pressure;
            let mut g = self.core.set_pressure(cpu, ram, now);
            g.extend(self.core.set_limits(limits, now));
            g
        };
        if hold {
            grants.clear();
        }
        self.deliver(grants, now);
    }

    /// Despierta a los que ya corren. Un grant sin nadie esperando (el hook se fue) se
    /// libera enseguida, y lo que eso libere se reparte en la misma pasada.
    fn deliver(&mut self, mut grants: Vec<Grant>, now: i64) {
        while let Some(g) = grants.pop() {
            match self.waiters.remove(&g.id) {
                Some(tx) => {
                    self.leases.insert(g.id.clone(), now + LEASE_MS);
                    if tx.send(g.clone()).is_err() {
                        self.leases.remove(&g.id);
                        grants.extend(self.core.release(&g.id, now));
                    }
                }
                None => {
                    self.leases.remove(&g.id);
                    grants.extend(self.core.release(&g.id, now));
                }
            }
        }
    }
}

/// Una operación que esperará su turno.
pub struct Queued {
    pub reason: WaitReason,
    id: String,
    rx: oneshot::Receiver<Grant>,
}

pub enum Submitted {
    /// Corre ya. `held`: ocupa un slot pesado.
    Run {
        held: bool,
    },
    Queued(Queued),
}

pub enum Waited {
    Granted {
        waited_ms: i64,
    },
    /// Esperó el máximo del hook sin turno: se rechaza con esta razón.
    TimedOut {
        reason: WaitReason,
    },
}

pub struct Scheduling {
    inner: Mutex<Inner>,
    /// Tope en segundos de la retención de un hook (0 = el del proveedor).
    hold_cap_secs: std::sync::atomic::AtomicU64,
}

impl Scheduling {
    pub fn new(limits: Limits) -> Self {
        let max_wait_ms = limits.max_wait_ms;
        Self {
            inner: Mutex::new(Inner {
                core: Scheduler::new(limits),
                waiters: HashMap::new(),
                leases: HashMap::new(),
                max_wait_ms,
                pressure: (0.0, 0.0),
                disk_low: false,
            }),
            hold_cap_secs: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// `[performance] hold_max_secs`: recorta la retención del proveedor.
    pub fn set_hold_cap(&self, secs: Option<u64>) {
        self.hold_cap_secs
            .store(secs.unwrap_or(0), std::sync::atomic::Ordering::Relaxed);
    }

    /// Cuánto retiene un hook de este proveedor (ADR-0003, con el tope de la config).
    pub fn hold_for(&self, provider: Option<&str>) -> Duration {
        let by_provider =
            symphony_adapter_common::hooks::HoldTimes::for_provider(provider).hold_secs;
        let cap = self
            .hold_cap_secs
            .load(std::sync::atomic::Ordering::Relaxed);
        Duration::from_secs(if cap == 0 {
            by_provider
        } else {
            by_provider.min(cap)
        })
    }

    /// Pide correr. Si hay que esperar, devuelve `Queued` con la razón: quien llama la
    /// persiste y luego hace `wait`.
    pub fn submit(&self, req: Request) -> Submitted {
        let Ok(mut inner) = self.inner.lock() else {
            // Un mutex envenenado no debe frenar a los agentes: se deja correr.
            return Submitted::Run { held: false };
        };
        let (id, now) = (req.id.clone(), req.now_ms);
        match inner.core.submit(req) {
            Admission::Run { held } => {
                if held {
                    inner.leases.insert(id, now + LEASE_MS);
                }
                Submitted::Run { held }
            }
            Admission::Wait(reason) => {
                let (tx, rx) = oneshot::channel();
                inner.waiters.insert(id.clone(), tx);
                Submitted::Queued(Queued { reason, id, rx })
            }
        }
    }

    /// Espera el turno hasta `hold`. Si vence, saca a la operación de la cola.
    pub async fn wait(&self, q: Queued, hold: Duration) -> Waited {
        let Queued { id, rx, .. } = q;
        match tokio::time::timeout(hold, rx).await {
            Ok(Ok(g)) => Waited::Granted {
                waited_ms: g.waited_ms,
            },
            Ok(Err(_)) | Err(_) => {
                let Ok(mut inner) = self.inner.lock() else {
                    return Waited::Granted { waited_ms: 0 };
                };
                inner.waiters.remove(&id);
                let now = now_ms();
                // La razón vigente (puede haber cambiado desde que se encoló).
                let reason = inner.core.reason_for(&id, now);
                if inner.core.abandon(&id) {
                    let reason = reason.unwrap_or_else(|| WaitReason::SlotsFull {
                        holders: Vec::new(),
                        max: inner.core.limits().max_heavy,
                    });
                    Waited::TimedOut { reason }
                } else {
                    // Se concedió justo al vencer: ya tiene su slot.
                    Waited::Granted { waited_ms: 0 }
                }
            }
        }
    }

    /// La operación terminó (o se abandonó).
    pub fn release(&self, id: &str) {
        let now = now_ms();
        if let Ok(mut inner) = self.inner.lock() {
            inner.leases.remove(id);
            let grants = inner.core.release(id, now);
            inner.deliver(grants, now);
        }
    }

    /// El agente ya no corre: se libera todo lo suyo.
    pub fn release_agent(&self, agent: &str) {
        let now = now_ms();
        if let Ok(mut inner) = self.inner.lock() {
            let grants = inner.core.release_agent(agent, now);
            inner.deliver(grants, now);
        }
    }

    /// Última lectura de CPU y RAM de la máquina.
    pub fn set_pressure(&self, cpu_pct: f64, ram_pct: f64) {
        let now = now_ms();
        if let Ok(mut inner) = self.inner.lock() {
            inner.pressure = (cpu_pct, ram_pct);
            inner.apply_pressure(now);
        }
    }

    /// El disco del proyecto bajó de [`crate::resources::DISK_LOW`] (`true`) o volvió a tener
    /// espacio (`false`). Reutiliza la presión del scheduler para retener las operaciones
    /// pesadas (clases 3–4) mientras dure el aviso y reanudarlas al liberarse el disco.
    pub fn set_disk_low(&self, low: bool) {
        let now = now_ms();
        if let Ok(mut inner) = self.inner.lock() {
            inner.disk_low = low;
            inner.apply_pressure(now);
        }
    }

    pub fn set_limits(&self, mut limits: Limits) {
        let now = now_ms();
        if let Ok(mut inner) = self.inner.lock() {
            inner.max_wait_ms = limits.max_wait_ms;
            if inner.disk_low || (inner.pressure.0 >= 100.0 && inner.pressure.1 >= 100.0) {
                limits.max_wait_ms = i64::MAX;
            }
            let grants = inner.core.set_limits(limits, now);
            inner.deliver(grants, now);
        }
    }

    pub fn snapshot(&self) -> Option<Snapshot> {
        self.inner.lock().ok().map(|i| i.core.snapshot(now_ms()))
    }

    fn heartbeat(&self) {
        let now = now_ms();
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let expired: Vec<String> = inner
            .leases
            .iter()
            .filter(|(_, until)| **until <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            tracing::warn!(op = %id, "slot pesado caducado por arrendamiento");
            inner.leases.remove(&id);
            let grants = inner.core.release(&id, now);
            inner.deliver(grants, now);
        }
        let grants = inner.core.tick(now);
        inner.deliver(grants, now);
    }

    /// Latido y liberación por cambio de estado. Termina con `shutdown`.
    pub fn spawn(
        self: &std::sync::Arc<Self>,
        writer: WriterHandle,
        shutdown: CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        let mut states = writer.state_changes();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(HEARTBEAT);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    _ = tick.tick() => me.heartbeat(),
                    change = states.recv() => match change {
                        Ok(c) => {
                            // Mientras espera o corre, el agente conserva sus slots.
                            if !matches!(
                                c.to,
                                AgentState::Running | AgentState::WaitingResource | AgentState::Testing
                            ) {
                                me.release_agent(&c.agent_id);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        })
    }
}

/// Texto para el modelo cuando se rechaza una operación en cola.
pub fn denial_text(reason: &WaitReason, hold: Duration) -> String {
    format!(
        "{}. Esperó {} s sin turno; reintenta en unos segundos.",
        explain(reason),
        hold.as_secs()
    )
}
