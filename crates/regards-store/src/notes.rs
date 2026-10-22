//! Notes about the people an account writes with (ADR-0020): one fact each,
//! by the person or an agent, with the message it was drawn from when one
//! was given. Kept per account and address (lower case), like `people`, so
//! an agent allowed into one account never sees what another knows.

use crate::StoreError;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

/// Characters a note may have at most.
pub const TEXT_MAX: usize = 500;
/// Notes kept at most about one person in one account.
pub const PER_PERSON_MAX: u32 = 100;

/// Who wrote a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Author {
    Person,
    Agent,
}

impl Author {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Agent => "agent",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "agent" => Self::Agent,
            _ => Self::Person,
        }
    }
}

/// The message a note was drawn from: by Message-ID, subject and sender, so
/// it survives the message moving or going.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Source {
    pub message_id: Option<String>,
    pub subject: Option<String>,
    pub from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNote {
    pub account_id: String,
    /// The person the note is about.
    pub address: String,
    pub text: String,
    pub author: Author,
    /// For a note an agent writes: its session.
    pub session: Option<String>,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: i64,
    pub account_id: String,
    pub address: String,
    pub text: String,
    pub author: Author,
    pub session: Option<String>,
    pub source: Source,
    pub created_at: i64,
    pub updated_at: i64,
}

/// An address as notes are kept by.
pub fn address_key(address: &str) -> String {
    address.trim().to_lowercase()
}

/// A note's text as kept: trimmed, not empty, not too long.
pub fn checked_text(text: &str) -> Result<String, StoreError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(StoreError::Conflict("a note needs some text".into()));
    }
    if text.chars().count() > TEXT_MAX {
        return Err(StoreError::Conflict(format!(
            "a note can have at most {TEXT_MAX} characters"
        )));
    }
    Ok(text.to_owned())
}

