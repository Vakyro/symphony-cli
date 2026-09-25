//! Render de las vistas (FLOW §18: 01–09, 12, 13, 22, 24, 30). Funciones puras de `&App`.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs, Wrap};
use serde_json::Value;

use crate::app::{
    App, FAILOVER, Notice, PERFORMANCE, PRIORITY, PickFor, Screen, Tab, ago, state_phrase,
};

const ACCENT: Color = Color::Cyan;

fn bold(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().add_modifier(Modifier::BOLD))
}

fn dim(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(Color::DarkGray))
}

fn colored(s: impl Into<String>, c: Color) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(c))
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or("—").to_string()
}

/// Un número JSON como texto (el `Display` de `Value` ignora el ancho de `format!`).
fn num(v: &Value) -> String {
    v.as_i64()
        .map_or_else(|| "—".to_string(), |n| n.to_string())
}

fn state_color(state: &str) -> Color {
    match state {
        "RUNNING" | "TESTING" | "READY" | "COMPLETED" => Color::Green,
        "FAILED" | "BLOCKED" | "ERROR" | "NOT_FOUND" => Color::Red,
        "CANCELLED" => Color::DarkGray,
        _ => Color::Yellow,
    }
}

/// Marca de la fila elegida en una lista.
fn marker(selected: bool) -> Span<'static> {
    if selected {
        colored("▸ ", ACCENT)
    } else {
        Span::raw("  ")
    }
}

fn section(title: &str) -> Line<'static> {
    Line::from(colored(title.to_string(), ACCENT).add_modifier(Modifier::BOLD))
}

pub fn render(app: &App, f: &mut Frame) {
    let bottom = if app.command.is_some() { 2 } else { 1 };
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(bottom),
    ])
    .areas(f.area());

    let title = match app.screen {
        Screen::Launch | Screen::NotARepo => "",
        Screen::FirstRun => "· Configuración inicial",
        Screen::ProviderSetup => "· Proveedores",
        Screen::Home => "",
        Screen::NewAgent => "· Nuevo agente",
        Screen::ModelPicker => "· Elegir ejecución",
        Screen::Agent => "· Agente",
        Screen::Providers => "· Proveedores",
        Screen::Recovery => "· Recovery Center",
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            colored(" SYMPHONY ", ACCENT).add_modifier(Modifier::BOLD),
            bold(app.project_name().to_string()),
            Span::raw(" "),
            dim(title),
        ])),
        header,
    );

    match app.screen {
        Screen::Launch => launch(app, f, body),
        Screen::NotARepo => not_a_repo(app, f, body),
        Screen::FirstRun => first_run(app, f, body),
        Screen::ProviderSetup | Screen::Providers => providers(app, f, body),
        Screen::Home => home(app, f, body),
        Screen::NewAgent => new_agent(app, f, body),
        Screen::ModelPicker => model_picker(app, f, body),
        Screen::Agent => agent(app, f, body),
        Screen::Recovery => recovery(app, f, body),
    }

    footer_line(app, f, footer);
}

fn hints(app: &App) -> &'static str {
    match app.screen {
        Screen::Launch if app.loaded() && !app.initialized() => {
            "i inicializar · o abrir sin configurar · q salir"
        }
        Screen::Launch | Screen::NotARepo => "q salir",
        Screen::FirstRun => "↑↓ campo · ←→ opción · Enter siguiente · Esc volver",
        Screen::ProviderSetup => {
            "↑↓ elegir · r reintentar · d activar/desactivar · Enter continuar"
        }
        Screen::Providers => "↑↓ elegir · r volver a detectar · d activar/desactivar · Esc volver",
        Screen::Home => {
            "↑↓ elegir · Enter abrir · n nuevo · p proveedores · r recovery · : comando · q salir"
        }
        Screen::NewAgent => "↑↓ campo · ←→ opción · Enter siguiente/crear · Esc cancelar",
        Screen::ModelPicker => "↑↓ elegir · Enter usar · Esc volver",
        Screen::Agent => match app.agent.as_ref() {
            Some(a) if a.typing => "Enter enviar · Esc cancelar",
            _ => {
                "←→ pestaña · m mensaje · p pausa · s cambiar modelo · d diff · x detener · Esc volver"
            }
        },
        Screen::Recovery => {
            "↑↓ elegir · r reiniciar · c recuperar · s detener · x descartar · Enter abrir · Esc volver"
        }
    }
}

