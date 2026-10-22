//! JSON-RPC 2.0 messages and the typed Regards methods.

use crate::token::UiToken;
pub use regards_core::agents::Access as AgentAccess;
pub use regards_core::assistant::{Kind as AssistantKind, Task as AssistantTask};
use regards_core::body::Attachment;
pub use regards_core::compose::{AutoCopy, CopyKind, SendCheck};
pub use regards_core::executor::Executor;
use regards_core::headers::Person;
use regards_core::mailbox::Role;
pub use regards_core::oauth::Provider;
use regards_core::secret::Secret;
pub use regards_core::smart::{Policy, Reason, Screening, Section};
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
    /// Remove everything Regards keeps on this computer (Support): every
    /// credential at once, then the daemon stops and its files go once
    /// nothing holds them ([`DaemonEraseParams`] → [`DaemonErased`]). `ui` only.
    pub const DAEMON_ERASE: &str = "daemon.erase";
    /// List mail accounts. `ui` only, until agent access levels exist (ADR-0012).
    pub const ACCOUNT_LIST: &str = "account.list";
    /// Verify a password sign-in and add the account. `ui` only (ADR-0006).
    pub const ACCOUNT_ADD: &str = "account.add";
    /// Remove an account and its stored password. `ui` only (ADR-0006).
    pub const ACCOUNT_REMOVE: &str = "account.remove";
    /// Rename an account; the name it sends with follows if it was the same. `ui` only.
    pub const ACCOUNT_UPDATE: &str = "account.update";
    /// A copy (Cc or Bcc to an address) every new draft of an account gets,
    /// or none. `ui` only.
    pub const ACCOUNT_SET_AUTO_COPY: &str = "account.set_auto_copy";
    /// An address's mail servers, found before the person types them
    /// (known providers, DNS, then the usual names, checked). `ui` only.
    pub const ACCOUNT_DISCOVER: &str = "account.discover";
    /// The providers this build can sign in with (ADR-0018). `ui` only.
    pub const ACCOUNT_OAUTH_PROVIDERS: &str = "account.oauth_providers";
    /// Begin signing in with a provider: the daemon waits for the browser to
    /// come back and answers with the page to open in it. `ui` only.
    pub const ACCOUNT_OAUTH_START: &str = "account.oauth_start";
    /// Wait for the browser, then add the account, or renew the sign-in of
    /// the account already there. `ui` only.
    pub const ACCOUNT_OAUTH_FINISH: &str = "account.oauth_finish";
    /// Give up a sign-in that was started. `ui` only.
    pub const ACCOUNT_OAUTH_CANCEL: &str = "account.oauth_cancel";
    /// Sync an account now and wait for the result. `ui` only for now.
    pub const ACCOUNT_SYNC: &str = "account.sync";
    /// Drop an account's mail on this device and fetch it again (Support ›
    /// Troubleshooting): [`AccountRef`] → [`AccountResyncResult`]. The
    /// account, its settings, drafts, outbox, pins, snoozes and reminders
    /// stay. `ui` only.
    pub const ACCOUNT_RESYNC: &str = "account.resync";
    /// The mailboxes of an account and its sync state. `ui` only for now.
    pub const MAILBOX_LIST: &str = "mailbox.list";
    /// A page of a mailbox's messages, newest first. `ui` only for now.
    pub const MESSAGE_LIST: &str = "message.list";
    /// One message with its body, downloading it if needed. `ui` only for now.
    pub const MESSAGE_GET: &str = "message.get";
    /// The sanitised HTML version of a message (ADR-0006 §4). `ui` only for now.
    pub const MESSAGE_HTML: &str = "message.html";
    /// A message as it came, headers and all ("See the original message"):
    /// [`MessageSource`]. Agents: with Read access to its account.
    pub const MESSAGE_SOURCE: &str = "message.source";
    /// Every unread message of a folder read, or of one Smart Inbox section
    /// of it, with one undo: [`MarkAllReadParams`] → [`SetFlagsResult`].
    /// Only what is on this device yet; `ui` only.
    pub const MESSAGE_MARK_ALL_READ: &str = "message.mark_all_read";
    /// The conversation a message belongs to, across folders. `ui` only for now.
    pub const THREAD_GET: &str = "thread.get";
    /// Mark messages read or unread, flagged or not. Applied at once, sent to the
    /// server by the operation queue (ADR-0010). `ui` only for now.
    pub const MESSAGE_SET_FLAGS: &str = "message.set_flags";
    /// Move messages to another folder of their account; undoable. `ui` only for now.
    pub const MESSAGE_MOVE: &str = "message.move";
    /// Move messages to their account's Archive folder; undoable. `ui` only for now.
    pub const MESSAGE_ARCHIVE: &str = "message.archive";
    /// Move messages to their account's Trash folder; undoable. `ui` only for now.
    pub const MESSAGE_TRASH: &str = "message.trash";
    /// Delete messages for good, from Trash or Junk only. It cannot be undone,
    /// so clients confirm first. `ui` only for now.
    pub const MESSAGE_DELETE: &str = "message.delete";
    /// Reverse a recent action, by the undo id it returned. `ui` only for now.
    pub const UNDO_APPLY: &str = "undo.apply";
    /// Pin or unpin messages (kept on this device); undoable. `ui` only for now.
    pub const MESSAGE_PIN: &str = "message.pin";
    /// Set messages aside (out of the inbox) or bring them back; undoable.
    pub const MESSAGE_SET_ASIDE: &str = "message.set_aside";
    /// Snooze messages until a time, or "someday"; or end a snooze. Undoable.
    pub const MESSAGE_SNOOZE: &str = "message.snooze";
    /// The Smart Inbox (plan §5.4): an account's inbox by section, with counts.
    pub const SMART_OVERVIEW: &str = "smart.overview";
    /// Why a message is in its section ("why is this here?"), and what is
    /// known about its sender.
    pub const MESSAGE_EXPLAIN: &str = "message.explain";
    /// The person's choice for a sender: a section (or the rules), and whether
    /// they are a priority. Their mail follows.
    pub const SENDER_SET: &str = "sender.set";
    /// Block a sender or `@domain` (their inbox mail goes to Junk, and what
    /// arrives later; undoable), or unblock.
    pub const SENDER_BLOCK: &str = "sender.block";
    /// Let a sender's or `@domain`'s remote images load from then on, or
    /// block them again (ADR-0015 §4). `ui` only: a reading-privacy choice.
    pub const SENDER_SET_IMAGES: &str = "sender.set_images";
    /// The senders an account loads remote images from. `ui` only.
    pub const SENDER_IMAGES: &str = "sender.images";
    /// How new senders' mail is shown: off, section or hold.
    pub const SMART_SCREENING: &str = "smart.screening";
    pub const SMART_SET_SCREENING: &str = "smart.set_screening";
    /// "Remind me if no reply by" a time, or no reminder (also: done with one
    /// that came due). Undoable.
    pub const MESSAGE_REMIND: &str = "message.remind";
    /// An account's pinned, set-aside, snoozed or reminder messages, newest first.
    pub const MESSAGE_VIEW: &str = "message.view";
    /// Start a new message (Phase 2). Drafts and the outbox are `ui` only for now.
    pub const DRAFT_CREATE: &str = "draft.create";
    /// Start a reply (or reply all) to a stored message, quoted and addressed.
    pub const DRAFT_REPLY: &str = "draft.reply";
    /// Start forwarding a stored message.
    pub const DRAFT_FORWARD: &str = "draft.forward";
    pub const DRAFT_GET: &str = "draft.get";
    /// What to ask about before a draft goes: no recipients, no subject, an
    /// attachment it speaks of but lacks, something left to fill in.
    pub const DRAFT_CHECK: &str = "draft.check";
    /// Drafts not being sent, most recently changed first; of one account, or all.
    pub const DRAFT_LIST: &str = "draft.list";
    /// Save what the draft says (autosave). Refused while it is being sent.
    pub const DRAFT_UPDATE: &str = "draft.update";
    pub const DRAFT_DELETE: &str = "draft.delete";
    /// Attach a file the user chose, by path; the daemon reads and keeps it. `ui`
    /// only: an agent must never make the daemon read and send local files.
    pub const DRAFT_ATTACH_FILE: &str = "draft.attach_file";
    pub const DRAFT_DETACH: &str = "draft.detach";
    /// An account's identities: the names and addresses it sends as, each with
    /// its signature, the default first. `ui` only.
    pub const IDENTITY_LIST: &str = "identity.list";
    /// Add an identity (without `id`) or change one.
    pub const IDENTITY_SAVE: &str = "identity.save";
    /// Remove an identity; not the default.
    pub const IDENTITY_REMOVE: &str = "identity.remove";
    /// Make an identity its account's default (new messages go out from it).
    pub const IDENTITY_MAKE_DEFAULT: &str = "identity.make_default";
    /// Drafts in the server's Drafts folders written elsewhere (another
    /// device, webmail), to pick up. `ui` only.
    pub const DRAFT_ELSEWHERE: &str = "draft.elsewhere";
    /// Pick up a draft written elsewhere: it becomes a draft here, kept in step.
    pub const DRAFT_ADOPT: &str = "draft.adopt";
    /// Templates: text to start a message with, kept on this device. `ui` only.
    pub const TEMPLATE_LIST: &str = "template.list";
    /// Add a template (without `id`) or change one.
    pub const TEMPLATE_SAVE: &str = "template.save";
    pub const TEMPLATE_REMOVE: &str = "template.remove";
    /// Put an image in a draft's text (pasted, say): stored with a
    /// Content-ID, which the HTML refers to as `cid:`. `ui` only.
    pub const DRAFT_ATTACH_IMAGE: &str = "draft.attach_image";
    /// The images in a draft's text, to show while writing. `ui` only.
    pub const DRAFT_IMAGES: &str = "draft.images";
    /// Send a draft from another of its account's identities; the signature
    /// in a plain-text draft is swapped for the new one's.
    pub const DRAFT_SET_FROM: &str = "draft.set_from";
    /// Freeze the draft into the outbox; it goes once the undo window is over,
    /// or at the time asked for (send later).
    pub const DRAFT_SEND: &str = "draft.send";
    /// What is waiting to be sent, scheduled, held, failed or may not have gone.
    pub const OUTBOX_LIST: &str = "outbox.list";
    /// Take back a message not yet sent (undo send, a scheduled or held one, or
    /// edit after a failure).
    pub const OUTBOX_UNDO: &str = "outbox.undo";
    /// Send a scheduled or held message at another time, or now.
    pub const OUTBOX_RESCHEDULE: &str = "outbox.reschedule";
    /// Send a failed or uncertain message again: the user decided.
    pub const OUTBOX_SEND_AGAIN: &str = "outbox.send_again";
    /// Forget a failed or uncertain message (for example: it did go out).
    pub const OUTBOX_DISMISS: &str = "outbox.dismiss";
    /// What to tell the person that has not been shown yet: a snooze ended, a
    /// reminder came due, a send held or failed (ADR-0011). `ui` only.
    pub const NOTICE_LIST: &str = "notice.list";
    /// The notices up to an id have been shown (or seen in the app).
    pub const NOTICE_SHOWN: &str = "notice.shown";
    /// Whom to write to, as one types a recipient: the people an account
    /// writes with, best first. Agents: of accounts open to them (Read).
    pub const PEOPLE_SUGGEST: &str = "people.suggest";
    /// How messages are shown: [`ReadingSettings`]. `ui` only.
    pub const READING_SETTINGS: &str = "reading.settings";
    /// Change [`ReadingSettings`]. `ui` only.
    pub const READING_SET_SETTINGS: &str = "reading.set_settings";
    /// Whether recent inbox mail is downloaded ahead (off by default). `ui` only.
    pub const PREFETCH_GET: &str = "prefetch.get";
    /// Download recent inbox mail ahead, or not; on starts at once. `ui` only.
    pub const PREFETCH_SET: &str = "prefetch.set";
    /// The assistant (ADR-0015 §5): which AI model, if any, with the person's
    /// own key: [`AssistantSettings`]. `ui` only, like the three below.
    pub const ASSISTANT_SETTINGS: &str = "assistant.settings";
    /// Choose the assistant's provider, model and key, or turn it off.
    pub const ASSISTANT_SET_SETTINGS: &str = "assistant.set_settings";
    /// What a task would send, and where, before it is sent.
    pub const ASSISTANT_PREVIEW: &str = "assistant.preview";
    /// Do a task: a conversation's summary, or a reply drafted (never sent).
    pub const ASSISTANT_RUN: &str = "assistant.run";
    /// New mail not told of yet (plan §5.12): what came into an inbox since
    /// sync last looked, still unread, oldest first. `ui` only.
    pub const MAIL_ARRIVED: &str = "mail.arrived";
    /// New mail up to an id was looked at (told of, or let through by nothing). `ui` only.
    pub const MAIL_ARRIVED_SHOWN: &str = "mail.arrived_shown";
    /// What agents may do with an account: off, read, triage or send
    /// (ADR-0012). `ui` only: agents cannot change their own access.
    pub const ACCOUNT_SET_AGENT_ACCESS: &str = "account.set_agent_access";
    /// Whether an agent's sends wait for approval (the default) or go after
    /// an undo window the person can call them back in (ADR-0012 §3). `ui` only.
    pub const ACCOUNT_SET_AGENT_SEND: &str = "account.set_agent_send";
    /// What agents did, newest first: every request, allowed or refused
    /// (ADR-0012 §4). Kept 90 days. `ui` only.
    pub const AGENT_ACTIVITY: &str = "agent.activity";
    /// Every agent at once (Settings › Agents): [`AgentsSettings`]. `ui` only,
    /// like the two below.
    pub const AGENTS_SETTINGS: &str = "agents.settings";
    /// Turn every agent off, or on again ([`AgentsEnabledParams`]). Off, the
    /// CLI and MCP get nothing but `hello`; each account's own level stays
    /// as it is. Turning off also ends the agents' connections.
    pub const AGENTS_SET_ENABLED: &str = "agents.set_enabled";
    /// End every agent's connection now: [`AgentsDisconnected`].
    pub const AGENTS_DISCONNECT: &str = "agents.disconnect";
    /// What agents asked that waits for the person, newest first. `ui` only.
    pub const HOLD_LIST: &str = "hold.list";
    /// One hold. Agents: holds of the accounts open to them, to follow theirs.
    pub const HOLD_GET: &str = "hold.get";
    /// Approve a hold: it runs as the agent asked. `ui` only.
    pub const HOLD_APPROVE: &str = "hold.approve";
    /// Reject a hold. `ui` only.
    pub const HOLD_REJECT: &str = "hold.reject";
    /// Search the mail: Gmail-style words and filters (plan §5.10), newest
    /// first. Agents: the accounts open to them (read).
    pub const SEARCH: &str = "search.query";
    /// [`SEARCH`] on the servers too: for words in messages not opened here
    /// (their text is not indexed on this device). Slower: it asks each
    /// server. Takes [`SearchParams`](super::SearchParams). Agents: as [`SEARCH`].
    pub const SEARCH_SERVER: &str = "search.server";
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
    /// The request waits for the person's approval (ADR-0012 §3): `data`
    /// has `hold_id`, `expires_at` and `description`. Follow it with `hold.get`.
    pub const HELD: i32 = -32014;
    /// An agent asked too much too fast (ADR-0012 §5): `data.retry_after_ms`
    /// says when to try again. Or too many of its requests wait for the person.
    pub const RATE_LIMITED: i32 = -32015;
    /// A sign-in with a provider did not complete. `data.reason` is an
    /// [`OauthFailure`](super::OauthFailure).
    pub const OAUTH_FAILED: i32 = -32016;
    /// The assistant's provider could not be reached, refused the key, was
    /// busy or declined: the message says which.
    pub const ASSISTANT_FAILED: i32 = -32017;
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

