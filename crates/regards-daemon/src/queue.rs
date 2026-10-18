//! The operation queue runner (ADR-0010): one task per account sends
//! pending operations to the server, oldest first.
//!
//! - **Connection trouble** (offline, timeouts, TLS): everything still due is
//!   retried later, backing off from 5 s to 10 min.
//! - **A server refusal**, or a mailbox whose UIDVALIDITY changed: the
//!   operation can never apply. It is dropped and the mailbox is synced, which
//!   brings the local copy back to the server's state, and the account's sync
//!   status shows why.

use crate::State;
use crate::sync::{self, SyncError};
use regards_imap::{ImapError, Session, SystemFlag};
use regards_store::mailboxes::{self, Mailbox};
use regards_store::operations::{self, FlagName, Operation, OperationKind};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;

const FIRST_RETRY: Duration = Duration::from_secs(5);
const MAX_RETRY: Duration = Duration::from_secs(10 * 60);

/// One runner per account with pending work.
#[derive(Default)]
pub(crate) struct Queue {
    runners: Mutex<HashMap<String, Arc<Notify>>>,
}

impl Queue {
    /// Make sure a runner is working on `account_id`'s operations now.
    pub(crate) fn wake(&self, state: &Arc<State>, account_id: &str) {
        let mut runners = self.lock();
        if let Some(notify) = runners.get(account_id) {
            notify.notify_one();
            return;
        }
        let notify = Arc::new(Notify::new());
        runners.insert(account_id.to_owned(), Arc::clone(&notify));
        let (state, account_id) = (Arc::clone(state), account_id.to_owned());
        tokio::spawn(async move {
            run(&state, &account_id, notify).await;
            state.queue.lock().remove(&account_id);
        });
    }

