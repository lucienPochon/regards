//! The Smart Inbox's state (plan §5.4): each message's section, and what is
//! known about each sender. The rules are [`regards_core::smart::classify`];
//! this keeps their results current:
//!
//! - **When a message is stored**, it is classified at once.
//! - **When the account writes to someone** (a message from one of its own
//!   addresses), they stop being a new sender, and their mail moves to People.
//! - **When the person chooses** a section for a sender, or makes them a
//!   priority, all of the sender's mail follows.
//! - **Mail stored before** the Smart Inbox has no traits yet: it is
//!   classified from what is known, and again once its headers are read.

use crate::StoreError;
use regards_core::smart::{self, Policy, Reason, Section, Sender};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

/// What is known about `address` (any case) for an account: its own entry,
/// and its domain's (`@example.org`) where its own says nothing. A choice
/// for the address wins over one for its domain.
pub fn sender(conn: &Connection, account_id: &str, address: &str) -> Result<Sender, StoreError> {
    let own = entry(conn, account_id, address)?;
    let domain = match smart::domain_key(address) {
        Some(key) if !address.starts_with('@') => entry(conn, account_id, &key)?,
        _ => Sender::default(),
    };
    Ok(Sender {
        policy: own.policy.or(domain.policy),
        priority: own.priority || domain.priority,
        written_to: own.written_to,
        blocked: own.blocked || domain.blocked,
    })
}

/// One entry of the senders table, by address or `@domain` key.
fn entry(conn: &Connection, account_id: &str, key: &str) -> Result<Sender, StoreError> {
    let found = conn
        .prepare_cached(
            "SELECT policy, priority, written_to_at, blocked FROM senders WHERE account_id = ?1 AND address = lower(?2)",
        )?
        .query_row(params![account_id, key], |row| {
            let policy: Option<String> = row.get(0)?;
            let written_to: Option<i64> = row.get(2)?;
            Ok(Sender {
                policy: policy.as_deref().and_then(Policy::parse),
                priority: row.get(1)?,
                written_to: written_to.is_some(),
                blocked: row.get(3)?,
            })
        })
        .optional()?;
    Ok(found.unwrap_or_default())
}

/// The messages of an account from `key`: an address, or every address of
/// an `@domain`. With `mailbox`, only those in that folder.
pub fn from_sender(
    conn: &Connection,
    account_id: &str,
    key: &str,
    mailbox: Option<i64>,
) -> Result<Vec<i64>, StoreError> {
    let (pattern, exact) = match key.strip_prefix('@') {
        Some(_) => (format!("%{}", key.to_lowercase()), false),
        None => (key.to_lowercase(), true),
    };
    let sql = format!(
        "SELECT m.id FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id \
         WHERE b.account_id = ?1 AND {} AND (?3 IS NULL OR m.mailbox_id = ?3) AND m.deleted = 0",
        if exact {
            "lower(m.from_address) = ?2"
        } else {
            "lower(m.from_address) LIKE ?2"
        }
    );
    let rows = conn
        .prepare_cached(&sql)?
        .query_map(params![account_id, pattern, mailbox], |row| row.get(0))?
        .collect::<Result<Vec<i64>, _>>()?;
    Ok(rows)
}

/// Block or unblock a sender (or `@domain`). Moving their mail is the caller's.
pub fn set_blocked(tx: &Transaction<'_>, account_id: &str, key: &str, blocked: bool) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO senders (account_id, address, blocked) VALUES (?1, lower(?2), ?3) \
         ON CONFLICT (account_id, address) DO UPDATE SET blocked = ?3",
        params![account_id, key, blocked],
    )?;
    Ok(())
}

/// How new senders' mail is shown.
pub fn screening(conn: &Connection) -> Result<smart::Screening, StoreError> {
    let value: Option<String> = conn
        .prepare_cached("SELECT value FROM settings WHERE key = 'screening'")?
        .query_row([], |row| row.get(0))
        .optional()?;
    Ok(value.as_deref().and_then(smart::Screening::parse).unwrap_or_default())
}