/// Parameters of [`methods::ACCOUNT_SET_AUTO_COPY`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoCopyParams {
    pub account_id: String,
    /// `None`: no copy.
    #[serde(default)]
    pub auto_copy: Option<AutoCopy>,
}

/// Parameters of [`methods::ACCOUNT_UPDATE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountUpdateParams {
    pub id: String,
    /// Empty: the address.
    pub display_name: String,
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
    /// What agents may do with it (ADR-0012); `off` until its owner allows them.
    #[serde(default)]
    pub agent_access: AgentAccess,
    /// An agent's send goes after this many seconds, during which the
    /// person can call it back; without it (the default), each waits for
    /// approval. At most 20 an hour go so; more wait for approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_send_window: Option<u32>,
    /// The copy every new draft gets (Cc or Bcc to an address), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_copy: Option<AutoCopy>,
    /// How it signs in.
    #[serde(default)]
    pub sign_in: SignIn,
    /// Its OAuth sign-in expired or was withdrawn: sign in again
    /// (`account.oauth_start` with its address) before it syncs or sends.
    #[serde(default)]
    pub needs_sign_in: bool,
}

/// How an account signs in (ADR-0018).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignIn {
    /// A password or app password.
    #[default]
    Password,
    /// OAuth with Microsoft.
    Microsoft,
    /// OAuth with Google.
    Google,
}

