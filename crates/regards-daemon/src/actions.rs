//! User actions (ADR-0007, ADR-0010): each one changes the local copy and
//! queues what the server must be told in the same transaction, then wakes
//! the account's queue runner. The UI updates at once, online or not.

use crate::State;
use regards_core::mailbox::Role;
use regards_ipc::protocol::{
    AsideParams, DeleteResult, MarkAllReadParams, MarkResult, MessageIds, MoveParams, MoveResult, PinParams,
    RemindParams, RpcError, SenderBlockParams, SetFlagsParams, SetFlagsResult, SnoozeParams, UndoInfo, UndoParams,
    UndoResult, codes,
};
use regards_store::marks::{self, Mark};
use regards_store::operations::{self, FlagName, NewOperation, OperationKind};
use regards_store::{StoreError, mailboxes, messages, undo};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// At most this many messages per action.
const MAX_MESSAGES: usize = 1000;
/// UIDs in one server command at most: as ranges, a few kilobytes even when
/// none are next to each other (Dovecot takes 64 KB lines).
const MAX_UIDS_PER_COMMAND: usize = 2000;

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
            let mut wanted = Vec::new();
            for id in &ids {
                if messages::get(tx, *id)?.is_none() {
                    return Err(StoreError::Conflict(format!("no message {id}")));
                }
                wanted.extend(changes.iter().map(|(flag, value)| (*id, *flag, *value)));
            }
            let (done, accounts) = change_flags(tx, &wanted, now)?;
            let changed: BTreeSet<i64> = done.iter().map(|(id, _, _)| *id).collect();
            let undo = if done.is_empty() {
                None
            } else {
                let description = describe_flags(&changes, changed.len());
                let inverse = serde_json::to_string(&Inverse {
                    flags: done,
                    ..Inverse::default()
                })
                .expect("plain data serializes");
                let id = undo::record(tx, &description, &inverse, now, now + UNDO_FOR_MS)?;
                Some(UndoInfo { id, description })
            };
            Ok((changed.len(), undo, accounts))
        })
        .await
        .map_err(|error| match error {
            StoreError::Conflict(message) => RpcError::new(codes::NOT_FOUND, message),
            other => crate::session::internal(other),
        })?;

    let (changed, undo, accounts) = committed.value;
    for account in accounts {
        state.queue.wake(state, &account);
    }
    Ok(json!(SetFlagsResult { changed, undo }))
}

/// `message.mark_all_read`: the folder's unread messages read (those on
/// this device), with one undo.
pub(crate) async fn mark_all_read(state: &Arc<State>, params: MarkAllReadParams) -> Result<Value, RpcError> {
    let now = now_ms();
    let committed = state
        .store
        .write(move |tx| {
            if mailboxes::get(tx, params.mailbox_id)?.is_none() {
                return Err(StoreError::Conflict("no such folder".into()));
            }
            let wanted: Vec<FlagUndo> = messages::unread_in(tx, params.mailbox_id, params.section)?
                .into_iter()
                .map(|id| (id, FlagName::Seen, true))
                .collect();
            let (done, accounts) = change_flags(tx, &wanted, now)?;
            let undo = if done.is_empty() {
                None
            } else {
                let description = describe_flags(&[(FlagName::Seen, true)], done.len());
                let inverse = serde_json::to_string(&Inverse {
                    flags: done.clone(),
                    ..Inverse::default()
                })
                .expect("plain data serializes");
                let id = undo::record(tx, &description, &inverse, now, now + UNDO_FOR_MS)?;
                Some(UndoInfo { id, description })
            };
            Ok((done.len(), undo, accounts))
        })
        .await
        .map_err(|error| match error {
            StoreError::Conflict(message) => RpcError::new(codes::NOT_FOUND, message),
            other => crate::session::internal(other),
        })?;
    let (changed, undo, accounts) = committed.value;
    for account in accounts {
        state.queue.wake(state, &account);
    }
    Ok(json!(SetFlagsResult { changed, undo }))
}

