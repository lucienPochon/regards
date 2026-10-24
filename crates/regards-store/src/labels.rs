//! Gmail labels (migration 37). Over IMAP, Gmail shows each label as a
//! folder: a message has a label when a copy of it is in that folder,
//! matched by Message-ID within the account. A label added on this device
//! counts at once, until its copy comes back in a sync of the label's folder.

use crate::StoreError;
use rusqlite::{Connection, Transaction, params};
use std::collections::{BTreeMap, BTreeSet};

/// The folders (aliased `b`) that are labels: an account's own folders, not
/// its system ones. Inbox, Sent and the rest have a role; Gmail's own, such
/// as Important, sit under "[Gmail]" (or "[Google Mail]").
const LABEL_FOLDER: &str = "b.role IS NULL AND b.selectable = 1 \
    AND b.name NOT LIKE '[Gmail]/%' AND b.name NOT LIKE '[Google Mail]/%'";

/// Whether `mailbox_id` is a folder that can be a label.
pub fn is_label_folder(conn: &Connection, mailbox_id: i64) -> Result<bool, StoreError> {
    Ok(conn.query_row(
        &format!("SELECT EXISTS (SELECT 1 FROM mailboxes b WHERE b.id = ?1 AND {LABEL_FOLDER})"),
        [mailbox_id],
        |r| r.get(0),
    )?)
}

/// Each of `ids` (in their order) with its labels, as folder ids, smallest
/// first: the label folders holding a copy of it that still shows there
/// (not deleted; or moved there), and the labels added here and not synced
/// back yet. A message without a Message-ID has none.
pub fn of(conn: &Connection, ids: &[i64]) -> Result<Vec<(i64, Vec<i64>)>, StoreError> {
    let mut found: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
    let mut copies = conn.prepare_cached(&format!(
        "SELECT b.id FROM messages m \
         JOIN mailboxes a ON a.id = m.mailbox_id \
         JOIN messages c ON c.message_id_header = m.message_id_header AND c.deleted = 0 \
         JOIN mailboxes b ON b.id = COALESCE(c.moving_to, c.mailbox_id) AND b.account_id = a.account_id \
         WHERE m.id = ?1 AND m.message_id_header IS NOT NULL AND {LABEL_FOLDER}"
    ))?;
    let mut added = conn.prepare_cached(
        "SELECT l.mailbox_id FROM messages m \
         JOIN mailboxes a ON a.id = m.mailbox_id \
         JOIN label_adds l ON l.account_id = a.account_id AND l.message_id_header = m.message_id_header \
         WHERE m.id = ?1",
    )?;
    for id in ids {
        if found.contains_key(id) {
            continue;
        }
        let mut labels = BTreeSet::new();
        for row in copies.query_map([id], |r| r.get::<_, i64>(0))? {
            labels.insert(row?);
        }
        for row in added.query_map([id], |r| r.get::<_, i64>(0))? {
            labels.insert(row?);
        }
        found.insert(*id, labels);
    }
    Ok(ids
        .iter()
        .map(|id| (*id, found[id].iter().copied().collect()))
        .collect())
}

