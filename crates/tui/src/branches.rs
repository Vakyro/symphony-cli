//! Vista de ramas de git (P17.S6): línea por rama con su estado y la máquina de
//! confirmaciones del borrado. Cambiar de rama no pide confirmación; borrar siempre,
//! y una rama no integrada solo se fuerza tras una segunda confirmación explícita.

use serde_json::Value;

/// En qué paso del borrado está una confirmación pendiente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteStep {
    /// Primera confirmación: pregunta si se borra (git lo hará sin forzar).
    Confirm,
    /// Segunda confirmación: la rama no está integrada y el usuario insiste en forzar.
    Force,
}

/// Borrado pedido y aún no confirmado (caduca con cualquier otra tecla, como el aviso).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDelete {
    pub branch: String,
    pub step: DeleteStep,
}

/// Qué hay que hacer al pulsar `d` sobre la rama elegida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteAction {
    /// Pedir confirmación (guarda este pendiente y avisa).
    Ask(PendingDelete),
    /// Confirmado: mandar `git.delete_branch` con este `force`.
    Delete { branch: String, force: bool },
    /// No se puede ni se debe hacer nada (sin selección o rama actual).
    Cancel,
}

/// Transición de la tecla `d`: `pending` es lo que había pedido antes (si sigue en pie),
/// `branch` la rama seleccionada ahora y `current` si es la rama en uso (no se borra).
pub fn delete_key(
    pending: Option<&PendingDelete>,
    branch: Option<&str>,
    current: bool,
) -> DeleteAction {
    let Some(name) = branch.filter(|b| !b.is_empty()) else {
        return DeleteAction::Cancel;
    };
    if current {
        return DeleteAction::Cancel;
    }
    match pending {
        None => DeleteAction::Ask(PendingDelete {
            branch: name.into(),
            step: DeleteStep::Confirm,
        }),
        Some(p) if p.branch == name && p.step == DeleteStep::Confirm => DeleteAction::Delete {
            branch: name.into(),
            force: false,
        },
        Some(p) if p.branch == name && p.step == DeleteStep::Force => DeleteAction::Delete {
            branch: name.into(),
            force: true,
        },
        // Otra rama en medio: la confirmación anterior caduca y empieza la de esta.
        Some(_) => DeleteAction::Ask(PendingDelete {
            branch: name.into(),
            step: DeleteStep::Confirm,
        }),
    }
}

/// Aviso de la primera confirmación.
pub fn confirm_notice(branch: &str) -> String {
    format!("Pulsa d otra vez para borrar `{branch}`.")
}

/// Aviso cuando git dice que la rama no está integrada: fuerza la segunda confirmación.
pub fn not_merged_notice(branch: &str) -> String {
    format!(
        "La rama `{branch}` no está integrada en HEAD: pulsa d otra vez para forzar el borrado (sus commits se perderán)."
    )
}

/// Etiqueta del estado de la rama: la actual, integrada o pendiente de integrar.
pub fn state_label(branch: &Value) -> (&'static str, bool) {
    if branch["current"].as_bool().unwrap_or(false) {
        ("actual", true)
    } else if branch["merged"].as_bool().unwrap_or(false) {
        ("integrada", false)
    } else {
        ("sin integrar", false)
    }
}

/// Divergencia con el upstream: `+2 −1`; `—` si no tiene upstream (ni ella, en ese caso).
pub fn divergence(branch: &Value) -> String {
    let ahead = branch["ahead"].as_u64().unwrap_or(0);
    let behind = branch["behind"].as_u64().unwrap_or(0);
    match (ahead, behind) {
        (0, 0) => "—".into(),
        (a, 0) => format!("+{a}"),
        (0, b) => format!("−{b}"),
        (a, b) => format!("+{a} −{b}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_first_d_asks_and_the_second_deletes_without_force() {
        let ask = delete_key(None, Some("f/x"), false);
        let DeleteAction::Ask(p) = &ask else {
            panic!("{ask:?}");
        };
        assert_eq!(p.step, DeleteStep::Confirm);
        let del = delete_key(Some(p), Some("f/x"), false);
        assert_eq!(
            del,
            DeleteAction::Delete {
                branch: "f/x".into(),
                force: false
            }
        );
    }

    #[test]
    fn a_not_merged_branch_needs_the_second_confirmation_with_force() {
        // Tras el aviso de git, el pendiente vuelve en paso Force: la segunda `d` fuerza.
        let pending = PendingDelete {
            branch: "f/x".into(),
            step: DeleteStep::Force,
        };
        assert_eq!(
            delete_key(Some(&pending), Some("f/x"), false),
            DeleteAction::Delete {
                branch: "f/x".into(),
                force: true
            }
        );
    }

    #[test]
    fn switching_branches_resets_the_pending_confirmation() {
        let p = PendingDelete {
            branch: "f/x".into(),
            step: DeleteStep::Confirm,
        };
        let ask = delete_key(Some(&p), Some("otra"), false);
        let DeleteAction::Ask(new) = ask else {
            panic!("{ask:?}");
        };
        assert_eq!(new.branch, "otra");
        assert_eq!(new.step, DeleteStep::Confirm);
    }

    #[test]
    fn the_current_branch_and_empty_selection_cannot_be_deleted() {
        assert_eq!(delete_key(None, Some("main"), true), DeleteAction::Cancel);
        assert_eq!(delete_key(None, None, false), DeleteAction::Cancel);
        assert_eq!(delete_key(None, Some(""), false), DeleteAction::Cancel);
        // Ni aunque quede un pendiente viejo de otra rama.
        let p = PendingDelete {
            branch: "f/x".into(),
            step: DeleteStep::Force,
        };
        assert_eq!(
            delete_key(Some(&p), Some("main"), true),
            DeleteAction::Cancel
        );
    }

    #[test]
    fn labels_and_divergence() {
        let current = json!({"current": true, "merged": true});
        assert_eq!(state_label(&current), ("actual", true));
        let merged = json!({"current": false, "merged": true});
        assert_eq!(state_label(&merged), ("integrada", false));
        let open = json!({"current": false, "merged": false});
        assert_eq!(state_label(&open), ("sin integrar", false));

        assert_eq!(divergence(&json!({"ahead": 0, "behind": 0})), "—");
        assert_eq!(divergence(&json!({"ahead": 2, "behind": 0})), "+2");
        assert_eq!(divergence(&json!({"ahead": 0, "behind": 1})), "−1");
        assert_eq!(divergence(&json!({"ahead": 2, "behind": 1})), "+2 −1");
        assert_eq!(divergence(&Value::Null), "—");

        assert!(not_merged_notice("f/x").contains("no está integrada"));
        assert!(confirm_notice("f/x").contains("otra vez"));
    }
}
