//! Regards desktop app: a Tauri 2 shell around the React UI.
//!
//! The WebView gets a deliberately narrow command set (ADR-0006): it never
//! talks to the daemon directly, only through the commands defined here.
//!
//! The app starts the daemon itself (ADR-0005) and keeps the per-launch token
//! the daemon hands it, which is what gives this process the `ui` principal.
//! In background mode, on by default, it starts at login and stays in the
//! tray when its window closes ([`background`]).

// Release builds on Windows have no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod background;
mod mail_frame;
mod notify;

use regards_core::paths::DataDirs;
use regards_core::secret::Secret;
use regards_ipc::launcher::{self, Connected, LaunchError, Launcher};
use regards_ipc::protocol::{
    AccountAddParams, AccountRef, AccountRemoveParams, AccountSummary, AsideParams, ClientKind, ConnectionSecurity,
    DeleteResult, DraftAttachFileParams, DraftCreateParams, DraftDetachParams, DraftListParams, DraftListResult,
    DraftRef, DraftReplyParams, DraftSendParams, DraftSendResult, DraftSetFromParams, DraftUpdateParams, DraftView,
    ExplainResult, IdentityListResult, IdentityRef, IdentitySaveParams, IdentityView, MailboxListResult, MarkResult,
    MessageCursor, MessageDetail, MessageIds, MessageListParams, MessageListResult, MessageRef, MoveParams, MoveResult,
    OutboxListResult, OutboxRef, OutboxRescheduleParams, OutboxUndoResult, PinParams, Principal, RemindParams,
    Screening, ScreeningParams, Section, SenderBlockParams, SenderSetParams, SenderSetResult, ServerSettings,
    SetFlagsParams, SetFlagsResult, SmartOverviewResult, SnoozeParams, SyncResult, TemplateListResult, TemplateRef,
    TemplateSaveParams, TemplateView, ThreadView, UndoParams, UndoResult, View, ViewParams, methods,
};
use regards_ipc::{Client, ClientError, UiToken};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tokio::sync::Mutex;

/// Overrides where the `regards` program is looked for (development and tests).
const DAEMON_PROGRAM_ENV: &str = "REGARDS_DAEMON_BIN";

/// The app's link to the daemon.
pub(crate) struct Bridge {
    /// `Err` when the data directory or the `regards` program could not be located.
    launcher: Result<Launcher, String>,
    /// Token of the daemon this process started, if it is still the one running.
    token: Mutex<Option<UiToken>>,
    /// Set while the app quits, so nothing starts a new daemon behind it.
    closing: AtomicBool,
}

impl Bridge {
    fn new() -> Self {
        Self {
            launcher: locate().map(|(dirs, program)| Launcher::new(dirs, program)),
            token: Mutex::new(None),
            closing: AtomicBool::new(false),
        }
    }

    /// Stop the daemon if this process started it: the app is quitting (from
    /// the tray, or its window closed with background mode off).
    async fn stop_own_daemon(&self) {
        let (Ok(launcher), Some(token)) = (&self.launcher, self.token.lock().await.take()) else {
            return;
        };
        // A daemon that no longer knows the token was replaced by someone else; leave it alone.
        if let Ok(mut client) = Client::connect_with(launcher.endpoint(), ClientKind::Desktop, Some(&token)).await {
            let _ = client.shutdown().await;
        }
    }
}

fn locate() -> Result<(DataDirs, PathBuf), String> {
    let dirs = DataDirs::resolve().map_err(|error| error.to_string())?;
    let program = match std::env::var_os(DAEMON_PROGRAM_ENV).filter(|v| !v.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => launcher::sibling_program().map_err(|error| format!("cannot locate the app: {error}"))?,
    };
    Ok((dirs, program))
}

/// What the UI shows about the background service.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DaemonStatus {
    running: bool,
    endpoint: String,
    version: Option<String>,
    build_id: Option<String>,
    pid: Option<u32>,
    uptime_secs: Option<u64>,
    /// Whether this app holds desktop access (the `ui` principal).
    desktop_access: bool,
    /// Whether this check started the service.
    started: bool,
    /// Whether a service from another build, or one started elsewhere, was replaced.
    replaced: bool,
    error: Option<String>,
}

