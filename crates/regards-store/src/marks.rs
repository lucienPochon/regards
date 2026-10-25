//! Marks the person puts on messages, kept on this device (plan §5.5–5.6):
//! pinned, set aside, snoozed, reminders. Keyed by account and Message-ID, so
//! a mark follows its message across folders, moves and resyncs (ADR-0010
//! §2). A message without a Message-ID cannot be marked.
//!
//! Two marks record what the scheduler did rather than what the person
//! asked: `surfaced_at`, when a message came back to the inbox (to list it on
//! top), and `reminded_at`, a reminder that came due with no reply.

use crate::StoreError;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    Pinned,
    Aside,
    Snoozed,
    /// "Remind me if no reply by" this time.
    Remind,
    /// The reminder came due with no reply: shown in the inbox until done.
    Reminded,
    /// Came back to the inbox at this time; listed there by it.
    Surfaced,
}

impl Mark {
    fn column(self) -> &'static str {
        match self {
            Self::Pinned => "pinned_at",
            Self::Aside => "aside_at",
            Self::Snoozed => "snoozed_until",
            Self::Remind => "remind_at",
            Self::Reminded => "reminded_at",
            Self::Surfaced => "surfaced_at",
        }
    }
}

/// A message's marks; times are Unix milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Marks {
    pub pinned_at: Option<i64>,
    pub aside_at: Option<i64>,
    pub snoozed_until: Option<i64>,
    pub remind_at: Option<i64>,
    pub reminded_at: Option<i64>,
    pub surfaced_at: Option<i64>,
}

impl Marks {
    pub fn get(&self, mark: Mark) -> Option<i64> {
        match mark {
            Mark::Pinned => self.pinned_at,
            Mark::Aside => self.aside_at,
            Mark::Snoozed => self.snoozed_until,
            Mark::Remind => self.remind_at,
            Mark::Reminded => self.reminded_at,
            Mark::Surfaced => self.surfaced_at,
        }
    }
}

const MARK_COLUMNS: &str = "pinned_at, aside_at, snoozed_until, remind_at, reminded_at, surfaced_at";

fn marks_from(row: &Row<'_>) -> rusqlite::Result<Marks> {
    Ok(Marks {
        pinned_at: row.get(0)?,
        aside_at: row.get(1)?,
        snoozed_until: row.get(2)?,
        remind_at: row.get(3)?,
        reminded_at: row.get(4)?,
        surfaced_at: row.get(5)?,
    })
}

pub fn get(conn: &Connection, account_id: &str, message_id: &str) -> Result<Marks, StoreError> {
    Ok(conn
        .query_row(
            &format!("SELECT {MARK_COLUMNS} FROM marks WHERE account_id = ?1 AND message_id = ?2"),
            params![account_id, message_id],
            marks_from,
        )
        .optional()?
        .unwrap_or_default())
}

/// Set or clear one mark; returns what it was before (for undo).
pub fn set(
    tx: &Transaction<'_>,
    account_id: &str,
    message_id: &str,
    mark: Mark,
    value: Option<i64>,
) -> Result<Option<i64>, StoreError> {
    let before = get(tx, account_id, message_id)?.get(mark);
    let column = mark.column();
    tx.execute(
        &format!(
            "INSERT INTO marks (account_id, message_id, {column}) VALUES (?1, ?2, ?3) \
             ON CONFLICT (account_id, message_id) DO UPDATE SET {column} = excluded.{column}"
        ),
        params![account_id, message_id, value],
    )?;
    tx.execute(
        "DELETE FROM marks WHERE account_id = ?1 AND message_id = ?2 \
         AND pinned_at IS NULL AND aside_at IS NULL AND snoozed_until IS NULL \
         AND remind_at IS NULL AND reminded_at IS NULL AND surfaced_at IS NULL",
        params![account_id, message_id],
    )?;
    Ok(before)
}

/// "Someday": snoozed with no time to come back.
pub const SOMEDAY: i64 = 32_503_680_000_000; // 3000-01-01

