//! New mail to tell the person of (plan §5.12). Sync records what came into
//! an inbox since its last look; the app shows what the person's choice for
//! the account lets through (all, People and Priority, Priority only, or
//! none), then marks it shown. Never a folder's first look, so adding an
//! account does not tell of its whole inbox.

use crate::StoreError;
use rusqlite::{Connection, Transaction, params};

/// Arrivals are kept this long, shown or not.
const KEPT_MS: i64 = 24 * 3600 * 1000;

/// The messages of `uids` in `mailbox_id` came in: those still unread and
/// in the folder (not sent on to Junk) are new mail. Returns how many.
pub fn add(
    tx: &Transaction<'_>,
    mailbox_id: i64,
    account_id: &str,
    uids: &[u32],
    now: i64,
) -> Result<usize, StoreError> {
    tx.execute("DELETE FROM arrivals WHERE arrived_at < ?1", [now - KEPT_MS])?;
    if uids.is_empty() {
        return Ok(0);
    }
    let uids = serde_json::to_string(uids).expect("numbers always serialize");
    Ok(tx.execute(
        "INSERT INTO arrivals (message_id, account_id, arrived_at) \
         SELECT id, ?2, ?4 FROM messages \
         WHERE mailbox_id = ?1 AND uid IN (SELECT value FROM json_each(?3)) \
         AND seen = 0 AND deleted = 0 AND moving_to IS NULL ORDER BY uid",
        params![mailbox_id, account_id, uids, now],
    )?)
}

/// New mail not shown yet, oldest first: (arrival id, message id). Mail read
/// or moved meanwhile is left out.
pub fn unshown(conn: &Connection, limit: u32) -> Result<Vec<(i64, i64)>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT a.id, a.message_id FROM arrivals a JOIN messages m ON m.id = a.message_id \
         WHERE a.shown = 0 AND m.seen = 0 AND m.deleted = 0 AND m.moving_to IS NULL \
         ORDER BY a.id LIMIT ?1",
    )?;
    let rows = statement.query_map([limit], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Everything up to `up_to` was looked at (shown, or let through by nothing).
pub fn mark_shown(tx: &Transaction<'_>, up_to: i64) -> Result<usize, StoreError> {
    Ok(tx.execute("UPDATE arrivals SET shown = 1 WHERE id <= ?1 AND shown = 0", [up_to])?)
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

    fn mail(uid: u32, seen: bool) -> NewMessage {
        NewMessage {
            uid,
            modseq: None,
            flags: MessageFlags {
                seen,
                ..MessageFlags::default()
            },
            received_at: Some(1_000 + i64::from(uid)),
            size: Some(100),
            headers: Headers {
                subject: format!("Hello {uid}"),
                message_id: Some(format!("m{uid}@example.org")),
                ..Headers::default()
            },
        }
    }

    #[tokio::test]
    async fn unread_mail_that_came_in_until_it_is_shown() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let inbox = store
            .write(|tx| {
                let server = Server {
                    host: "h".into(),
                    port: 993,
                    security: Security::Tls,
                };
                accounts::insert(
                    tx,
                    NewAccount {
                        id: "a1".into(),
                        display_name: "Me".into(),
                        email: "me@example.org".into(),
                        auth_kind: AuthKind::Password,
                        username: "me".into(),
                        secret_key: "k".into(),
                        imap: server.clone(),
                        smtp: server,
                        imap_capabilities: vec![],
                    },
                )?;
                mailboxes::reconcile(
                    tx,
                    "a1",
                    &[RemoteMailbox {
                        remote_name: b"INBOX".to_vec(),
                        name: "INBOX".into(),
                        delimiter: Some('/'),
                        role: Some(Role::Inbox),
                        selectable: true,
                        attributes: vec![],
                    }],
                )?;
                let inbox = mailboxes::list(tx, "a1")?[0].id;
                messages::upsert(tx, inbox, &[mail(1, false), mail(2, true), mail(3, false)])?;
                Ok(inbox)
            })
            .await
            .unwrap()
            .value;
        let added = store
            .write(move |tx| add(tx, inbox, "a1", &[1, 2, 3], 10_000))
            .await
            .unwrap()
            .value;
        assert_eq!(added, 2, "read elsewhere already: not new");
        let waiting = store.read(|conn| unshown(conn, 10)).await.unwrap();
        assert_eq!(waiting.len(), 2);

        // Read before it was told of: left out.
        store
            .write(move |tx| Ok(tx.execute("UPDATE messages SET seen = 1 WHERE uid = 3", [])?))
            .await
            .unwrap();
        let waiting = store.read(|conn| unshown(conn, 10)).await.unwrap();
        assert_eq!(waiting.len(), 1);

        let up_to = waiting[0].0;
        store.write(move |tx| mark_shown(tx, up_to)).await.unwrap();
        assert!(store.read(|conn| unshown(conn, 10)).await.unwrap().is_empty());

        // A day later, they are gone.
        store
            .write(move |tx| add(tx, inbox, "a1", &[], 10_000 + KEPT_MS + 1))
            .await
            .unwrap();
        let left: i64 = store
            .read(|conn| Ok(conn.query_row("SELECT count(*) FROM arrivals", [], |row| row.get(0))?))
            .await
            .unwrap();
        assert_eq!(left, 0);
    }
}