/// A flag set on a message, as undo knows it: the message, the flag, what it was set to.
type FlagUndo = (i64, FlagName, bool);

/// Set each `(message, flag, value)` here, and queue it for the server.
/// What really changed comes back (to undo), with the accounts to sync.
fn change_flags(
    tx: &rusqlite::Transaction<'_>,
    wanted: &[FlagUndo],
    now: i64,
) -> Result<(Vec<FlagUndo>, BTreeSet<String>), StoreError> {
    // Grouped by mailbox, keeping only messages whose flag really changes.
    let mut work: BTreeMap<(i64, FlagName, bool), BTreeSet<u32>> = BTreeMap::new();
    let mut done = Vec::new();
    for (id, flag, value) in wanted {
        let Some(message) = messages::get(tx, *id)? else {
            continue;
        };
        let current = match flag {
            FlagName::Seen => message.flags.seen,
            FlagName::Flagged => message.flags.flagged,
            _ => unreachable!("only seen and flagged are offered"),
        };
        if current != *value {
            work.entry((message.mailbox_id, *flag, *value))
                .or_default()
                .insert(message.uid);
            done.push((*id, *flag, *value));
        }
    }
    let mut accounts = BTreeSet::new();
    for ((mailbox_id, flag, value), uids) in work {
        let mailbox = mailboxes::get(tx, mailbox_id)?
            .ok_or_else(|| StoreError::Conflict("the folder no longer exists".into()))?;
        let uid_validity = mailbox
            .uid_validity
            .ok_or_else(|| StoreError::Conflict("the folder has not been synced yet".into()))?;
        let uids: Vec<u32> = uids.into_iter().collect();
        operations::apply_locally(tx, mailbox_id, &uids, &[flag], value)?;
        // A whole folder at once would make a command line longer than servers take.
        for chunk in uids.chunks(MAX_UIDS_PER_COMMAND) {
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
                    uids: chunk.to_vec(),
                    flags: vec![flag],
                    target_mailbox_id: None,
                },
                now,
            )?;
        }
        accounts.insert(mailbox.account_id);
    }
    Ok((done, accounts))
}

fn describe_flags(changes: &[(FlagName, bool)], count: usize) -> String {
    let messages = if count == 1 {
        "1 message".to_owned()
    } else {
        format!("{count} messages")
    };
    match changes {
        [(FlagName::Seen, true)] => format!("Marked {messages} read"),
        [(FlagName::Seen, false)] => format!("Marked {messages} unread"),
        [(FlagName::Flagged, true)] => format!("Flagged {messages}"),
        [(FlagName::Flagged, false)] => format!("Unflagged {messages}"),
        _ => format!("Changed the flags of {messages}"),
    }
}

