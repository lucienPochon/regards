//! Agents (ADR-0012): what each account lets them do, checked here for every
//! request an agent makes, whatever program it comes through (the CLI, MCP).
//! Only the app sets the level; the app itself is never limited by it.

use crate::State;
use regards_core::agents::Access;
use regards_ipc::protocol::{RpcError, codes};
use regards_store::marks::{self, Marks};
use regards_store::{accounts, drafts, mailboxes, messages};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// An account's address and agent access.
pub(crate) async fn current(state: &State, account_id: &str) -> Result<(String, Access), RpcError> {
    let id = account_id.to_owned();
    state
        .store
        .read(move |conn| accounts::agent_access_of(conn, &id))
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such account"))
}

/// Whether `current` access allows `needed`; the refusal names the setting.
pub(crate) fn permit(needed: Access, account_id: &str, email: &str, current: Access) -> Result<(), RpcError> {
    if current >= needed {
        return Ok(());
    }
    // An account closed to agents is not named to them.
    let message = match current {
        Access::Off => "this account is not open to agents. Its owner can open it in the Regards app, \
                        under Settings › Agents."
            .to_owned(),
        _ => format!(
            "{email} allows agents only to {allowed}; this needs agent access {needed}. Its owner can \
             change that in the Regards app, under Settings › Agents.",
            allowed = allows(current),
            needed = needed.label(),
        ),
    };
    let mut error = RpcError::new(codes::FORBIDDEN, message);
    error.data = Some(json!({
        "setting": "agent_access",
        "account_id": account_id,
        "needed": needed,
        "current": current,
    }));
    Err(error)
}

fn allows(access: Access) -> &'static str {
    match access {
        Access::Off => "do nothing",
        Access::Read => "read its mail",
        Access::Triage => "read and organise its mail",
        Access::Send => "read, organise and send",
    }
}

/// The account a mailbox belongs to.
pub(crate) async fn account_of_mailbox(state: &State, mailbox_id: i64) -> Result<String, RpcError> {
    state
        .store
        .read(move |conn| Ok(mailboxes::get(conn, mailbox_id)?.map(|m| m.account_id)))
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such mailbox"))
}

/// The account a message belongs to.
pub(crate) async fn account_of_message(state: &State, message_id: i64) -> Result<String, RpcError> {
    state
        .store
        .read(move |conn| {
            let Some(message) = messages::get(conn, message_id)? else {
                return Ok(None);
            };
            Ok(mailboxes::get(conn, message.mailbox_id)?.map(|m| m.account_id))
        })
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such message"))
}

/// The accounts that allow agents at least `needed`, in list order.
pub(crate) async fn open_to_agents(state: &State, needed: Access) -> Result<Vec<String>, RpcError> {
    let levels = state
        .store
        .read(accounts::agent_access)
        .await
        .map_err(crate::session::internal)?;
    Ok(levels
        .into_iter()
        .filter(|(_, access)| *access >= needed)
        .map(|(id, _)| id)
        .collect())
}

/// The accounts of messages an agent is about to change, and how those
/// messages stand now (kept with its activity).
pub(crate) async fn messages_before(state: &State, ids: &[i64]) -> Result<(Vec<String>, Value), RpcError> {
    let ids = ids.to_vec();
    state
        .store
        .read(move |conn| {
            let mut accounts: BTreeSet<String> = BTreeSet::new();
            let mut before = Vec::new();
            for id in ids {
                let Some(message) = messages::get(conn, id)? else {
                    return Ok(Err(id));
                };
                let Some(mailbox) = mailboxes::get(conn, message.mailbox_id)? else {
                    return Ok(Err(id));
                };
                let marks = match &message.message_id {
                    Some(found) => marks::of(conn, &mailbox.account_id, &[found.as_str()])?
                        .remove(found)
                        .unwrap_or_default(),
                    None => Marks::default(),
                };
                before.push(json!({
                    "id": id,
                    "mailbox_id": message.moving_to.unwrap_or(message.mailbox_id),
                    "subject": message.subject,
                    "seen": message.flags.seen,
                    "flagged": message.flags.flagged,
                    "pinned": marks.pinned_at.is_some(),
                    "aside": marks.aside_at.is_some(),
                    "snoozed_until": marks.snoozed_until,
                    "remind_at": marks.remind_at,
                }));
                accounts.insert(mailbox.account_id);
            }
            Ok(Ok((accounts.into_iter().collect(), Value::Array(before))))
        })
        .await
        .map_err(crate::session::internal)?
        .map_err(|id| RpcError::new(codes::NOT_FOUND, format!("no such message: {id}")))
}

/// The account of a draft an agent is about to touch, and the draft as it is.
pub(crate) async fn draft_before(state: &State, id: i64) -> Result<(String, Value), RpcError> {
    let draft = state
        .store
        .read(move |conn| drafts::get(conn, id))
        .await
        .map_err(crate::session::internal)?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such draft"))?;
    let body: String = draft.body.chars().take(2_000).collect();
    let before = json!({
        "id": draft.id,
        "to": draft.to,
        "cc": draft.cc,
        "bcc": draft.bcc,
        "subject": draft.subject,
        "body": body,
        "sending": draft.sending,
    });
    Ok((draft.account_id, before))
}

/// How a sender stands with an account, before an agent changes it.
pub(crate) async fn sender_before(state: &State, account_id: &str, address: &str) -> Result<Value, RpcError> {
    let (account, who) = (account_id.to_owned(), address.to_owned());
    let sender = state
        .store
        .read(move |conn| regards_store::smart::sender(conn, &account, &who))
        .await
        .map_err(crate::session::internal)?;
    Ok(json!({
        "address": address,
        "policy": sender.policy,
        "priority": sender.priority,
        "blocked": sender.blocked,
    }))
}
