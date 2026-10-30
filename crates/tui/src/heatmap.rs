//! Heatmap mensual de uso (P17.S6): días × semanas estilo GitHub, con la intensidad
//! de cada día proporcional a sus tokens. Lógica pura: la fecha se recibe como
//! `YYYY-MM-DD` (lo que devuelve `usage.global`) y no hay ni E/S ni dependencia de
//! la hora del sistema.

/// Día del calendario con su uso y su nivel de intensidad (0 = sin uso, 4 = el pico del mes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Day {
    pub date: String,
    pub tokens: u64,
    pub level: u8,
}

/// Una columna de la rejilla: las 7 celdas de lunes a domingo. `None` es un día
/// que queda fuera del mes (la primera o la última semana, cortadas).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Week(pub [Option<Day>; 7]);

/// Nivel de intensidad (0–4) relativo al máximo del mes: sin uso 0, y el resto
/// repartido en cuatro escalones proporcionales (el pico del mes siempre es 4).
pub fn level(tokens: u64, max: u64) -> u8 {
    if tokens == 0 || max == 0 {
        return 0;
    }
    u8::try_from(tokens.saturating_mul(4) / max.max(1))
        .unwrap_or(4)
        .clamp(1, 4)
}

/// Glifo de cada nivel: se distingue en cualquier terminal (también sin color).
pub const GLYPHS: [char; 5] = ['·', '░', '▒', '▓', '█'];

pub fn glyph(l: u8) -> char {
    GLYPHS[usize::from(l.min(4))]
}

/// Iniciales de los días de la semana (lunes primero, como en GitHub).
pub const WEEKDAYS: [&str; 7] = ["L", "M", "X", "J", "V", "S", "D"];

