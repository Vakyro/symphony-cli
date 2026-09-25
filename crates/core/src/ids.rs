//! IDs de entidades: ULID en texto (DB §1, convenciones). Cada entidad tiene
//! su propio tipo para que no se puedan mezclar.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("ID inválido `{value}`: no es un ULID")]
pub struct InvalidId {
    pub value: String,
}

macro_rules! ulid_id {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(Ulid);

        impl $name {
            pub fn new() -> Self {
                Self(Ulid::generate())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = InvalidId;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ulid::from_string(s).map(Self).map_err(|_| InvalidId { value: s.to_string() })
            }
        }

        impl TryFrom<String> for $name {
            type Error = InvalidId;
            fn try_from(s: String) -> Result<Self, Self::Error> {
                s.parse()
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.to_string()
            }
        }
    )+};
}

ulid_id!(
    ProjectId,
    SessionId,
    TaskId,
    AgentId,
    RunId,
    WorktreeId,
    CheckpointId,
    RecoveryItemId,
    MessageId,
    ToolCallId,
    ContextObjectId,
    HandoffId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_text_and_json() {
        let id = AgentId::new();
        let text = id.to_string();
        assert_eq!(text.len(), 26);
        assert_eq!(text.parse::<AgentId>().unwrap(), id);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<AgentId>(&json).unwrap(), id);
    }

    #[test]
    fn rejects_non_ulid() {
        assert!("agent-3".parse::<AgentId>().is_err());
        assert!(serde_json::from_str::<TaskId>("\"not-a-ulid\"").is_err());
    }

    #[test]
    fn ids_sort_by_creation_time() {
        let a = RunId::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = RunId::new();
        assert!(a < b);
    }
}
