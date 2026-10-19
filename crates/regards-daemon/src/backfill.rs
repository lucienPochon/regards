//! Older mail (ADR-0009): the first sync of a folder fetches only its newest
//! messages ([`crate::SyncLimits::initial_window`]). Afterwards one
//! background task per account fetches the rest, newest first, a batch at a
//! time, until every folder is complete.
//!
//! Each pass asks the server which UIDs exist (`UID SEARCH ALL`) and fetches
//! the newest of those below the oldest one stored. A batch is stored only if
//! the folder's UIDVALIDITY is still the one it was fetched under, so a sync
//! that rebuilt the folder meanwhile wins.

use crate::State;
use crate::sync::{self, SyncError};
use regards_core::headers;
use regards_core::mailbox::Role;
use regards_imap::{MessageSet, Session};
use regards_store::mailboxes::{self, Mailbox};
use regards_store::messages::{self, NewMessage};
use regards_store::smart;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

/// A pause between batches, so other work on the connection's server goes first.
const PAUSE: Duration = Duration::from_millis(200);

/// The running backfills, by account.
#[derive(Default)]
pub(crate) struct Backfills {
    stops: Mutex<HashMap<String, watch::Sender<bool>>>,
}

impl Backfills {
    /// Fill in an account's older mail, unless that is under way already.
    pub(crate) fn start(&self, state: &Arc<State>, account_id: &str) {
        let mut stops = self.lock();
        if stops.contains_key(account_id) {
            return;
        }
        let (stop, stopped) = watch::channel(false);
        stops.insert(account_id.to_owned(), stop);
        let (state, account_id) = (Arc::clone(state), account_id.to_owned());
        tokio::spawn(async move {
            match run(&state, &account_id, stopped).await {
                Ok(0) => {}
                Ok(added) => tracing::info!(account = %account_id, added, "older mail fetched"),
                Err(error) => tracing::warn!(account = %account_id, %error, "fetching older mail stopped"),
            }
            state.backfills.lock().remove(&account_id);
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

/// Backfill every folder of the account, inbox first; returns how many messages were added.
async fn run(state: &State, account_id: &str, stopped: watch::Receiver<bool>) -> Result<usize, SyncError> {
    let id = account_id.to_owned();
    let folders = state.store.read(move |conn| mailboxes::list(conn, &id)).await?;
    let mut session: Option<Session> = None;
    let mut added = 0;
    for folder in folders.into_iter().filter(|m| m.selectable && m.uid_validity.is_some()) {
        if *stopped.borrow() {
            break;
        }
        if session.is_none() {
            session = Some(sync::login(state, account_id).await?);
        }
        let connection = session.as_mut().expect("signed in above");
        if folder.role == Some(Role::Inbox) {
            refresh_traits(state, connection, &folder, &stopped).await?;
        }
        refresh_details(state, connection, &folder, &stopped).await?;
        added += backfill_folder(state, connection, &folder, &stopped).await?;
    }
    if let Some(session) = session {
        session.logout().await;
    }
    Ok(added)
}

/// Inbox mail stored before the Smart Inbox read its headers (plan §5.4): its
/// headers again, newest first, a batch at a time, so it is sorted like new
/// mail. Messages the server no longer has count as having no traits.
async fn refresh_traits(
    state: &State,
    session: &mut Session,
    folder: &Mailbox,
    stopped: &watch::Receiver<bool>,
) -> Result<usize, SyncError> {
    let id = folder.id;
    let selected = session.select(&folder.remote_name).await?;
    let validity = state
        .store
        .read(move |conn| Ok(mailboxes::get(conn, id)?.and_then(|m| m.uid_validity)))
        .await?;
    if validity != Some(i64::from(selected.uid_validity)) {
        return Ok(0);
    }
    let limit = state.limits.backfill_batch;
    let mut refreshed = 0;
    while !*stopped.borrow() {
        let mut uids = state
            .store
            .read(move |conn| smart::traits_unknown(conn, id, limit))
            .await?;
        if uids.is_empty() {
            break;
        }
        uids.sort_unstable();
        let fetched = session.fetch_headers(MessageSet::Uids(uid_set(&uids))).await?;
        let found: HashMap<u32, u32> = fetched
            .items
            .iter()
            .map(|m| (m.uid, headers::parse(&m.header).traits))
            .collect();
        refreshed += uids.len();
        state
            .store
            .write(move |tx| {
                for uid in &uids {
                    smart::set_traits(tx, id, *uid, found.get(uid).copied().unwrap_or(0))?;
                }
                Ok(())
            })
            .await?;
        tokio::time::sleep(PAUSE).await;
    }
    if refreshed > 0 {
        tracing::info!(mailbox = id, refreshed, "inbox sorted for the Smart Inbox");
    }
    Ok(refreshed)
}

/// Previews and paperclips for what a sync left without ([`crate::details`]),
/// newest first, until the folder is complete or the server cannot say.
async fn refresh_details(
    state: &State,
    session: &mut Session,
    folder: &Mailbox,
    stopped: &watch::Receiver<bool>,
) -> Result<(), SyncError> {
    let id = folder.id;
    let selected = session.select(&folder.remote_name).await?;
    let validity = state
        .store
        .read(move |conn| Ok(mailboxes::get(conn, id)?.and_then(|m| m.uid_validity)))
        .await?;
    if validity != Some(i64::from(selected.uid_validity)) {
        return Ok(());
    }
    let batch = crate::details::AT_SYNC as u32;
    while !*stopped.borrow() {
        let uids = state
            .store
            .read(move |conn| messages::details_unknown(conn, id, batch))
            .await?;
        if uids.is_empty() || !crate::details::fill(state, session, id, &uids).await {
            break;
        }
        tokio::time::sleep(PAUSE).await;
    }
    Ok(())
}

/// Fetch the messages of `folder` older than the oldest stored, newest first.
async fn backfill_folder(
    state: &State,
    session: &mut Session,
    folder: &Mailbox,
    stopped: &watch::Receiver<bool>,
) -> Result<usize, SyncError> {
    let selected = session.select(&folder.remote_name).await?;
    let id = folder.id;
    let (stored_validity, lowest) = state
        .store
        .read(move |conn| {
            let validity = mailboxes::get(conn, id)?.and_then(|m| m.uid_validity);
            Ok((validity, messages::uids(conn, id)?.first().copied()))
        })
        .await?;
    // Not synced under this UIDVALIDITY, or nothing stored to go back from: the sync handles it.
    let (Some(validity), Some(lowest)) = (stored_validity, lowest) else {
        return Ok(0);
    };
    if validity != i64::from(selected.uid_validity) {
        return Ok(0);
    }
    let present = session.uid_search_all().await?;
    let fetched_under = selected.uid_validity;
    let mut below = lowest;
    let mut added = 0;
    loop {
        let batch = older(&present, below, state.limits.backfill_batch as usize);
        let Some(&oldest) = batch.first() else {
            return Ok(added);
        };
        // Move on whatever the server returns: expunged or unreadable messages are not retried here.
        below = oldest;
        let fetched = session.fetch_headers(MessageSet::Uids(uid_set(batch))).await?;
        let records: Vec<NewMessage> = fetched
            .items
            .into_iter()
            .filter(|m| m.uid < lowest)
            .map(|m| NewMessage {
                uid: m.uid,
                modseq: m.modseq,
                flags: sync::message_flags(m.flags),
                received_at: m.internal_date,
                size: m.size,
                headers: headers::parse(&m.header),
            })
            .collect();
        let stored = state
            .store
            .write(move |tx| {
                // A sync may have rebuilt the folder meanwhile: these UIDs would mean nothing.
                let current = mailboxes::get(tx, id)?.and_then(|m| m.uid_validity);
                if current != Some(i64::from(fetched_under)) {
                    return Ok(None);
                }
                Ok(Some(messages::upsert(tx, id, &records)?))
            })
            .await?
            .value;
        let Some(stored) = stored else {
            return Ok(added);
        };
        added += stored;
        if *stopped.borrow() {
            return Ok(added);
        }
        tokio::time::sleep(PAUSE).await;
    }
}

/// The newest `limit` UIDs below `lowest`, oldest first; `present` is sorted.
fn older(present: &[u32], lowest: u32, limit: usize) -> &[u32] {
    let below = present.partition_point(|uid| *uid < lowest);
    &present[below.saturating_sub(limit)..below]
}

/// A compact UID set for sorted UIDs: runs become ranges ("1:3,7,9:10").
pub(crate) fn uid_set(uids: &[u32]) -> String {
    let mut parts = Vec::new();
    let mut index = 0;
    while index < uids.len() {
        let start = uids[index];
        let mut end = start;
        while index + 1 < uids.len() && uids[index + 1] == end + 1 {
            index += 1;
            end = uids[index];
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}:{end}")
        });
        index += 1;
    }
    parts.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_older_uids_come_first() {
        let present = [1, 2, 5, 6, 7, 9, 12, 20];
        assert_eq!(older(&present, 12, 3), [6, 7, 9]);
        assert_eq!(older(&present, 12, 10), [1, 2, 5, 6, 7, 9]);
        assert_eq!(older(&present, 1, 10), [] as [u32; 0]);
        assert_eq!(older(&present, 100, 2), [12, 20]);
        assert_eq!(older(&[], 5, 2), [] as [u32; 0]);
    }

    #[test]
    fn uid_sets_are_compact() {
        assert_eq!(uid_set(&[1, 2, 3, 7, 9, 10]), "1:3,7,9:10");
        assert_eq!(uid_set(&[4]), "4");
        assert_eq!(uid_set(&[]), "");
    }
}