/// Parameters of [`methods::ACCOUNT_DISCOVER`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverParams {
    pub email: String,
}

/// Where [`methods::ACCOUNT_DISCOVER`] found the servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverSource {
    /// A provider Regards knows, by the address's domain.
    Known,
    /// The domain's mail is hosted by a known provider (its MX records), as
    /// with Microsoft 365 or Google Workspace.
    Mx,
    /// The domain's SRV records (RFC 6186), and the servers answered.
    Dns,
    /// The usual names on the domain (`imap.`, `mail.`, `smtp.`), which answered.
    Guess,
}

/// What [`methods::ACCOUNT_DISCOVER`] found: a proposal to fill in the form
/// with, checked when the account is added. Missing parts were not found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverResult {
    /// `None`: nothing found; ask for the servers.
    pub source: Option<DiscoverSource>,
    pub imap: Option<ServerSettings>,
    pub smtp: Option<ServerSettings>,
    /// The provider's OAuth sign-in, if Regards knows one (offer it when
    /// `account.oauth_providers` has it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<Provider>,
    /// Only OAuth works: no password form.
    #[serde(default)]
    pub oauth_only: bool,
    /// What to tell the person about the password (an app password …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_help: Option<String>,
}

/// The result of [`methods::ACCOUNT_OAUTH_PROVIDERS`]: those this build has
/// a registered app for. Others sign in with a password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OauthProvidersResult {
    pub providers: Vec<Provider>,
}