const COLUMNS: &str = "id, account_id, address, text, author, session, source_message_id, source_subject, \
    source_from, created_at, updated_at";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Note> {
    let author: String = row.get(4)?;
    Ok(Note {
        id: row.get(0)?,
        account_id: row.get(1)?,
        address: row.get(2)?,
        text: row.get(3)?,
        author: Author::parse(&author),
        session: row.get(5)?,
        source: Source {
            message_id: row.get(6)?,
            subject: row.get(7)?,
            from: row.get(8)?,
        },
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

/// Keep a note. Refused when its text is empty or too long, or when the
/// person already has [`PER_PERSON_MAX`] notes in that account.
pub fn add(tx: &Transaction<'_>, note: &NewNote, now: i64) -> Result<i64, StoreError> {
    let text = checked_text(&note.text)?;
    let address = address_key(&note.address);
    if count_about(tx, &note.account_id, &address)? >= PER_PERSON_MAX {
        return Err(StoreError::Conflict(format!(
            "{address} already has {PER_PERSON_MAX} notes; remove some first"
        )));
    }
    tx.prepare_cached(
        "INSERT INTO notes (account_id, address, text, author, session, source_message_id, source_subject, \
         source_from, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
    )?
    .execute(params![
        note.account_id,
        address,
        text,
        note.author.as_str(),
        note.session,
        note.source.message_id,
        note.source.subject,
        note.source.from,
        now,
    ])?;
    Ok(tx.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Note>, StoreError> {
    Ok(conn
        .query_row(&format!("SELECT {COLUMNS} FROM notes WHERE id = ?1"), [id], from_row)
        .optional()?)
}

/// The latest `limit` notes about `address` in an account, oldest first.
pub fn about(conn: &Connection, account_id: &str, address: &str, limit: u32) -> Result<Vec<Note>, StoreError> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM (SELECT * FROM notes WHERE account_id = ?1 AND address = ?2 \
         ORDER BY created_at DESC, id DESC LIMIT ?3) ORDER BY created_at, id"
    ))?;
    let rows = statement.query_map(params![account_id, address_key(address), limit], from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// How many notes there are about `address` in an account.
pub fn count_about(conn: &Connection, account_id: &str, address: &str) -> Result<u32, StoreError> {
    Ok(conn.query_row(
        "SELECT count(*) FROM notes WHERE account_id = ?1 AND address = ?2",
        params![account_id, address_key(address)],
        |row| row.get(0),
    )?)
}

/// How many notes there are in all, and about how many people.
pub fn totals(conn: &Connection) -> Result<(u32, u32), StoreError> {
    Ok(conn.query_row(
        "SELECT count(*), count(DISTINCT account_id || ' ' || address) FROM notes",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?)
}

/// The person changes a note's text: it is theirs from then on. `false` if
/// there is no such note.
pub fn set_text(tx: &Transaction<'_>, id: i64, text: &str, now: i64) -> Result<bool, StoreError> {
    let text = checked_text(text)?;
    Ok(tx.execute(
        "UPDATE notes SET text = ?2, author = 'person', session = NULL, updated_at = ?3 WHERE id = ?1",
        params![id, text, now],
    )? == 1)
}

/// Remove a note; what it was, for undo, or `None` if there is none.
pub fn remove(tx: &Transaction<'_>, id: i64) -> Result<Option<Note>, StoreError> {
    let note = get(tx, id)?;
    if note.is_some() {
        tx.execute("DELETE FROM notes WHERE id = ?1", [id])?;
    }
    Ok(note)
}

/// Put a removed note back as it was (undo). `false` if its account is gone
/// or its id taken.
pub fn restore(tx: &Transaction<'_>, note: &Note) -> Result<bool, StoreError> {
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO notes (id, account_id, address, text, author, session, source_message_id, \
         source_subject, source_from, created_at, updated_at) \
         SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11 WHERE EXISTS (SELECT 1 FROM accounts WHERE id = ?2)",
        params![
            note.id,
            note.account_id,
            note.address,
            note.text,
            note.author.as_str(),
            note.session,
            note.source.message_id,
            note.source.subject,
            note.source.from,
            note.created_at,
            note.updated_at,
        ],
    )?;
    Ok(inserted == 1)
}

/// Remove every note, in every account; how many there were.
pub fn forget_all(tx: &Transaction<'_>) -> Result<usize, StoreError> {
    Ok(tx.execute("DELETE FROM notes", [])?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use regards_core::paths::DataDirs;

    async fn account(store: &Store, email: &str) -> String {
        let id = accounts::new_id();
        let (a, email) = (id.clone(), email.to_owned());
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
                        id: a,
                        display_name: "Ada".into(),
                        email,
                        auth_kind: AuthKind::Password,
                        username: "ada".into(),
                        secret_key: "k".into(),
                        imap: server.clone(),
                        smtp: server,
                        imap_capabilities: vec![],
                    },
                )
            })
            .await
            .unwrap();
        id
    }

    fn new(account_id: &str, address: &str, text: &str, author: Author) -> NewNote {
        NewNote {
            account_id: account_id.into(),
            address: address.into(),
            text: text.into(),
            author,
            session: (author == Author::Agent).then(|| "s1".into()),
            source: Source::default(),
        }
    }

    #[tokio::test]
    async fn notes_are_kept_per_account_and_person_and_can_be_changed_and_undone() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let (work, home) = (
            account(&store, "ada@work.test").await,
            account(&store, "ada@home.test").await,
        );

        let mut drawn = new(
            &work,
            " Anna@Client.test ",
            "Prefers calls before 10:00.",
            Author::Agent,
        );
        drawn.source = Source {
            message_id: Some("<m1@client.test>".into()),
            subject: Some("Renouvellement 2027".into()),
            from: Some("anna@client.test".into()),
        };
        let first = new(&work, "anna@client.test", "  Decides on renewals.  ", Author::Person);
        let other = new(&home, "anna@client.test", "A friend from school.", Author::Person);
        let (a, b, _) = store
            .write(move |tx| Ok((add(tx, &first, 10)?, add(tx, &drawn, 20)?, add(tx, &other, 30)?)))
            .await
            .unwrap()
            .value;

        let w = work.clone();
        let found = store.read(move |c| about(c, &w, "ANNA@client.test", 20)).await.unwrap();
        assert_eq!(
            found
                .iter()
                .map(|n| (n.id, n.text.as_str(), n.author))
                .collect::<Vec<_>>(),
            [
                (a, "Decides on renewals.", Author::Person),
                (b, "Prefers calls before 10:00.", Author::Agent)
            ],
            "oldest first, trimmed, only this account's"
        );
        assert_eq!(found[1].address, "anna@client.test");
        assert_eq!(found[1].session.as_deref(), Some("s1"));
        assert_eq!(found[1].source.subject.as_deref(), Some("Renouvellement 2027"));
        let w = work.clone();
        let latest = store.read(move |c| about(c, &w, "anna@client.test", 1)).await.unwrap();
        assert_eq!(
            latest.iter().map(|n| n.id).collect::<Vec<_>>(),
            [b],
            "the latest, when limited"
        );
        assert_eq!(store.read(totals).await.unwrap(), (3, 2), "two people: one per account");

        // The person edits an agent's note: it is theirs from then on.
        assert!(
            store
                .write(move |tx| set_text(tx, b, "Calls before 10:00 only.", 40))
                .await
                .unwrap()
                .value
        );
        let edited = store.read(move |c| get(c, b)).await.unwrap().unwrap();
        assert_eq!(
            (edited.author, edited.session, edited.updated_at),
            (Author::Person, None, 40)
        );

        // Removed, then put back as it was.
        let removed = store.write(move |tx| remove(tx, a)).await.unwrap().value.unwrap();
        assert!(store.read(move |c| get(c, a)).await.unwrap().is_none());
        let back = removed.clone();
        assert!(store.write(move |tx| restore(tx, &back)).await.unwrap().value);
        assert_eq!(store.read(move |c| get(c, a)).await.unwrap(), Some(removed));

        assert_eq!(store.write(forget_all).await.unwrap().value, 3);
        assert_eq!(store.read(totals).await.unwrap(), (0, 0));
    }

    #[tokio::test]
    async fn empty_long_and_too_many_notes_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let work = account(&store, "ada@work.test").await;

        for text in ["   ".to_owned(), "x".repeat(TEXT_MAX + 1)] {
            let note = new(&work, "bob@x.test", &text, Author::Person);
            assert!(matches!(
                store.write(move |tx| add(tx, &note, 1)).await,
                Err(StoreError::Conflict(_))
            ));
        }
        let w = work.clone();
        store
            .write(move |tx| {
                for i in 0..PER_PERSON_MAX {
                    add(
                        tx,
                        &new(&w, "bob@x.test", &format!("Fact {i}"), Author::Person),
                        i64::from(i),
                    )?;
                }
                Ok(())
            })
            .await
            .unwrap();
        let note = new(&work, "Bob@X.test", "One more.", Author::Agent);
        assert!(matches!(
            store.write(move |tx| add(tx, &note, 999)).await,
            Err(StoreError::Conflict(_))
        ));

        // Removing the account removes its notes.
        let w = work.clone();
        store.write(move |tx| accounts::delete(tx, &w)).await.unwrap();
        assert_eq!(store.read(totals).await.unwrap(), (0, 0));
    }
}