/// A snoozed message is back: mark every copy unread, here and (through
/// the queue) on the server, so it returns as new. Returns whether any changed.
pub(crate) fn mark_unread_everywhere(
    tx: &rusqlite::Transaction<'_>,
    account_id: &str,
    message_id: &str,
    now: i64,
) -> Result<bool, StoreError> {
    let mut changed = false;
    for copy in messages::copies(tx, account_id, message_id)? {
        if !copy.flags.seen {
            continue;
        }
        let Some(uid_validity) = mailboxes::get(tx, copy.mailbox_id)?.and_then(|m| m.uid_validity) else {
            continue;
        };
        operations::apply_locally(tx, copy.mailbox_id, &[copy.uid], &[FlagName::Seen], false)?;
        operations::enqueue(
            tx,
            &NewOperation {
                mailbox_id: copy.mailbox_id,
                uid_validity: uid_validity as u32,
                kind: OperationKind::RemoveFlags,
                uids: vec![copy.uid],
                flags: vec![FlagName::Seen],
                target_mailbox_id: None,
            },
            now,
        )?;
        changed = true;
    }
    Ok(changed)
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

pub(crate) async fn pin(state: &State, params: PinParams) -> Result<Value, RpcError> {
    let done = if params.pinned { "Pinned" } else { "Unpinned" };
    mark_messages(state, params.ids, Mark::Pinned, Target::Toggle(params.pinned), done).await
}

pub(crate) async fn set_aside(state: &State, params: AsideParams) -> Result<Value, RpcError> {
    let done = if params.aside { "Set aside" } else { "Back in the inbox" };
    mark_messages(state, params.ids, Mark::Aside, Target::Toggle(params.aside), done).await
}

/// Snooze until a time or "someday", or end a snooze; the scheduler wakes them.
pub(crate) async fn snooze(state: &State, params: SnoozeParams) -> Result<Value, RpcError> {
    let until = if params.someday {
        Some(marks::SOMEDAY)
    } else {
        params.until
    };
    if until.is_some_and(|at| at <= now_ms()) {
        return Err(RpcError::new(codes::INVALID_PARAMS, "choose a time in the future"));
    }
    let done = if until.is_some() {
        "Snoozed"
    } else {
        "Back in the inbox"
    };
    let result = mark_messages(state, params.ids, Mark::Snoozed, Target::Snooze(until), done).await?;
    state.scheduler.wake();
    Ok(result)
}

/// "Remind me if no reply by" a time, or no reminder: either way, done with
/// one that came due (it leaves the inbox). The scheduler checks for replies.
pub(crate) async fn remind(state: &State, params: RemindParams) -> Result<Value, RpcError> {
    if params.at.is_some_and(|at| at <= now_ms()) {
        return Err(RpcError::new(codes::INVALID_PARAMS, "choose a time in the future"));
    }
    let done = if params.at.is_some() {
        "Reminder set"
    } else {
        "Reminder off"
    };
    let result = mark_messages(state, params.ids, Mark::Remind, Target::Remind(params.at), done).await?;
    state.scheduler.wake();
    Ok(result)
}

/// What a mark becomes.
#[derive(Clone, Copy)]
enum Target {
    /// On (from now) or off.
    Toggle(bool),
    /// Snoozed until then, or not.
    Snooze(Option<i64>),
    /// A reminder then, or none; and done with one that came due.
    Remind(Option<i64>),
}

impl Target {
    /// The marks to change on a message that has `current`, and their new values.
    fn changes(self, mark: Mark, current: &marks::Marks, now: i64) -> Vec<(Mark, Option<i64>)> {
        let wanted = match self {
            Self::Toggle(on) if current.get(mark).is_some() == on => vec![],
            Self::Toggle(on) => vec![(mark, on.then_some(now))],
            Self::Snooze(until) => vec![(mark, until)],
            Self::Remind(at) if current.reminded_at.is_some() => {
                vec![(Mark::Remind, at), (Mark::Reminded, None), (Mark::Surfaced, None)]
            }
            Self::Remind(at) => vec![(Mark::Remind, at)],
        };
        wanted
            .into_iter()
            .filter(|(mark, value)| current.get(*mark) != *value)
            .collect()
    }
}

/// Put a mark on messages, or take it off; one undo entry for all of them.
async fn mark_messages(
    state: &State,
    ids: Vec<i64>,
    mark: Mark,
    target: Target,
    done: &str,
) -> Result<Value, RpcError> {
    check_count(&ids)?;
    let now = now_ms();
    let done = done.to_owned();
    let committed = state
        .store
        .write(move |tx| {
            let mut seen = BTreeSet::new();
            let mut inverse = Vec::new();
            let mut changed = 0;
            for id in &ids {
                let message =
                    messages::get(tx, *id)?.ok_or_else(|| StoreError::Conflict(format!("no message {id}")))?;
                let account = mailboxes::get(tx, message.mailbox_id)?
                    .ok_or_else(|| StoreError::Conflict("the folder no longer exists".into()))?
                    .account_id;
                let message_id = message.message_id.clone().ok_or_else(|| {
                    StoreError::Conflict("this message has no Message-ID, so it cannot be marked".into())
                })?;
                // Copies of one message (Inbox and All Mail, say) carry one mark.
                if !seen.insert((account.clone(), message_id.clone())) {
                    continue;
                }
                let current = marks::get(tx, &account, &message_id)?;
                let changes = target.changes(mark, &current, now);
                if changes.is_empty() {
                    continue;
                }
                changed += 1;
                for (mark, value) in changes {
                    let before = marks::set(tx, &account, &message_id, mark, value)?;
                    inverse.push((account.clone(), message_id.clone(), mark, before));
                }
            }
            let undo = if changed > 0 {
                let description = if changed == 1 {
                    done.clone()
                } else {
                    format!("{done} ({changed} messages)")
                };
                let inverse = serde_json::to_string(&Inverse {
                    marks: inverse,
                    ..Inverse::default()
                })
                .expect("plain data serializes");
                let id = undo::record(tx, &description, &inverse, now, now + UNDO_FOR_MS)?;
                Some(UndoInfo { id, description })
            } else {
                None
            };
            Ok(MarkResult { changed, undo })
        })
        .await
        .map_err(conflict)?;
    Ok(json!(committed.value))
}

/// Undo entries last this long; the app offers undo for the first seconds.
const UNDO_FOR_MS: i64 = 10 * 60 * 1000;

/// A mark to put back on undo: account, Message-ID, which mark, what it was.
type MarkUndo = (String, String, Mark, Option<i64>);

/// What reverses a set of moves: put each message (id) back where it was seen (folder id).
#[derive(Debug, Default, Serialize, Deserialize)]
struct Inverse {
    #[serde(default)]
    moves: Vec<(i64, i64)>,
    /// Marks to put back: account, Message-ID, which mark, what it was.
    #[serde(default)]
    marks: Vec<MarkUndo>,
    /// Senders (account, address or `@domain`) to unblock.
    #[serde(default)]
    unblock: Vec<(String, String)>,
    /// Flags to set back, where they are still as the action left them:
    /// one changed since, by the person or another client, is theirs.
    #[serde(default)]
    flags: Vec<FlagUndo>,
    /// Notes an agent added, to remove (ADR-0020).
    #[serde(default)]
    notes_added: Vec<i64>,
    /// Notes removed, to put back as they were.
    #[serde(default)]
    notes_removed: Vec<crate::notes::Kept>,
    /// Drafts deleted, to make again as they were.
    #[serde(default)]
    drafts_deleted: Vec<crate::compose::KeptDraft>,
}

/// Keep how to undo a draft's deletion, for a while.
pub(crate) fn record_draft_undo(
    tx: &rusqlite::Transaction<'_>,
    kept: crate::compose::KeptDraft,
    now: i64,
) -> Result<UndoInfo, StoreError> {
    let description = "Deleted a draft";
    let inverse = serde_json::to_string(&Inverse {
        drafts_deleted: vec![kept],
        ..Inverse::default()
    })
    .expect("plain data serializes");
    let id = undo::record(tx, description, &inverse, now, now + UNDO_FOR_MS)?;
    Ok(UndoInfo {
        id,
        description: description.to_owned(),
    })
}

/// Keep how to undo notes added or removed, for a while.
pub(crate) fn record_notes_undo(
    tx: &rusqlite::Transaction<'_>,
    description: &str,
    added: Vec<i64>,
    removed: Vec<crate::notes::Kept>,
    now: i64,
) -> Result<UndoInfo, StoreError> {
    let inverse = serde_json::to_string(&Inverse {
        notes_added: added,
        notes_removed: removed,
        ..Inverse::default()
    })
    .expect("plain data serializes");
    let id = undo::record(tx, description, &inverse, now, now + UNDO_FOR_MS)?;
    Ok(UndoInfo {
        id,
        description: description.to_owned(),
    })
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
    run_moves(state, requests, Vec::new(), &format!("Moved to {name}")).await
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

/// Move each message to the folder with `role` in its own account. Archiving
/// ("done") a reminder shown in the inbox from another folder (Sent, usually)
/// ends the reminder instead: the message stays filed where it is.
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
    let (requests, reminders) = state
        .store
        .read(move |conn| {
            let mut requests = Vec::new();
            let mut reminders = Vec::new();
            for id in ids {
                let message =
                    messages::get(conn, id)?.ok_or_else(|| StoreError::Conflict(format!("no message {id}")))?;
                let mailbox = mailboxes::get(conn, message.mailbox_id)?
                    .ok_or_else(|| StoreError::Conflict("the folder no longer exists".into()))?;
                if role == Role::Archive
                    && let Some(message_id) = &message.message_id
                {
                    let shown_in = mailboxes::get(conn, message.moving_to.unwrap_or(message.mailbox_id))?;
                    let in_inbox = shown_in.is_some_and(|m| m.role == Some(Role::Inbox));
                    if !in_inbox && marks::get(conn, &mailbox.account_id, message_id)?.reminded_at.is_some() {
                        reminders.push((mailbox.account_id.clone(), message_id.clone()));
                        continue;
                    }
                }
                let folders = mailboxes::list(conn, &mailbox.account_id)?;
                let target = destination(&folders, role).ok_or_else(|| StoreError::Conflict(missing.clone()))?;
                requests.push((id, target.id));
            }
            Ok((requests, reminders))
        })
        .await
        .map_err(conflict)?;
    run_moves(state, requests, reminders, done).await
}

/// The folder `role` puts a message in. Archiving goes to the Archive
/// folder, or, in an account without one, to All Mail (Gmail): out of the
/// inbox and still kept, as Gmail itself archives.
fn destination(folders: &[mailboxes::Mailbox], role: Role) -> Option<&mailboxes::Mailbox> {
    let with = |role| folders.iter().find(|folder| folder.role == Some(role));
    with(role).or_else(|| if role == Role::Archive { with(Role::All) } else { None })
}

/// Apply moves, and end the reminders shown in the inbox in `reminders`
/// (account, Message-ID); one undo entry for all of it.
async fn run_moves(
    state: &Arc<State>,
    requests: Vec<(i64, i64)>,
    reminders: Vec<(String, String)>,
    done: &str,
) -> Result<Value, RpcError> {
    let now = now_ms();
    let done = done.to_owned();
    let committed = state
        .store
        .write(move |tx| {
            let (moved, inverse, accounts) = plan_moves(tx, &requests, now)?;
            let mut marks = leave_the_inbox(tx, &inverse)?;
            for (account, message_id) in &reminders {
                clear_came_back(tx, account, message_id, &mut marks)?;
            }
            let moved = moved + reminders.len();
            let undo = if moved > 0 {
                let description = if moved == 1 {
                    done.clone()
                } else {
                    format!("{done} ({moved} messages)")
                };
                let inverse = serde_json::to_string(&Inverse {
                    moves: inverse,
                    marks,
                    ..Inverse::default()
                })
                .expect("plain data serializes");
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
            let (moved, _, accounts) = plan_moves(tx, &present, now)?;
            for (account, message_id, mark, before) in &inverse.marks {
                marks::set(tx, account, message_id, *mark, *before)?;
            }
            for (account, key) in &inverse.unblock {
                regards_store::smart::set_blocked(tx, account, key, false)?;
            }
            let mut back = Vec::new();
            for (id, flag, set_to) in &inverse.flags {
                let still = messages::get(tx, *id)?.is_some_and(|message| match flag {
                    FlagName::Seen => message.flags.seen == *set_to,
                    FlagName::Flagged => message.flags.flagged == *set_to,
                    _ => false,
                });
                if still {
                    back.push((*id, *flag, !*set_to));
                }
            }
            let (flags, flag_accounts) = change_flags(tx, &back, now)?;
            let mut accounts = accounts;
            accounts.extend(flag_accounts);
            let mut notes = 0;
            for id in &inverse.notes_added {
                notes += usize::from(regards_store::notes::remove(tx, *id)?.is_some());
            }
            for kept in inverse.notes_removed {
                notes += usize::from(regards_store::notes::restore(tx, &kept.into())?);
            }
            let mut drafts = 0;
            for kept in &inverse.drafts_deleted {
                drafts += usize::from(crate::compose::restore_draft(tx, kept, now)?.is_some());
            }
            Ok((
                moved + inverse.marks.len() + inverse.unblock.len() + flags.len() + notes + drafts,
                accounts,
            ))
        })
        .await
        .map_err(conflict)?;
    let (undone, accounts) = committed.value;
    for account in accounts {
        state.queue.wake(state, &account);
    }
    Ok(json!(UndoResult { undone }))
}

/// Moved messages no longer count as come back to the inbox, nor as
/// reminders shown there; returns the marks to put back on undo.
fn leave_the_inbox(tx: &rusqlite::Transaction<'_>, moved: &[(i64, i64)]) -> Result<Vec<MarkUndo>, StoreError> {
    let mut seen = BTreeSet::new();
    let mut inverse = Vec::new();
    for (id, _) in moved {
        let Some(message) = messages::get(tx, *id)? else {
            continue;
        };
        let (Some(message_id), Some(mailbox)) = (message.message_id, mailboxes::get(tx, message.mailbox_id)?) else {
            continue;
        };
        if !seen.insert((mailbox.account_id.clone(), message_id.clone())) {
            continue;
        }
        clear_came_back(tx, &mailbox.account_id, &message_id, &mut inverse)?;
    }
    Ok(inverse)
}

/// Take a message out of what came back to the inbox: its place on top, and a
/// reminder shown there. What it was goes into `inverse`.
fn clear_came_back(
    tx: &rusqlite::Transaction<'_>,
    account_id: &str,
    message_id: &str,
    inverse: &mut Vec<MarkUndo>,
) -> Result<(), StoreError> {
    let current = marks::get(tx, account_id, message_id)?;
    for mark in [Mark::Surfaced, Mark::Reminded] {
        if current.get(mark).is_some() {
            let before = marks::set(tx, account_id, message_id, mark, None)?;
            inverse.push((account_id.to_owned(), message_id.to_owned(), mark, before));
        }
    }
    Ok(())
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

pub(crate) fn check_count(ids: &[i64]) -> Result<(), RpcError> {
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

/// Block a sender, or every address of an `@domain`: their mail in the inbox
/// goes to Junk, and so does what arrives from them later (see
/// [`junk_blocked`]). Undo brings the mail back and unblocks. Unblocking
/// moves nothing back by itself.
pub(crate) async fn block(state: &Arc<State>, params: SenderBlockParams) -> Result<Value, RpcError> {
    let raw = params.address.trim();
    let key = if raw.starts_with('@') {
        regards_core::smart::domain_key(raw)
    } else {
        raw.contains('@').then(|| raw.to_lowercase())
    }
    .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, "give an address, or a domain as @example.org"))?;
    let (account_id, blocked, now) = (params.account_id, params.blocked, now_ms());
    let committed = state
        .store
        .write(move |tx| {
            let folders = mailboxes::list(tx, &account_id)?;
            if folders.is_empty() && regards_store::accounts::get(tx, &account_id)?.is_none() {
                return Err(StoreError::Conflict("no such account".into()));
            }
            regards_store::smart::set_blocked(tx, &account_id, &key, blocked)?;
            if !blocked {
                return Ok((0, None, BTreeSet::new()));
            }
            let role = |role| folders.iter().find(|m| m.role == Some(role)).map(|m| m.id);
            let junk =
                role(Role::Junk).ok_or_else(|| StoreError::Conflict("This account has no Junk folder".into()))?;
            let requests: Vec<(i64, i64)> = match role(Role::Inbox) {
                Some(inbox) => regards_store::smart::from_sender(tx, &account_id, &key, Some(inbox))?
                    .into_iter()
                    .map(|id| (id, junk))
                    .collect(),
                None => Vec::new(),
            };
            let (moved, inverse, accounts) = plan_moves(tx, &requests, now)?;
            let marks = leave_the_inbox(tx, &inverse)?;
            let description = format!("Blocked {key}");
            let inverse = serde_json::to_string(&Inverse {
                moves: inverse,
                marks,
                unblock: vec![(account_id.clone(), key.clone())],
                ..Inverse::default()
            })
            .expect("plain data serializes");
            let id = undo::record(tx, &description, &inverse, now, now + UNDO_FOR_MS)?;
            Ok((moved, Some(UndoInfo { id, description }), accounts))
        })
        .await
        .map_err(conflict)?;
    let (moved, undo, accounts) = committed.value;
    for account in accounts {
        state.queue.wake(state, &account);
    }
    Ok(json!(MoveResult { moved, undo }))
}

/// New mail in an inbox from a blocked sender goes to Junk, as a move the
/// queue tells the server about. Returns the accounts whose queue to wake.
pub(crate) fn junk_blocked(
    tx: &rusqlite::Transaction<'_>,
    mailbox_id: i64,
    uids: &[u32],
    now: i64,
) -> Result<BTreeSet<String>, StoreError> {
    let Some(inbox) = mailboxes::get(tx, mailbox_id)?.filter(|m| m.role == Some(Role::Inbox)) else {
        return Ok(BTreeSet::new());
    };
    let Some(junk) = mailboxes::list(tx, &inbox.account_id)?
        .into_iter()
        .find(|m| m.role == Some(Role::Junk))
    else {
        return Ok(BTreeSet::new());
    };
    let mut requests = Vec::new();
    let mut lookup = tx.prepare_cached("SELECT id, from_address FROM messages WHERE mailbox_id = ?1 AND uid = ?2")?;
    for uid in uids {
        let found: Option<(i64, Option<String>)> = lookup
            .query_row(rusqlite::params![mailbox_id, uid], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        if let Some((id, Some(from))) = found
            && regards_store::smart::sender(tx, &inbox.account_id, &from)?.blocked
        {
            requests.push((id, junk.id));
        }
    }
    if requests.is_empty() {
        return Ok(BTreeSet::new());
    }
    let (_, _, accounts) = plan_moves(tx, &requests, now)?;
    Ok(accounts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: i64, name: &str, role: Option<Role>) -> mailboxes::Mailbox {
        mailboxes::Mailbox {
            id,
            account_id: "a".into(),
            remote_name: name.as_bytes().to_vec(),
            name: name.into(),
            delimiter: Some('/'),
            role,
            selectable: true,
            attributes: Vec::new(),
            uid_validity: None,
            uid_next: None,
            highest_modseq: None,
            synced_at: None,
        }
    }

    #[test]
    fn archiving_goes_to_the_archive_or_else_to_all_mail() {
        let gmail = [
            folder(1, "INBOX", Some(Role::Inbox)),
            folder(2, "[Gmail]/All Mail", Some(Role::All)),
            folder(3, "[Gmail]/Trash", Some(Role::Trash)),
        ];
        assert_eq!(destination(&gmail, Role::Archive).map(|f| f.id), Some(2));
        assert_eq!(destination(&gmail, Role::Trash).map(|f| f.id), Some(3));
        let both = [
            folder(4, "Archive", Some(Role::Archive)),
            folder(5, "All", Some(Role::All)),
        ];
        assert_eq!(destination(&both, Role::Archive).map(|f| f.id), Some(4));
        let neither = [folder(6, "INBOX", Some(Role::Inbox))];
        assert!(destination(&neither, Role::Archive).is_none());
        assert!(destination(&gmail, Role::Junk).is_none(), "only archiving falls back");
    }
}
