//! Mailboxes (folders), reconciled with the server's `LIST`.

use crate::StoreError;
use regards_core::mailbox::Role;
use rusqlite::{Connection, Row, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A mailbox as the server listed it, with its role already assigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMailbox {
    pub remote_name: Vec<u8>,
    pub name: String,
    pub delimiter: Option<char>,
    pub role: Option<Role>,
    pub selectable: bool,
    pub attributes: Vec<String>,
}

/// A stored mailbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mailbox {
    pub id: i64,
    pub account_id: String,
    pub remote_name: Vec<u8>,
    pub name: String,
    pub delimiter: Option<char>,
    pub role: Option<Role>,
    pub selectable: bool,
    pub attributes: Vec<String>,
    pub uid_validity: Option<i64>,
    pub uid_next: Option<i64>,
    pub highest_modseq: Option<i64>,
    /// Unix time in milliseconds.
    pub synced_at: Option<i64>,
}

/// What a reconciliation changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reconciled {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
}

const COLUMNS: &str = "id, account_id, remote_name, name, delimiter, role, selectable, attributes, \
    uid_validity, uid_next, highest_modseq, synced_at";

/// Make the stored mailboxes of `account_id` match `listed`: add new ones,
/// update changed ones and remove those the server no longer lists. Sync
/// state (UIDVALIDITY and so on) of kept mailboxes is preserved.
pub fn reconcile(tx: &Transaction<'_>, account_id: &str, listed: &[RemoteMailbox]) -> Result<Reconciled, StoreError> {
    let existing: HashMap<Vec<u8>, Mailbox> = list(tx, account_id)?
        .into_iter()
        .map(|m| (m.remote_name.clone(), m))
        .collect();
    let mut result = Reconciled::default();

    // Roles can move between mailboxes; clear them first so the
    // one-per-role index never sees two owners mid-update.
    tx.execute("UPDATE mailboxes SET role = NULL WHERE account_id = ?1", [account_id])?;

    for mailbox in listed {
        let delimiter = mailbox.delimiter.map(String::from);
        let role = mailbox.role.map(Role::as_str);
        let attributes = serde_json::to_string(&mailbox.attributes).expect("strings always serialize");
        match existing.get(&mailbox.remote_name) {
            None => {
                tx.execute(
                    "INSERT INTO mailboxes (account_id, remote_name, name, delimiter, role, selectable, attributes) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        account_id,
                        mailbox.remote_name,
                        mailbox.name,
                        delimiter,
                        role,
                        mailbox.selectable,
                        attributes
                    ],
                )?;
                result.added += 1;
            }
            Some(stored) => {
                tx.execute(
                    "UPDATE mailboxes SET name = ?2, delimiter = ?3, role = ?4, selectable = ?5, attributes = ?6 \
                     WHERE id = ?1",
                    params![stored.id, mailbox.name, delimiter, role, mailbox.selectable, attributes],
                )?;
                let unchanged = stored.name == mailbox.name
                    && stored.delimiter == mailbox.delimiter
                    && stored.role == mailbox.role
                    && stored.selectable == mailbox.selectable
                    && stored.attributes == mailbox.attributes;
                if !unchanged {
                    result.updated += 1;
                }
            }
        }
    }

    for (remote_name, stored) in &existing {
        if !listed.iter().any(|m| &m.remote_name == remote_name) {
            tx.execute("DELETE FROM mailboxes WHERE id = ?1", [stored.id])?;
            result.removed += 1;
        }
    }
    Ok(result)
}

/// The mailboxes of an account: role folders first (inbox, drafts, sent, …), then by name.
pub fn list(conn: &Connection, account_id: &str) -> Result<Vec<Mailbox>, StoreError> {
    let mut statement = conn.prepare(&format!("SELECT {COLUMNS} FROM mailboxes WHERE account_id = ?1"))?;
    let mut mailboxes = statement
        .query_map([account_id], from_row)?
        .map(|row| row?)
        .collect::<Result<Vec<_>, _>>()?;
    mailboxes.sort_by_key(|m| (role_rank(m.role), m.name.to_lowercase()));
    Ok(mailboxes)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Mailbox>, StoreError> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM mailboxes WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .transpose()
}

/// Folders renamed on the server, here too: each (id, remote name, name)
/// takes its new path and keeps its mail and sync state, so the next
/// reconciliation finds it rather than a new folder.
pub fn set_paths(tx: &Transaction<'_>, paths: &[(i64, Vec<u8>, String)]) -> Result<(), StoreError> {
    let mut statement = tx.prepare_cached("UPDATE mailboxes SET remote_name = ?2, name = ?3 WHERE id = ?1")?;
    for (id, remote_name, name) in paths {
        statement.execute(params![id, remote_name, name])?;
    }
    Ok(())
}

/// What the last message sync of a mailbox established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncPoint {
    pub uid_validity: u32,
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
    /// Unix time in milliseconds.
    pub synced_at: i64,
}

pub fn set_sync_point(tx: &Transaction<'_>, id: i64, point: SyncPoint) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE mailboxes SET uid_validity = ?2, uid_next = ?3, highest_modseq = ?4, synced_at = ?5 WHERE id = ?1",
        params![
            id,
            point.uid_validity,
            point.uid_next,
            point.highest_modseq.map(|m| m as i64),
            point.synced_at
        ],
    )?;
    Ok(())
}

