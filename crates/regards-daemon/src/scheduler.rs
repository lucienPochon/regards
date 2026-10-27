//! Work at a time (ADR-0011): snoozed messages come back, and reminders come
//! due, when their time comes. One task, on the device, in every release.
//!
//! - **Wall clock, checked often:** it never sleeps more than [`MAX_NAP`]
//!   before looking at the time again, so a laptop asleep past a wake time, a
//!   clock change or a time-zone change delays a wake-up by seconds, not hours
//!   (a timer alone counts only time awake on some systems).
//! - **Catch-up:** what came due while Regards was not running happens at
//!   once when it starts.
//! - **Waking** clears the snooze and marks the message unread, here and on
//!   the server, so it is back in the inbox as new, and on top of it.
//! - **Reminders** (§4): if someone else has written in the conversation since
//!   the message, the reminder just goes. Otherwise the message shows on top
//!   of the inbox, wherever it is filed (Sent, usually), until it is done.

use crate::State;
use crate::compose::now_ms;
use regards_ipc::protocol::{DaemonWaitingResult, RpcError};
use regards_store::marks::{self, Mark};
use regards_store::notices::{self, Kind, NewNotice};
use regards_store::{StoreError, accounts, messages, outbox};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

/// The longest the scheduler sleeps before checking the clock again.
const MAX_NAP: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct Scheduler {
    notify: Notify,
}

impl Scheduler {
    /// Something was scheduled or changed: look again now.
    pub(crate) fn wake(&self) {
        self.notify.notify_one();
    }
}

/// What waits for Regards to run (`daemon.waiting`): sends from this
/// computer, snoozes and reminders, for a word before quitting.
pub(crate) async fn waiting(state: &State) -> Result<Value, RpcError> {
    let (sends, (snoozed, reminders)) = state
        .store
        .read(|conn| Ok((outbox::waiting_here(conn)?, marks::waiting(conn)?)))
        .await
        .map_err(crate::session::internal)?;
    Ok(json!(DaemonWaitingResult {
        sends,
        snoozed,
        reminders
    }))
}

pub(crate) async fn run(state: Arc<State>) {
    loop {
        match wake_due(&state).await {
            Ok(0) => {}
            Ok(woken) => tracing::info!(woken, "snoozed messages are back"),
            Err(error) => tracing::warn!(%error, "waking snoozed messages failed"),
        }
        match remind_due(&state).await {
            Ok((0, 0)) => {}
            Ok((shown, answered)) => tracing::info!(shown, answered, "reminders came due"),
            Err(error) => tracing::warn!(%error, "reminders failed"),
        }
        let next = state.store.read(marks::next_wake).await.ok().flatten();
        let nap = next
            .map(|at| Duration::from_millis(u64::try_from(at - now_ms()).unwrap_or(0)))
            .unwrap_or(MAX_NAP)
            .min(MAX_NAP);
        tokio::select! {
            () = tokio::time::sleep(nap) => {}
            () = state.scheduler.notify.notified() => {}
        }
    }
}

/// Wake every snooze whose time has come; returns how many.
async fn wake_due(state: &Arc<State>) -> Result<usize, StoreError> {
    let now = now_ms();
    let due = state.store.read(move |conn| marks::due_snoozes(conn, now)).await?;
    if due.is_empty() {
        return Ok(0);
    }
    let count = due.len();
    let accounts = state
        .store
        .write(move |tx| {
            let mut accounts = BTreeSet::new();
            for (account, message_id) in &due {
                // Snoozed to come back quietly: unread in the inbox, without a notification.
                let quiet = marks::get(tx, account, message_id)?.snooze_quiet.is_some();
                marks::set(tx, account, message_id, Mark::Snoozed, None)?;
                marks::set(tx, account, message_id, Mark::SnoozeQuiet, None)?;
                marks::set(tx, account, message_id, Mark::Surfaced, Some(now))?;
                if !quiet {
                    notice(tx, account, message_id, Kind::SnoozeEnded, now)?;
                }
                if crate::actions::mark_unread_everywhere(tx, account, message_id, now)? {
                    accounts.insert(account.clone());
                }
            }
            Ok(accounts)
        })
        .await?
        .value;
    for account in accounts {
        state.queue.wake(state, &account);
    }
    Ok(count)
}

/// Every reminder whose time has come: shown in the inbox, or gone if it was
/// answered. Returns (shown, answered).
async fn remind_due(state: &Arc<State>) -> Result<(usize, usize), StoreError> {
    let now = now_ms();
    let due = state.store.read(move |conn| marks::due_reminders(conn, now)).await?;
    if due.is_empty() {
        return Ok((0, 0));
    }
    let counted = state
        .store
        .write(move |tx| {
            let (mut shown, mut answered) = (0, 0);
            for (account, message_id) in &due {
                marks::set(tx, account, message_id, Mark::Remind, None)?;
                let Some(own) = accounts::get(tx, account)?.map(|a| a.email) else {
                    continue;
                };
                // No copy stored yet (its Sent copy not synced): shown once it is.
                if marks::replied_after(tx, account, message_id, &own)? == Some(true) {
                    answered += 1;
                    continue;
                }
                marks::set(tx, account, message_id, Mark::Reminded, Some(now))?;
                marks::set(tx, account, message_id, Mark::Surfaced, Some(now))?;
                notice(tx, account, message_id, Kind::ReminderDue, now)?;
                shown += 1;
            }
            Ok((shown, answered))
        })
        .await?;
    Ok(counted.value)
}

/// Tell the person: who (the sender; for a reminder, the recipients) and the
/// subject, from a stored copy (one in the inbox if there is one). Nothing
/// is said about a message with no copy stored.
fn notice(
    tx: &rusqlite::Transaction<'_>,
    account: &str,
    message_id: &str,
    kind: Kind,
    now: i64,
) -> Result<(), StoreError> {
    let copies = messages::copies(tx, account, message_id)?;
    let mut chosen = None;
    for copy in &copies {
        let role = regards_store::mailboxes::get(tx, copy.mailbox_id)?.and_then(|m| m.role);
        if role == Some(regards_core::mailbox::Role::Inbox) || chosen.is_none() {
            chosen = Some(copy);
        }
    }
    let Some(message) = chosen else { return Ok(()) };
    let people = match kind {
        Kind::ReminderDue => message.to.iter().chain(&message.cc).cloned().collect(),
        _ => message.from.clone().into_iter().collect(),
    };
    notices::add(
        tx,
        &NewNotice {
            account_id: account.to_owned(),
            kind,
            message_id: Some(message.id),
            outbox_id: None,
            people,
            subject: message.subject.clone(),
        },
        now,
    )?;
    Ok(())
}