/// Parameters of [`methods::ACCOUNT_OAUTH_START`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OauthStartParams {
    pub provider: Provider,
    /// The address, when known, so the provider offers it first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// The result of [`methods::ACCOUNT_OAUTH_START`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OauthStartResult {
    /// For [`methods::ACCOUNT_OAUTH_FINISH`] and [`methods::ACCOUNT_OAUTH_CANCEL`].
    pub flow_id: String,
    /// The provider's sign-in page, to open in the system browser.
    pub url: String,
}

/// Parameters of [`methods::ACCOUNT_OAUTH_FINISH`] and [`methods::ACCOUNT_OAUTH_CANCEL`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OauthFlowRef {
    pub flow_id: String,
}

/// Why a sign-in failed: `data.reason` of [`codes::OAUTH_FAILED`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OauthFailure {
    /// This build has no app registered with the provider.
    NotAvailable,
    /// The person said no in the browser, or the sign-in was given up.
    Cancelled,
    /// Nobody came back from the browser in time.
    TimedOut,
    /// The provider refused (the message says how).
    Refused,
    /// The provider could not be reached.
    Offline,
    /// The provider did not say which address signed in.
    NoAddress,
}

/// Parameters of [`methods::HOLD_LIST`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldListParams {
    /// Only those still waiting.
    #[serde(default)]
    pub pending: bool,
    /// 1 to 200; 50 when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldListResult {
    pub holds: Vec<HoldView>,
}

/// Parameters naming a hold: [`methods::HOLD_GET`], [`methods::HOLD_APPROVE`], [`methods::HOLD_REJECT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldRef {
    pub id: i64,
}

/// What an agent asked that waits (or waited) for the person.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldView {
    pub id: i64,
    /// Unix ms.
    pub created_at: i64,
    /// Undecided by then, it expires.
    pub expires_at: i64,
    /// The agent's connection, and what it came through (`cli`, `mcp`).
    pub session: String,
    pub client: String,
    pub account_id: String,
    /// The request, as the agent made it: run as is once approved.
    pub method: String,
    pub params: Value,
    /// For the person: "Send “Plans” to Bob", "Archive 120 messages".
    pub description: String,
    pub reason: HoldReason,
    /// For a send: the draft (`draft.get` shows it). Changed since, it is
    /// not sent on approval.
    pub draft_id: Option<i64>,
    pub status: HoldStatus,
    pub decided_at: Option<i64>,
    /// Once approved: what running it gave (as the method answers), or why
    /// it could not run.
    pub result: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldReason {
    /// Sending: every send by an agent waits.
    Sending,
    /// More messages at once than an agent may change.
    Bulk,
    /// Deleting for good.
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldStatus {
    Pending,
    /// Approved, and it ran.
    Approved,
    Rejected,
    /// Not decided in time.
    Expired,
    /// Approved, but it could not run (see `error`).
    Failed,
}

/// Parameters of [`methods::AGENT_ACTIVITY`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentActivityParams {
    /// Only what was done with this account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Page size, 1 to 500; 100 when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Older than this entry (`next` of the page before).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentActivityResult {
    pub entries: Vec<AgentActivity>,
    pub next: Option<i64>,
}

/// One request an agent made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentActivity {
    pub id: i64,
    /// Unix time in milliseconds.
    pub at: i64,
    /// The agent's connection: requests made together share it.
    pub session: String,
    /// What it came through: `cli`, `mcp` (or `desktop` without the app's token).
    pub client: String,
    pub method: String,
    /// Its parameters, secrets taken out; `{ "cut": … }` when long.
    pub args: Value,
    /// The account it acted on, when it named one, and what that account allowed then.
    pub account_id: Option<String>,
    pub access: Option<AgentAccess>,
    pub decision: AgentDecision,
    /// Set when it failed (`decision` is `denied` when it was not allowed).
    pub error_code: Option<i32>,
    pub error: Option<String>,
    pub duration_ms: i64,
    /// For a change it made: how what it changed stood before (messages,
    /// a draft or a sender), as JSON.
    #[serde(default)]
    pub before: Option<Value>,
    /// What reverses that change ([`methods::UNDO_APPLY`]), and whether it
    /// still can (undo lasts ten minutes, and once).
    #[serde(default)]
    pub undo_id: Option<i64>,
    #[serde(default)]
    pub undoable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDecision {
    Allowed,
    Denied,
    /// Waiting for the person to approve it.
    Held,
    Approved,
    Rejected,
}

/// Parameters of [`methods::ACCOUNT_SET_AGENT_ACCESS`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAccessParams {
    pub account_id: String,
    pub access: AgentAccess,
}

/// Parameters of [`methods::ACCOUNT_SET_AGENT_SEND`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSendParams {
    pub account_id: String,
    /// Seconds, 10 to 600, that an agent's send waits before it goes, during
    /// which the person can call it back. `None`: each waits for approval.
    #[serde(default)]
    pub window_seconds: Option<u32>,
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
    /// Unread messages; for the inbox, leaving out what is set aside or snoozed.
    #[serde(default)]
    pub unread: u32,
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
    /// The last message's place in the list: its `listed_at`.
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
    /// In the inbox: only this Smart Inbox section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<Section>,
}

/// Parameters of [`methods::SEARCH`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchParams {
    /// What was typed. Words match subject, people and text (the last one
    /// also as the start of a word); `"a phrase"`, `-word`; `from:` `to:`
    /// `subject:`; `has:attachment`; `is:unread` `is:read` `is:flagged`;
    /// `in:<folder or role>` (`in:anywhere` takes in Trash and Junk, left
    /// out otherwise); `before:` / `after:` `YYYY-MM-DD`.
    pub query: String,
    /// Only this account; every account when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Page size, 1 to 500; 50 when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<MessageCursor>,
}

/// What [`methods::SEARCH`] found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    /// One copy of each message (its inbox copy, if it has one), newest first.
    pub messages: Vec<MessageSummary>,
    /// On the first page, when the words match few messages: the best
    /// matches, best first (they are among the pages too).
    #[serde(default)]
    pub top: Vec<MessageSummary>,
    /// Set when there may be older results.
    pub next: Option<MessageCursor>,
    /// Changes still to index: while it is above 0, recent mail may be missing.
    pub indexing: u64,
    /// When nothing was found: the search with its unknown words replaced
    /// by the nearest known ones, which does find something ("did you
    /// mean …?"). To offer, not to run unasked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