impl DaemonStatus {
    fn failed(endpoint: String, error: impl ToString) -> Self {
        Self {
            running: false,
            endpoint,
            version: None,
            build_id: None,
            pid: None,
            uptime_secs: None,
            desktop_access: false,
            started: false,
            replaced: false,
            error: Some(error.to_string()),
        }
    }
}

impl Bridge {
    /// Connect with desktop access, starting or replacing the daemon as needed.
    pub(crate) async fn connect(&self) -> Result<Connected, String> {
        if self.closing.load(Ordering::SeqCst) {
            return Err("Regards is quitting".into());
        }
        let launcher = self.launcher.as_ref().map_err(Clone::clone)?;
        // Held while connecting, so concurrent calls never start two daemons.
        let mut token = self.token.lock().await;
        let connected = launcher
            .connect_or_start(ClientKind::Desktop, token.clone(), true)
            .await
            .map_err(explain)?;
        if let Some(launched) = &connected.launched {
            *token = Some(launched.ui_token.clone());
        }
        Ok(connected)
    }
}

/// Connect to the daemon, starting or replacing it as needed, and report on it.
#[tauri::command]
async fn daemon_status(bridge: State<'_, Bridge>) -> Result<DaemonStatus, String> {
    let endpoint = match &bridge.launcher {
        Ok(launcher) => launcher.endpoint().to_string(),
        Err(error) => return Ok(DaemonStatus::failed(String::new(), error)),
    };
    let mut connected = match bridge.connect().await {
        Ok(connected) => connected,
        Err(error) => return Ok(DaemonStatus::failed(endpoint, error)),
    };

    let status = match connected.client.health().await {
        Ok(health) => DaemonStatus {
            running: true,
            endpoint,
            version: Some(health.version),
            build_id: Some(health.build_id),
            pid: Some(health.pid),
            uptime_secs: Some(health.uptime_secs),
            desktop_access: connected.client.principal() == Principal::Ui,
            started: connected.launched.is_some(),
            replaced: connected.replaced,
            error: None,
        },
        Err(error) => DaemonStatus::failed(endpoint, error),
    };
    Ok(status)
}

/// The account form as the UI sends it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountInput {
    display_name: String,
    email: String,
    username: Option<String>,
    password: Secret,
    imap_host: String,
    imap_port: u16,
    smtp_host: String,
    smtp_port: u16,
    smtp_security: ConnectionSecurity,
}

#[tauri::command]
async fn account_list(bridge: State<'_, Bridge>) -> Result<Vec<AccountSummary>, String> {
    let mut client = bridge.connect().await?.client;
    client.call(methods::ACCOUNT_LIST, Value::Null).await.map_err(describe)
}

/// Check the sign-in with the server, then save the account (password in the OS credential store).
#[tauri::command]
async fn account_add(bridge: State<'_, Bridge>, input: AccountInput) -> Result<AccountSummary, String> {
    let smtp_host = match input.smtp_host.trim() {
        "" => input.imap_host.clone(),
        host => host.to_owned(),
    };
    let params = AccountAddParams {
        display_name: input.display_name,
        email: input.email,
        username: input.username,
        password: input.password,
        imap: ServerSettings {
            host: input.imap_host,
            port: input.imap_port,
            security: ConnectionSecurity::Tls,
        },
        smtp: ServerSettings {
            host: smtp_host,
            port: input.smtp_port,
            security: input.smtp_security,
        },
    };
    let mut client = bridge.connect().await?.client;
    client.call(methods::ACCOUNT_ADD, params).await.map_err(describe)
}

#[tauri::command]
async fn account_remove(bridge: State<'_, Bridge>, id: String) -> Result<(), String> {
    let mut client = bridge.connect().await?.client;
    let _: Value = client
        .call(methods::ACCOUNT_REMOVE, AccountRemoveParams { id })
        .await
        .map_err(describe)?;
    Ok(())
}

