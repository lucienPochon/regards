//! Writing and sending mail (Phase 2): drafts, replies and forwards, and the
//! outbox's user-facing side. Sending itself is the outbox runner's job
//! ([`crate::outbox`]). Only the `ui` principal reaches these for now; agents
//! get them through access levels and holds (ADR-0012).

use crate::State;
use regards_core::compose::{self, Outgoing};
use regards_core::headers::{self, Person};
use regards_ipc::protocol::{
    DraftAttachFileParams, DraftAttachment, DraftCreateParams, DraftDetachParams, DraftListParams, DraftListResult,
    DraftRef, DraftReplyParams, DraftSendParams, DraftSendResult, DraftUpdateParams, DraftView, MessageRef,
    OutboxEntryView, OutboxListResult, OutboxRef, OutboxRescheduleParams, OutboxState, OutboxUndoResult, RpcError,
    codes,
};
use regards_store::drafts::{self, Content, Draft, Edit};
use regards_store::outbox::{self, Entry, NewEntry, State as EntryState};
use regards_store::{StoreError, accounts, mailboxes};
use rusqlite::Transaction;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// ADR-0010 §4: undo send for 10 s by default, at most 30.
const DEFAULT_UNDO_SECONDS: u32 = 10;
const MAX_UNDO_SECONDS: u32 = 30;
/// More recipients than this is a mailing, not a message.
const MAX_RECIPIENTS: usize = 100;
/// Bodies larger than this are not a typed message.
const MAX_BODY: usize = 2 * 1024 * 1024;
/// Most mail servers take messages up to about 25 MB (attachments grow by a third in transit).
const MAX_ATTACHMENTS: u64 = 25 * 1024 * 1024;

pub(crate) async fn create(state: &State, params: DraftCreateParams) -> Result<Value, RpcError> {
    check_content(&params.to, &params.cc, &params.bcc, &params.body)?;
    let account_id = params.account_id.clone();
    let now = now_ms();
    let id = state
        .store
        .write(move |tx| {
            accounts::get(tx, &account_id)?.ok_or_else(|| StoreError::Conflict("no such account".into()))?;
            let content = Content {
                to: params.to,
                cc: params.cc,
                bcc: params.bcc,
                subject: params.subject,
                body: params.body,
                ..Content::default()
            };
            drafts::create(tx, &account_id, &content, now)
        })
        .await
        .map_err(conflict)?
        .value;
    view(state, id).await
}

/// A reply, addressed (Reply-To, sender, everyone for reply all) and quoted.
pub(crate) async fn reply(state: &State, params: DraftReplyParams) -> Result<Value, RpcError> {
    let (message, source) = crate::mail::load(state, params.message_id).await?;
    let mailbox_id = message.mailbox_id;
    let account = state
        .store
        .read(move |conn| {
            let mailbox = mailboxes::get(conn, mailbox_id)?;
            Ok(match mailbox {
                Some(mailbox) => accounts::get(conn, &mailbox.account_id)?,
                None => None,
            })
        })
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "the message's account no longer exists"))?;
    let original = headers::parse(&source);
    let text = regards_core::body::parse(&source).text;
    let me = [account.email.as_str()];
    let (to, cc) = compose::reply_recipients(
        original.from.as_ref(),
        &original.reply_to,
        &original.to,
        &original.cc,
        &me,
        params.all,
    );
    let content = Content {
        to,
        cc,
        bcc: Vec::new(),
        subject: compose::reply_subject(&original.subject),
        body: compose::quote(&text, original.from.as_ref(), original.date),
        in_reply_to: original.message_id.clone(),
        references: compose::reply_references(&original.references, original.message_id.as_deref()),
        replying_to: Some(message.id),
    };
    store_new(state, account.id, content).await
}

