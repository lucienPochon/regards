//! Writing and sending mail (Phase 2): drafts, replies and forwards, and the
//! outbox's user-facing side. Sending itself is the outbox runner's job
//! ([`crate::outbox`]). Only the `ui` principal reaches these for now; agents
//! get them through access levels and holds (ADR-0012).

use crate::State;
use regards_core::compose::{self, Outgoing};
use regards_core::headers::{self, Person};
use regards_ipc::protocol::{
    DraftCreateParams, DraftRef, DraftReplyParams, DraftSendParams, DraftSendResult, DraftUpdateParams, DraftView,
    MessageRef, OutboxEntryView, OutboxListResult, OutboxRef, OutboxState, OutboxUndoResult, RpcError, codes,
};
use regards_store::drafts::{self, Content, Draft, Edit};
use regards_store::outbox::{self, Entry, NewEntry, State as EntryState};
use regards_store::{StoreError, accounts, mailboxes};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// ADR-0010 §4: undo send for 10 s by default, at most 30.
const DEFAULT_UNDO_SECONDS: u32 = 10;
const MAX_UNDO_SECONDS: u32 = 30;
/// More recipients than this is a mailing, not a message.
const MAX_RECIPIENTS: usize = 100;
/// Bodies larger than this are not a typed message.
const MAX_BODY: usize = 2 * 1024 * 1024;

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
    let text = regards_core::body::parse(&source).text;
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
    store_new(state, account_id, content).await
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

pub(crate) async fn list(state: &State, params: regards_ipc::protocol::AccountRef) -> Result<Value, RpcError> {
    let account_id = params.account_id;
    let drafts = state
        .store
        .read(move |conn| drafts::list(conn, &account_id))
        .await
        .map_err(crate::session::internal)?;
    Ok(json!({ "drafts": drafts.into_iter().map(draft_view).collect::<Vec<_>>() }))
}

/// Autosave: the whole of what the draft says.
pub(crate) async fn update(state: &State, params: DraftUpdateParams) -> Result<Value, RpcError> {
    check_content(&params.to, &params.cc, &params.bcc, &params.body)?;
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
                body: &params.body,
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

/// Freeze the draft's exact bytes into the outbox. It goes once the undo
/// window is over; until then [`undo`] takes it back.
pub(crate) async fn send(state: &Arc<State>, params: DraftSendParams) -> Result<Value, RpcError> {
    let undo_seconds = params.undo_seconds.unwrap_or(DEFAULT_UNDO_SECONDS);
    if undo_seconds > MAX_UNDO_SECONDS {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("undo_seconds must be 0 to {MAX_UNDO_SECONDS}"),
        ));
    }
    let id = params.id;
    let now = now_ms();
    let undo_until = now + i64::from(undo_seconds) * 1000;
    let outbox_id = state
        .store
        .write(move |tx| {
            let draft = drafts::get(tx, id)?.ok_or_else(|| StoreError::Conflict("no such draft".into()))?;
            if draft.sending {
                return Err(StoreError::Conflict("this message is already being sent".into()));
            }
            let account =
                accounts::get(tx, &draft.account_id)?.ok_or_else(|| StoreError::Conflict("no such account".into()))?;
            let outgoing = Outgoing {
                from: Person {
                    name: Some(account.display_name.clone()).filter(|name| name != &account.email),
                    address: Some(account.email.clone()),
                },
                to: draft.to.clone(),
                cc: draft.cc.clone(),
                bcc: draft.bcc.clone(),
                subject: draft.subject.clone(),
                text: draft.body.clone(),
                message_id: compose::new_message_id(&account.email),
                in_reply_to: draft.in_reply_to.clone(),
                references: draft.references.clone(),
                date: now,
            };
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
                answering: draft.replying_to,
            };
            outbox::queue(tx, &entry, now)
        })
        .await
        .map_err(conflict)?
        .value;
    state.outbox.wake();
    Ok(json!(DraftSendResult { outbox_id, undo_until }))
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
    let draft = state
        .store
        .read(move |conn| drafts::get(conn, id))
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such draft"))?;
    Ok(json!(draft_view(draft)))
}

fn draft_view(draft: Draft) -> DraftView {
    DraftView {
        id: draft.id,
        account_id: draft.account_id,
        to: draft.to,
        cc: draft.cc,
        bcc: draft.bcc,
        subject: draft.subject,
        body: draft.body,
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
            // Sent entries are not listed; a sending one shows as sending.
            EntryState::Sending | EntryState::Sent => OutboxState::Sending,
            EntryState::Failed => OutboxState::Failed,
            EntryState::Uncertain => OutboxState::Uncertain,
        },
        undo_until: entry.undo_until,
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
