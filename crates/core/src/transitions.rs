//! Transiciones válidas de `AgentState` y `TaskStatus` como funciones puras.
//!
//! FLOW §7 y §9.2 listan los estados, pero no traen una tabla de transiciones.
//! Esta tabla sale de su semántica (FLOW §6, §9.3, §10.2; IDEA §5.10) y está
//! registrada como decisión en la bitácora de P02. Un estado nunca pasa a sí mismo.

use crate::enums::{AgentState, TaskStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("transición inválida de {kind}: {from} → {to}")]
pub struct InvalidTransition {
    pub kind: &'static str,
    pub from: &'static str,
    pub to: &'static str,
}

impl AgentState {
    pub fn can_transition_to(self, to: Self) -> bool {
        use AgentState::*;
        let allowed: &[AgentState] = match self {
            // Si el workspace no se puede preparar, el agente no queda "medio roto" (FLOW §6).
            Created => &[Ready, Failed, Cancelled],
            Ready => &[
                Running,
                WaitingProvider,
                WaitingDependency,
                Paused,
                Cancelled,
            ],
            Running => &[
                WaitingProvider,
                WaitingResource,
                Testing,
                Blocked,
                Paused,
                Completed,
                Failed,
                Cancelled,
            ],
            // Failover o cuota recuperada → vuelve a correr. Sin proveedor elegible → decisión del usuario.
            WaitingProvider => &[Running, Blocked, Paused, Failed, Cancelled],
            // Se libera el slot (FLOW §10.2).
            WaitingResource => &[Running, Testing, Paused, Cancelled],
            // Dependencias DONE → READY; una dependencia falla → BLOCKED (FLOW §9.3).
            WaitingDependency => &[Ready, Blocked, Paused, Cancelled],
            Testing => &[
                Running,
                WaitingResource,
                Blocked,
                Paused,
                Completed,
                Failed,
                Cancelled,
            ],
            Blocked => &[Ready, Cancelled, Failed],
            Paused => &[Ready, Running, Cancelled],
            // Reclaim / restart (IDEA §5.10).
            Failed => &[Ready, Cancelled],
            Completed | Cancelled => &[],
        };
        allowed.contains(&to)
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }

    /// FLOW §7: los estados de espera o bloqueo exigen una frase humana (`agents.state_reason`).
    pub fn requires_reason(self) -> bool {
        use AgentState::*;
        matches!(
            self,
            WaitingProvider | WaitingResource | WaitingDependency | Blocked | Failed
        )
    }

    pub fn transition(self, to: Self) -> Result<Self, InvalidTransition> {
        if self.can_transition_to(to) {
            Ok(to)
        } else {
            Err(InvalidTransition {
                kind: "AgentState",
                from: self.as_str(),
                to: to.as_str(),
            })
        }
    }
}

