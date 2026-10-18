//! User actions (ADR-0007, ADR-0010): each one changes the local copy and
//! queues what the server must be told in the same transaction, then wakes
//! the account's queue runner. The UI updates at once, online or not.

use crate::State;
use regards_core::mailbox::Role;
use regards_ipc::protocol::{
    DeleteResult, MessageIds, MoveParams, MoveResult, RpcError, SetFlagsParams, SetFlagsResult, UndoInfo, UndoParams,
    UndoResult, codes,
};
use regards_store::operations::{self, FlagName, NewOperation, OperationKind};
use regards_store::{StoreError, mailboxes, messages, undo};
use serde::{Deserialize, Serialize};
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
                        target_mailbox_id: None,
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

/// After a reply went out: mark the message it answers `\Answered`, here and
/// (through the queue) on the server. Returns the account to wake.
pub(crate) fn mark_answered(
    tx: &rusqlite::Transaction<'_>,
    message_id: i64,
    now: i64,
) -> Result<Option<String>, StoreError> {
    let Some(message) = messages::get(tx, message_id)? else {
        return Ok(None);
    };
    if message.flags.answered {
        return Ok(None);
    }
    let Some(mailbox) = mailboxes::get(tx, message.mailbox_id)? else {
        return Ok(None);
    };
    let Some(uid_validity) = mailbox.uid_validity else {
        return Ok(None);
    };
    operations::apply_locally(tx, mailbox.id, &[message.uid], &[FlagName::Answered], true)?;
    operations::enqueue(
        tx,
        &NewOperation {
            mailbox_id: mailbox.id,
            uid_validity: uid_validity as u32,
            kind: OperationKind::AddFlags,
            uids: vec![message.uid],
            flags: vec![FlagName::Answered],
            target_mailbox_id: None,
        },
        now,
    )?;
    Ok(Some(mailbox.account_id))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Delete messages for good. Only what is shown in Trash or Junk, so a
/// mistaken call cannot destroy mail anywhere else; there is no undo.
pub(crate) async fn delete_forever(state: &Arc<State>, params: MessageIds) -> Result<Value, RpcError> {
    check_count(&params.ids)?;
    let ids = params.ids;
    let now = now_ms();
    let committed = state
        .store
        .write(move |tx| {
            let mut groups: BTreeMap<i64, BTreeSet<u32>> = BTreeMap::new();
            for id in &ids {
                let message =
                    messages::get(tx, *id)?.ok_or_else(|| StoreError::Conflict(format!("no message {id}")))?;
                if message.flags.deleted {
                    continue;
                }
                if message.moving_to.is_some() {
                    return Err(StoreError::Conflict(
                        "This message is still being moved. Try again in a moment.".into(),
                    ));
                }
                let mailbox = mailboxes::get(tx, message.mailbox_id)?
                    .ok_or_else(|| StoreError::Conflict("the folder no longer exists".into()))?;
                if !matches!(mailbox.role, Some(Role::Trash | Role::Junk)) {
                    return Err(StoreError::Conflict(
                        "Only messages in Trash or Junk can be deleted for good".into(),
                    ));
                }
                groups.entry(message.mailbox_id).or_default().insert(message.uid);
            }
            let mut deleted = 0;
            let mut accounts = BTreeSet::new();
            for (mailbox_id, uids) in groups {
                let mailbox = mailboxes::get(tx, mailbox_id)?
                    .ok_or_else(|| StoreError::Conflict("the folder no longer exists".into()))?;
                let uid_validity = mailbox
                    .uid_validity
                    .ok_or_else(|| StoreError::Conflict("the folder has not been synced yet".into()))?;
                let uids: Vec<u32> = uids.into_iter().collect();
                operations::apply_locally(tx, mailbox_id, &uids, &[FlagName::Deleted], true)?;
                operations::enqueue(
                    tx,
                    &NewOperation {
                        mailbox_id,
                        uid_validity: uid_validity as u32,
                        kind: OperationKind::Delete,
                        uids: uids.clone(),
                        flags: vec![],
                        target_mailbox_id: None,
                    },
                    now,
                )?;
                deleted += uids.len();
                accounts.insert(mailbox.account_id);
            }
            Ok((deleted, accounts))
        })
        .await
        .map_err(conflict)?;
    let (deleted, accounts) = committed.value;
    for account in accounts {
        state.queue.wake(state, &account);
    }
    Ok(json!(DeleteResult { deleted }))
}

/// Undo entries last this long; the app offers undo for the first seconds.
const UNDO_FOR_MS: i64 = 10 * 60 * 1000;

/// What reverses a set of moves: put each message (id) back where it was seen (folder id).
#[derive(Debug, Serialize, Deserialize)]
struct Inverse {
    moves: Vec<(i64, i64)>,
}

pub(crate) async fn move_to(state: &Arc<State>, params: MoveParams) -> Result<Value, RpcError> {
    check_count(&params.ids)?;
    let target = params.mailbox_id;
    let found = state
        .store
        .read(move |conn| mailboxes::get(conn, target))
        .await
        .map_err(crate::session::internal)?;
    let name = found
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such folder"))?
        .name;
    let requests = params.ids.iter().map(|id| (*id, target)).collect();
    run_moves(state, requests, &format!("Moved to {name}")).await
}

pub(crate) async fn archive(state: &Arc<State>, params: MessageIds) -> Result<Value, RpcError> {
    to_role(
        state,
        params,
        Role::Archive,
        "Archived",
        "This account has no Archive folder",
    )
    .await
}

pub(crate) async fn trash(state: &Arc<State>, params: MessageIds) -> Result<Value, RpcError> {
    to_role(
        state,
        params,
        Role::Trash,
        "Moved to Trash",
        "This account has no Trash folder",
    )
    .await
}

/// Move each message to the folder with `role` in its own account.
async fn to_role(
    state: &Arc<State>,
    params: MessageIds,
    role: Role,
    done: &str,
    missing: &str,
) -> Result<Value, RpcError> {
    check_count(&params.ids)?;
    let ids = params.ids;
    let missing = missing.to_owned();
    let requests = state
        .store
        .read(move |conn| {
            let mut requests = Vec::new();
            for id in ids {
                let message =
                    messages::get(conn, id)?.ok_or_else(|| StoreError::Conflict(format!("no message {id}")))?;
                let mailbox = mailboxes::get(conn, message.mailbox_id)?
                    .ok_or_else(|| StoreError::Conflict("the folder no longer exists".into()))?;
                let target = mailboxes::list(conn, &mailbox.account_id)?
                    .into_iter()
                    .find(|m| m.role == Some(role))
                    .ok_or_else(|| StoreError::Conflict(missing.clone()))?;
                requests.push((id, target.id));
            }
            Ok(requests)
        })
        .await
        .map_err(conflict)?;
    run_moves(state, requests, done).await
}

async fn run_moves(state: &Arc<State>, requests: Vec<(i64, i64)>, done: &str) -> Result<Value, RpcError> {
    let now = now_ms();
    let done = done.to_owned();
    let committed = state
        .store
        .write(move |tx| {
            let (moved, inverse, accounts) = plan_moves(tx, &requests, now)?;
            let undo = if moved > 0 {
                let description = if moved == 1 {
                    done.clone()
                } else {
                    format!("{done} ({moved} messages)")
                };
                let inverse = serde_json::to_string(&Inverse { moves: inverse }).expect("plain data serializes");
                let id = undo::record(tx, &description, &inverse, now, now + UNDO_FOR_MS)?;
                Some(UndoInfo { id, description })
            } else {
                None
            };
            Ok((moved, undo, accounts))
        })
        .await
        .map_err(conflict)?;
    let (moved, undo, accounts) = committed.value;
    for account in accounts {
        state.queue.wake(state, &account);
    }
    Ok(json!(MoveResult { moved, undo }))
}

pub(crate) async fn apply_undo(state: &Arc<State>, params: UndoParams) -> Result<Value, RpcError> {
    let now = now_ms();
    let id = params.id;
    let committed = state
        .store
        .write(move |tx| {
            let Some(inverse) = undo::take(tx, id, now)? else {
                return Err(StoreError::Conflict("this can no longer be undone".into()));
            };
            let inverse: Inverse =
                serde_json::from_str(&inverse).map_err(|error| StoreError::Corrupt(format!("undo entry: {error}")))?;
            // Messages that have since gone (moved without COPYUID, or deleted) are skipped.
            let mut present = Vec::new();
            for (message, folder) in inverse.moves {
                if messages::get(tx, message)?.is_some_and(|m| !m.flags.deleted) {
                    present.push((message, folder));
                }
            }
            let (undone, _, accounts) = plan_moves(tx, &present, now)?;
            Ok((undone, accounts))
        })
        .await
        .map_err(conflict)?;
    let (undone, accounts) = committed.value;
    for account in accounts {
        state.queue.wake(state, &account);
    }
    Ok(json!(UndoResult { undone }))
}

/// What a batch of moves did: how many moved, the inverse, and the accounts to wake.
type Planned = (usize, Vec<(i64, i64)>, BTreeSet<String>);

/// Apply moves (message id, target folder id) to the local copy and queue
/// them, in the caller's transaction.
fn plan_moves(tx: &rusqlite::Transaction<'_>, requests: &[(i64, i64)], now: i64) -> Result<Planned, StoreError> {
    let mut groups: BTreeMap<(i64, i64), BTreeSet<u32>> = BTreeMap::new();
    let mut inverse = Vec::new();
    let mut moved = 0;
    let mut accounts = BTreeSet::new();
    for (id, target) in requests {
        let message = messages::get(tx, *id)?
            .filter(|message| !message.flags.deleted)
            .ok_or_else(|| StoreError::Conflict(format!("no message {id}")))?;
        let source = mailboxes::get(tx, message.mailbox_id)?
            .ok_or_else(|| StoreError::Conflict("the folder no longer exists".into()))?;
        let destination = mailboxes::get(tx, *target)?.ok_or_else(|| StoreError::Conflict("no such folder".into()))?;
        if destination.account_id != source.account_id {
            return Err(StoreError::Conflict(
                "messages can only move within their own account".into(),
            ));
        }
        if !destination.selectable {
            return Err(StoreError::Conflict(format!(
                "{} cannot hold messages",
                destination.name
            )));
        }
        // Where the user sees the message now.
        let shown_in = message.moving_to.unwrap_or(message.mailbox_id);
        if shown_in == *target {
            continue;
        }
        inverse.push((*id, shown_in));
        moved += 1;
        accounts.insert(source.account_id.clone());
        if message.moving_to.is_some() {
            // A change of plan before the server was told: withdraw the pending move.
            operations::withdraw_move(tx, message.mailbox_id, message.uid)?;
            if *target == message.mailbox_id {
                messages::set_moving_to(tx, message.mailbox_id, &[message.uid], None)?;
                continue;
            }
        }
        groups
            .entry((message.mailbox_id, *target))
            .or_default()
            .insert(message.uid);
    }
    for ((source, target), uids) in groups {
        let mailbox =
            mailboxes::get(tx, source)?.ok_or_else(|| StoreError::Conflict("the folder no longer exists".into()))?;
        let uid_validity = mailbox
            .uid_validity
            .ok_or_else(|| StoreError::Conflict("the folder has not been synced yet".into()))?;
        let uids: Vec<u32> = uids.into_iter().collect();
        messages::set_moving_to(tx, source, &uids, Some(target))?;
        operations::enqueue(
            tx,
            &NewOperation {
                mailbox_id: source,
                uid_validity: uid_validity as u32,
                kind: OperationKind::Move,
                uids,
                flags: vec![],
                target_mailbox_id: Some(target),
            },
            now,
        )?;
    }
    Ok((moved, inverse, accounts))
}

fn check_count(ids: &[i64]) -> Result<(), RpcError> {
    if ids.is_empty() || ids.len() > MAX_MESSAGES {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("give 1 to {MAX_MESSAGES} message ids"),
        ));
    }
    Ok(())
}

fn conflict(error: StoreError) -> RpcError {
    match error {
        StoreError::Conflict(message) => RpcError::new(codes::CONFLICT, message),
        other => crate::session::internal(other),
    }
}
