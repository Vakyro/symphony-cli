//! Vista de procesos observados por el daemon, sin control interactivo.
use ratatui::text::Line;
use serde_json::Value;

pub fn rows(value: &Value) -> Vec<&Value> {
    value["processes"]
        .as_array()
        .map_or_else(Vec::new, |items| items.iter().collect())
}

pub fn line(process: &Value) -> Line<'static> {
    let command = process["command"].as_str().unwrap_or("—");
    let state = process["state"].as_str().unwrap_or("—");
    let started = process["started_at"]
        .as_i64()
        .map_or_else(|| "—".into(), |at| at.to_string());
    Line::raw(format!("{state:<10} {started:<14} {command}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_empty_for_missing_payload() {
        assert!(rows(&Value::Null).is_empty());
    }

    #[test]
    fn line_keeps_command_state_and_start() {
        let value = serde_json::json!({"command":"cargo test","state":"RUNNING","started_at":42});
        assert_eq!(
            line(&value).to_string(),
            "RUNNING    42             cargo test"
        );
    }
}
