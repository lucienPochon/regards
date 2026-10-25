//! The outbox (ADR-0010 §3–4): messages queued for sending, with their exact
//! bytes. Each state change is one guarded statement, so an undo and the
//! runner's claim can never both succeed:
//!
//! `queued` → (claim, after `undo_until` and `send_at`) → `sending` → `sent` |
//! `failed` | `uncertain`, and a retryable failure goes back to `queued` later.
//!
//! **Send later** (ADR-0011 §2): an entry with a `send_at` waits for it. One
//! found too late, because the computer was off or asleep then, becomes `held`
//! and waits for the person, who sends it now, picks a new time or takes it back.
//!
//! **Executors** (plan §5.7): this device sends only entries whose executor is
//! `device`, the one executor in 1.0. An entry another executor holds is
//! `handed_off` (from 1.1).

use crate::StoreError;
pub use regards_core::executor::Executor;
use regards_core::headers::Person;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Queued,
    /// Scheduled, and found too late: waits for the person.
    Held,
    /// A remote executor holds it, to send at its time (1.1).
    HandedOff,
    Sending,
    Sent,
    Failed,
    Uncertain,
}

impl State {
    fn parse(text: &str) -> Result<Self, StoreError> {
        Ok(match text {
            "queued" => Self::Queued,
            "held" => Self::Held,
            "handed_off" => Self::HandedOff,
            "sending" => Self::Sending,
            "sent" => Self::Sent,
            "failed" => Self::Failed,
            "uncertain" => Self::Uncertain,
            other => return Err(StoreError::Corrupt(format!("unknown outbox state {other:?}"))),
        })
    }
}

/// What to queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEntry {
    pub account_id: String,
    pub draft_id: Option<i64>,
    pub message_id: String,
    pub subject: String,
    pub recipients: Vec<Person>,
    pub envelope_from: String,
    pub envelope_to: Vec<String>,
    pub mime: Vec<u8>,
    pub undo_until: i64,
    /// Send later: not before this time (Unix ms).
    pub send_at: Option<i64>,
    pub answering: Option<i64>,
    pub executor: Executor,
    /// Queued at an agent's request (ADR-0012).
    pub by_agent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: i64,
    pub account_id: String,
    pub draft_id: Option<i64>,
    pub message_id: String,
    pub subject: String,
    pub recipients: Vec<Person>,
    pub envelope_from: String,
    pub envelope_to: Vec<String>,
    pub state: State,
    pub undo_until: i64,
    pub send_at: Option<i64>,
    pub next_attempt_at: i64,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub sent_at: Option<i64>,
    pub answering: Option<i64>,
    pub created_at: i64,
    pub executor: Executor,
    pub by_agent: bool,
}