/// What [`methods::SEARCH_SERVER`] found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSearchResult {
    /// Found by the servers and on this device: one copy of each (its inbox
    /// copy, if it has one), newest first.
    pub messages: Vec<MessageSummary>,
    /// Set when there may be older results.
    pub next: Option<MessageCursor>,
    /// Found by the servers but not on this device yet: older than what has
    /// been fetched so far, or just arrived.
    pub not_here: u32,
    /// Accounts whose server could not be searched now (offline, say).
    pub unreachable: Vec<String>,
}

/// A message in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSummary {
    pub id: i64,
    /// The folder it is in, or on its way to.
    #[serde(default)]
    pub mailbox_id: i64,
    pub uid: u32,
    pub subject: String,
    pub from: Option<Person>,
    /// The first few recipients (To, then Cc): enough to show mail one sent
    /// as "to Lena, Bob".
    #[serde(default)]
    pub to: Vec<Person>,
    /// When the server received it, Unix time in milliseconds.
    pub received_at: i64,
    /// The Date header, Unix time in milliseconds.
    pub sent_at: Option<i64>,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub size: Option<u32>,
    /// Pinned on this device (plan §5.5).
    #[serde(default)]
    pub pinned: bool,
    /// Set aside: out of the inbox until brought back.
    #[serde(default)]
    pub aside: bool,
    /// Out of the inbox until then (Unix ms).
    #[serde(default)]
    pub snoozed_until: Option<i64>,
    /// "Remind me if no reply by" then (Unix ms).
    #[serde(default)]
    pub remind_at: Option<i64>,
    /// A reminder came due with no reply: it shows in the inbox, wherever the
    /// message is filed, until it is done ([`methods::MESSAGE_REMIND`] with no time).
    #[serde(default)]
    pub reminded: bool,
    /// Its Smart Inbox section (none until sorted).
    #[serde(default)]
    pub section: Option<Section>,
    /// How many messages its conversation has (1 when alone).
    #[serde(default = "one")]
    pub thread_size: u32,
    /// It has attached files (not inline images).
    #[serde(default)]
    pub has_attachments: bool,
    /// The start of its text, whitespace collapsed (about 140 characters).
    #[serde(default)]
    pub preview: Option<String>,
    /// Its place in the list: `received_at`, except in the inbox for what came
    /// back to it (a snooze that ended, a reminder), listed by when it did.
    #[serde(default)]
    pub listed_at: i64,
}

fn one() -> u32 {
    1
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
    /// The folder it is in, or on its way to.
    pub mailbox_id: i64,
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
    /// The text version, or text derived from the HTML.
    pub text: String,
    /// Whether there is an HTML version to show instead.
    pub has_html: bool,
    /// Remote images in the HTML version, which are not loaded unless asked for.
    pub remote_images: usize,
    /// The person lets this sender's (or its domain's) remote images load:
    /// `message.html` shows them without `allow_remote`.
    #[serde(default)]
    pub images_from_sender: bool,
    pub attachments: Vec<Attachment>,
}

/// Parameters of [`methods::MESSAGE_SET_FLAGS`]. Omitted flags stay as they are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetFlagsParams {
    pub ids: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flagged: Option<bool>,
}

/// Result of [`methods::MESSAGE_SET_FLAGS`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetFlagsResult {
    /// Messages whose flags actually changed.
    pub changed: usize,
    /// Set when something changed. Undo sets back only the flags still as
    /// this left them.
    #[serde(default)]
    pub undo: Option<UndoInfo>,
}

/// One message of a conversation, and where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadEntry {
    pub message: MessageSummary,
    pub mailbox_id: i64,
    pub mailbox_name: String,
    pub mailbox_role: Option<Role>,
}

/// Result of [`methods::THREAD_GET`]: oldest first, one entry per message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadView {
    pub messages: Vec<ThreadEntry>,
}

/// Parameters of [`methods::MESSAGE_ARCHIVE`] and [`methods::MESSAGE_TRASH`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageIds {
    pub ids: Vec<i64>,
}

/// Parameters of [`methods::MESSAGE_MOVE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveParams {
    pub ids: Vec<i64>,
    /// The folder to move them to.
    pub mailbox_id: i64,
}

/// How to reverse an action, for a few minutes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoInfo {
    pub id: i64,
    /// What was done, for example "Archived 2 messages".
    pub description: String,
}

/// Result of the move actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveResult {
    pub moved: usize,
    /// Set when something moved.
    pub undo: Option<UndoInfo>,
}

/// Result of [`methods::MESSAGE_DELETE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteResult {
    pub deleted: usize,
}

/// Parameters of [`methods::UNDO_APPLY`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoParams {
    pub id: i64,
}

/// Result of [`methods::UNDO_APPLY`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoResult {
    /// Messages put back.
    pub undone: usize,
}

/// Parameters of [`methods::MESSAGE_PIN`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinParams {
    pub ids: Vec<i64>,
    pub pinned: bool,
}

/// Parameters of [`methods::MESSAGE_SET_ASIDE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsideParams {
    pub ids: Vec<i64>,
    /// `false` brings them back to the inbox.
    pub aside: bool,
}

/// Parameters of [`methods::MESSAGE_SNOOZE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnoozeParams {
    pub ids: Vec<i64>,
    /// When they come back (Unix ms). `None` with `someday` false ends the snooze.
    #[serde(default)]
    pub until: Option<i64>,
    /// No time: they come back only when asked (or when someone replies).
    #[serde(default)]
    pub someday: bool,
}

/// One section of the Smart Inbox: how many messages, how many unread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionCount {
    pub section: Section,
    pub total: u32,
    pub unread: u32,
}

/// Result of [`methods::SMART_OVERVIEW`]: the sections with mail, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartOverviewResult {
    pub sections: Vec<SectionCount>,
}

/// What is known about a sender, for one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderView {
    pub address: String,
    /// The section the person chose for them, if any.
    pub policy: Option<Policy>,
    pub priority: bool,
    /// The account has written to them.
    pub written_to: bool,
    /// Blocked, by address or domain: their mail goes to Junk.
    #[serde(default)]
    pub blocked: bool,
}