#[tauri::command]
async fn mailbox_list(bridge: State<'_, Bridge>, account_id: String) -> Result<MailboxListResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::MAILBOX_LIST, AccountRef { account_id })
        .await
        .map_err(describe)
}

/// Sync an account now; resolves when the sync is done.
#[tauri::command]
async fn account_sync(bridge: State<'_, Bridge>, account_id: String) -> Result<SyncResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::ACCOUNT_SYNC, AccountRef { account_id })
        .await
        .map_err(describe)
}

/// A page of a mailbox's messages, newest first.
#[tauri::command]
async fn message_list(
    bridge: State<'_, Bridge>,
    mailbox_id: i64,
    before: Option<MessageCursor>,
    limit: Option<u32>,
    section: Option<Section>,
) -> Result<MessageListResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(
            methods::MESSAGE_LIST,
            MessageListParams {
                mailbox_id,
                limit,
                before,
                section,
            },
        )
        .await
        .map_err(describe)
}

/// The Smart Inbox's sections of an account's inbox, with counts.
#[tauri::command]
async fn smart_overview(bridge: State<'_, Bridge>, account_id: String) -> Result<SmartOverviewResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::SMART_OVERVIEW, AccountRef { account_id })
        .await
        .map_err(describe)
}

/// Why a message is in its section, and what is known about its sender.
#[tauri::command]
async fn message_explain(bridge: State<'_, Bridge>, id: i64) -> Result<ExplainResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::MESSAGE_EXPLAIN, MessageRef { id })
        .await
        .map_err(describe)
}

/// Block a sender or `@domain` (their inbox mail goes to Junk; undoable), or unblock.
#[tauri::command]
async fn sender_block(bridge: State<'_, Bridge>, sender: SenderBlockParams) -> Result<MoveResult, String> {
    let mut client = bridge.connect().await?.client;
    client.call(methods::SENDER_BLOCK, sender).await.map_err(describe)
}

/// How new senders' mail is shown: off, section or hold.
#[tauri::command]
async fn smart_screening(bridge: State<'_, Bridge>) -> Result<ScreeningParams, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::SMART_SCREENING, Value::Null)
        .await
        .map_err(describe)
}

#[tauri::command]
async fn smart_set_screening(bridge: State<'_, Bridge>, mode: Screening) -> Result<ScreeningParams, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::SMART_SET_SCREENING, ScreeningParams { mode })
        .await
        .map_err(describe)
}

/// The person's choice for a sender: a section (or the rules), and priority.
#[tauri::command]
async fn sender_set(bridge: State<'_, Bridge>, sender: SenderSetParams) -> Result<SenderSetResult, String> {
    let mut client = bridge.connect().await?.client;
    client.call(methods::SENDER_SET, sender).await.map_err(describe)
}

/// One message with its body; the daemon downloads it the first time.
#[tauri::command]
async fn message_get(bridge: State<'_, Bridge>, id: i64) -> Result<MessageDetail, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::MESSAGE_GET, MessageRef { id })
        .await
        .map_err(describe)
}

/// The conversation a message belongs to, across folders, oldest first.
#[tauri::command]
async fn thread_get(bridge: State<'_, Bridge>, id: i64) -> Result<ThreadView, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::THREAD_GET, MessageRef { id })
        .await
        .map_err(describe)
}

/// Mark messages read or unread, flagged or not; applied at once, sent to the server in the background.
#[tauri::command]
async fn message_set_flags(
    bridge: State<'_, Bridge>,
    ids: Vec<i64>,
    seen: Option<bool>,
    flagged: Option<bool>,
) -> Result<SetFlagsResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::MESSAGE_SET_FLAGS, SetFlagsParams { ids, seen, flagged })
        .await
        .map_err(describe)
}

#[tauri::command]
async fn message_archive(bridge: State<'_, Bridge>, ids: Vec<i64>) -> Result<MoveResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::MESSAGE_ARCHIVE, MessageIds { ids })
        .await
        .map_err(describe)
}

