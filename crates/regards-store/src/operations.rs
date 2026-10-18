//! The operation queue (ADR-0010): what the server still has to be told
//! about actions already applied to the local copy.

use crate::StoreError;
use rusqlite::{Connection, Row, Transaction, params};
use serde::{Deserialize, Serialize};

/// A flag that actions can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagName {
    Seen,
    Flagged,
    Answered,
    Draft,
    Deleted,
}

impl FlagName {
    /// The `messages` column holding this flag.
    fn column(self) -> &'static str {
        match self {
            Self::Seen => "seen",
            Self::Flagged => "flagged",
            Self::Answered => "answered",
            Self::Draft => "draft",
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    AddFlags,
    RemoveFlags,
}

impl OperationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::AddFlags => "add_flags",
            Self::RemoveFlags => "remove_flags",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOperation {
    pub mailbox_id: i64,
    pub uid_validity: u32,
    pub kind: OperationKind,
    pub uids: Vec<u32>,
    pub flags: Vec<FlagName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    pub id: i64,
    pub mailbox_id: i64,
    pub uid_validity: u32,
    pub kind: OperationKind,
    pub uids: Vec<u32>,
    pub flags: Vec<FlagName>,
    pub attempts: u32,
    pub next_attempt_at: i64,
    pub last_error: Option<String>,
}

/// Set or clear `flags` on the local copy of `uids`; returns the rows changed.
pub fn apply_locally(
    tx: &Transaction<'_>,
    mailbox_id: i64,
    uids: &[u32],
    flags: &[FlagName],
    value: bool,
) -> Result<usize, StoreError> {
    let mut changed = 0;
    for flag in flags {
        let mut statement = tx.prepare_cached(&format!(
            "UPDATE messages SET {column} = ?1 WHERE mailbox_id = ?2 AND uid = ?3 AND {column} != ?1",
            column = flag.column()
        ))?;
        for uid in uids {
            changed += statement.execute(params![value, mailbox_id, uid])?;
        }
    }
    Ok(changed)
}

pub fn enqueue(tx: &Transaction<'_>, operation: &NewOperation, now: i64) -> Result<i64, StoreError> {
    tx.execute(
        "INSERT INTO operations (mailbox_id, uid_validity, kind, uids, flags, created_at, next_attempt_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            operation.mailbox_id,
            operation.uid_validity,
            operation.kind.as_str(),
            json(&operation.uids),
            json(&operation.flags),
            now,
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

const COLUMNS: &str =
    "o.id, o.mailbox_id, o.uid_validity, o.kind, o.uids, o.flags, o.attempts, o.next_attempt_at, o.last_error";

/// Operations of an account that are due at `now`, oldest first.
pub fn due(conn: &Connection, account_id: &str, now: i64) -> Result<Vec<Operation>, StoreError> {
    query(
        conn,
        &format!(
            "SELECT {COLUMNS} FROM operations o JOIN mailboxes m ON m.id = o.mailbox_id \
             WHERE m.account_id = ?1 AND o.next_attempt_at <= ?2 ORDER BY o.id"
        ),
        params![account_id, now],
    )
}

/// Every pending operation of a mailbox, oldest first.
pub fn pending(conn: &Connection, mailbox_id: i64) -> Result<Vec<Operation>, StoreError> {
    query(
        conn,
        &format!("SELECT {COLUMNS} FROM operations o WHERE o.mailbox_id = ?1 ORDER BY o.id"),
        params![mailbox_id],
    )
}

/// When the next operation of an account is due, if any is pending.
pub fn next_due_at(conn: &Connection, account_id: &str) -> Result<Option<i64>, StoreError> {
    Ok(conn.query_row(
        "SELECT MIN(o.next_attempt_at) FROM operations o JOIN mailboxes m ON m.id = o.mailbox_id \
         WHERE m.account_id = ?1",
        [account_id],
        |row| row.get(0),
    )?)
}

/// Accounts with at least one pending operation.
pub fn accounts_with_pending(conn: &Connection) -> Result<Vec<String>, StoreError> {
    let mut statement =
        conn.prepare("SELECT DISTINCT m.account_id FROM operations o JOIN mailboxes m ON m.id = o.mailbox_id")?;
    let accounts = statement.query_map([], |row| row.get(0))?.collect::<Result<_, _>>()?;
    Ok(accounts)
}

/// The server has applied it, or it can never be applied: forget it.
pub fn remove(tx: &Transaction<'_>, id: i64) -> Result<(), StoreError> {
    tx.execute("DELETE FROM operations WHERE id = ?1", [id])?;
    Ok(())
}

/// Try again at `next_attempt_at`.
pub fn retry_later(tx: &Transaction<'_>, id: i64, error: &str, next_attempt_at: i64) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE operations SET attempts = attempts + 1, last_error = ?2, next_attempt_at = ?3 WHERE id = ?1",
        params![id, error, next_attempt_at],
    )?;
    Ok(())
}

/// After a sync wrote the server's flags: re-apply what the server has not
/// been told yet, so pending actions do not appear to undo themselves
/// (ADR-0010 §2). Operations from another UIDVALIDITY are void and dropped.
pub fn reapply_pending(tx: &Transaction<'_>, mailbox_id: i64, uid_validity: u32) -> Result<(), StoreError> {
    for operation in pending(tx, mailbox_id)? {
        if operation.uid_validity != uid_validity {
            remove(tx, operation.id)?;
            continue;
        }
        let value = operation.kind == OperationKind::AddFlags;
        apply_locally(tx, mailbox_id, &operation.uids, &operation.flags, value)?;
    }
    Ok(())
}

fn query(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<Vec<Operation>, StoreError> {
    let mut statement = conn.prepare_cached(sql)?;
    let rows = statement.query_map(params, from_row)?;
    rows.map(|row| row?).collect()
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Result<Operation, StoreError>> {
    let kind: String = row.get(3)?;
    let uids: String = row.get(4)?;
    let flags: String = row.get(5)?;
    let operation = (|| {
        Ok(Operation {
            id: row.get(0)?,
            mailbox_id: row.get(1)?,
            uid_validity: row.get(2)?,
            kind: match kind.as_str() {
                "add_flags" => OperationKind::AddFlags,
                "remove_flags" => OperationKind::RemoveFlags,
                other => return Err(StoreError::Corrupt(format!("unknown operation {other:?}"))),
            },
            uids: serde_json::from_str(&uids).map_err(|e| StoreError::Corrupt(format!("operation uids: {e}")))?,
            flags: serde_json::from_str(&flags).map_err(|e| StoreError::Corrupt(format!("operation flags: {e}")))?,
            attempts: row.get(6)?,
            next_attempt_at: row.get(7)?,
            last_error: row.get(8)?,
        })
    })();
    Ok(operation)
}

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("plain data always serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use crate::mailboxes::{self, RemoteMailbox};
    use crate::messages::{self, FlagChange, MessageFlags, NewMessage};
    use regards_core::headers::Headers;
    use regards_core::mailbox::Role;
    use regards_core::paths::DataDirs;

    async fn store_with_messages() -> (tempfile::TempDir, Store, String, i64) {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let account = accounts::new_id();
        let id = account.clone();
        let mailbox = store
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
                mailboxes::reconcile(
                    tx,
                    &id,
                    &[RemoteMailbox {
                        remote_name: b"INBOX".to_vec(),
                        name: "INBOX".into(),
                        delimiter: None,
                        role: Some(Role::Inbox),
                        selectable: true,
                        attributes: vec![],
                    }],
                )?;
                let mailbox = mailboxes::list(tx, &id)?[0].id;
                let new = (1..=3)
                    .map(|uid| NewMessage {
                        uid,
                        modseq: None,
                        flags: MessageFlags::default(),
                        received_at: Some(i64::from(uid)),
                        size: None,
                        headers: Headers::default(),
                    })
                    .collect::<Vec<_>>();
                messages::upsert(tx, mailbox, &new)?;
                Ok(mailbox)
            })
            .await
            .unwrap()
            .value;
        (temp, store, account, mailbox)
    }

    #[tokio::test]
    async fn enqueue_due_retry_and_remove() {
        let (_temp, store, account, mailbox) = store_with_messages().await;
        let operation = NewOperation {
            mailbox_id: mailbox,
            uid_validity: 7,
            kind: OperationKind::AddFlags,
            uids: vec![1, 2],
            flags: vec![FlagName::Seen],
        };
        let id = store.write(move |tx| enqueue(tx, &operation, 100)).await.unwrap().value;

        let a = account.clone();
        let due_now = store.read(move |conn| due(conn, &a, 100)).await.unwrap();
        assert_eq!(due_now.len(), 1);
        assert_eq!(due_now[0].uids, [1, 2]);
        assert_eq!(due_now[0].flags, [FlagName::Seen]);

        store
            .write(move |tx| retry_later(tx, id, "offline", 500))
            .await
            .unwrap();
        let a = account.clone();
        assert!(store.read(move |conn| due(conn, &a, 499)).await.unwrap().is_empty());
        let a = account.clone();
        assert_eq!(store.read(move |conn| next_due_at(conn, &a)).await.unwrap(), Some(500));
        assert_eq!(
            store.read(accounts_with_pending).await.unwrap(),
            std::slice::from_ref(&account)
        );

        store.write(move |tx| remove(tx, id)).await.unwrap();
        let a = account.clone();
        assert_eq!(store.read(move |conn| next_due_at(conn, &a)).await.unwrap(), None);
    }

    #[tokio::test]
    async fn pending_actions_survive_a_sync_that_brings_older_server_flags() {
        let (_temp, store, _account, mailbox) = store_with_messages().await;
        // The user marks message 1 read: local change plus a pending operation.
        store
            .write(move |tx| {
                apply_locally(tx, mailbox, &[1], &[FlagName::Seen], true)?;
                enqueue(
                    tx,
                    &NewOperation {
                        mailbox_id: mailbox,
                        uid_validity: 7,
                        kind: OperationKind::AddFlags,
                        uids: vec![1],
                        flags: vec![FlagName::Seen],
                    },
                    0,
                )
            })
            .await
            .unwrap();

        // A sync arrives before the server was told: it still says unread.
        store
            .write(move |tx| {
                messages::apply_flags(
                    tx,
                    mailbox,
                    &[FlagChange {
                        uid: 1,
                        modseq: None,
                        flags: MessageFlags::default(),
                    }],
                )?;
                reapply_pending(tx, mailbox, 7)
            })
            .await
            .unwrap();
        let listed = store
            .read(move |conn| messages::list(conn, mailbox, 10, None))
            .await
            .unwrap();
        assert!(
            listed.iter().find(|m| m.uid == 1).unwrap().flags.seen,
            "the action is not undone"
        );

        // After a UIDVALIDITY change the operation is void and dropped.
        store.write(move |tx| reapply_pending(tx, mailbox, 8)).await.unwrap();
        assert!(store.read(move |conn| pending(conn, mailbox)).await.unwrap().is_empty());
    }
}
