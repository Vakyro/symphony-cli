//! Estado de la TUI y su lógica (FLOW §4–§8, §12, §13, §16), sin E/S:
//! `update` recibe un `Msg` y devuelve las llamadas IPC que hay que hacer.
//! Así cada vista se prueba con `TestBackend`, sin daemon ni terminal.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// 00 Chat: la vista inicial (P07.5.S7).
    Chat,
    /// 01 Launch.
    Launch,
    /// 01 Launch, rama "no parece un repo".
    NotARepo,
    /// 02 First-run Setup.
    FirstRun,
    /// 03 Provider Setup.
    ProviderSetup,
    /// 04 Home.
    Home,
    /// 05 New Agent.
    NewAgent,
    /// 13 Model Picker.
    ModelPicker,
    /// 06–09 y 12: vista de agente con pestañas.
    Agent,
    /// 14 Explain Route: por qué se eligió el modelo del agente.
    ExplainRoute,
    /// 22 Providers.
    Providers,
    /// 30 Recovery Center.
    Recovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Conversation,
    Activity,
    Changes,
    History,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Overview,
        Tab::Conversation,
        Tab::Activity,
        Tab::Changes,
        Tab::History,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Resumen",
            Tab::Conversation => "Conversación",
            Tab::Activity => "Actividad",
            Tab::Changes => "Cambios",
            Tab::History => "Historial",
        }
    }

    fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// A qué responde una llamada IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Req {
    ProjectStatus,
    ProjectInit,
    Providers,
    ProviderToggle,
    Agents,
    Models,
    Recovery,
    RecoveryAct,
    Create,
    Inspect,
    Logs,
    Activity,
    Diff,
    History,
    Control,
    Send,
    Switch,
    Attach,
    Export,
    SkillsAnthropic,
    SkillsOpenai,
    ChatGet,
    ChatCreate,
    Profiles,
    Explain,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub req: Req,
    pub method: &'static str,
    pub params: Value,
}

/// Error que devolvió el daemon (o la conexión).
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectPhase {
    Start,
    Move,
    End,
}

/// Texto seleccionado con el ratón: de `start` a `end` (columna, fila), en orden de lectura.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub start: (u16, u16),
    pub end: (u16, u16),
    /// Ya se soltó el botón: el loop copia el texto una vez.
    pub done: bool,
    pub copied: bool,
}