fn footer_line(app: &App, f: &mut Frame, area: Rect) {
    let mut lines = Vec::new();
    if let Some(cmd) = &app.command {
        lines.push(Line::from(vec![
            colored("> ", ACCENT),
            Span::raw(cmd.clone()),
            Span::raw("▏"),
        ]));
    }
    lines.push(match &app.notice {
        Some(Notice::Error(e)) => Line::from(colored(format!(" {e}"), Color::Red)),
        Some(Notice::Info(i)) => Line::from(colored(format!(" {i}"), Color::Green)),
        None => Line::from(dim(format!(" {}", hints(app)))),
    });
    f.render_widget(Paragraph::new(lines), area);
}

fn boxed(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().add_modifier(Modifier::BOLD),
        ))
}

fn paragraph(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>) {
    f.render_widget(
        Paragraph::new(lines)
            .block(boxed(title))
            .wrap(Wrap { trim: false }),
        area,
    );
}

// --- 01 Launch --------------------------------------------------------------

fn launch(app: &App, f: &mut Frame, area: Rect) {
    let check = |done: bool| {
        if done {
            colored("listo", Color::Green)
        } else {
            dim("…")
        }
    };
    let mut lines = vec![
        Line::from(vec![
            Span::raw("Proyecto:  "),
            bold(if app.loaded() {
                app.project_name().to_string()
            } else {
                app.cwd.clone()
            }),
        ]),
        Line::from(vec![
            Span::raw("Revisando el estado del proyecto…  "),
            check(app.loaded()),
        ]),
        Line::from(vec![
            Span::raw("Revisando proveedores…             "),
            check(!app.providers.is_empty()),
        ]),
    ];
    if app.loaded() && !app.initialized() {
        lines.push(Line::raw(""));
        lines.push(Line::raw(
            "Symphony todavía no está configurado en este proyecto.",
        ));
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            bold("[i]"),
            Span::raw(" Inicializar    "),
            bold("[o]"),
            Span::raw(" Abrir sin configurar    "),
            bold("[q]"),
            Span::raw(" Salir"),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::raw("Restaurando sesión…                "),
            dim("…"),
        ]));
    }
    paragraph(f, area, "Symphony", lines);
}

fn not_a_repo(app: &App, f: &mut Frame, area: Rect) {
    let path = app.project["path"].as_str().unwrap_or(&app.cwd).to_string();
    paragraph(
        f,
        area,
        "Symphony",
        vec![
            Line::raw("Esta carpeta no parece un repositorio git:"),
            Line::from(bold(format!("  {path}"))),
            Line::raw(""),
            Line::raw("Abre symphony dentro de la carpeta de tu proyecto"),
            Line::raw("(o corre `git init` si es un proyecto nuevo)."),
        ],
    );
}

// --- 02 First-run -----------------------------------------------------------

fn field(label: &str, value: Line<'static>, selected: bool) -> Line<'static> {
    let mut spans = vec![marker(selected), bold(format!("{label:<14}"))];
    spans.extend(value.spans);
    Line::from(spans)
}

/// Texto editable: el cursor solo se ve en el campo activo.
fn cursor(text: &str, active: bool) -> String {
    if active {
        format!("{text}▏")
    } else {
        text.to_string()
    }
}

fn choice(options: &[&str], chosen: usize) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, o) in options.iter().enumerate() {
        if i == chosen {
            spans.push(colored(format!("(•) {o}  "), ACCENT));
        } else {
            spans.push(dim(format!("( ) {o}  ")));
        }
    }
    Line::from(spans)
}

