//! Drafts in the server's Drafts folder too (plan §5.9), so other devices and
//! webmail have them; drafts written elsewhere can be picked up here.
//!
//! One task, every pass (10 s):
//!
//! 1. **Settles** what the folder's sync found: a copy gone from the server
//!    takes an unchanged draft with it (sent or thrown away elsewhere), and
//!    stray copies are queued for deletion ([`regards_store::draft_sync`]).
//! 2. **Deletes** the queued copies: of drafts sent or discarded here, and
//!    copies replaced by newer ones.
//! 3. **Uploads** drafts left alone a moment (5 s): the new copy
//!    (with its Bcc, `\Draft` and `\Seen`), then the old one is queued.
//!
//! Only servers with UIDPLUS: without the new copy's UID, replacing it
//! safely is not possible, so drafts then stay on this device.

use crate::State;
use crate::compose::{self, KeptFile};
use crate::sync::{self, SyncError};
use regards_core::headers;
use regards_core::mailbox::Role;
use regards_imap::{Session, SystemFlag};
use regards_ipc::protocol::{DraftListParams, MessageListResult, MessageRef, RpcError, codes};
use regards_store::draft_sync::{self, CopyToDelete};
use regards_store::drafts::{self, Content};
use regards_store::{StoreError, accounts, mailboxes, messages};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Drafts uploaded per pass.
const BATCH: u32 = 20;

pub(crate) async fn run(state: Arc<State>) {
    loop {
        if let Err(error) = pass(&state).await {
            tracing::debug!(%error, "drafts not synced with the server for now");
        }
        tokio::time::sleep(state.draft_pace.0).await;
    }
}

async fn pass(state: &State) -> Result<(), SyncError> {
    // Read first: an idle pass writes nothing, so it wakes no client.
    if state.store.read(draft_sync::unsettled).await? {
        let gone = state.store.write(draft_sync::settle).await?.value;
        if gone > 0 {
            tracing::info!(gone, "drafts sent or discarded elsewhere");
        }
    }
    delete_copies(state).await?;
    upload(state).await
}

