//! Cola, slots pesados y equidad (P08.S3, IDEA §5.4). Máquina de estados síncrona y
//! determinista: no duerme, no lee relojes ni toca disco; quien la usa le pasa `now_ms`.
//!
//! Reglas:
//! - Solo las clases 3 y 4 ocupan un slot (`Limits::max_heavy`); lo demás entra directo.
//! - A lo sumo una clase 4 a la vez.
//! - Si la máquina está por encima de los techos de CPU/RAM, los pesados esperan, salvo
//!   que ya hayan esperado `max_wait_ms` (nadie espera para siempre con slots libres).
//! - Entre los que esperan manda la prioridad; luego quien menos pesados tiene corriendo,
//!   luego el más antiguo.

use std::cmp::Reverse;

use crate::classify::OpClass;

/// Límites de un perfil de rendimiento (IDEA §5.4).
#[derive(Debug, Clone, PartialEq)]
pub struct Limits {
    /// Operaciones pesadas (clases 3–4) simultáneas.
    pub max_heavy: usize,
    /// Por encima de este CPU (%) los pesados esperan.
    pub cpu_ceiling_pct: f64,
    /// Por encima de esta RAM (%) los pesados esperan.
    pub ram_ceiling_pct: f64,
    /// Tras esta espera, la presión de la máquina deja de retener a un pesado.
    pub max_wait_ms: i64,
}

impl Limits {
    /// Límites del perfil elegido, con los valores de `[performance]` que los pisan.
    pub fn from_config(cfg: &symphony_core::PerformanceConfig) -> Self {
        use symphony_core::PerformanceProfile as P;
        let mut l = match cfg.profile {
            P::Eco => Self::eco(),
            P::Performance => Self::performance(),
            P::Balanced | P::Custom => Self::balanced(),
        };
        if let Some(n) = cfg.max_heavy {
            l.max_heavy = n as usize;
        }
        if let Some(c) = cfg.cpu_ceiling_pct {
            l.cpu_ceiling_pct = c;
        }
        if let Some(r) = cfg.ram_ceiling_pct {
            l.ram_ceiling_pct = r;
        }
        l
    }

    pub fn eco() -> Self {
        Self {
            max_heavy: 1,
            cpu_ceiling_pct: 50.0,
            ram_ceiling_pct: 60.0,
            max_wait_ms: 120_000,
        }
    }

    /// `balanced`: 1 proceso pesado, CPU/RAM al 70 % (IDEA §5.4).
    pub fn balanced() -> Self {
        Self {
            max_heavy: 1,
            cpu_ceiling_pct: 70.0,
            ram_ceiling_pct: 70.0,
            max_wait_ms: 60_000,
        }
    }

    pub fn performance() -> Self {
        Self {
            max_heavy: 3,
            cpu_ceiling_pct: 90.0,
            ram_ceiling_pct: 85.0,
            max_wait_ms: 30_000,
        }
    }
}

/// Límites que el SO hace cumplir sobre el árbol de procesos de cada agente (S4).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OsLimits {
    pub cpu_cores: Option<f64>,
    pub max_memory_bytes: Option<u64>,
    pub max_processes: Option<u32>,
}

