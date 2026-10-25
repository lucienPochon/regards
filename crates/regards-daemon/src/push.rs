//! Push: one long-lived connection per account waits in IMAP IDLE on the
//! inbox, and syncs it as soon as the server reports a change (ADR-0009).
//! Every few minutes it also syncs the other folders on that connection.
//! Servers without IDLE are polled instead.

use crate::State;
use crate::sync::{self, SyncError};
use regards_core::mailbox::Role;
use regards_imap::IdleEnd;
use regards_store::mailboxes;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;

/// Re-issue IDLE this often; servers may drop idle sessions after 30 minutes (RFC 2177).
const IDLE_REFRESH: Duration = Duration::from_secs(25 * 60);
/// How often to sync when the server has no IDLE.
const POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Reconnect delays: 1 s, doubling, at most this.
const MAX_BACKOFF: Duration = Duration::from_secs(5 * 60);

/// The running watchers, by account.
#[derive(Default)]
pub(crate) struct Watchers {
    running: Mutex<HashMap<String, Watcher>>,
    /// Tells a watcher from one started after it for the same account.
    next: AtomicU64,
    /// Ends the wait of watchers between two failed connections.
    reconnect: Notify,
}

struct Watcher {
    id: u64,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl Watchers {
    /// Start watching an account, unless it is watched already.
    pub(crate) fn start(&self, state: &Arc<State>, account_id: &str) {
        let mut running = self.lock();
        if running.contains_key(account_id) {
            return;
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (stop, stopped) = watch::channel(false);
        let (state, account) = (Arc::clone(state), account_id.to_owned());
        let task = tokio::spawn(async move {
            run(&state, &account, stopped).await;
            // Unless a new watcher took its place meanwhile.
            let mut running = state.watchers.lock();
            if running.get(&account).is_some_and(|watcher| watcher.id == id) {
                running.remove(&account);
            }
        });
        running.insert(account_id.to_owned(), Watcher { id, stop, task });
    }

    /// Stop watching an account. It may still finish a sync: the task
    /// returned ends when it has.
    pub(crate) fn stop(&self, account_id: &str) -> Option<JoinHandle<()>> {
        let watcher = self.lock().remove(account_id)?;
        let _ = watcher.stop.send(true);
        Some(watcher.task)
    }

    /// [`Watchers::stop`], then wait (`max` at most) for the watcher to
    /// finish: a sync of its still running would write over what the caller
    /// does next, such as dropping the account's mail.
    pub(crate) async fn stop_and_wait(&self, account_id: &str, max: Duration) {
        if let Some(task) = self.stop(account_id)
            && tokio::time::timeout(max, task).await.is_err()
        {
            tracing::warn!(account = %account_id, "the push connection did not stop in time");
        }
    }

    pub(crate) fn stop_all(&self) {
        for (_, watcher) in self.lock().drain() {
            let _ = watcher.stop.send(true);
        }
    }

    /// Watchers waiting to connect again (up to [`MAX_BACKOFF`]) try now:
    /// the person asked for mail, perhaps because the network is back.
    pub(crate) fn reconnect_now(&self) {
        self.reconnect.notify_waiters();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Watcher>> {
        self.running.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

async fn run(state: &Arc<State>, account_id: &str, mut stopped: watch::Receiver<bool>) {
    let mut backoff = Duration::from_secs(1);
    loop {
        let mut connected = false;
        match watch_once(state, account_id, &mut stopped, &mut connected).await {
            Ok(()) => return,
            Err(SyncError::NoAccount) => return,
            Err(error) => {
                if connected {
                    // It worked for a while: this is a new problem, not the same one again.
                    backoff = Duration::from_secs(1);
                }
                tracing::warn!(account = %account_id, %error, retry_in = ?backoff, "push connection failed");
                tokio::select! {
                    () = tokio::time::sleep(backoff) => {}
                    () = state.watchers.reconnect.notified() => {}
                    () = until_stopped(&mut stopped) => return,
                }
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

/// Watch until stopped (`Ok`) or the connection fails (`Err`); `connected`
/// tells whether it got as far as signing in.
async fn watch_once(
    state: &Arc<State>,
    account_id: &str,
    stopped: &mut watch::Receiver<bool>,
    connected: &mut bool,
) -> Result<(), SyncError> {
    let mut session = sync::login(state, account_id).await?;
    *connected = true;
    // The server answers again, perhaps after an outage: send actions and
    // messages that wait for a retry.
    crate::queue::Queue::retry_now(state, account_id).await;
    crate::sender::Outbox::retry_now(state, account_id).await;
    let id = account_id.to_owned();
    let inbox = state
        .store
        .read(move |conn| mailboxes::list(conn, &id))
        .await?
        .into_iter()
        .find(|m| m.role == Some(Role::Inbox))
        .ok_or(SyncError::NoAccount)?;

    if !session.has_capability("IDLE") {
        session.logout().await;
        tracing::info!(account = %account_id, "no IDLE on this server; polling every 5 minutes");
        loop {
            tokio::select! {
                () = tokio::time::sleep(POLL_INTERVAL) => { sync::sync_account(state, account_id).await; }
                () = until_stopped(stopped) => return Ok(()),
            }
        }
    }

    // IDLE reports only what changes after it starts. Catch up first, so mail
    // that arrived since the last sync (or while reconnecting) is not missed.
    sync::sync_mailbox(state, &mut session, &inbox).await?;
    tracing::info!(account = %account_id, "waiting for new mail (IDLE)");
    state.sync.set_push(account_id, true);
    let outcome = idle_loop(state, account_id, &mut session, inbox.id, stopped).await;
    state.sync.set_push(account_id, false);
    if outcome.is_ok() {
        session.logout().await;
    }
    outcome
}

async fn idle_loop(
    state: &State,
    account_id: &str,
    session: &mut regards_imap::Session,
    inbox_id: i64,
    stopped: &mut watch::Receiver<bool>,
) -> Result<(), SyncError> {
    let every = state.limits.folders_every;
    let mut folders_due = Instant::now() + every;
    loop {
        let stop = {
            let mut stopped = stopped.clone();
            async move { until_stopped(&mut stopped).await }
        };
        let wait = folders_due
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_secs(1), IDLE_REFRESH);
        match session.idle(wait, stop).await? {
            IdleEnd::Changed if *stopped.borrow() => return Ok(()),
            IdleEnd::Changed => {
                let changes = sync::sync_mailbox(state, session, &inbox(state, inbox_id).await?).await?;
                tracing::debug!(account = %account_id, added = changes.messages_added, "inbox updated by push");
            }
            IdleEnd::Timeout => {}
            IdleEnd::Stopped => return Ok(()),
        }
        if Instant::now() < folders_due {
            continue;
        }
        folders_due = Instant::now() + every;
        // A sync someone asked for (Check for mail) covers them already.
        if state.sync.get(account_id).running {
            continue;
        }
        let changes = sync::sync_other_folders(state, session, account_id, Some(inbox_id)).await?;
        tracing::debug!(account = %account_id, ?changes, "other folders synced");
        // IDLE waits on the selected folder: the inbox again, caught up.
        sync::sync_mailbox(state, session, &inbox(state, inbox_id).await?).await?;
    }
}

/// The inbox as the last sync left it, not as it was when push started.
async fn inbox(state: &State, inbox_id: i64) -> Result<mailboxes::Mailbox, SyncError> {
    state
        .store
        .read(move |conn| mailboxes::get(conn, inbox_id))
        .await?
        .ok_or(SyncError::NoAccount)
}

/// Resolves once a stop is requested (or the requester is gone).
async fn until_stopped(stopped: &mut watch::Receiver<bool>) {
    // Drop the borrowed value at once: it must not live across an await.
    let _ = stopped.wait_for(|stop| *stop).await;
}