#[tauri::command]
async fn message_trash(bridge: State<'_, Bridge>, ids: Vec<i64>) -> Result<MoveResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::MESSAGE_TRASH, MessageIds { ids })
        .await
        .map_err(describe)
}

/// Delete for good, from Trash or Junk only; the UI confirms first.
#[tauri::command]
async fn message_delete(bridge: State<'_, Bridge>, ids: Vec<i64>) -> Result<DeleteResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::MESSAGE_DELETE, MessageIds { ids })
        .await
        .map_err(describe)
}

#[tauri::command]
async fn message_move(bridge: State<'_, Bridge>, ids: Vec<i64>, mailbox_id: i64) -> Result<MoveResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::MESSAGE_MOVE, MoveParams { ids, mailbox_id })
        .await
        .map_err(describe)
}

#[tauri::command]
async fn undo_apply(bridge: State<'_, Bridge>, id: i64) -> Result<UndoResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::UNDO_APPLY, UndoParams { id })
        .await
        .map_err(describe)
}

/// Pin or unpin messages (kept on this device); undoable.
#[tauri::command]
async fn message_pin(bridge: State<'_, Bridge>, ids: Vec<i64>, pinned: bool) -> Result<MarkResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::MESSAGE_PIN, PinParams { ids, pinned })
        .await
        .map_err(describe)
}

/// Set messages aside (out of the inbox) or bring them back; undoable.
#[tauri::command]
async fn message_set_aside(bridge: State<'_, Bridge>, ids: Vec<i64>, aside: bool) -> Result<MarkResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::MESSAGE_SET_ASIDE, AsideParams { ids, aside })
        .await
        .map_err(describe)
}

/// Snooze until a time or "someday", or end a snooze (neither); undoable.
#[tauri::command]
async fn message_snooze(
    bridge: State<'_, Bridge>,
    ids: Vec<i64>,
    until: Option<i64>,
    someday: bool,
) -> Result<MarkResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::MESSAGE_SNOOZE, SnoozeParams { ids, until, someday })
        .await
        .map_err(describe)
}

/// "Remind me if no reply by" `at`, or no reminder (also: done with one that came due).
#[tauri::command]
async fn message_remind(bridge: State<'_, Bridge>, ids: Vec<i64>, at: Option<i64>) -> Result<MarkResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::MESSAGE_REMIND, RemindParams { ids, at })
        .await
        .map_err(describe)
}

/// An account's pinned, set-aside, snoozed or reminder messages, newest first.
#[tauri::command]
async fn message_view(
    bridge: State<'_, Bridge>,
    account_id: String,
    view: View,
    before: Option<MessageCursor>,
    limit: Option<u32>,
) -> Result<MessageListResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(
            methods::MESSAGE_VIEW,
            ViewParams {
                account_id,
                view,
                limit,
                before,
            },
        )
        .await
        .map_err(describe)
}

/// A new, empty message from an account.
#[tauri::command]
async fn draft_create(bridge: State<'_, Bridge>, account_id: String) -> Result<DraftView, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(
            methods::DRAFT_CREATE,
            DraftCreateParams {
                account_id,
                ..DraftCreateParams::default()
            },
        )
        .await
        .map_err(describe)
}

/// A reply (or reply all), addressed and quoted by the daemon.
#[tauri::command]
async fn draft_reply(bridge: State<'_, Bridge>, message_id: i64, all: bool) -> Result<DraftView, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::DRAFT_REPLY, DraftReplyParams { message_id, all })
        .await
        .map_err(describe)
}

#[tauri::command]
async fn draft_forward(bridge: State<'_, Bridge>, message_id: i64) -> Result<DraftView, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::DRAFT_FORWARD, MessageRef { id: message_id })
        .await
        .map_err(describe)
}

#[tauri::command]
async fn draft_get(bridge: State<'_, Bridge>, id: i64) -> Result<DraftView, String> {
    let mut client = bridge.connect().await?.client;
    client.call(methods::DRAFT_GET, DraftRef { id }).await.map_err(describe)
}

