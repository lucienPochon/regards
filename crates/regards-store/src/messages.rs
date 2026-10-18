//! Messages: one row per message per mailbox (IMAP mailbox and UID).

use crate::StoreError;
use regards_core::headers::{Headers, Person};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageFlags {
    pub seen: bool,
    pub answered: bool,
    pub flagged: bool,
    pub draft: bool,
    pub deleted: bool,
    pub keywords: Vec<String>,
}

/// A message fetched from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMessage {
    pub uid: u32,
    pub modseq: Option<u64>,
    pub flags: MessageFlags,
    /// INTERNALDATE, Unix time in milliseconds.
    pub received_at: Option<i64>,
    pub size: Option<u32>,
    pub headers: Headers,
}

/// New flags for a stored message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagChange {
    pub uid: u32,
    pub modseq: Option<u64>,
    pub flags: MessageFlags,
}

/// A stored message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub mailbox_id: i64,
    pub uid: u32,
    pub modseq: Option<u64>,
    pub flags: MessageFlags,
    pub received_at: i64,
    pub size: Option<u32>,
    pub sent_at: Option<i64>,
    pub subject: String,
    pub from: Option<Person>,
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
    pub message_id: Option<String>,
    /// Hash of the downloaded source in `blobs/`, once it has been fetched.
    pub blob_hash: Option<String>,
    /// The folder this message is being moved to, until the server has done it.
    pub moving_to: Option<i64>,
    /// Its conversation (ADR-0009 §5); `None` until threaded.
    pub thread_id: Option<i64>,
}

/// Where a page of a message list ends: list strictly older messages next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub received_at: i64,
    pub id: i64,
}

/// Insert messages, or replace them if a row for the same UID exists.
pub fn upsert(tx: &Transaction<'_>, mailbox_id: i64, messages: &[NewMessage]) -> Result<usize, StoreError> {
    let account_id: String = tx.query_row("SELECT account_id FROM mailboxes WHERE id = ?1", [mailbox_id], |r| {
        r.get(0)
    })?;
    let mut statement = tx.prepare_cached(
        "INSERT INTO messages (mailbox_id, uid, modseq, seen, answered, flagged, draft, deleted, keywords, \
         received_at, size, sent_at, subject, from_name, from_address, to_json, cc_json, message_id_header, \
         in_reply_to, references_ids) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20) \
         ON CONFLICT (mailbox_id, uid) DO UPDATE SET modseq = excluded.modseq, seen = excluded.seen, \
         answered = excluded.answered, flagged = excluded.flagged, draft = excluded.draft, \
         deleted = excluded.deleted, keywords = excluded.keywords, received_at = excluded.received_at, \
         size = excluded.size, sent_at = excluded.sent_at, subject = excluded.subject, \
         from_name = excluded.from_name, from_address = excluded.from_address, to_json = excluded.to_json, \
         cc_json = excluded.cc_json, message_id_header = excluded.message_id_header, \
         in_reply_to = excluded.in_reply_to, references_ids = excluded.references_ids RETURNING id",
    )?;
    for message in messages {
        let headers = &message.headers;
        let received_at = message.received_at.or(headers.date).unwrap_or(0);
        let row: i64 = statement.query_row(
            params![
                mailbox_id,
                message.uid,
                message.modseq.map(|m| m as i64),
                message.flags.seen,
                message.flags.answered,
                message.flags.flagged,
                message.flags.draft,
                message.flags.deleted,
                json(&message.flags.keywords),
                received_at,
                message.size,
                headers.date,
                headers.subject,
                headers.from.as_ref().and_then(|p| p.name.clone()),
                headers.from.as_ref().and_then(|p| p.address.clone()),
                json(&headers.to),
                json(&headers.cc),
                headers.message_id,
                json(&headers.in_reply_to),
                json(&headers.references),
            ],
            |r| r.get(0),
        )?;
        let mut references = headers.in_reply_to.clone();
        references.extend(headers.references.iter().cloned());
        crate::threads::assign(tx, &account_id, row, headers.message_id.as_deref(), &references)?;
    }
    Ok(messages.len())
}