impl Selection {
    /// Extremos ordenados por posición de lectura (fila y luego columna).
    pub fn ordered(&self) -> ((u16, u16), (u16, u16)) {
        let key = |p: (u16, u16)| (p.1, p.0);
        if key(self.start) <= key(self.end) {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    Key(KeyEvent),
    Reply(Req, Result<Value, Failure>),
    /// Evento del bus (`agent.event`, `bus.lagged`).
    Event(String),
    /// Reloj (ms desde epoch): refresco y edades relativas.
    Tick(i64),
    Resize,
    /// Texto pegado en la terminal (pegado entre corchetes: los saltos de línea no son Enter).
    Paste(String),
    /// Rueda del ratón: positivo = hacia atrás en el historial.
    Scroll(i16),
    /// Arrastre del ratón para seleccionar texto (columna, fila de la pantalla).
    Select(SelectPhase, u16, u16),
    Disconnected(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    Info(String),
    Error(String),
}

pub const PERFORMANCE: [(&str, &str); 4] = [
    ("ECO", "Eco"),
    ("BALANCED", "Balanced"),
    ("PERFORMANCE", "Performance"),
    ("CUSTOM", "Custom"),
];
pub const FAILOVER: [(&str, &str); 3] = [
    ("ANY", "Cualquier proveedor"),
    ("SAME_PROVIDER", "Solo el mismo"),
    ("NONE", "Desactivado"),
];
pub const PRIORITY: [(i64, &str); 3] = [(0, "Normal"), (1, "Alta"), (-1, "Baja")];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FirstRun {
    pub name: String,
    pub performance: usize,
    pub failover: usize,
    /// 0 nombre · 1 rendimiento · 2 failover · 3 continuar.
    pub field: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewAgent {
    pub task: String,
    pub exec: Exec,
    pub model: Option<String>,
    /// Profile elegido (`@code`…) cuando `exec == Exec::Profile`.
    pub profile: Option<String>,
    pub failover: usize,
    pub priority: usize,
    /// 0 tarea · 1 ejecución · 2 failover · 3 prioridad · 4 crear.
    pub field: usize,
}

/// Cómo se elige el executor de un agente nuevo (FLOW §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Exec {
    /// Un modelo exacto: se obedece y no se sustituye en silencio.
    Exact,
    /// Un profile: Symphony elige entre los modelos utilizables ahora.
    Profile,
    /// Se decide después.
    #[default]
    Later,
}

impl Exec {
    fn cycle(self, forward: bool) -> Self {
        const ALL: [Exec; 3] = [Exec::Exact, Exec::Profile, Exec::Later];
        let i = ALL.iter().position(|e| *e == self).unwrap_or(0);
        ALL[cycle(i, ALL.len(), forward)]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickFor {
    NewAgent,
    Switch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentView {
    pub id: String,
    pub tab: Tab,
    pub inspect: Value,
    pub messages: Vec<Value>,
    pub activity: Vec<Value>,
    pub diff: String,
    pub history: Value,
    pub scroll: u16,
    /// Tope de `scroll` en Conversación (lo fija el render): ahí `scroll` cuenta desde el final.
    pub scroll_max: std::cell::Cell<u16>,
    /// Escribiendo un mensaje para el executor.
    pub typing: bool,
    pub input: String,
    /// Se pulsó `x` una vez: la segunda confirma el stop.
    pub confirm_stop: bool,
}

/// Desplaza la vista de agente; positivo = hacia lo más nuevo / hacia abajo. En Conversación
/// `scroll` cuenta desde el final (con tope); en el resto, desde el principio.
fn scroll_view(a: &mut AgentView, toward_newer: i32) {
    let now = i32::from(a.scroll);
    let to = if a.tab == Tab::Conversation {
        (now - toward_newer).clamp(0, i32::from(a.scroll_max.get()))
    } else {
        (now + toward_newer).max(0)
    };
    a.scroll = u16::try_from(to).unwrap_or(u16::MAX);
}

impl AgentView {
    fn new(id: String) -> Self {
        Self {
            id,
            tab: Tab::Overview,
            inspect: Value::Null,
            messages: Vec::new(),
            activity: Vec::new(),
            diff: String::new(),
            history: Value::Null,
            scroll: 0,
            scroll_max: std::cell::Cell::new(u16::MAX),
            typing: false,
            input: String::new(),
            confirm_stop: false,
        }
    }

    pub fn state(&self) -> &str {
        self.inspect["agent"]["state"].as_str().unwrap_or("")
    }

    /// Modelo del run abierto o, si no hay, del último que tuvo.
    pub fn model(&self) -> Option<&str> {
        self.inspect["current_run"]["model_id"]
            .as_str()
            .or_else(|| self.inspect["runs"].as_array()?.last()?["model_id"].as_str())
    }
}

/// Cada cuántos ticks (1 s) se relee la vista aunque no llegue ningún evento.
/// Los cambios de estado de agentes llegan por el bus (`AgentStateChanged`, P07.5.S5):
/// esto es solo la red de seguridad para lo que no emite eventos.
pub const POLL_TICKS: u64 = 15;

#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub screen: Screen,
    /// Carpeta desde la que se abrió Symphony.
    pub cwd: String,
    /// Respuesta de `project.status` (`Null` hasta que llega).
    pub project: Value,
    pub providers: Vec<Value>,
    pub agents: Vec<Value>,
    pub models: Vec<Value>,
    /// Profiles de routing (`profiles.list`).
    pub profiles: Vec<Value>,
    /// Decisiones de routing del agente abierto (`route.explain`).
    pub explain: Option<Value>,
    /// Catálogo de skills y comandos por proveedor (`anthropic`, `openai`) para el autocompletado.
    pub skills: std::collections::HashMap<String, Vec<Value>>,
    /// Opción elegida en el desplegable de skills.
    pub skill_sel: usize,
    /// Lo escrito cuando se cerró el desplegable con Esc: mientras no cambie, no se reabre.
    pub skill_dismissed: Option<String>,
    pub recovery: Vec<Value>,
    pub selected: usize,
    pub first_run: FirstRun,
    pub new_agent: NewAgent,
    pub pick_for: PickFor,
    /// A dónde sigue Provider Setup al continuar.
    pub after_setup: Screen,
    pub agent: Option<AgentView>,
    /// El chat del proyecto (`None` hasta que exista: nace con el primer mensaje).
    pub chat: Option<AgentView>,
    pub chat_input: String,
    /// Modelo para el próximo mensaje del chat (Tab lo cambia).
    pub chat_model: Option<String>,
    /// Líneas por encima del final que se está mirando (0 = lo más nuevo).
    pub chat_scroll: u16,
    /// Fotograma de las animaciones (lo avanza el loop mientras algo trabaja).
    pub frame: u32,
    /// Selección con el ratón en curso o ya copiada (se borra al teclear, desplazar o hacer clic).
    pub selection: Option<Selection>,
    /// Texto que el loop debe copiar al portapapeles (lo toma con `take`).
    pub copy: Option<String>,
    /// Captura del ratón (rueda). Apagada, la terminal deja seleccionar y copiar con el ratón.
    pub mouse: bool,
    /// Tope de `chat_scroll` con el tamaño actual (lo fija el render; `ui::chat`).
    pub chat_max: std::cell::Cell<u16>,
    /// Barra de comandos abierta (`:`), con lo escrito.
    pub command: Option<String>,
    pub notice: Option<Notice>,
    pub now_ms: i64,
    pub connected: bool,
    pub quit: bool,
    dirty: bool,
    inflight: usize,
    ticks: u64,
}

fn call(req: Req, method: &'static str, params: Value) -> Call {
    Call {
        req,
        method,
        params,
    }
}

fn cycle(i: usize, len: usize, forward: bool) -> usize {
    if forward {
        (i + 1) % len
    } else {
        (i + len - 1) % len
    }
}

impl App {
    pub fn new(cwd: impl Into<String>) -> Self {
        Self {
            screen: Screen::Launch,
            cwd: cwd.into(),
            project: Value::Null,
            providers: Vec::new(),
            agents: Vec::new(),
            models: Vec::new(),
            profiles: Vec::new(),
            explain: None,
            skills: std::collections::HashMap::new(),
            skill_sel: 0,
            skill_dismissed: None,
            recovery: Vec::new(),
            selected: 0,
            first_run: FirstRun {
                performance: 1,
                ..FirstRun::default()
            },
            new_agent: NewAgent::default(),
            pick_for: PickFor::NewAgent,
            after_setup: Screen::Chat,
            agent: None,
            chat: None,
            chat_input: String::new(),
            chat_model: None,
            chat_scroll: 0,
            frame: 0,
            selection: None,
            copy: None,
            mouse: true,
            chat_max: std::cell::Cell::new(u16::MAX),
            command: None,
            notice: None,
            now_ms: 0,
            connected: true,
            quit: false,
            dirty: false,
            inflight: 0,
            ticks: 0,
        }
    }

    /// Las llamadas del arranque (Launch: proyecto y proveedores).
    pub fn start(&mut self) -> Vec<Call> {
        let calls = vec![
            call(
                Req::ProjectStatus,
                "project.status",
                json!({ "project_root": self.cwd }),
            ),
            call(Req::Providers, "providers.list", json!({})),
        ];
        self.inflight += calls.len();
        calls
    }

    pub fn update(&mut self, msg: Msg) -> Vec<Call> {
        if matches!(msg, Msg::Reply(..)) {
            self.inflight = self.inflight.saturating_sub(1);
        }
        let calls = self.handle(msg);
        self.inflight += calls.len();
        calls
    }

    // --- datos derivados --------------------------------------------------

    pub fn loaded(&self) -> bool {
        !self.project.is_null()
    }

    /// Raíz del repo según el daemon (o la carpeta de arranque mientras no se sabe).
    pub fn root(&self) -> &str {
        self.project["root"].as_str().unwrap_or(&self.cwd)
    }

    pub fn project_name(&self) -> &str {
        self.project["name"].as_str().unwrap_or("")
    }

    pub fn initialized(&self) -> bool {
        self.project["initialized"].as_bool().unwrap_or(false)
    }

    /// Proveedores que necesitan atención (FLOW §4.3).
    pub fn providers_needing_attention(&self) -> usize {
        self.providers
            .iter()
            .filter(|p| p["enabled"].as_bool().unwrap_or(true) && p["setup_state"] != "READY")
            .count()
    }

    /// Agentes vivos primero; completados y cancelados al final (FLOW §5).
    pub fn active_agents(&self) -> Vec<&Value> {
        self.agents.iter().filter(|a| !is_finished(a)).collect()
    }

    pub fn finished_agents(&self) -> Vec<&Value> {
        self.agents.iter().filter(|a| is_finished(a)).collect()
    }

    fn home_order(&self) -> Vec<&Value> {
        let mut all = self.active_agents();
        all.extend(self.finished_agents());
        all
    }

    fn list_len(&self) -> usize {
        match self.screen {
            Screen::Home => self.agents.len(),
            Screen::ProviderSetup | Screen::Providers => self.providers.len(),
            Screen::ModelPicker => self.profiles.len() + self.models.len(),
            Screen::Recovery => self.recovery.len(),
            _ => 0,
        }
    }

    // --- llamadas -----------------------------------------------------------

    fn agents_call(&self) -> Option<Call> {
        // Sin proyecto en la base, `agent.list` devolvería los de todos los proyectos.
        let project = self.project["project_id"].as_str()?;
        Some(call(
            Req::Agents,
            "agent.list",
            json!({ "project_id": project, "all": true }),
        ))
    }

    fn recovery_call(&self) -> Call {
        call(
            Req::Recovery,
            "recovery.list",
            json!({ "project_root": self.root() }),
        )
    }

    fn providers_call() -> Call {
        call(Req::Providers, "providers.list", json!({}))
    }

    fn status_call(&self) -> Call {
        call(
            Req::ProjectStatus,
            "project.status",
            json!({ "project_root": self.cwd }),
        )
    }

    /// La vista de agente que está en pantalla: la del chat o la de un agente.
    /// ¿Hay algo trabajando que se anima en pantalla? El loop redibuja rápido solo entonces.
    pub fn animating(&self) -> bool {
        self.screen == Screen::Chat && self.chat.as_ref().is_some_and(|c| c.state() == "RUNNING")
    }

    fn view(&self) -> Option<&AgentView> {
        if self.screen == Screen::Chat {
            self.chat.as_ref()
        } else {
            self.agent.as_ref()
        }
    }

    fn view_mut(&mut self) -> Option<&mut AgentView> {
        if self.screen == Screen::Chat {
            self.chat.as_mut()
        } else {
            self.agent.as_mut()
        }
    }

    /// Inspect y logs del chat, si existe.
    fn chat_calls(&self) -> Vec<Call> {
        let Some(chat) = &self.chat else {
            return Vec::new();
        };
        vec![
            call(Req::Inspect, "agent.inspect", json!({ "agent": chat.id })),
            call(
                Req::Logs,
                "agent.logs",
                json!({ "agent": chat.id, "limit": 200 }),
            ),
            // Las herramientas que va usando el agente: el proceso en vivo (P07.5.S7b).
            call(Req::Activity, "agent.activity", json!({ "agent": chat.id })),
        ]
    }

    fn available_models(&self) -> Vec<String> {
        self.models
            .iter()
            .filter(|m| m["available"].as_bool().unwrap_or(false))
            .filter_map(|m| m["id"].as_str().map(str::to_string))
            .collect()
    }

    /// Sin elección del usuario, el próximo mensaje va al modelo del chat o, si el chat no
    /// existe, al primero disponible. Con chat existente se espera a conocer su modelo: si no,
    /// un mensaje normal cambiaría de modelo sin que el usuario lo pida.
    fn default_chat_model(&mut self) {
        if self.chat_model.is_some() {
            return;
        }
        self.chat_model = match &self.chat {
            Some(c) => c.model().map(str::to_string),
            None => self.available_models().into_iter().next(),
        };
    }

    fn agent_call(&self, req: Req, method: &'static str) -> Option<Call> {
        let id = &self.view()?.id;
        let params = match req {
            Req::Logs => json!({ "agent": id, "limit": 200 }),
            _ => json!({ "agent": id }),
        };
        Some(call(req, method, params))
    }

    fn tab_call(&self) -> Option<Call> {
        match self.agent.as_ref()?.tab {
            Tab::Overview => None,
            Tab::Conversation => self.agent_call(Req::Logs, "agent.logs"),
            Tab::Activity => self.agent_call(Req::Activity, "agent.activity"),
            Tab::Changes => self.agent_call(Req::Diff, "agent.diff"),
            Tab::History => self.agent_call(Req::History, "agent.history"),
        }
    }

    /// Lo que muestra la pantalla actual, releído del daemon.
    fn refresh(&self) -> Vec<Call> {
        let mut calls = Vec::new();
        match self.screen {
            Screen::Home => {
                calls.extend(self.agents_call());
                calls.push(Self::providers_call());
                calls.push(self.recovery_call());
            }
            Screen::Providers | Screen::ProviderSetup => calls.push(Self::providers_call()),
            Screen::Recovery => {
                calls.push(self.recovery_call());
                calls.extend(self.agents_call());
            }
            Screen::Agent => {
                calls.extend(self.agent_call(Req::Inspect, "agent.inspect"));
                calls.extend(self.tab_call());
            }
            Screen::ModelPicker => {
                calls.push(call(Req::Models, "models.list", json!({})));
                calls.push(call(Req::Profiles, "profiles.list", json!({})));
            }
            Screen::ExplainRoute => calls.extend(self.agent_call(Req::Explain, "route.explain")),
            Screen::Chat => {
                if self.chat.is_none() {
                    calls.push(call(
                        Req::ChatGet,
                        "chat.get",
                        json!({ "project_root": self.root() }),
                    ));
                }
                if self.models.is_empty() {
                    calls.push(call(Req::Models, "models.list", json!({})));
                }
                if self.skills.is_empty() {
                    for (req, provider) in [
                        (Req::SkillsAnthropic, "anthropic"),
                        (Req::SkillsOpenai, "openai"),
                    ] {
                        calls.push(call(
                            req,
                            "skills.list",
                            json!({ "provider": provider, "project_root": self.root() }),
                        ));
                    }
                }
                calls.extend(self.chat_calls());
            }
            Screen::Launch | Screen::NotARepo | Screen::FirstRun | Screen::NewAgent => {}
        }
        calls
    }

    fn go(&mut self, screen: Screen) -> Vec<Call> {
        self.screen = screen;
        self.selected = 0;
        self.refresh()
    }

    fn open_agent(&mut self, id: String) -> Vec<Call> {
        self.agent = Some(AgentView::new(id));
        self.go(Screen::Agent)
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice::Info(text.into()));
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice::Error(text.into()));
    }

    // --- mensajes -----------------------------------------------------------

    fn handle(&mut self, msg: Msg) -> Vec<Call> {
        match msg {
            Msg::Key(key) => {
                self.selection = None;
                self.key(key)
            }
            Msg::Reply(req, result) => self.reply(req, result),
            Msg::Event(_) => {
                self.dirty = true;
                Vec::new()
            }
            Msg::Tick(now) => {
                self.now_ms = now;
                self.ticks += 1;
                let poll = self.ticks.is_multiple_of(POLL_TICKS);
                if self.loaded() && self.connected && self.inflight == 0 && (self.dirty || poll) {
                    self.dirty = false;
                    return self.refresh();
                }
                Vec::new()
            }
            Msg::Resize => Vec::new(),
            Msg::Paste(text) => {
                if self.screen == Screen::Chat {
                    self.chat_input
                        .push_str(&text.replace("\r\n", "\n").replace('\r', "\n"));
                }
                Vec::new()
            }
            Msg::Select(phase, x, y) => {
                match (phase, self.selection.as_mut()) {
                    (SelectPhase::Start, _) => {
                        self.selection = Some(Selection {
                            start: (x, y),
                            end: (x, y),
                            done: false,
                            copied: false,
                        });
                    }
                    (SelectPhase::Move, Some(sel)) if !sel.done => sel.end = (x, y),
                    // Un clic sin arrastrar no es una selección.
                    (SelectPhase::End, Some(sel)) if !sel.done => {
                        sel.end = (x, y);
                        if sel.start == sel.end {
                            self.selection = None;
                        } else {
                            sel.done = true;
                        }
                    }
                    _ => {}
                }
                Vec::new()
            }
            Msg::Scroll(n) => {
                // El contenido se mueve: la selección ya no apunta a lo mismo.
                self.selection = None;
                if self.screen == Screen::Chat {
                    self.scroll_chat(i32::from(n));
                }
                if let (Screen::Agent, Some(a)) = (self.screen, self.agent.as_mut()) {
                    scroll_view(a, -i32::from(n));
                }
                Vec::new()
            }
            Msg::Disconnected(why) => {
                self.connected = false;
                self.error(format!(
                    "Se perdió la conexión con el daemon ({why}). Sal con Ctrl+C y vuelve a abrir symphony."
                ));
                Vec::new()
            }
        }
    }

    fn reply(&mut self, req: Req, result: Result<Value, Failure>) -> Vec<Call> {
        let v = match result {
            Ok(v) => v,
            Err(f) => return self.failed(req, f),
        };
        let list = |key: &str| v[key].as_array().cloned().unwrap_or_default();
        match req {
            Req::ProjectStatus => {
                self.project = v;
                if self.screen != Screen::Launch {
                    return Vec::new();
                }
                if !self.project["is_repo"].as_bool().unwrap_or(false) {
                    self.screen = Screen::NotARepo;
                } else if self.initialized() {
                    let recovery = self.project["recovery_open"].as_i64().unwrap_or(0) > 0;
                    // Estado inconsistente → Recovery antes del Home (FLOW §4.1).
                    return self.go(if recovery {
                        Screen::Recovery
                    } else {
                        Screen::Chat
                    });
                } else if self.first_run.name.is_empty() {
                    self.first_run.name = self.project_name().to_string();
                }
            }
            Req::ProjectInit => {
                self.info("Proyecto inicializado.");
                let next = if self.providers_needing_attention() > 0 {
                    self.after_setup = Screen::Chat;
                    Screen::ProviderSetup
                } else {
                    Screen::Chat
                };
                let mut calls = vec![self.status_call()];
                calls.extend(self.go(next));
                return calls;
            }
            Req::Providers => self.providers = list("providers"),
            Req::ProviderToggle => {
                let enabled = v["enabled"].as_bool().unwrap_or(true);
                self.info(format!(
                    "{} {}.",
                    v["id"].as_str().unwrap_or("proveedor"),
                    if enabled { "activado" } else { "desactivado" }
                ));
                return vec![Self::providers_call()];
            }
            Req::SkillsAnthropic => {
                self.skills.insert("anthropic".into(), list("skills"));
            }
            Req::SkillsOpenai => {
                self.skills.insert("openai".into(), list("skills"));
            }
            Req::Agents => self.agents = list("agents"),
            Req::Profiles => self.profiles = list("profiles"),
            Req::Explain => self.explain = Some(v.clone()),
            Req::Models => {
                self.models = list("models");
                self.default_chat_model();
            }
            Req::ChatGet => {
                if let Some(id) = v["agent_id"].as_str() {
                    self.chat = Some(AgentView::new(id.to_string()));
                    // El valor por defecto ya pudo fijarse sin chat: vale el del chat.
                    self.chat_model = None;
                    return self.chat_calls();
                }
            }
            Req::ChatCreate => {
                let Some(id) = v["agent_id"].as_str() else {
                    return Vec::new();
                };
                self.chat = Some(AgentView::new(id.to_string()));
                self.info("Chat iniciado en la rama symphony/chat.");
                return self.chat_calls();
            }
            Req::Recovery => self.recovery = list("items"),
            Req::RecoveryAct => {
                self.info("Hecho.");
                return self.refresh();
            }
            Req::Create => {
                self.new_agent = NewAgent::default();
                self.info(format!("Agente #{} creado.", v["number"]));
                let mut calls = vec![self.status_call()];
                if let Some(id) = v["agent_id"].as_str() {
                    calls.extend(self.open_agent(id.to_string()));
                }
                return calls;
            }
            Req::Inspect => {
                if let Some(a) = self.view_mut() {
                    a.inspect = v;
                }
                self.default_chat_model();
            }
            Req::Logs => {
                let messages = list("messages");
                if let Some(a) = self.view_mut() {
                    a.messages = messages;
                }
            }
            Req::Activity => {
                let items = list("items");
                if let Some(a) = self.view_mut() {
                    a.activity = items;
                }
            }
            Req::Diff => {
                if let Some(a) = &mut self.agent {
                    a.diff = v["diff"].as_str().unwrap_or("").to_string();
                }
            }
            Req::History => {
                if let Some(a) = &mut self.agent {
                    a.history = v;
                }
            }
            Req::Control | Req::Switch => {
                if let Some(state) = v["state"].as_str() {
                    self.info(format!("Agente {}.", state_phrase(state)));
                } else if let Some(model) = v["model"].as_str() {
                    self.info(format!("Executor cambiado a {model}."));
                }
                return self.refresh();
            }
            Req::Export => {
                let path = v["path"].as_str().unwrap_or("?");
                self.info(format!("Conversación exportada: {path}"));
            }
            Req::Send => {
                self.info("Mensaje enviado.");
                return self.refresh();
            }
            Req::Attach => {
                let cli = v["cli"].as_str().unwrap_or("el CLI");
                if v["opened"].as_bool().unwrap_or(false) {
                    self.info(format!("Sesión abierta en {cli} en una terminal nueva."));
                } else {
                    self.error(format!(
                        "No pude abrir una terminal ({}). Córrelo a mano: {}",
                        v["error"].as_str().unwrap_or("?"),
                        v["command"].as_str().unwrap_or("")
                    ));
                }
                return self.refresh();
            }
        }
        Vec::new()
    }

    fn failed(&mut self, req: Req, f: Failure) -> Vec<Call> {
        if f.code == "disconnected" {
            return self.handle(Msg::Disconnected(f.message));
        }
        match (req, f.code.as_str()) {
            // FLOW §6: sin proveedores elegibles → Provider Setup sin perder la tarea.
            (Req::Create, "no_eligible_provider") => {
                self.error(format!(
                    "{} Habilita un proveedor y vuelve: la tarea sigue escrita.",
                    f.message
                ));
                self.after_setup = Screen::NewAgent;
                self.go(Screen::ProviderSetup)
            }
            (Req::Create, "exact_model_unavailable") => {
                self.error(format!(
                    "{} Elige otro modelo, decide después o Esc para cancelar.",
                    f.message
                ));
                Vec::new()
            }
            _ => {
                self.error(f.message);
                Vec::new()
            }
        }
    }

    // --- teclado -------------------------------------------------------------

    fn key(&mut self, key: KeyEvent) -> Vec<Call> {
        // Salto de línea de un pegado (`spawn_input`): solo el chat lo escribe; en el resto es Enter.
        if key.code == KeyCode::Char('\n') && self.screen != Screen::Chat {
            return self.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return Vec::new();
        }
        if self.command.is_some() {
            return self.command_key(key);
        }
        // Una tecla nueva reemplaza el aviso anterior.
        self.notice = None;
        match self.screen {
            Screen::Launch => self.launch_key(key),
            Screen::NotARepo => {
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter) {
                    self.quit = true;
                }
                Vec::new()
            }
            Screen::FirstRun => self.first_run_key(key),
            Screen::ProviderSetup | Screen::Providers => self.providers_key(key),
            Screen::Chat => self.chat_key(key),
            Screen::Home => self.home_key(key),
            Screen::NewAgent => self.new_agent_key(key),
            Screen::ModelPicker => self.picker_key(key),
            Screen::ExplainRoute => self.explain_key(key),
            Screen::Agent => self.agent_key(key),
            Screen::Recovery => self.recovery_key(key),
        }
    }

    fn move_selection(&mut self, key: KeyCode) -> bool {
        let len = self.list_len();
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < len {
                    self.selected += 1;
                }
                true
            }
            _ => false,
        }
    }

