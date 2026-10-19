//! Mail sync with each account's server (ADR-0009):
//!
//! 1. Sign in with the stored password.
//! 2. `LIST`, assign roles, reconcile the mailboxes.
//! 3. For every selectable mailbox, inbox first: new messages' headers, flag
//!    changes of known messages, and deletions.
//!
//! Engine rules from spike 07 §7: deletions are inferred only from a
//! completed `UID SEARCH`; unreadable responses are skipped and counted; a
//! missing HIGHESTMODSEQ is tolerated.

use crate::State;
use crate::feed::Feed;
use regards_core::headers;
use regards_core::mailbox::{Candidate, assign_roles};
use regards_imap::{FlagUpdate, Flags, ImapError, ImapServer, MessageSet, Selected, Session, Since};
use regards_ipc::protocol::SyncState;
use regards_store::StoreError;
use regards_store::accounts;
use regards_store::mailboxes::{self, Mailbox, Reconciled, RemoteMailbox, SyncPoint};
use regards_store::messages::{self, FlagChange, MessageFlags, NewMessage};
use regards_store::operations;
use regards_vault::SecretKey;
use std::collections::{BTreeSet, HashMap};
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

/// What one sync of an account changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub mailboxes: Reconciled,
    pub messages_added: usize,
    pub flag_changes: usize,
    pub messages_removed: usize,
    /// Server responses that could not be read and were skipped.
    pub unreadable: usize,
}

/// Sync progress per account, for the UI. Every change is announced on the feed.
pub(crate) struct Tracker {
    states: Mutex<HashMap<String, SyncState>>,
    feed: Arc<Feed>,
}

impl Tracker {
    pub(crate) fn new(feed: Arc<Feed>) -> Self {
        Self {
            states: Mutex::default(),
            feed,
        }
    }

    pub(crate) fn get(&self, account_id: &str) -> SyncState {
        self.lock().get(account_id).cloned().unwrap_or_default()
    }

    pub(crate) fn forget(&self, account_id: &str) {
        self.lock().remove(account_id);
        self.feed.bump();
    }

    /// Show a problem in the account's status until the next successful sync.
    pub(crate) fn note_error(&self, account_id: &str, message: &str) {
        self.lock().entry(account_id.to_owned()).or_default().last_error = Some(message.to_owned());
        self.feed.bump();
    }

    /// Record whether a push connection is waiting for new mail.
    pub(crate) fn set_push(&self, account_id: &str, active: bool) {
        self.lock().entry(account_id.to_owned()).or_default().push_active = active;
        self.feed.bump();
    }

    /// Mark a sync as started; `false` if one is already running.
    fn start(&self, account_id: &str) -> bool {
        let started = {
            let mut states = self.lock();
            let state = states.entry(account_id.to_owned()).or_default();
            !std::mem::replace(&mut state.running, true)
        };
        self.feed.bump();
        started
    }

