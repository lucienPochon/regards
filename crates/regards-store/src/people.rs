//! The people an account writes with (migration 0032), for suggesting
//! recipients as one types. A trigger counts every stored message: who
//! wrote to the account, and whom it wrote to.
//!
//! Ranked by how well they match what is typed, then by how much the account
//! deals with them: writing to someone counts three times receiving from
//! them, and it all fades over a month or so. Addresses nobody answers
//! (no-reply, notifications, bounces) are left out unless the account wrote
//! to them; blocked senders always are, and those one asked to forget, until
//! there is new mail with them (migration 0035).

use crate::StoreError;
use regards_core::search::fold;
use rusqlite::{Connection, OptionalExtension, params};

/// A month, in ms: how fast what is past counts less.
const FADE_MS: f64 = 30.0 * 24.0 * 3600.0 * 1000.0;

/// Someone to suggest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub name: Option<String>,
    pub address: String,
}

/// Up to `limit` people for what is typed, best first; of `accounts` only,
/// or every account (the same address counted once). Accents and case do
/// not count; each typed word may start a word of the name, a part of the
/// address (`lena.okafor`, `okafor`) or of its domain (`example`), in any
/// order. Those whose name or address starts with what is typed come first,
/// those found only by their domain last.
pub fn suggest(
    conn: &Connection,
    typed: &str,
    accounts: Option<&[String]>,
    limit: u32,
    now: i64,
) -> Result<Vec<Person>, StoreError> {
    let typed = fold(typed.trim());
    let words: Vec<&str> = typed.split_whitespace().collect();
    let accounts = accounts.map(|ids| serde_json::to_string(ids).expect("strings always serialize"));
    let mut statement = conn.prepare_cached(
        "SELECT address, MAX(name), SUM(score) AS total FROM ( \
             SELECT p.address, p.name, (3.0 * p.sent + p.received) / (1.0 + MAX(0, ?2 - p.last_at) / ?3) AS score \
             FROM people p \
             WHERE (?1 IS NULL OR p.account_id IN (SELECT value FROM json_each(?1))) \
             AND (p.forgotten_at IS NULL OR p.last_at > p.forgotten_at) \
             AND NOT EXISTS (SELECT 1 FROM senders s WHERE s.account_id = p.account_id AND s.blocked = 1 \
                 AND (s.address = p.address OR s.address = substr(p.address, instr(p.address, '@')))) \
             AND (p.sent > 0 OR NOT (p.address LIKE '%noreply%' OR p.address LIKE '%no-reply%' \
                  OR p.address LIKE '%donotreply%' OR p.address LIKE '%do-not-reply%' \
                  OR p.address LIKE 'mailer-daemon@%' OR p.address LIKE 'postmaster@%' \
                  OR p.address LIKE 'notifications@%' OR p.address LIKE 'notification@%' \
                  OR p.address LIKE 'bounce%' OR p.address LIKE '%@bounce%'))) \
         GROUP BY address",
    )?;
    let rows = statement.query_map(params![accounts, now, FADE_MS], |row| {
        let person = Person {
            address: row.get(0)?,
            name: row.get(1)?,
        };
        Ok((person, row.get::<_, f64>(2)?))
    })?;
    let mut found = Vec::new();
    for row in rows {
        let (person, total) = row?;
        if let Some(fit) = fit(&typed, &words, &person) {
            found.push((fit, total, person));
        }
    }
    found.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.1.total_cmp(&a.1))
            .then_with(|| a.2.address.cmp(&b.2.address))
    });
    found.truncate(limit as usize);
    Ok(found.into_iter().map(|(_, _, person)| person).collect())
}

/// How well `person` matches what is typed (folded, and its words): 0, the
/// name or address starts with it; 1, each word starts a word of the name
/// or a part of the address; 2, the same with the domain's parts, or the
/// address holds it as typed (`okafor@ex`). `None`: not at all.
fn fit(typed: &str, words: &[&str], person: &Person) -> Option<u8> {
    let name = person.name.as_deref().map(fold).unwrap_or_default();
    let address = fold(&person.address);
    if typed.is_empty() || name.starts_with(typed) || address.starts_with(typed) {
        return Some(0);
    }
    let parts = |text: &str| -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let starts = |parts: &[String]| words.iter().all(|word| parts.iter().any(|part| part.starts_with(word)));
    let (local, domain) = address.split_once('@').unwrap_or((&address, ""));
    let mut near = parts(&name);
    near.extend(parts(local));
    if starts(&near) {
        return Some(1);
    }
    near.extend(parts(domain));
    (starts(&near) || (typed.contains(['@', '.']) && address.contains(typed))).then_some(2)
}

/// Not to be suggested any more (a mistyped address, say), by `accounts` or
/// every account, until there is new mail with them. How many accounts
/// knew them.
pub fn forget(conn: &Connection, address: &str, accounts: Option<&[String]>, now: i64) -> Result<usize, StoreError> {
    let accounts = accounts.map(|ids| serde_json::to_string(ids).expect("strings always serialize"));
    Ok(conn.execute(
        "UPDATE people SET forgotten_at = MAX(?3, last_at) \
         WHERE address = lower(trim(?1)) AND (?2 IS NULL OR account_id IN (SELECT value FROM json_each(?2)))",
        params![address, accounts, now],
    )?)
}

/// What an account has had to do with one person (ADR-0020): the name they
/// last gave, how many messages each side wrote, and when last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dealings {
    pub name: Option<String>,
    /// Written by the account to them.
    pub sent: u32,
    /// Written by them to the account.
    pub received: u32,
    pub last_at: i64,
}

