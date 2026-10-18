//! The outbox (ADR-0010 §3–4): messages queued for sending, with their exact
//! bytes. Each state change is one guarded statement, so an undo and the
//! runner's claim can never both succeed:
//!
//! `queued` → (claim, after `undo_until`) → `sending` → `sent` | `failed` |
//! `uncertain`, and a retryable failure goes back to `queued` later.

use crate::StoreError;
use regards_core::headers::Person;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Queued,
    Sending,
    Sent,
    Failed,
    Uncertain,
}

impl State {
    fn parse(text: &str) -> Result<Self, StoreError> {
        Ok(match text {
            "queued" => Self::Queued,
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
    pub answering: Option<i64>,
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
    pub next_attempt_at: i64,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub sent_at: Option<i64>,
    pub answering: Option<i64>,
    pub created_at: i64,
}

pub fn queue(tx: &Transaction<'_>, entry: &NewEntry, now: i64) -> Result<i64, StoreError> {
    let hash: String = Sha256::digest(&entry.mime).iter().map(|b| format!("{b:02x}")).collect();
    tx.execute(
        "INSERT INTO outbox (account_id, draft_id, message_id, subject, recipients_json, envelope_from, envelope_to, \
         mime, mime_sha256, state, undo_until, next_attempt_at, answering, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'queued', ?10, ?10, ?11, ?12)",
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
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Take an entry back before it is sent (undo send, or "edit" after a
/// failure); returns its draft. `None` if it is no longer queued or failed:
/// the runner claimed it first.
pub fn take_back(tx: &Transaction<'_>, id: i64) -> Result<Option<Option<i64>>, StoreError> {
    Ok(tx
        .query_row(
            "DELETE FROM outbox WHERE id = ?1 AND state IN ('queued', 'failed') RETURNING draft_id",
            [id],
            |row| row.get(0),
        )
        .optional()?)
}

/// Claim a due entry for sending. Only one claim can succeed, and only once
/// its undo window is over.
pub fn claim(tx: &Transaction<'_>, id: i64, now: i64) -> Result<bool, StoreError> {
    Ok(tx.execute(
        "UPDATE outbox SET state = 'sending', attempts = attempts + 1 \
         WHERE id = ?1 AND state = 'queued' AND undo_until <= ?2 AND next_attempt_at <= ?2",
        params![id, now],
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

/// Send a failed or uncertain entry again, now: the user decided.
pub fn send_again(tx: &Transaction<'_>, id: i64, now: i64) -> Result<bool, StoreError> {
    Ok(tx.execute(
        "UPDATE outbox SET state = 'queued', undo_until = ?2, next_attempt_at = ?2, last_error = NULL \
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
    state, undo_until, next_attempt_at, attempts, last_error, sent_at, answering, created_at";

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

/// Entries to send now, and sent ones whose Sent copy is still to do.
pub fn due(conn: &Connection, now: i64) -> Result<Vec<Entry>, StoreError> {
    query(
        conn,
        &format!(
            "SELECT {COLUMNS} FROM outbox WHERE (state = 'queued' AND undo_until <= ?1 AND next_attempt_at <= ?1) \
             OR (state = 'sent' AND next_attempt_at <= ?1) ORDER BY id"
        ),
        [now],
    )
}

/// When something will next be due.
pub fn next_due_at(conn: &Connection) -> Result<Option<i64>, StoreError> {
    Ok(conn.query_row(
        "SELECT MIN(MAX(undo_until, next_attempt_at)) FROM outbox WHERE state IN ('queued', 'sent')",
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
            next_attempt_at: row.get(10)?,
            attempts: row.get(11)?,
            last_error: row.get(12)?,
            sent_at: row.get(13)?,
            answering: row.get(14)?,
            created_at: row.get(15)?,
        })
    };
    Ok(parse())
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
            answering: None,
        }
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
}