/// A forward: the original's text and headers below, nobody addressed yet.
pub(crate) async fn forward(state: &State, params: MessageRef) -> Result<Value, RpcError> {
    let (message, source) = crate::mail::load(state, params.id).await?;
    let mailbox_id = message.mailbox_id;
    let account_id = state
        .store
        .read(move |conn| Ok(mailboxes::get(conn, mailbox_id)?.map(|m| m.account_id)))
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "the message's account no longer exists"))?;
    let original = headers::parse(&source);
    let blobs = state.blobs.clone();
    // The original's files go too, kept in blobs/ like attached ones (up to the usual limit).
    let (text, files) = tokio::task::spawn_blocking(move || -> std::io::Result<_> {
        let text = regards_core::body::parse(&source).text;
        let mut total = 0u64;
        let mut kept = Vec::new();
        for file in regards_core::body::files(&source) {
            total += file.data.len() as u64;
            if total > MAX_ATTACHMENTS {
                break;
            }
            let hash = blobs.put(&file.data)?;
            kept.push((hash, compose::file_name(&file.name), file.mime, file.data.len() as u64));
        }
        Ok((text, kept))
    })
    .await
    .map_err(crate::session::internal)?
    .map_err(crate::session::internal)?;
    let content = Content {
        subject: compose::forward_subject(&original.subject),
        body: compose::forwarded(
            &text,
            original.from.as_ref(),
            &original.to,
            &original.subject,
            original.date,
        ),
        ..Content::default()
    };
    let now = now_ms();
    let id = state
        .store
        .write(move |tx| {
            let id = drafts::create(tx, &account_id, &content, now)?;
            for (hash, name, mime, size) in &files {
                drafts::attach(tx, id, hash, name, mime, *size)?;
            }
            Ok(id)
        })
        .await
        .map_err(conflict)?
        .value;
    view(state, id).await
}

async fn store_new(state: &State, account_id: String, content: Content) -> Result<Value, RpcError> {
    let now = now_ms();
    let id = state
        .store
        .write(move |tx| drafts::create(tx, &account_id, &content, now))
        .await
        .map_err(conflict)?
        .value;
    view(state, id).await
}

pub(crate) async fn get(state: &State, params: DraftRef) -> Result<Value, RpcError> {
    view(state, params.id).await
}

pub(crate) async fn list(state: &State, params: DraftListParams) -> Result<Value, RpcError> {
    let account_id = params.account_id;
    let (drafts, senders) = state
        .store
        .read(move |conn| {
            let senders: HashMap<String, Person> = accounts::list(conn)?
                .iter()
                .map(|account| (account.id.clone(), sender(account)))
                .collect();
            let mut drafts = Vec::new();
            for draft in drafts::list(conn, account_id.as_deref())? {
                let files = drafts::attachments(conn, draft.id)?;
                drafts.push((draft, files));
            }
            Ok((drafts, senders))
        })
        .await
        .map_err(crate::session::internal)?;
    let drafts = drafts
        .into_iter()
        .filter_map(|(draft, files)| {
            let from = senders.get(&draft.account_id)?.clone();
            Some(draft_view(draft, from, files))
        })
        .collect();
    Ok(json!(DraftListResult { drafts }))
}

/// Autosave: the whole of what the draft says.
pub(crate) async fn update(state: &State, params: DraftUpdateParams) -> Result<Value, RpcError> {
    check_content(&params.to, &params.cc, &params.bcc, &params.body)?;
    if params.html.as_ref().is_some_and(|html| html.len() > MAX_BODY) {
        return Err(RpcError::new(codes::TOO_LARGE, "the message text is too long"));
    }
    // Formatted text is sanitised here, once, and its plain version derived from it.
    let (body, html) = match &params.html {
        Some(written) => {
            let clean = regards_core::html::sanitize_outgoing(written);
            (regards_core::html::to_text(&clean), Some(clean))
        }
        None => (params.body.clone(), None),
    };
    let id = params.id;
    let now = now_ms();
    state
        .store
        .write(move |tx| {
            let edit = Edit {
                to: &params.to,
                cc: &params.cc,
                bcc: &params.bcc,
                subject: &params.subject,
                body: &body,
                html: html.as_deref(),
            };
            drafts::update(tx, params.id, &edit, now)
        })
        .await
        .map_err(conflict)?;
    view(state, id).await
}

pub(crate) async fn delete(state: &State, params: DraftRef) -> Result<Value, RpcError> {
    let id = params.id;
    let deleted = state
        .store
        .write(move |tx| drafts::delete(tx, id))
        .await
        .map_err(conflict)?
        .value;
    if !deleted {
        return Err(RpcError::new(codes::NOT_FOUND, "no such draft"));
    }
    Ok(json!({ "deleted": true }))
}