fn first_run(app: &App, f: &mut Frame, area: Rect) {
    let fr = &app.first_run;
    let ready = app
        .providers
        .iter()
        .filter(|p| p["setup_state"] == "READY")
        .count();
    let mut lines = vec![
        Line::from(dim(format!("Ruta: {}", app.root()))),
        Line::raw(""),
        field(
            "Proyecto",
            Line::raw(cursor(&fr.name, fr.field == 0)),
            fr.field == 0,
        ),
        field(
            "Rendimiento",
            choice(&PERFORMANCE.map(|p| p.1), fr.performance),
            fr.field == 1,
        ),
        field(
            "Failover",
            choice(&FAILOVER.map(|p| p.1), fr.failover),
            fr.field == 2,
        ),
        Line::raw(""),
        section("PROVEEDORES DETECTADOS"),
    ];
    for p in &app.providers {
        let state = text(&p["setup_state"]);
        lines.push(Line::from(vec![
            Span::raw(format!("  {:<12}", text(&p["display_name"]))),
            colored(state.clone(), state_color(&state)),
        ]));
    }
    if ready == 0 {
        lines.push(Line::from(colored(
            "  Ningún proveedor listo: podrás configurarlos en el siguiente paso.",
            Color::Yellow,
        )));
    }
    lines.push(Line::raw(""));
    lines.push(field(
        "",
        Line::from(if fr.field == 3 {
            colored("[ Entrar a Symphony ]", ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Span::raw("[ Entrar a Symphony ]")
        }),
        fr.field == 3,
    ));
    paragraph(f, area, "Configuración inicial", lines);
}

// --- 03 Provider Setup / 22 Providers ---------------------------------------

/// FLOW §4.3: mensaje y acciones por estado.
fn provider_advice(state: &str, name: &str) -> String {
    match state {
        "READY" => format!("{name} está listo para usarse."),
        "LOGIN_REQUIRED" => format!(
            "{name} está instalado pero sin sesión. Inicia sesión con su CLI oficial y pulsa r; o d para saltarlo."
        ),
        "NOT_FOUND" => format!(
            "{name} no está instalado en esta máquina. Instálalo y pulsa r, o ignóralo: puedes empezar con un solo proveedor."
        ),
        _ => format!("No se pudo verificar {name}. Pulsa r para reintentar o d para desactivarlo."),
    }
}

fn providers(app: &App, f: &mut Frame, area: Rect) {
    let mut lines = vec![Line::from(dim(format!(
        "  {:<12} {:<16} {:<12} {:>7}",
        "PROVEEDOR", "ESTADO", "VERSIÓN", "MODELOS"
    )))];
    for (i, p) in app.providers.iter().enumerate() {
        let enabled = p["enabled"].as_bool().unwrap_or(true);
        let state = text(&p["setup_state"]);
        let shown = if enabled {
            state.clone()
        } else {
            "DESACTIVADO".into()
        };
        lines.push(Line::from(vec![
            marker(i == app.selected),
            Span::raw(format!("{:<12} ", text(&p["display_name"]))),
            colored(
                format!("{shown:<16} "),
                if enabled {
                    state_color(&state)
                } else {
                    Color::DarkGray
                },
            ),
            Span::raw(format!(
                "{:<12} {:>7}",
                text(&p["cli_version"]),
                num(&p["models"])
            )),
        ]));
    }
    if app.providers.is_empty() {
        lines.push(Line::from(dim(
            "  Todavía no hay proveedores detectados. Pulsa r.",
        )));
    }
    if let Some(p) = app.providers.get(app.selected) {
        lines.push(Line::raw(""));
        lines.push(Line::raw(provider_advice(
            p["setup_state"].as_str().unwrap_or(""),
            p["display_name"].as_str().unwrap_or("El proveedor"),
        )));
        if let Some(path) = p["cli_path"].as_str() {
            lines.push(Line::from(dim(format!("Ruta: {path}"))));
        }
    }
    if app.screen == Screen::ProviderSetup {
        lines.push(Line::raw(""));
        lines.push(Line::from(dim(
            "No hace falta configurar todos: puedes empezar con uno y agregar otros después.",
        )));
    }
    let title = if app.screen == Screen::ProviderSetup {
        "Configurar proveedores"
    } else {
        "Proveedores"
    };
    paragraph(f, area, title, lines);
}

// --- 04 Home ----------------------------------------------------------------

fn agent_row(a: &Value, selected: bool) -> Line<'static> {
    let state = text(&a["state"]);
    let model = a["model_id"].as_str().unwrap_or("—").to_string();
    Line::from(vec![
        marker(selected),
        bold(format!("#{:<3}", num(&a["number"]))),
        colored(format!("{state:<19}"), state_color(&state)),
        Span::raw(format!("{model:<18} ")),
        Span::raw(text(&a["task_title"])),
    ])
}