/// The copies of a message (by Message-ID) that show in `mailbox_id`: what
/// removing that label deletes, as (id, UID).
pub fn copies_in(conn: &Connection, mailbox_id: i64, message_id: &str) -> Result<Vec<(i64, u32)>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT id, uid FROM messages \
         WHERE mailbox_id = ?1 AND message_id_header = ?2 AND deleted = 0 AND moving_to IS NULL",
    )?;
    let rows = statement.query_map(params![mailbox_id, message_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Note a label added here, while its copy is on its way.
pub fn add_pending(
    tx: &Transaction<'_>,
    account_id: &str,
    message_id: &str,
    mailbox_id: i64,
    now: i64,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT OR REPLACE INTO label_adds (account_id, message_id_header, mailbox_id, added_at) \
         VALUES (?1, ?2, ?3, ?4)",
        params![account_id, message_id, mailbox_id, now],
    )?;
    Ok(())
}

/// Forget a label added here: removed again, or its copy could not be made.
/// Whether one was pending.
pub fn remove_pending(
    tx: &Transaction<'_>,
    account_id: &str,
    message_id: &str,
    mailbox_id: i64,
) -> Result<bool, StoreError> {
    Ok(tx.execute(
        "DELETE FROM label_adds WHERE account_id = ?1 AND message_id_header = ?2 AND mailbox_id = ?3",
        params![account_id, message_id, mailbox_id],
    )? > 0)
}

/// A message came in from a sync of `mailbox_id`: a label added here for it
/// is now shown by the copy itself.
pub(crate) fn synced(tx: &Transaction<'_>, mailbox_id: i64, message_id: &str) -> Result<(), StoreError> {
    tx.prepare_cached("DELETE FROM label_adds WHERE mailbox_id = ?1 AND message_id_header = ?2")?
        .execute(params![mailbox_id, message_id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use crate::mailboxes::{self, RemoteMailbox};
    use crate::messages::{self, MessageFlags, NewMessage};
    use regards_core::headers::Headers;
    use regards_core::mailbox::Role;
    use regards_core::paths::DataDirs;

    fn folder(name: &str, role: Option<Role>) -> RemoteMailbox {
        RemoteMailbox {
            remote_name: name.as_bytes().to_vec(),
            name: name.into(),
            delimiter: Some('/'),
            role,
            selectable: true,
            attributes: vec![],
        }
    }

    fn message(uid: u32, message_id: &str) -> NewMessage {
        NewMessage {
            uid,
            modseq: Some(u64::from(uid)),
            flags: MessageFlags::default(),
            received_at: Some(i64::from(uid) * 1000),
            size: Some(100),
            headers: Headers {
                subject: format!("m{uid}"),
                message_id: Some(message_id.into()),
                ..Headers::default()
            },
        }
    }

    #[tokio::test]
    async fn a_label_is_a_copy_in_a_label_folder_or_one_added_here() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let account = accounts::new_id();
        let server = Server {
            host: "imap.gmail.com".into(),
            port: 993,
            security: Security::Tls,
        };
        let a = account.clone();
        let folders = store
            .write(move |tx| {
                accounts::insert(
                    tx,
                    NewAccount {
                        id: a.clone(),
                        display_name: "Ada".into(),
                        email: "ada@gmail.test".into(),
                        auth_kind: AuthKind::Password,
                        username: "ada".into(),
                        secret_key: "k".into(),
                        imap: server.clone(),
                        smtp: server,
                        imap_capabilities: vec!["X-GM-EXT-1".into()],
                    },
                )?;
                mailboxes::reconcile(
                    tx,
                    &a,
                    &[
                        folder("INBOX", Some(Role::Inbox)),
                        folder("Work", None),
                        folder("Family", None),
                        folder("[Gmail]/All Mail", Some(Role::All)),
                        folder("[Gmail]/Important", None),
                    ],
                )?;
                let by_name = |name: &str| -> Result<i64, StoreError> {
                    Ok(mailboxes::list(tx, &a)?
                        .into_iter()
                        .find(|m| m.name == name)
                        .unwrap()
                        .id)
                };
                let ids = ["INBOX", "Work", "Family", "[Gmail]/All Mail", "[Gmail]/Important"]
                    .map(|name| by_name(name).unwrap());
                messages::upsert(tx, ids[0], &[message(1, "a@x"), message(2, "b@x")])?;
                messages::upsert(tx, ids[1], &[message(7, "a@x")])?;
                messages::upsert(tx, ids[3], &[message(30, "a@x"), message(31, "b@x")])?;
                messages::upsert(tx, ids[4], &[message(40, "a@x")])?;
                Ok(ids)
            })
            .await
            .unwrap()
            .value;
        let [inbox, work, family, _all, important] = folders;
        let row = |mailbox: i64, uid: u32| {
            store.read(move |c| {
                Ok(c.query_row(
                    "SELECT id FROM messages WHERE mailbox_id = ?1 AND uid = ?2",
                    params![mailbox, uid],
                    |r| r.get::<_, i64>(0),
                )?)
            })
        };
        let (a, b) = (row(inbox, 1).await.unwrap(), row(inbox, 2).await.unwrap());

        let labels = store.read(move |c| of(c, &[a, b])).await.unwrap();
        assert_eq!(
            labels,
            [(a, vec![work]), (b, vec![])],
            "Work's copy; not Inbox, All Mail or Important"
        );
        assert!(store.read(move |c| is_label_folder(c, family)).await.unwrap());
        assert!(!store.read(move |c| is_label_folder(c, important)).await.unwrap());

        // Added here: shown at once, until the copy comes back.
        let acc = account.clone();
        store
            .write(move |tx| add_pending(tx, &acc, "b@x", family, 1))
            .await
            .unwrap();
        assert_eq!(store.read(move |c| of(c, &[b])).await.unwrap(), [(b, vec![family])]);
        store
            .write(move |tx| messages::upsert(tx, family, &[message(3, "b@x")]))
            .await
            .unwrap();
        let pending: i64 = store
            .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM label_adds", [], |r| r.get(0))?))
            .await
            .unwrap();
        assert_eq!(pending, 0, "the synced copy replaces the pending label");
        assert_eq!(store.read(move |c| of(c, &[b])).await.unwrap(), [(b, vec![family])]);

        // Removing a label is deleting its copy.
        let copies = store.read(move |c| copies_in(c, work, "a@x")).await.unwrap();
        assert_eq!(copies.len(), 1);
        let (_, uid) = copies[0];
        store
            .write(move |tx| {
                crate::operations::apply_locally(tx, work, &[uid], &[crate::operations::FlagName::Deleted], true)
            })
            .await
            .unwrap();
        assert_eq!(store.read(move |c| of(c, &[a])).await.unwrap(), [(a, vec![])]);
    }
}