/// Attach a file the user chose. The daemon reads it once into `blobs/`;
/// after that the draft no longer depends on the file.
pub(crate) async fn attach_file(state: &State, params: DraftAttachFileParams) -> Result<Value, RpcError> {
    let path = std::path::PathBuf::from(&params.path);
    if !path.is_absolute() {
        return Err(RpcError::new(codes::INVALID_PARAMS, "give the file's full path"));
    }
    let draft_id = params.draft_id;
    let current: u64 = state
        .store
        .read(move |conn| Ok(drafts::attachments(conn, draft_id)?.iter().map(|a| a.size).sum()))
        .await
        .map_err(crate::session::internal)?;
    let blobs = state.blobs.clone();
    let read = tokio::task::spawn_blocking(move || -> Result<(String, String, u64), RpcError> {
        let metadata = std::fs::metadata(&path)
            .map_err(|error| RpcError::new(codes::NOT_FOUND, format!("cannot open the file: {error}")))?;
        if !metadata.is_file() {
            return Err(RpcError::new(codes::INVALID_PARAMS, "that is not a file"));
        }
        if current + metadata.len() > MAX_ATTACHMENTS {
            return Err(RpcError::new(
                codes::TOO_LARGE,
                format!(
                    "attachments are limited to {} MB per message, which most mail servers accept",
                    MAX_ATTACHMENTS / 1024 / 1024
                ),
            ));
        }
        let data = std::fs::read(&path)
            .map_err(|error| RpcError::new(codes::NOT_FOUND, format!("cannot read the file: {error}")))?;
        let hash = blobs.put(&data).map_err(crate::session::internal)?;
        let name = compose::file_name(
            &path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        Ok((hash, name, data.len() as u64))
    })
    .await
    .map_err(crate::session::internal)??;
    let (hash, name, size) = read;
    let mime = compose::guess_mime(&name).to_owned();
    state
        .store
        .write(move |tx| drafts::attach(tx, draft_id, &hash, &name, &mime, size))
        .await
        .map_err(conflict)?;
    view(state, draft_id).await
}

pub(crate) async fn detach(state: &State, params: DraftDetachParams) -> Result<Value, RpcError> {
    let (draft_id, attachment_id) = (params.draft_id, params.attachment_id);
    let removed = state
        .store
        .write(move |tx| drafts::detach(tx, draft_id, attachment_id))
        .await
        .map_err(conflict)?
        .value;
    if !removed {
        return Err(RpcError::new(codes::NOT_FOUND, "no such attachment"));
    }
    view(state, draft_id).await
}

/// Send later: further ahead than this is surely a mistake (seconds read as
/// milliseconds, say).
const MAX_SEND_AHEAD_MS: i64 = 5 * 366 * 24 * 3600 * 1000;

/// Freeze the draft's exact bytes into the outbox. It goes once the undo
/// window is over, or at `send_at`; until then [`undo`] takes it back.
pub(crate) async fn send(state: &Arc<State>, params: DraftSendParams) -> Result<Value, RpcError> {
    let undo_seconds = params.undo_seconds.unwrap_or(DEFAULT_UNDO_SECONDS);
    if undo_seconds > MAX_UNDO_SECONDS {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("undo_seconds must be 0 to {MAX_UNDO_SECONDS}"),
        ));
    }
    let (id, send_at) = (params.id, params.send_at);
    let now = now_ms();
    if let Some(at) = send_at {
        check_send_at(at, now)?;
    }
    let undo_until = now + i64::from(undo_seconds) * 1000;
    let (seen_version, files) = draft_files(state, id).await?;
    let outbox_id = state
        .store
        .write(move |tx| {
            let (draft, account) = unchanged_draft(tx, id, seen_version)?;
            if draft.sending {
                return Err(StoreError::Conflict("this message is already being sent".into()));
            }
            // Dated for when it goes, so a scheduled one does not arrive looking old.
            let message_id = compose::new_message_id(&account.email);
            let outgoing = outgoing(&draft, &account, files, message_id, send_at.unwrap_or(now));
            let mime = outgoing
                .build()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
            let entry = NewEntry {
                account_id: account.id.clone(),
                draft_id: Some(draft.id),
                message_id: outgoing.message_id.clone(),
                subject: draft.subject.clone(),
                recipients: draft.to.iter().chain(&draft.cc).chain(&draft.bcc).cloned().collect(),
                envelope_from: account.email.clone(),
                envelope_to: outgoing.recipients(),
                mime,
                undo_until,
                send_at,
                answering: draft.replying_to,
            };
            outbox::queue(tx, &entry, now)
        })
        .await
        .map_err(conflict)?
        .value;
    state.outbox.wake();
    Ok(json!(DraftSendResult {
        outbox_id,
        undo_until,
        send_at,
    }))
}