/// Pick files with the system's file dialog and attach them. The WebView
/// never sees a path: the dialog runs here and the daemon reads the files.
/// `None` if the person cancelled.
#[tauri::command]
async fn draft_attach_files(
    app: AppHandle,
    bridge: State<'_, Bridge>,
    draft_id: i64,
) -> Result<Option<DraftView>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picker = app.dialog().file().set_title("Attach files");
    let picked = tauri::async_runtime::spawn_blocking(move || picker.blocking_pick_files())
        .await
        .map_err(|error| error.to_string())?;
    let Some(paths) = picked else { return Ok(None) };
    let mut client = bridge.connect().await?.client;
    let mut latest = None;
    for path in paths {
        let path = path.into_path().map_err(|error| error.to_string())?;
        let draft: DraftView = client
            .call(
                methods::DRAFT_ATTACH_FILE,
                DraftAttachFileParams {
                    draft_id,
                    path: path.to_string_lossy().into_owned(),
                },
            )
            .await
            .map_err(describe)?;
        latest = Some(draft);
    }
    Ok(latest)
}

#[tauri::command]
async fn draft_detach(bridge: State<'_, Bridge>, draft_id: i64, attachment_id: i64) -> Result<DraftView, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(
            methods::DRAFT_DETACH,
            DraftDetachParams {
                draft_id,
                attachment_id,
            },
        )
        .await
        .map_err(describe)
}

/// Drafts being written, of every account.
#[tauri::command]
async fn draft_list(bridge: State<'_, Bridge>) -> Result<DraftListResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::DRAFT_LIST, DraftListParams::default())
        .await
        .map_err(describe)
}

/// Autosave: the whole of what the draft says.
#[tauri::command]
async fn draft_update(bridge: State<'_, Bridge>, draft: DraftUpdateParams) -> Result<DraftView, String> {
    let mut client = bridge.connect().await?.client;
    client.call(methods::DRAFT_UPDATE, draft).await.map_err(describe)
}

/// Send a draft from another of its account's identities (its signature swaps).
#[tauri::command]
async fn draft_set_from(bridge: State<'_, Bridge>, id: i64, identity_id: i64) -> Result<DraftView, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::DRAFT_SET_FROM, DraftSetFromParams { id, identity_id })
        .await
        .map_err(describe)
}

/// Drafts in the server's Drafts folders written elsewhere, to pick up.
#[tauri::command]
async fn draft_elsewhere(bridge: State<'_, Bridge>) -> Result<MessageListResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::DRAFT_ELSEWHERE, DraftListParams { account_id: None })
        .await
        .map_err(describe)
}

/// Pick up a draft written elsewhere: it becomes a draft here.
#[tauri::command]
async fn draft_adopt(bridge: State<'_, Bridge>, message_id: i64) -> Result<DraftView, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::DRAFT_ADOPT, MessageRef { id: message_id })
        .await
        .map_err(describe)
}

/// Templates: text to start a message with, by name.
#[tauri::command]
async fn template_list(bridge: State<'_, Bridge>) -> Result<TemplateListResult, String> {
    let mut client = bridge.connect().await?.client;
    client.call(methods::TEMPLATE_LIST, Value::Null).await.map_err(describe)
}

#[tauri::command]
async fn template_save(bridge: State<'_, Bridge>, template: TemplateSaveParams) -> Result<TemplateView, String> {
    let mut client = bridge.connect().await?.client;
    client.call(methods::TEMPLATE_SAVE, template).await.map_err(describe)
}

#[tauri::command]
async fn template_remove(bridge: State<'_, Bridge>, id: i64) -> Result<Value, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::TEMPLATE_REMOVE, TemplateRef { id })
        .await
        .map_err(describe)
}

/// An account's identities: the names and addresses it sends as, with signatures.
#[tauri::command]
async fn identity_list(bridge: State<'_, Bridge>, account_id: String) -> Result<IdentityListResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::IDENTITY_LIST, AccountRef { account_id })
        .await
        .map_err(describe)
}

