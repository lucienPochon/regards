//! JSON-RPC 2.0 messages and the typed Regards methods.

use crate::token::UiToken;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Protocol versions this build speaks, oldest first.
pub const PROTOCOL_VERSIONS: &[u32] = &[1];

/// The JSON-RPC version marker.
pub const JSONRPC: &str = "2.0";

/// Method names.
pub mod methods {
    /// Handshake; must be the first request on a connection.
    pub const HELLO: &str = "hello";
    /// Daemon health and identity.
    pub const DAEMON_HEALTH: &str = "daemon.health";
    /// Ask the daemon to stop.
    pub const DAEMON_SHUTDOWN: &str = "daemon.shutdown";
}

/// Error codes. Negative values in -32768..=-32000 are reserved by JSON-RPC;
/// Regards-specific codes use -32000..=-32099.
pub mod codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
    /// A request arrived before a successful `hello`.
    pub const HANDSHAKE_REQUIRED: i32 = -32001;
    /// No protocol version in common.
    pub const UNSUPPORTED_PROTOCOL: i32 = -32002;
    /// The connection's principal may not call this method.
    pub const FORBIDDEN: i32 = -32003;
}

/// A client request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

impl Request {
    pub fn new(id: u64, method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: JSONRPC.to_owned(),
            id,
            method: method.into(),
            params,
        }
    }
}

/// A reply to a request. `id` is `None` only when the request could not be parsed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    pub fn success(id: u64, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC.to_owned(),
            id: Some(id),
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: Option<u64>, error: RpcError) -> Self {
        Self {
            jsonrpc: JSONRPC.to_owned(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// A server-to-client message without a reply (used by the change feed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

/// Anything the daemon sends to a client.
///
/// `Notification` is listed first: serde tries untagged variants in order, and
/// a notification would otherwise also match `Response` (whose `id` is optional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServerMessage {
    Notification(Notification),
    Response(Response),
}

/// A JSON-RPC error object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{message} (code {code})")]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

/// What kind of program opened the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    Desktop,
    Cli,
    Mcp,
}

/// Who the daemon treats the connection as (ADR-0006). CLI and MCP
/// connections are always `Agent`, subject to per-account access levels.
/// `Ui` is reserved for the desktop bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Principal {
    Ui,
    Agent,
}

/// Parameters of [`methods::HELLO`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloParams {
    pub protocol_versions: Vec<u32>,
    pub build_id: String,
    pub client_kind: ClientKind,
    /// The desktop bridge's per-launch secret. A matching token gives the
    /// `ui` principal; a wrong one is rejected with [`codes::FORBIDDEN`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_token: Option<UiToken>,
}

/// Result of [`methods::HELLO`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloResult {
    pub protocol_version: u32,
    pub daemon_version: String,
    pub daemon_build_id: String,
    pub principal: Principal,
}

/// Result of [`methods::DAEMON_HEALTH`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResult {
    pub status: String,
    pub version: String,
    pub build_id: String,
    pub pid: u32,
    pub uptime_secs: u64,
}

/// Pick the highest version both sides support.
pub fn negotiate(client_versions: &[u32]) -> Option<u32> {
    client_versions
        .iter()
        .copied()
        .filter(|v| PROTOCOL_VERSIONS.contains(v))
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn negotiation_picks_highest_common_version() {
        assert_eq!(negotiate(&[1]), Some(1));
        assert_eq!(negotiate(&[1, 99]), Some(1));
        assert_eq!(negotiate(&[99]), None);
        assert_eq!(negotiate(&[]), None);
    }

    #[test]
    fn request_omits_null_params() {
        let text = serde_json::to_string(&Request::new(7, methods::DAEMON_HEALTH, Value::Null)).unwrap();
        assert_eq!(text, r#"{"jsonrpc":"2.0","id":7,"method":"daemon.health"}"#);
    }

    #[test]
    fn server_messages_are_distinguished_by_id() {
        let response: ServerMessage =
            serde_json::from_value(json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}})).unwrap();
        assert!(matches!(response, ServerMessage::Response(r) if r.id == Some(1)));

        let notification: ServerMessage =
            serde_json::from_value(json!({"jsonrpc": "2.0", "method": "changes", "params": {"revision": 3}})).unwrap();
        assert!(matches!(notification, ServerMessage::Notification(n) if n.method == "changes"));
    }

    #[test]
    fn client_kind_uses_snake_case() {
        assert_eq!(serde_json::to_value(ClientKind::Desktop).unwrap(), json!("desktop"));
        assert_eq!(serde_json::to_value(Principal::Agent).unwrap(), json!("agent"));
    }
}