pub fn queue(tx: &Transaction<'_>, entry: &NewEntry, now: i64) -> Result<i64, StoreError> {
    let hash = sha256_hex(&entry.mime);
    tx.execute(
        "INSERT INTO outbox (account_id, draft_id, message_id, subject, recipients_json, envelope_from, envelope_to, \
         mime, mime_sha256, state, undo_until, send_at, next_attempt_at, answering, created_at, executor, by_agent) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'queued', ?10, ?13, MAX(?10, COALESCE(?13, ?10)), ?11, ?12, ?14, ?15)",
        params![
            entry.account_id,
            entry.draft_id,
            entry.message_id,
            entry.subject,
            json(&entry.recipients),
            entry.envelope_from,
            json(&entry.envelope_to),
            entry.mime,
            hash,
            entry.undo_until,
            entry.answering,
            now,
            entry.send_at,
            entry.executor.as_str(),
            entry.by_agent,
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Take an entry back before it is sent (undo send, a scheduled message
/// called back, or "edit" after a failure); returns its draft. `None` if it is
/// no longer queued, held or failed: the runner claimed it first.
pub fn take_back(tx: &Transaction<'_>, id: i64) -> Result<Option<Option<i64>>, StoreError> {
    Ok(tx
        .query_row(
            "DELETE FROM outbox WHERE id = ?1 AND state IN ('queued', 'held', 'failed') RETURNING draft_id",
            [id],
            |row| row.get(0),
        )
        .optional()?)
}

/// Claim a due entry for sending from this device. Only one claim can
/// succeed, and only once its undo window is over and its time has come.
pub fn claim(tx: &Transaction<'_>, id: i64, now: i64) -> Result<bool, StoreError> {
    Ok(tx.execute(
        "UPDATE outbox SET state = 'sending', attempts = attempts + 1 \
         WHERE id = ?1 AND state = 'queued' AND executor = 'device' AND undo_until <= ?2 AND next_attempt_at <= ?2",
        params![id, now],
    )? == 1)
}

/// A scheduled entry found too late: hold it for the person. `false` if it
/// was taken back or claimed meanwhile.
pub fn hold(tx: &Transaction<'_>, id: i64) -> Result<bool, StoreError> {
    Ok(tx.execute(
        "UPDATE outbox SET state = 'held' WHERE id = ?1 AND state = 'queued' AND send_at IS NOT NULL",
        [id],
    )? == 1)
}

/// Send a queued or held entry at another time, or now (`send_at: None`),
/// with its bytes dated anew (same message, same Message-ID). `false` if it
/// is no longer waiting.
pub fn reschedule(
    tx: &Transaction<'_>,
    id: i64,
    send_at: Option<i64>,
    mime: &[u8],
    now: i64,
) -> Result<bool, StoreError> {
    Ok(tx.execute(
        "UPDATE outbox SET state = 'queued', send_at = ?2, next_attempt_at = COALESCE(?2, ?3), mime = ?4, \
         mime_sha256 = ?5 WHERE id = ?1 AND state IN ('queued', 'held') AND executor = 'device'",
        params![id, send_at, now, mime, sha256_hex(mime)],
    )? == 1)
}

/// It went out: the draft is done (the entry keeps the bytes for the Sent copy).
pub fn sent(tx: &Transaction<'_>, id: i64, reply: &str, now: i64) -> Result<(), StoreError> {
    let draft: Option<i64> = tx
        .query_row("SELECT draft_id FROM outbox WHERE id = ?1", [id], |row| row.get(0))
        .optional()?
        .flatten();
    tx.execute(
        "UPDATE outbox SET state = 'sent', server_reply = ?2, sent_at = ?3, last_error = NULL WHERE id = ?1",
        params![id, reply, now],
    )?;
    // The entry's draft_id goes to NULL with it (ON DELETE SET NULL).
    if let Some(draft) = draft {
        tx.execute("DELETE FROM drafts WHERE id = ?1", [draft])?;
    }
    Ok(())
}

/// Not sent, worth trying again at `next`.
pub fn retry_later(tx: &Transaction<'_>, id: i64, error: &str, next: i64) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE outbox SET state = 'queued', last_error = ?2, next_attempt_at = ?3 WHERE id = ?1",
        params![id, error, next],
    )?;
    Ok(())
}

/// Whether an account has entries waiting out a retry delay (what
/// [`retry_now`] would move), read before writing: most reconnects find none.
pub fn waiting_retry(conn: &Connection, account_id: &str, now: i64) -> Result<bool, StoreError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM outbox \
         WHERE account_id = ?1 AND state = 'queued' AND executor = 'device' AND attempts > 0 AND next_attempt_at > ?2)",
        params![account_id, now],
        |row| row.get(0),
    )?)
}

/// An account's entries waiting to try again go at once, not before their
/// undo window or time (its sign-in was renewed, or its server answers
/// again). Returns how many.
pub fn retry_now(tx: &Transaction<'_>, account_id: &str, now: i64) -> Result<usize, StoreError> {
    Ok(tx.execute(
        "UPDATE outbox SET next_attempt_at = MAX(undo_until, COALESCE(send_at, 0), ?2) \
         WHERE account_id = ?1 AND state = 'queued' AND executor = 'device' AND attempts > 0 AND next_attempt_at > ?2",
        params![account_id, now],
    )?)
}

/// Not sent, and something must change first.
pub fn failed(tx: &Transaction<'_>, id: i64, error: &str) -> Result<(), StoreError> {
    set_state(tx, id, "failed", error)
}

/// Perhaps sent: never retried without the user.
pub fn uncertain(tx: &Transaction<'_>, id: i64, error: &str) -> Result<(), StoreError> {
    set_state(tx, id, "uncertain", error)
}

fn set_state(tx: &Transaction<'_>, id: i64, state: &str, error: &str) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE outbox SET state = ?2, last_error = ?3 WHERE id = ?1",
        params![id, state, error],
    )?;
    Ok(())
}

