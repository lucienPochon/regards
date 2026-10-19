//! Downloading recent inbox mail ahead (Settings › General, off by default):
//! the messages then open at once, offline too, and their whole text is
//! searchable. While on, every [`PASS_EVERY`] and as soon as it is turned on,
//! the inboxes' messages of the last [`RECENT_DAYS`] days not downloaded yet
//! are fetched, newest first, at most [`PER_PASS`] at a time and none larger
//! than [`MAX_SIZE`], with one connection per account.

use crate::{State, mail, sync};
use regards_ipc::protocol::{PrefetchParams, RpcError};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

const PASS_EVERY: Duration = Duration::from_secs(10 * 60);
const RECENT_DAYS: i64 = 30;
const PER_PASS: u32 = 100;
const MAX_SIZE: i64 = 25 * 1024 * 1024;
/// The settings key.
const KEY: &str = "prefetch";

#[derive(Default)]
pub(crate) struct Prefetch {
    wake: Notify,
}

pub(crate) async fn run(state: Arc<State>) {
    loop {
        if is_on(&state).await
            && let Err(error) = pass(&state).await
        {
            tracing::info!(%error, "downloading ahead paused");
        }
        tokio::select! {
            () = tokio::time::sleep(PASS_EVERY) => {}
            () = state.prefetch.wake.notified() => {}
        }
    }
}

async fn is_on(state: &State) -> bool {
    state
        .store
        .read(|conn| regards_store::settings::get(conn, KEY))
        .await
        .ok()
        .flatten()
        .is_some_and(|value| value == "on")
}

/// One pass: what is due, by account, one connection each.
async fn pass(state: &State) -> Result<(), String> {
    let since = crate::compose::now_ms() - RECENT_DAYS * 86_400_000;
    let due = state
        .store
        .read(move |conn| regards_store::messages::to_prefetch(conn, since, MAX_SIZE, PER_PASS))
        .await
        .map_err(|error| error.to_string())?;
    let mut by_account: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for (id, account_id) in due {
        by_account.entry(account_id).or_default().push(id);
    }
    let mut fetched = 0;
    for (account_id, ids) in by_account {
        // An account that cannot sign in now waits for the next pass; the others go on.
        let mut session = match sync::login(state, &account_id).await {
            Ok(session) => session,
            Err(error) => {
                tracing::debug!(account = %account_id, %error, "not downloading ahead for now");
                continue;
            }
        };
        for id in ids {
            match mail::fetch_with(state, &mut session, id).await {
                Ok(true) => fetched += 1,
                Ok(false) => {}
                Err(error) => {
                    tracing::debug!(message = id, error = %error.message, "not downloaded ahead");
                }
            }
        }
        session.logout().await;
    }
    if fetched > 0 {
        tracing::info!(fetched, "recent mail downloaded ahead");
    }
    Ok(())
}

/// `prefetch.get`.
pub(crate) async fn get(state: &State) -> Result<Value, RpcError> {
    Ok(json!(PrefetchParams { on: is_on(state).await }))
}

/// `prefetch.set`: on at once (a pass starts), or off.
pub(crate) async fn set(state: &State, params: PrefetchParams) -> Result<Value, RpcError> {
    let value = if params.on { "on" } else { "off" };
    state
        .store
        .write(move |tx| regards_store::settings::set(tx, KEY, value))
        .await
        .map_err(crate::session::internal)?;
    if params.on {
        state.prefetch.wake.notify_one();
    }
    Ok(json!(params))
}