fn role_rank(role: Option<Role>) -> u8 {
    match role {
        Some(Role::Inbox) => 0,
        Some(Role::Drafts) => 1,
        Some(Role::Sent) => 2,
        Some(Role::Archive) => 3,
        Some(Role::All) => 4,
        Some(Role::Flagged) => 5,
        Some(Role::Junk) => 6,
        Some(Role::Trash) => 7,
        None => 8,
    }
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Result<Mailbox, StoreError>> {
    let delimiter: Option<String> = row.get(4)?;
    let role: Option<String> = row.get(5)?;
    let attributes: String = row.get(7)?;
    let mailbox = (|| {
        Ok(Mailbox {
            id: row.get(0)?,
            account_id: row.get(1)?,
            remote_name: row.get(2)?,
            name: row.get(3)?,
            delimiter: delimiter.and_then(|d| d.chars().next()),
            role: match role {
                None => None,
                Some(value) => {
                    Some(Role::parse(&value).ok_or_else(|| StoreError::Corrupt(format!("unknown role {value:?}")))?)
                }
            },
            selectable: row.get(6)?,
            attributes: serde_json::from_str(&attributes)
                .map_err(|error| StoreError::Corrupt(format!("mailbox attributes: {error}")))?,
            uid_validity: row.get(8)?,
            uid_next: row.get(9)?,
            highest_modseq: row.get(10)?,
            synced_at: row.get(11)?,
        })
    })();
    Ok(mailbox)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use regards_core::paths::DataDirs;

    fn remote(name: &str, role: Option<Role>) -> RemoteMailbox {
        RemoteMailbox {
            remote_name: name.as_bytes().to_vec(),
            name: name.into(),
            delimiter: Some('/'),
            role,
            selectable: true,
            attributes: vec![],
        }
    }

    async fn store_with_account() -> (tempfile::TempDir, Store, String) {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let id = accounts::new_id();
        let server = Server {
            host: "mail.example.test".into(),
            port: 993,
            security: Security::Tls,
        };
        let account = NewAccount {
            id: id.clone(),
            display_name: "Ada".into(),
            email: "ada@example.test".into(),
            auth_kind: AuthKind::Password,
            username: "ada".into(),
            secret_key: format!("{id}/mail-password"),
            imap: server.clone(),
            smtp: server,
            imap_capabilities: vec![],
        };
        store.write(move |tx| accounts::insert(tx, account)).await.unwrap();
        (temp, store, id)
    }

    #[tokio::test]
    async fn reconcile_adds_updates_and_removes_and_keeps_sync_state() {
        let (_temp, store, id) = store_with_account().await;
        let first = vec![
            remote("INBOX", Some(Role::Inbox)),
            remote("Sent", Some(Role::Sent)),
            remote("Old", None),
            remote("Archive", None),
        ];
        let account = id.clone();
        let result = store
            .write(move |tx| reconcile(tx, &account, &first))
            .await
            .unwrap()
            .value;
        assert_eq!(
            result,
            Reconciled {
                added: 4,
                updated: 0,
                removed: 0
            }
        );

        let account = id.clone();
        store
            .write(move |tx| {
                tx.execute(
                    "UPDATE mailboxes SET uid_validity = 42 WHERE account_id = ?1 AND name = 'INBOX'",
                    [&account],
                )?;
                Ok(())
            })
            .await
            .unwrap();

        // "Old" disappears; the archive role moves onto "Archive"; the inbox is unchanged.
        let second = vec![
            remote("INBOX", Some(Role::Inbox)),
            remote("Sent", Some(Role::Sent)),
            remote("Archive", Some(Role::Archive)),
        ];
        let account = id.clone();
        let result = store
            .write(move |tx| reconcile(tx, &account, &second))
            .await
            .unwrap()
            .value;
        assert_eq!(
            result,
            Reconciled {
                added: 0,
                updated: 1,
                removed: 1
            }
        );

        let account = id.clone();
        let stored = store.read(move |conn| list(conn, &account)).await.unwrap();
        let names: Vec<&str> = stored.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            ["INBOX", "Sent", "Archive"],
            "role folders first, in a fixed order"
        );
        assert_eq!(stored[0].uid_validity, Some(42), "sync state survives a reconcile");
    }

    #[tokio::test]
    async fn a_role_can_move_to_another_mailbox_in_one_reconcile() {
        let (_temp, store, id) = store_with_account().await;
        let account = id.clone();
        store
            .write(move |tx| {
                reconcile(
                    tx,
                    &account,
                    &[remote("Sent", Some(Role::Sent)), remote("Sent Items", None)],
                )
            })
            .await
            .unwrap();
        let account = id.clone();
        store
            .write(move |tx| {
                reconcile(
                    tx,
                    &account,
                    &[remote("Sent", None), remote("Sent Items", Some(Role::Sent))],
                )
            })
            .await
            .unwrap();
        let account = id.clone();
        let stored = store.read(move |conn| list(conn, &account)).await.unwrap();
        assert_eq!(stored[0].name, "Sent Items");
        assert_eq!(stored[0].role, Some(Role::Sent));
    }

    #[tokio::test]
    async fn removing_the_account_removes_its_mailboxes() {
        let (_temp, store, id) = store_with_account().await;
        let account = id.clone();
        store
            .write(move |tx| reconcile(tx, &account, &[remote("INBOX", Some(Role::Inbox))]))
            .await
            .unwrap();
        let account = id.clone();
        store.write(move |tx| accounts::delete(tx, &account)).await.unwrap();
        let count: i64 = store
            .read(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM mailboxes", [], |r| r.get(0))?))
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