/// Send a scheduled or held message at another time, or now. Its draft is
/// frozen, so the bytes are built again from it only to date them for when
/// they go; the Message-ID stays.
pub(crate) async fn reschedule(state: &Arc<State>, params: OutboxRescheduleParams) -> Result<Value, RpcError> {
    let (id, send_at) = (params.id, params.send_at);
    let now = now_ms();
    if let Some(at) = send_at {
        check_send_at(at, now)?;
    }
    let too_late = || RpcError::new(codes::CONFLICT, "too late: the message is being sent or has gone");
    let entry = state
        .store
        .read(move |conn| outbox::get(conn, id))
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such message in the outbox"))?;
    if !matches!(entry.state, EntryState::Queued | EntryState::Held) {
        return Err(too_late());
    }
    let draft_id = entry.draft_id.ok_or_else(too_late)?;
    let (seen_version, files) = draft_files(state, draft_id).await?;
    let message_id = entry.message_id;
    let moved = state
        .store
        .write(move |tx| {
            let (draft, account) = unchanged_draft(tx, draft_id, seen_version)?;
            let mime = outgoing(&draft, &account, files, message_id, send_at.unwrap_or(now))
                .build()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
            outbox::reschedule(tx, id, send_at, &mime, now)
        })
        .await
        .map_err(conflict)?
        .value;
    if !moved {
        return Err(too_late());
    }
    state.outbox.wake();
    Ok(json!({ "rescheduled": true }))
}

fn check_send_at(at: i64, now: i64) -> Result<(), RpcError> {
    if at <= now {
        return Err(RpcError::new(codes::INVALID_PARAMS, "that time has passed"));
    }
    if at - now > MAX_SEND_AHEAD_MS {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            "send_at is more than five years ahead; it is Unix time in milliseconds",
        ));
    }
    Ok(())
}

/// A draft's version and files, read before the write so the writer does not
/// wait on the disk.
async fn draft_files(state: &State, id: i64) -> Result<(i64, Vec<compose::Attachment>), RpcError> {
    let listed = state
        .store
        .read(move |conn| {
            let Some(draft) = drafts::get(conn, id)? else {
                return Ok(None);
            };
            Ok(Some((draft.updated_at, drafts::attachments(conn, id)?)))
        })
        .await
        .map_err(crate::session::internal)?;
    let (version, listed) = listed.ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such draft"))?;
    let mut files = Vec::with_capacity(listed.len());
    for attachment in listed {
        let blobs = state.blobs.clone();
        let hash = attachment.blob_hash.clone();
        let data = tokio::task::spawn_blocking(move || blobs.get(&hash))
            .await
            .map_err(crate::session::internal)?
            .map_err(crate::session::internal)?
            .ok_or_else(|| {
                RpcError::new(
                    codes::CONFLICT,
                    format!("the attached file {} is missing; attach it again", attachment.name),
                )
            })?;
        files.push(compose::Attachment {
            name: attachment.name,
            mime: attachment.mime,
            data,
        });
    }
    Ok((version, files))
}

/// The draft and its account, if the draft is as it was when its files were
/// read: what is frozen must be what was shown.
fn unchanged_draft(tx: &Transaction<'_>, id: i64, seen_version: i64) -> Result<(Draft, accounts::Account), StoreError> {
    let draft = drafts::get(tx, id)?.ok_or_else(|| StoreError::Conflict("no such draft".into()))?;
    if draft.updated_at != seen_version {
        return Err(StoreError::Conflict(
            "the draft changed while it was being sent; send it again".into(),
        ));
    }
    let account =
        accounts::get(tx, &draft.account_id)?.ok_or_else(|| StoreError::Conflict("no such account".into()))?;
    Ok((draft, account))
}

fn outgoing(
    draft: &Draft,
    account: &accounts::Account,
    attachments: Vec<compose::Attachment>,
    message_id: String,
    date: i64,
) -> Outgoing {
    Outgoing {
        attachments,
        from: sender(account),
        to: draft.to.clone(),
        cc: draft.cc.clone(),
        bcc: draft.bcc.clone(),
        subject: draft.subject.clone(),
        text: draft.body.clone(),
        html: draft.html.clone(),
        message_id,
        in_reply_to: draft.in_reply_to.clone(),
        references: draft.references.clone(),
        date,
    }
}

pub(crate) async fn outbox_list(state: &State) -> Result<Value, RpcError> {
    let entries = state.store.read(outbox::list).await.map_err(crate::session::internal)?;
    Ok(json!(OutboxListResult {
        entries: entries.into_iter().map(entry_view).collect(),
    }))
}