/// `None` if neither side ever wrote to the other.
pub fn dealings(conn: &Connection, account_id: &str, address: &str) -> Result<Option<Dealings>, StoreError> {
    Ok(conn
        .prepare_cached("SELECT name, sent, received, last_at FROM people WHERE account_id = ?1 AND address = ?2")?
        .query_row(params![account_id, address.trim().to_lowercase()], |row| {
            Ok(Dealings {
                name: row.get(0)?,
                sent: row.get(1)?,
                received: row.get(2)?,
                last_at: row.get(3)?,
            })
        })
        .optional()?)
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

    const DAY: i64 = 86_400_000;
    const NOW: i64 = 1_000 * DAY;
    const ME: (&str, &str) = ("Me", "me@example.org");

    /// An account, a1 (me@example.org), with `inbox` in its inbox, and
    /// bad.example blocked.
    async fn store_with(inbox: Vec<NewMessage>) -> (tempfile::TempDir, Store) {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
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
                let mailbox = mailboxes::list(tx, "a1")?[0].id;
                messages::upsert(tx, mailbox, &inbox)?;
                crate::smart::set_blocked(tx, "a1", "@bad.example", true)
            })
            .await
            .unwrap();
        (temp, store)
    }

    /// The addresses suggested for `typed`, from every account.
    async fn suggested(store: &Store, typed: &'static str) -> Vec<String> {
        store
            .read(move |conn| suggest(conn, typed, None, 10, NOW))
            .await
            .unwrap()
            .into_iter()
            .map(|person| person.address)
            .collect()
    }

    #[tokio::test]
    async fn those_written_with_most_come_first_as_one_types() {
        let (day, now) = (DAY, NOW);
        let me = || who(ME.0, ME.1);
        let (_temp, store) = store_with(vec![
            // Lena wrote twice; the account wrote to Bob once, recently.
            mail(1, now - 3 * day, who("Lena Okafor", "lena@example.net"), vec![me()]),
            mail(2, now - 2 * day, who("Lena Okafor", "lena@example.net"), vec![me()]),
            mail(3, now - day, me(), vec![who("Bob Smith", "Bob@Example.net"), me()]),
            mail(4, now, who("Shop", "noreply@shop.example"), vec![me()]),
            mail(5, now, who("Spammer", "lena.spam@bad.example"), vec![me()]),
            // Long ago: counts less.
            mail(6, now - 400 * day, me(), vec![who("Old Friend", "old@example.net")]),
        ])
        .await;
        let found = |typed| suggested(&store, typed);
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

        // One person, as context for an agent.
        let lena = store
            .read(|conn| dealings(conn, "a1", " Lena@Example.net "))
            .await
            .unwrap();
        assert_eq!(
            lena,
            Some(Dealings {
                name: Some("Lena Okafor".into()),
                sent: 0,
                received: 2,
                last_at: now - 2 * day,
            })
        );
        assert_eq!(
            store
                .read(|conn| dealings(conn, "a1", "nobody@example.net"))
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn accents_parts_and_domains_match_and_the_forgotten_go() {
        let (day, now) = (DAY, NOW);
        let me = || who(ME.0, ME.1);
        let (_temp, store) = store_with(vec![
            mail(
                1,
                now - day,
                me(),
                vec![who("Hélène Dupré", "helene.dupre@acme.example")],
            ),
            mail(2, now - day, me(), vec![who("Zoë Ancel", "zoe@other.example")]),
            mail(
                3,
                now - 2 * day,
                who("Jean-Pierre Martin", "jp.martin@acme.example"),
                vec![me()],
            ),
            mail(4, now - 3 * day, me(), vec![who("", "lena.okafor@exmaple.net")]),
        ])
        .await;
        let found = |typed| suggested(&store, typed);
        assert_eq!(
            found("hel").await,
            ["helene.dupre@acme.example"],
            "accents do not count"
        );
        assert_eq!(found("ZOË").await, ["zoe@other.example"]);
        assert_eq!(
            found("dupre hél").await,
            ["helene.dupre@acme.example"],
            "words in any order"
        );
        assert_eq!(found("pierre").await, ["jp.martin@acme.example"]);
        assert_eq!(
            found("okafor").await,
            ["lena.okafor@exmaple.net"],
            "a part of the address"
        );
        assert_eq!(found("okafor@exm").await, ["lena.okafor@exmaple.net"]);
        assert_eq!(
            found("acme").await,
            ["helene.dupre@acme.example", "jp.martin@acme.example"],
            "by the domain, the most written with first"
        );
        assert_eq!(
            found("a").await,
            [
                "zoe@other.example",
                "helene.dupre@acme.example",
                "jp.martin@acme.example"
            ],
            "a word of the name before a domain"
        );

        // A mistyped address, forgotten: gone until there is new mail with it.
        let forgotten = store
            .write(move |tx| forget(tx, " Lena.Okafor@exmaple.net", None, now))
            .await
            .unwrap();
        assert_eq!(forgotten.value, 1);
        assert!(found("okafor").await.is_empty());
        store
            .write(move |tx| {
                let mailbox = mailboxes::list(tx, "a1")?[0].id;
                let again = mail(5, now + day, me(), vec![who("", "lena.okafor@exmaple.net")]);
                messages::upsert(tx, mailbox, &[again]).map(|_| ())
            })
            .await
            .unwrap();
        assert_eq!(found("okafor").await, ["lena.okafor@exmaple.net"], "written to again");
    }
}