/// Apply flag changes; returns how many stored messages actually changed.
pub fn apply_flags(tx: &Transaction<'_>, mailbox_id: i64, changes: &[FlagChange]) -> Result<usize, StoreError> {
    let mut statement = tx.prepare_cached(
        "UPDATE messages SET seen = ?3, answered = ?4, flagged = ?5, draft = ?6, deleted = ?7, keywords = ?8, \
         modseq = COALESCE(?9, modseq) \
         WHERE mailbox_id = ?1 AND uid = ?2 AND (seen, answered, flagged, draft, deleted, keywords) \
         IS NOT (?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    let mut changed = 0;
    for change in changes {
        changed += statement.execute(params![
            mailbox_id,
            change.uid,
            change.flags.seen,
            change.flags.answered,
            change.flags.flagged,
            change.flags.draft,
            change.flags.deleted,
            json(&change.flags.keywords),
            change.modseq.map(|m| m as i64),
        ])?;
    }
    Ok(changed)
}

/// UIDs stored for a mailbox, ascending.
pub fn uids(conn: &Connection, mailbox_id: i64) -> Result<Vec<u32>, StoreError> {
    let mut statement = conn.prepare_cached("SELECT uid FROM messages WHERE mailbox_id = ?1 ORDER BY uid")?;
    let uids = statement
        .query_map([mailbox_id], |row| row.get(0))?
        .collect::<Result<Vec<u32>, _>>()?;
    Ok(uids)
}

/// Remove the given UIDs; returns how many were removed.
pub fn delete_uids(tx: &Transaction<'_>, mailbox_id: i64, uids: &[u32]) -> Result<usize, StoreError> {
    let mut statement = tx.prepare_cached("DELETE FROM messages WHERE mailbox_id = ?1 AND uid = ?2")?;
    let mut removed = 0;
    for uid in uids {
        removed += statement.execute(params![mailbox_id, uid])?;
    }
    Ok(removed)
}

/// Mark `uids` of `mailbox_id` as being moved to `target` (or no longer, with `None`).
pub fn set_moving_to(
    tx: &Transaction<'_>,
    mailbox_id: i64,
    uids: &[u32],
    target: Option<i64>,
) -> Result<(), StoreError> {
    let mut statement = tx.prepare_cached("UPDATE messages SET moving_to = ?3 WHERE mailbox_id = ?1 AND uid = ?2")?;
    for uid in uids {
        statement.execute(params![mailbox_id, uid, target])?;
    }
    Ok(())
}

/// The server moved `uids` from `source` to `target`. With the new UIDs
/// (COPYUID) the rows move in place and keep their ids, so undo still finds
/// them; without, they are removed and the next sync of `target` brings them.
pub fn complete_move(
    tx: &Transaction<'_>,
    source: i64,
    target: i64,
    uids: &[u32],
    new_uids: Option<&[(u32, u32)]>,
) -> Result<(), StoreError> {
    if let Some(pairs) = new_uids {
        let mut statement = tx.prepare_cached(
            "UPDATE OR IGNORE messages SET mailbox_id = ?2, uid = ?4, moving_to = NULL \n             WHERE mailbox_id = ?1 AND uid = ?3 AND moving_to = ?2",
        )?;
        for (old, new) in pairs {
            statement.execute(params![source, target, old, new])?;
        }
    }
    // Whatever did not move in place (no COPYUID, or a sync already fetched it there).
    let mut statement =
        tx.prepare_cached("DELETE FROM messages WHERE mailbox_id = ?1 AND uid = ?2 AND moving_to = ?3")?;
    for uid in uids {
        statement.execute(params![source, uid, target])?;
    }
    Ok(())
}

/// Remove every message of a mailbox, for example after a UIDVALIDITY change.
pub fn clear(tx: &Transaction<'_>, mailbox_id: i64) -> Result<usize, StoreError> {
    Ok(tx.execute("DELETE FROM messages WHERE mailbox_id = ?1", [mailbox_id])?)
}

const COLUMNS: &str = "id, mailbox_id, uid, modseq, seen, answered, flagged, draft, deleted, keywords, \
    received_at, size, sent_at, subject, from_name, from_address, to_json, cc_json, message_id_header, blob_hash, moving_to, thread_id";

/// Record where the downloaded source of message `id` is stored.
pub fn set_blob(tx: &Transaction<'_>, id: i64, hash: &str) -> Result<(), StoreError> {
    tx.execute("UPDATE messages SET blob_hash = ?2 WHERE id = ?1", params![id, hash])?;
    Ok(())
}

/// A page of a mailbox's messages, newest first, older than `before` if given.
pub fn list(
    conn: &Connection,
    mailbox_id: i64,
    limit: u32,
    before: Option<Cursor>,
) -> Result<Vec<Message>, StoreError> {
    let (received_at, id) = before.map_or((i64::MAX, i64::MAX), |c| (c.received_at, c.id));
    let mut statement = conn.prepare_cached(&format!(
        // A message being moved shows in its target folder at once. One marked
        // deleted (being deleted for good, or by another client) is not shown.
        "SELECT {COLUMNS} FROM messages \
         WHERE ((mailbox_id = ?1 AND moving_to IS NULL) OR moving_to = ?1) AND deleted = 0 \
         AND (received_at, id) < (?2, ?3) \
         ORDER BY received_at DESC, id DESC LIMIT ?4"
    ))?;
    let messages = statement
        .query_map(params![mailbox_id, received_at, id, limit], from_row)?
        .map(|row| row?)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(messages)
}

/// The messages of a conversation, oldest first, one per Message-ID (copies of
/// one message in several folders are listed once). Deleted ones are left out.
pub fn in_thread(conn: &Connection, thread_id: i64) -> Result<Vec<Message>, StoreError> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM messages WHERE thread_id = ?1 AND deleted = 0 \
         ORDER BY COALESCE(sent_at, received_at), id"
    ))?;
    let rows = statement
        .query_map([thread_id], from_row)?
        .map(|row| row?)
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = std::collections::HashSet::new();
    Ok(rows
        .into_iter()
        .filter(|m| m.message_id.as_ref().is_none_or(|id| seen.insert(id.clone())))
        .collect())
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Message>, StoreError> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM messages WHERE id = ?1"), [id], from_row)
        .optional()?
        .transpose()
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Result<Message, StoreError>> {
    let keywords: String = row.get(9)?;
    let to: String = row.get(16)?;
    let cc: String = row.get(17)?;
    let from_name: Option<String> = row.get(14)?;
    let from_address: Option<String> = row.get(15)?;
    let modseq: Option<i64> = row.get(3)?;
    let message = (|| {
        Ok(Message {
            id: row.get(0)?,
            mailbox_id: row.get(1)?,
            uid: row.get(2)?,
            modseq: modseq.map(|m| m as u64),
            flags: MessageFlags {
                seen: row.get(4)?,
                answered: row.get(5)?,
                flagged: row.get(6)?,
                draft: row.get(7)?,
                deleted: row.get(8)?,
                keywords: parse_json(&keywords, "keywords")?,
            },
            received_at: row.get(10)?,
            size: row.get(11)?,
            sent_at: row.get(12)?,
            subject: row.get(13)?,
            from: (from_name.is_some() || from_address.is_some()).then_some(Person {
                name: from_name,
                address: from_address,
            }),
            to: parse_json(&to, "recipients")?,
            cc: parse_json(&cc, "recipients")?,
            message_id: row.get(18)?,
            blob_hash: row.get(19)?,
            moving_to: row.get(20)?,
            thread_id: row.get(21)?,
        })
    })();
    Ok(message)
}

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("plain data always serializes")
}