/// Undo send, or "edit" after a failure: the entry goes, the draft stays.
pub(crate) async fn undo(state: &State, params: OutboxRef) -> Result<Value, RpcError> {
    let id = params.id;
    let taken = state
        .store
        .write(move |tx| outbox::take_back(tx, id))
        .await
        .map_err(crate::session::internal)?
        .value;
    let Some(draft_id) = taken else {
        return Err(RpcError::new(
            codes::CONFLICT,
            "too late: the message is being sent or has gone",
        ));
    };
    Ok(json!(OutboxUndoResult { draft_id }))
}

pub(crate) async fn send_again(state: &State, params: OutboxRef) -> Result<Value, RpcError> {
    let id = params.id;
    let now = now_ms();
    let queued = state
        .store
        .write(move |tx| outbox::send_again(tx, id, now))
        .await
        .map_err(crate::session::internal)?
        .value;
    if !queued {
        return Err(RpcError::new(
            codes::CONFLICT,
            "only failed or uncertain messages can be sent again",
        ));
    }
    state.outbox.wake();
    Ok(json!({ "queued": true }))
}

pub(crate) async fn dismiss(state: &State, params: OutboxRef) -> Result<Value, RpcError> {
    let id = params.id;
    let dismissed = state
        .store
        .write(move |tx| outbox::dismiss(tx, id))
        .await
        .map_err(crate::session::internal)?
        .value;
    if !dismissed {
        return Err(RpcError::new(
            codes::CONFLICT,
            "only failed or uncertain messages can be dismissed",
        ));
    }
    Ok(json!({ "dismissed": true }))
}

async fn view(state: &State, id: i64) -> Result<Value, RpcError> {
    let found = state
        .store
        .read(move |conn| {
            let Some(draft) = drafts::get(conn, id)? else {
                return Ok(None);
            };
            let account = accounts::get(conn, &draft.account_id)?;
            let files = drafts::attachments(conn, id)?;
            Ok(account.map(|account| (draft, sender(&account), files)))
        })
        .await
        .map_err(crate::session::internal)?;
    let (draft, from, files) = found.ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such draft"))?;
    Ok(json!(draft_view(draft, from, files)))
}

/// Who an account's messages come from: its name (when it is not just the address) and address.
fn sender(account: &accounts::Account) -> Person {
    Person {
        name: Some(account.display_name.clone()).filter(|name| name != &account.email && !name.trim().is_empty()),
        address: Some(account.email.clone()),
    }
}

fn draft_view(draft: Draft, from: Person, files: Vec<drafts::Attachment>) -> DraftView {
    DraftView {
        attachments: files
            .into_iter()
            .map(|file| DraftAttachment {
                id: file.id,
                name: file.name,
                mime: file.mime,
                size: file.size,
            })
            .collect(),
        id: draft.id,
        account_id: draft.account_id,
        from,
        to: draft.to,
        cc: draft.cc,
        bcc: draft.bcc,
        subject: draft.subject,
        body: draft.body,
        html: draft.html,
        replying_to: draft.replying_to,
        updated_at: draft.updated_at,
        sending: draft.sending,
    }
}

fn entry_view(entry: Entry) -> OutboxEntryView {
    OutboxEntryView {
        id: entry.id,
        account_id: entry.account_id,
        draft_id: entry.draft_id,
        subject: entry.subject,
        recipients: entry.recipients,
        state: match entry.state {
            EntryState::Queued => OutboxState::Queued,
            EntryState::Held => OutboxState::Held,
            // Sent entries are not listed; a sending one shows as sending.
            EntryState::Sending | EntryState::Sent => OutboxState::Sending,
            EntryState::Failed => OutboxState::Failed,
            EntryState::Uncertain => OutboxState::Uncertain,
        },
        undo_until: entry.undo_until,
        send_at: entry.send_at,
        next_attempt_at: entry.next_attempt_at,
        attempts: entry.attempts,
        last_error: entry.last_error,
    }
}

fn check_content(to: &[Person], cc: &[Person], bcc: &[Person], body: &str) -> Result<(), RpcError> {
    if to.len() + cc.len() + bcc.len() > MAX_RECIPIENTS {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("at most {MAX_RECIPIENTS} recipients"),
        ));
    }
    if body.len() > MAX_BODY {
        return Err(RpcError::new(codes::TOO_LARGE, "the message text is too long"));
    }
    Ok(())
}

fn conflict(error: StoreError) -> RpcError {
    match error {
        StoreError::Conflict(message) => RpcError::new(codes::CONFLICT, message),
        other => crate::session::internal(other),
    }
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