    fn finish(&self, account_id: &str, outcome: &Result<Summary, SyncError>) {
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
        drop(states);
        self.feed.bump();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, SyncState>> {
        self.states.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Sync one account. Returns `None` if a sync of it is already running.
pub(crate) async fn sync_account(state: &State, account_id: &str) -> Option<Result<Summary, SyncError>> {
    if !state.sync.start(account_id) {
        return None;
    }
    let outcome = run(state, account_id).await;
    match &outcome {
        Ok(summary) => tracing::info!(account = %account_id, ?summary, "account synced"),
        Err(error) => tracing::warn!(account = %account_id, %error, "account sync failed"),
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

/// Sync an account in the background; once a sync succeeds, keep a push
/// connection open for it.
pub(crate) fn spawn_one(state: Arc<State>, account_id: String) {
    tokio::spawn(async move {
        if let Some(Ok(_)) = sync_account(&state, &account_id).await {
            state.watchers.start(&state, &account_id);
            state.backfills.start(&state, &account_id);
        }
    });
}

/// Sign in to an account's IMAP server with its stored password.
pub(crate) async fn login(state: &State, account_id: &str) -> Result<Session, SyncError> {
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
    Ok(Session::login(&server, &account.username, &password, &state.tls).await?)
}

async fn run(state: &State, account_id: &str) -> Result<Summary, SyncError> {
    let mut session = login(state, account_id).await?;
    let outcome = sync_with(state, &mut session, account_id).await;
    session.logout().await;
    outcome
}

async fn sync_with(state: &State, session: &mut Session, account_id: &str) -> Result<Summary, SyncError> {
    let mut summary = Summary {
        mailboxes: sync_mailbox_list(state, session, account_id).await?,
        ..Summary::default()
    };
    let id = account_id.to_owned();
    let mailboxes = state.store.read(move |conn| mailboxes::list(conn, &id)).await?;
    for mailbox in mailboxes.into_iter().filter(|m| m.selectable) {
        let changes = sync_mailbox(state, session, &mailbox).await?;
        summary.messages_added += changes.messages_added;
        summary.flag_changes += changes.flag_changes;
        summary.messages_removed += changes.messages_removed;
        summary.unreadable += changes.unreadable;
    }
    Ok(summary)
}

async fn sync_mailbox_list(state: &State, session: &mut Session, account_id: &str) -> Result<Reconciled, SyncError> {
    let listed = session.list_mailboxes().await?;
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
    let id = account_id.to_owned();
    Ok(state
        .store
        .write(move |tx| mailboxes::reconcile(tx, &id, &remote))
        .await?
        .value)
}

/// What is stored about a mailbox before syncing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stored {
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
    /// Lowest and highest stored UID.
    pub known: Option<(u32, u32)>,
}

/// How to bring a mailbox up to date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    /// UIDVALIDITY changed: every stored UID is meaningless, start over.
    pub reset: bool,
    /// Headers to fetch.
    pub new: Option<MessageSet>,
    /// Keep only fetched messages with at least this UID (`n:*` returns the
    /// last message even when there is none newer).
    pub new_from: u32,
    pub flags: Option<FlagCheck>,
    pub check_deletions: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FlagCheck {
    /// CONDSTORE: only messages changed after this modseq.
    ChangedSince(u64),
    /// Re-read the flags of the stored UID range.
    Range(u32, u32),
}

/// `window`: how many of the newest messages a first sync fetches.
pub(crate) fn plan(stored: &Stored, selected: &Selected, condstore: bool, window: u32) -> Plan {
    let reset = stored.uid_validity.is_some_and(|v| v != selected.uid_validity);
    let first = reset || stored.uid_validity.is_none() || stored.uid_next.is_none();
    let known = if reset { None } else { stored.known };

    let (new, new_from) = if selected.exists == 0 {
        (None, 1)
    } else if first {
        let start = selected.exists.saturating_sub(window) + 1;
        (Some(MessageSet::Sequence(format!("{start}:*"))), 1)
    } else {
        let next = stored.uid_next.unwrap_or(1);
        let nothing_new = selected.uid_next.is_some_and(|server_next| server_next <= next);
        if nothing_new {
            (None, next)
        } else {
            (Some(MessageSet::Uids(format!("{next}:*"))), next)
        }
    };

    let flags = match (first, known) {
        (true, _) | (_, None) => None,
        (false, Some((low, high))) => match (condstore, stored.highest_modseq) {
            (true, Some(stored_modseq)) => {
                (selected.highest_modseq != Some(stored_modseq)).then_some(FlagCheck::ChangedSince(stored_modseq))
            }
            _ => Some(FlagCheck::Range(low, high)),
        },
    };

    Plan {
        reset,
        new,
        new_from,
        flags,
        check_deletions: !first && known.is_some(),
    }
}

/// What syncing one mailbox changed.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Changes {
    pub(crate) messages_added: usize,
    pub(crate) flag_changes: usize,
    pub(crate) messages_removed: usize,
    pub(crate) unreadable: usize,
}

pub(crate) async fn sync_mailbox(
    state: &State,
    session: &mut Session,
    mailbox: &Mailbox,
) -> Result<Changes, SyncError> {
    let mailbox_id = mailbox.id;
    let known_uids = state.store.read(move |conn| messages::uids(conn, mailbox_id)).await?;
    let stored = Stored {
        uid_validity: mailbox.uid_validity.map(|v| v as u32),
        uid_next: mailbox.uid_next.map(|v| v as u32),
        highest_modseq: mailbox.highest_modseq.map(|v| v as u64),
        known: known_uids.first().copied().zip(known_uids.last().copied()),
    };
    // With QRESYNC the SELECT itself says what changed and vanished since the
    // last sync; otherwise (or on a first sync) it is a plain SELECT.
    let since = Since {
        uid_validity: stored.uid_validity.unwrap_or(0),
        modseq: stored.highest_modseq.unwrap_or(0),
        known: stored.known,
    };
    let resynced = session.select_since(&mailbox.remote_name, &since).await?;
    let selected = resynced.selected.clone();
    let mut plan = plan(
        &stored,
        &selected,
        session.has_capability("CONDSTORE"),
        state.limits.initial_window,
    );
    let mut changes = Changes {
        unreadable: resynced.skipped,
        ..Changes::default()
    };
    let mut flag_updates: Vec<FlagUpdate> = Vec::new();
    let mut removed: Vec<u32> = Vec::new();
    if resynced.resynced && !plan.reset {
        plan.flags = None;
        plan.check_deletions = false;
        flag_updates = resynced.changed;
        removed = known_uids
            .iter()
            .copied()
            .filter(|uid| resynced.vanished.iter().any(|(low, high)| (*low..=*high).contains(uid)))
            .collect();
    }

    let mut new = Vec::new();
    if let Some(set) = plan.new.clone() {
        let fetched = session.fetch_headers(set).await?;
        changes.unreadable += fetched.skipped;
        new = fetched
            .items
            .into_iter()
            .filter(|m| m.uid >= plan.new_from)
            .collect::<Vec<_>>();
    }

    if let Some(check) = &plan.flags {
        let fetched = match check {
            FlagCheck::ChangedSince(modseq) => session.fetch_flags("1:*", Some(*modseq)).await?,
            FlagCheck::Range(low, high) => session.fetch_flags(&format!("{low}:{high}"), None).await?,
        };
        changes.unreadable += fetched.skipped;
        flag_updates = fetched.items;
    }

    if plan.check_deletions {
        let present: BTreeSet<u32> = session.uid_search_all().await?.into_iter().collect();
        let known: BTreeSet<u32> = if plan.reset {
            BTreeSet::new()
        } else {
            known_uids.iter().copied().collect()
        };
        removed = known
            .iter()
            .chain(new.iter().map(|m| &m.uid))
            .filter(|uid| !present.contains(uid))
            .copied()
            .collect();
    }

    let seen_modseq = new
        .iter()
        .filter_map(|m| m.modseq)
        .chain(flag_updates.iter().filter_map(|u| u.modseq))
        .max();
    let highest_uid = new.iter().map(|m| m.uid).chain(known_uids.last().copied()).max();
    let point = SyncPoint {
        uid_validity: selected.uid_validity,
        uid_next: selected.uid_next.or(highest_uid.map(|uid| uid + 1)),
        // Dovecot can omit HIGHESTMODSEQ on a new mailbox (spike 07 §6).
        highest_modseq: selected.highest_modseq.or(seen_modseq).or(if plan.reset {
            None
        } else {
            stored.highest_modseq
        }),
        synced_at: now_ms(),
    };

    let records: Vec<NewMessage> = new
        .into_iter()
        .map(|m| NewMessage {
            uid: m.uid,
            modseq: m.modseq,
            flags: message_flags(m.flags),
            received_at: m.internal_date,
            size: m.size,
            headers: headers::parse(&m.header),
        })
        .collect();
    let flag_changes: Vec<FlagChange> = flag_updates
        .into_iter()
        .map(|u| FlagChange {
            uid: u.uid,
            modseq: u.modseq,
            flags: message_flags(u.flags),
        })
        .collect();
    let reset = plan.reset;
    let first_look = reset || stored.uid_validity.is_none();
    let new_uids: Vec<u32> = records.iter().map(|r| r.uid).collect();
    // The newest of what came in get their previews now; the backfill does the rest.
    let mut detail_uids = new_uids.clone();
    detail_uids.sort_unstable_by(|a, b| b.cmp(a));
    detail_uids.truncate(crate::details::AT_SYNC);
    let arrived: Vec<(String, Option<String>)> = records
        .iter()
        .filter_map(|r| {
            let id = r.headers.message_id.clone()?;
            Some((id, r.headers.from.as_ref().and_then(|p| p.address.clone())))
        })
        .collect();
    let committed = state
        .store
        .write(move |tx| {
            if reset {
                messages::clear(tx, mailbox_id)?;
            }
            let added = messages::upsert(tx, mailbox_id, &records)?;
            // Mail from blocked senders goes on to Junk.
            let junked = crate::actions::junk_blocked(tx, mailbox_id, &new_uids, now_ms())?;
            // New mail (not a first look at the folder) from someone else ends
            // the snooze of its conversation.
            if added > 0 && !first_look {
                end_answered_snoozes(tx, mailbox_id, &arrived)?;
            }
            let flagged = messages::apply_flags(tx, mailbox_id, &flag_changes)?;
            let removed = messages::delete_uids(tx, mailbox_id, &removed)?;
            // Actions the server has not heard about yet win over its older flags (ADR-0010 §2).
            operations::reapply_pending(tx, mailbox_id, point.uid_validity)?;
            mailboxes::set_sync_point(tx, mailbox_id, point)?;
            Ok((added, flagged, removed, junked))
        })
        .await?;
    let (added, flagged, removed, junked) = committed.value;
    for account in junked {
        state.queue.wake_account(&account);
    }
    let _ = crate::details::fill(state, session, mailbox_id, &detail_uids).await;
    changes.messages_added = added;
    changes.flag_changes = flagged;
    changes.messages_removed = removed;
    if changes.unreadable > 0 {
        tracing::warn!(
            mailbox = mailbox_id,
            unreadable = changes.unreadable,
            "skipped unreadable server responses"
        );
    }
    Ok(changes)
}

/// New mail arrived: a message from someone else ends the snooze of its
/// conversation (ADR-0011 §3), and is the reply a reminder there waited for
/// (§4): the reminder goes, and one that came due leaves the inbox, where the
/// reply now is. The new message itself is unread, so it shows.
/// `arrived`: the new messages' Message-IDs and senders.
fn end_answered_snoozes(
    tx: &rusqlite::Transaction<'_>,
    mailbox_id: i64,
    arrived: &[(String, Option<String>)],
) -> Result<(), StoreError> {
    use regards_store::marks::{self, Mark};
    let Some(mailbox) = mailboxes::get(tx, mailbox_id)? else {
        return Ok(());
    };
    let Some(account) = accounts::get(tx, &mailbox.account_id)? else {
        return Ok(());
    };
    for (message_id, from) in arrived {
        if from
            .as_deref()
            .is_some_and(|from| from.eq_ignore_ascii_case(&account.email))
        {
            continue; // My own follow-up does not count.
        }
        for snoozed in marks::snoozed_in_thread_of(tx, &account.id, message_id)? {
            marks::set(tx, &account.id, &snoozed, Mark::Snoozed, None)?;
        }
        for waiting in marks::reminders_in_thread_of(tx, &account.id, message_id)? {
            let came_due = marks::get(tx, &account.id, &waiting)?.reminded_at.is_some();
            marks::set(tx, &account.id, &waiting, Mark::Remind, None)?;
            if came_due {
                marks::set(tx, &account.id, &waiting, Mark::Reminded, None)?;
                marks::set(tx, &account.id, &waiting, Mark::Surfaced, None)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn message_flags(flags: Flags) -> MessageFlags {
    MessageFlags {
        seen: flags.seen,
        answered: flags.answered,
        flagged: flags.flagged,
        draft: flags.draft,
        deleted: flags.deleted,
        keywords: flags.keywords,
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected(uid_validity: u32, uid_next: u32, modseq: Option<u64>, exists: u32) -> Selected {
        Selected {
            uid_validity,
            uid_next: Some(uid_next),
            highest_modseq: modseq,
            exists,
        }
    }

    const NEVER_SYNCED: Stored = Stored {
        uid_validity: None,
        uid_next: None,
        highest_modseq: None,
        known: None,
    };

    #[test]
    fn a_first_sync_fetches_the_newest_window_and_nothing_else() {
        let plan = plan(&NEVER_SYNCED, &selected(7, 5001, Some(9), 5000), true, 2000);
        assert_eq!(plan.new, Some(MessageSet::Sequence("3001:*".into())));
        assert_eq!((plan.flags, plan.check_deletions, plan.reset), (None, false, false));

        let small = super::plan(&NEVER_SYNCED, &selected(7, 4, None, 3), false, 2000);
        assert_eq!(small.new, Some(MessageSet::Sequence("1:*".into())));

        let empty = super::plan(&NEVER_SYNCED, &selected(7, 1, None, 0), false, 2000);
        assert_eq!(empty.new, None);
    }

    #[test]
    fn a_later_sync_fetches_new_uids_and_checks_flags_and_deletions() {
        let stored = Stored {
            uid_validity: Some(7),
            uid_next: Some(101),
            highest_modseq: Some(50),
            known: Some((1, 100)),
        };
        let with_condstore = plan(&stored, &selected(7, 105, Some(60), 104), true, 2000);
        assert_eq!(with_condstore.new, Some(MessageSet::Uids("101:*".into())));
        assert_eq!(with_condstore.new_from, 101);
        assert_eq!(with_condstore.flags, Some(FlagCheck::ChangedSince(50)));
        assert!(with_condstore.check_deletions);

        // Without CONDSTORE (Infomaniak before login), flags are re-read for the stored range.
        let without = plan(&stored, &selected(7, 105, None, 104), false, 2000);
        assert_eq!(without.flags, Some(FlagCheck::Range(1, 100)));
    }

    #[test]
    fn nothing_new_and_an_unchanged_modseq_skip_those_fetches() {
        let stored = Stored {
            uid_validity: Some(7),
            uid_next: Some(101),
            highest_modseq: Some(50),
            known: Some((1, 100)),
        };
        let plan = plan(&stored, &selected(7, 101, Some(50), 100), true, 2000);
        assert_eq!((plan.new, plan.flags), (None, None));
        assert!(
            plan.check_deletions,
            "expunges do not always change HIGHESTMODSEQ visibly"
        );
    }

    #[test]
    fn a_new_uidvalidity_starts_the_mailbox_over() {
        let stored = Stored {
            uid_validity: Some(7),
            uid_next: Some(101),
            highest_modseq: Some(50),
            known: Some((1, 100)),
        };
        let plan = plan(&stored, &selected(8, 4, Some(3), 3), true, 2000);
        assert!(plan.reset);
        assert_eq!(plan.new, Some(MessageSet::Sequence("1:*".into())));
        assert_eq!((plan.flags, plan.check_deletions), (None, false));
    }
}
