//! Tema de la TUI: paleta de marca (morado apagado). Respeta `NO_COLOR`.

use ratatui::style::Color;

/// Rol semántico de un color dentro del tema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Brand,
    Accent,
    Ok,
    Warn,
    Error,
    Muted,
    Border,
    Text,
}

/// Paleta de marca. Todos los campos son `Option<Color>`: `None` significa
/// "no pintar" (respeta `NO_COLOR` o el color por defecto de la terminal).
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub brand: Option<Color>,
    pub accent: Option<Color>,
    pub ok: Option<Color>,
    pub warn: Option<Color>,
    pub error: Option<Color>,
    pub muted: Option<Color>,
    pub border: Option<Color>,
    pub text: Option<Color>,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            brand: Some(Color::Rgb(140, 120, 180)),
            accent: Some(Color::Rgb(120, 96, 160)),
            ok: Some(Color::Rgb(122, 148, 120)),
            warn: Some(Color::Rgb(190, 160, 110)),
            error: Some(Color::Rgb(178, 108, 108)),
            muted: Some(Color::Rgb(120, 120, 132)),
            border: Some(Color::Rgb(80, 76, 96)),
            text: None,
        }
    }
}

impl Theme {
    /// Todos los campos a `None`: respeta `NO_COLOR`.
    pub fn no_color() -> Self {
        Self {
            brand: None,
            accent: None,
            ok: None,
            warn: None,
            error: None,
            muted: None,
            border: None,
            text: None,
        }
    }

    /// Lee `NO_COLOR`: si está definida y no vacía, devuelve `no_color()`.
    pub fn from_env() -> Self {
        match std::env::var("NO_COLOR") {
            Ok(v) if !v.is_empty() => Self::no_color(),
            _ => Self::default(),
        }
    }

    /// Color del rol pedido, si el tema lo tiene pintado.
    pub fn color(&self, role: Role) -> Option<Color> {
        match role {
            Role::Brand => self.brand,
            Role::Accent => self.accent,
            Role::Ok => self.ok,
            Role::Warn => self.warn,
            Role::Error => self.error,
            Role::Muted => self.muted,
            Role::Border => self.border,
            Role::Text => self.text,
        }
    }
}