/// After a crash: an entry left `sending` may have gone out.
pub fn recover(tx: &Transaction<'_>) -> Result<usize, StoreError> {
    Ok(tx.execute(
        "UPDATE outbox SET state = 'uncertain', last_error = 'Regards stopped while sending this message; it may or may not have been sent' \
         WHERE state = 'sending'",
        [],
    )?)
}

/// Send a failed or uncertain entry again, now: the user decided (so a
/// scheduled one is no longer "late").
pub fn send_again(tx: &Transaction<'_>, id: i64, now: i64) -> Result<bool, StoreError> {
    Ok(tx.execute(
        "UPDATE outbox SET state = 'queued', undo_until = ?2, send_at = NULL, next_attempt_at = ?2, last_error = NULL \
         WHERE id = ?1 AND state IN ('failed', 'uncertain')",
        params![id, now],
    )? == 1)
}

/// The user says an uncertain entry did go out (or gives up on a failed
/// one): forget it and its draft.
pub fn dismiss(tx: &Transaction<'_>, id: i64) -> Result<bool, StoreError> {
    let draft: Option<Option<i64>> = tx
        .query_row(
            "DELETE FROM outbox WHERE id = ?1 AND state IN ('failed', 'uncertain') RETURNING draft_id",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(draft) = draft else { return Ok(false) };
    if let Some(draft) = draft {
        tx.execute("DELETE FROM drafts WHERE id = ?1", [draft])?;
    }
    Ok(true)
}

/// Its copy is in the Sent folder, or needs none: the entry and its draft are done.
pub fn finish(tx: &Transaction<'_>, id: i64) -> Result<(), StoreError> {
    let draft: Option<Option<i64>> = tx
        .query_row(
            "DELETE FROM outbox WHERE id = ?1 AND state = 'sent' RETURNING draft_id",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(Some(draft)) = draft {
        tx.execute("DELETE FROM drafts WHERE id = ?1", [draft])?;
    }
    Ok(())
}

const COLUMNS: &str = "id, account_id, draft_id, message_id, subject, recipients_json, envelope_from, envelope_to, \
    state, undo_until, next_attempt_at, attempts, last_error, sent_at, answering, created_at, send_at, executor, by_agent";

pub fn get(conn: &Connection, id: i64) -> Result<Option<Entry>, StoreError> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM outbox WHERE id = ?1"), [id], from_row)
        .optional()?
        .transpose()
}

/// The exact bytes to send.
pub fn mime(conn: &Connection, id: i64) -> Result<Option<Vec<u8>>, StoreError> {
    Ok(conn
        .query_row("SELECT mime FROM outbox WHERE id = ?1", [id], |row| row.get(0))
        .optional()?)
}

/// What the user sees in the Outbox: everything not yet done, oldest first.
pub fn list(conn: &Connection) -> Result<Vec<Entry>, StoreError> {
    query(
        conn,
        &format!("SELECT {COLUMNS} FROM outbox WHERE state != 'sent' ORDER BY created_at, id"),
        [],
    )
}

/// Entries for this device to send now, and sent ones whose Sent copy is still to do.
pub fn due(conn: &Connection, now: i64) -> Result<Vec<Entry>, StoreError> {
    query(
        conn,
        &format!(
            "SELECT {COLUMNS} FROM outbox WHERE (state = 'queued' AND executor = 'device' AND undo_until <= ?1 \
             AND next_attempt_at <= ?1) OR (state = 'sent' AND next_attempt_at <= ?1) ORDER BY id"
        ),
        [now],
    )
}

/// When something will next be due on this device.
pub fn next_due_at(conn: &Connection) -> Result<Option<i64>, StoreError> {
    Ok(conn.query_row(
        "SELECT MIN(MAX(undo_until, next_attempt_at)) FROM outbox \
         WHERE (state = 'queued' AND executor = 'device') OR state = 'sent'",
        [],
        |row| row.get(0),
    )?)
}

/// The Sent copy failed: try it again at `next`.
pub fn copy_later(tx: &Transaction<'_>, id: i64, next: i64) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE outbox SET next_attempt_at = ?2, attempts = attempts + 1 WHERE id = ?1",
        params![id, next],
    )?;
    Ok(())
}

