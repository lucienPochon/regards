//! The Smart Inbox (plan §5.4): an inbox by section, why a message is where it
//! is, and the person's choices for senders. The rules live in
//! [`regards_core::smart`]; the store keeps every message's section current.
//!
//! At start, mail stored before the Smart Inbox is sorted from what is known;
//! the backfill then reads its headers again ([`crate::backfill`]).

use crate::State;
use crate::compose::now_ms;
use regards_core::mailbox::Role;
use regards_ipc::protocol::{
    AccountRef, ExplainResult, MessageRef, RpcError, SectionCount, SenderSetParams, SenderSetResult, SenderView,
    SmartOverviewResult, codes,
};
use regards_store::{StoreError, mailboxes, messages, smart};
use serde_json::{Value, json};
use std::sync::Arc;

/// Mail sorted at start per write, so other writes are not held up long.
const BATCH: u32 = 500;

pub(crate) async fn overview(state: &State, params: AccountRef) -> Result<Value, RpcError> {
    let account_id = params.account_id;
    let now = now_ms();
    let sections = state
        .store
        .read(move |conn| {
            let inbox = mailboxes::list(conn, &account_id)?
                .into_iter()
                .find(|m| m.role == Some(Role::Inbox));
            let Some(inbox) = inbox else { return Ok(None) };
            Ok(Some(smart::overview(conn, inbox.id, &account_id, now)?))
        })
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such account, or it has no inbox"))?;
    Ok(json!(SmartOverviewResult {
        sections: sections
            .into_iter()
            .map(|(section, total, unread)| SectionCount { section, total, unread })
            .collect(),
    }))
}

/// "Why is this here?": the rule that placed a message, and what is known
/// about its sender.
pub(crate) async fn explain(state: &State, params: MessageRef) -> Result<Value, RpcError> {
    let id = params.id;
    let found = state
        .store
        .read(move |conn| {
            let Some(message) = messages::get(conn, id)? else {
                return Ok(None);
            };
            let Some(mailbox) = mailboxes::get(conn, message.mailbox_id)? else {
                return Ok(None);
            };
            let account = mailbox.account_id;
            let traits: Option<u32> =
                conn.query_row("SELECT traits FROM messages WHERE id = ?1", [id], |row| row.get(0))?;
            let from = message.from.and_then(|person| person.address);
            let own = smart::own_addresses(conn, &account)?;
            let (section, reason) = smart::classify(conn, &account, &own, traits, from.as_deref())?;
            let sender = match from {
                Some(address) => {
                    let known = smart::sender(conn, &account, &address)?;
                    Some(SenderView {
                        address: address.to_lowercase(),
                        policy: known.policy,
                        priority: known.priority,
                        written_to: known.written_to,
                    })
                }
                None => None,
            };
            Ok(Some(ExplainResult {
                section,
                reason,
                sender,
            }))
        })
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such message"))?;
    Ok(json!(found))
}

/// The person's choice for a sender; their mail follows. Corrections persist:
/// mail from them that arrives later is sorted the same way.
pub(crate) async fn set_sender(state: &State, params: SenderSetParams) -> Result<Value, RpcError> {
    let address = params.address.trim().to_owned();
    if !address.contains('@') || address.len() > 320 {
        return Err(RpcError::new(codes::INVALID_PARAMS, "that is not an address"));
    }
    let account_id = params.account_id;
    let changed = state
        .store
        .write(move |tx| {
            if regards_store::accounts::get(tx, &account_id)?.is_none() {
                return Err(StoreError::Conflict("no such account".into()));
            }
            smart::set_sender(tx, &account_id, &address, params.policy, params.priority)
        })
        .await
        .map_err(|error| match error {
            StoreError::Conflict(message) => RpcError::new(codes::NOT_FOUND, message),
            other => crate::session::internal(other),
        })?
        .value;
    Ok(json!(SenderSetResult { changed }))
}

/// Sort mail stored before the Smart Inbox, a batch per write, at start.
pub(crate) async fn sort_stored(state: Arc<State>) {
    let mut sorted = 0;
    loop {
        let pending = match state.store.read(|conn| smart::unclassified(conn, BATCH)).await {
            Ok(pending) if pending.is_empty() => break,
            Ok(pending) => pending,
            Err(error) => {
                tracing::warn!(%error, "sorting mail for the Smart Inbox stopped");
                return;
            }
        };
        let count = pending.len();
        let written = state
            .store
            .write(move |tx| {
                let mut own: Option<(String, Vec<String>)> = None;
                for (id, account) in &pending {
                    if own.as_ref().is_none_or(|(known, _)| known != account) {
                        own = Some((account.clone(), smart::own_addresses(tx, account)?));
                    }
                    let addresses = &own.as_ref().expect("set above").1;
                    smart::reclassify(tx, account, addresses, *id)?;
                }
                Ok(())
            })
            .await;
        if let Err(error) = written {
            tracing::warn!(%error, "sorting mail for the Smart Inbox stopped");
            return;
        }
        sorted += count;
        tokio::task::yield_now().await;
    }
    if sorted > 0 {
        tracing::info!(sorted, "stored mail sorted for the Smart Inbox");
    }
}
