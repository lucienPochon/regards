//! User actions (ADR-0007, ADR-0010): each one changes the local copy and
//! queues what the server must be told in the same transaction, then wakes
//! the account's queue runner. The UI updates at once, online or not.

use crate::State;
use regards_ipc::protocol::{RpcError, SetFlagsParams, SetFlagsResult, codes};
use regards_store::operations::{self, FlagName, NewOperation, OperationKind};
use regards_store::{StoreError, mailboxes, messages};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// At most this many messages per action.
const MAX_MESSAGES: usize = 1000;

pub(crate) async fn set_flags(state: &Arc<State>, params: SetFlagsParams) -> Result<Value, RpcError> {
    if params.ids.is_empty() || params.ids.len() > MAX_MESSAGES {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("give 1 to {MAX_MESSAGES} message ids"),
        ));
    }
    let changes: Vec<(FlagName, bool)> = [(FlagName::Seen, params.seen), (FlagName::Flagged, params.flagged)]
        .into_iter()
        .filter_map(|(flag, value)| value.map(|value| (flag, value)))
        .collect();
    if changes.is_empty() {
        return Err(RpcError::new(codes::INVALID_PARAMS, "say which flags to change"));
    }

    let ids = params.ids;
    let now = now_ms();
    let committed = state
        .store
        .write(move |tx| {
            // Group by mailbox, keeping only messages whose flag really changes.
            let mut work: BTreeMap<(i64, FlagName, bool), BTreeSet<u32>> = BTreeMap::new();
            for id in &ids {
                let Some(message) = messages::get(tx, *id)? else {
                    return Err(StoreError::Conflict(format!("no message {id}")));
                };
                for (flag, value) in &changes {
                    let current = match flag {
                        FlagName::Seen => message.flags.seen,
                        FlagName::Flagged => message.flags.flagged,
                        _ => unreachable!("only seen and flagged are offered"),
                    };
                    if current != *value {
                        work.entry((message.mailbox_id, *flag, *value))
                            .or_default()
                            .insert(message.uid);
                    }
                }
            }
            let mut changed = BTreeSet::new();
            let mut accounts = BTreeSet::new();
            for ((mailbox_id, flag, value), uids) in work {
                let mailbox = mailboxes::get(tx, mailbox_id)?
                    .ok_or_else(|| StoreError::Conflict("the folder no longer exists".into()))?;
                let uid_validity = mailbox
                    .uid_validity
                    .ok_or_else(|| StoreError::Conflict("the folder has not been synced yet".into()))?;
                let uids: Vec<u32> = uids.into_iter().collect();
                operations::apply_locally(tx, mailbox_id, &uids, &[flag], value)?;
                operations::enqueue(
                    tx,
                    &NewOperation {
                        mailbox_id,
                        uid_validity: uid_validity as u32,
                        kind: if value {
                            OperationKind::AddFlags
                        } else {
                            OperationKind::RemoveFlags
                        },
                        uids: uids.clone(),
                        flags: vec![flag],
                    },
                    now,
                )?;
                changed.extend(uids.iter().map(|uid| (mailbox_id, *uid)));
                accounts.insert(mailbox.account_id);
            }
            Ok((changed.len(), accounts))
        })
        .await
        .map_err(|error| match error {
            StoreError::Conflict(message) => RpcError::new(codes::NOT_FOUND, message),
            other => crate::session::internal(other),
        })?;

    let (changed, accounts) = committed.value;
    for account in accounts {
        state.queue.wake(state, &account);
    }
    Ok(json!(SetFlagsResult { changed }))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
