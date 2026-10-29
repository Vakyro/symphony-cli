//! Render de las vistas (FLOW §18: 01–09, 12, 13, 22, 24, 30). Funciones puras de `&App`.

use std::cell::RefCell;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap};
use serde_json::Value;

use crate::app::{
    AgentView, App, Exec, FAILOVER, Notice, PERFORMANCE, PRIORITY, PickFor, Screen, Selection, Tab,
    ago, state_phrase,
};
use crate::theme::Theme;

thread_local! {
    static THEME: RefCell<Option<Theme>> = const { RefCell::new(None) };
}

fn current_theme() -> Theme {
    THEME.with(|c| c.borrow().clone().unwrap_or_default())
}

fn theme_fg(color: Option<Color>) -> Style {
    let mut s = Style::default();
    if let Some(c) = color {
        s = s.fg(c);
    }
    s
}

fn bold(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().add_modifier(Modifier::BOLD))
}

fn dim(s: impl Into<String>) -> Span<'static> {
    let mut st = Style::default();
    if let Some(c) = current_theme().muted {
        st = st.fg(c);
    }
    Span::styled(s.into(), st)
}

fn colored(s: impl Into<String>, c: Option<Color>) -> Span<'static> {
    let mut st = Style::default();
    if let Some(c) = c {
        st = st.fg(c);
    }
    Span::styled(s.into(), st)
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or("—").to_string()
}

/// Un número JSON como texto (el `Display` de `Value` ignora el ancho de `format!`).
fn num(v: &Value) -> String {
    v.as_i64()
        .map_or_else(|| "—".to_string(), |n| n.to_string())
}

fn state_color(state: &str) -> Option<Color> {
    let t = current_theme();
    match state {
        "RUNNING" | "TESTING" | "READY" | "COMPLETED" => t.ok,
        "FAILED" | "BLOCKED" | "ERROR" | "NOT_FOUND" => t.error,
        "CANCELLED" => t.muted,
        _ => t.warn,
    }
}

/// Marca de la fila elegida en una lista.
fn marker(selected: bool) -> Span<'static> {
    if selected {
        colored("▸ ", current_theme().brand)
    } else {
        Span::raw("  ")
    }
}

fn section(title: &str) -> Line<'static> {
    Line::from(colored(title.to_string(), current_theme().brand).add_modifier(Modifier::BOLD))
}

pub fn render(app: &App, f: &mut Frame) {
    THEME.with(|c| *c.borrow_mut() = Some(app.theme.clone()));
    let bottom = if app.command.is_some() { 2 } else { 1 };
    let [header, body, status_bar, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(bottom),
    ])
    .areas(f.area());

    let title = match app.screen {
        Screen::Launch | Screen::NotARepo => "",
        Screen::Chat => "· Chat",
        Screen::FirstRun => "· Configuración inicial",
        Screen::ProviderSetup => "· Proveedores",
        Screen::Home => "",
        Screen::NewAgent => "· Nuevo agente",
        Screen::ModelPicker => "· Elegir ejecución",
        Screen::Agent => "· Agente",
        Screen::ExplainRoute => "· Por qué este modelo",
        Screen::Providers => "· Proveedores",
        Screen::Recovery => "· Recovery Center",
        Screen::ContextOverview => "· Contexto",
        Screen::ContextObject => "· Objeto de contexto",
        Screen::SharedSkills => "· Skills compartidas",
        Screen::SharedMcp => "· Servidores MCP",
        Screen::Stats => "· Uso y contexto",
        Screen::Tasks => "· Tareas",
        Screen::TaskDetail => "· Tarea",
        Screen::Milestones => "· Milestones",
        Screen::System => "· Sistema",
        Screen::PerformanceProfile => "· Perfil de rendimiento",
        Screen::Validation => "· Validación",
        Screen::MergePreflight => "· Integración",
        Screen::ConflictReview => "· Conflictos",
        Screen::ReviewResult => "· Revisión",
        Screen::Help => "· Atajos",
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            colored(" SYMPHONY ", current_theme().brand).add_modifier(Modifier::BOLD),
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
        Screen::Chat => chat(app, f, body),
        Screen::Home => home(app, f, body),
        Screen::NewAgent => new_agent(app, f, body),
        Screen::ModelPicker => model_picker(app, f, body),
        Screen::Agent => agent(app, f, body),
        Screen::ExplainRoute => explain_route(app, f, body),
        Screen::Recovery => recovery(app, f, body),
        Screen::ContextOverview => context_overview(app, f, body),
        Screen::ContextObject => context_object(app, f, body),
        Screen::SharedSkills => shared_skills(app, f, body),
        Screen::SharedMcp => shared_mcp(app, f, body),
        Screen::Stats => stats(app, f, body),
        Screen::Tasks => tasks(app, f, body),
        Screen::TaskDetail => task_detail(app, f, body),
        Screen::Milestones => milestones(app, f, body),
        Screen::System => system(app, f, body),
        Screen::PerformanceProfile => performance_profile(app, f, body),
        Screen::Validation => validation(app, f, body),
        Screen::MergePreflight => merge_preflight(app, f, body),
        Screen::ConflictReview => conflict_review(app, f, body),
        Screen::ReviewResult => review_result(app, f, body),
        Screen::Help => help(app, f, body),
    }

    status_line(app, f, status_bar);
    footer_line(app, f, footer);
    THEME.with(|c| *c.borrow_mut() = None);
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
        Screen::Chat => {
            "Enter enviar · Tab modelo · ^Y copiar · ^E exportar · ^R revisar · ^G a main · F2 ratón · Esc"
        }
        Screen::Home => {
            "↑↓ elegir · Enter abrir · n nuevo · t tareas · m milestones · s sistema · p proveedores · r recovery · : comando · Esc chat · q salir"
        }
        Screen::NewAgent => "↑↓ campo · ←→ opción · Enter siguiente/crear · Esc cancelar",
        Screen::ModelPicker => "↑↓ elegir · Enter usar · Esc volver",
        Screen::ExplainRoute => "Esc volver al agente",
        Screen::Agent => match app.agent.as_ref() {
            Some(a) if a.typing => "Enter enviar · Esc cancelar",
            _ => {
                "←→ pestaña · m mensaje · p pausa · s modelo · ? por qué · v validación · g integrar · c contexto · o CLI · e exportar · d diff · x detener · Esc"
            }
        },
        Screen::Recovery => {
            "↑↓ elegir · r reiniciar · c recuperar · s detener · x descartar · Enter abrir · Esc volver"
        }
        Screen::ContextOverview => "↑↓ elegir · Enter ver el original · Esc volver",
        Screen::ContextObject => "↑↓ PgUp/PgDn desplazar · Esc volver al resumen",
        Screen::SharedSkills | Screen::SharedMcp => {
            "↑↓ elegir · Espacio/Enter encender o apagar · r releer · Esc volver"
        }
        Screen::Stats => "r releer · Esc volver",
        Screen::Tasks => {
            "↑↓ elegir · Enter detalle · a agente · m milestones · r releer · Esc volver"
        }
        Screen::TaskDetail => "a abrir el agente · r releer · Esc volver a tareas",
        Screen::Milestones => {
            "↑↓ elegir · n nuevo · a aprobar · c pedir cambios · h retener · r releer · Esc volver"
        }
        Screen::System => "p cambiar perfil · r releer · Esc volver",
        Screen::PerformanceProfile => "↑↓ elegir · Enter aplicar · Esc volver",
        Screen::Validation => {
            "r volver a validar · e enviar el fallo al agente · g integración · Esc volver al agente"
        }
        Screen::MergePreflight => {
            "p preflight · a aprobar · i integrar · x abandonar · c conflictos · w revisar · y veredicto"
        }
        Screen::ConflictReview => {
            "m resuelto a mano · w pedir revisor · p volver a comprobar · x abandonar · Esc"
        }
        Screen::ReviewResult => "w pedir revisión · r releer · Esc volver",
        Screen::Help => "Esc cerrar",
    }
}

fn footer_line(app: &App, f: &mut Frame, area: Rect) {
    let mut lines = Vec::new();
    if let Some(cmd) = &app.command {
        lines.push(Line::from(vec![
            colored("> ", current_theme().brand),
            Span::raw(cmd.clone()),
            Span::raw("▏"),
        ]));
    }
    lines.push(match &app.notice {
        Some(Notice::Error(e)) => Line::from(colored(format!(" {e}"), current_theme().error)),
        Some(Notice::Info(i)) => Line::from(colored(format!(" {i}"), current_theme().ok)),
        None => Line::from(dim(format!(" {}", hints(app)))),
    });
    f.render_widget(Paragraph::new(lines), area);
}

/// Barra de estado de una línea, siempre visible.
struct StatusField {
    id: &'static str,
    label: Option<&'static str>,
    value: String,
    color: Option<Color>,
}

impl StatusField {
    fn width(&self) -> usize {
        let label_w = self.label.map_or(0, |l| l.chars().count() + 1); // etiqueta + espacio
        label_w + self.value.chars().count()
    }
}

