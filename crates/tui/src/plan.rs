//! Avance de agentes en el plan del proyecto (P17.S6): el % del plan son las tareas
//! hechas sobre el total (el daemon ya excluye la tarea del chat), las fases son los
//! milestones y cada agente muestra su rama, su tarea y su estado. Lógica pura.

use serde_json::Value;

/// `(hechas, total)` de las tareas del plan (las `DONE` sobre todas las listadas).
pub fn progress(tasks: &[Value]) -> (usize, usize) {
    let total = tasks.len();
    let done = tasks
        .iter()
        .filter(|t| t["status"].as_str() == Some("DONE"))
        .count();
    (done, total)
}

/// Porcentaje redondeado (0–100) del avance; sin tareas, 0.
pub fn percent(done: usize, total: usize) -> u8 {
    if total == 0 {
        return 0;
    }
    u8::try_from(done.saturating_mul(100) / total).unwrap_or(100)
}

/// Barra de texto de `width` caracteres: lo hecho relleno, lo demás vacío.
pub fn bar(done: usize, total: usize, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let filled = done
        .saturating_mul(width)
        .checked_div(total)
        .unwrap_or(0)
        .min(width);
    format!("{}{}", "█".repeat(filled), "─".repeat(width - filled))
}

/// Fase de una tarea: su milestone o `Sin fase` cuando no tiene.
pub fn phase_of(task: &Value) -> String {
    task["milestone"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| "Sin fase".into())
}

/// Fases del plan con su avance, en orden alfabético: `(nombre, hechas, total)`.
pub fn phases(tasks: &[Value]) -> Vec<(String, usize, usize)> {
    let mut out: Vec<(String, usize, usize)> = Vec::new();
    for t in tasks {
        let name = phase_of(t);
        let done = usize::from(t["status"].as_str() == Some("DONE"));
        match out.iter_mut().find(|(n, _, _)| *n == name) {
            Some((_, d, total)) => {
                *d += done;
                *total += 1;
            }
            None => out.push((name, done, 1)),
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn task(status: &str, milestone: Option<&str>) -> Value {
        json!({"status": status, "milestone": milestone})
    }

    #[test]
    fn progress_counts_done_over_all_tasks() {
        let tasks = [
            task("DONE", Some("P1")),
            task("RUNNING", Some("P1")),
            task("DONE", Some("P2")),
            task("BACKLOG", None),
        ];
        assert_eq!(progress(&tasks), (2, 4));
        assert_eq!(percent(2, 4), 50);
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(3, 3), 100);
        assert_eq!(percent(1, 3), 33, "redondea hacia abajo");
    }

    #[test]
    fn the_bar_reflects_the_ratio_and_width() {
        assert_eq!(bar(1, 2, 10), "█████─────");
        assert_eq!(bar(0, 0, 10), "──────────");
        assert_eq!(bar(4, 4, 6), "██████");
        assert_eq!(bar(3, 4, 0), "");
        assert_eq!(bar(9, 4, 6), "██████", "nunca pasa del ancho");
    }

    #[test]
    fn phases_group_tasks_by_milestone_sorted() {
        let tasks = [
            task("DONE", Some("P2 · TUI")),
            task("RUNNING", Some("P1 · Core")),
            task("DONE", Some("P1 · Core")),
            task("BACKLOG", None),
        ];
        assert_eq!(
            phases(&tasks),
            vec![
                ("P1 · Core".to_string(), 1, 2),
                ("P2 · TUI".to_string(), 1, 1),
                ("Sin fase".to_string(), 0, 1),
            ]
        );
    }
}