fn home(app: &App, f: &mut Frame, area: Rect) {
    let mut lines = Vec::new();
    // Banner de problema crítico (FLOW §5): visible pero no intrusivo.
    let recovery = app.recovery.len();
    if recovery > 0 {
        lines.push(Line::from(colored(
            format!(
                "⚠ {recovery} problema(s) por recuperar: pulsa r para abrir el Recovery Center."
            ),
            Color::Yellow,
        )));
    }
    let attention = app.providers_needing_attention();
    if attention > 0 {
        lines.push(Line::from(colored(
            format!("⚠ {attention} proveedor(es) necesitan atención: pulsa p."),
            Color::Yellow,
        )));
    }
    if !app.initialized() && app.loaded() {
        lines.push(Line::from(dim(
            "Proyecto abierto sin configurar: se usan los valores por defecto.",
        )));
    }
    if !lines.is_empty() {
        lines.push(Line::raw(""));
    }

    lines.push(section("AGENTES"));
    let active = app.active_agents();
    let finished = app.finished_agents();
    if active.is_empty() && finished.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::raw("  Todavía no hay agentes. "),
            bold("Pulsa n para crear el primero."),
        ]));
        lines.push(Line::from(dim(
            "  También: p proveedores · r recovery · : comandos",
        )));
    }
    for (i, a) in active.iter().enumerate() {
        lines.push(agent_row(a, i == app.selected));
        let state = a["state"].as_str().unwrap_or("");
        if !matches!(state, "RUNNING" | "READY") {
            let why = a["state_reason"]
                .as_str()
                .map_or_else(|| state_phrase(state).to_string(), str::to_string);
            lines.push(Line::from(dim(format!("      {why}"))));
        }
    }
    if !finished.is_empty() {
        lines.push(Line::raw(""));
        lines.push(section("COMPLETADOS"));
        for (i, a) in finished.iter().enumerate() {
            lines.push(agent_row(a, active.len() + i == app.selected));
        }
    }

    lines.push(Line::raw(""));
    lines.push(section("PROVEEDORES"));
    let mut strip = vec![Span::raw("  ")];
    for p in &app.providers {
        let enabled = p["enabled"].as_bool().unwrap_or(true);
        let state = if enabled {
            text(&p["setup_state"])
        } else {
            "DESACTIVADO".into()
        };
        strip.push(Span::raw(format!("{} ", text(&p["display_name"]))));
        strip.push(colored(format!("{state}   "), state_color(&state)));
    }
    lines.push(Line::from(strip));

    lines.push(Line::raw(""));
    lines.push(section("SISTEMA"));
    lines.push(Line::from(dim(
        "  CPU, RAM y operaciones pesadas: se muestran cuando llegue el scheduler (v0.5).",
    )));

    paragraph(f, area, "Inicio", lines);
}

// --- 05 New Agent / 13 Model Picker -----------------------------------------