impl TaskStatus {
    pub fn can_transition_to(self, to: Self) -> bool {
        use TaskStatus::*;
        let allowed: &[TaskStatus] = match self {
            Backlog => &[Ready, Waiting, Cancelled],
            Ready => &[Backlog, Running, Waiting, Blocked, Cancelled],
            // Dependencias pendientes: completas → READY, una falla → BLOCKED (FLOW §9.3).
            Waiting => &[Ready, Blocked, Cancelled],
            // Reclaim / reasignación devuelve la task a READY (IDEA §5.10).
            Running => &[Ready, Blocked, Done, Failed, Cancelled],
            // Replanificar, ignorar la dependencia o cancelar (FLOW §9.3).
            Blocked => &[Ready, Waiting, Cancelled],
            Failed => &[Ready, Cancelled],
            Done | Cancelled => &[],
        };
        allowed.contains(&to)
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Cancelled)
    }

    pub fn transition(self, to: Self) -> Result<Self, InvalidTransition> {
        if self.can_transition_to(to) {
            Ok(to)
        } else {
            Err(InvalidTransition {
                kind: "TaskStatus",
                from: self.as_str(),
                to: to.as_str(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::sample::select;

    /// Tabla completa esperada: cada par (from, to) listado es válido; todo lo demás, inválido.
    const AGENT_VALID: &[(&str, &[&str])] = &[
        ("CREATED", &["READY", "FAILED", "CANCELLED"]),
        (
            "READY",
            &[
                "RUNNING",
                "WAITING_PROVIDER",
                "WAITING_DEPENDENCY",
                "PAUSED",
                "CANCELLED",
            ],
        ),
        (
            "RUNNING",
            &[
                "WAITING_PROVIDER",
                "WAITING_RESOURCE",
                "TESTING",
                "BLOCKED",
                "PAUSED",
                "COMPLETED",
                "FAILED",
                "CANCELLED",
            ],
        ),
        (
            "WAITING_PROVIDER",
            &["RUNNING", "BLOCKED", "PAUSED", "FAILED", "CANCELLED"],
        ),
        (
            "WAITING_RESOURCE",
            &["RUNNING", "TESTING", "PAUSED", "CANCELLED"],
        ),
        (
            "WAITING_DEPENDENCY",
            &["READY", "BLOCKED", "PAUSED", "CANCELLED"],
        ),
        (
            "TESTING",
            &[
                "RUNNING",
                "WAITING_RESOURCE",
                "BLOCKED",
                "PAUSED",
                "COMPLETED",
                "FAILED",
                "CANCELLED",
            ],
        ),
        ("BLOCKED", &["READY", "CANCELLED", "FAILED"]),
        ("PAUSED", &["READY", "RUNNING", "CANCELLED"]),
        ("COMPLETED", &[]),
        ("FAILED", &["READY", "CANCELLED"]),
        ("CANCELLED", &[]),
    ];

    const TASK_VALID: &[(&str, &[&str])] = &[
        ("BACKLOG", &["READY", "WAITING", "CANCELLED"]),
        (
            "READY",
            &["BACKLOG", "RUNNING", "WAITING", "BLOCKED", "CANCELLED"],
        ),
        (
            "RUNNING",
            &["READY", "BLOCKED", "DONE", "FAILED", "CANCELLED"],
        ),
        ("WAITING", &["READY", "BLOCKED", "CANCELLED"]),
        ("BLOCKED", &["READY", "WAITING", "CANCELLED"]),
        ("DONE", &[]),
        ("FAILED", &["READY", "CANCELLED"]),
        ("CANCELLED", &[]),
    ];

    #[test]
    fn agent_transition_table_is_exact() {
        assert_eq!(AGENT_VALID.len(), AgentState::ALL.len());
        for &from in AgentState::ALL {
            let (_, valid) = AGENT_VALID
                .iter()
                .find(|(f, _)| *f == from.as_str())
                .unwrap();
            for &to in AgentState::ALL {
                let expected = valid.contains(&to.as_str());
                assert_eq!(from.can_transition_to(to), expected, "{from} → {to}");
                assert_eq!(from.transition(to).is_ok(), expected, "{from} → {to}");
            }
        }
    }

    #[test]
    fn task_transition_table_is_exact() {
        assert_eq!(TASK_VALID.len(), TaskStatus::ALL.len());
        for &from in TaskStatus::ALL {
            let (_, valid) = TASK_VALID
                .iter()
                .find(|(f, _)| *f == from.as_str())
                .unwrap();
            for &to in TaskStatus::ALL {
                let expected = valid.contains(&to.as_str());
                assert_eq!(from.can_transition_to(to), expected, "{from} → {to}");
                assert_eq!(from.transition(to).is_ok(), expected, "{from} → {to}");
            }
        }
    }

    #[test]
    fn every_non_terminal_state_can_end() {
        for &s in AgentState::ALL.iter().filter(|s| !s.is_terminal()) {
            assert!(
                s.can_transition_to(AgentState::Cancelled),
                "{s} no se puede cancelar"
            );
        }
        for &s in TaskStatus::ALL.iter().filter(|s| !s.is_terminal()) {
            assert!(
                s.can_transition_to(TaskStatus::Cancelled),
                "{s} no se puede cancelar"
            );
        }
    }

    proptest! {
        /// Aplicar una secuencia arbitraria: las transiciones inválidas nunca cambian el estado.
        #[test]
        fn invalid_agent_transition_never_changes_state(steps in prop::collection::vec(select(AgentState::ALL), 0..64)) {
            let mut state = AgentState::Created;
            for to in steps {
                let before = state;
                match state.transition(to) {
                    Ok(next) => { prop_assert!(before.can_transition_to(to)); state = next; }
                    Err(e) => {
                        prop_assert!(!before.can_transition_to(to));
                        prop_assert_eq!(state, before);
                        prop_assert_eq!(e.from, before.as_str());
                    }
                }
            }
        }

        #[test]
        fn invalid_task_transition_never_changes_state(steps in prop::collection::vec(select(TaskStatus::ALL), 0..64)) {
            let mut status = TaskStatus::Backlog;
            for to in steps {
                let before = status;
                match status.transition(to) {
                    Ok(next) => status = next,
                    Err(_) => prop_assert_eq!(status, before),
                }
            }
        }
    }
}