impl OsLimits {
    /// La CPU sale del perfil (`ECO` 1 núcleo, `BALANCED` 2, `PERFORMANCE` sin límite) y los
    /// tres se pueden fijar en `[performance]`. Memoria y procesos: solo si se piden.
    pub fn from_config(cfg: &symphony_core::PerformanceConfig) -> Self {
        use symphony_core::PerformanceProfile as P;
        let by_profile = match cfg.profile {
            P::Eco => Some(1.0),
            P::Balanced => Some(2.0),
            P::Performance | P::Custom => None,
        };
        Self {
            cpu_cores: cfg.os_cpu_cores.or(by_profile),
            max_memory_bytes: cfg
                .os_max_memory_mb
                .map(|mb| mb.saturating_mul(1024 * 1024)),
            max_processes: cfg.os_max_processes,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::balanced()
    }
}

/// Una operación que pide correr.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Identidad estable de la operación (`run:tool_use_id`).
    pub id: String,
    /// Etiqueta opaca que el llamador recupera en las razones (el id de su `tool_call`).
    pub tag: String,
    pub agent: String,
    /// Cómo se nombra al agente en los textos («Agent #2»).
    pub agent_label: String,
    pub class: OpClass,
    /// Mayor = antes.
    pub priority: i32,
    pub command: Option<String>,
    pub now_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
struct Active {
    req: Request,
}

#[derive(Debug, Clone, PartialEq)]
struct Waiting {
    req: Request,
    seq: u64,
}

/// Quién ocupa un slot.
#[derive(Debug, Clone, PartialEq)]
pub struct Holder {
    pub tag: String,
    pub agent_label: String,
    pub class: OpClass,
    pub command: Option<String>,
}

/// Por qué una operación no corre todavía.
#[derive(Debug, Clone, PartialEq)]
pub enum WaitReason {
    /// Todos los slots pesados están ocupados.
    SlotsFull { holders: Vec<Holder>, max: usize },
    /// Ya corre una operación muy pesada (clase 4) y solo cabe una.
    VeryHeavyRunning { holder: Holder },
    /// La máquina está por encima de un techo.
    Pressure {
        cpu_pct: f64,
        ram_pct: f64,
        cpu_ceiling_pct: f64,
        ram_ceiling_pct: f64,
    },
}

impl WaitReason {
    /// A quién se espera, si hay alguien concreto (para `blocked_by_tool_call_id`).
    pub fn blocker(&self) -> Option<&Holder> {
        match self {
            WaitReason::SlotsFull { holders, .. } => holders.first(),
            WaitReason::VeryHeavyRunning { holder } => Some(holder),
            WaitReason::Pressure { .. } => None,
        }
    }
}

/// Resultado de pedir correr.
#[derive(Debug, Clone, PartialEq)]
pub enum Admission {
    /// Corre ya. `held` es `true` si ocupa un slot (clases 3–4).
    Run {
        held: bool,
    },
    Wait(WaitReason),
}

/// Una operación que estaba esperando y ahora corre.
#[derive(Debug, Clone, PartialEq)]
pub struct Grant {
    pub id: String,
    pub tag: String,
    pub waited_ms: i64,
}

/// Foto para las vistas (P08.S8).
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub running: Vec<Holder>,
    pub waiting: Vec<(Holder, WaitReason)>,
    pub max_heavy: usize,
}

#[derive(Debug, Clone)]
pub struct Scheduler {
    limits: Limits,
    running: Vec<Active>,
    waiting: Vec<Waiting>,
    pressure: Option<(f64, f64)>,
    next_seq: u64,
}