pub fn set_screening(tx: &Transaction<'_>, mode: smart::Screening) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO settings (key, value) VALUES ('screening', ?1) ON CONFLICT (key) DO UPDATE SET value = ?1",
        [mode.as_str()],
    )?;
    Ok(())
}

/// The account's own addresses, lowercase: its identities and its login address.
pub fn own_addresses(conn: &Connection, account_id: &str) -> Result<Vec<String>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT lower(email) FROM identities WHERE account_id = ?1 UNION SELECT lower(email) FROM accounts WHERE id = ?1",
    )?;
    let rows = statement.query_map([account_id], |row| row.get(0))?;
    Ok(rows.collect::<Result<Vec<String>, _>>()?)
}

/// A message's section and why, from its traits (none known counts as none)
/// and sender.
pub fn classify(
    conn: &Connection,
    account_id: &str,
    own: &[String],
    traits: Option<u32>,
    from: Option<&str>,
) -> Result<(Section, Reason), StoreError> {
    let from_you = from.is_some_and(|address| own.iter().any(|mine| mine.eq_ignore_ascii_case(address)));
    let known = match from {
        Some(address) => sender(conn, account_id, address)?,
        None => Sender::default(),
    };
    Ok(smart::classify(traits.unwrap_or(0), known, from_you))
}

/// Classify a stored message (by id) and record its section.
pub fn reclassify(tx: &Transaction<'_>, account_id: &str, own: &[String], id: i64) -> Result<(), StoreError> {
    let (traits, from): (Option<u32>, Option<String>) =
        tx.query_row("SELECT traits, from_address FROM messages WHERE id = ?1", [id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
    let (section, _) = classify(tx, account_id, own, traits, from.as_deref())?;
    tx.prepare_cached("UPDATE messages SET section = ?2 WHERE id = ?1 AND section IS NOT ?2")?
        .execute(params![id, section.as_str()])?;
    Ok(())
}

/// The account wrote to `addresses` at `at`: they are no longer new senders.
/// Their mail waiting as from new senders moves to People.
pub fn note_written_to(
    tx: &Transaction<'_>,
    account_id: &str,
    addresses: &[String],
    at: i64,
) -> Result<(), StoreError> {
    let mut first_time = tx.prepare_cached(
        "INSERT INTO senders (account_id, address, written_to_at) VALUES (?1, lower(?2), ?3) \
         ON CONFLICT (account_id, address) DO UPDATE SET written_to_at = ?3 WHERE written_to_at IS NULL",
    )?;
    let mut moved = tx.prepare_cached(
        "UPDATE messages SET section = 'people' WHERE section = 'new_senders' AND lower(from_address) = lower(?2) \
         AND mailbox_id IN (SELECT id FROM mailboxes WHERE account_id = ?1)",
    )?;
    for address in addresses {
        if first_time.execute(params![account_id, address, at])? > 0 {
            moved.execute(params![account_id, address])?;
        }
    }
    Ok(())
}

/// The person's choice for a sender: a section (or none, to let the rules
/// decide), and whether they are a priority. All their mail follows; returns
/// how many messages changed section.
pub fn set_sender(
    tx: &Transaction<'_>,
    account_id: &str,
    address: &str,
    policy: Option<Policy>,
    priority: bool,
) -> Result<usize, StoreError> {
    tx.execute(
        "INSERT INTO senders (account_id, address, policy, priority) VALUES (?1, lower(?2), ?3, ?4) \
         ON CONFLICT (account_id, address) DO UPDATE SET policy = ?3, priority = ?4",
        params![account_id, address, policy.map(Policy::as_str), priority],
    )?;
    let own = own_addresses(tx, account_id)?;
    let ids = from_sender(tx, account_id, address, None)?;
    let before = changes(tx)?;
    for id in &ids {
        reclassify(tx, account_id, &own, *id)?;
    }
    Ok(changes(tx)? - before)
}

fn changes(conn: &Connection) -> Result<usize, StoreError> {
    Ok(conn.query_row("SELECT total_changes()", [], |row| row.get::<_, i64>(0))? as usize)
}

/// Messages not classified yet (stored before the Smart Inbox), at most
/// `limit`: (id, account).
pub fn unclassified(conn: &Connection, limit: u32) -> Result<Vec<(i64, String)>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT m.id, b.account_id FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id \
         WHERE m.section IS NULL LIMIT ?1",
    )?;
    let rows = statement.query_map([limit], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Inbox messages whose headers were stored before the Smart Inbox read
/// them: (UID), at most `limit`, newest first.
pub fn traits_unknown(conn: &Connection, mailbox_id: i64, limit: u32) -> Result<Vec<u32>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT uid FROM messages WHERE mailbox_id = ?1 AND traits IS NULL ORDER BY uid DESC LIMIT ?2",
    )?;
    let rows = statement.query_map(params![mailbox_id, limit], |row| row.get(0))?;
    Ok(rows.collect::<Result<Vec<u32>, _>>()?)
}

