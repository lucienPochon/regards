//! The search index kept current (plan §5.10). The store queues every
//! change to a message ([`regards_store::search`]); this works the queue off
//! a moment after changes happen, in batches, and at start indexes the text
//! of messages downloaded before (before there was an index, or a rebuild).
//!
//! Its writes are quiet, so they do not wake this task. Once the queue is
//! worked off, the change feed moves (no database revision): a search on
//! screen, refetched when the change came in, is refetched once more and
//! finds what was just indexed.

use crate::State;
use regards_store::{StoreError, search};
use std::sync::Arc;
use std::time::Duration;

/// Changes indexed per write: it holds the writer for at most a few hundred ms.
const BATCH: u32 = 500;
/// Downloaded messages whose text is read back per write.
const TEXTS: u32 = 50;
/// How long the full text of a message no longer stored is kept: its row in
/// the folder it was moved to comes with that folder's next sync.
const KEEP_TEXT_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// Changes gather this long, so a sync's are indexed together.
const SETTLE: Duration = Duration::from_millis(300);

pub(crate) async fn run(state: Arc<State>) {
    let mut changes = state.store.subscribe();
    let started = async {
        work_off(&state).await?;
        catch_up(&state).await
    };
    if let Err(error) = started.await {
        tracing::warn!(%error, "the search index is not up to date for now");
    }
    loop {
        if changes.changed().await.is_err() {
            return;
        }
        tokio::time::sleep(SETTLE).await;
        changes.borrow_and_update();
        if let Err(error) = work_off(&state).await {
            tracing::warn!(%error, "changes not indexed for search for now");
        }
    }
}

async fn work_off(state: &State) -> Result<(), StoreError> {
    let mut left = state.store.read(search::pending).await?;
    if left == 0 {
        return Ok(());
    }
    while left > 0 {
        left = state.store.write_quiet(|tx| search::index(tx, BATCH)).await?;
    }
    state.feed.bump();
    Ok(())
}

/// The full text of messages downloaded but not indexed, read from their
/// stored source; and texts kept long enough forgotten.
async fn catch_up(state: &State) -> Result<(), StoreError> {
    let before = crate::compose::now_ms() - KEEP_TEXT_MS;
    let forgotten = state
        .store
        .write_quiet(move |tx| search::forget_texts(tx, before))
        .await?;
    if forgotten > 0 {
        tracing::debug!(forgotten, "full texts of messages no longer stored forgotten");
    }
    loop {
        let missing = state.store.read(|conn| search::texts_missing(conn, TEXTS)).await?;
        if missing.is_empty() {
            return Ok(());
        }
        let blobs = state.blobs.clone();
        let texts = tokio::task::spawn_blocking(move || {
            missing
                .into_iter()
                .map(|message| {
                    // Unreadable: kept as no text, so it is not tried again.
                    let text = blobs
                        .get(&message.blob_hash)
                        .ok()
                        .flatten()
                        .map(|source| regards_core::body::parse(&source).text)
                        .unwrap_or_default();
                    (message, text)
                })
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|_| StoreError::Panicked)?;
        let now = crate::compose::now_ms();
        state
            .store
            .write_quiet(move |tx| {
                for (message, text) in &texts {
                    search::set_text(
                        tx,
                        &message.account_id,
                        message.id,
                        message.message_id.as_deref(),
                        text,
                        now,
                    )?;
                }
                Ok(())
            })
            .await?;
        work_off(state).await?;
    }
}