impl Scheduler {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            running: Vec::new(),
            waiting: Vec::new(),
            pressure: None,
            next_seq: 0,
        }
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Cambia los límites (perfil nuevo) y reparte lo que ahora quepa.
    pub fn set_limits(&mut self, limits: Limits, now_ms: i64) -> Vec<Grant> {
        self.limits = limits;
        self.pump(now_ms)
    }

    pub fn running_heavy(&self) -> usize {
        self.running.len()
    }

    pub fn waiting_len(&self) -> usize {
        self.waiting.len()
    }

    /// Pide correr. Las clases 0–2 no ocupan nada. Un `id` repetido no se duplica.
    pub fn submit(&mut self, req: Request) -> Admission {
        if !req.class.is_heavy() {
            return Admission::Run { held: false };
        }
        if self.running.iter().any(|a| a.req.id == req.id) {
            return Admission::Run { held: true };
        }
        if let Some(w) = self.waiting.iter().find(|w| w.req.id == req.id) {
            return Admission::Wait(self.block_reason(&w.req, req.now_ms).unwrap_or(
                WaitReason::SlotsFull {
                    holders: Vec::new(),
                    max: self.limits.max_heavy,
                },
            ));
        }
        let (id, now) = (req.id.clone(), req.now_ms);
        let seq = self.next_seq;
        self.next_seq += 1;
        self.waiting.push(Waiting { req, seq });
        // El propio `submit` no devuelve su concesión: si corre, sale como `Run`.
        let granted = self.pump(now);
        if granted.iter().any(|g| g.id == id) {
            return Admission::Run { held: true };
        }
        self.reason_for(&id, now)
            .map_or(Admission::Run { held: true }, Admission::Wait)
    }

    /// Una operación terminó (o se abandonó): libera su slot y reparte.
    pub fn release(&mut self, id: &str, now_ms: i64) -> Vec<Grant> {
        let before = self.running.len() + self.waiting.len();
        self.running.retain(|a| a.req.id != id);
        self.waiting.retain(|w| w.req.id != id);
        if self.running.len() + self.waiting.len() == before {
            return Vec::new();
        }
        self.pump(now_ms)
    }

    /// Libera todo lo de un agente (su run murió).
    pub fn release_agent(&mut self, agent: &str, now_ms: i64) -> Vec<Grant> {
        self.running.retain(|a| a.req.agent != agent);
        self.waiting.retain(|w| w.req.agent != agent);
        self.pump(now_ms)
    }

    /// Última lectura de CPU/RAM de la máquina.
    pub fn set_pressure(&mut self, cpu_pct: f64, ram_pct: f64, now_ms: i64) -> Vec<Grant> {
        self.pressure = Some((cpu_pct, ram_pct));
        self.pump(now_ms)
    }

    /// El tiempo pasó: puede que alguien ya haya esperado `max_wait_ms`.
    pub fn tick(&mut self, now_ms: i64) -> Vec<Grant> {
        self.pump(now_ms)
    }

    /// Quita de la cola a quien ya no espera (timeout del hook). No toca lo que corre.
    pub fn abandon(&mut self, id: &str) -> bool {
        let n = self.waiting.len();
        self.waiting.retain(|w| w.req.id != id);
        self.waiting.len() != n
    }

    /// La razón por la que `id` sigue esperando, o `None` si no espera (corre o no existe).
    pub fn reason_for(&self, id: &str, now_ms: i64) -> Option<WaitReason> {
        let w = self.waiting.iter().find(|w| w.req.id == id)?;
        self.block_reason(&w.req, now_ms)
    }

    pub fn snapshot(&self, now_ms: i64) -> Snapshot {
        let mut waiting: Vec<&Waiting> = self.waiting.iter().collect();
        waiting.sort_by_key(|w| self.order_key(w));
        Snapshot {
            running: self.running.iter().map(|a| holder(&a.req)).collect(),
            waiting: waiting
                .into_iter()
                .filter_map(|w| {
                    self.block_reason(&w.req, now_ms)
                        .map(|r| (holder(&w.req), r))
                })
                .collect(),
            max_heavy: self.limits.max_heavy,
        }
    }

    // --- internos ----------------------------------------------------------

    fn agent_running(&self, agent: &str) -> usize {
        self.running.iter().filter(|a| a.req.agent == agent).count()
    }

    fn order_key(&self, w: &Waiting) -> (Reverse<i32>, usize, i64, u64) {
        (
            Reverse(w.req.priority),
            self.agent_running(&w.req.agent),
            w.req.now_ms,
            w.seq,
        )
    }

    /// Por qué `req` no puede correr ahora; `None` si puede.
    fn block_reason(&self, req: &Request, now_ms: i64) -> Option<WaitReason> {
        if self.running.len() >= self.limits.max_heavy.max(1) {
            return Some(WaitReason::SlotsFull {
                holders: self.running.iter().map(|a| holder(&a.req)).collect(),
                max: self.limits.max_heavy.max(1),
            });
        }
        if req.class == OpClass::VeryHeavy
            && let Some(a) = self
                .running
                .iter()
                .find(|a| a.req.class == OpClass::VeryHeavy)
        {
            return Some(WaitReason::VeryHeavyRunning {
                holder: holder(&a.req),
            });
        }
        if let Some((cpu, ram)) = self.pressure {
            let hot = cpu > self.limits.cpu_ceiling_pct || ram > self.limits.ram_ceiling_pct;
            let aged = now_ms - req.now_ms >= self.limits.max_wait_ms;
            if hot && !aged {
                return Some(WaitReason::Pressure {
                    cpu_pct: cpu,
                    ram_pct: ram,
                    cpu_ceiling_pct: self.limits.cpu_ceiling_pct,
                    ram_ceiling_pct: self.limits.ram_ceiling_pct,
                });
            }
        }
        None
    }

    /// Concede, en orden, todo lo que quepa.
    fn pump(&mut self, now_ms: i64) -> Vec<Grant> {
        let mut granted = Vec::new();
        loop {
            let mut order: Vec<usize> = (0..self.waiting.len()).collect();
            order.sort_by_key(|&i| self.order_key(&self.waiting[i]));
            let next = order
                .into_iter()
                .find(|&i| self.block_reason(&self.waiting[i].req, now_ms).is_none());
            let Some(i) = next else { break };
            let w = self.waiting.remove(i);
            granted.push(Grant {
                id: w.req.id.clone(),
                tag: w.req.tag.clone(),
                waited_ms: (now_ms - w.req.now_ms).max(0),
            });
            self.running.push(Active { req: w.req });
        }
        granted
    }
}

