//! Drafts kept in the server's Drafts folder too (plan §5.9), so other
//! devices and webmail have them, and drafts written elsewhere can be picked
//! up here. The daemon's uploader does the talking; this keeps the books:
//!
//! - **Due:** a draft whose copy is missing or older, once it has been left
//!   alone a moment, and not while it is being sent.
//! - **Uploaded:** the new copy is recorded and the one it replaces is to be
//!   deleted. A copy made for a draft that went meanwhile is deleted too.
//! - **Deleted with the draft:** a trigger (migration 21) queues the copy.
//! - **Vanished:** a copy the server no longer has was sent or thrown away
//!   elsewhere. An unchanged draft follows it; a changed one is uploaded anew.
//! - **Strays:** other copies carrying a draft's Message-ID (left by a crash
//!   between upload and record) are deleted.

use crate::StoreError;
use crate::drafts::{self, Content};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

/// A draft to upload: its account and the version to upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    pub draft_id: i64,
    pub account_id: String,
    pub version: i64,
}

/// Drafts whose copy is missing or older than they are, changed at least
/// `quiet_ms` ago, of accounts that have a Drafts folder; oldest first.
pub fn due(conn: &Connection, now: i64, quiet_ms: i64, limit: u32) -> Result<Vec<Due>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT d.id, d.account_id, d.updated_at FROM drafts d \
         WHERE (d.uploaded_at IS NULL OR d.uploaded_at != d.updated_at) AND d.updated_at <= ?1 - ?2 \
         AND NOT EXISTS (SELECT 1 FROM outbox o WHERE o.draft_id = d.id) \
         AND EXISTS (SELECT 1 FROM mailboxes b WHERE b.account_id = d.account_id AND b.role = 'drafts') \
         ORDER BY d.updated_at LIMIT ?3",
    )?;
    let rows = statement.query_map(params![now, quiet_ms, limit], |row| {
        Ok(Due {
            draft_id: row.get(0)?,
            account_id: row.get(1)?,
            version: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// The Message-ID every copy of the draft carries: `make` gives it the first
/// time. `None` if the draft is gone.
pub fn message_id(
    tx: &Transaction<'_>,
    draft_id: i64,
    make: impl FnOnce() -> String,
) -> Result<Option<String>, StoreError> {
    let current: Option<Option<String>> = tx
        .query_row(
            "SELECT server_message_id FROM drafts WHERE id = ?1",
            [draft_id],
            |row| row.get(0),
        )
        .optional()?;
    match current {
        None => Ok(None),
        Some(Some(id)) => Ok(Some(id)),
        Some(None) => {
            let id = make();
            tx.execute(
                "UPDATE drafts SET server_message_id = ?2 WHERE id = ?1",
                params![draft_id, id],
            )?;
            Ok(Some(id))
        }
    }
}

/// The copy holding `version` of the draft is in `mailbox_id` as `uid`. The
/// copy it replaces is queued for deletion; if the draft went meanwhile, or
/// moved to another account, so is this one. Returns whether it is kept.
pub fn uploaded(
    tx: &Transaction<'_>,
    account_id: &str,
    draft_id: i64,
    version: i64,
    mailbox_id: i64,
    uid: u32,
) -> Result<bool, StoreError> {
    let old: Option<(String, Option<i64>, Option<u32>)> = tx
        .query_row(
            "SELECT account_id, server_mailbox_id, server_uid FROM drafts WHERE id = ?1",
            [draft_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((_, old_mailbox, old_uid)) = old.filter(|(owner, _, _)| owner == account_id) else {
        queue_delete(tx, account_id, mailbox_id, uid)?;
        return Ok(false);
    };
    let old = (old_mailbox, old_uid);
    tx.execute(
        "UPDATE drafts SET server_mailbox_id = ?2, server_uid = ?3, uploaded_at = ?4 WHERE id = ?1",
        params![draft_id, mailbox_id, uid, version],
    )?;
    if let (Some(old_mailbox), Some(old_uid)) = old
        && (old_mailbox, old_uid) != (mailbox_id, uid)
    {
        queue_delete(tx, account_id, old_mailbox, old_uid)?;
    }
    Ok(true)
}

/// The copy `uid` in `mailbox_id`, of `account_id`'s server, is to go.
pub(crate) fn queue_delete(
    tx: &Transaction<'_>,
    account_id: &str,
    mailbox_id: i64,
    uid: u32,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO draft_copies_to_delete (account_id, mailbox_id, uid) SELECT ?1, ?2, ?3 \
         WHERE NOT EXISTS (SELECT 1 FROM draft_copies_to_delete WHERE mailbox_id = ?2 AND uid = ?3)",
        params![account_id, mailbox_id, uid],
    )?;
    Ok(())
}

/// A copy to remove from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyToDelete {
    pub id: i64,
    pub account_id: String,
    pub mailbox_id: i64,
    pub uid: u32,
}

pub fn copies_to_delete(conn: &Connection) -> Result<Vec<CopyToDelete>, StoreError> {
    let mut statement =
        conn.prepare_cached("SELECT id, account_id, mailbox_id, uid FROM draft_copies_to_delete ORDER BY id")?;
    let rows = statement.query_map([], |row| {
        Ok(CopyToDelete {
            id: row.get(0)?,
            account_id: row.get(1)?,
            mailbox_id: row.get(2)?,
            uid: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// These copies are gone from the server (or their folder or account is).
pub fn copies_deleted(tx: &Transaction<'_>, ids: &[i64]) -> Result<(), StoreError> {
    let mut statement = tx.prepare_cached("DELETE FROM draft_copies_to_delete WHERE id = ?1")?;
    for id in ids {
        statement.execute([id])?;
    }
    Ok(())
}

/// Whether [`settle`] has anything to do: a read, so an idle pass writes nothing.
pub fn unsettled(conn: &Connection) -> Result<bool, StoreError> {
    Ok(
        conn.query_row(&format!("SELECT EXISTS ({VANISHED}) OR EXISTS ({STRAYS})"), [], |row| {
            row.get(0)
        })?,
    )
}

/// Copies the server no longer has (and the sync has looked past them):
/// their draft id, version and uploaded version.
const VANISHED: &str = "SELECT d.id, d.updated_at, d.uploaded_at FROM drafts d \
    JOIN mailboxes b ON b.id = d.server_mailbox_id \
    WHERE d.server_uid IS NOT NULL AND b.uid_next > d.server_uid \
    AND NOT EXISTS (SELECT 1 FROM messages m WHERE m.mailbox_id = d.server_mailbox_id AND m.uid = d.server_uid AND m.deleted = 0) \
    AND NOT EXISTS (SELECT 1 FROM outbox o WHERE o.draft_id = d.id)";

/// Copies in a Drafts folder carrying a draft's Message-ID that are not its
/// current copy: account, folder, UID.
const STRAYS: &str = "SELECT d.account_id, m.mailbox_id, m.uid FROM drafts d \
    JOIN messages m ON m.message_id_header = d.server_message_id \
    JOIN mailboxes b ON b.id = m.mailbox_id AND b.account_id = d.account_id AND b.role = 'drafts' \
    WHERE m.deleted = 0 AND (d.server_uid IS NULL OR m.uid != d.server_uid OR m.mailbox_id IS NOT d.server_mailbox_id) \
    AND NOT EXISTS (SELECT 1 FROM draft_copies_to_delete q WHERE q.mailbox_id = m.mailbox_id AND q.uid = m.uid)";

/// Act on vanished copies and strays; returns how many drafts went.
pub fn settle(tx: &Transaction<'_>) -> Result<usize, StoreError> {
    let vanished: Vec<(i64, i64, Option<i64>)> = tx
        .prepare(VANISHED)?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let mut gone = 0;
    for (draft, version, uploaded) in vanished {
        // Forget the copy first, so the trigger does not ask to delete it.
        tx.execute(
            "UPDATE drafts SET server_mailbox_id = NULL, server_uid = NULL, uploaded_at = NULL WHERE id = ?1",
            [draft],
        )?;
        if uploaded == Some(version) {
            tx.execute("DELETE FROM drafts WHERE id = ?1", [draft])?;
            gone += 1;
        }
    }
    let strays: Vec<(String, i64, u32)> = tx
        .prepare(STRAYS)?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (account, mailbox, uid) in strays {
        queue_delete(tx, &account, mailbox, uid)?;
    }
    Ok(gone)
}

/// A draft written elsewhere, picked up here: a draft of its own, whose
/// copy is that message (so it is uploaded again only once changed).
pub fn adopt(
    tx: &Transaction<'_>,
    account_id: &str,
    content: &Content,
    copy: (i64, u32, &str),
    now: i64,
) -> Result<i64, StoreError> {
    let id = drafts::create(tx, account_id, content, now)?;
    let (mailbox_id, uid, message_id) = copy;
    tx.execute(
        "UPDATE drafts SET server_message_id = ?2, server_mailbox_id = ?3, server_uid = ?4, uploaded_at = updated_at \
         WHERE id = ?1",
        params![id, message_id, mailbox_id, uid],
    )?;
    Ok(id)
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

    async fn fixture() -> (tempfile::TempDir, Store, String, i64) {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let (account, drafts_box) = store
            .write(|tx| account_with_drafts(tx, "ada@example.test"))
            .await
            .unwrap()
            .value;
        (temp, store, account, drafts_box)
    }

    /// An account with a Drafts folder: its id and the folder's.
    fn account_with_drafts(tx: &Transaction<'_>, email: &str) -> Result<(String, i64), StoreError> {
        let id = accounts::new_id();
        let server = Server {
            host: "h".into(),
            port: 993,
            security: Security::Tls,
        };
        accounts::insert(
            tx,
            NewAccount {
                id: id.clone(),
                display_name: "Ada".into(),
                email: email.into(),
                auth_kind: AuthKind::Password,
                username: "ada".into(),
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
                remote_name: b"Drafts".to_vec(),
                name: "Drafts".into(),
                delimiter: Some('/'),
                role: Some(Role::Drafts),
                selectable: true,
                attributes: vec![],
            }],
        )?;
        let folder = mailboxes::list(tx, &id)?[0].id;
        Ok((id, folder))
    }

    fn content(subject: &str) -> Content {
        Content {
            subject: subject.into(),
            body: "Half written".into(),
            ..Content::default()
        }
    }

    #[tokio::test]
    async fn copies_follow_their_drafts_and_go_with_them() {
        let (_temp, store, account, folder) = fixture().await;
        let a = account.clone();
        let draft = store
            .write(move |tx| drafts::create(tx, &a, &content("Plans"), 1_000))
            .await
            .unwrap()
            .value;
        // Not while it is being written; then due.
        assert!(store.read(|c| due(c, 3_000, 5_000, 10)).await.unwrap().is_empty());
        let found = store.read(|c| due(c, 6_000, 5_000, 10)).await.unwrap();
        assert_eq!((found[0].draft_id, found[0].version), (draft, 1_000));

        // One Message-ID for all its copies.
        let first = store
            .write(move |tx| message_id(tx, draft, || "d1@example.test".into()))
            .await
            .unwrap()
            .value;
        let again = store
            .write(move |tx| message_id(tx, draft, || "other@example.test".into()))
            .await
            .unwrap()
            .value;
        assert_eq!(
            (first.as_deref(), again.as_deref()),
            (Some("d1@example.test"), Some("d1@example.test"))
        );

        // Uploaded as UID 7: up to date. A newer copy (UID 8) replaces it, and 7 is to delete.
        let a = account.clone();
        assert!(
            store
                .write(move |tx| uploaded(tx, &a, draft, 1_000, folder, 7))
                .await
                .unwrap()
                .value
        );
        assert!(store.read(|c| due(c, 99_000, 5_000, 10)).await.unwrap().is_empty());
        let a = account.clone();
        store
            .write(move |tx| uploaded(tx, &a, draft, 1_000, folder, 8))
            .await
            .unwrap();
        let queued = store.read(copies_to_delete).await.unwrap();
        assert_eq!(queued.iter().map(|c| c.uid).collect::<Vec<_>>(), [7]);
        let ids: Vec<i64> = queued.iter().map(|c| c.id).collect();
        store.write(move |tx| copies_deleted(tx, &ids)).await.unwrap();

        // Sent or discarded here: its copy is to delete.
        store
            .write(move |tx| drafts::delete(tx, draft).map(drop))
            .await
            .unwrap();
        assert_eq!(store.read(copies_to_delete).await.unwrap()[0].uid, 8);
        // A copy made for a draft that went meanwhile is deleted too.
        let a = account.clone();
        assert!(
            !store
                .write(move |tx| uploaded(tx, &a, draft, 1_000, folder, 9))
                .await
                .unwrap()
                .value
        );
        assert_eq!(store.read(copies_to_delete).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_draft_moved_to_another_account_leaves_its_old_copy_to_delete() {
        let (_temp, store, ada, ada_drafts) = fixture().await;
        let a = ada.clone();
        let (draft, bea, bea_drafts) = store
            .write(move |tx| {
                let draft = drafts::create(tx, &a, &content("Plans"), 1_000)?;
                uploaded(tx, &a, draft, 1_000, ada_drafts, 7)?;
                message_id(tx, draft, || "d1@example.test".into())?;
                let (bea, bea_drafts) = account_with_drafts(tx, "bea@example.test")?;
                let from = crate::identities::default_for(tx, &bea)?
                    .expect("an account has an address")
                    .id;
                drafts::move_to(tx, draft, &bea, from, "Half written", 2_000)?;
                Ok((draft, bea, bea_drafts))
            })
            .await
            .unwrap()
            .value;

        // Ada's copy is to go, from Ada's server; Bea's Drafts folder is due one.
        let queued = store.read(copies_to_delete).await.unwrap();
        assert_eq!(
            queued
                .iter()
                .map(|c| (c.account_id.as_str(), c.mailbox_id, c.uid))
                .collect::<Vec<_>>(),
            [(ada.as_str(), ada_drafts, 7)]
        );
        let found = store.read(|c| due(c, 99_000, 5_000, 10)).await.unwrap();
        assert_eq!((found[0].draft_id, found[0].account_id.as_str()), (draft, bea.as_str()));
        let moved = store.read(move |c| drafts::get(c, draft)).await.unwrap().unwrap();
        assert_eq!(moved.account_id, bea);

        // An upload to Ada's server that finishes after the move is not kept,
        // and that copy goes too, from Ada's server. Bea's upload is kept.
        let a = ada.clone();
        assert!(
            !store
                .write(move |tx| uploaded(tx, &a, draft, 1_000, ada_drafts, 8))
                .await
                .unwrap()
                .value
        );
        let b = bea.clone();
        assert!(
            store
                .write(move |tx| uploaded(tx, &b, draft, 2_000, bea_drafts, 1))
                .await
                .unwrap()
                .value
        );
        let queued = store.read(copies_to_delete).await.unwrap();
        assert_eq!(
            queued
                .iter()
                .map(|c| (c.account_id.as_str(), c.uid))
                .collect::<Vec<_>>(),
            [(ada.as_str(), 7), (ada.as_str(), 8)]
        );
        let fresh = store
            .write(move |tx| message_id(tx, draft, || "d2@example.test".into()))
            .await
            .unwrap()
            .value;
        assert_eq!(
            fresh.as_deref(),
            Some("d2@example.test"),
            "a Message-ID of its own on the new server"
        );
    }

    #[tokio::test]
    async fn a_copy_gone_from_the_server_takes_an_unchanged_draft_with_it() {
        let (_temp, store, account, folder) = fixture().await;
        let a = account.clone();
        let (unchanged, changed) = store
            .write(move |tx| {
                let unchanged = drafts::create(tx, &a, &content("Sent elsewhere"), 1_000)?;
                let changed = drafts::create(tx, &a, &content("Still writing"), 1_000)?;
                uploaded(tx, &a, unchanged, 1_000, folder, 3)?;
                uploaded(tx, &a, changed, 1_000, folder, 4)?;
                // The second was changed here since.
                tx.execute("UPDATE drafts SET updated_at = 2000 WHERE id = ?1", [changed])?;
                Ok((unchanged, changed))
            })
            .await
            .unwrap()
            .value;
        // The sync has not looked that far yet: nothing to settle.
        assert!(!store.read(unsettled).await.unwrap());
        store
            .write(move |tx| Ok(tx.execute("UPDATE mailboxes SET uid_next = 10 WHERE id = ?1", [folder])?))
            .await
            .unwrap();
        assert!(store.read(unsettled).await.unwrap());
        assert_eq!(store.write(settle).await.unwrap().value, 1);
        assert!(store.read(move |c| drafts::get(c, unchanged)).await.unwrap().is_none());
        let kept = store.read(move |c| drafts::get(c, changed)).await.unwrap();
        assert!(kept.is_some(), "a changed draft stays, to be uploaded again");
        assert_eq!(
            store.read(|c| due(c, 99_000, 5_000, 10)).await.unwrap()[0].draft_id,
            changed
        );
        assert!(
            store.read(copies_to_delete).await.unwrap().is_empty(),
            "nothing to delete: they are gone"
        );
    }

    #[tokio::test]
    async fn stray_copies_and_drafts_written_elsewhere_are_told_apart() {
        let (_temp, store, account, folder) = fixture().await;
        let a = account.clone();
        let draft = store
            .write(move |tx| {
                let draft = drafts::create(tx, &a, &content("Mine"), 1_000)?;
                message_id(tx, draft, || "mine@example.test".into())?;
                uploaded(tx, &a, draft, 1_000, folder, 2)?;
                Ok(draft)
            })
            .await
            .unwrap()
            .value;
        let copy = |uid: u32, id: &str| NewMessage {
            uid,
            modseq: None,
            flags: MessageFlags::default(),
            received_at: Some(1_000),
            size: Some(10),
            headers: Headers {
                subject: id.into(),
                message_id: Some(id.into()),
                ..Headers::default()
            },
        };
        // UID 1: an older copy of mine a crash left; 2: the current one; 3: from webmail.
        let arriving = vec![
            copy(1, "mine@example.test"),
            copy(2, "mine@example.test"),
            copy(3, "web@example.test"),
        ];
        store
            .write(move |tx| messages::upsert(tx, folder, &arriving))
            .await
            .unwrap();
        let a = account.clone();
        let elsewhere = store
            .read(move |c| messages::drafts_elsewhere(c, Some(&a)))
            .await
            .unwrap();
        assert_eq!(elsewhere.iter().map(|m| m.uid).collect::<Vec<_>>(), [3]);
        store.write(settle).await.unwrap();
        let queued = store.read(copies_to_delete).await.unwrap();
        assert_eq!(queued.iter().map(|c| c.uid).collect::<Vec<_>>(), [1], "only the stray");

        // Picking up the webmail draft makes it one of ours.
        let a = account.clone();
        store
            .write(move |tx| adopt(tx, &a, &content("From the web"), (folder, 3, "web@example.test"), 5_000))
            .await
            .unwrap();
        let a = account.clone();
        assert!(
            store
                .read(move |c| messages::drafts_elsewhere(c, Some(&a)))
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            store.read(|c| due(c, 99_000, 5_000, 10)).await.unwrap().is_empty(),
            "nothing to upload"
        );
        let _ = draft;
    }
}