fn status_line(app: &App, f: &mut Frame, area: Rect) {
    let theme = current_theme();

    // modelo del momento
    let model_value = app
        .chat_model
        .clone()
        .or_else(|| {
            app.agent
                .as_ref()
                .and_then(|a| a.model().map(str::to_string))
        })
        .unwrap_or_else(|| "—".to_string());

    // Proveedor del modelo: manda el catálogo (`anthropic`), porque el prefijo del id no siempre
    // coincide (`claude/sonnet`); si el modelo no está en el catálogo, el `provider_id` del run y,
    // como último recurso, lo de antes de la primera barra.
    let provider = if model_value == "—" {
        String::new()
    } else {
        let from_catalog = app
            .models
            .iter()
            .find(|m| m["id"].as_str() == Some(model_value.as_str()))
            .and_then(|m| m["provider_id"].as_str());
        let from_run = app
            .agent
            .as_ref()
            .filter(|a| a.model() == Some(model_value.as_str()))
            .and_then(|a| a.inspect["current_run"]["provider_id"].as_str());
        from_catalog
            .or(from_run)
            .map(str::to_string)
            .unwrap_or_else(|| model_value.split('/').next().unwrap_or("").to_string())
    };

    // cuota
    let mut cuota_value = "—".to_string();
    if !provider.is_empty()
        && let Some(p) = app
            .providers
            .iter()
            .find(|p| p["id"].as_str() == Some(&provider))
    {
        let h = &p["health"];
        if let Some(rem) = h["remaining"].as_f64() {
            let pct = (rem * 100.0).round() as i64;
            let mut text = format!("{pct} %");
            // «reinicia en …» solo cuando la barra va ancha: en 80 columnas se come el resto.
            if area.width >= 100
                && let Some(reset) = h["reset_at"].as_i64()
                && reset > app.now_ms
            {
                text = format!("{text} (reinicia en {})", ago(reset, app.now_ms));
            }
            cuota_value = text;
        }
    }

    // carpeta
    let folder_path = app
        .project
        .get("root")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| app.cwd.clone());
    let folder = std::path::Path::new(&folder_path)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or(folder_path);

    // rama
    let branch = app
        .project
        .get("branch")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("—")
        .to_string();

    let mut fields = vec![
        StatusField {
            id: "modelo",
            label: Some("modelo"),
            value: model_value.clone(),
            color: theme.brand,
        },
        StatusField {
            id: "cuota",
            label: Some("cuota"),
            value: cuota_value,
            color: theme.muted,
        },
        StatusField {
            id: "carpeta",
            label: None,
            value: folder,
            color: theme.muted,
        },
        StatusField {
            id: "rama",
            label: Some("rama"),
            value: branch,
            color: theme.muted,
        },
        StatusField {
            id: "tokens",
            label: Some("tokens"),
            value: "—".to_string(),
            color: theme.muted,
        },
        StatusField {
            id: "modo",
            label: Some("modo"),
            value: "—".to_string(),
            color: theme.muted,
        },
    ];

    // Responsivo por ancho: umbrales fijos.
    if area.width < 110 {
        fields.retain(|f| f.id != "modo" && f.id != "tokens" && f.id != "carpeta");
    }
    if area.width < 80 {
        fields.retain(|f| f.id != "rama");
    }

    let sep = "  |  ";
    let sep_w = sep.chars().count();

    let total_width = |fs: &[StatusField]| -> usize {
        fs.iter().map(StatusField::width).sum::<usize>() + fs.len().saturating_sub(1) * sep_w
    };

    // Caída de campos de derecha a izquierda por prioridad.
    let drop_priority = ["modo", "tokens", "carpeta", "rama"];
    while total_width(&fields) > usize::from(area.width) && fields.len() > 2 {
        let mut idx = None;
        for pid in &drop_priority {
            if let Some(i) = fields.iter().position(|f| f.id == *pid) {
                idx = Some(i);
                break;
            }
        }
        match idx {
            Some(i) => {
                fields.remove(i);
            }
            None => break,
        }
    }

    // Recorte del modelo si aún no cabe.
    let other_w: usize = fields
        .iter()
        .filter(|f| f.id != "modelo")
        .map(StatusField::width)
        .sum();
    let sep_count = fields.len().saturating_sub(1);
    let available = usize::from(area.width)
        .saturating_sub(other_w)
        .saturating_sub(sep_count * sep_w);
    if let Some(mf) = fields.iter_mut().find(|f| f.id == "modelo")
        && mf.width() > available
    {
        let prefix = "modelo ";
        let prefix_w = prefix.chars().count();
        if available <= prefix_w {
            mf.value = "…".to_string();
        } else {
            let avail_value = available.saturating_sub(prefix_w);
            let chars: Vec<char> = mf.value.chars().collect();
            if chars.len() + prefix_w <= available {
                // ya cabe, nada
            } else if avail_value <= 1 {
                mf.value = "…".to_string();
            } else {
                let take = avail_value.saturating_sub(1);
                let trunc: String = chars.into_iter().take(take).collect();
                mf.value = format!("{trunc}…");
            }
        }
    }

    // Construcción de la línea (sin wrap).
    let mut spans: Vec<Span> = Vec::new();
    for (i, ffield) in fields.iter().enumerate() {
        if i > 0 {
            spans.push(colored(sep.to_string(), theme.muted));
        }
        if let Some(label) = ffield.label {
            spans.push(colored(format!("{label} "), theme.muted));
        }
        spans.push(colored(ffield.value.clone(), ffield.color));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn boxed(title: &str) -> Block<'static> {
    let mut st = Style::default();
    if let Some(c) = current_theme().border {
        st = st.fg(c);
    }
    Block::default()
        .borders(Borders::ALL)
        .border_style(st)
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

pub const LAUNCH_ART: &[&str] = &[
    "   *     .-.     *",
    "         (o o)",
    "          |=|",
    "         /| |\\",
    "        / | | \\  *",
    "       '  | |  ' /",
    "          | |",
    "         /   \\",
];

fn launch(app: &App, f: &mut Frame, area: Rect) {
    let check = |done: bool| {
        if done {
            colored("listo", current_theme().ok)
        } else {
            dim("…")
        }
    };
    let art_lines: Vec<Line<'static>> = if area.width < 66 || area.height < 20 {
        let inner = area.width.saturating_sub(2) as usize;
        let label = "SYMPHONY";
        let pad = inner.saturating_sub(label.len()) / 2;
        vec![
            Line::from(vec![
                Span::raw(" ".repeat(pad)),
                colored(label.to_string(), current_theme().brand).add_modifier(Modifier::BOLD),
            ]),
            Line::raw(""),
        ]
    } else {
        let inner = area.width.saturating_sub(2) as usize;
        let mut v: Vec<Line<'static>> = Vec::new();
        for l in LAUNCH_ART {
            let pad = inner.saturating_sub(l.chars().count()) / 2;
            v.push(Line::from(vec![
                Span::raw(" ".repeat(pad)),
                colored(l.to_string(), current_theme().brand),
            ]));
        }
        v.push(Line::raw(""));
        v
    };
    let status_lines = vec![
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
    let mut lines = art_lines;
    lines.extend(status_lines);
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
        lines.push(Line::raw(""));
        lines.push(section("PARA EMPEZAR"));
        for (k, v) in [
            (
                "i",
                "configurar el proyecto (nombre, rendimiento y failover)",
            ),
            ("o", "abrir sin configurar"),
            (
                ":",
                "barra de comandos (new, tasks, stats, providers, system...)",
            ),
            ("q", "salir"),
        ] {
            lines.push(Line::from(vec![
                bold(k.to_string()),
                Span::raw(format!("        {}", v)),
            ]));
        }
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
            spans.push(colored(format!("(•) {o}  "), current_theme().brand));
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
            current_theme().warn,
        )));
    }
    lines.push(Line::raw(""));
    lines.push(field(
        "",
        Line::from(if fr.field == 3 {
            colored("[ Entrar a Symphony ]", current_theme().brand).add_modifier(Modifier::BOLD)
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

/// Color de un estado de salud (FLOW §12.2).
fn health_color(state: &str) -> Option<Color> {
    let t = current_theme();
    match state {
        "HEALTHY" => t.ok,
        "EXHAUSTED" | "AUTH_ERROR" | "OFFLINE" => t.error,
        "UNKNOWN" => t.muted,
        _ => t.warn,
    }
}

/// Cuota con su certeza, sin inventar nunca un porcentaje (FLOW §12.3).
fn quota_text(h: &Value) -> String {
    match h["certainty"].as_str() {
        Some("KNOWN") => h["remaining"]
            .as_f64()
            .map_or("restante desconocido".into(), |r| {
                format!("{:.0} % restante", r * 100.0)
            }),
        Some("ESTIMATED") => "estimada (sin %)".into(),
        _ => "restante desconocido".into(),
    }
}

/// `12345` → `12,3k`; `1234567` → `1,2M`.
fn tokens_text(n: i64) -> String {
    match n {
        0..1000 => n.to_string(),
        1000..1_000_000 => format!("{:.1}k", n as f64 / 1000.0).replace('.', ","),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0).replace('.', ","),
    }
}

/// Detalle del proveedor seleccionado (FLOW §12.1).
fn provider_detail(app: &App, p: &Value, lines: &mut Vec<Line<'static>>) {
    let h = &p["health"];
    if h.is_null() {
        return;
    }
    let state = h["state"].as_str().unwrap_or("UNKNOWN");
    let mut health = format!("Salud: {state}");
    if let Some(e) = h["evidence"].as_str() {
        health.push_str(&format!(" · evidencia: {e}"));
    }
    if let Some(at) = h["retry_after_at"].as_i64().filter(|t| *t > app.now_ms) {
        health.push_str(&format!(" · reintenta en {}", ago(at, app.now_ms)));
    }
    if let Some(at) = h["reset_at"].as_i64().filter(|t| *t > app.now_ms) {
        health.push_str(&format!(" · se reinicia en {}", ago(at, app.now_ms)));
    }
    lines.push(Line::from(colored(health, health_color(state))));
    lines.push(Line::raw(format!(
        "Cuota: {} ({}) · reserva {:.0} %",
        quota_text(h),
        h["certainty"].as_str().unwrap_or("UNKNOWN"),
        h["reserve"].as_f64().unwrap_or(0.2) * 100.0
    )));
    let names: Vec<String> = p["model_names"]
        .as_array()
        .map(|a| a.iter().map(text).collect())
        .unwrap_or_default();
    if !names.is_empty() {
        lines.push(Line::raw(format!(
            "Modelos ({}): {}",
            names.len(),
            names.join(", ")
        )));
    }
    lines.push(Line::raw(format!(
        "Agentes que lo usan ahora: {}",
        num(&p["agents_active"])
    )));
    let f = &p["last_failure"];
    if f.is_object() {
        let msg = f["message"].as_str().unwrap_or("");
        lines.push(Line::from(dim(format!(
            "Último error: {}{} · hace {}",
            text(&f["type"]),
            if msg.is_empty() {
                String::new()
            } else {
                format!(" — {msg}")
            },
            ago(app.now_ms, f["at"].as_i64().unwrap_or(0))
        ))));
    }
    let u = &p["usage_7d"];
    let (rep, est) = (
        u["reported_tokens"].as_i64().unwrap_or(0),
        u["estimated_tokens"].as_i64().unwrap_or(0),
    );
    if rep > 0 || est > 0 {
        lines.push(Line::from(dim(format!(
            "Uso (7 días): {} tokens informados · {} estimados",
            tokens_text(rep),
            tokens_text(est)
        ))));
    }
}

fn providers(app: &App, f: &mut Frame, area: Rect) {
    let mut lines = vec![Line::from(dim(format!(
        "  {:<11} {:<15} {:<13} {:<21} {:<9} {:>7}",
        "PROVEEDOR", "ESTADO", "SALUD", "CUOTA", "VERSIÓN", "MODELOS"
    )))];
    for (i, p) in app.providers.iter().enumerate() {
        let enabled = p["enabled"].as_bool().unwrap_or(true);
        let state = text(&p["setup_state"]);
        let shown = if enabled {
            state.clone()
        } else {
            "DESACTIVADO".into()
        };
        let health = &p["health"];
        let health_state = health["state"].as_str().unwrap_or("UNKNOWN").to_string();
        lines.push(Line::from(vec![
            marker(i == app.selected),
            Span::raw(format!("{:<11} ", text(&p["display_name"]))),
            colored(
                format!("{shown:<15} "),
                if enabled {
                    state_color(&state)
                } else {
                    current_theme().muted
                },
            ),
            colored(
                format!("{health_state:<13} "),
                if enabled && state == "READY" {
                    health_color(&health_state)
                } else {
                    current_theme().muted
                },
            ),
            dim(format!("{:<21} ", quota_text(health))),
            Span::raw(format!(
                "{:<9} {:>7}",
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
        if app.screen == Screen::Providers {
            provider_detail(app, p, &mut lines);
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

// --- 14 Explain Route -----------------------------------------------------------

fn explain_route(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let decisions = app
        .explain
        .as_ref()
        .and_then(|e| e["decisions"].as_array())
        .cloned()
        .unwrap_or_default();
    if app.explain.is_none() {
        lines.push(Line::from(dim("Cargando…")));
    } else if decisions.is_empty() {
        lines.push(Line::raw(
            "Este agente todavía no tiene decisiones de routing registradas.",
        ));
    }
    if let Some(d) = decisions.first() {
        let trigger = match d["trigger"].as_str().unwrap_or("") {
            "SPAWN" => "al crear el agente",
            "FAILOVER" => "en un failover",
            "SWITCH" => "en un cambio de modelo",
            _ => "por una sugerencia",
        };
        lines.push(Line::from(vec![
            bold("Decisión más reciente: "),
            Span::raw(format!(
                "{trigger} · {} · hace {}",
                d["profile"]
                    .as_str()
                    .map_or("modelo exacto".into(), str::to_string),
                ago(app.now_ms, d["decided_at"].as_i64().unwrap_or(0))
            )),
        ]));
        lines.push(Line::raw(""));
        for l in d["explanation"].as_str().unwrap_or("").lines() {
            lines.push(Line::raw(l.to_string()));
        }
        lines.push(Line::raw(""));
        lines.push(section("CANDIDATOS"));
        lines.push(Line::from(dim(format!(
            "  {:<28} {:<10} {:>7}  MOTIVO",
            "MODELO", "ESTADO", "PUNTAJE"
        ))));
        let selected = d["selected"].as_str().unwrap_or("");
        for c in d["candidates"].as_array().cloned().unwrap_or_default() {
            let id = text(&c["model_id"]);
            let eligible = c["eligible"].as_bool().unwrap_or(false);
            lines.push(Line::from(vec![
                Span::raw(if id == selected { "▸ " } else { "  " }),
                Span::raw(format!("{id:<28} ")),
                colored(
                    format!("{:<10} ", if eligible { "elegible" } else { "descartado" }),
                    if eligible {
                        current_theme().ok
                    } else {
                        current_theme().muted
                    },
                ),
                Span::raw(format!(
                    "{:>7}  ",
                    c["score"]
                        .as_f64()
                        .map_or("—".into(), |s| format!("{s:.2}"))
                )),
                dim(c["reject_reason"].as_str().unwrap_or("").to_string()),
            ]));
        }
        if decisions.len() > 1 {
            lines.push(Line::raw(""));
            lines.push(section("DECISIONES ANTERIORES"));
            for p in &decisions[1..] {
                lines.push(Line::from(dim(format!(
                    "  hace {} · {} → {}",
                    ago(app.now_ms, p["decided_at"].as_i64().unwrap_or(0)),
                    text(&p["trigger"]),
                    p["selected"].as_str().unwrap_or("ninguno")
                ))));
            }
        }
    }
    paragraph(f, area, "Explain Route", lines);
}

// --- 00 Chat ---------------------------------------------------------------

/// Parte `s` en líneas de a lo sumo `width` caracteres, cortando en espacios cuando se puede.
fn wrap(s: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for src in s.lines() {
        let mut line = String::new();
        let mut len = 0;
        for word in src.split(' ') {
            let mut word: Vec<char> = word.chars().collect();
            // Una palabra más larga que la línea se parte a la fuerza.
            while word.len() > width {
                if len > 0 {
                    out.push(std::mem::take(&mut line));
                    len = 0;
                }
                out.push(word.drain(..width).collect());
            }
            let n = word.len();
            if len > 0 && len + 1 + n > width {
                out.push(std::mem::take(&mut line));
                len = 0;
            }
            if len > 0 {
                line.push(' ');
                len += 1;
            }
            line.extend(word);
            len += n;
        }
        out.push(line);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Prompt de un handoff: lo ve el modelo nuevo, no el usuario (FLOW §13.4 ya marca el cambio).
// ponytail: se reconoce por su frase inicial (crates/context/src/handoff.rs); si cambia, hay que
// actualizarla acá o el chat mostraría el handoff entero.
// Sin «de código»: los handoffs viejos lo decían y los nuevos no (ADR-0020).
const HANDOFF_PROMPT: &str = "Retomas una tarea";

/// Líneas de un mensaje (vacío si no se muestra).
fn message_lines(m: &Value, width: usize) -> Vec<Line<'static>> {
    let content = m["content"].as_str().unwrap_or("");
    let mut lines = Vec::new();
    match m["role"].as_str().unwrap_or("") {
        "EXECUTOR_CHANGE" => {
            for l in wrap(content, width) {
                lines.push(Line::from(colored(l, current_theme().accent)));
            }
        }
        "USER" if content.starts_with(HANDOFF_PROMPT) => return lines,
        role => {
            let (who, color) = match role {
                "USER" => ("tú", current_theme().brand),
                "ASSISTANT" => ("agente", current_theme().ok),
                _ => ("sistema", current_theme().muted),
            };
            for (i, l) in wrap(content, width.saturating_sub(9))
                .into_iter()
                .enumerate()
            {
                let prefix = if i == 0 {
                    format!("{who:>7} │ ")
                } else {
                    "        │ ".into()
                };
                lines.push(Line::from(vec![colored(prefix, color), Span::raw(l)]));
            }
        }
    }
    lines.push(Line::raw(""));
    lines
}

/// Una herramienta que usó el agente, en una línea tenue.
fn tool_line(t: &Value, width: usize) -> Line<'static> {
    let (mark, color) = match t["status"].as_str().unwrap_or("") {
        "DONE" => ("✓", current_theme().ok),
        "FAILED" | "DENIED" => ("✗", current_theme().error),
        _ => ("…", current_theme().warn),
    };
    let cmd = t["command"].as_str().unwrap_or("").replace('\n', " ");
    let tool = t["tool"].as_str().unwrap_or("?");
    let room = width.saturating_sub(tool.chars().count() + 12);
    let cmd: String = cmd.chars().take(room).collect();
    Line::from(vec![
        dim("        ⚙ "),
        colored(format!("{mark} "), color),
        dim(format!("{tool} {cmd}")),
    ])
}

/// La conversación con lo que el agente hizo entre mensajes (herramientas), por orden de hora.
/// `tools` viene de `agent.activity` (lo más nuevo primero); solo cuentan los `kind == "tool"`.
fn chat_lines(messages: &[Value], tools: &[Value], width: usize) -> Vec<Line<'static>> {
    let at = |v: &Value| v["at"].as_i64().unwrap_or(0);
    let mut timeline: Vec<(i64, Vec<Line<'static>>)> = messages
        .iter()
        .map(|m| (at(m), message_lines(m, width)))
        .collect();
    timeline.extend(
        tools
            .iter()
            .rev()
            .filter(|t| t["kind"] == "tool")
            .map(|t| (at(t), vec![tool_line(t, width)])),
    );
    // Estable: a igual hora conserva el orden (mensajes antes que herramientas).
    timeline.sort_by_key(|(t, _)| *t);
    timeline.into_iter().flat_map(|(_, l)| l).collect()
}

/// La sugerencia de cambio de modelo pendiente (FLOW §8.5), en un par de líneas. `keys` dice con
/// qué teclas se resuelve en esta vista.
fn suggestion_lines(s: Option<&Value>, keys: &str, width: usize) -> Vec<Line<'static>> {
    let Some(s) = s else { return Vec::new() };
    let text = format!(
        "Sugerencia: {} → {} · {}",
        text(&s["current_model_id"]),
        text(&s["suggested_model_id"]),
        text(&s["reason"])
    );
    let mut out = vec![Line::raw("")];
    for (i, l) in wrap(&text, width.saturating_sub(2)).into_iter().enumerate() {
        out.push(Line::from(colored(
            if i == 0 {
                format!("◆ {l}")
            } else {
                format!("  {l}")
            },
            current_theme().accent,
        )));
    }
    out.push(Line::from(dim(format!("  {keys}"))));
    out
}

/// El mensaje que el agente está escribiendo (vacío si no hay), con cursor al final.
fn partial_lines(partial: &str, width: usize) -> Vec<Line<'static>> {
    if partial.is_empty() {
        return Vec::new();
    }
    let m = serde_json::json!({ "role": "ASSISTANT", "content": format!("{partial}▍") });
    message_lines(&m, width)
}

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// «⠋ pensando… 12 s» / «⠋ ejecutando Bash… 3 s»: lo que el agente está haciendo ahora.
fn working_line(app: &App, view: &AgentView) -> Line<'static> {
    let running_tool = view
        .activity
        .iter()
        .find(|t| t["kind"] == "tool" && t["status"] == "RUNNING")
        .and_then(|t| t["tool"].as_str());
    // Tiempo desde lo último que se supo del agente (mensaje o herramienta).
    let last = view
        .messages
        .iter()
        .chain(view.activity.iter())
        .filter_map(|v| v["at"].as_i64())
        .max();
    let secs = last.map_or(0, |l| ((app.now_ms - l) / 1000).max(0));
    let what = running_tool.map_or_else(|| "pensando".to_string(), |t| format!("ejecutando {t}"));
    Line::from(vec![
        colored(
            format!("{} ", SPINNER[app.frame as usize % SPINNER.len()]),
            current_theme().warn,
        ),
        dim(format!("{what}… {secs} s")),
    ])
}

fn chat(app: &App, f: &mut Frame, area: Rect) {
    let view = app.chat.as_ref();
    let state = view.map_or("", |v| v.state()).to_string();
    let reason = view
        .and_then(|v| v.inspect["agent"]["state_reason"].as_str())
        .map_or_else(|| state_phrase(&state).to_string(), str::to_string);
    let current = view.and_then(|v| v.model());
    let mut model = vec![
        dim("Modelo:  "),
        Span::raw(current.unwrap_or("").to_string()),
    ];
    if current.is_none() && app.chat_model.is_none() {
        model.push(Span::raw("—"));
    }
    match (&app.chat_model, current) {
        (Some(next), Some(cur)) if next != cur => {
            model.push(dim("  →  "));
            model.push(colored(next.clone(), current_theme().brand).add_modifier(Modifier::BOLD));
            model.push(dim("  (cambia al enviar)"));
        }
        (Some(next), None) => {
            model.push(colored(next.clone(), current_theme().brand).add_modifier(Modifier::BOLD));
            model.push(dim("  (primer mensaje)"));
        }
        _ => {}
    }
    model.push(dim("   Rama: symphony/chat"));
    let head = vec![
        if view.is_some() {
            Line::from(vec![
                bold("CHAT  "),
                colored(state.clone(), state_color(&state)).add_modifier(Modifier::BOLD),
                dim(format!("  {reason}")),
            ])
        } else {
            Line::from(vec![bold("CHAT  "), dim("todavía no empezó")])
        },
        Line::from(model),
    ];
    let [top, body, input] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(3),
        Constraint::Length(4),
    ])
    .areas(area);
    f.render_widget(Paragraph::new(head).block(boxed("Chat")), top);

    let inner_w = usize::from(body.width.saturating_sub(2));
    let inner_h = usize::from(body.height.saturating_sub(2));
    let mut lines = view.map_or_else(Vec::new, |v| chat_lines(&v.messages, &v.activity, inner_w));
    lines.extend(suggestion_lines(
        view.and_then(AgentView::suggestion),
        "^T cambiar · ^K mantener · ^N no sugerir más",
        inner_w,
    ));
    if let Some(v) = view.filter(|v| v.state() == "RUNNING") {
        // Con texto parcial, el texto mismo es la señal de trabajo.
        if v.partial.is_empty() {
            lines.push(working_line(app, v));
        } else {
            lines.extend(partial_lines(&v.partial, inner_w));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(dim("Escribe tu primer mensaje para empezar.")));
        lines.push(Line::from(dim(
            "Symphony trabaja en la rama symphony/chat de este proyecto y guarda un commit por turno.",
        )));
        lines.push(Line::from(dim(
            "Con Tab eliges el modelo de cada mensaje: cambiar a mitad conserva la conversación.",
        )));
    }
    let max = lines.len().saturating_sub(inner_h);
    app.chat_max.set(u16::try_from(max).unwrap_or(u16::MAX));
    let scroll = usize::from(app.chat_scroll).min(max);
    let offset = max - scroll;
    let position = if max == 0 {
        String::new()
    } else if scroll == 0 {
        " final ".to_string()
    } else {
        format!(
            " ↑ {scroll} líneas antes del final · {}-{} de {} ",
            offset + 1,
            (offset + inner_h).min(lines.len()),
            lines.len()
        )
    };
    f.render_widget(
        Paragraph::new(lines)
            .block(boxed("Conversación").title_bottom(Line::from(dim(position)).right_aligned()))
            .scroll((u16::try_from(offset).unwrap_or(u16::MAX), 0)),
        body,
    );

    // Lo escrito se muestra por su cola: las últimas líneas ya ajustadas, así lo que se está
    // tecleando siempre se ve.
    let rows = usize::from(input.height.saturating_sub(2)).max(1);
    let wrapped = wrap(
        &format!("{}▏", app.chat_input),
        usize::from(input.width.saturating_sub(2)),
    );
    let shown = wrapped[wrapped.len().saturating_sub(rows)..].join(
        "
",
    );
    let mut title = match &app.chat_model {
        Some(m) => format!("Mensaje · para {m} (Tab cambia)"),
        None => "Mensaje".to_string(),
    };
    // Prefijo de skill del otro proveedor: no se reconocería; se avisa en vez de fallar callado.
    if let (Some(prefix), Some(first)) = (app.skill_prefix(), app.chat_input.chars().next()) {
        let other = if prefix == '/' { '$' } else { '/' };
        if first == other && !app.chat_input.contains(char::is_whitespace) {
            title = format!("Con este modelo las skills empiezan con {prefix}");
        }
    }
    f.render_widget(Paragraph::new(shown).block(boxed(&title)), input);
    skill_popup(app, f, body);
}

/// Desplegable de skills sobre la conversación, pegado a la caja de mensaje.
fn skill_popup(app: &App, f: &mut Frame, body: Rect) {
    let matches = app.skill_matches();
    if matches.is_empty() {
        return;
    }
    let rows = matches.len().min(8);
    let sel = app.skill_sel.min(matches.len() - 1);
    let start = (sel + 1).saturating_sub(rows);
    let height = u16::try_from(rows + 2).unwrap_or(10).min(body.height);
    let area = Rect {
        y: body.y + body.height - height,
        height,
        ..body
    };
    let prefix = app.skill_prefix().unwrap_or('/');
    let lines: Vec<Line> = matches[start..start + rows]
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let selected = start + i == sel;
            let name = format!("{prefix}{}", text(&m["name"]));
            let row = Line::from(vec![
                marker(selected),
                bold(format!("{name:<30} ")),
                dim(text(&m["description"])),
            ]);
            if selected {
                row.style(theme_fg(current_theme().brand))
            } else {
                row
            }
        })
        .collect();
    let who = if prefix == '/' { "Claude" } else { "Codex" };
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(boxed(&format!(
            "Skills de {who} ({}) · → completa · ↑↓ elige · Esc cierra",
            matches.len()
        ))),
        area,
    );
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
            current_theme().warn,
        )));
    }
    let attention = app.providers_needing_attention();
    if attention > 0 {
        lines.push(Line::from(colored(
            format!("⚠ {attention} proveedor(es) necesitan atención: pulsa p."),
            current_theme().warn,
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
    match app.system.as_ref() {
        Some(s) => {
            let pct = |v: &Value| v.as_f64().map_or("—".to_string(), |p| format!("{p:.0}%"));
            lines.push(Line::raw(format!(
                "  Perfil {} · CPU {} · RAM {} · slots pesados {}/{}",
                text(&s["profile"]),
                pct(&s["cpu_pct"]),
                pct(&s["ram_pct"]),
                num(&s["heavy_slots_used"]),
                num(&s["heavy_slots_total"])
            )));
            let waiting = s["waiting"].as_array().map_or(0, Vec::len);
            if waiting > 0 {
                lines.push(Line::from(colored(
                    format!("  {waiting} operación(es) esperando turno: s para ver por qué."),
                    current_theme().warn,
                )));
            }
        }
        None => lines.push(Line::from(dim("  Midiendo…"))),
    }

    paragraph(f, area, "Inicio", lines);
}

// --- 05 New Agent / 13 Model Picker -----------------------------------------

fn new_agent(app: &App, f: &mut Frame, area: Rect) {
    let na = &app.new_agent;
    let pick = |on: bool, label: String| {
        if on {
            colored(format!("(•) {label}"), current_theme().brand)
        } else {
            dim(format!("( ) {label}"))
        }
    };
    let model = na
        .model
        .clone()
        .unwrap_or_else(|| "Enter para elegir".into());
    let profile = na
        .profile
        .clone()
        .unwrap_or_else(|| "Enter para elegir".into());
    let exec = Line::from(vec![
        pick(na.exec == Exec::Exact, format!("Modelo exacto: {model}")),
        Span::raw("   "),
        pick(na.exec == Exec::Profile, format!("Profile: {profile}")),
        Span::raw("   "),
        pick(na.exec == Exec::Later, "Decidir después".into()),
    ]);
    let lines = vec![
        field(
            "Tarea",
            Line::raw(cursor(&na.task, na.field == 0)),
            na.field == 0,
        ),
        Line::raw(""),
        field("Ejecución", exec, na.field == 1),
        Line::from(dim(
            "                  Un modelo exacto no se sustituye en silencio; un profile (@code, @fast…) deja elegir a Symphony.",
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
                colored("[ Crear agente ]", current_theme().brand).add_modifier(Modifier::BOLD)
            } else {
                Span::raw("[ Crear agente ]")
            }),
            na.field == 4,
        ),
    ];
    paragraph(f, area, "Nuevo agente", lines);
}

/// FLOW §8.1: el estado que se ve junto a cada modelo.
fn model_status(m: &Value) -> (String, Option<Color>) {
    if !m["available"].as_bool().unwrap_or(false) {
        return (text(&m["setup_state"]), current_theme().warn);
    }
    match m["health"].as_str().unwrap_or("UNKNOWN") {
        "HEALTHY" | "UNKNOWN" | "PROBING" => ("DISPONIBLE".into(), current_theme().ok),
        other => (other.to_string(), health_color(other)),
    }
}

fn model_picker(app: &App, f: &mut Frame, area: Rect) {
    let mut lines = vec![section(
        "PROFILES (Symphony elige entre los modelos utilizables)",
    )];
    for (i, p) in app.profiles.iter().enumerate() {
        lines.push(Line::from(vec![
            marker(i == app.selected),
            bold(format!("{:<12} ", text(&p["id"]))),
            dim(text(&p["description"])),
        ]));
    }
    if app.profiles.is_empty() {
        lines.push(Line::from(dim("  Cargando profiles…")));
    }
    lines.push(Line::raw(""));
    lines.push(section(
        "MODELOS EXACTOS (se obedecen; no se sustituyen en silencio)",
    ));
    let offset = app.profiles.len();
    for (i, m) in app.models.iter().enumerate() {
        let (status, color) = model_status(m);
        lines.push(Line::from(vec![
            marker(offset + i == app.selected),
            Span::raw(format!("{:<12} / ", text(&m["provider"]))),
            Span::raw(format!("{:<20} ", text(&m["display_name"]))),
            colored(status, color),
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
        .highlight_style(theme_fg(current_theme().brand).add_modifier(Modifier::BOLD)),
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
            current_theme().warn,
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
    // Con ajuste de línea: un texto largo del agente no se corta en el borde.
    let width = usize::from(area.width.saturating_sub(2));
    let mut lines = chat_lines(&a.messages, &[], width);
    lines.extend(partial_lines(&a.partial, width));
    lines.extend(suggestion_lines(
        a.suggestion(),
        "S cambiar · K mantener · N no sugerir más",
        width,
    ));
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
    let inner = usize::from(area.height.saturating_sub(2));
    a.scroll_max
        .set(u16::try_from(lines.len().saturating_sub(inner)).unwrap_or(u16::MAX));
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
                colored(format!("checkpoint #{}", it["seq"]), current_theme().accent),
                dim(format!("  {}", it["current_step"].as_str().unwrap_or(""))),
            ]));
        } else {
            let status = it["status"].as_str().unwrap_or("");
            let (mark, color) = match status {
                "DONE" => ("✓", current_theme().ok),
                "FAILED" | "DENIED" => ("✗", current_theme().error),
                _ => ("…", current_theme().warn),
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
    let stats = crate::app::diff_stats(&a.diff);

    let diff_lines: Vec<Line<'static>> = a
        .diff
        .lines()
        .map(|l| {
            let color = if l.starts_with("+++") || l.starts_with("---") {
                Some(Color::White)
            } else if l.starts_with('+') {
                current_theme().ok
            } else if l.starts_with('-') {
                current_theme().error
            } else if l.starts_with("@@") {
                current_theme().accent
            } else {
                Some(Color::Reset)
            };
            Line::from(colored(l.to_string(), color))
        })
        .collect();

    let mut lines: Vec<Line<'static>> = Vec::new();
    if !stats.is_empty() {
        for s in &stats {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::raw(s.path.clone()),
                Span::raw("  "),
                colored(format!("+{}", s.added), current_theme().ok),
                Span::raw(" "),
                colored(format!("\u{2212}{}", s.removed), current_theme().error),
            ]));
        }
        lines.push(Line::raw(""));
    }
    if diff_lines.is_empty() && stats.is_empty() {
        lines.push(Line::from(dim("Sin cambios contra la base.")));
    } else {
        lines.extend(diff_lines);
    }

    let title = if stats.is_empty() {
        "Cambios".to_string()
    } else {
        let n = stats.len();
        let total_added: usize = stats.iter().map(|s| s.added).sum();
        let total_removed: usize = stats.iter().map(|s| s.removed).sum();
        let archivo = if n == 1 { "archivo" } else { "archivos" };
        format!(
            "Cambios · {} {} +{} \u{2212}{}",
            n, archivo, total_added, total_removed
        )
    };

    f.render_widget(
        Paragraph::new(lines)
            .block(boxed(&title))
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
            current_theme().accent,
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
        lines.push(Line::from(colored(
            "Nada que recuperar.",
            current_theme().ok,
        )));
        lines.push(Line::from(dim("Esc para volver al inicio.")));
    }
    for (i, it) in app.recovery.iter().enumerate() {
        let who = it["agent_number"]
            .as_i64()
            .map_or_else(|| "proyecto".to_string(), |n| format!("agente #{n}"));
        lines.push(Line::from(vec![
            marker(i == app.selected),
            colored(format!("{:<20} ", text(&it["kind"])), current_theme().warn),
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

// --- 20 Context Overview ----------------------------------------------------

/// Nombre de cada nivel de fidelidad (`handoff_items.fidelity`); los mismos que `symphony
/// context inspect` y `symphony_context::handoff::fidelity`.
fn fidelity_name(level: Option<i64>) -> &'static str {
    match level {
        Some(0) => "omitido",
        Some(1) => "referencia",
        Some(2) => "esqueleto",
        Some(3) => "resumen",
        Some(4) => "recortado",
        Some(5) => "completo",
        _ => "—",
    }
}

fn context_overview(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(o) = app.ctx_overview.as_ref() else {
        lines.push(Line::from(dim("Cargando…")));
        return paragraph(f, area, "Contexto del handoff", lines);
    };
    if o["handoff"].is_null() {
        lines.push(Line::raw("Este agente todavía no tiene handoffs."));
        lines.push(Line::from(dim(
            "Aparece uno cuando continúa en otro modelo o proveedor.",
        )));
        return paragraph(f, area, "Contexto del handoff", lines);
    }
    let h = &o["handoff"];
    let raw = h["tokens_raw_estimate"].as_i64();
    let sent = h["tokens_sent"].as_i64();
    let saved = match (raw, sent) {
        (Some(r), Some(s)) if r > 0 => format!(" ({}% del original)", s * 100 / r),
        _ => String::new(),
    };
    lines.push(Line::from(vec![
        bold("Modo: "),
        Span::raw(text(&h["mode"])),
        Span::raw("   "),
        bold("Enviado: "),
        Span::raw(format!("{} tokens{saved}", num(&h["tokens_sent"]))),
        Span::raw("   "),
        bold("Original: "),
        Span::raw(format!("{} tokens", num(&h["tokens_raw_estimate"]))),
    ]));
    lines.push(Line::from(dim(format!(
        "  armado en {} ms · resultado: {}",
        num(&h["build_ms"]),
        h["outcome"].as_str().unwrap_or("sin dato")
    ))));
    lines.push(Line::raw(""));
    lines.push(Line::from(dim(format!(
        "  {:<13} {:<40} {:<13} {:>7}",
        "SECCIÓN", "RUTA", "FIDELIDAD", "TOKENS"
    ))));
    let items = app.overview_items();
    for (i, it) in items.iter().enumerate() {
        lines.push(Line::from(vec![
            marker(i == app.selected),
            Span::raw(format!(
                "{:<13} {:<40} {:<13} {:>7}",
                text(&it["section"]),
                it["path"].as_str().unwrap_or("—"),
                fidelity_name(it["fidelity"].as_i64()),
                num(&it["tokens"])
            )),
        ]));
    }
    if items.is_empty() {
        lines.push(Line::from(dim("  El handoff no detalla partes.")));
    }
    paragraph(f, area, "Contexto del handoff", lines);
}

// --- 21 Context Object ------------------------------------------------------

fn context_object(app: &App, f: &mut Frame, area: Rect) {
    let uri = app.ctx_uri.clone().unwrap_or_default();
    let mut lines: Vec<Line<'static>> = vec![Line::from(vec![bold("Dirección: "), Span::raw(uri)])];
    match app.ctx_object.as_ref() {
        None => lines.push(Line::from(dim("Cargando…"))),
        Some(o) => {
            lines.push(Line::from(dim(format!(
                "  {} · {} bytes · compresor {} · {} → {} tokens",
                text(&o["kind"]),
                num(&o["bytes"]),
                o["compressor"].as_str().unwrap_or("ninguno"),
                num(&o["tokens_original"]),
                num(&o["tokens_compressed"])
            ))));
            lines.push(Line::raw(""));
            let skip = usize::from(app.ctx_scroll);
            for l in o["text"].as_str().unwrap_or("").lines().skip(skip) {
                lines.push(Line::raw(l.to_string()));
            }
        }
    }
    paragraph(f, area, "Objeto de contexto", lines);
}

// --- 32 Shared Skills / 33 Shared MCP ---------------------------------------

fn switch_span(item: &Value) -> Span<'static> {
    if item["enabled"].as_bool() == Some(true) {
        colored("[on ] ", current_theme().ok)
    } else {
        dim("[off] ")
    }
}

fn shared_skills(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    if app.shared_skills.is_empty() {
        lines.push(Line::raw("No hay skills compartidas."));
        lines.push(Line::from(dim(
            "Crea ~/.symphony/skills/<nombre>/SKILL.md o .symphony/skills/<nombre>/SKILL.md.",
        )));
    }
    for (i, s) in app.shared_skills.iter().enumerate() {
        let shadow = if s["shadowed"].as_bool() == Some(true) {
            " · tapada por la del proyecto"
        } else {
            ""
        };
        lines.push(Line::from(vec![
            marker(i == app.selected),
            switch_span(s),
            bold(format!("{:<22} ", text(&s["name"]))),
            dim(format!("{:<8} ", text(&s["scope"]))),
            Span::raw(s["description"].as_str().unwrap_or("").to_string()),
            dim(shadow.to_string()),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(dim(
        "Claude las carga como plugin de sesión. Codex no tiene un directorio de skills por invocación: aún no las recibe.",
    )));
    paragraph(f, area, "Skills compartidas", lines);
}

fn shared_mcp(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    if app.shared_mcp.is_empty() {
        lines.push(Line::raw("No hay servidores MCP compartidos."));
        lines.push(Line::from(dim(
            "Defínelos en ~/.symphony/mcp.toml o .symphony/mcp.toml con [servers.<nombre>].",
        )));
    }
    for (i, s) in app.shared_mcp.iter().enumerate() {
        let target = s["command"]
            .as_str()
            .or_else(|| s["url"].as_str())
            .unwrap_or("")
            .to_string();
        lines.push(Line::from(vec![
            marker(i == app.selected),
            switch_span(s),
            bold(format!("{:<18} ", text(&s["name"]))),
            dim(format!(
                "{:<8} {:<6} ",
                text(&s["scope"]),
                text(&s["transport"])
            )),
            Span::raw(target),
        ]));
        if let Some(n) = s["note"].as_str() {
            lines.push(Line::from(dim(format!("      {n}"))));
        }
    }
    for w in &app.shared_warnings {
        lines.push(Line::from(colored(
            format!("aviso: {w}"),
            current_theme().warn,
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(dim(
        "El broker de contexto (symphony-context) siempre está: no se apaga aquí.",
    )));
    paragraph(f, area, "Servidores MCP", lines);
}

// --- 35 Usage / Context Stats -----------------------------------------------

fn stats(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(s) = app.ctx_stats.as_ref() else {
        lines.push(Line::from(dim("Cargando…")));
        return paragraph(f, area, "Uso y contexto", lines);
    };
    lines.push(section("CONTEXTO"));
    lines.push(Line::raw(format!(
        "  {} objetos · {} chunks indexados",
        num(&s["objects"]),
        num(&s["chunks"])
    )));
    lines.push(Line::from(dim(format!(
        "  {:<14} {:>8} {:>12} {:>12}",
        "COMPRESOR", "OBJETOS", "ORIGINAL", "COMPRIMIDO"
    ))));
    for c in s["compression"].as_array().cloned().unwrap_or_default() {
        lines.push(Line::raw(format!(
            "  {:<14} {:>8} {:>12} {:>12}",
            text(&c["compressor"]),
            num(&c["objects"]),
            num(&c["tokens_original"]),
            num(&c["tokens_compressed"])
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(dim(format!(
        "  {:<12} {:>9} {:>11} {:>10} {:>11} {:>8}",
        "HANDOFF", "HANDOFFS", "ORIGINAL", "ENVIADO", "RECUPERÓ", "FALLÓ"
    ))));
    for h in s["handoffs"].as_array().cloned().unwrap_or_default() {
        lines.push(Line::raw(format!(
            "  {:<12} {:>9} {:>11} {:>10} {:>11} {:>8}",
            text(&h["mode"]),
            num(&h["handoffs"]),
            num(&h["tokens_raw"]),
            num(&h["tokens_sent"]),
            num(&h["needed_retrieval"]),
            num(&h["failed_to_continue"])
        )));
    }
    let r = &s["retrievals"];
    lines.push(Line::raw(""));
    lines.push(Line::raw(format!(
        "  Recuperaciones: {} ({} sin resultado) · {} tokens devueltos",
        num(&r["total"]),
        num(&r["misses"]),
        num(&r["tokens_returned"])
    )));
    lines.push(Line::raw(""));
    lines.push(section("USO POR PROVEEDOR (7 días)"));
    let usage = app
        .usage
        .as_ref()
        .and_then(|u| u["usage"].as_array().cloned())
        .unwrap_or_default();
    if usage.is_empty() {
        lines.push(Line::from(dim("  Sin uso registrado.")));
    }
    for u in usage {
        let source = match u["source"].as_str() {
            Some("REPORTED") => "reportado",
            Some("ESTIMATED") => "estimado",
            _ => "—",
        };
        lines.push(Line::raw(format!(
            "  {:<10} {:<20} {:<10} {:>3} corridas {:>10} ent. {:>8} sal.",
            text(&u["provider_id"]),
            text(&u["model_id"]),
            source,
            num(&u["runs"]),
            num(&u["tokens_in"]),
            num(&u["tokens_out"])
        )));
    }
    paragraph(f, area, "Uso y contexto", lines);
}

// --- selección con el ratón -------------------------------------------------

/// Tramos `(fila, desde, hasta)` de la selección, ambos extremos incluidos y dentro del buffer.
fn selection_rows(area: Rect, sel: &Selection) -> Vec<(u16, u16, u16)> {
    let ((x0, y0), (x1, y1)) = sel.ordered();
    let (left, right) = (area.left(), area.right().saturating_sub(1));
    let last_row = area.bottom().saturating_sub(1);
    (y0..=y1.min(last_row))
        .map(|y| {
            let from = if y == y0 { x0.max(left) } else { left };
            let to = if y == y1 { x1.min(right) } else { right };
            (y, from, to)
        })
        .filter(|(_, from, to)| from <= to)
        .collect()
}

/// Resalta (video inverso) lo seleccionado.
pub fn highlight(buf: &mut Buffer, sel: &Selection) {
    for (y, from, to) in selection_rows(buf.area, sel) {
        buf.set_style(
            Rect::new(from, y, to - from + 1, 1),
            Style::default().add_modifier(Modifier::REVERSED),
        );
    }
}

/// Una fila de pantalla sin el marco ni la etiqueta del hablante (`  tú │ `); `None` si es
/// una línea de marco.
fn clean_row(row: &str) -> Option<String> {
    let t = row.trim_end();
    if t.trim_start().starts_with(['┌', '└']) {
        return None;
    }
    let mut t = t.trim_end_matches('│').trim_end();
    if t.trim_start().starts_with('│') {
        t = &t[t.find('│').map_or(0, |i| i + '│'.len_utf8())..];
    }
    if let Some(pos) = t.find('│') {
        let label = &t[..pos];
        if label.chars().count() <= 9 && label.chars().all(|c| c.is_alphabetic() || c == ' ') {
            t = &t[pos + '│'.len_utf8()..];
            t = t.strip_prefix(' ').unwrap_or(t);
        }
    }
    Some(t.trim_end().to_string())
}

/// El texto que se ve en la selección, tal como está en pantalla.
// ponytail: lo que se lee es lo dibujado: los saltos de línea del ajuste al ancho quedan como
// saltos de línea; para el texto original está `Ctrl+Y` (última respuesta) o exportar a `.md`.
pub fn selected_text(buf: &Buffer, sel: &Selection) -> String {
    let rows: Vec<String> = selection_rows(buf.area, sel)
        .into_iter()
        .filter_map(|(y, from, to)| {
            let row: String = (from..=to).map(|x| buf[(x, y)].symbol()).collect();
            clean_row(&row)
        })
        .collect();
    rows.join(
        "
",
    )
    .trim()
    .to_string()
}

// --- Fase 2: tareas, milestones, sistema, validación e integración (P08.S8) -------------

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

fn status_color(status: &str) -> Option<Color> {
    let t = current_theme();
    match status {
        "DONE" | "PASS" | "APPROVED" | "MERGED" | "READY" => t.ok,
        "FAILED" | "FAIL" | "BLOCKED" | "CANCELLED" | "ABORTED" | "REJECT" => t.error,
        "RUNNING" | "OPEN" | "TESTING" => t.accent,
        _ => t.warn,
    }
}

/// 15 Task List.
fn tasks(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    if app.tasks.is_empty() {
        lines.push(Line::from(dim(
            "Todavía no hay tareas: crea un agente con n desde Inicio.",
        )));
    } else {
        lines.push(Line::from(dim(format!(
            "  {:<6} {:<30} {:<10} {:<20} {}",
            "CÓDIGO", "TAREA", "ESTADO", "AGENTE", "ESPERA / INTEGRACIÓN"
        ))));
    }
    for (i, t) in app.tasks.iter().enumerate() {
        let status = text(&t["status"]);
        let agent = match (t["agent_number"].as_i64(), t["agent_state"].as_str()) {
            (Some(n), Some(s)) => format!("#{n} {s}"),
            _ => "—".to_string(),
        };
        let waits = t["waiting_for"].as_array().cloned().unwrap_or_default();
        let tail = if !waits.is_empty() {
            format!(
                "espera a {}",
                waits.iter().map(text).collect::<Vec<_>>().join(", ")
            )
        } else if let Some(m) = t["merge_status"].as_str() {
            format!("integración {m}")
        } else {
            String::new()
        };
        lines.push(Line::from(vec![
            marker(i == app.selected),
            Span::raw(format!(
                "{:<6} {:<30} ",
                text(&t["code"]),
                truncate(&text(&t["title"]), 30)
            )),
            colored(format!("{status:<10} "), status_color(&status)),
            Span::raw(format!("{agent:<20} ")),
            dim(tail),
        ]));
    }
    paragraph(f, area, "Tareas", lines);
}

/// 16 Task Detail.
fn task_detail(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(t) = app.detail_task.as_ref() else {
        lines.push(Line::from(dim("Elige una tarea en la lista.")));
        return paragraph(f, area, "Tarea", lines);
    };
    let status = text(&t["status"]);
    lines.push(Line::from(vec![bold(format!(
        "{} · {}",
        text(&t["code"]),
        text(&t["title"])
    ))]));
    lines.push(Line::from(vec![
        Span::raw("  Estado: "),
        colored(status.clone(), status_color(&status)),
    ]));
    if let Some(r) = t["status_reason"].as_str() {
        lines.push(Line::from(dim(format!("  {r}"))));
    }
    if let Some(m) = t["milestone"].as_str() {
        lines.push(Line::raw(format!("  Milestone: {m}")));
    }
    match (t["agent_number"].as_i64(), t["agent_state"].as_str()) {
        (Some(n), Some(s)) => {
            lines.push(Line::raw(format!("  Agente: #{n} ({s}) — a para abrirlo")))
        }
        _ => lines.push(Line::from(dim("  Sin agente todavía."))),
    }
    if let Some(m) = t["merge_status"].as_str() {
        lines.push(Line::raw(format!("  Integración: {m}")));
    }
    lines.push(Line::raw(""));
    lines.push(section("DEPENDENCIAS"));
    let deps = app
        .task_deps
        .as_ref()
        .and_then(|d| d["dependencies"].as_array().cloned())
        .unwrap_or_default();
    if deps.is_empty() {
        lines.push(Line::from(dim("  Ninguna.")));
    }
    for d in deps {
        let s = text(&d["status"]);
        lines.push(Line::from(vec![
            Span::raw(format!(
                "  {} {:<28} ",
                text(&d["code"]),
                truncate(&text(&d["title"]), 28)
            )),
            colored(s.clone(), status_color(&s)),
            dim(if s == "DONE" { "" } else { "  ← la bloquea" }),
        ]));
    }
    paragraph(f, area, "Tarea", lines);
}

/// 17 Milestone Review.
fn milestones(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    if app.milestones.is_empty() {
        lines.push(Line::from(dim(
            "No hay milestones: un lote delegado (`batch.create`) crea uno.",
        )));
    }
    for (i, m) in app.milestones.iter().enumerate() {
        let status = text(&m["status"]);
        let gate = if m["requires_approval"].as_bool().unwrap_or(false) {
            "con aprobación"
        } else {
            "sin aprobación"
        };
        lines.push(Line::from(vec![
            marker(i == app.selected),
            bold(truncate(&text(&m["name"]), 34)),
            Span::raw(format!(
                "  {}/{} tareas  ",
                num(&m["tasks_done"]),
                num(&m["tasks_total"])
            )),
            colored(format!("{status:<18}"), status_color(&status)),
            dim(gate),
        ]));
        if status == "IN_REVIEW" && i == app.selected {
            lines.push(Line::from(dim(
                "      Gate Review: a aprobar · c pedir cambios · h retener",
            )));
        }
    }
    if let Some((name, approval)) = &app.milestone_input {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            bold("Nuevo milestone: "),
            Span::raw(format!("{name}▏")),
        ]));
        lines.push(Line::from(dim(format!(
            "  Tab: {} · Enter crear · Esc cancelar",
            if *approval {
                "con aprobación"
            } else {
                "sin aprobación"
            }
        ))));
    }
    paragraph(f, area, "Milestones", lines);
}

fn bar(pct: f64, width: usize) -> String {
    let filled = ((pct.clamp(0.0, 100.0) / 100.0) * width as f64).round() as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

/// 18 System / Resources.
fn system(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(s) = app.system.as_ref() else {
        lines.push(Line::from(dim("Cargando…")));
        return paragraph(f, area, "Sistema", lines);
    };
    lines.push(Line::from(vec![
        Span::raw("Perfil: "),
        bold(text(&s["profile"])),
        dim("   p para cambiarlo"),
    ]));
    lines.push(Line::raw(""));
    let metric = |name: &str, value: Option<f64>, target: Option<f64>| -> Line<'static> {
        match value {
            Some(v) => {
                let over = target.is_some_and(|t| v > t);
                Line::from(vec![
                    Span::raw(format!("  {name:<5} ")),
                    colored(
                        bar(v, 20),
                        if over {
                            current_theme().error
                        } else {
                            current_theme().ok
                        },
                    ),
                    Span::raw(format!(" {v:>3.0}%")),
                    dim(target.map_or(String::new(), |t| format!(" / objetivo {t:.0}%"))),
                ])
            }
            None => Line::from(dim(format!("  {name:<5} sin muestra todavía"))),
        }
    };
    lines.push(metric(
        "CPU",
        s["cpu_pct"].as_f64(),
        s["cpu_ceiling_pct"].as_f64(),
    ));
    lines.push(metric(
        "RAM",
        s["ram_pct"].as_f64(),
        s["ram_ceiling_pct"].as_f64(),
    ));
    lines.push(Line::raw(format!(
        "  Slots pesados {} / {}",
        num(&s["heavy_slots_used"]),
        num(&s["heavy_slots_total"])
    )));
    lines.push(Line::raw(""));
    lines.push(section("EJECUTANDO"));
    let running = s["running"].as_array().cloned().unwrap_or_default();
    if running.is_empty() {
        lines.push(Line::from(dim("  Nada pesado corriendo.")));
    }
    for r in running {
        lines.push(Line::raw(format!(
            "  {} · clase {} · {}",
            text(&r["agent"]),
            num(&r["class"]),
            truncate(&text(&r["command"]), 50)
        )));
    }
    lines.push(Line::raw(""));
    lines.push(section("EN COLA"));
    let waiting = s["waiting"].as_array().cloned().unwrap_or_default();
    if waiting.is_empty() {
        lines.push(Line::from(dim("  Nadie espera.")));
    }
    for w in waiting {
        lines.push(Line::raw(format!(
            "  {} · {}",
            text(&w["agent"]),
            truncate(&text(&w["command"]), 50)
        )));
        lines.push(Line::from(dim(format!("      {}", text(&w["why"])))));
    }
    paragraph(f, area, "Sistema", lines);
}

/// 19 Performance Profile.
fn performance_profile(app: &App, f: &mut Frame, area: Rect) {
    let current = app
        .system
        .as_ref()
        .and_then(|s| s["profile"].as_str())
        .unwrap_or("");
    let describe = |id: &str| match id {
        "ECO" => "1 proceso pesado, CPU al 50 % y RAM al 60 %: la máquina casi no lo nota.",
        "BALANCED" => "1 proceso pesado, CPU y RAM al 70 %: trabajar mientras los agentes corren.",
        "PERFORMANCE" => {
            "3 procesos pesados, CPU al 90 % y RAM al 85 %: la máquina es de los agentes."
        }
        _ => "Los valores de `[performance]` en config.toml (max_heavy y techos).",
    };
    let mut lines: Vec<Line<'static>> = vec![
        Line::from(dim("El cambio vale al instante para lo que está en cola.")),
        Line::raw(""),
    ];
    for (i, (id, name)) in PERFORMANCE.iter().enumerate() {
        let mark = if i == app.perf_sel { "(•)" } else { "( )" };
        let now = if *id == current { "  ← vigente" } else { "" };
        lines.push(Line::from(vec![
            marker(i == app.perf_sel),
            if i == app.perf_sel {
                colored(format!("{mark} {name}"), current_theme().brand)
            } else {
                Span::raw(format!("{mark} {name}"))
            },
            dim(now),
        ]));
        lines.push(Line::from(dim(format!("      {}", describe(id)))));
    }
    paragraph(f, area, "Perfil de rendimiento", lines);
}

/// 28 Validation.
fn validation(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(v) = app.validation.as_ref() else {
        lines.push(Line::from(dim("Cargando…")));
        return paragraph(f, area, "Validación", lines);
    };
    let runs = v["runs"].as_array().cloned().unwrap_or_default();
    if runs.is_empty() {
        lines.push(Line::from(dim(
            "Todavía no se validó esta tarea. r para correr la validación del proyecto.",
        )));
    }
    // Más nuevo primero: cada tier muestra solo su última pasada.
    let mut shown: Vec<i64> = Vec::new();
    let mut by_tier: Vec<&Value> = Vec::new();
    for r in &runs {
        let tier = r["tier"].as_i64().unwrap_or(0);
        if !shown.contains(&tier) {
            shown.push(tier);
            by_tier.push(r);
        }
    }
    by_tier.sort_by_key(|r| r["tier"].as_i64().unwrap_or(0));
    for r in by_tier {
        let status = text(&r["status"]);
        lines.push(Line::from(vec![
            bold(format!("Tier {}", num(&r["tier"]))),
            Span::raw("  "),
            colored(status.clone(), status_color(&status)),
            dim(format!("  ({})", text(&r["trigger"]))),
        ]));
        for c in r["checks"].as_array().cloned().unwrap_or_default() {
            let s = text(&c["status"]);
            let counts = match (c["passed"].as_i64(), c["failed"].as_i64()) {
                (Some(p), Some(fl)) => format!("  {p} ok · {fl} fallos"),
                _ => String::new(),
            };
            lines.push(Line::from(vec![
                Span::raw(format!("  {:<16} ", text(&c["name"]))),
                colored(s.clone(), status_color(&s)),
                dim(counts),
            ]));
            if s != "PASS"
                && let Some(sum) = c["summary"].as_str()
            {
                for l in sum.lines().take(4) {
                    lines.push(Line::from(dim(format!("      {}", truncate(l, 80)))));
                }
            }
        }
        lines.push(Line::raw(""));
    }
    if !shown.contains(&3) && !runs.is_empty() {
        lines.push(Line::from(dim(
            "Tier 3: todavía no se requiere (corre al integrar).",
        )));
    }
    paragraph(f, area, "Validación", lines);
}

fn flow_agent_label(app: &App) -> String {
    app.flow_agent
        .as_ref()
        .and_then(|id| {
            app.agents
                .iter()
                .find(|a| a["agent_id"].as_str() == Some(id.as_str()))
        })
        .and_then(|a| a["number"].as_i64())
        .map_or_else(String::new, |n| format!(" · Agente #{n}"))
}

/// 26 Merge Preflight.
fn merge_preflight(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(m) = app.merge.as_ref() else {
        lines.push(Line::from(dim("Cargando…")));
        return paragraph(f, area, "Integración", lines);
    };
    let mr = &m["merge_request"];
    if mr.is_null() {
        lines.push(Line::from(dim(
            "Todavía no hay una solicitud de integración. p para correr el preflight.",
        )));
        return paragraph(
            f,
            area,
            &format!("Integración{}", flow_agent_label(app)),
            lines,
        );
    }
    let status = text(&mr["status"]);
    lines.push(Line::from(vec![
        Span::raw("Destino: "),
        bold(text(&mr["target_branch"])),
        Span::raw("   Estado: "),
        colored(status.clone(), status_color(&status)),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(dim(format!(
        "  {:<14} {:<7} {}",
        "CHECK", "ESTADO", "DETALLE"
    ))));
    for c in mr["preflight"]["checks"]
        .as_array()
        .cloned()
        .unwrap_or_default()
    {
        let s = text(&c["status"]);
        lines.push(Line::from(vec![
            Span::raw(format!("  {:<14} ", text(&c["name"]))),
            colored(format!("{s:<7} "), status_color(&s)),
            Span::raw(truncate(&text(&c["detail"]), 56)),
        ]));
    }
    lines.push(Line::raw(""));
    let requires = mr["requires_approval"].as_bool().unwrap_or(true);
    let approved = !mr["approved_at"].is_null();
    lines.push(Line::raw(format!(
        "  Política: {}",
        match (requires, approved) {
            (false, _) => "integración automática permitida",
            (true, true) => "aprobada por ti",
            (true, false) => "necesita tu aprobación (a)",
        }
    )));
    if !mr["merge_commit"].is_null() {
        lines.push(Line::raw(format!(
            "  Integrada en el commit {}",
            truncate(&text(&mr["merge_commit"]), 10)
        )));
    }
    let conflicts = m["conflicts"].as_array().map_or(0, Vec::len);
    if conflicts > 0 {
        lines.push(Line::from(colored(
            format!("  {conflicts} archivo(s) en conflicto: c para revisarlos."),
            current_theme().error,
        )));
    }
    if let Some(r) = m["reviews"].as_array().and_then(|r| r.first()) {
        lines.push(Line::raw(format!(
            "  Review: {} ({}) — y para ver los hallazgos",
            text(&r["status"]),
            r["recommendation"].as_str().unwrap_or("sin recomendación")
        )));
    }
    paragraph(
        f,
        area,
        &format!("Integración{}", flow_agent_label(app)),
        lines,
    );
}

/// 27 Conflict Review.
fn conflict_review(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let conflicts = app
        .merge
        .as_ref()
        .and_then(|m| m["conflicts"].as_array().cloned())
        .unwrap_or_default();
    if conflicts.is_empty() {
        lines.push(Line::from(dim("Sin conflictos.")));
    } else {
        lines.push(Line::from(colored(
            "La tarea no entra limpia en la rama del chat.",
            current_theme().error,
        )));
        lines.push(Line::raw(""));
        lines.push(Line::from(dim(format!(
            "  {:<34} {:<14} {}",
            "ARCHIVO", "CHOCA CON", "RESOLUCIÓN"
        ))));
    }
    for c in conflicts {
        let other = c["other_agent_id"]
            .as_str()
            .and_then(|id| {
                app.agents
                    .iter()
                    .find(|a| a["agent_id"].as_str() == Some(id))
                    .and_then(|a| a["number"].as_i64())
            })
            .map_or("—".to_string(), |n| format!("Agente #{n}"));
        lines.push(Line::raw(format!(
            "  {:<34} {:<14} {}",
            truncate(&text(&c["path"]), 34),
            other,
            text(&c["resolution"])
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(dim(
        "  Resolver a mano: integra la rama del chat en la del agente, arregla y vuelve (p).",
    )));
    paragraph(
        f,
        area,
        &format!("Conflictos{}", flow_agent_label(app)),
        lines,
    );
}

/// 29 Review Agent Result.
fn review_result(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let reviews = app
        .merge
        .as_ref()
        .and_then(|m| m["reviews"].as_array().cloned())
        .unwrap_or_default();
    if reviews.is_empty() {
        lines.push(Line::from(dim(
            "Todavía no hay una revisión de esta tarea. w para pedirla.",
        )));
    }
    for r in reviews {
        let status = text(&r["status"]);
        let rec = r["recommendation"].as_str().unwrap_or("—").to_string();
        lines.push(Line::from(vec![
            bold("Revisión "),
            colored(status.clone(), status_color(&status)),
            Span::raw("  recomienda "),
            colored(rec.clone(), status_color(&rec)),
        ]));
        let findings = r["findings"].as_array().cloned().unwrap_or_default();
        if findings.is_empty() && status == "DONE" {
            lines.push(Line::from(dim("  Sin hallazgos.")));
        }
        for fi in findings {
            lines.push(Line::raw(format!(
                "  [{}] {}: {}",
                text(&fi["severity"]),
                text(&fi["file"]),
                truncate(&text(&fi["text"]), 60)
            )));
        }
        if let Some(e) = r["findings"]["error"].as_str() {
            lines.push(Line::from(colored(format!("  {e}"), current_theme().error)));
        }
        lines.push(Line::raw(""));
    }
    lines.push(Line::from(dim(
        "Un review solo recomienda: la decisión de integrar es tuya.",
    )));
    paragraph(
        f,
        area,
        &format!("Revisión{}", flow_agent_label(app)),
        lines,
    );
}

// --- Help --------------------------------------------------------------------

fn help(app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        bold("Symphony"),
        Span::raw(format!("  v{}", crate::app::SYMPHONY_VERSION)),
    ]));
    if !app.news.is_empty() {
        lines.push(Line::raw(""));
        lines.push(section("ANUNCIOS LOCALES (.symphony/anuncios.md)"));
        for n in &app.news {
            lines.push(Line::from(dim(n.clone())));
        }
    }
    lines.push(Line::raw(""));
    lines.push(section("ATAJOS"));
    for (k, v) in [
        (
            "?",
            "abre esta ayuda (en cualquier pantalla salvo el agente)",
        ),
        ("Esc / q", "volver o salir"),
        ("j/k o ↑/↓", "mover la selección"),
        ("Enter", "abrir / confirmar"),
        (": o /", "barra de comandos"),
    ] {
        lines.push(Line::from(vec![bold(format!("{:<12}", k)), Span::raw(v)]));
    }
    lines.push(Line::raw(""));
    lines.push(section("HOME"));
    for (k, v) in [
        ("n", "nuevo agente"),
        ("t", "tareas"),
        ("m", "milestones"),
        ("s", "sistema"),
        ("p", "proveedores"),
        ("r", "recovery"),
        ("c", "contexto"),
    ] {
        lines.push(Line::from(vec![bold(format!("{:<12}", k)), Span::raw(v)]));
    }
    lines.push(Line::raw(""));
    lines.push(section("CHAT / AGENTE"));
    for (k, v) in [
        ("Tab", "cambiar modelo"),
        ("^Y", "copiar"),
        ("^E", "exportar"),
        ("^R", "revisar"),
        ("^G", "integrar a main"),
        ("F2", "ratón"),
        ("m", "mensaje"),
        ("p", "pausa"),
        ("s", "modelo"),
        ("v", "validación"),
        ("g", "integrar"),
        ("d", "diff"),
        ("x", "detener"),
    ] {
        lines.push(Line::from(vec![bold(format!("{:<12}", k)), Span::raw(v)]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(dim(
        "Sin telemetría: los avisos se leen solo del repo local.",
    )));
    lines.push(Line::from(dim("Esc para cerrar.")));
    paragraph(f, area, "Atajos", lines);
}