fn query(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<Vec<Entry>, StoreError> {
    let mut statement = conn.prepare_cached(sql)?;
    let rows = statement.query_map(params, from_row)?;
    rows.map(|row| row?).collect()
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Result<Entry, StoreError>> {
    let (recipients, envelope_to, state): (String, String, String) = (row.get(5)?, row.get(7)?, row.get(8)?);
    let executor: String = row.get(17)?;
    let parse = || -> Result<Entry, StoreError> {
        Ok(Entry {
            id: row.get(0)?,
            account_id: row.get(1)?,
            draft_id: row.get(2)?,
            message_id: row.get(3)?,
            subject: row.get(4)?,
            recipients: from_json(&recipients)?,
            envelope_from: row.get(6)?,
            envelope_to: from_json(&envelope_to)?,
            state: State::parse(&state)?,
            undo_until: row.get(9)?,
            send_at: row.get(16)?,
            next_attempt_at: row.get(10)?,
            attempts: row.get(11)?,
            last_error: row.get(12)?,
            sent_at: row.get(13)?,
            answering: row.get(14)?,
            created_at: row.get(15)?,
            executor: Executor::parse(&executor)
                .ok_or_else(|| StoreError::Corrupt(format!("unknown outbox executor {executor:?}")))?,
            by_agent: row.get(18)?,
        })
    };
    Ok(parse())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn json<T: serde::Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).expect("plain data always serializes")
}

fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, StoreError> {
    serde_json::from_str(text).map_err(|error| StoreError::Corrupt(format!("outbox entry: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use crate::drafts::{self, Content};
    use regards_core::paths::DataDirs;

    async fn store_with_draft() -> (tempfile::TempDir, Store, String, i64) {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let account = accounts::new_id();
        let id = account.clone();
        let draft = store
            .write(move |tx| {
                let server = Server {
                    host: "h".into(),
                    port: 993,
                    security: Security::Tls,
                };
                accounts::insert(
                    tx,
                    NewAccount {
                        id: id.clone(),
                        display_name: "A".into(),
                        email: "a@example.test".into(),
                        auth_kind: AuthKind::Password,
                        username: "a".into(),
                        secret_key: "k".into(),
                        imap: server.clone(),
                        smtp: server,
                        imap_capabilities: vec![],
                    },
                )?;
                drafts::create(
                    tx,
                    &id,
                    &Content {
                        subject: "Hi".into(),
                        body: "Hello".into(),
                        ..Content::default()
                    },
                    0,
                )
            })
            .await
            .unwrap()
            .value;
        (temp, store, account, draft)
    }

    fn entry(account: &str, draft: i64, undo_until: i64) -> NewEntry {
        NewEntry {
            account_id: account.to_owned(),
            draft_id: Some(draft),
            message_id: "m@example.test".into(),
            subject: "Hi".into(),
            recipients: vec![],
            envelope_from: "a@example.test".into(),
            envelope_to: vec!["b@example.test".into()],
            mime: b"Subject: Hi\r\n\r\nHello\r\n".to_vec(),
            undo_until,
            send_at: None,
            answering: None,
            executor: Executor::Device,
            by_agent: false,
        }
    }

    #[tokio::test]
    async fn only_entries_for_this_device_are_its_to_send() {
        let (_temp, store, account, draft) = store_with_draft().await;
        let a = account.clone();
        let id = store
            .write(move |tx| {
                let remote = NewEntry {
                    executor: Executor::SendService,
                    ..entry(&a, draft, 100)
                };
                queue(tx, &remote, 0)
            })
            .await
            .unwrap()
            .value;
        let kept = store.read(move |conn| get(conn, id)).await.unwrap().unwrap();
        assert_eq!((kept.executor, kept.state), (Executor::SendService, State::Queued));
        assert!(store.read(|conn| due(conn, 1_000)).await.unwrap().is_empty());
        assert_eq!(store.read(next_due_at).await.unwrap(), None);
        assert!(!store.write(move |tx| claim(tx, id, 1_000)).await.unwrap().value);
    }

    #[tokio::test]
    async fn undo_and_claim_exclude_each_other() {
        let (_temp, store, account, draft) = store_with_draft().await;
        let a = account.clone();
        let id = store
            .write(move |tx| queue(tx, &entry(&a, draft, 100), 0))
            .await
            .unwrap()
            .value;
        // Queued: the draft is frozen.
        let frozen = store
            .write(move |tx| {
                let edit = drafts::Edit {
                    to: &[],
                    cc: &[],
                    bcc: &[],
                    subject: "x",
                    body: "y",
                    html: None,
                };
                drafts::update(tx, draft, &edit, 1)
            })
            .await;
        assert!(matches!(frozen, Err(StoreError::Conflict(_))));
        // Not before the undo window is over.
        assert!(!store.write(move |tx| claim(tx, id, 99)).await.unwrap().value);
        // Undo wins before the claim: back to the draft.
        assert_eq!(
            store.write(move |tx| take_back(tx, id)).await.unwrap().value,
            Some(Some(draft))
        );
        assert!(!store.write(move |tx| claim(tx, id, 200)).await.unwrap().value);

        // Queue again: once claimed, undo no longer can.
        let a = account.clone();
        let id = store
            .write(move |tx| queue(tx, &entry(&a, draft, 100), 0))
            .await
            .unwrap()
            .value;
        assert!(store.write(move |tx| claim(tx, id, 100)).await.unwrap().value);
        assert!(
            !store.write(move |tx| claim(tx, id, 100)).await.unwrap().value,
            "claimed once"
        );
        assert_eq!(store.write(move |tx| take_back(tx, id)).await.unwrap().value, None);

        // A crash while sending leaves it uncertain, never queued again by itself.
        assert_eq!(store.write(recover).await.unwrap().value, 1);
        let got = store.read(move |c| get(c, id)).await.unwrap().unwrap();
        assert_eq!(got.state, State::Uncertain);
        assert!(store.read(move |c| due(c, 10_000)).await.unwrap().is_empty());

        // The user sends it again; then it is sent and finished with its draft.
        assert!(store.write(move |tx| send_again(tx, id, 300)).await.unwrap().value);
        assert!(store.write(move |tx| claim(tx, id, 300)).await.unwrap().value);
        store.write(move |tx| sent(tx, id, "250 ok", 301)).await.unwrap();
        assert!(
            store.read(list).await.unwrap().is_empty(),
            "sent is not in the outbox view"
        );
        assert!(
            store.read(move |c| drafts::get(c, draft)).await.unwrap().is_none(),
            "the draft is done once sent"
        );
        store.write(move |tx| finish(tx, id)).await.unwrap();
        assert!(store.read(move |c| drafts::get(c, draft)).await.unwrap().is_none());
        assert_eq!(store.read(move |c| mime(c, id)).await.unwrap(), None);
    }

    #[tokio::test]
    async fn scheduled_entries_wait_for_their_time_and_late_ones_wait_for_the_person() {
        let (_temp, store, account, draft) = store_with_draft().await;
        let a = account.clone();
        let id = store
            .write(move |tx| {
                let mut scheduled = entry(&a, draft, 10);
                scheduled.send_at = Some(1_000);
                queue(tx, &scheduled, 0)
            })
            .await
            .unwrap()
            .value;
        // Past its undo window, but not its time.
        assert!(store.read(move |c| due(c, 999)).await.unwrap().is_empty());
        assert_eq!(store.read(next_due_at).await.unwrap(), Some(1_000));
        assert!(!store.write(move |tx| claim(tx, id, 999)).await.unwrap().value);
        // A new time: the bytes stay, the time moves.
        assert!(
            store
                .write(move |tx| reschedule(tx, id, Some(2_000), b"later", 500))
                .await
                .unwrap()
                .value
        );
        assert_eq!(store.read(move |c| mime(c, id)).await.unwrap().unwrap(), b"later");
        assert!(store.read(move |c| due(c, 1_999)).await.unwrap().is_empty());
        let got = store.read(move |c| get(c, id)).await.unwrap().unwrap();
        assert_eq!((got.send_at, got.next_attempt_at), (Some(2_000), 2_000));

        // Found too late: held, never claimed, not due, but still listed.
        assert!(store.write(move |tx| hold(tx, id)).await.unwrap().value);
        assert!(!store.write(move |tx| claim(tx, id, 10_000)).await.unwrap().value);
        assert!(store.read(move |c| due(c, 10_000)).await.unwrap().is_empty());
        assert_eq!(store.read(next_due_at).await.unwrap(), None);
        assert_eq!(store.read(list).await.unwrap()[0].state, State::Held);

        // The person sends it now.
        assert!(
            store
                .write(move |tx| reschedule(tx, id, None, b"now", 10_000))
                .await
                .unwrap()
                .value
        );
        assert!(store.write(move |tx| claim(tx, id, 10_000)).await.unwrap().value);
        assert!(
            !store.write(move |tx| hold(tx, id)).await.unwrap().value,
            "claimed first"
        );
        assert!(
            !store
                .write(move |tx| reschedule(tx, id, Some(20_000), b"x", 10_000))
                .await
                .unwrap()
                .value,
            "too late to move"
        );

        // Or calls a held one back to its draft.
        store.write(move |tx| failed(tx, id, "x")).await.unwrap();
        assert_eq!(
            store.write(move |tx| take_back(tx, id)).await.unwrap().value,
            Some(Some(draft))
        );
        let a = account.clone();
        let id = store
            .write(move |tx| {
                let mut scheduled = entry(&a, draft, 0);
                scheduled.send_at = Some(1_000);
                queue(tx, &scheduled, 0)
            })
            .await
            .unwrap()
            .value;
        assert!(store.write(move |tx| hold(tx, id)).await.unwrap().value);
        assert_eq!(
            store.write(move |tx| take_back(tx, id)).await.unwrap().value,
            Some(Some(draft))
        );
    }

    #[tokio::test]
    async fn sending_a_late_failure_again_is_no_longer_late() {
        let (_temp, store, account, draft) = store_with_draft().await;
        let id = store
            .write(move |tx| {
                let mut scheduled = entry(&account, draft, 0);
                scheduled.send_at = Some(1_000);
                queue(tx, &scheduled, 0)
            })
            .await
            .unwrap()
            .value;
        assert!(store.write(move |tx| claim(tx, id, 1_000)).await.unwrap().value);
        store.write(move |tx| failed(tx, id, "mailbox full")).await.unwrap();
        assert!(store.write(move |tx| send_again(tx, id, 99_000)).await.unwrap().value);
        let got = store.read(move |c| get(c, id)).await.unwrap().unwrap();
        assert_eq!((got.state, got.send_at), (State::Queued, None));
        assert!(
            !store.write(move |tx| hold(tx, id)).await.unwrap().value,
            "sent now, not held"
        );
    }

    #[tokio::test]
    async fn retryable_failures_wait_and_permanent_ones_stop() {
        let (_temp, store, account, draft) = store_with_draft().await;
        let a = account.clone();
        let id = store
            .write(move |tx| queue(tx, &entry(&a, draft, 0), 0))
            .await
            .unwrap()
            .value;
        assert!(store.write(move |tx| claim(tx, id, 0)).await.unwrap().value);
        store
            .write(move |tx| retry_later(tx, id, "offline", 5_000))
            .await
            .unwrap();
        assert!(store.read(move |c| due(c, 4_999)).await.unwrap().is_empty());
        assert_eq!(store.read(next_due_at).await.unwrap(), Some(5_000));
        assert!(store.write(move |tx| claim(tx, id, 5_000)).await.unwrap().value);
        store.write(move |tx| failed(tx, id, "bad address")).await.unwrap();
        let shown = store.read(list).await.unwrap();
        assert_eq!((shown[0].state, shown[0].attempts), (State::Failed, 2));
        assert_eq!(shown[0].last_error.as_deref(), Some("bad address"));
        // "Edit": back to the draft.
        assert_eq!(
            store.write(move |tx| take_back(tx, id)).await.unwrap().value,
            Some(Some(draft))
        );
        assert!(
            !store
                .read(move |c| drafts::get(c, draft))
                .await
                .unwrap()
                .unwrap()
                .sending
        );
    }

    #[tokio::test]
    async fn a_send_waiting_out_a_retry_goes_once_the_server_answers_again() {
        let (_temp, store, account, draft) = store_with_draft().await;
        let a = account.clone();
        let id = store
            .write(move |tx| queue(tx, &entry(&a, draft, 0), 0))
            .await
            .unwrap()
            .value;
        let a = account.clone();
        assert!(
            !store.read(move |c| waiting_retry(c, &a, 1_000)).await.unwrap(),
            "never tried yet"
        );
        assert!(store.write(move |tx| claim(tx, id, 0)).await.unwrap().value);
        store
            .write(move |tx| retry_later(tx, id, "offline", 600_000))
            .await
            .unwrap();
        let a = account.clone();
        assert!(store.read(move |c| waiting_retry(c, &a, 1_000)).await.unwrap());
        assert!(store.read(move |c| due(c, 1_000)).await.unwrap().is_empty());
        let a = account.clone();
        assert_eq!(store.write(move |tx| retry_now(tx, &a, 1_000)).await.unwrap().value, 1);
        assert_eq!(
            store.read(move |c| due(c, 1_000)).await.unwrap().len(),
            1,
            "due at once"
        );
        let a = account.clone();
        assert!(!store.read(move |c| waiting_retry(c, &a, 1_000)).await.unwrap());
    }
}
