//! Mensajes del protocolo (STACK §6.2, §46). Todo mensaje lleva `protocol`;
//! una versión desconocida o un campo desconocido es un error explícito.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ProtocolError;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Request(Request),
    Response(Response),
    Event(Event),
    Subscribe(Subscribe),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: u32,
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub protocol: u32,
    /// El `id` del `Request` al que responde.
    pub id: String,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok(Value),
    Error(ErrorBody),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    /// Código estable y legible por máquina, p. ej. `unknown_method`.
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub protocol: u32,
    pub topic: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subscribe {
    pub protocol: u32,
    pub id: String,
    pub topics: Vec<String>,
}

impl Request {
    pub fn new(id: impl Into<String>, method: impl Into<String>, params: Value) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

impl Response {
    pub fn ok(id: impl Into<String>, result: Value) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            id: id.into(),
            outcome: Outcome::Ok(result),
        }
    }

    pub fn error(
        id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let body = ErrorBody {
            code: code.into(),
            message: message.into(),
        };
        Self {
            protocol: PROTOCOL_VERSION,
            id: id.into(),
            outcome: Outcome::Error(body),
        }
    }
}

impl Event {
    pub fn new(topic: impl Into<String>, payload: Value) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            topic: topic.into(),
            payload,
        }
    }
}

impl Subscribe {
    pub fn new(id: impl Into<String>, topics: Vec<String>) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            id: id.into(),
            topics,
        }
    }
}

impl Message {
    pub fn to_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        Ok(serde_json::to_vec(self)?)
    }

    /// Decodifica validando primero la versión, para que un cliente de otra
    /// versión reciba "versión no soportada" y no un error de parseo confuso.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let value: Value = serde_json::from_slice(bytes)?;
        match value.get("protocol").and_then(Value::as_u64) {
            Some(v) if v == u64::from(PROTOCOL_VERSION) => Ok(serde_json::from_value(value)?),
            Some(v) => Err(ProtocolError::UnsupportedVersion { got: v }),
            None => Err(ProtocolError::MissingVersion),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn roundtrip_every_kind() {
        let msgs = [
            Message::Request(Request::new("01J", "ping", json!({}))),
            Message::Response(Response::ok("01J", json!({"pong": true}))),
            Message::Response(Response::error(
                "01J",
                "unknown_method",
                "método desconocido",
            )),
            Message::Event(Event::new("agent.state", json!({"state": "RUNNING"}))),
            Message::Subscribe(Subscribe::new("01K", vec!["agent.*".into()])),
        ];
        for m in msgs {
            assert_eq!(Message::from_bytes(&m.to_bytes().unwrap()).unwrap(), m);
        }
    }

    #[test]
    fn wire_shape_is_stable() {
        let bytes = Message::Request(Request::new(
            "01J",
            "agent.pause",
            json!({"agent_id": "01A"}),
        ))
        .to_bytes()
        .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            v,
            json!({"type": "request", "protocol": 1, "id": "01J", "method": "agent.pause", "params": {"agent_id": "01A"}})
        );
    }

    #[test]
    fn unknown_version_is_explicit_error() {
        let bytes = br#"{"type":"request","protocol":2,"id":"x","method":"ping","params":{}}"#;
        assert!(matches!(
            Message::from_bytes(bytes),
            Err(ProtocolError::UnsupportedVersion { got: 2 })
        ));
    }

    #[test]
    fn missing_version_is_error() {
        let bytes = br#"{"type":"request","id":"x","method":"ping"}"#;
        assert!(matches!(
            Message::from_bytes(bytes),
            Err(ProtocolError::MissingVersion)
        ));
    }

    #[test]
    fn unknown_fields_and_types_are_rejected() {
        let extra =
            br#"{"type":"request","protocol":1,"id":"x","method":"ping","params":{},"sudo":true}"#;
        assert!(matches!(
            Message::from_bytes(extra),
            Err(ProtocolError::Json(_))
        ));
        let kind = br#"{"type":"broadcast","protocol":1}"#;
        assert!(matches!(
            Message::from_bytes(kind),
            Err(ProtocolError::Json(_))
        ));
    }
}
