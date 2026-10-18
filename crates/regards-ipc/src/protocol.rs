//! JSON-RPC 2.0 messages and the typed Regards methods.

use crate::token::UiToken;
use regards_core::body::Attachment;
use regards_core::headers::Person;
use regards_core::mailbox::Role;
use regards_core::secret::Secret;
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
    /// List mail accounts. `ui` only, until agent access levels exist (ADR-0012).
    pub const ACCOUNT_LIST: &str = "account.list";
    /// Verify a password sign-in and add the account. `ui` only (ADR-0006).
    pub const ACCOUNT_ADD: &str = "account.add";
    /// Remove an account and its stored password. `ui` only (ADR-0006).
    pub const ACCOUNT_REMOVE: &str = "account.remove";
    /// Sync an account now and wait for the result. `ui` only for now.
    pub const ACCOUNT_SYNC: &str = "account.sync";
    /// The mailboxes of an account and its sync state. `ui` only for now.
    pub const MAILBOX_LIST: &str = "mailbox.list";
    /// A page of a mailbox's messages, newest first. `ui` only for now.
    pub const MESSAGE_LIST: &str = "message.list";
    /// One message with its body, downloading it if needed. `ui` only for now.
    pub const MESSAGE_GET: &str = "message.get";
    /// Dedicate this connection to change notifications (ADR-0007). `ui` only for now.
    pub const CHANGES_SUBSCRIBE: &str = "changes.subscribe";
}

/// Notification methods the daemon sends.
pub mod notifications {
    /// Something clients may be showing has changed; refetch it. Params: [`ChangesNotification`](super::ChangesNotification).
    pub const CHANGES: &str = "changes";
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
    /// The referenced object does not exist.
    pub const NOT_FOUND: i32 = -32004;
    /// The request conflicts with existing data, for example a duplicate account.
    pub const CONFLICT: i32 = -32005;
    /// Connecting or signing in to the mail server failed. `data.reason` is an
    /// [`AccountCheckFailure`](super::AccountCheckFailure).
    pub const ACCOUNT_CHECK_FAILED: i32 = -32010;
    /// The operating system's credential store could not be used.
    pub const CREDENTIAL_STORE: i32 = -32011;
    /// A sync with the mail server failed; the message says why.
    pub const SYNC_FAILED: i32 = -32012;
    /// The message is too large to download for now.
    pub const TOO_LARGE: i32 = -32013;
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
    /// Revision of the latest committed change to the mail database (ADR-0007).
    pub revision: i64,
}

/// How a connection is encrypted. There is no plaintext option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionSecurity {
    /// TLS from the first byte (IMAP 993, SMTP 465).
    Tls,
    /// STARTTLS before authentication (SMTP 587).
    Starttls,
}

/// A mail server endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
    pub security: ConnectionSecurity,
}

/// Parameters of [`methods::ACCOUNT_ADD`]. `Debug` redacts the password.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountAddParams {
    pub display_name: String,
    pub email: String,
    /// Defaults to `email`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    pub password: Secret,
    pub imap: ServerSettings,
    pub smtp: ServerSettings,
}

/// An account as clients see it: no secrets, no credential-store keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSummary {
    pub id: String,
    pub display_name: String,
    pub email: String,
    pub username: String,
    pub imap: ServerSettings,
    pub smtp: ServerSettings,
    /// What the IMAP server advertised after sign-in.
    pub imap_capabilities: Vec<String>,
    /// Unix time in milliseconds.
    pub created_at: i64,
}

/// Parameters of [`methods::ACCOUNT_REMOVE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRemoveParams {
    pub id: String,
}

/// Parameters naming one account: [`methods::ACCOUNT_SYNC`], [`methods::MAILBOX_LIST`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRef {
    pub account_id: String,
}

/// Where an account's sync stands.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
    pub running: bool,
    /// Unix time in milliseconds of the last complete sync since the daemon started.
    pub last_success_at: Option<i64>,
    /// Why the last sync failed, if it did.
    pub last_error: Option<String>,
    /// A push connection is waiting for new mail (IMAP IDLE), so changes arrive within seconds.
    #[serde(default)]
    pub push_active: bool,
}

/// A mailbox as clients see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailboxSummary {
    pub id: i64,
    pub name: String,
    pub delimiter: Option<char>,
    pub role: Option<Role>,
    pub selectable: bool,
}

/// Result of [`methods::MAILBOX_LIST`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailboxListResult {
    /// Role folders first (inbox, drafts, sent, …), then by name.
    pub mailboxes: Vec<MailboxSummary>,
    pub sync: SyncState,
}

/// Result of [`methods::ACCOUNT_SYNC`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncResult {
    /// `true` when a sync was already running; nothing else was started.
    pub already_running: bool,
    pub mailboxes_added: usize,
    pub mailboxes_updated: usize,
    pub mailboxes_removed: usize,
    pub messages_added: usize,
    pub flag_changes: usize,
    pub messages_removed: usize,
    /// Server responses that could not be read and were skipped.
    pub unreadable: usize,
}

/// Result of [`methods::CHANGES_SUBSCRIBE`] and params of [`notifications::CHANGES`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangesNotification {
    /// Increases with every change of any kind, including sync status.
    pub version: u64,
    /// The mail database revision.
    pub revision: i64,
}

/// Where a page of messages ended; pass it as `before` for the next page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageCursor {
    pub received_at: i64,
    pub id: i64,
}

/// Parameters of [`methods::MESSAGE_LIST`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageListParams {
    pub mailbox_id: i64,
    /// Page size, 1 to 500; 50 when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<MessageCursor>,
}

/// A message in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSummary {
    pub id: i64,
    pub uid: u32,
    pub subject: String,
    pub from: Option<Person>,
    /// When the server received it, Unix time in milliseconds.
    pub received_at: i64,
    /// The Date header, Unix time in milliseconds.
    pub sent_at: Option<i64>,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub size: Option<u32>,
}

/// Parameters naming one message: [`methods::MESSAGE_GET`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageRef {
    pub id: i64,
}

/// Result of [`methods::MESSAGE_GET`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageDetail {
    pub message: MessageSummary,
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
    /// The text version, or text derived from the HTML.
    pub text: String,
    /// Whether there is an HTML version to show instead.
    pub has_html: bool,
    pub attachments: Vec<Attachment>,
}

/// Result of [`methods::MESSAGE_LIST`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageListResult {
    pub messages: Vec<MessageSummary>,
    /// Set when there may be older messages.
    pub next: Option<MessageCursor>,
}

/// Why an account check failed: `data.reason` of [`codes::ACCOUNT_CHECK_FAILED`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountCheckFailure {
    /// The server could not be reached.
    Connect,
    /// The server's certificate was not accepted.
    Tls,
    Timeout,
    /// Something other than an IMAP server answered.
    NotImap,
    /// Wrong user name or password.
    AuthFailed,
    /// The server does not allow password sign-in.
    LoginDisabled,
    Protocol,
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
