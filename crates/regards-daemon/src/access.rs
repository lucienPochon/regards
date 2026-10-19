//! Agents (ADR-0012): what each account lets them do, checked here for every
//! request an agent makes, whatever program it comes through (the CLI, MCP).
//! Only the app sets the level; the app itself is never limited by it.

use crate::State;
use regards_core::agents::Access;
use regards_ipc::protocol::{RpcError, codes};
use regards_store::{accounts, mailboxes, messages};
use serde_json::json;

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