/// A message's traits, read again from its headers: recorded, and its
/// section brought up to date.
pub fn set_traits(tx: &Transaction<'_>, mailbox_id: i64, uid: u32, traits: u32) -> Result<(), StoreError> {
    let found: Option<(i64, String)> = tx
        .query_row(
            "UPDATE messages SET traits = ?3 WHERE mailbox_id = ?1 AND uid = ?2 \
             RETURNING id, (SELECT account_id FROM mailboxes WHERE id = ?1)",
            params![mailbox_id, uid, traits],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((id, account_id)) = found {
        let own = own_addresses(tx, &account_id)?;
        reclassify(tx, &account_id, &own, id)?;
    }
    Ok(())
}

/// Per section: how many messages the inbox holds, and how many are unread.
/// What is set aside or snoozed is left out, as from the inbox itself.
pub fn overview(
    conn: &Connection,
    inbox_id: i64,
    account_id: &str,
    now: i64,
) -> Result<Vec<(Section, u32, u32)>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT section, COUNT(*), SUM(seen = 0) FROM messages \
         WHERE mailbox_id = ?1 AND moving_to IS NULL AND deleted = 0 AND section IS NOT NULL \
         AND NOT EXISTS (SELECT 1 FROM marks k WHERE k.account_id = ?2 AND k.message_id = messages.message_id_header \
             AND (k.aside_at IS NOT NULL OR k.snoozed_until > ?3)) \
         GROUP BY section",
    )?;
    let rows = statement.query_map(params![inbox_id, account_id, now], |row| {
        let section: String = row.get(0)?;
        Ok((section, row.get::<_, u32>(1)?, row.get::<_, u32>(2)?))
    })?;
    let mut found = Vec::new();
    for row in rows {
        let (section, total, unread) = row?;
        if let Some(section) = Section::parse(&section) {
            found.push((section, total, unread));
        }
    }
    found.sort_by_key(|(section, _, _)| Section::ALL.iter().position(|s| s == section));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use crate::mailboxes::{self, RemoteMailbox};
    use crate::messages::{self, MessageFlags, NewMessage};
    use regards_core::headers::{Headers, Person};
    use regards_core::mailbox::Role;
    use regards_core::paths::DataDirs;
    use regards_core::smart::traits;

    struct Fixture {
        _temp: tempfile::TempDir,
        store: Store,
        account: String,
        inbox: i64,
        sent: i64,
    }

    async fn fixture() -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let account = accounts::new_id();
        let id = account.clone();
        let (inbox, sent) = store
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
                        display_name: "Ada".into(),
                        email: "ada@example.test".into(),
                        auth_kind: AuthKind::Password,
                        username: "ada".into(),
                        secret_key: "k".into(),
                        imap: server.clone(),
                        smtp: server,
                        imap_capabilities: vec![],
                    },
                )?;
                let folder = |name: &str, role| RemoteMailbox {
                    remote_name: name.as_bytes().to_vec(),
                    name: name.into(),
                    delimiter: Some('/'),
                    role: Some(role),
                    selectable: true,
                    attributes: vec![],
                };
                mailboxes::reconcile(tx, &id, &[folder("INBOX", Role::Inbox), folder("Sent", Role::Sent)])?;
                let listed = mailboxes::list(tx, &id)?;
                let find = |role| listed.iter().find(|m| m.role == Some(role)).unwrap().id;
                Ok((find(Role::Inbox), find(Role::Sent)))
            })
            .await
            .unwrap()
            .value;
        Fixture {
            _temp: temp,
            store,
            account,
            inbox,
            sent,
        }
    }

    fn mail(uid: u32, from: &str, to: &str, subject: &str, traits: u32) -> NewMessage {
        let person = |address: &str| Person {
            name: None,
            address: Some(address.into()),
        };
        NewMessage {
            uid,
            modseq: None,
            flags: MessageFlags::default(),
            received_at: Some(i64::from(uid) * 1000),
            size: Some(100),
            headers: Headers {
                subject: subject.into(),
                from: Some(person(from)),
                to: vec![person(to)],
                message_id: Some(format!("{subject}@x")),
                traits,
                ..Headers::default()
            },
        }
    }

    async fn sections(f: &Fixture) -> Vec<(String, Option<String>)> {
        let inbox = f.inbox;
        f.store
            .read(move |c| {
                let mut statement =
                    c.prepare("SELECT subject, section FROM messages WHERE mailbox_id = ?1 ORDER BY uid")?;
                let rows = statement.query_map([inbox], |r| Ok((r.get(0)?, r.get(1)?)))?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .await
            .unwrap()
    }

    fn named(pairs: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
        pairs
            .iter()
            .map(|(subject, section)| ((*subject).to_owned(), Some((*section).to_owned())))
            .collect()
    }

    #[tokio::test]
    async fn mail_is_sorted_as_it_arrives_and_follows_the_choices_made_for_senders() {
        let f = fixture().await;
        let arriving = vec![
            mail(1, "lena@example.net", "ada@example.test", "hello", 0),
            mail(2, "news@shop.example", "ada@example.test", "sale", traits::LIST),
            mail(3, "no-reply@bank.example", "ada@example.test", "alert", traits::NOREPLY),
            mail(4, "bob@example.org", "ada@example.test", "quote", 0),
        ];
        let inbox = f.inbox;
        f.store
            .write(move |tx| messages::upsert(tx, inbox, &arriving))
            .await
            .unwrap();
        assert_eq!(
            sections(&f).await,
            named(&[
                ("hello", "new_senders"),
                ("sale", "newsletters"),
                ("alert", "notifications"),
                ("quote", "new_senders")
            ])
        );

        // Ada writes to Lena (her copy in Sent): Lena is no longer new.
        let sent = f.sent;
        let reply = vec![mail(1, "Ada@Example.test", "LENA@example.net", "re-hello", 0)];
        f.store
            .write(move |tx| messages::upsert(tx, sent, &reply))
            .await
            .unwrap();
        assert_eq!(sections(&f).await[0], ("hello".to_owned(), Some("people".to_owned())));
        // New mail from her is People from the start.
        let more = vec![mail(5, "lena@example.net", "ada@example.test", "again", 0)];
        f.store
            .write(move |tx| messages::upsert(tx, inbox, &more))
            .await
            .unwrap();
        assert_eq!(sections(&f).await[4].1.as_deref(), Some("people"));

        // The shop goes to People, and Bob becomes a priority.
        let a = f.account.clone();
        let changed = f
            .store
            .write(move |tx| {
                let shop = set_sender(tx, &a, "NEWS@shop.example", Some(Policy::People), false)?;
                let bob = set_sender(tx, &a, "bob@example.org", None, true)?;
                Ok((shop, bob))
            })
            .await
            .unwrap()
            .value;
        assert_eq!(changed, (1, 1));
        let now = sections(&f).await;
        assert_eq!(now[1].1.as_deref(), Some("people"));
        assert_eq!(now[3].1.as_deref(), Some("priority"));
        // ...and back to the rules: the list goes back to Newsletters.
        let a = f.account.clone();
        f.store
            .write(move |tx| set_sender(tx, &a, "news@shop.example", None, false))
            .await
            .unwrap();
        assert_eq!(sections(&f).await[1].1.as_deref(), Some("newsletters"));

        // Counts per section, the unread among them.
        let (inbox, a) = (f.inbox, f.account.clone());
        let counts = f.store.read(move |c| overview(c, inbox, &a, 0)).await.unwrap();
        assert_eq!(
            counts,
            [
                (Section::Priority, 1, 1),
                (Section::People, 2, 2),
                (Section::Notifications, 1, 1),
                (Section::Newsletters, 1, 1)
            ]
        );

        // Explaining uses the same rules, with the reason.
        let a = f.account.clone();
        let why = f
            .store
            .read(move |c| {
                let own = own_addresses(c, &a)?;
                classify(c, &a, &own, Some(traits::LIST), Some("news@shop.example"))
            })
            .await
            .unwrap();
        assert_eq!(why, (Section::Newsletters, Reason::MailingList));
    }

    #[tokio::test]
    async fn mail_stored_before_is_sorted_again_once_its_headers_are_read() {
        let f = fixture().await;
        let inbox = f.inbox;
        let old = vec![mail(1, "news@shop.example", "ada@example.test", "sale", 0)];
        f.store
            .write(move |tx| {
                messages::upsert(tx, inbox, &old)?;
                // As migrated: neither traits nor section known.
                tx.execute("UPDATE messages SET traits = NULL, section = NULL", [])?;
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(f.store.read(|c| unclassified(c, 10)).await.unwrap().len(), 1);
        assert_eq!(f.store.read(move |c| traits_unknown(c, inbox, 10)).await.unwrap(), [1]);
        f.store
            .write(move |tx| set_traits(tx, inbox, 1, traits::LIST))
            .await
            .unwrap();
        assert_eq!(sections(&f).await, named(&[("sale", "newsletters")]));
        assert!(f.store.read(|c| unclassified(c, 10)).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_domain_can_be_sorted_or_blocked_and_an_address_overrides_it() {
        let f = fixture().await;
        let inbox = f.inbox;
        let arriving = vec![
            mail(1, "news@shop.example", "ada@example.test", "sale", 0),
            mail(2, "help@Shop.example", "ada@example.test", "ticket", 0),
            mail(3, "bob@example.org", "ada@example.test", "quote", 0),
        ];
        f.store
            .write(move |tx| messages::upsert(tx, inbox, &arriving))
            .await
            .unwrap();

        // The whole shop is a newsletter, except its help desk.
        let a = f.account.clone();
        f.store
            .write(move |tx| {
                set_sender(tx, &a, "@shop.example", Some(Policy::Newsletters), false)?;
                set_sender(tx, &a, "help@shop.example", Some(Policy::People), false)
            })
            .await
            .unwrap();
        assert_eq!(
            sections(&f).await,
            named(&[("sale", "newsletters"), ("ticket", "people"), ("quote", "new_senders")])
        );

        // Blocking the domain: both addresses count as blocked; Bob does not.
        let a = f.account.clone();
        let (shop, help, bob, from_shop) = f
            .store
            .write(move |tx| {
                set_blocked(tx, &a, "@SHOP.example", true)?;
                Ok((
                    sender(tx, &a, "news@shop.example")?.blocked,
                    sender(tx, &a, "help@shop.example")?.blocked,
                    sender(tx, &a, "bob@example.org")?.blocked,
                    from_sender(tx, &a, "@shop.example", Some(inbox))?.len(),
                ))
            })
            .await
            .unwrap()
            .value;
        assert_eq!((shop, help, bob, from_shop), (true, true, false, 2));
        let a = f.account.clone();
        let exact = f
            .store
            .read(move |c| from_sender(c, &a, "NEWS@shop.example", None))
            .await
            .unwrap();
        assert_eq!(exact.len(), 1);

        // The screening setting: section unless chosen otherwise.
        assert_eq!(f.store.read(screening).await.unwrap(), smart::Screening::Section);
        f.store
            .write(|tx| set_screening(tx, smart::Screening::Hold))
            .await
            .unwrap();
        assert_eq!(f.store.read(screening).await.unwrap(), smart::Screening::Hold);
    }
}