/// Parameters of [`methods::SENDER_SET_IMAGES`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderImagesParams {
    pub account_id: String,
    /// An address, or a whole domain as `@example.org`.
    pub address: String,
    pub allow: bool,
}

/// The result of [`methods::SENDER_IMAGES`]: addresses and `@domain`s, sorted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderImagesResult {
    pub senders: Vec<String>,
}

/// Parameters of [`methods::SENDER_BLOCK`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderBlockParams {
    pub account_id: String,
    /// An address, or a whole domain as `@example.org`.
    pub address: String,
    pub blocked: bool,
}

/// Parameters and result of [`methods::SMART_SET_SCREENING`] and [`methods::SMART_SCREENING`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreeningParams {
    pub mode: Screening,
}

/// Result of [`methods::MESSAGE_EXPLAIN`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplainResult {
    pub section: Section,
    /// The rule that placed it.
    pub reason: Reason,
    /// None for a message without a sender's address.
    pub sender: Option<SenderView>,
}

/// Parameters of [`methods::SENDER_SET`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderSetParams {
    pub account_id: String,
    pub address: String,
    /// Where their mail goes; none to let the rules decide.
    #[serde(default)]
    pub policy: Option<Policy>,
    #[serde(default)]
    pub priority: bool,
}

/// Result of [`methods::SENDER_SET`]: how many messages changed section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderSetResult {
    pub changed: usize,
}

/// Parameters of [`methods::MESSAGE_REMIND`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemindParams {
    pub ids: Vec<i64>,
    /// When to remind if nobody has replied (Unix ms, in the future). `None`:
    /// no reminder, and done with one that came due.
    #[serde(default)]
    pub at: Option<i64>,
}

/// Result of marking messages (pin, set aside, snooze, remind).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkResult {
    /// Messages whose mark changed.
    pub changed: usize,
    /// Set when something changed.
    pub undo: Option<UndoInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    Pinned,
    Aside,
    Snoozed,
    /// Waiting for a reply, with a reminder set.
    Reminders,
}

/// Parameters of [`methods::MESSAGE_VIEW`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewParams {
    pub account_id: String,
    pub view: View,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<MessageCursor>,
}

/// A message being written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftView {
    pub id: i64,
    pub account_id: String,
    /// Who it goes out from: its identity's name and address.
    pub from: Person,
    /// That identity (`None` only for an account without one).
    #[serde(default)]
    pub identity_id: Option<i64>,
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
    pub bcc: Vec<Person>,
    pub subject: String,
    /// The text; with `html`, its plain-text version.
    pub body: String,
    /// The formatted version, when it was written with formatting.
    #[serde(default)]
    pub html: Option<String>,
    /// The stored message it answers, if any.
    pub replying_to: Option<i64>,
    /// Unix time in milliseconds.
    pub updated_at: i64,
    /// Files going with it, in the order attached.
    #[serde(default)]
    pub attachments: Vec<DraftAttachment>,
    /// It is in the outbox: take it back before changing it.
    pub sending: bool,
}

/// A file attached to a draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftAttachment {
    pub id: i64,
    pub name: String,
    pub mime: String,
    pub size: u64,
    /// An image in the text: the draft's HTML refers to it as `cid:<content_id>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_id: Option<String>,
}

/// Parameters of [`methods::DRAFT_ATTACH_IMAGE`]: an image put in the text
/// (pasted, say), as its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftAttachImageParams {
    pub draft_id: i64,
    pub name: String,
    /// image/png, image/jpeg, image/gif or image/webp.
    pub mime: String,
    /// The bytes, base64.
    pub data: String,
}

/// What [`methods::DRAFT_ATTACH_IMAGE`] gives: the image's Content-ID, and the draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftImageAttached {
    pub content_id: String,
    pub draft: DraftView,
}

/// What [`methods::DRAFT_IMAGES`] gives: the images in a draft's text, to show while writing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftImagesResult {
    pub images: Vec<DraftImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftImage {
    pub content_id: String,
    pub mime: String,
    /// The bytes, base64.
    pub data: String,
}

/// Parameters of [`methods::DRAFT_ATTACH_FILE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftAttachFileParams {
    pub draft_id: i64,
    /// An absolute path to a file the user chose.
    pub path: String,
    /// An image to put in the text rather than beside it.
    #[serde(default)]
    pub inline: bool,
}

/// Parameters of [`methods::DRAFT_DETACH`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftDetachParams {
    pub draft_id: i64,
    pub attachment_id: i64,
}

/// Parameters of [`methods::DRAFT_LIST`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftListParams {
    /// Only this account's; all accounts' when absent.
    #[serde(default)]
    pub account_id: Option<String>,
}

/// Result of [`methods::DRAFT_LIST`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftListResult {
    pub drafts: Vec<DraftView>,
}

/// Parameters of [`methods::DRAFT_CREATE`]; everything may start empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftCreateParams {
    pub account_id: String,
    #[serde(default)]
    pub to: Vec<Person>,
    #[serde(default)]
    pub cc: Vec<Person>,
    #[serde(default)]
    pub bcc: Vec<Person>,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    /// Who it is from; the account's default identity when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_id: Option<i64>,
}

/// Parameters of [`methods::DRAFT_REPLY`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftReplyParams {
    /// The stored message to answer.
    pub message_id: i64,
    /// Reply to everyone, not just the sender.
    #[serde(default)]
    pub all: bool,
}

/// Names a draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftRef {
    pub id: i64,
}

/// Parameters of [`methods::DRAFT_UPDATE`]: the whole of what the draft says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftUpdateParams {
    pub id: i64,
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
    pub bcc: Vec<Person>,
    pub subject: String,
    /// The text. Ignored when `html` is given: the daemon derives it.
    pub body: String,
    /// The formatted version; the daemon sanitises it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
}

/// One of the names and addresses an account sends as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityView {
    pub id: i64,
    pub account_id: String,
    pub name: String,
    pub email: String,
    /// Plain text, put below what is written and above a quote.
    pub signature: String,
    pub is_default: bool,
}

/// Result of [`methods::IDENTITY_LIST`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityListResult {
    pub identities: Vec<IdentityView>,
}

/// Parameters of [`methods::IDENTITY_SAVE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentitySaveParams {
    /// The identity to change; none to add one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub account_id: String,
    #[serde(default)]
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub signature: String,
}

