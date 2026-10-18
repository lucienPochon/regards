//! Work at a time (ADR-0011): snoozed messages come back when their time
//! comes. One task, on the device, in every release.
//!
//! - **Wall clock, checked often:** it never sleeps more than [`MAX_NAP`]
//!   before looking at the time again, so a laptop asleep past a wake time, a
//!   clock change or a time-zone change delays a wake-up by seconds, not hours
//!   (a timer alone counts only time awake on some systems).
//! - **Catch-up:** snoozes that came due while Regards was not running wake at
//!   once when it starts.
//! - **Waking** clears the snooze and marks the message unread, here and on
//!   the server, so it is back in the inbox as new.

use crate::State;
use crate::compose::now_ms;
use regards_store::StoreError;
use regards_store::marks::{self, Mark};
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

pub(crate) async fn run(state: Arc<State>) {
    loop {
        match wake_due(&state).await {
            Ok(0) => {}
            Ok(woken) => tracing::info!(woken, "snoozed messages are back"),
            Err(error) => tracing::warn!(%error, "waking snoozed messages failed"),
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
                marks::set(tx, account, message_id, Mark::Snoozed, None)?;
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
