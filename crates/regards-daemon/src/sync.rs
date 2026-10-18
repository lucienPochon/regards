//! Mail sync with each account's server. For now this is the mailbox list:
//! signed in with the stored password, `LIST`, then reconciled into the store.
//! Message sync and push (IDLE) build on it.

use crate::State;
use regards_core::mailbox::{Candidate, assign_roles};
use regards_imap::{ImapError, ImapServer, Session};
use regards_ipc::protocol::SyncState;
use regards_store::StoreError;
use regards_store::accounts;
use regards_store::mailboxes::{self, Reconciled, RemoteMailbox};
use regards_vault::SecretKey;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Why a sync did not complete, in words for the account owner.
#[derive(Debug, thiserror::Error)]
pub(crate) enum SyncError {
    #[error("the account no longer exists")]
    NoAccount,
    #[error("the saved password is missing; remove the account and add it again")]
    NoPassword,
    #[error("the system credential store is not available: {0}")]
    Vault(String),
    #[error(transparent)]
    Imap(#[from] ImapError),
    #[error("could not save the result: {0}")]
    Store(#[from] StoreError),
}

/// Sync progress per account, for the UI.
#[derive(Default)]
pub(crate) struct Tracker {
    states: Mutex<HashMap<String, SyncState>>,
}

impl Tracker {
    pub(crate) fn get(&self, account_id: &str) -> SyncState {
        self.lock().get(account_id).cloned().unwrap_or_default()
    }

    pub(crate) fn forget(&self, account_id: &str) {
        self.lock().remove(account_id);
    }

    /// Mark a sync as started; `false` if one is already running.
    fn start(&self, account_id: &str) -> bool {
        let mut states = self.lock();
        let state = states.entry(account_id.to_owned()).or_default();
        !std::mem::replace(&mut state.running, true)
    }

    fn finish(&self, account_id: &str, outcome: &Result<Reconciled, SyncError>) {
        let mut states = self.lock();
        let state = states.entry(account_id.to_owned()).or_default();
        state.running = false;
        match outcome {
            Ok(_) => {
                state.last_success_at = Some(now_ms());
                state.last_error = None;
            }
            Err(error) => state.last_error = Some(error.to_string()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, SyncState>> {
        self.states.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Sync the mailbox list of one account. Returns `None` if a sync of that
/// account is already running.
pub(crate) async fn sync_account(state: &State, account_id: &str) -> Option<Result<Reconciled, SyncError>> {
    if !state.sync.start(account_id) {
        return None;
    }
    let outcome = sync_mailboxes(state, account_id).await;
    match &outcome {
        Ok(changes) => tracing::info!(account = %account_id, ?changes, "mailboxes synced"),
        Err(error) => tracing::warn!(account = %account_id, %error, "mailbox sync failed"),
    }
    state.sync.finish(account_id, &outcome);
    Some(outcome)
}

/// Sync every account in the background, for example at startup.
pub(crate) fn spawn_all(state: Arc<State>) {
    tokio::spawn(async move {
        let accounts = match state.store.read(accounts::list).await {
            Ok(accounts) => accounts,
            Err(error) => return tracing::error!(%error, "cannot list accounts to sync"),
        };
        for account in accounts {
            spawn_one(Arc::clone(&state), account.id);
        }
    });
}

pub(crate) fn spawn_one(state: Arc<State>, account_id: String) {
    tokio::spawn(async move {
        sync_account(&state, &account_id).await;
    });
}

async fn sync_mailboxes(state: &State, account_id: &str) -> Result<Reconciled, SyncError> {
    let id = account_id.to_owned();
    let account = state
        .store
        .read(move |conn| accounts::get(conn, &id))
        .await?
        .ok_or(SyncError::NoAccount)?;
    let vault = crate::accounts::vault(state)
        .await
        .map_err(|error| SyncError::Vault(error.message))?;
    let password = vault
        .get(&SecretKey::from_stored(account.secret_key.clone()))
        .await
        .map_err(|error| SyncError::Vault(error.to_string()))?
        .ok_or(SyncError::NoPassword)?;

    let server = ImapServer {
        host: account.imap.host.clone(),
        port: account.imap.port,
    };
    let mut session = Session::login(&server, &account.username, &password, &state.imap_tls).await?;
    let listed = session.list_mailboxes().await;
    session.logout().await;
    let listed = listed?;

    let candidates: Vec<Candidate<'_>> = listed
        .iter()
        .map(|m| Candidate {
            name: &m.name,
            delimiter: m.delimiter,
            attributes: &m.attributes,
        })
        .collect();
    let roles = assign_roles(&candidates);
    let remote: Vec<RemoteMailbox> = listed
        .iter()
        .zip(&candidates)
        .zip(roles)
        .map(|((m, candidate), role)| RemoteMailbox {
            remote_name: m.remote_name.clone(),
            name: m.name.clone(),
            delimiter: m.delimiter,
            role,
            selectable: candidate.is_selectable(),
            attributes: m.attributes.clone(),
        })
        .collect();

    let id = account.id.clone();
    let committed = state
        .store
        .write(move |tx| mailboxes::reconcile(tx, &id, &remote))
        .await?;
    Ok(committed.value)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
