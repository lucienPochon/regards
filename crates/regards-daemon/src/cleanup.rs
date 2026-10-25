//! Stored files nothing points to any more (a message deleted, a draft sent
//! or thrown away) are removed once a day, so `blobs/` does not only grow.

use crate::State;
use regards_store::blobs;
use std::sync::Arc;
use std::time::Duration;

/// The first sweep waits for starting up (and its first syncs) to be over.
const FIRST: Duration = Duration::from_secs(10 * 60);
const EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// A file stored more recently stays: it may not be in the database yet.
const GRACE: Duration = Duration::from_secs(24 * 60 * 60);

pub(crate) async fn run(state: Arc<State>) {
    tokio::time::sleep(FIRST).await;
    loop {
        sweep(&state).await;
        tokio::time::sleep(EVERY).await;
    }
}

async fn sweep(state: &State) {
    let keep = match state.store.read(blobs::referenced).await {
        Ok(keep) => keep,
        Err(error) => {
            tracing::warn!(%error, "unused stored files not looked for this time");
            return;
        }
    };
    let blobs = state.blobs.clone();
    match tokio::task::spawn_blocking(move || blobs.remove_unused(&keep, GRACE)).await {
        Ok(Ok((0, _))) => {}
        Ok(Ok((files, bytes))) => tracing::info!(files, bytes, "unused stored files removed"),
        Ok(Err(error)) => tracing::warn!(%error, "unused stored files not removed this time"),
        Err(error) => tracing::warn!(%error, "the sweep of unused stored files stopped"),
    }
}