#[tauri::command]
async fn identity_save(bridge: State<'_, Bridge>, identity: IdentitySaveParams) -> Result<IdentityView, String> {
    let mut client = bridge.connect().await?.client;
    client.call(methods::IDENTITY_SAVE, identity).await.map_err(describe)
}

#[tauri::command]
async fn identity_remove(bridge: State<'_, Bridge>, id: i64) -> Result<Value, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::IDENTITY_REMOVE, IdentityRef { id })
        .await
        .map_err(describe)
}

#[tauri::command]
async fn identity_make_default(bridge: State<'_, Bridge>, id: i64) -> Result<Value, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::IDENTITY_MAKE_DEFAULT, IdentityRef { id })
        .await
        .map_err(describe)
}

#[tauri::command]
async fn draft_delete(bridge: State<'_, Bridge>, id: i64) -> Result<Value, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::DRAFT_DELETE, DraftRef { id })
        .await
        .map_err(describe)
}

/// Queue the draft; it goes once the undo window is over.
#[tauri::command]
async fn draft_send(
    bridge: State<'_, Bridge>,
    id: i64,
    undo_seconds: Option<u32>,
    send_at: Option<i64>,
    remind_at: Option<i64>,
) -> Result<DraftSendResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(
            methods::DRAFT_SEND,
            DraftSendParams {
                id,
                undo_seconds,
                send_at,
                remind_at,
            },
        )
        .await
        .map_err(describe)
}

#[tauri::command]
async fn outbox_list(bridge: State<'_, Bridge>) -> Result<OutboxListResult, String> {
    let mut client = bridge.connect().await?.client;
    client.call(methods::OUTBOX_LIST, Value::Null).await.map_err(describe)
}

/// Undo send, or edit a failed message: returns the draft to reopen.
#[tauri::command]
async fn outbox_undo(bridge: State<'_, Bridge>, id: i64) -> Result<OutboxUndoResult, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::OUTBOX_UNDO, OutboxRef { id })
        .await
        .map_err(describe)
}

/// Send a scheduled or held message at another time, or now (`send_at: None`).
#[tauri::command]
async fn outbox_reschedule(bridge: State<'_, Bridge>, id: i64, send_at: Option<i64>) -> Result<Value, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::OUTBOX_RESCHEDULE, OutboxRescheduleParams { id, send_at })
        .await
        .map_err(describe)
}

#[tauri::command]
async fn outbox_send_again(bridge: State<'_, Bridge>, id: i64) -> Result<Value, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::OUTBOX_SEND_AGAIN, OutboxRef { id })
        .await
        .map_err(describe)
}

#[tauri::command]
async fn outbox_dismiss(bridge: State<'_, Bridge>, id: i64) -> Result<Value, String> {
    let mut client = bridge.connect().await?.client;
    client
        .call(methods::OUTBOX_DISMISS, OutboxRef { id })
        .await
        .map_err(describe)
}