fn holder(req: &Request) -> Holder {
    Holder {
        tag: req.tag.clone(),
        agent_label: req.agent_label.clone(),
        class: req.class,
        command: req.command.clone(),
    }
}

/// Texto humano de por qué algo espera (FLOW §10.1); el modelo lo recibe como razón del
/// `deny` y la TUI lo muestra tal cual.
pub fn explain(reason: &WaitReason) -> String {
    fn what(h: &Holder) -> String {
        match h.command.as_deref() {
            Some(c) => format!("{} corre `{}`", h.agent_label, shorten(c)),
            None => format!("{} corre una operación pesada", h.agent_label),
        }
    }
    match reason {
        WaitReason::SlotsFull { holders, max } => {
            let who = holders.iter().map(what).collect::<Vec<_>>().join("; ");
            let slots = if *max == 1 {
                "el slot pesado está ocupado".to_string()
            } else {
                format!("los {max} slots pesados están ocupados")
            };
            if who.is_empty() {
                format!("en cola: {slots}")
            } else {
                format!("en cola: {slots} ({who})")
            }
        }
        WaitReason::VeryHeavyRunning { holder } => format!(
            "en cola: solo cabe una operación muy pesada a la vez ({})",
            what(holder)
        ),
        WaitReason::Pressure {
            cpu_pct,
            ram_pct,
            cpu_ceiling_pct,
            ram_ceiling_pct,
        } => format!(
            "en cola: la máquina está al {cpu_pct:.0} % de CPU y {ram_pct:.0} % de RAM \
             (techos {cpu_ceiling_pct:.0} % y {ram_ceiling_pct:.0} %)"
        ),
    }
}