    /// Start runners for every account with pending operations, for example at startup.
    pub(crate) fn wake_all(state: &Arc<State>) {
        let state = Arc::clone(state);
        tokio::spawn(async move {
            match state.store.read(operations::accounts_with_pending).await {
                Ok(accounts) => {
                    for account in accounts {
                        state.queue.wake(&state, &account);
                    }
                }
                Err(error) => tracing::error!(%error, "cannot read the operation queue"),
            }
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<Notify>>> {
        self.runners.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

async fn run(state: &Arc<State>, account_id: &str, notify: Arc<Notify>) {
    loop {
        if let Err(error) = process_due(state, account_id).await {
            tracing::warn!(account = %account_id, %error, "operation queue paused");
        }
        let id = account_id.to_owned();
        let next = match state.store.read(move |conn| operations::next_due_at(conn, &id)).await {
            Ok(next) => next,
            Err(_) => return,
        };
        let Some(next) = next else {
            // Nothing left: stop; the next action starts a runner again.
            return;
        };
        let wait = Duration::from_millis(u64::try_from(next - now_ms()).unwrap_or(0));
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = notify.notified() => {}
        }
    }
}

async fn process_due(state: &Arc<State>, account_id: &str) -> Result<(), SyncError> {
    let id = account_id.to_owned();
    let now = now_ms();
    let due = state.store.read(move |conn| operations::due(conn, &id, now)).await?;
    if due.is_empty() {
        return Ok(());
    }
    let mut session = match sync::login(state, account_id).await {
        Ok(session) => session,
        Err(error) => {
            postpone(state, &due, &error.to_string()).await?;
            return Err(error);
        }
    };
    let outcome = send(state, &mut session, account_id, &due).await;
    session.logout().await;
    outcome
}

async fn send(state: &Arc<State>, session: &mut Session, account_id: &str, due: &[Operation]) -> Result<(), SyncError> {
    let mut selected: Option<(i64, u32)> = None;
    let mut mailboxes_cache: HashMap<i64, Option<Mailbox>> = HashMap::new();
    for (index, operation) in due.iter().enumerate() {
        let mailbox = match mailboxes_cache.get(&operation.mailbox_id) {
            Some(mailbox) => mailbox.clone(),
            None => {
                let id = operation.mailbox_id;
                let mailbox = state.store.read(move |conn| mailboxes::get(conn, id)).await?;
                mailboxes_cache.insert(id, mailbox.clone());
                mailbox
            }
        };
        let Some(mailbox) = mailbox else {
            // The mailbox is gone, and with it the operation (cascade); nothing to send.
            continue;
        };

        let result = async {
            let uid_validity = match selected {
                Some((id, validity)) if id == mailbox.id => validity,
                _ => {
                    let validity = session.select(&mailbox.remote_name).await?.uid_validity;
                    selected = Some((mailbox.id, validity));
                    validity
                }
            };
            if uid_validity != operation.uid_validity {
                return Ok(false);
            }
            let uids = operation.uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
            let flags: Vec<SystemFlag> = operation.flags.iter().map(|flag| system_flag(*flag)).collect();
            session
                .store_flags(&uids, operation.kind == OperationKind::AddFlags, &flags)
                .await?;
            Ok::<_, ImapError>(true)
        }
        .await;

        match result {
            Ok(true) => finish(state, operation.id).await?,
            Ok(false) => {
                tracing::warn!(account = %account_id, "dropping an action for a mailbox that was rebuilt on the server");
                give_up(
                    state,
                    account_id,
                    operation,
                    &mailbox,
                    "the folder changed on the server",
                )
                .await?;
            }
            Err(error) if is_transient(&error) => {
                postpone(state, &due[index..], &error.to_string()).await?;
                return Err(error.into());
            }
            Err(error) => {
                tracing::warn!(account = %account_id, %error, "the server refused an action");
                give_up(state, account_id, operation, &mailbox, &error.to_string()).await?;
            }
        }
    }
    Ok(())
}

async fn finish(state: &State, id: i64) -> Result<(), SyncError> {
    state.store.write(move |tx| operations::remove(tx, id)).await?;
    Ok(())
}

/// Drop an operation that can never apply, and resync its mailbox so the
/// local copy shows the server's state again.
async fn give_up(
    state: &Arc<State>,
    account_id: &str,
    operation: &Operation,
    mailbox: &Mailbox,
    reason: &str,
) -> Result<(), SyncError> {
    finish(state, operation.id).await?;
    state.sync.note_error(
        account_id,
        &format!("A change could not be made on the server: {reason}"),
    );
    let (state, account_id, mailbox) = (Arc::clone(state), account_id.to_owned(), mailbox.clone());
    tokio::spawn(async move {
        if let Ok(mut session) = sync::login(&state, &account_id).await {
            let _ = sync::sync_mailbox(&state, &mut session, &mailbox).await;
            session.logout().await;
        }
    });
    Ok(())
}

async fn postpone(state: &State, operations: &[Operation], error: &str) -> Result<(), SyncError> {
    let now = now_ms();
    let updates: Vec<(i64, i64)> = operations
        .iter()
        .map(|operation| {
            let delay = FIRST_RETRY
                .saturating_mul(2u32.saturating_pow(operation.attempts.min(16)))
                .min(MAX_RETRY);
            (operation.id, now + delay.as_millis() as i64)
        })
        .collect();
    let error = error.to_owned();
    state
        .store
        .write(move |tx| {
            for (id, next) in &updates {
                operations::retry_later(tx, *id, &error, *next)?;
            }
            Ok(())
        })
        .await?;
    Ok(())
}

/// Problems that may go away by themselves.
fn is_transient(error: &ImapError) -> bool {
    matches!(
        error,
        ImapError::Connect(_) | ImapError::Timeout(_) | ImapError::Tls(_) | ImapError::NotImap(_)
    )
}

fn system_flag(flag: FlagName) -> SystemFlag {
    match flag {
        FlagName::Seen => SystemFlag::Seen,
        FlagName::Flagged => SystemFlag::Flagged,
        FlagName::Answered => SystemFlag::Answered,
        FlagName::Draft => SystemFlag::Draft,
        FlagName::Deleted => SystemFlag::Deleted,
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
