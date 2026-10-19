//! The people an account writes with (migration 0032), for suggesting
//! recipients as one types. A trigger counts every stored message: who
//! wrote to the account, and whom it wrote to.
//!
//! Ranked by how much the account deals with them: writing to someone
//! counts three times receiving from them, and it all fades over a month or
//! so. Addresses nobody answers (no-reply, notifications, bounces) are left
//! out unless the account wrote to them; blocked senders always are.

use crate::StoreError;
use rusqlite::{Connection, params};

/// A month, in ms: how fast what is past counts less.
const FADE_MS: f64 = 30.0 * 24.0 * 3600.0 * 1000.0;

/// Someone to suggest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub name: Option<String>,
    pub address: String,
}

/// Up to `limit` people whose address, or a word of whose name, starts with
/// `typed`, best first; of `accounts` only, or every account (the same
/// address counted once).
pub fn suggest(
    conn: &Connection,
    typed: &str,
    accounts: Option<&[String]>,
    limit: u32,
    now: i64,
) -> Result<Vec<Person>, StoreError> {
    let typed = typed.trim().to_lowercase();
    let escaped = typed.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let accounts = accounts.map(|ids| serde_json::to_string(ids).expect("strings always serialize"));
    let mut statement = conn.prepare_cached(
        "SELECT address, MAX(name), SUM(score) AS total FROM ( \
             SELECT p.address, p.name, (3.0 * p.sent + p.received) / (1.0 + MAX(0, ?4 - p.last_at) / ?5) AS score \
             FROM people p \
             WHERE (?1 IS NULL OR p.account_id IN (SELECT value FROM json_each(?1))) \
             AND (?2 = '' OR p.address LIKE ?2 || '%' ESCAPE '\\' OR lower(p.name) LIKE ?2 || '%' ESCAPE '\\' \
                  OR lower(p.name) LIKE '% ' || ?2 || '%' ESCAPE '\\') \
             AND NOT EXISTS (SELECT 1 FROM senders s WHERE s.account_id = p.account_id AND s.blocked = 1 \
                 AND (s.address = p.address OR s.address = substr(p.address, instr(p.address, '@')))) \
             AND (p.sent > 0 OR NOT (p.address LIKE '%noreply%' OR p.address LIKE '%no-reply%' \
                  OR p.address LIKE '%donotreply%' OR p.address LIKE '%do-not-reply%' \
                  OR p.address LIKE 'mailer-daemon@%' OR p.address LIKE 'postmaster@%' \
                  OR p.address LIKE 'notifications@%' OR p.address LIKE 'notification@%' \
                  OR p.address LIKE 'bounce%' OR p.address LIKE '%@bounce%'))) \
         GROUP BY address ORDER BY total DESC, address LIMIT ?3",
    )?;
    let rows = statement.query_map(params![accounts, escaped, limit, now, FADE_MS], |row| {
        Ok(Person {
            address: row.get(0)?,
            name: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use crate::mailboxes::{self, RemoteMailbox};
    use crate::messages::{self, MessageFlags, NewMessage};
    use regards_core::headers::{Headers, Person as Who};
    use regards_core::mailbox::Role;
    use regards_core::paths::DataDirs;

    fn who(name: &str, address: &str) -> Who {
        Who {
            name: (!name.is_empty()).then(|| name.into()),
            address: Some(address.into()),
        }
    }

    fn mail(uid: u32, at: i64, from: Who, to: Vec<Who>) -> NewMessage {
        NewMessage {
            uid,
            modseq: None,
            flags: MessageFlags::default(),
            received_at: Some(at),
            size: Some(100),
            headers: Headers {
                subject: format!("m{uid}"),
                from: Some(from),
                to,
                message_id: Some(format!("m{uid}@example.org")),
                ..Headers::default()
            },
        }
    }

    #[tokio::test]
    async fn those_written_with_most_come_first_as_one_types() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let day = 86_400_000;
        let now = 1_000 * day;
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
                let me = || who("Me", "me@example.org");
                messages::upsert(
                    tx,
                    inbox,
                    &[
                        // Lena wrote twice; the account wrote to Bob once, recently.
                        mail(1, now - 3 * day, who("Lena Okafor", "lena@example.net"), vec![me()]),
                        mail(2, now - 2 * day, who("Lena Okafor", "lena@example.net"), vec![me()]),
                        mail(3, now - day, me(), vec![who("Bob Smith", "Bob@Example.net"), me()]),
                        mail(4, now, who("Shop", "noreply@shop.example"), vec![me()]),
                        mail(5, now, who("Spammer", "lena.spam@bad.example"), vec![me()]),
                        // Long ago: counts less.
                        mail(6, now - 400 * day, me(), vec![who("Old Friend", "old@example.net")]),
                    ],
                )?;
                crate::smart::set_blocked(tx, "a1", "@bad.example", true)
            })
            .await
            .unwrap();
        let found = |typed: &'static str| {
            let store = &store;
            async move {
                store
                    .read(move |conn| suggest(conn, typed, None, 10, now))
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|person| person.address)
                    .collect::<Vec<_>>()
            }
        };
        assert_eq!(
            found("").await,
            ["bob@example.net", "lena@example.net", "old@example.net"],
            "written to beats received from; no-reply, blocked and one's own left out"
        );
        assert_eq!(found("le").await, ["lena@example.net"]);
        assert_eq!(found("smi").await, ["bob@example.net"], "a word of the name");
        assert_eq!(found("OKAF").await, ["lena@example.net"]);
        assert!(found("noreply").await.is_empty());
        assert!(found("%").await.is_empty(), "typed as is, not a pattern");
        let named = store
            .read(move |conn| suggest(conn, "bob", Some(&["a1".to_owned()]), 10, now))
            .await
            .unwrap();
        assert_eq!(named[0].name.as_deref(), Some("Bob Smith"));
        assert!(
            store
                .read(move |conn| suggest(conn, "bob", Some(&["other".to_owned()]), 10, now))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