async fn delete_copies(state: &State) -> Result<(), SyncError> {
    let queued = state.store.read(draft_sync::copies_to_delete).await?;
    let mut by_account: BTreeMap<String, Vec<CopyToDelete>> = BTreeMap::new();
    for copy in queued {
        by_account.entry(copy.account_id.clone()).or_default().push(copy);
    }
    for (account_id, copies) in by_account {
        let id = account_id.clone();
        let exists = state
            .store
            .read(move |conn| Ok(accounts::get(conn, &id)?.is_some()))
            .await?;
        let mut done: Vec<i64> = Vec::new();
        if exists {
            let mut session = sync::login(state, &account_id).await?;
            let mut by_folder: BTreeMap<i64, Vec<&CopyToDelete>> = BTreeMap::new();
            for copy in &copies {
                by_folder.entry(copy.mailbox_id).or_default().push(copy);
            }
            for (mailbox_id, folder_copies) in by_folder {
                let found = state.store.read(move |conn| mailboxes::get(conn, mailbox_id)).await?;
                if let Some(folder) = found {
                    session.select(&folder.remote_name).await?;
                    let mut uids: Vec<u32> = folder_copies.iter().map(|copy| copy.uid).collect();
                    uids.sort_unstable();
                    match session.delete_messages(&crate::backfill::uid_set(&uids)).await {
                        Ok(()) => {}
                        // Refused (or no UIDPLUS): asking again would not help.
                        Err(regards_imap::ImapError::Protocol(reason)) => {
                            tracing::warn!(mailbox = mailbox_id, %reason, "old draft copies left on the server");
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                done.extend(folder_copies.iter().map(|copy| copy.id));
            }
            session.logout().await;
        } else {
            done.extend(copies.iter().map(|copy| copy.id));
        }
        state
            .store
            .write(move |tx| draft_sync::copies_deleted(tx, &done))
            .await?;
    }
    Ok(())
}

async fn upload(state: &State) -> Result<(), SyncError> {
    let (now, quiet) = (compose::now_ms(), state.draft_pace.1.as_millis() as i64);
    let due = state
        .store
        .read(move |conn| draft_sync::due(conn, now, quiet, BATCH))
        .await?;
    let mut by_account: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for draft in due {
        by_account.entry(draft.account_id).or_default().push(draft.draft_id);
    }
    for (account_id, drafts) in by_account {
        let id = account_id.clone();
        let folder = state
            .store
            .read(move |conn| {
                Ok(mailboxes::list(conn, &id)?
                    .into_iter()
                    .find(|m| m.role == Some(Role::Drafts)))
            })
            .await?;
        let Some(folder) = folder else { continue };
        let mut session: Option<Session> = None;
        for draft_id in drafts {
            let copy = match compose::draft_copy(state, draft_id).await {
                Ok(Some(copy)) => copy,
                Ok(None) => continue,
                Err(error) => {
                    tracing::warn!(draft = draft_id, error = %error.message, "draft not uploaded");
                    continue;
                }
            };
            if session.is_none() {
                let opened = sync::login(state, &account_id).await?;
                if !opened.has_capability("UIDPLUS") {
                    tracing::debug!(account = %account_id, "drafts stay on this device: the server lacks UIDPLUS");
                    opened.logout().await;
                    break;
                }
                session = Some(opened);
            }
            let connection = session.as_mut().expect("signed in above");
            let (account, version, mime) = copy;
            let Some(uid) = connection
                .append(&folder.remote_name, &[SystemFlag::Draft, SystemFlag::Seen], &mime)
                .await?
            else {
                continue;
            };
            let mailbox_id = folder.id;
            state
                .store
                .write(move |tx| draft_sync::uploaded(tx, &account, draft_id, version, mailbox_id, uid))
                .await?;
        }
        if let Some(session) = session {
            session.logout().await;
        }
    }
    Ok(())
}

/// Drafts in the server's Drafts folders that were not written here.
pub(crate) async fn elsewhere(state: &State, params: DraftListParams) -> Result<Value, RpcError> {
    let account_id = params.account_id;
    let rows = state
        .store
        .read(move |conn| messages::drafts_elsewhere(conn, account_id.as_deref()))
        .await
        .map_err(crate::session::internal)?;
    let messages = rows.into_iter().map(crate::mail::plain_summary).collect();
    Ok(json!(MessageListResult { messages, next: None }))
}

/// Pick up a draft written elsewhere: a draft here, whose copy it is.
pub(crate) async fn adopt(state: &State, params: MessageRef) -> Result<Value, RpcError> {
    let (message, source) = crate::mail::load(state, params.id).await?;
    let mailbox_id = message.mailbox_id;
    let folder = state
        .store
        .read(move |conn| mailboxes::get(conn, mailbox_id))
        .await
        .map_err(crate::session::internal)?
        .filter(|folder| folder.role == Some(Role::Drafts))
        .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, "that is not a draft"))?;
    let original = headers::parse(&source);
    let (text, files) = compose::text_and_files(state, source).await?;
    let (uid, message_id, now) = (
        message.uid,
        original.message_id.clone().unwrap_or_default(),
        compose::now_ms(),
    );
    let from = original.from.as_ref().and_then(|person| person.address.clone());
    let id = state
        .store
        .write(move |tx| {
            let account =
                accounts::get(tx, &folder.account_id)?.ok_or_else(|| StoreError::Conflict("no such account".into()))?;
            let identity = match &from {
                Some(address) => regards_store::identities::for_addresses(tx, &account.id, &[address.as_str()])?,
                None => None,
            };
            let content = Content {
                to: original.to,
                cc: original.cc,
                bcc: original.bcc,
                subject: original.subject,
                body: text,
                in_reply_to: original.in_reply_to.first().cloned(),
                references: original.references,
                replying_to: None,
                identity_id: identity.map(|identity| identity.id),
                html: None,
            };
            let id = draft_sync::adopt(tx, &account.id, &content, (folder.id, uid, &message_id), now)?;
            attach(tx, id, &files)?;
            Ok(id)
        })
        .await
        .map_err(|error| match error {
            StoreError::Conflict(message) => RpcError::new(codes::CONFLICT, message),
            other => crate::session::internal(other),
        })?
        .value;
    compose::view(state, id).await
}

fn attach(tx: &rusqlite::Transaction<'_>, draft: i64, files: &[KeptFile]) -> Result<(), StoreError> {
    for (hash, name, mime, size) in files {
        drafts::attach(tx, draft, hash, name, mime, *size)?;
    }
    Ok(())
}