/// Keep one subscribed connection to the daemon and re-emit its change
/// notifications to the WebView as `changes` events (ADR-0007). Reconnects
/// with backoff when the daemon restarts or is replaced.
async fn forward_changes(app: AppHandle) {
    let mut backoff = Duration::from_millis(500);
    loop {
        let bridge = app.state::<Bridge>();
        if bridge.closing.load(Ordering::SeqCst) {
            return;
        }
        let subscribed = match bridge.connect().await {
            Ok(connected) => connected.client.subscribe_changes().await.map_err(describe),
            Err(error) => Err(error),
        };
        match subscribed {
            Ok(mut feed) => {
                backoff = Duration::from_millis(500);
                // Views may be stale after a reconnect: refresh them once.
                let _ = app.emit("changes", feed.current());
                // What happened while the app was away is told now.
                app.state::<notify::Notifier>().poke();
                while let Ok(change) = feed.next().await {
                    let _ = app.emit("changes", change);
                    app.state::<notify::Notifier>().poke();
                }
            }
            Err(error) => eprintln!("change feed unavailable: {error}"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

/// Daemon errors carry messages written for people; show those as they are.
pub(crate) fn describe(error: ClientError) -> String {
    match error {
        ClientError::Rpc(error) => error.message,
        other => other.to_string(),
    }
}

fn explain(error: LaunchError) -> String {
    match error {
        LaunchError::ProgramMissing(path) => format!(
            "The Regards service program is missing ({}). In a development checkout, run \
             `cargo build -p regards-cli` first.",
            path.display()
        ),
        other => other.to_string(),
    }
}

/// Quit Regards fully, as the tray's Quit does: the app's own daemon stops
/// with it (`RunEvent::Exit`), and with it scheduled work.
#[tauri::command]
fn app_quit(app: AppHandle) {
    app.exit(0);
}

/// Whether Regards keeps running in the background (ADR-0017 §1).
#[tauri::command]
fn background_get(background: State<'_, background::Background>) -> bool {
    background.on()
}

#[tauri::command]
fn background_set(app: AppHandle, background: State<'_, background::Background>, on: bool) -> Result<(), String> {
    background.set(&app, on)
}

/// Whether notifications are shown (ADR-0011).
#[tauri::command]
fn notifications_get(background: State<'_, background::Background>) -> bool {
    background.notifications()
}

#[tauri::command]
fn notifications_set(background: State<'_, background::Background>, on: bool) -> Result<(), String> {
    background.set_notifications(on)
}

fn main() {
    let app = tauri::Builder::default()
        // Must come first: a second app instance would replace this one's daemon.
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            background::show(app)
        }))
        // Used from Rust only: the WebView is granted none of its commands.
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![background::BACKGROUND_ARG]),
        ))
        // Used from Rust only, as above.
        .plugin(tauri_plugin_notification::init())
        .manage(notify::Notifier::default())
        .manage(Bridge::new())
        .manage(background::Background::load(DataDirs::resolve().ok().as_ref()))
        .register_asynchronous_uri_scheme_protocol(mail_frame::SCHEME, mail_frame::handle)
        .setup(|app| {
            tauri::async_runtime::spawn(forward_changes(app.handle().clone()));
            tauri::async_runtime::spawn(notify::run(app.handle().clone()));
            background::setup(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // In background mode, closing the window leaves Regards in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.state::<background::Background>().on()
            {
                api.prevent_close();
                let _ = window.hide();
                window.state::<background::Background>().hidden(window.app_handle());
            }
        })
        .invoke_handler(tauri::generate_handler![
            daemon_status,
            app_quit,
            background_get,
            background_set,
            notifications_get,
            notifications_set,
            account_list,
            account_add,
            account_remove,
            account_sync,
            mailbox_list,
            message_list,
            smart_overview,
            message_explain,
            sender_set,
            sender_block,
            smart_screening,
            smart_set_screening,
            message_get,
            thread_get,
            message_set_flags,
            message_archive,
            message_trash,
            message_delete,
            message_move,
            undo_apply,
            message_pin,
            message_set_aside,
            message_snooze,
            message_remind,
            message_view,
            draft_create,
            draft_reply,
            draft_forward,
            draft_get,
            draft_list,
            draft_attach_files,
            draft_detach,
            draft_update,
            draft_delete,
            draft_set_from,
            identity_list,
            draft_elsewhere,
            draft_adopt,
            template_list,
            template_save,
            template_remove,
            identity_save,
            identity_remove,
            identity_make_default,
            draft_send,
            outbox_list,
            outbox_undo,
            outbox_reschedule,
            outbox_send_again,
            outbox_dismiss
        ])
        .build(tauri::generate_context!())
        .expect("failed to start the Regards desktop app");

    app.run(|app, event| match event {
        RunEvent::Exit => {
            let bridge = app.state::<Bridge>();
            bridge.closing.store(true, Ordering::SeqCst);
            tauri::async_runtime::block_on(async {
                let _ = tokio::time::timeout(Duration::from_secs(3), bridge.stop_own_daemon()).await;
            });
        }
        // The Dock icon, clicked while the window is hidden.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => background::show(app),
        _ => {}
    });
}
