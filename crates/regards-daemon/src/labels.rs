//! Gmail labels (migration 37). Over IMAP, Gmail shows each label as a folder
//! that holds a copy of every message with that label. Adding a label copies
//! the message there; removing one deletes that copy, which Gmail takes as
//! the label removed (the mail stays in All Mail). Both apply here at once,
//! then reach the server through the operation queue (ADR-0010).

use crate::State;
use crate::actions::check_count;
use crate::compose::now_ms;
use regards_ipc::protocol::{
    LabelsResult, MessageIds, MessageLabels, RpcError, SetLabelsParams, SetLabelsResult, codes,
};
use regards_store::operations::{self, FlagName, NewOperation, OperationKind};
use regards_store::{StoreError, labels, mailboxes, messages};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// `message.labels`: each message's labels, as label folder ids.
pub(crate) async fn labels(state: &State, params: MessageIds) -> Result<Value, RpcError> {
    check_count(&params.ids)?;
    let ids = params.ids;
    let found = state
        .store
        .read(move |conn| labels::of(conn, &ids))
        .await
        .map_err(conflict)?;
    Ok(json!(LabelsResult {
        messages: listed(found)
    }))
}

/// `message.set_labels`: the same labels added to, and removed from, each message.
pub(crate) async fn set(state: &Arc<State>, params: SetLabelsParams) -> Result<Value, RpcError> {
    check_count(&params.ids)?;
    if params.add.iter().any(|label| params.remove.contains(label)) {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            "a label cannot be added and removed at once",
        ));
    }
    let now = now_ms();
    let committed = state
        .store
        .write(move |tx| {
            let mut copies: BTreeMap<(i64, i64), BTreeSet<u32>> = BTreeMap::new();
            let mut deletes: BTreeMap<i64, BTreeSet<u32>> = BTreeMap::new();
            let mut gone = Vec::new();
            let mut accounts = BTreeSet::new();
            for id in &params.ids {
                let message = messages::get(tx, *id)?
                    .filter(|message| !message.flags.deleted)
                    .ok_or_else(|| StoreError::Conflict(format!("no message {id}")))?;
                let folder = mailboxes::get(tx, message.mailbox_id)?
                    .ok_or_else(|| StoreError::Conflict("the folder no longer exists".into()))?;
                let message_id = message.message_id.clone().ok_or_else(|| {
                    StoreError::Conflict("This message has no Message-ID, so it cannot be labelled".into())
                })?;
                for label in params.add.iter().chain(&params.remove) {
                    let target =
                        mailboxes::get(tx, *label)?.ok_or_else(|| StoreError::Conflict("no such label".into()))?;
                    if target.account_id != folder.account_id || !labels::is_label_folder(tx, *label)? {
                        return Err(StoreError::Conflict(format!(
                            "{} is not a label of this account",
                            target.name
                        )));
                    }
                }
                let has: BTreeSet<i64> = labels::of(tx, &[*id])?
                    .into_iter()
                    .flat_map(|(_, labels)| labels)
                    .collect();
                for label in &params.add {
                    if has.contains(label) {
                        continue;
                    }
                    labels::add_pending(tx, &folder.account_id, &message_id, *label, now)?;
                    copies
                        .entry((message.mailbox_id, *label))
                        .or_default()
                        .insert(message.uid);
                    accounts.insert(folder.account_id.clone());
                }
                for label in &params.remove {
                    if labels::remove_pending(tx, &folder.account_id, &message_id, *label)? {
                        // Added and removed before the server heard of it.
                        operations::withdraw_copy(tx, message.mailbox_id, message.uid, *label)?;
                    }
                    for (copy, uid) in labels::copies_in(tx, *label, &message_id)? {
                        deletes.entry(*label).or_default().insert(uid);
                        gone.push(copy);
                    }
                    accounts.insert(folder.account_id.clone());
                }
            }
            for ((source, target), uids) in copies {
                let uid_validity = synced(tx, source)?;
                operations::enqueue(
                    tx,
                    &NewOperation {
                        mailbox_id: source,
                        uid_validity,
                        kind: OperationKind::Copy,
                        uids: uids.into_iter().collect(),
                        flags: vec![],
                        target_mailbox_id: Some(target),
                    },
                    now,
                )?;
            }
            for (mailbox_id, uids) in deletes {
                let uid_validity = synced(tx, mailbox_id)?;
                let uids: Vec<u32> = uids.into_iter().collect();
                operations::apply_locally(tx, mailbox_id, &uids, &[FlagName::Deleted], true)?;
                operations::enqueue(
                    tx,
                    &NewOperation {
                        mailbox_id,
                        uid_validity,
                        kind: OperationKind::Delete,
                        uids,
                        flags: vec![],
                        target_mailbox_id: None,
                    },
                    now,
                )?;
            }
            Ok((labels::of(tx, &params.ids)?, gone, accounts))
        })
        .await
        .map_err(conflict)?;
    let (now_labelled, gone, accounts) = committed.value;
    for account in accounts {
        state.queue.wake(state, &account);
    }
    Ok(json!(SetLabelsResult {
        messages: listed(now_labelled),
        gone,
    }))
}

/// The folder's UIDVALIDITY: operations need it, so it must have been synced.
fn synced(tx: &rusqlite::Transaction<'_>, mailbox_id: i64) -> Result<u32, StoreError> {
    let folder =
        mailboxes::get(tx, mailbox_id)?.ok_or_else(|| StoreError::Conflict("the folder no longer exists".into()))?;
    folder
        .uid_validity
        .map(|validity| validity as u32)
        .ok_or_else(|| StoreError::Conflict(format!("{} has not been synced yet", folder.name)))
}

fn listed(found: Vec<(i64, Vec<i64>)>) -> Vec<MessageLabels> {
    found
        .into_iter()
        .map(|(id, labels)| MessageLabels { id, labels })
        .collect()
}

fn conflict(error: StoreError) -> RpcError {
    match error {
        StoreError::Conflict(message) => RpcError::new(codes::CONFLICT, message),
        other => crate::session::internal(other),
    }
}