fn parse_json<T: serde::de::DeserializeOwned>(text: &str, what: &str) -> Result<T, StoreError> {
    serde_json::from_str(text).map_err(|error| StoreError::Corrupt(format!("message {what}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use crate::mailboxes::{self, RemoteMailbox};
    use regards_core::mailbox::Role;
    use regards_core::paths::DataDirs;

    async fn store_with_inbox() -> (tempfile::TempDir, Store, i64) {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let id = accounts::new_id();
        let server = Server {
            host: "h".into(),
            port: 993,
            security: Security::Tls,
        };
        let account = NewAccount {
            id: id.clone(),
            display_name: "Ada".into(),
            email: "ada@example.test".into(),
            auth_kind: AuthKind::Password,
            username: "ada".into(),
            secret_key: "k".into(),
            imap: server.clone(),
            smtp: server,
            imap_capabilities: vec![],
        };
        let mailbox_id = store
            .write(move |tx| {
                accounts::insert(tx, account)?;
                mailboxes::reconcile(
                    tx,
                    &id,
                    &[RemoteMailbox {
                        remote_name: b"INBOX".to_vec(),
                        name: "INBOX".into(),
                        delimiter: Some('/'),
                        role: Some(Role::Inbox),
                        selectable: true,
                        attributes: vec![],
                    }],
                )?;
                Ok(mailboxes::list(tx, &id)?[0].id)
            })
            .await
            .unwrap()
            .value;
        (temp, store, mailbox_id)
    }

    fn message(uid: u32, received_at: i64, subject: &str) -> NewMessage {
        NewMessage {
            uid,
            modseq: Some(u64::from(uid)),
            flags: MessageFlags::default(),
            received_at: Some(received_at),
            size: Some(100),
            headers: Headers {
                subject: subject.into(),
                from: Some(Person {
                    name: Some("Ada".into()),
                    address: Some("ada@example.test".into()),
                }),
                ..Headers::default()
            },
        }
    }

    #[tokio::test]
    async fn upsert_list_page_flags_and_delete() {
        let (_temp, store, mailbox) = store_with_inbox().await;
        let batch: Vec<NewMessage> = (1..=5)
            .map(|uid| message(uid, i64::from(uid) * 1000, &format!("m{uid}")))
            .collect();
        store.write(move |tx| upsert(tx, mailbox, &batch)).await.unwrap();

        // Newest first, two per page.
        let page = store.read(move |conn| list(conn, mailbox, 2, None)).await.unwrap();
        assert_eq!(
            page.iter().map(|m| m.subject.as_str()).collect::<Vec<_>>(),
            ["m5", "m4"]
        );
        let cursor = Cursor {
            received_at: page[1].received_at,
            id: page[1].id,
        };
        let next = store
            .read(move |conn| list(conn, mailbox, 2, Some(cursor)))
            .await
            .unwrap();
        assert_eq!(
            next.iter().map(|m| m.subject.as_str()).collect::<Vec<_>>(),
            ["m3", "m2"]
        );
        assert_eq!(next[0].from.as_ref().unwrap().name.as_deref(), Some("Ada"));

        let seen = MessageFlags {
            seen: true,
            ..MessageFlags::default()
        };
        let changes = vec![
            FlagChange {
                uid: 1,
                modseq: Some(9),
                flags: seen.clone(),
            },
            FlagChange {
                uid: 2,
                modseq: None,
                flags: MessageFlags::default(), // unchanged
            },
        ];
        let changed = store
            .write(move |tx| apply_flags(tx, mailbox, &changes))
            .await
            .unwrap()
            .value;
        assert_eq!(changed, 1, "only real changes count");

        let removed = store
            .write(move |tx| delete_uids(tx, mailbox, &[2, 4, 99]))
            .await
            .unwrap()
            .value;
        assert_eq!(removed, 2);
        assert_eq!(store.read(move |conn| uids(conn, mailbox)).await.unwrap(), [1, 3, 5]);

        let oldest = store.read(move |conn| list(conn, mailbox, 10, None)).await.unwrap();
        let first = oldest.iter().find(|m| m.uid == 1).unwrap();
        assert!(first.flags.seen);
        assert_eq!(first.modseq, Some(9));

        // Upserting an existing UID replaces it rather than duplicating it.
        let again = vec![message(1, 1000, "m1 again")];
        store.write(move |tx| upsert(tx, mailbox, &again)).await.unwrap();
        assert_eq!(store.read(move |conn| uids(conn, mailbox)).await.unwrap(), [1, 3, 5]);

        let cleared = store.write(move |tx| clear(tx, mailbox)).await.unwrap().value;
        assert_eq!(cleared, 3);
    }
}