/// Names an identity: [`methods::IDENTITY_REMOVE`], [`methods::IDENTITY_MAKE_DEFAULT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityRef {
    pub id: i64,
}

/// A template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateView {
    pub id: i64,
    pub name: String,
    pub subject: String,
    /// The text; with `html`, its plain-text version.
    pub body: String,
    /// The formatted version, sanitised.
    #[serde(default)]
    pub html: Option<String>,
    pub updated_at: i64,
}

/// Result of [`methods::TEMPLATE_LIST`]: by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateListResult {
    pub templates: Vec<TemplateView>,
}

/// Parameters of [`methods::TEMPLATE_SAVE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateSaveParams {
    /// The template to change; none to add one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub name: String,
    #[serde(default)]
    pub subject: String,
    /// The text. Ignored when `html` is given: the daemon derives it.
    #[serde(default)]
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
}

/// Names a template: [`methods::TEMPLATE_REMOVE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateRef {
    pub id: i64,
}

/// Parameters of [`methods::DRAFT_SET_FROM`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftSetFromParams {
    pub id: i64,
    pub identity_id: i64,
}

/// Parameters of [`methods::DRAFT_SEND`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftSendParams {
    pub id: i64,
    /// Seconds during which the send can be undone: 0 to 60, default 10 (ADR-0010 §4).
    #[serde(default)]
    pub undo_seconds: Option<u32>,
    /// Send later: when to send it, Unix time in milliseconds, in the future.
    /// It goes from this device, so the computer must be on then; if it was
    /// off or asleep and the time is more than an hour past, the message is
    /// held for the person instead (ADR-0011 §2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_at: Option<i64>,
    /// Remind me if nobody has replied by then (Unix ms), as [`methods::MESSAGE_REMIND`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remind_at: Option<i64>,
}

/// Result of [`methods::DRAFT_SEND`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftSendResult {
    pub outbox_id: i64,
    /// Until when [`methods::OUTBOX_UNDO`] stops it, Unix time in milliseconds.
    /// A scheduled message can be taken back until it goes.
    pub undo_until: i64,
    #[serde(default)]
    pub send_at: Option<i64>,
    /// Who sends it (plan §5.7); in 1.0 always this device.
    #[serde(default)]
    pub executor: Executor,
    /// It goes from this computer: with `send_at`, the computer must be on,
    /// with Regards running, at that time. Say so ("your computer must be on
    /// at 09:00", ADR-0019).
    #[serde(default)]
    pub needs_this_computer: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxState {
    /// Waiting: for its undo window, its time (send later), or to retry.
    Queued,
    /// Scheduled, but found well past its time (the computer was off or
    /// asleep): it waits for the person to send it, move it or take it back.
    Held,
    /// A remote executor holds it and sends it at its time, with this
    /// computer on or off (from 1.1).
    HandedOff,
    Sending,
    /// Not sent; something must change first.
    Failed,
    /// It may or may not have gone out; the user decides.
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxEntryView {
    pub id: i64,
    pub account_id: String,
    pub draft_id: Option<i64>,
    pub subject: String,
    pub recipients: Vec<Person>,
    pub state: OutboxState,
    pub undo_until: i64,
    /// Send later: when it is meant to go.
    #[serde(default)]
    pub send_at: Option<i64>,
    pub next_attempt_at: i64,
    pub attempts: u32,
    pub last_error: Option<String>,
    /// Who sends it; see [`DraftSendResult::executor`].
    #[serde(default)]
    pub executor: Executor,
    /// See [`DraftSendResult::needs_this_computer`].
    #[serde(default)]
    pub needs_this_computer: bool,
    /// Queued, and its account must be signed in again
    /// ([`AccountSummary::needs_sign_in`]): it goes as soon as that is done.
    #[serde(default)]
    pub waiting_for_sign_in: bool,
    /// An agent asked for it (ADR-0012): it goes after the account's undo
    /// window for agents, or the person approved it.
    #[serde(default)]
    pub by_agent: bool,
}

/// Result of [`methods::OUTBOX_LIST`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxListResult {
    pub entries: Vec<OutboxEntryView>,
}

/// Names an outbox entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxRef {
    pub id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    /// A snoozed message is back in the inbox.
    SnoozeEnded,
    /// Nobody replied by the reminder's time: the message is back in the inbox.
    ReminderDue,
    /// A scheduled message was found well past its time; it waits in the Outbox.
    SendHeld,
    /// A message could not be sent.
    SendFailed,
    /// A message may or may not have been sent.
    SendUncertain,
    /// An agent asks something that waits for the person: the subject is
    /// what it asks ("Send “Plans” to Bob"), or the draft's subject.
    AgentAsks,
    /// An agent's message is on its way (`outbox_id`): it can be called
    /// back (`outbox.undo`) until its undo window is over.
    AgentSending,
}

/// One notice: what happened, who (the sender, or the recipients) and the
/// subject. Never the message's text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticeView {
    pub id: i64,
    pub account_id: String,
    pub kind: NoticeKind,
    /// The stored message it is about, if any.
    pub message_id: Option<i64>,
    /// The outbox entry it is about, if any.
    pub outbox_id: Option<i64>,
    pub people: Vec<Person>,
    pub subject: String,
    /// Unix ms.
    pub created_at: i64,
}

/// Result of [`methods::NOTICE_LIST`]: oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticeListResult {
    pub notices: Vec<NoticeView>,
}

/// The result of [`methods::DRAFT_CHECK`]: nothing when it can go as it is.
/// `no_recipients` stops it; the others are for the person to confirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftCheckResult {
    pub checks: Vec<SendCheck>,
}

/// Parameters of [`methods::PEOPLE_SUGGEST`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeopleSuggestParams {
    /// What was typed: the start of an address or of a word of a name.
    /// Empty: those written with most.
    pub query: String,
    /// Only this account's people; every account's when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// 1 to 50; 8 when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// The result of [`methods::PEOPLE_SUGGEST`]: best first (written to most,
/// lately; no-reply and blocked addresses left out).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeopleSuggestResult {
    pub people: Vec<Person>,
}

/// The result of [`methods::ACCOUNT_RESYNC`]: the mail is coming back, as
/// after adding the account (newest first, older in the background).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountResyncResult {
    /// Messages dropped from this device.
    pub dropped: u32,
}

