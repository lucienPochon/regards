//! Push: one long-lived connection per account waits in IMAP IDLE on the
//! inbox, and syncs it as soon as the server reports a change (ADR-0009).
//! Servers without IDLE are polled instead.

use crate::State;
use crate::sync::{self, SyncError};
use regards_core::mailbox::Role;
use regards_imap::IdleEnd;
use regards_store::mailboxes;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

/// Re-issue IDLE this often; servers may drop idle sessions after 30 minutes (RFC 2177).
const IDLE_REFRESH: Duration = Duration::from_secs(25 * 60);
/// How often to sync when the server has no IDLE.
const POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Reconnect delays: 1 s, doubling, at most this.
const MAX_BACKOFF: Duration = Duration::from_secs(5 * 60);

/// The running watchers, by account.
#[derive(Default)]
pub(crate) struct Watchers {
    stops: Mutex<HashMap<String, watch::Sender<bool>>>,
}

impl Watchers {
    /// Start watching an account, unless it is watched already.
    pub(crate) fn start(&self, state: &Arc<State>, account_id: &str) {
        let mut stops = self.lock();
        if stops.contains_key(account_id) {
            return;
        }
        let (stop, stopped) = watch::channel(false);
        stops.insert(account_id.to_owned(), stop);
        let (state, account_id) = (Arc::clone(state), account_id.to_owned());
        tokio::spawn(async move {
            run(&state, &account_id, stopped).await;
            state.watchers.lock().remove(&account_id);
        });
    }

    pub(crate) fn stop(&self, account_id: &str) {
        if let Some(stop) = self.lock().remove(account_id) {
            let _ = stop.send(true);
        }
    }

    pub(crate) fn stop_all(&self) {
        for (_, stop) in self.lock().drain() {
            let _ = stop.send(true);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, watch::Sender<bool>>> {
        self.stops.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
    // The server answers again, perhaps after an outage: send actions that wait for a retry.
    crate::queue::Queue::retry_now(state, account_id).await;
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
    loop {
        let stop = {
            let mut stopped = stopped.clone();
            async move { until_stopped(&mut stopped).await }
        };
        match session.idle(IDLE_REFRESH, stop).await? {
            IdleEnd::Changed => {
                // From where the last sync left it, not from when push started.
                let inbox = state
                    .store
                    .read(move |conn| mailboxes::get(conn, inbox_id))
                    .await?
                    .ok_or(SyncError::NoAccount)?;
                let changes = sync::sync_mailbox(state, session, &inbox).await?;
                tracing::debug!(account = %account_id, added = changes.messages_added, "inbox updated by push");
            }
            IdleEnd::Timeout => {}
            IdleEnd::Stopped => return Ok(()),
        }
    }
}

/// Resolves once a stop is requested (or the requester is gone).
async fn until_stopped(stopped: &mut watch::Receiver<bool>) {
    // Drop the borrowed value at once: it must not live across an await.
    let _ = stopped.wait_for(|stop| *stop).await;
}
