//! Marks the person puts on messages, kept on this device (plan §5.5–5.6):
//! pinned, set aside, snoozed. Keyed by account and Message-ID, so a mark
//! follows its message across folders, moves and resyncs (ADR-0010 §2). A
//! message without a Message-ID cannot be marked.

use crate::StoreError;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    Pinned,
    Aside,
    Snoozed,
}

impl Mark {
    fn column(self) -> &'static str {
        match self {
            Self::Pinned => "pinned_at",
            Self::Aside => "aside_at",
            Self::Snoozed => "snoozed_until",
        }
    }
}

/// A message's marks; times are Unix milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Marks {
    pub pinned_at: Option<i64>,
    pub aside_at: Option<i64>,
    pub snoozed_until: Option<i64>,
}

impl Marks {
    pub fn get(&self, mark: Mark) -> Option<i64> {
        match mark {
            Mark::Pinned => self.pinned_at,
            Mark::Aside => self.aside_at,
            Mark::Snoozed => self.snoozed_until,
        }
    }
}

pub fn get(conn: &Connection, account_id: &str, message_id: &str) -> Result<Marks, StoreError> {
    Ok(conn
        .query_row(
            "SELECT pinned_at, aside_at, snoozed_until FROM marks WHERE account_id = ?1 AND message_id = ?2",
            params![account_id, message_id],
            |row| {
                Ok(Marks {
                    pinned_at: row.get(0)?,
                    aside_at: row.get(1)?,
                    snoozed_until: row.get(2)?,
                })
            },
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
         AND pinned_at IS NULL AND aside_at IS NULL AND snoozed_until IS NULL",
        params![account_id, message_id],
    )?;
    Ok(before)
}

/// "Someday": snoozed with no time to come back.
pub const SOMEDAY: i64 = 32_503_680_000_000; // 3000-01-01

/// Snoozes whose time has come: (account, Message-ID).
pub fn due_snoozes(conn: &Connection, now: i64) -> Result<Vec<(String, String)>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT account_id, message_id FROM marks WHERE snoozed_until IS NOT NULL AND snoozed_until <= ?1",
    )?;
    let due = statement
        .query_map([now], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(due)
}

/// When the next snooze ends, if one will ("someday" never does).
pub fn next_wake(conn: &Connection) -> Result<Option<i64>, StoreError> {
    Ok(conn.query_row(
        "SELECT MIN(snoozed_until) FROM marks WHERE snoozed_until IS NOT NULL AND snoozed_until < ?1",
        [SOMEDAY],
        |row| row.get(0),
    )?)
}

/// Snoozed messages of an account in the conversation of `message_id`: a
/// new message there from someone else ends their snooze (ADR-0011 §3). The
/// caller decides what is new and from whom: comparing the server's arrival
/// times with this device's clock would miss replies whenever the two differ.
pub fn snoozed_in_thread_of(conn: &Connection, account_id: &str, message_id: &str) -> Result<Vec<String>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT DISTINCT k.message_id FROM marks k          JOIN messages s ON s.message_id_header = k.message_id          JOIN mailboxes b ON b.id = s.mailbox_id AND b.account_id = k.account_id          WHERE k.account_id = ?1 AND k.snoozed_until IS NOT NULL AND k.message_id != ?2 AND s.thread_id IN (              SELECT n.thread_id FROM messages n JOIN mailboxes nb ON nb.id = n.mailbox_id              WHERE nb.account_id = ?1 AND n.message_id_header = ?2 AND n.thread_id IS NOT NULL)",
    )?;
    let found = statement
        .query_map(params![account_id, message_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(found)
}

/// The marks of some of an account's messages, by Message-ID (for a page of a list).
pub fn of(conn: &Connection, account_id: &str, message_ids: &[&str]) -> Result<HashMap<String, Marks>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT pinned_at, aside_at, snoozed_until FROM marks WHERE account_id = ?1 AND message_id = ?2",
    )?;
    let mut found = HashMap::new();
    for id in message_ids {
        let marks = statement
            .query_row(params![account_id, id], |row| {
                Ok(Marks {
                    pinned_at: row.get(0)?,
                    aside_at: row.get(1)?,
                    snoozed_until: row.get(2)?,
                })
            })
            .optional()?;
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
                snoozed_until: None
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
    }
}