/// Snoozes whose time has come: (account, Message-ID).
pub fn due_snoozes(conn: &Connection, now: i64) -> Result<Vec<(String, String)>, StoreError> {
    due(
        conn,
        "SELECT account_id, message_id FROM marks WHERE snoozed_until IS NOT NULL AND snoozed_until <= ?1",
        now,
    )
}

/// Snoozes with a time to come back ("someday" has none) and reminders not
/// yet come: what waits for the daemon, for a word before quitting.
pub fn waiting(conn: &Connection) -> Result<(u32, u32), StoreError> {
    Ok(conn.query_row(
        "SELECT COUNT(CASE WHEN snoozed_until < ?1 THEN 1 END), COUNT(remind_at) FROM marks",
        [SOMEDAY],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?)
}

/// Reminders whose time has come: (account, Message-ID).
pub fn due_reminders(conn: &Connection, now: i64) -> Result<Vec<(String, String)>, StoreError> {
    due(
        conn,
        "SELECT account_id, message_id FROM marks WHERE remind_at IS NOT NULL AND remind_at <= ?1",
        now,
    )
}

fn due(conn: &Connection, sql: &str, now: i64) -> Result<Vec<(String, String)>, StoreError> {
    let mut statement = conn.prepare_cached(sql)?;
    let due = statement
        .query_map([now], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(due)
}

/// When the next snooze ends or reminder comes due, if any will ("someday"
/// never does).
pub fn next_wake(conn: &Connection) -> Result<Option<i64>, StoreError> {
    Ok(conn.query_row(
        "SELECT MIN(at) FROM ( \
             SELECT MIN(snoozed_until) AS at FROM marks WHERE snoozed_until IS NOT NULL AND snoozed_until < ?1 \
             UNION ALL SELECT MIN(remind_at) FROM marks WHERE remind_at IS NOT NULL)",
        [SOMEDAY],
        |row| row.get(0),
    )?)
}

/// Snoozed messages of an account in the conversation of `message_id`: a
/// new message there from someone else ends their snooze (ADR-0011 §3). The
/// caller decides what is new and from whom: comparing the server's arrival
/// times with this device's clock would miss replies whenever the two differ.
pub fn snoozed_in_thread_of(conn: &Connection, account_id: &str, message_id: &str) -> Result<Vec<String>, StoreError> {
    in_thread_of(conn, account_id, message_id, "k.snoozed_until IS NOT NULL")
}

/// Reminders, waiting or come due, in the conversation of `message_id`: a
/// new message there from someone else is the reply they waited for (ADR-0011 §4).
pub fn reminders_in_thread_of(
    conn: &Connection,
    account_id: &str,
    message_id: &str,
) -> Result<Vec<String>, StoreError> {
    in_thread_of(
        conn,
        account_id,
        message_id,
        "(k.remind_at IS NOT NULL OR k.reminded_at IS NOT NULL)",
    )
}

fn in_thread_of(
    conn: &Connection,
    account_id: &str,
    message_id: &str,
    marked: &str,
) -> Result<Vec<String>, StoreError> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT DISTINCT k.message_id FROM marks k \
         JOIN messages s ON s.message_id_header = k.message_id \
         JOIN mailboxes b ON b.id = s.mailbox_id AND b.account_id = k.account_id \
         WHERE k.account_id = ?1 AND {marked} AND k.message_id != ?2 AND s.thread_id IN ( \
             SELECT n.thread_id FROM messages n JOIN mailboxes nb ON nb.id = n.mailbox_id \
             WHERE nb.account_id = ?1 AND n.message_id_header = ?2 AND n.thread_id IS NOT NULL)"
    ))?;
    let found = statement
        .query_map(params![account_id, message_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(found)
}

/// Whether someone other than `own_address` wrote in the conversation of
/// `message_id` after it arrived. Both times are the server's, so they
/// compare. `None` when no copy of the message is stored yet.
pub fn replied_after(
    conn: &Connection,
    account_id: &str,
    message_id: &str,
    own_address: &str,
) -> Result<Option<bool>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT r.thread_id, MIN(r.received_at) FROM messages r JOIN mailboxes rb ON rb.id = r.mailbox_id \
         WHERE rb.account_id = ?1 AND r.message_id_header = ?2 AND r.deleted = 0",
    )?;
    let (thread, sent): (Option<i64>, Option<i64>) =
        statement.query_row(params![account_id, message_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let Some(sent) = sent else { return Ok(None) };
    let Some(thread) = thread else { return Ok(Some(false)) };
    let mut statement = conn.prepare_cached(
        "SELECT EXISTS (SELECT 1 FROM messages n JOIN mailboxes nb ON nb.id = n.mailbox_id \
         WHERE n.thread_id = ?1 AND nb.account_id = ?2 AND n.deleted = 0 AND n.received_at > ?3 \
         AND n.from_address IS NOT NULL AND lower(n.from_address) != lower(?4))",
    )?;
    let replied: bool = statement.query_row(params![thread, account_id, sent, own_address], |row| row.get(0))?;
    Ok(Some(replied))
}

/// The marks of some of an account's messages, by Message-ID (for a page of a list).
pub fn of(conn: &Connection, account_id: &str, message_ids: &[&str]) -> Result<HashMap<String, Marks>, StoreError> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {MARK_COLUMNS} FROM marks WHERE account_id = ?1 AND message_id = ?2"
    ))?;
    let mut found = HashMap::new();
    for id in message_ids {
        let marks = statement.query_row(params![account_id, id], marks_from).optional()?;
        if let Some(marks) = marks {
            found.insert((*id).to_owned(), marks);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use regards_core::paths::DataDirs;

    #[tokio::test]
    async fn marks_are_set_cleared_and_remembered_for_undo() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let account = accounts::new_id();
        let a = account.clone();
        store
            .write(move |tx| {
                let server = Server {
                    host: "h".into(),
                    port: 993,
                    security: Security::Tls,
                };
                accounts::insert(
                    tx,
                    NewAccount {
                        id: a,
                        display_name: "A".into(),
                        email: "a@example.test".into(),
                        auth_kind: AuthKind::Password,
                        username: "a".into(),
                        secret_key: "k".into(),
                        imap: server.clone(),
                        smtp: server,
                        imap_capabilities: vec![],
                    },
                )
            })
            .await
            .unwrap();
        let a = account.clone();
        let before = store
            .write(move |tx| {
                let first = set(tx, &a, "m@x", Mark::Pinned, Some(100))?;
                let second = set(tx, &a, "m@x", Mark::Aside, Some(200))?;
                let third = set(tx, &a, "m@x", Mark::Pinned, None)?;
                Ok((first, second, third))
            })
            .await
            .unwrap()
            .value;
        assert_eq!(before, (None, None, Some(100)));
        let a = account.clone();
        let marks = store.read(move |c| get(c, &a, "m@x")).await.unwrap();
        assert_eq!(
            marks,
            Marks {
                pinned_at: None,
                aside_at: Some(200),
                ..Marks::default()
            }
        );
        // Nothing left: the row goes.
        let a = account.clone();
        store
            .write(move |tx| set(tx, &a, "m@x", Mark::Aside, None))
            .await
            .unwrap();
        let rows: i64 = store
            .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM marks", [], |r| r.get(0))?))
            .await
            .unwrap();
        assert_eq!(rows, 0);
        let a = account.clone();
        assert!(
            store
                .read(move |c| of(c, &a, &["m@x", "other@x"]))
                .await
                .unwrap()
                .is_empty()
        );

        // What waits for the daemon: a snooze with a time, not "someday"; a reminder.
        let a = account.clone();
        store
            .write(move |tx| {
                set(tx, &a, "soon@x", Mark::Snoozed, Some(1_000))?;
                set(tx, &a, "someday@x", Mark::Snoozed, Some(SOMEDAY))?;
                set(tx, &a, "sent@x", Mark::Remind, Some(2_000))?;
                set(tx, &a, "pinned@x", Mark::Pinned, Some(3_000))
            })
            .await
            .unwrap();
        assert_eq!(store.read(waiting).await.unwrap(), (1, 1));
    }
}