/// The result of [`methods::AGENTS_SETTINGS`] and [`methods::AGENTS_SET_ENABLED`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentsSettings {
    /// On by default; each account is still closed to agents until opened.
    pub enabled: bool,
    /// Agents connected now (the CLI, MCP servers).
    pub connected: u32,
}

/// Parameters of [`methods::AGENTS_SET_ENABLED`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentsEnabledParams {
    pub enabled: bool,
}

/// The result of [`methods::AGENTS_DISCONNECT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentsDisconnected {
    pub disconnected: u32,
}

/// Parameters of [`methods::DAEMON_ERASE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonEraseParams {
    /// Exactly [`DaemonEraseParams::CONFIRMATION`], so nothing erases by mistake.
    pub confirm: String,
}

impl DaemonEraseParams {
    pub const CONFIRMATION: &str = "erase everything";

    pub fn confirmed() -> Self {
        Self {
            confirm: Self::CONFIRMATION.to_owned(),
        }
    }
}

/// The result of [`methods::DAEMON_ERASE`]. The daemon stops right after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonErased {
    /// Credentials removed from the system's credential store.
    pub credentials: u32,
}

/// How messages are shown, kept by the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadingSettings {
    /// Links in messages lose their tracking parameters (`utm_…`, `fbclid`
    /// …) before they can be opened (ADR-0015 §4). On by default.
    pub clean_links: bool,
}

/// The result of [`methods::ASSISTANT_SETTINGS`] and
/// [`methods::ASSISTANT_SET_SETTINGS`]. The key never comes back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantSettings {
    /// `None`: off (the default). Nothing is ever sent then.
    pub kind: Option<AssistantKind>,
    /// Where requests go: `https://api.anthropic.com`, or an OpenAI-compatible
    /// server's base (`https://…/v1`, `http://localhost:11434/v1`).
    pub base_url: Option<String>,
    pub model: Option<String>,
    /// A key is kept in the system's credential store.
    pub has_key: bool,
}

/// Parameters of [`methods::ASSISTANT_SET_SETTINGS`].
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssistantSetSettingsParams {
    /// `None` turns the assistant off and forgets the key.
    pub kind: Option<AssistantKind>,
    /// The provider's own when omitted (Anthropic); needed otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The provider's suggested model when omitted (Anthropic); needed otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// A new key. Omitted: the one kept stays. A server on this computer may need none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<Secret>,
}

/// Parameters of [`methods::ASSISTANT_PREVIEW`] and [`methods::ASSISTANT_RUN`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantParams {
    /// A message of the conversation; a reply answers this one.
    pub message_id: i64,
    pub task: AssistantTask,
    /// A reply to everyone, not just the sender.
    #[serde(default)]
    pub all: bool,
}

/// The result of [`methods::ASSISTANT_PREVIEW`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantPreview {
    /// Messages of the conversation that would be sent: the latest.
    pub messages: u32,
    /// Characters of their text, together.
    pub characters: u32,
    /// Messages left out because they look like they carry a sign-in or
    /// verification code.
    pub left_out: u32,
    /// The host that would get them (`api.anthropic.com`, `localhost` …).
    pub destination: String,
    pub model: String,
}

/// The result of [`methods::ASSISTANT_RUN`]: text written by a model, to
/// read and check, never followed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantResult {
    /// The summary, or the reply's text.
    pub text: String,
    /// For a reply: the draft it was written into, above the signature and
    /// the quoted message, to review and send (or not).
    pub draft: Option<DraftView>,
}

/// Parameters of [`methods::PREFETCH_SET`], and the result of both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefetchParams {
    /// The inboxes' messages of the last 30 days (up to 25 MB each) are
    /// downloaded ahead: they open at once, offline too, and their whole
    /// text is searchable.
    pub on: bool,
}

/// One new message to tell of: who, the subject and its Smart Inbox
/// section, so the app can tell of only what the person chose (People and
/// Priority, say). Never its text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrivalView {
    /// For [`methods::MAIL_ARRIVED_SHOWN`].
    pub id: i64,
    pub message_id: i64,
    pub account_id: String,
    pub from: Option<Person>,
    pub subject: String,
    /// `None` until sorted.
    pub section: Option<Section>,
    /// Unix ms.
    pub received_at: i64,
}

/// The result of [`methods::MAIL_ARRIVED`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrivedResult {
    pub arrivals: Vec<ArrivalView>,
}

/// Parameters of [`methods::NOTICE_SHOWN`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticeShownParams {
    pub up_to: i64,
}

/// Parameters of [`methods::OUTBOX_RESCHEDULE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxRescheduleParams {
    pub id: i64,
    /// The new time, Unix ms in the future; none to send it now.
    #[serde(default)]
    pub send_at: Option<i64>,
}

/// Result of [`methods::OUTBOX_UNDO`]: the draft it came from, to reopen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxUndoResult {
    pub draft_id: Option<i64>,
}

/// Parameters of [`methods::MESSAGE_HTML`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageHtmlParams {
    pub id: i64,
    /// Load remote images. They can tell the sender the message was opened.
    #[serde(default)]
    pub allow_remote: bool,
    /// When given, every web link becomes `<link_prefix><percent-encoded URL>`,
    /// so the frame's host can open it outside the app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_prefix: Option<String>,
}

/// Result of [`methods::MESSAGE_HTML`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageHtml {
    /// A complete, sanitised document. Show it only in a sandboxed frame with a strict CSP.
    pub html: String,
    pub blocked_remote: usize,
}

/// Result of [`methods::MESSAGE_SOURCE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSource {
    /// The RFC 5322 text, as plain text: show it as text, never as HTML.
    /// Bytes that are not UTF-8 show as U+FFFD.
    pub text: String,
    /// Only the first [`MessageSource::MAX`] bytes are in `text`.
    pub truncated: bool,
    /// The whole message's size, in bytes.
    pub size: u64,
}

impl MessageSource {
    /// Bytes of a message's source given at most.
    pub const MAX: usize = 1024 * 1024;
}

/// Parameters of [`methods::MESSAGE_MARK_ALL_READ`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkAllReadParams {
    pub mailbox_id: i64,
    /// Only this Smart Inbox section's; the whole folder's when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<Section>,
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