/// Nombre del mes en español (el status global se pinta en español).
pub fn month_label(year: i32, month: u32) -> String {
    const NAMES: [&str; 12] = [
        "enero",
        "febrero",
        "marzo",
        "abril",
        "mayo",
        "junio",
        "julio",
        "agosto",
        "septiembre",
        "octubre",
        "noviembre",
        "diciembre",
    ];
    let name = NAMES
        .get(month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("?");
    format!("{name} {year}")
}

/// `YYYY-MM-DD` → `(año, mes, día)`; `None` si el texto no tiene esa forma.
pub fn parse_date(date: &str) -> Option<(i32, u32, u32)> {
    let (y, rest) = date.split_once('-')?;
    let (m, d) = rest.split_once('-')?;
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    Some((y.parse().ok()?, m.parse().ok()?, d.parse().ok()?))
}

/// Día de la semana (0 = lunes … 6 = domingo), algoritmo de Sakamoto. Sin dependencias.
pub fn weekday(year: i32, month: u32, day: u32) -> u32 {
    let t = [0i32, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    let idx =
        (y + y / 4 - y / 100 + y / 400 + t[(month.clamp(1, 12) - 1) as usize] + day.min(31) as i32)
            as u64;
    u32::try_from(((idx % 7) + 6) % 7).unwrap_or(0) // Sakamoto da 0 = domingo; lo pasamos a 0 = lunes
}

/// Días que tiene un mes (bisiestos incluidos).
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

/// Rejilla del mes: semanas en columnas (lunes a domingo) y un `Day` por celda.
/// `daily` se consulta por fecha; los días sin entrada valen 0. Devuelve las
/// semanas completas que tocan al mes, con `None` fuera de él.
pub fn month_grid(
    year: i32,
    month: u32,
    daily: &std::collections::BTreeMap<String, u64>,
) -> Vec<Week> {
    let max = daily.values().copied().max().unwrap_or(0);
    let dim = days_in_month(year, month);
    let first_row = weekday(year, month, 1);
    let total = first_row + dim;
    let weeks = total.div_ceil(7);
    let mut out: Vec<Week> = Vec::with_capacity(weeks as usize);
    for w in 0..weeks {
        let mut cells: [Option<Day>; 7] = [None, None, None, None, None, None, None];
        for (row, cell) in cells.iter_mut().enumerate() {
            let day = (w * 7 + row as u32 + 1) as i64 - first_row as i64;
            if day >= 1 && day <= dim as i64 {
                let date = format!("{year:04}-{month:02}-{day:02}");
                let tokens = daily.get(&date).copied().unwrap_or(0);
                *cell = Some(Day {
                    date,
                    tokens,
                    level: level(tokens, max),
                });
            }
        }
        out.push(Week(cells));
    }
    out
}

/// Año y mes de una lista de fechas `YYYY-MM-DD`; `None` si no hay ninguna válida.
pub fn month_of(dates: &[String]) -> Option<(i32, u32)> {
    dates
        .iter()
        .find_map(|d| parse_date(d))
        .map(|(y, m, _)| (y, m))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Vec<Week> {
        // Octubre 2026: 31 días, empieza en jueves (fila 3).
        let mut daily = std::collections::BTreeMap::new();
        daily.insert("2026-10-02".into(), 10_000u64);
        daily.insert("2026-10-29".into(), 40_000);
        month_grid(2026, 10, &daily)
    }

    #[test]
    fn levels_are_relative_to_the_month_peak() {
        assert_eq!(level(0, 40_000), 0);
        assert_eq!(level(40_000, 40_000), 4);
        assert_eq!(level(10_000, 40_000), 1);
        assert_eq!(level(20_000, 40_000), 2);
        assert_eq!(level(30_000, 40_000), 3);
        assert_eq!(level(1, 0), 0);
    }

    #[test]
    fn glyphs_cover_all_levels() {
        assert_eq!(GLYPHS, ['·', '░', '▒', '▓', '█']);
        assert_eq!(glyph(9), '█', "los niveles se acotan");
    }

    #[test]
    fn weekday_is_monday_first() {
        // 2026-10-01 fue jueves → fila 3; 2026-10-05 lunes → fila 0.
        assert_eq!(weekday(2026, 10, 1), 3);
        assert_eq!(weekday(2026, 10, 5), 0);
        assert_eq!(weekday(2026, 10, 4), 6);
        // Referencia: 2000-01-01 fue sábado.
        assert_eq!(weekday(2000, 1, 1), 5);
    }

    #[test]
    fn february_respects_leap_years() {
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2000, 2), 29);
    }

    #[test]
    fn the_grid_covers_the_whole_month() {
        let weeks = grid();
        assert_eq!(weeks.len(), 5);
        let cells: Vec<&Day> = weeks.iter().flat_map(|w| w.0.iter().flatten()).collect();
        assert_eq!(cells.len(), 31);
        assert_eq!(cells[0].date, "2026-10-01");
        assert_eq!(cells[30].date, "2026-10-31");
        // La primera semana corta los tres días de septiembre (L, M, X).
        assert!(weeks[0].0[0].is_none() && weeks[0].0[1].is_none() && weeks[0].0[2].is_none());
        assert!(weeks[0].0[3].is_some(), "jueves 1");
    }

    #[test]
    fn tokens_land_on_their_day_with_the_right_level() {
        let weeks = grid();
        let d2 = weeks[0].0[4].as_ref().expect("viernes 2");
        assert_eq!((d2.tokens, d2.level), (10_000, 1));
        let d29 = weeks[4].0[3].as_ref().expect("jueves 29");
        assert_eq!((d29.tokens, d29.level), (40_000, 4));
        let d3 = weeks[0].0[5].as_ref().expect("sábado 3");
        assert_eq!(d3.level, 0);
    }

    #[test]
    fn month_of_takes_the_first_parseable_date() {
        assert_eq!(month_of(&["2026-10-02".into()]), Some((2026, 10)));
        assert!(month_of(&["mal".into()]).is_none());
        assert!(month_of(&[]).is_none());
    }

    #[test]
    fn labels_are_spanish() {
        assert_eq!(month_label(2026, 10), "octubre 2026");
        assert_eq!(month_label(2026, 13), "? 2026");
        assert_eq!(parse_date("2026-1-2"), None);
        assert_eq!(parse_date("2026-01-02"), Some((2026, 1, 2)));
    }
}
