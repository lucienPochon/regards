//! Identities (plan §5.9): the names and addresses an account sends as, its
//! own and its aliases, each with a plain-text signature and a voice
//! (ADR-0020). Exactly one per account is the default; it cannot be
//! removed, only replaced as default.

use crate::StoreError;
use rusqlite::{Connection, ErrorCode, OptionalExtension, Row, Transaction, params};

/// Characters a voice may have at most.
pub const VOICE_MAX: usize = 2_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub id: i64,
    pub account_id: String,
    pub name: String,
    pub email: String,
    pub signature: String,
    /// How agents should write as this address; kept while switched off.
    pub voice: String,
    pub voice_on: bool,
    pub is_default: bool,
}

impl Identity {
    /// The voice agents are given: none while it is off or empty.
    pub fn active_voice(&self) -> Option<&str> {
        let voice = self.voice.trim();
        (self.voice_on && !voice.is_empty()).then_some(voice)
    }
}

/// What can be set on an identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fields {
    pub name: String,
    pub email: String,
    pub signature: String,
}

const COLUMNS: &str = "id, account_id, name, email, signature, voice, voice_on, is_default";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Identity> {
    Ok(Identity {
        id: row.get(0)?,
        account_id: row.get(1)?,
        name: row.get(2)?,
        email: row.get(3)?,
        signature: row.get(4)?,
        voice: row.get(5)?,
        voice_on: row.get(6)?,
        is_default: row.get(7)?,
    })
}

/// An account's identities, the default first, then in the order added.
pub fn list(conn: &Connection, account_id: &str) -> Result<Vec<Identity>, StoreError> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM identities WHERE account_id = ?1 ORDER BY is_default DESC, id"
    ))?;
    let rows = statement.query_map([account_id], from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Identity>, StoreError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM identities WHERE id = ?1"),
            [id],
            from_row,
        )
        .optional()?)
}

pub fn default_for(conn: &Connection, account_id: &str) -> Result<Option<Identity>, StoreError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM identities WHERE account_id = ?1 AND is_default = 1"),
            [account_id],
            from_row,
        )
        .optional()?)
}