    fn launch_key(&mut self, key: KeyEvent) -> Vec<Call> {
        if !self.loaded() || self.initialized() {
            if key.code == KeyCode::Char('q') {
                self.quit = true;
            }
            return Vec::new();
        }
        match key.code {
            KeyCode::Char('i') | KeyCode::Enter => {
                self.screen = Screen::FirstRun;
                Vec::new()
            }
            KeyCode::Char('o') => self.go(Screen::Chat),
            KeyCode::Char('q') | KeyCode::Esc => {
                self.quit = true;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn first_run_key(&mut self, key: KeyEvent) -> Vec<Call> {
        let root = self.root().to_string();
        let f = &mut self.first_run;
        match key.code {
            KeyCode::Esc => self.screen = Screen::Launch,
            KeyCode::Up | KeyCode::BackTab => f.field = cycle(f.field, 4, false),
            KeyCode::Down | KeyCode::Tab => f.field = cycle(f.field, 4, true),
            KeyCode::Left | KeyCode::Right => {
                let fwd = key.code == KeyCode::Right;
                match f.field {
                    1 => f.performance = cycle(f.performance, PERFORMANCE.len(), fwd),
                    2 => f.failover = cycle(f.failover, FAILOVER.len(), fwd),
                    _ => {}
                }
            }
            KeyCode::Backspace if f.field == 0 => {
                f.name.pop();
            }
            KeyCode::Char(c) if f.field == 0 => f.name.push(c),
            KeyCode::Enter if f.field < 3 => f.field += 1,
            KeyCode::Enter => {
                return vec![call(
                    Req::ProjectInit,
                    "project.init",
                    json!({
                        "project_root": root,
                        "name": f.name.trim(),
                        "performance": PERFORMANCE[f.performance].0,
                        "failover": FAILOVER[f.failover].0,
                    }),
                )];
            }
            _ => {}
        }
        Vec::new()
    }

    fn providers_key(&mut self, key: KeyEvent) -> Vec<Call> {
        if self.move_selection(key.code) {
            return Vec::new();
        }
        match key.code {
            KeyCode::Char('r') => {
                self.info("Detectando CLIs…");
                vec![call(Req::Providers, "providers.refresh", json!({}))]
            }
            KeyCode::Char('d') => {
                let Some(p) = self.providers.get(self.selected) else {
                    return Vec::new();
                };
                vec![call(
                    Req::ProviderToggle,
                    "provider.set_enabled",
                    json!({ "id": p["id"], "enabled": !p["enabled"].as_bool().unwrap_or(true) }),
                )]
            }
            KeyCode::Enter | KeyCode::Char('c') | KeyCode::Esc
                if self.screen == Screen::ProviderSetup =>
            {
                let next = self.after_setup;
                self.go(next)
            }
            KeyCode::Esc | KeyCode::Char('q') => self.go(Screen::Home),
            _ => Vec::new(),
        }
    }

    /// Positivo = hacia atrás en el historial; nunca pasa del tope ni baja de 0.
    fn scroll_chat(&mut self, delta: i32) {
        let to = i32::from(self.chat_scroll).saturating_add(delta);
        self.chat_scroll = to.clamp(0, i32::from(self.chat_max.get())) as u16;
    }

    /// Proveedor (`anthropic`/`openai`) del modelo elegido para el próximo mensaje.
    fn chat_provider(&self) -> Option<&str> {
        let id = self.chat_model.as_deref()?;
        self.models
            .iter()
            .find(|m| m["id"] == id)
            .and_then(|m| m["provider_id"].as_str())
    }

    /// Prefijo con el que el proveedor invoca una skill: `/` en Claude, `$` en Codex.
    pub fn skill_prefix(&self) -> Option<char> {
        match self.chat_provider()? {
            "anthropic" => Some('/'),
            "openai" => Some('$'),
            _ => None,
        }
    }

    /// Skills que coinciden con lo que se está escribiendo (`/fl…`, `$de…`), primero las que
    /// empiezan igual. Vacío si no se escribe una invocación, si ya hay un espacio (ya se
    /// eligió) o si se cerró el desplegable con Esc.
    pub fn skill_matches(&self) -> Vec<&Value> {
        let input = self.chat_input.as_str();
        let Some(prefix) = self.skill_prefix() else {
            return Vec::new();
        };
        let Some(token) = input.strip_prefix(prefix) else {
            return Vec::new();
        };
        if token.contains(char::is_whitespace) || self.skill_dismissed.as_deref() == Some(input) {
            return Vec::new();
        }
        let token = token.to_lowercase();
        let Some(catalog) = self.chat_provider().and_then(|p| self.skills.get(p)) else {
            return Vec::new();
        };
        let name = |s: &Value| s["name"].as_str().unwrap_or("").to_lowercase();
        let mut found: Vec<&Value> = catalog
            .iter()
            .filter(|s| name(s).contains(&token))
            .collect();
        found.sort_by_key(|s| !name(s).starts_with(&token));
        found
    }

    /// Lo último que dijo el agente en el chat (para copiarlo).
    fn last_reply(&self) -> Option<String> {
        self.chat
            .as_ref()?
            .messages
            .iter()
            .rev()
            .find(|m| m["role"] == "ASSISTANT")
            .and_then(|m| m["content"].as_str())
            .map(str::to_string)
    }

    fn chat_key(&mut self, key: KeyEvent) -> Vec<Call> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if key.code == KeyCode::Char('e') {
                return self
                    .chat
                    .as_ref()
                    .map(|c| call(Req::Export, "agent.export", json!({ "agent": c.id })))
                    .into_iter()
                    .collect();
            }
            if key.code == KeyCode::Char('y') {
                match self.last_reply() {
                    Some(t) => self.copy = Some(t),
                    None => self.error("Todavía no hay una respuesta del agente que copiar."),
                }
            }
            return Vec::new();
        }
        let matches = self.skill_matches();
        if !matches.is_empty() {
            let (last, name) = (
                matches.len() - 1,
                matches[self.skill_sel.min(matches.len() - 1)]["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            );
            match key.code {
                KeyCode::Up => {
                    self.skill_sel = self.skill_sel.min(last).saturating_sub(1);
                    return Vec::new();
                }
                KeyCode::Down => {
                    self.skill_sel = (self.skill_sel + 1).min(last);
                    return Vec::new();
                }
                KeyCode::Right => {
                    let prefix = self.skill_prefix().unwrap_or('/');
                    self.chat_input = format!("{prefix}{name} ");
                    self.skill_sel = 0;
                    return Vec::new();
                }
                KeyCode::Esc => {
                    self.skill_dismissed = Some(self.chat_input.clone());
                    return Vec::new();
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::F(2) => {
                self.mouse = !self.mouse;
                self.info(if self.mouse {
                    "Rueda del ratón activada."
                } else {
                    "Modo selección: selecciona y copia con el ratón; la rueda no desplaza (F2 para volver)."
                });
            }
            KeyCode::Esc => return self.go(Screen::Home),
            KeyCode::Enter => return self.chat_send(),
            KeyCode::Tab | KeyCode::BackTab => {
                let models = self.available_models();
                if !models.is_empty() {
                    let at = self
                        .chat_model
                        .as_ref()
                        .and_then(|m| models.iter().position(|x| x == m));
                    let next = match at {
                        Some(i) => cycle(i, models.len(), key.code == KeyCode::Tab),
                        None => 0,
                    };
                    self.chat_model = models.get(next).cloned();
                }
            }
            KeyCode::Backspace => {
                self.chat_input.pop();
                self.skill_sel = 0;
            }
            KeyCode::Char(c) => {
                self.chat_input.push(c);
                self.skill_sel = 0;
            }
            KeyCode::Up => self.scroll_chat(1),
            KeyCode::Down => self.scroll_chat(-1),
            KeyCode::PageUp => self.scroll_chat(10),
            KeyCode::PageDown => self.scroll_chat(-10),
            _ => {}
        }
        Vec::new()
    }

    /// Enter en el chat: el primer mensaje crea el chat; los siguientes se mandan, y si el
    /// modelo elegido (Tab) no es el del chat, cambian de executor llevando el mensaje.
    fn chat_send(&mut self) -> Vec<Call> {
        let text = self.chat_input.trim().to_string();
        if text.is_empty() {
            return Vec::new();
        }
        let model = self.chat_model.clone();
        let next = match &self.chat {
            None => {
                let Some(model) = model else {
                    self.error("No hay modelos disponibles: habilita un proveedor (Esc, luego p).");
                    return Vec::new();
                };
                call(
                    Req::ChatCreate,
                    "agent.create",
                    json!({
                        "title": text,
                        "project_root": self.root(),
                        "model": model,
                        "failover": "ANY",
                        "chat": true,
                    }),
                )
            }
            Some(chat) => match model.filter(|m| Some(m.as_str()) != chat.model()) {
                Some(m) => call(
                    Req::Switch,
                    "agent.switch",
                    json!({ "agent": chat.id, "model": m, "message": text }),
                ),
                None => call(
                    Req::Send,
                    "agent.send",
                    json!({ "agent": chat.id, "text": text }),
                ),
            },
        };
        self.chat_input.clear();
        self.chat_scroll = 0;
        vec![next]
    }

    fn home_key(&mut self, key: KeyEvent) -> Vec<Call> {
        if self.move_selection(key.code) {
            return Vec::new();
        }
        match key.code {
            KeyCode::Enter => match self.home_order().get(self.selected) {
                Some(a) => {
                    let id = a["agent_id"].as_str().unwrap_or_default().to_string();
                    self.open_agent(id)
                }
                None => self.new_agent(),
            },
            KeyCode::Char('n') => self.new_agent(),
            KeyCode::Esc => self.go(Screen::Chat),
            KeyCode::Char('p') => self.go(Screen::Providers),
            KeyCode::Char('r') => self.go(Screen::Recovery),
            KeyCode::Char(':') | KeyCode::Char('/') => {
                self.command = Some(String::new());
                Vec::new()
            }
            KeyCode::Char('q') => {
                self.quit = true;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn new_agent(&mut self) -> Vec<Call> {
        self.screen = Screen::NewAgent;
        Vec::new()
    }

    fn command_key(&mut self, key: KeyEvent) -> Vec<Call> {
        let Some(text) = &mut self.command else {
            return Vec::new();
        };
        match key.code {
            KeyCode::Esc => self.command = None,
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Char(c) => text.push(c),
            KeyCode::Enter => {
                let line = text.trim().trim_start_matches(['/', ':']).to_string();
                self.command = None;
                return self.run_command(&line);
            }
            _ => {}
        }
        Vec::new()
    }

    /// Barra de comandos de Home: `spawn [proveedor/modelo] <tarea>`, `agent <n>`,
    /// `new`, `providers`, `recovery`, `quit`.
    fn run_command(&mut self, line: &str) -> Vec<Call> {
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        match cmd {
            "spawn" if !rest.is_empty() => {
                let (model, task) = match rest.split_once(' ') {
                    Some((m, t)) if looks_like_model(m) && !t.trim().is_empty() => {
                        (Some(m), t.trim())
                    }
                    _ => (None, rest),
                };
                self.new_agent.task = task.to_string();
                self.new_agent.exec = if model.is_some() {
                    Exec::Exact
                } else {
                    Exec::Later
                };
                self.new_agent.model = model.map(str::to_string);
                self.create()
            }
            "explain-route" | "explain" => self.open_explain(),
            "agent" | "a" => {
                let n = rest.trim_start_matches('#');
                match self
                    .agents
                    .iter()
                    .find(|a| n.parse().ok() == a["number"].as_i64())
                {
                    Some(a) => {
                        let id = a["agent_id"].as_str().unwrap_or_default().to_string();
                        self.open_agent(id)
                    }
                    None => {
                        self.error(format!("No hay un agente #{n}."));
                        Vec::new()
                    }
                }
            }
            "new" | "spawn" => self.new_agent(),
            "providers" => self.go(Screen::Providers),
            "recovery" => self.go(Screen::Recovery),
            "quit" | "q" => {
                self.quit = true;
                Vec::new()
            }
            "" => Vec::new(),
            other => {
                self.error(format!(
                    "Comando desconocido `{other}`. Usa: spawn [modelo] <tarea> · agent <n> · new · providers · recovery · quit"
                ));
                Vec::new()
            }
        }
    }

    fn create(&mut self) -> Vec<Call> {
        let f = &self.new_agent;
        if f.task.trim().is_empty() {
            self.error("Escribe la tarea del agente.");
            return Vec::new();
        }
        if f.exec == Exec::Exact && f.model.is_none() {
            self.error("Elige un modelo (Enter en «Ejecución») o cambia a «Decidir después».");
            return Vec::new();
        }
        if f.exec == Exec::Profile && f.profile.is_none() {
            self.error("Elige un profile (Enter en «Ejecución») o cambia a «Decidir después».");
            return Vec::new();
        }
        let params = json!({
            "title": f.task.trim(),
            "project_root": self.root(),
            "model": if f.exec == Exec::Exact { f.model.clone() } else { None },
            "profile": if f.exec == Exec::Profile { f.profile.clone() } else { None },
            "failover": FAILOVER[f.failover].0,
            "priority": PRIORITY[f.priority].0,
        });
        self.info("Creando agente…");
        vec![call(Req::Create, "agent.create", params)]
    }

    fn new_agent_key(&mut self, key: KeyEvent) -> Vec<Call> {
        let f = &mut self.new_agent;
        match key.code {
            KeyCode::Esc => return self.go(Screen::Home),
            KeyCode::Up | KeyCode::BackTab => f.field = cycle(f.field, 5, false),
            KeyCode::Down | KeyCode::Tab => f.field = cycle(f.field, 5, true),
            KeyCode::Left | KeyCode::Right => {
                let fwd = key.code == KeyCode::Right;
                match f.field {
                    1 => f.exec = f.exec.cycle(fwd),
                    2 => f.failover = cycle(f.failover, FAILOVER.len(), fwd),
                    3 => f.priority = cycle(f.priority, PRIORITY.len(), fwd),
                    _ => {}
                }
            }
            KeyCode::Backspace if f.field == 0 => {
                f.task.pop();
            }
            KeyCode::Char(c) if f.field == 0 => f.task.push(c),
            KeyCode::Enter if f.field == 1 && f.exec != Exec::Later => {
                self.pick_for = PickFor::NewAgent;
                return self.go(Screen::ModelPicker);
            }
            KeyCode::Enter if f.field < 4 => f.field += 1,
            KeyCode::Enter => return self.create(),
            _ => {}
        }
        Vec::new()
    }

    /// 14 Explain Route: solo lectura; Esc vuelve al agente.
    fn explain_key(&mut self, key: KeyEvent) -> Vec<Call> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
                self.screen = Screen::Agent;
                self.refresh()
            }
            _ => Vec::new(),
        }
    }

    /// `/explain-route`: abre la explicación de la decisión de routing del agente abierto.
    fn open_explain(&mut self) -> Vec<Call> {
        if self.agent.is_none() {
            self.error("Abre un agente para ver por qué se eligió su modelo.");
            return Vec::new();
        }
        self.explain = None;
        self.screen = Screen::ExplainRoute;
        self.refresh()
    }

    fn picker_key(&mut self, key: KeyEvent) -> Vec<Call> {
        if self.move_selection(key.code) {
            return Vec::new();
        }
        let back = match self.pick_for {
            PickFor::NewAgent => Screen::NewAgent,
            PickFor::Switch => Screen::Agent,
        };
        match key.code {
            KeyCode::Esc => {
                self.screen = back;
                if back == Screen::Agent {
                    return self.refresh();
                }
                Vec::new()
            }
            KeyCode::Enter if self.selected < self.profiles.len() => {
                // FLOW §8.3: un profile → Symphony elige entre los modelos utilizables.
                let profile = self.profiles[self.selected]["id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                match self.pick_for {
                    PickFor::NewAgent => {
                        self.new_agent.exec = Exec::Profile;
                        self.new_agent.profile = Some(profile);
                        self.new_agent.field = 2;
                        self.screen = Screen::NewAgent;
                        Vec::new()
                    }
                    PickFor::Switch => {
                        self.screen = Screen::Agent;
                        self.info(format!("Eligiendo el modelo para {profile}…"));
                        self.agent_call(Req::Switch, "agent.switch")
                            .map(|mut c| {
                                c.params["profile"] = json!(profile);
                                c
                            })
                            .into_iter()
                            .collect()
                    }
                }
            }
            KeyCode::Enter => {
                let Some(m) = self
                    .models
                    .get(self.selected - self.profiles.len())
                    .cloned()
                else {
                    return Vec::new();
                };
                let id = m["id"].as_str().unwrap_or_default().to_string();
                // FLOW §8.2: un modelo exacto no disponible nunca se sustituye en silencio.
                if !m["available"].as_bool().unwrap_or(false) {
                    self.error(format!(
                        "{id} no está disponible ({}). Elige otro, espera o Esc para cancelar.",
                        m["setup_state"].as_str().unwrap_or("?")
                    ));
                    return Vec::new();
                }
                match self.pick_for {
                    PickFor::NewAgent => {
                        self.new_agent.exec = Exec::Exact;
                        self.new_agent.model = Some(id);
                        self.new_agent.field = 2;
                        self.screen = Screen::NewAgent;
                        Vec::new()
                    }
                    PickFor::Switch => {
                        self.screen = Screen::Agent;
                        self.info(format!("Cambiando executor a {id}…"));
                        self.agent_call(Req::Switch, "agent.switch")
                            .map(|mut c| {
                                c.params["model"] = json!(id);
                                c
                            })
                            .into_iter()
                            .collect()
                    }
                }
            }
            _ => Vec::new(),
        }
    }

    fn agent_key(&mut self, key: KeyEvent) -> Vec<Call> {
        let Some(a) = &mut self.agent else {
            return self.go(Screen::Home);
        };
        if a.typing {
            match key.code {
                KeyCode::Esc => {
                    a.typing = false;
                    a.input.clear();
                }
                KeyCode::Backspace => {
                    a.input.pop();
                }
                KeyCode::Char(c) => a.input.push(c),
                KeyCode::Enter if !a.input.trim().is_empty() => {
                    let text = std::mem::take(&mut a.input);
                    a.typing = false;
                    return vec![call(
                        Req::Send,
                        "agent.send",
                        json!({ "agent": a.id, "text": text.trim() }),
                    )];
                }
                _ => {}
            }
            return Vec::new();
        }
        let confirm = std::mem::replace(&mut a.confirm_stop, false);
        let switch_tab = |a: &mut AgentView, tab: Tab| {
            a.tab = tab;
            a.scroll = 0;
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.agent = None;
                return self.go(Screen::Home);
            }
            KeyCode::Right | KeyCode::Tab => {
                let next = Tab::ALL[cycle(a.tab.index(), Tab::ALL.len(), true)];
                switch_tab(a, next);
            }
            KeyCode::Left | KeyCode::BackTab => {
                let next = Tab::ALL[cycle(a.tab.index(), Tab::ALL.len(), false)];
                switch_tab(a, next);
            }
            KeyCode::Char(c @ '1'..='5') => {
                let i = usize::from(c as u8 - b'1');
                switch_tab(a, Tab::ALL[i]);
            }
            KeyCode::Char('d') => switch_tab(a, Tab::Changes),
            KeyCode::Up | KeyCode::Char('k') => {
                scroll_view(a, -1);
                return Vec::new();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                scroll_view(a, 1);
                return Vec::new();
            }
            KeyCode::PageUp => {
                scroll_view(a, -10);
                return Vec::new();
            }
            KeyCode::PageDown => {
                scroll_view(a, 10);
                return Vec::new();
            }
            KeyCode::Char('?') => return self.open_explain(),
            KeyCode::Char('e') => {
                return vec![call(Req::Export, "agent.export", json!({ "agent": a.id }))];
            }
            KeyCode::Char('m') => {
                a.typing = true;
                switch_tab(a, Tab::Conversation);
            }
            KeyCode::Char('p') => {
                let method = if a.state() == "PAUSED" {
                    "agent.resume"
                } else {
                    "agent.pause"
                };
                return vec![call(Req::Control, method, json!({ "agent": a.id }))];
            }
            KeyCode::Char('s') => {
                self.pick_for = PickFor::Switch;
                return self.go(Screen::ModelPicker);
            }
            KeyCode::Char('o') => {
                let params = json!({ "agent": a.id });
                self.info("Abriendo la sesión en el CLI oficial…");
                return vec![call(Req::Attach, "agent.attach", params)];
            }
            KeyCode::Char('x') if confirm => {
                return vec![call(Req::Control, "agent.stop", json!({ "agent": a.id }))];
            }
            KeyCode::Char('x') => {
                a.confirm_stop = true;
                self.info("Pulsa x otra vez para detener el agente (el workspace se conserva).");
                return Vec::new();
            }
            _ => return Vec::new(),
        }
        self.tab_call().into_iter().collect()
    }

    fn recovery_key(&mut self, key: KeyEvent) -> Vec<Call> {
        if self.move_selection(key.code) {
            return Vec::new();
        }
        let item = self.recovery.get(self.selected).cloned();
        let act = |action: &str| -> Vec<Call> {
            item.as_ref()
                .map(|i| {
                    call(
                        Req::RecoveryAct,
                        "recovery.act",
                        json!({ "id": i["id"], "action": action }),
                    )
                })
                .into_iter()
                .collect()
        };
        match key.code {
            KeyCode::Char('r') => act("restart"),
            KeyCode::Char('c') => act("reclaim"),
            KeyCode::Char('s') => act("stop"),
            KeyCode::Char('x') => act("dismiss"),
            KeyCode::Enter => match item.as_ref().and_then(|i| i["agent_id"].as_str()) {
                Some(id) => self.open_agent(id.to_string()),
                None => Vec::new(),
            },
            KeyCode::Esc | KeyCode::Char('q') => self.go(Screen::Home),
            _ => Vec::new(),
        }
    }
}

/// `proveedor/modelo` y no una ruta como `src/main.rs` (extensión alfabética).
fn looks_like_model(word: &str) -> bool {
    let Some((_, m)) = word.split_once('/') else {
        return false;
    };
    let ext_like = m
        .rsplit_once('.')
        .is_some_and(|(_, e)| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphabetic()));
    !m.is_empty() && !m.contains('/') && !ext_like
}

fn is_finished(a: &Value) -> bool {
    matches!(a["state"].as_str(), Some("COMPLETED" | "CANCELLED"))
}

/// Frase humana de cada estado (FLOW §7, regla UX 1).
pub fn state_phrase(state: &str) -> &'static str {
    match state {
        "CREATED" => "creándose",
        "READY" => "listo, sin executor asignado",
        "RUNNING" => "trabajando",
        "WAITING_PROVIDER" => "esperando un proveedor disponible",
        "WAITING_RESOURCE" => "esperando recursos de la máquina",
        "WAITING_DEPENDENCY" => "esperando otra tarea",
        "TESTING" => "corriendo validaciones",
        "BLOCKED" => "bloqueado: necesita tu decisión",
        "PAUSED" => "pausado",
        "COMPLETED" => "terminó su tarea",
        "FAILED" => "falló; su trabajo quedó guardado",
        "CANCELLED" => "detenido",
        _ => "estado desconocido",
    }
}

/// `14 s`, `3 min`, `2 h`: edad relativa (sin falsa precisión).
pub fn ago(now_ms: i64, at_ms: i64) -> String {
    let s = (now_ms - at_ms).max(0) / 1000;
    match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} min", s / 60),
        3600..86_400 => format!("{} h", s / 3600),
        _ => format!("{} d", s / 86_400),
    }
}
