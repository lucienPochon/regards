//! Conversations by References threading (ADR-0009 §5).
//!
//! Every Message-ID a message has or refers to (In-Reply-To, References)
//! belongs to one thread per account. A message joins the thread of any ID it
//! shares; when it links two threads, they merge. Copies of one message in
//! several folders (Inbox, Sent, All Mail) share a Message-ID and so a thread.
//! Messages without any ID get a thread of their own.

use crate::StoreError;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::collections::BTreeSet;

/// Put message `row` (of `account_id`) in its thread; returns the thread id.
pub fn assign(
    tx: &Transaction<'_>,
    account_id: &str,
    row: i64,
    message_id: Option<&str>,
    references: &[String],
) -> Result<i64, StoreError> {
    let ids: BTreeSet<&str> = message_id
        .into_iter()
        .chain(references.iter().map(String::as_str))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .collect();

    let mut found = BTreeSet::new();
    {
        let mut statement =
            tx.prepare_cached("SELECT thread_id FROM thread_ids WHERE account_id = ?1 AND message_id = ?2")?;
        for id in &ids {
            if let Some(thread) = statement
                .query_row(params![account_id, id], |r| r.get::<_, i64>(0))
                .optional()?
            {
                found.insert(thread);
            }
        }
    }
    // A message that is already threaded keeps its thread when nothing links it elsewhere.
    let current: Option<i64> = tx.query_row("SELECT thread_id FROM messages WHERE id = ?1", [row], |r| r.get(0))?;
    found.extend(current);

    let thread = match found.first() {
        Some(thread) => *thread,
        None => {
            tx.execute("INSERT INTO threads (account_id) VALUES (?1)", [account_id])?;
            tx.last_insert_rowid()
        }
    };
    // This message links several threads: they are one conversation.
    for other in found.iter().skip(1) {
        tx.execute(
            "UPDATE thread_ids SET thread_id = ?1 WHERE thread_id = ?2",
            params![thread, other],
        )?;
        tx.execute(
            "UPDATE messages SET thread_id = ?1 WHERE thread_id = ?2",
            params![thread, other],
        )?;
        tx.execute("DELETE FROM threads WHERE id = ?1", [other])?;
    }
    let mut insert =
        tx.prepare_cached("INSERT OR REPLACE INTO thread_ids (account_id, message_id, thread_id) VALUES (?1, ?2, ?3)")?;
    for id in &ids {
        insert.execute(params![account_id, id, thread])?;
    }
    tx.execute("UPDATE messages SET thread_id = ?1 WHERE id = ?2", params![thread, row])?;
    Ok(thread)
}

/// Whether some messages have no thread yet.
pub fn any_missing(conn: &Connection) -> Result<bool, StoreError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM messages WHERE thread_id IS NULL)",
        [],
        |r| r.get(0),
    )?)
}

/// Thread up to `limit` messages that have no thread yet (those synced before
/// threading existed). Returns how many were threaded; 0 means none are left.
pub fn assign_missing(tx: &Transaction<'_>, limit: u32) -> Result<usize, StoreError> {
    let pending: Vec<(i64, String, Option<String>, String, String)> = {
        let mut statement = tx.prepare(
            "SELECT m.id, b.account_id, m.message_id_header, m.in_reply_to, m.references_ids \
             FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id \
             WHERE m.thread_id IS NULL ORDER BY m.id LIMIT ?1",
        )?;
        statement
            .query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
            .collect::<Result<_, _>>()?
    };
    for (row, account, message_id, in_reply_to, references) in &pending {
        let mut refs: Vec<String> = serde_json::from_str(in_reply_to).unwrap_or_default();
        refs.extend(serde_json::from_str::<Vec<String>>(references).unwrap_or_default());
        assign(tx, account, *row, message_id.as_deref(), &refs)?;
    }
    Ok(pending.len())
}

#[cfg(test)]
mod tests {
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use crate::mailboxes::{self, RemoteMailbox};
    use crate::messages::{self, MessageFlags, NewMessage};
    use regards_core::headers::Headers;
    use regards_core::mailbox::Role;
    use regards_core::paths::DataDirs;