fn shorten(s: &str) -> String {
    const MAX: usize = 60;
    let one_line: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= MAX {
        one_line
    } else {
        let cut: String = one_line.chars().take(MAX).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(id: &str, agent: u32, class: OpClass, now: i64) -> Request {
        Request {
            id: id.into(),
            tag: format!("tc-{id}"),
            agent: format!("a{agent}"),
            agent_label: format!("Agent #{agent}"),
            class,
            priority: 0,
            command: Some(format!("cmd-{id}")),
            now_ms: now,
        }
    }

    fn sched(max: usize) -> Scheduler {
        Scheduler::new(Limits {
            max_heavy: max,
            cpu_ceiling_pct: 70.0,
            ram_ceiling_pct: 70.0,
            max_wait_ms: 10_000,
        })
    }

    #[test]
    fn light_operations_never_take_a_slot() {
        let mut s = sched(1);
        for (i, c) in [OpClass::Network, OpClass::Cheap, OpClass::Medium]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                s.submit(req(&i.to_string(), 1, c, 0)),
                Admission::Run { held: false }
            );
        }
        assert_eq!(s.running_heavy(), 0);
    }

    #[test]
    fn the_second_heavy_waits_and_runs_when_the_first_ends() {
        let mut s = sched(1);
        assert_eq!(
            s.submit(req("x", 1, OpClass::Heavy, 0)),
            Admission::Run { held: true }
        );
        let Admission::Wait(reason) = s.submit(req("y", 2, OpClass::Heavy, 5)) else {
            panic!("tenía que esperar");
        };
        assert_eq!(reason.blocker().map(|h| h.tag.as_str()), Some("tc-x"));
        let text = explain(&reason);
        assert!(
            text.contains("Agent #1") && text.contains("cmd-x"),
            "{text}"
        );

        let granted = s.release("x", 100);
        assert_eq!(
            granted,
            vec![Grant {
                id: "y".into(),
                tag: "tc-y".into(),
                waited_ms: 95
            }]
        );
        assert_eq!(s.running_heavy(), 1);
    }

    #[test]
    fn only_one_very_heavy_at_a_time_even_with_free_slots() {
        let mut s = sched(3);
        s.submit(req("b1", 1, OpClass::VeryHeavy, 0));
        let Admission::Wait(r) = s.submit(req("b2", 2, OpClass::VeryHeavy, 1)) else {
            panic!("tenía que esperar");
        };
        assert!(matches!(r, WaitReason::VeryHeavyRunning { .. }));
        // Una clase 3 sí cabe al lado.
        assert_eq!(
            s.submit(req("t1", 3, OpClass::Heavy, 2)),
            Admission::Run { held: true }
        );
    }

    #[test]
    fn priority_then_fewest_running_then_age() {
        let mut s = sched(2);
        s.submit(req("r1", 1, OpClass::Heavy, 0));
        s.submit(req("r2", 1, OpClass::Heavy, 1));
        // Agent 1 ya tiene dos corriendo: Agent 2 pasa primero aunque llegó después.
        s.submit(req("a1-late", 1, OpClass::Heavy, 10));
        s.submit(req("a2", 2, OpClass::Heavy, 20));
        let g = s.release("r1", 40);
        assert_eq!(g.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["a2"]);
        // Con igual carga manda el más antiguo.
        let g = s.release("r2", 41);
        assert_eq!(
            g.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
            ["a1-late"]
        );

        // La prioridad le gana a todo lo demás.
        let mut s = sched(1);
        s.submit(req("run", 1, OpClass::Heavy, 0));
        s.submit(req("old", 2, OpClass::Heavy, 10));
        let mut vip = req("vip", 3, OpClass::Heavy, 30);
        vip.priority = 5;
        s.submit(vip);
        let g = s.release("run", 40);
        assert_eq!(g.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["vip"]);
    }

    #[test]
    fn pressure_holds_heavy_work_until_it_drops_or_the_wait_runs_out() {
        let mut s = sched(2);
        s.set_pressure(95.0, 40.0, 0);
        let Admission::Wait(r) = s.submit(req("t", 1, OpClass::Heavy, 0)) else {
            panic!("la presión tenía que retener");
        };
        assert!(explain(&r).contains("95 % de CPU"), "{}", explain(&r));
        // Baja la presión: corre.
        let g = s.set_pressure(30.0, 40.0, 500);
        assert_eq!(g.len(), 1);

        // Presión que no baja: tras `max_wait_ms` corre igual (con slots libres).
        s.set_pressure(99.0, 99.0, 1_000);
        s.submit(req("u", 2, OpClass::Heavy, 1_000));
        assert!(s.tick(5_000).is_empty());
        let g = s.tick(11_000);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].id, "u");
    }

    #[test]
    fn abandon_and_release_agent_clean_up_and_repump() {
        let mut s = sched(1);
        s.submit(req("r", 1, OpClass::Heavy, 0));
        s.submit(req("w", 2, OpClass::Heavy, 1));
        assert!(s.abandon("w"));
        assert!(!s.abandon("w"));
        s.submit(req("w2", 2, OpClass::Heavy, 2));
        // El agente 1 muere: su slot pasa al que espera.
        let g = s.release_agent("a1", 10);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].id, "w2");
    }

    #[test]
    fn resubmitting_the_same_id_does_not_duplicate() {
        let mut s = sched(1);
        s.submit(req("x", 1, OpClass::Heavy, 0));
        assert_eq!(
            s.submit(req("x", 1, OpClass::Heavy, 1)),
            Admission::Run { held: true }
        );
        assert_eq!(s.running_heavy(), 1);
        s.submit(req("y", 2, OpClass::Heavy, 2));
        s.submit(req("y", 2, OpClass::Heavy, 3));
        assert_eq!(s.waiting_len(), 1);
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        #[derive(Debug, Clone)]
        enum Step {
            Submit { agent: u8, class: u8, priority: i8 },
            ReleaseOldest,
            ReleaseAgent(u8),
            Abandon(u8),
            Pressure(u8, u8),
            Tick(u16),
        }

        fn step() -> impl Strategy<Value = Step> {
            prop_oneof![
                4 => (0u8..4, 0u8..5, -2i8..3).prop_map(|(agent, class, priority)| Step::Submit { agent, class, priority }),
                3 => Just(Step::ReleaseOldest),
                1 => (0u8..4).prop_map(Step::ReleaseAgent),
                1 => (0u8..30).prop_map(Step::Abandon),
                2 => (0u8..101, 0u8..101).prop_map(|(c, r)| Step::Pressure(c, r)),
                2 => (0u16..5_000).prop_map(Step::Tick),
            ]
        }

        fn class(n: u8) -> OpClass {
            match n {
                0 => OpClass::Network,
                1 => OpClass::Cheap,
                2 => OpClass::Medium,
                3 => OpClass::Heavy,
                _ => OpClass::VeryHeavy,
            }
        }

        proptest! {
            /// Con cualquier secuencia: nunca más de N pesados, nunca dos clase 4, y quien
            /// espera tiene siempre una razón (nadie espera con todo libre).
            #[test]
            fn invariants_hold_under_any_sequence(max in 1usize..4, steps in proptest::collection::vec(step(), 1..120)) {
                let mut s = Scheduler::new(Limits { max_heavy: max, cpu_ceiling_pct: 70.0, ram_ceiling_pct: 70.0, max_wait_ms: 3_000 });
                let (mut now, mut n) = (0i64, 0u32);
                let mut live: Vec<String> = Vec::new();
                for st in steps {
                    now += 100;
                    match st {
                        Step::Submit { agent, class: c, priority } => {
                            n += 1;
                            let id = format!("op{n}");
                            let mut r = req(&id, u32::from(agent), class(c), now);
                            r.priority = i32::from(priority);
                            if class(c).is_heavy() { live.push(id); }
                            s.submit(r);
                        }
                        Step::ReleaseOldest => {
                            if !live.is_empty() { let id = live.remove(0); s.release(&id, now); }
                        }
                        Step::ReleaseAgent(a) => { s.release_agent(&format!("a{a}"), now); }
                        Step::Abandon(i) => {
                            if let Some(id) = live.get(usize::from(i) % live.len().max(1)).cloned() { s.abandon(&id); }
                        }
                        Step::Pressure(c, r) => { s.set_pressure(f64::from(c), f64::from(r), now); }
                        Step::Tick(ms) => { now += i64::from(ms); s.tick(now); }
                    }
                    // El reloj avanzó: se reparte lo que ya envejeció antes de mirar.
                    s.tick(now);
                    let snap = s.snapshot(now);
                    prop_assert!(snap.running.len() <= max, "más de {max} pesados");
                    let very = snap.running.iter().filter(|h| h.class == OpClass::VeryHeavy).count();
                    prop_assert!(very <= 1, "dos clase 4 a la vez");
                    // Todo lo que espera tiene una razón concreta; si no, el pump lo habría concedido.
                    prop_assert_eq!(snap.waiting.len(), s.waiting_len());
                }
            }

            /// Sin presión y con slots libres, nadie queda en la cola tras un `submit`.
            #[test]
            fn nobody_waits_while_slots_are_free(n in 1usize..6) {
                let mut s = Scheduler::new(Limits { max_heavy: 6, ..Limits::balanced() });
                for i in 0..n {
                    let a = s.submit(req(&format!("o{i}"), i as u32, OpClass::Heavy, 0));
                    prop_assert_eq!(a, Admission::Run { held: true });
                }
                prop_assert_eq!(s.waiting_len(), 0);
            }
        }
    }
}