fn new_agent(app: &App, f: &mut Frame, area: Rect) {
    let na = &app.new_agent;
    let exec = if na.exact {
        let model = na
            .model
            .clone()
            .unwrap_or_else(|| "Enter para elegir".into());
        Line::from(vec![
            colored("(•) Modelo exacto: ", ACCENT),
            bold(model),
            dim("   ( ) Decidir después"),
        ])
    } else {
        Line::from(vec![
            dim("( ) Modelo exacto   "),
            colored("(•) Decidir después", ACCENT),
        ])
    };
    let lines = vec![
        field(
            "Tarea",
            Line::raw(cursor(&na.task, na.field == 0)),
            na.field == 0,
        ),
        Line::raw(""),
        field("Ejecución", exec, na.field == 1),
        Line::from(dim(
            "                  Profiles (@code, @fast…) llegan en v0.5.",
        )),
        field(
            "Failover",
            choice(&FAILOVER.map(|p| p.1), na.failover),
            na.field == 2,
        ),
        field(
            "Prioridad",
            choice(&PRIORITY.map(|p| p.1), na.priority),
            na.field == 3,
        ),
        Line::raw(""),
        field(
            "",
            Line::from(if na.field == 4 {
                colored("[ Crear agente ]", ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Span::raw("[ Crear agente ]")
            }),
            na.field == 4,
        ),
    ];
    paragraph(f, area, "Nuevo agente", lines);
}

fn model_picker(app: &App, f: &mut Frame, area: Rect) {
    let mut lines = vec![
        section("PROFILES"),
        Line::from(dim(
            "  @code @debug @fast @reasoning @docs @conserve — llegan en v0.5",
        )),
        Line::raw(""),
        section("MODELOS EXACTOS"),
    ];
    for (i, m) in app.models.iter().enumerate() {
        let available = m["available"].as_bool().unwrap_or(false);
        let status = if available {
            "DISPONIBLE".to_string()
        } else {
            text(&m["setup_state"])
        };
        lines.push(Line::from(vec![
            marker(i == app.selected),
            Span::raw(format!("{:<12} / ", text(&m["provider"]))),
            Span::raw(format!("{:<20} ", text(&m["display_name"]))),
            colored(
                status,
                if available {
                    Color::Green
                } else {
                    Color::Yellow
                },
            ),
        ]));
    }
    if app.models.is_empty() {
        lines.push(Line::from(dim(
            "  No hay modelos: ningún proveedor está listo (p en Inicio).",
        )));
    }
    if app.pick_for == PickFor::Switch {
        lines.push(Line::raw(""));
        lines.push(Line::from(dim(
            "Cambia el executor; el agente, la tarea y el workspace se conservan.",
        )));
    }
    paragraph(f, area, "Elegir ejecución", lines);
}

// --- 06–09, 12 Agent --------------------------------------------------------

fn agent(app: &App, f: &mut Frame, area: Rect) {
    let Some(a) = &app.agent else {
        return;
    };
    let ins = &a.inspect;
    let state = a.state().to_string();
    let reason = ins["agent"]["state_reason"]
        .as_str()
        .map_or_else(|| state_phrase(&state).to_string(), str::to_string);
    let run = &ins["current_run"];
    let last = ins["runs"].as_array().and_then(|r| r.last());
    let executor = match (run["model_id"].as_str(), last) {
        (Some(m), _) => format!("{m} (run #{})", num(&run["seq"])),
        // Sin run abierto: el último executor que tuvo, si tuvo alguno.
        (None, Some(r)) => format!(
            "{} (run #{} terminado)",
            text(&r["model_id"]),
            num(&r["seq"])
        ),
        (None, None) => "sin executor".to_string(),
    };
    let checkpoint = ins["latest_checkpoint"]["created_at"].as_i64().map_or_else(
        || "—".to_string(),
        |at| format!("hace {}", ago(app.now_ms, at)),
    );
    let head = vec![
        Line::from(vec![
            bold(format!("AGENTE #{}  ", ins["agent"]["number"])),
            colored(state.clone(), state_color(&state)).add_modifier(Modifier::BOLD),
            dim(format!("  {reason}")),
        ]),
        Line::from(vec![
            dim("Tarea:      "),
            Span::raw(text(&ins["task"]["title"])),
        ]),
        Line::from(vec![dim("Executor:   "), Span::raw(executor)]),
        Line::from(vec![
            dim("Failover:   "),
            Span::raw(text(&ins["agent"]["failover_policy"])),
            dim("   Workspace: "),
            Span::raw(text(&ins["worktree"]["branch"])),
            dim("   Checkpoint: "),
            Span::raw(checkpoint),
        ]),
    ];
    let [top, tabs, body] = Layout::vertical([
        Constraint::Length(head.len() as u16 + 2),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(area);
    // Sin wrap: una tarea larga se recorta en vez de empujar fuera las otras líneas.
    f.render_widget(Paragraph::new(head).block(boxed("Agente")), top);
    f.render_widget(
        Tabs::new(
            Tab::ALL
                .iter()
                .enumerate()
                .map(|(i, t)| format!("{} {}", i + 1, t.title())),
        )
        .select(Tab::ALL.iter().position(|t| *t == a.tab))
        .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        tabs,
    );
    match a.tab {
        Tab::Overview => overview(app, f, body),
        Tab::Conversation => conversation(app, f, body),
        Tab::Activity => activity(app, f, body),
        Tab::Changes => changes(app, f, body),
        Tab::History => history(app, f, body),
    }
}

fn overview(app: &App, f: &mut Frame, area: Rect) {
    let Some(a) = &app.agent else { return };
    let cp = &a.inspect["latest_checkpoint"];
    let mut lines = Vec::new();
    // «Decidir después»: el agente espera a que le asignes un executor.
    if a.state() == "READY" && a.inspect["runs_count"].as_i64() == Some(0) {
        lines.push(Line::from(colored(
            "Este agente todavía no tiene executor: pulsa s para elegir un modelo y arrancarlo.",
            Color::Yellow,
        )));
        lines.push(Line::raw(""));
    }
    lines.extend([
        section("AHORA"),
        Line::raw(format!("  {}", cp["current_step"].as_str().unwrap_or("—"))),
        section("SIGUIENTE"),
        Line::raw(format!("  {}", cp["next_step"].as_str().unwrap_or("—"))),
    ]);
    if let Some(plan) = cp["plan_tail"].as_str().filter(|p| !p.trim().is_empty()) {
        lines.push(section("ÚLTIMO PLAN"));
        let tail: Vec<&str> = plan.lines().rev().take(6).collect();
        for l in tail.into_iter().rev() {
            lines.push(Line::from(dim(format!("  {l}"))));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(dim(format!(
        "Runs: {} · Contexto: {} · Worktree: {}",
        a.inspect["runs_count"],
        text(&a.inspect["agent"]["context_mode"]),
        text(&a.inspect["worktree"]["path"]),
    ))));
    paragraph(f, area, "Resumen", lines);
}

/// Desplazamiento para mostrar el final (lo más nuevo); `scroll` sube desde ahí.
fn from_bottom(total: usize, height: u16, scroll: u16) -> u16 {
    let inner = usize::from(height.saturating_sub(2));
    let max = total.saturating_sub(inner);
    u16::try_from(max.saturating_sub(usize::from(scroll))).unwrap_or(u16::MAX)
}

fn conversation(app: &App, f: &mut Frame, area: Rect) {
    let Some(a) = &app.agent else { return };
    let mut lines = Vec::new();
    for m in &a.messages {
        let content = m["content"].as_str().unwrap_or("");
        match m["role"].as_str().unwrap_or("") {
            // FLOW §13.4: separador visible del cambio de executor.
            "EXECUTOR_CHANGE" => {
                for l in content.lines() {
                    lines.push(Line::from(colored(l.to_string(), Color::Magenta)));
                }
            }
            role => {
                let (who, color) = match role {
                    "USER" => ("tú", ACCENT),
                    "ASSISTANT" => ("agente", Color::Green),
                    _ => ("sistema", Color::DarkGray),
                };
                let mut first = true;
                for l in content.lines() {
                    let prefix = if first {
                        format!("{who:>7} │ ")
                    } else {
                        "        │ ".into()
                    };
                    first = false;
                    lines.push(Line::from(vec![
                        colored(prefix, color),
                        Span::raw(l.to_string()),
                    ]));
                }
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(dim("Sin mensajes todavía.")));
    }
    let area = if a.typing {
        let [msgs, input] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(area);
        f.render_widget(
            Paragraph::new(format!("{}▏", a.input)).block(boxed("Mensaje para el executor")),
            input,
        );
        msgs
    } else {
        area
    };
    let offset = from_bottom(lines.len(), area.height, a.scroll);
    f.render_widget(
        Paragraph::new(lines)
            .block(boxed("Conversación"))
            .scroll((offset, 0)),
        area,
    );
}

fn activity(app: &App, f: &mut Frame, area: Rect) {
    let Some(a) = &app.agent else { return };
    let mut lines = Vec::new();
    for it in &a.activity {
        let at = it["at"].as_i64().unwrap_or(0);
        let when = dim(format!("{:>7}  ", ago(app.now_ms, at)));
        if it["kind"] == "checkpoint" {
            lines.push(Line::from(vec![
                when,
                colored(format!("checkpoint #{}", it["seq"]), Color::Magenta),
                dim(format!("  {}", it["current_step"].as_str().unwrap_or(""))),
            ]));
        } else {
            let status = it["status"].as_str().unwrap_or("");
            let (mark, color) = match status {
                "DONE" => ("✓", Color::Green),
                "FAILED" | "DENIED" => ("✗", Color::Red),
                _ => ("…", Color::Yellow),
            };
            let exit = it["exit_code"]
                .as_i64()
                .map(|c| format!(" (exit {c})"))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                when,
                colored(format!("{mark} "), color),
                bold(format!("{:<8} ", text(&it["tool"]))),
                Span::raw(it["command"].as_str().unwrap_or("").to_string()),
                dim(exit),
            ]));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(dim("Sin actividad todavía.")));
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(boxed("Actividad"))
            .scroll((a.scroll, 0)),
        area,
    );
}

fn changes(app: &App, f: &mut Frame, area: Rect) {
    let Some(a) = &app.agent else { return };
    let mut lines: Vec<Line> = a
        .diff
        .lines()
        .map(|l| {
            let color = if l.starts_with("+++") || l.starts_with("---") {
                Color::White
            } else if l.starts_with('+') {
                Color::Green
            } else if l.starts_with('-') {
                Color::Red
            } else if l.starts_with("@@") {
                ACCENT
            } else {
                Color::Reset
            };
            Line::from(colored(l.to_string(), color))
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::from(dim("Sin cambios contra la base.")));
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(boxed("Cambios"))
            .scroll((a.scroll, 0)),
        area,
    );
}

fn history(app: &App, f: &mut Frame, area: Rect) {
    let Some(a) = &app.agent else { return };
    let h = &a.history;
    let mut lines = vec![section("EXECUTORS")];
    for r in h["runs"].as_array().into_iter().flatten() {
        let status = text(&r["status"]);
        let end = r["end_reason"]
            .as_str()
            .map(|e| format!(" · {e}"))
            .unwrap_or_default();
        lines.push(Line::from(vec![
            bold(format!("  #{:<3}", num(&r["seq"]))),
            Span::raw(format!("{:<20} ", text(&r["model_id"]))),
            colored(status.clone(), state_color(&status)),
            dim(end),
        ]));
    }
    let changes = h["changes"].as_array().cloned().unwrap_or_default();
    if !changes.is_empty() {
        lines.push(Line::raw(""));
        lines.push(section("CAMBIOS DE EXECUTOR"));
    }
    // Vista 24 (Failover Event): anterior, motivo, checkpoint y nuevo executor.
    for c in &changes {
        let why = c["failure"].as_str().map_or_else(
            || text(&c["reason"]),
            |f| format!("{} ({f})", text(&c["reason"])),
        );
        let age = c["checkpoint_age_ms"]
            .as_i64()
            .map(|ms| format!("checkpoint de hace {}", ago(ms, 0)))
            .unwrap_or_else(|| "sin checkpoint".into());
        lines.push(Line::from(colored(
            format!(
                "  ── {} → {}",
                text(&c["from_model"]),
                c["to_model"].as_str().unwrap_or("(esperando proveedor)")
            ),
            Color::Magenta,
        )));
        lines.push(Line::from(dim(format!(
            "     Motivo: {why} · {age} · hace {}",
            ago(app.now_ms, c["at"].as_i64().unwrap_or(0))
        ))));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(dim(
        "Cambia el executor, no el agente: tarea, workspace e historial se conservan.",
    )));
    f.render_widget(
        Paragraph::new(lines)
            .block(boxed("Historial"))
            .scroll((a.scroll, 0)),
        area,
    );
}

// --- 30 Recovery Center -----------------------------------------------------

fn recovery(app: &App, f: &mut Frame, area: Rect) {
    let mut lines = Vec::new();
    if app.recovery.is_empty() {
        lines.push(Line::from(colored("Nada que recuperar.", Color::Green)));
        lines.push(Line::from(dim("Esc para volver al inicio.")));
    }
    for (i, it) in app.recovery.iter().enumerate() {
        let who = it["agent_number"]
            .as_i64()
            .map_or_else(|| "proyecto".to_string(), |n| format!("agente #{n}"));
        lines.push(Line::from(vec![
            marker(i == app.selected),
            colored(format!("{:<20} ", text(&it["kind"])), Color::Yellow),
            bold(format!("{who:<11}")),
            dim(format!(
                " hace {}",
                ago(app.now_ms, it["created_at"].as_i64().unwrap_or(0))
            )),
        ]));
        lines.push(Line::from(dim(format!("    {}", text(&it["detail"])))));
    }
    if !app.recovery.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(dim(
            "El workspace y el último checkpoint se conservan: recuperar no pierde trabajo.",
        )));
    }
    paragraph(f, area, "Recovery Center", lines);
}