    fn new(uid: u32, id: &str, in_reply_to: &[&str], references: &[&str]) -> NewMessage {
        NewMessage {
            uid,
            modseq: None,
            flags: MessageFlags::default(),
            received_at: Some(i64::from(uid)),
            size: None,
            headers: Headers {
                message_id: (!id.is_empty()).then(|| id.to_owned()),
                in_reply_to: in_reply_to.iter().map(|s| s.to_string()).collect(),
                references: references.iter().map(|s| s.to_string()).collect(),
                ..Headers::default()
            },
        }
    }

    /// The thread of each UID in the inbox, in UID order.
    async fn threads_of(store: &Store, inbox: i64) -> Vec<Option<i64>> {
        store
            .read(move |conn| {
                let mut statement =
                    conn.prepare("SELECT thread_id FROM messages WHERE mailbox_id = ?1 ORDER BY uid")?;
                let threads = statement.query_map([inbox], |r| r.get(0))?.collect::<Result<_, _>>()?;
                Ok(threads)
            })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn replies_join_their_thread_and_links_merge_threads() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let account = accounts::new_id();
        let id = account.clone();
        let inbox = store
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
                Ok(mailboxes::list(tx, &id)?[0].id)
            })
            .await
            .unwrap()
            .value;

        // 1 starts a thread; 2 replies to 1; 3 is separate; 4 is unrelated and has no ID.
        let batch = vec![
            new(1, "root@x", &[], &[]),
            new(2, "reply@x", &["root@x"], &["root@x"]),
            new(3, "other@x", &[], &[]),
            new(4, "", &[], &[]),
        ];
        store
            .write(move |tx| messages::upsert(tx, inbox, &batch))
            .await
            .unwrap();
        let t = threads_of(&store, inbox).await;
        assert!(t.iter().all(Option::is_some), "every message is threaded");
        assert_eq!(t[0], t[1], "a reply joins its parent");
        assert_ne!(t[0], t[2]);
        assert_ne!(t[3], t[0]);
        assert_ne!(t[3], t[2]);

        // 5 references both 2 and 3: the two conversations are one.
        let link = vec![new(5, "link@x", &["reply@x"], &["root@x", "reply@x", "other@x"])];
        store.write(move |tx| messages::upsert(tx, inbox, &link)).await.unwrap();
        let t = threads_of(&store, inbox).await;
        assert_eq!(t[0], t[2]);
        assert_eq!(t[0], t[4]);

        // A reply arriving before its parent: the parent joins when it comes.
        let early = vec![new(6, "child@y", &["parent@y"], &["parent@y"])];
        store
            .write(move |tx| messages::upsert(tx, inbox, &early))
            .await
            .unwrap();
        let late = vec![new(7, "parent@y", &[], &[])];
        store.write(move |tx| messages::upsert(tx, inbox, &late)).await.unwrap();
        let t = threads_of(&store, inbox).await;
        assert_eq!(t[5], t[6]);
        assert_ne!(t[5], t[0]);

        // Messages stored before threading existed are threaded the same way, in batches.
        store
            .write(|tx| {
                tx.execute_batch("UPDATE messages SET thread_id = NULL; DELETE FROM thread_ids; DELETE FROM threads;")?;
                Ok(())
            })
            .await
            .unwrap();
        assert!(store.read(super::any_missing).await.unwrap());
        let mut batches = 0;
        while store.write(|tx| super::assign_missing(tx, 3)).await.unwrap().value > 0 {
            batches += 1;
        }
        assert_eq!(batches, 3);
        assert!(!store.read(super::any_missing).await.unwrap());
        let t = threads_of(&store, inbox).await;
        assert!(t.iter().all(Option::is_some));
        assert_eq!(t[0], t[1]);
        assert_eq!(t[0], t[2]);
        assert_eq!(t[0], t[4]);
        assert_eq!(t[5], t[6]);
        assert_ne!(t[5], t[0]);
        assert_ne!(t[3], t[0]);
    }
}