/// The identity a message was sent to, among `addresses` (a reply goes out
/// from the address that was written to); the first that matches.
pub fn for_addresses(conn: &Connection, account_id: &str, addresses: &[&str]) -> Result<Option<Identity>, StoreError> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM identities WHERE account_id = ?1 AND lower(email) = lower(?2)"
    ))?;
    for address in addresses {
        if let Some(found) = statement.query_row(params![account_id, address], from_row).optional()? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// Add an identity; the first of an account becomes its default.
pub fn add(tx: &Transaction<'_>, account_id: &str, fields: &Fields, now: i64) -> Result<i64, StoreError> {
    let first: bool = tx.query_row(
        "SELECT NOT EXISTS (SELECT 1 FROM identities WHERE account_id = ?1)",
        [account_id],
        |row| row.get(0),
    )?;
    let inserted = tx.execute(
        "INSERT INTO identities (account_id, name, email, signature, is_default, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![account_id, fields.name, fields.email, fields.signature, first, now],
    );
    taken(inserted, &fields.email)?;
    Ok(tx.last_insert_rowid())
}

/// Change an identity's name, address or signature. `false` if there is none.
pub fn update(tx: &Transaction<'_>, id: i64, fields: &Fields) -> Result<bool, StoreError> {
    let updated = tx.execute(
        "UPDATE identities SET name = ?2, email = ?3, signature = ?4 WHERE id = ?1",
        params![id, fields.name, fields.email, fields.signature],
    );
    Ok(taken(updated, &fields.email)? == 1)
}

/// Set an identity's voice, and whether agents get it. `false` if there is
/// no such identity.
pub fn set_voice(tx: &Transaction<'_>, id: i64, voice: &str, on: bool) -> Result<bool, StoreError> {
    let voice = voice.trim();
    if voice.chars().count() > VOICE_MAX {
        return Err(StoreError::Conflict(format!(
            "a voice can have at most {VOICE_MAX} characters"
        )));
    }
    Ok(tx.execute(
        "UPDATE identities SET voice = ?2, voice_on = ?3 WHERE id = ?1",
        params![id, voice, on],
    )? == 1)
}

fn taken(result: rusqlite::Result<usize>, email: &str) -> Result<usize, StoreError> {
    match result {
        Err(rusqlite::Error::SqliteFailure(error, _)) if error.code == ErrorCode::ConstraintViolation => Err(
            StoreError::Conflict(format!("{email} is already one of this account's addresses")),
        ),
        other => Ok(other?),
    }
}

/// Make an identity its account's default. `false` if there is none.
pub fn make_default(tx: &Transaction<'_>, id: i64) -> Result<bool, StoreError> {
    let Some(identity) = get(tx, id)? else { return Ok(false) };
    tx.execute(
        "UPDATE identities SET is_default = 0 WHERE account_id = ?1 AND is_default = 1",
        [&identity.account_id],
    )?;
    tx.execute("UPDATE identities SET is_default = 1 WHERE id = ?1", [id])?;
    Ok(true)
}

/// Remove an identity; drafts from it go back to the default. The default
/// cannot be removed.
pub fn remove(tx: &Transaction<'_>, id: i64) -> Result<bool, StoreError> {
    match get(tx, id)? {
        None => Ok(false),
        Some(identity) if identity.is_default => Err(StoreError::Conflict(
            "the default address cannot be removed; make another one the default first".into(),
        )),
        Some(_) => Ok(tx.execute("DELETE FROM identities WHERE id = ?1", [id])? == 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use regards_core::paths::DataDirs;

    #[tokio::test]
    async fn an_account_starts_with_its_own_address_and_can_send_as_others() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let account = accounts::new_id();
        let a = account.clone();
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
                        display_name: "Ada Lovelace".into(),
                        email: "ada@example.test".into(),
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
        let a = account.clone();
        let own = store.read(move |c| default_for(c, &a)).await.unwrap().unwrap();
        assert_eq!(
            (own.name.as_str(), own.email.as_str()),
            ("Ada Lovelace", "ada@example.test")
        );

        // An alias, with a signature.
        let a = account.clone();
        let alias = store
            .write(move |tx| {
                let fields = Fields {
                    name: "Ada (work)".into(),
                    email: "ada@work.test".into(),
                    signature: "Ada\nAnalytical Engines Ltd".into(),
                };
                add(tx, &a, &fields, 1)
            })
            .await
            .unwrap()
            .value;
        let a = account.clone();
        let found = store
            .read(move |c| for_addresses(c, &a, &["bob@x.test", "ADA@work.test"]))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, alias, "found by the address written to, in any case");

        // The same address twice: refused.
        let a = account.clone();
        let twice = store
            .write(move |tx| {
                let fields = Fields {
                    name: String::new(),
                    email: "Ada@Work.test".into(),
                    signature: String::new(),
                };
                add(tx, &a, &fields, 2)
            })
            .await;
        assert!(matches!(twice, Err(StoreError::Conflict(_))));

        // The default cannot go; once another is the default, it can.
        let own_id = own.id;
        assert!(matches!(
            store.write(move |tx| remove(tx, own_id)).await,
            Err(StoreError::Conflict(_))
        ));
        assert!(store.write(move |tx| make_default(tx, alias)).await.unwrap().value);
        let a = account.clone();
        let listed = store.read(move |c| list(c, &a)).await.unwrap();
        assert_eq!(
            listed.iter().map(|i| (i.id, i.is_default)).collect::<Vec<_>>(),
            [(alias, true), (own_id, false)]
        );
        assert!(store.write(move |tx| remove(tx, own_id)).await.unwrap().value);

        // A voice: off and empty at first; kept while switched off.
        let found = store.read(move |c| get(c, alias)).await.unwrap().unwrap();
        assert_eq!(
            (found.voice.as_str(), found.voice_on, found.active_voice()),
            ("", false, None)
        );
        assert!(
            store
                .write(move |tx| set_voice(tx, alias, "  Short and warm.\n", true))
                .await
                .unwrap()
                .value
        );
        let found = store.read(move |c| get(c, alias)).await.unwrap().unwrap();
        assert_eq!(found.active_voice(), Some("Short and warm."));
        assert!(
            store
                .write(move |tx| set_voice(tx, alias, "Short and warm.", false))
                .await
                .unwrap()
                .value
        );
        let found = store.read(move |c| get(c, alias)).await.unwrap().unwrap();
        assert_eq!((found.voice.as_str(), found.active_voice()), ("Short and warm.", None));
        let long = "a".repeat(VOICE_MAX + 1);
        assert!(matches!(
            store.write(move |tx| set_voice(tx, alias, &long, true)).await,
            Err(StoreError::Conflict(_))
        ));
        assert!(
            !store.write(|tx| set_voice(tx, 999, "", false)).await.unwrap().value,
            "no such identity"
        );
    }
}
