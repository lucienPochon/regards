//! Mail accounts. Secrets are not here: `secret_key` names an entry in the OS
//! credential store (`regards-vault`).

use crate::StoreError;
use regards_core::agents::Access;
use regards_core::compose::{AutoCopy, CopyKind};
use rusqlite::{Connection, ErrorCode, OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// How the account signs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    Password,
    OauthMicrosoft,
    OauthGoogle,
}

/// How a connection is encrypted. There is deliberately no plaintext option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Security {
    /// TLS from the first byte (IMAP 993, SMTP 465).
    Tls,
    /// Upgrade with STARTTLS before authenticating (SMTP 587).
    Starttls,
}

/// A server endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    pub host: String,
    pub port: u16,
    pub security: Security,
}

/// A new account, before it is stored.
#[derive(Debug, Clone)]
pub struct NewAccount {
    /// Chosen by the caller, so the credential can be stored under it first.
    pub id: String,
    pub display_name: String,
    pub email: String,
    pub auth_kind: AuthKind,
    pub username: String,
    pub secret_key: String,
    pub imap: Server,
    pub smtp: Server,
    pub imap_capabilities: Vec<String>,
}

/// A stored account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub display_name: String,
    pub email: String,
    pub auth_kind: AuthKind,
    pub username: String,
    pub secret_key: String,
    pub imap: Server,
    pub smtp: Server,
    pub imap_capabilities: Vec<String>,
    /// Unix time in milliseconds.
    pub created_at: i64,
    pub position: i64,
}

/// An account's mail on this device, dropped so it can be fetched again:
/// its messages (and with them new-mail notices), what each folder's sync
/// knew, and whom it writes with (counted again as mail comes back). The
/// account, its folders, drafts, outbox, marks and senders stay. Returns
/// how many messages were dropped.
pub fn forget_mail(tx: &Transaction<'_>, account_id: &str) -> Result<usize, StoreError> {
    let dropped = tx.execute(
        "DELETE FROM messages WHERE mailbox_id IN (SELECT id FROM mailboxes WHERE account_id = ?1)",
        [account_id],
    )?;
    tx.execute(
        "UPDATE mailboxes SET uid_validity = NULL, uid_next = NULL, highest_modseq = NULL, synced_at = NULL \
         WHERE account_id = ?1",
        [account_id],
    )?;
    tx.execute("DELETE FROM people WHERE account_id = ?1", [account_id])?;
    Ok(dropped)
}

/// A fresh random account ID.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

const COLUMNS: &str = "id, display_name, email, auth_kind, username, secret_key, \
    imap_host, imap_port, imap_security, smtp_host, smtp_port, smtp_security, \
    imap_capabilities, created_at, position";

/// Store a new account at the end of the list.
pub fn insert(tx: &Transaction<'_>, new: NewAccount) -> Result<Account, StoreError> {
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default();
    let position: i64 = tx.query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM accounts", [], |row| {
        row.get(0)
    })?;
    let capabilities = serde_json::to_string(&new.imap_capabilities).expect("strings always serialize");
    let inserted = tx.execute(
        &format!(
            "INSERT INTO accounts ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)"
        ),
        params![
            new.id,
            new.display_name,
            new.email,
            auth_kind_str(new.auth_kind),
            new.username,
            new.secret_key,
            new.imap.host,
            new.imap.port,
            security_str(new.imap.security),
            new.smtp.host,
            new.smtp.port,
            security_str(new.smtp.security),
            capabilities,
            created_at,
            position,
        ],
    );
    match inserted {
        Ok(_) => {}
        Err(rusqlite::Error::SqliteFailure(error, _)) if error.code == ErrorCode::ConstraintViolation => {
            return Err(StoreError::Conflict(format!(
                "an account for {} already exists",
                new.email
            )));
        }
        Err(error) => return Err(error.into()),
    }
    // Its own address is the identity it sends as by default.
    crate::identities::add(
        tx,
        &new.id,
        &crate::identities::Fields {
            name: new.display_name.clone(),
            email: new.email.clone(),
            signature_id: None,
        },
        created_at,
    )?;
    Ok(Account {
        id: new.id,
        display_name: new.display_name,
        email: new.email,
        auth_kind: new.auth_kind,
        username: new.username,
        secret_key: new.secret_key,
        imap: new.imap,
        smtp: new.smtp,
        imap_capabilities: new.imap_capabilities,
        created_at,
        position,
    })
}

/// All accounts, in list order.
pub fn list(conn: &Connection) -> Result<Vec<Account>, StoreError> {
    let mut statement = conn.prepare(&format!("SELECT {COLUMNS} FROM accounts ORDER BY position, created_at"))?;
    let rows = statement.query_map([], from_row)?;
    rows.map(|row| row?).collect()
}

/// What each account allows agents to do (ADR-0012), in list order.
pub fn agent_access(conn: &Connection) -> Result<Vec<(String, Access)>, StoreError> {
    let mut statement = conn.prepare_cached("SELECT id, agent_access FROM accounts ORDER BY position, created_at")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut levels = Vec::new();
    for row in rows {
        let (id, access) = row?;
        levels.push((id, Access::parse(&access).unwrap_or_default()));
    }
    Ok(levels)
}

/// An account's address and agent access.
pub fn agent_access_of(conn: &Connection, id: &str) -> Result<Option<(String, Access)>, StoreError> {
    let found = conn
        .prepare_cached("SELECT email, agent_access FROM accounts WHERE id = ?1")?
        .query_row([id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .optional()?;
    Ok(found.map(|(email, access)| (email, Access::parse(&access).unwrap_or_default())))
}

/// How long an agent's send waits, undoable, before it goes (ADR-0012 §3),
/// in seconds; `None`: each waits for the person's approval.
pub fn agent_send_windows(conn: &Connection) -> Result<Vec<(String, Option<u32>)>, StoreError> {
    let mut statement = conn.prepare_cached("SELECT id, agent_send_window FROM accounts")?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// [`agent_send_windows`] for one account; `None` also when there is none.
pub fn agent_send_window(conn: &Connection, id: &str) -> Result<Option<u32>, StoreError> {
    Ok(conn
        .prepare_cached("SELECT agent_send_window FROM accounts WHERE id = ?1")?
        .query_row([id], |row| row.get(0))
        .optional()?
        .flatten())
}

/// Let an agent's sends go after `window` seconds (10 to 600), or (`None`)
/// wait for approval; `false` if there is no such account.
pub fn set_agent_send_window(tx: &Transaction<'_>, id: &str, window: Option<u32>) -> Result<bool, StoreError> {
    let changed = tx.execute(
        "UPDATE accounts SET agent_send_window = ?2 WHERE id = ?1",
        params![id, window],
    )?;
    Ok(changed > 0)
}

/// The copy every new draft of the account gets, if any.
pub fn auto_copy(conn: &Connection, id: &str) -> Result<Option<AutoCopy>, StoreError> {
    let found: Option<(Option<String>, Option<String>)> = conn
        .prepare_cached("SELECT auto_copy_kind, auto_copy_address FROM accounts WHERE id = ?1")?
        .query_row([id], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?;
    Ok(match found {
        Some((Some(kind), Some(address))) => Some(AutoCopy {
            kind: if kind == "bcc" { CopyKind::Bcc } else { CopyKind::Cc },
            address,
        }),
        _ => None,
    })
}

/// Set (or, with `None`, clear) the copy every new draft gets; `false` if
/// there is no such account.
pub fn set_auto_copy(tx: &Transaction<'_>, id: &str, copy: Option<&AutoCopy>) -> Result<bool, StoreError> {
    let (kind, address) = match copy {
        Some(copy) => (
            Some(match copy.kind {
                CopyKind::Cc => "cc",
                CopyKind::Bcc => "bcc",
            }),
            Some(copy.address.as_str()),
        ),
        None => (None, None),
    };
    let changed = tx.execute(
        "UPDATE accounts SET auto_copy_kind = ?2, auto_copy_address = ?3 WHERE id = ?1",
        params![id, kind, address],
    )?;
    Ok(changed > 0)
}

/// Rename an account. Its identity for its own address follows when it
/// still had the old name (not one set for it). `None` if there is no such account.
pub fn rename(tx: &Transaction<'_>, id: &str, display_name: &str) -> Result<Option<Account>, StoreError> {
    let Some(before) = get(tx, id)? else {
        return Ok(None);
    };
    tx.execute(
        "UPDATE accounts SET display_name = ?2 WHERE id = ?1",
        params![id, display_name],
    )?;
    tx.execute(
        "UPDATE identities SET name = ?3 WHERE account_id = ?1 AND lower(email) = lower(?2) AND name = ?4",
        params![id, before.email, display_name, before.display_name],
    )?;
    get(tx, id)
}

/// A new sign-in for an account: its user name, servers, and what the IMAP
/// server advertised with them. `None` if there is no such account.
pub fn set_login(
    tx: &Transaction<'_>,
    id: &str,
    username: &str,
    imap: &Server,
    smtp: &Server,
    imap_capabilities: &[String],
) -> Result<Option<Account>, StoreError> {
    let capabilities = serde_json::to_string(imap_capabilities).expect("strings always serialize");
    let changed = tx.execute(
        "UPDATE accounts SET username = ?2, imap_host = ?3, imap_port = ?4, imap_security = ?5, \
         smtp_host = ?6, smtp_port = ?7, smtp_security = ?8, imap_capabilities = ?9 WHERE id = ?1",
        params![
            id,
            username,
            imap.host,
            imap.port,
            security_str(imap.security),
            smtp.host,
            smtp.port,
            security_str(smtp.security),
            capabilities,
        ],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    get(tx, id)
}

/// Set what an account allows agents to do; `false` if there is no such account.
pub fn set_agent_access(tx: &Transaction<'_>, id: &str, access: Access) -> Result<bool, StoreError> {
    let changed = tx.execute(
        "UPDATE accounts SET agent_access = ?2 WHERE id = ?1",
        params![id, access.as_str()],
    )?;
    Ok(changed > 0)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Account>, StoreError> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM accounts WHERE id = ?1"), [id], from_row)
        .optional()?
        .transpose()
}

/// Remove an account; returns it so the caller can delete its credential.
pub fn delete(tx: &Transaction<'_>, id: &str) -> Result<Option<Account>, StoreError> {
    let Some(account) = tx
        .query_row(&format!("SELECT {COLUMNS} FROM accounts WHERE id = ?1"), [id], from_row)
        .optional()?
        .transpose()?
    else {
        return Ok(None);
    };
    tx.execute("DELETE FROM accounts WHERE id = ?1", [id])?;
    Ok(Some(account))
}

/// Decode a row. The outer `Result` is SQLite's; the inner one reports values
/// that SQLite accepted but Regards cannot interpret.
fn from_row(row: &Row<'_>) -> rusqlite::Result<Result<Account, StoreError>> {
    let auth_kind: String = row.get(3)?;
    let imap_security: String = row.get(8)?;
    let smtp_security: String = row.get(11)?;
    let capabilities: String = row.get(12)?;
    let account = (|| {
        Ok(Account {
            id: row.get(0)?,
            display_name: row.get(1)?,
            email: row.get(2)?,
            auth_kind: parse_auth_kind(&auth_kind)?,
            username: row.get(4)?,
            secret_key: row.get(5)?,
            imap: Server {
                host: row.get(6)?,
                port: row.get(7)?,
                security: parse_security(&imap_security)?,
            },
            smtp: Server {
                host: row.get(9)?,
                port: row.get(10)?,
                security: parse_security(&smtp_security)?,
            },
            imap_capabilities: serde_json::from_str(&capabilities)
                .map_err(|error| StoreError::Corrupt(format!("account capabilities: {error}")))?,
            created_at: row.get(13)?,
            position: row.get(14)?,
        })
    })();
    Ok(account)
}

fn auth_kind_str(kind: AuthKind) -> &'static str {
    match kind {
        AuthKind::Password => "password",
        AuthKind::OauthMicrosoft => "oauth_microsoft",
        AuthKind::OauthGoogle => "oauth_google",
    }
}

fn parse_auth_kind(value: &str) -> Result<AuthKind, StoreError> {
    match value {
        "password" => Ok(AuthKind::Password),
        "oauth_microsoft" => Ok(AuthKind::OauthMicrosoft),
        "oauth_google" => Ok(AuthKind::OauthGoogle),
        other => Err(StoreError::Corrupt(format!("unknown auth kind {other:?}"))),
    }
}

fn security_str(security: Security) -> &'static str {
    match security {
        Security::Tls => "tls",
        Security::Starttls => "starttls",
    }
}

fn parse_security(value: &str) -> Result<Security, StoreError> {
    match value {
        "tls" => Ok(Security::Tls),
        "starttls" => Ok(Security::Starttls),
        other => Err(StoreError::Corrupt(format!("unknown connection security {other:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use regards_core::paths::DataDirs;

    fn new_account(email: &str) -> NewAccount {
        let id = new_id();
        NewAccount {
            secret_key: format!("{id}/mail-password"),
            id,
            display_name: "Ada".into(),
            email: email.into(),
            auth_kind: AuthKind::Password,
            username: email.into(),
            imap: Server {
                host: "mail.example.test".into(),
                port: 993,
                security: Security::Tls,
            },
            smtp: Server {
                host: "mail.example.test".into(),
                port: 587,
                security: Security::Starttls,
            },
            imap_capabilities: vec!["IMAP4rev1".into(), "IDLE".into()],
        }
    }

    #[tokio::test]
    async fn insert_list_get_and_delete() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();

        let first = store
            .write(|tx| insert(tx, new_account("ada@example.test")))
            .await
            .unwrap();
        let second = store
            .write(|tx| insert(tx, new_account("bob@example.test")))
            .await
            .unwrap();
        assert_eq!((first.value.position, second.value.position), (0, 1));
        assert_eq!(second.revision, first.revision + 1);

        let all = store.read(list).await.unwrap();
        assert_eq!(all, vec![first.value.clone(), second.value.clone()]);

        let id = first.value.id.clone();
        let found = store.read(move |conn| get(conn, &id)).await.unwrap();
        assert_eq!(found.as_ref(), Some(&first.value));

        let id = first.value.id.clone();
        let deleted = store.write(move |tx| delete(tx, &id)).await.unwrap().value;
        assert_eq!(deleted, Some(first.value));
        assert_eq!(store.read(list).await.unwrap(), vec![second.value]);

        let none = store.write(|tx| delete(tx, "no-such-id")).await.unwrap().value;
        assert_eq!(none, None);
    }

    #[tokio::test]
    async fn agents_are_kept_out_until_an_account_lets_them_in() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let (work, home) = (new_account("work@example.test"), new_account("home@example.test"));
        let (work_id, home_id) = (work.id.clone(), home.id.clone());
        store
            .write(move |tx| {
                insert(tx, work)?;
                insert(tx, home)
            })
            .await
            .unwrap();
        let levels = store.read(agent_access).await.unwrap();
        assert_eq!(
            levels,
            [(work_id.clone(), Access::Off), (home_id.clone(), Access::Off)],
            "off by default"
        );

        let id = work_id.clone();
        assert!(
            store
                .write(move |tx| set_agent_access(tx, &id, Access::Triage))
                .await
                .unwrap()
                .value
        );
        let id = work_id.clone();
        let found = store.read(move |conn| agent_access_of(conn, &id)).await.unwrap();
        assert_eq!(found, Some(("work@example.test".into(), Access::Triage)));
        assert!(
            !store
                .write(|tx| set_agent_access(tx, "nobody", Access::Send))
                .await
                .unwrap()
                .value
        );
        // Only the four levels can be stored.
        let id = home_id.clone();
        let refused = store
            .write(move |tx| Ok(tx.execute("UPDATE accounts SET agent_access = 'admin' WHERE id = ?1", [&id])?))
            .await;
        assert!(refused.is_err());
    }

    #[tokio::test]
    async fn a_new_login_keeps_the_account_and_its_place() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let added = store
            .write(|tx| insert(tx, new_account("ada@example.test")))
            .await
            .unwrap()
            .value;
        let moved = Server {
            host: "imap.new.test".into(),
            port: 994,
            security: Security::Tls,
        };
        let sending = Server {
            host: "smtp.new.test".into(),
            port: 465,
            security: Security::Tls,
        };
        let (id, imap, smtp) = (added.id.clone(), moved.clone(), sending.clone());
        let changed = store
            .write(move |tx| set_login(tx, &id, "ada", &imap, &smtp, &["IMAP4rev1".into()]))
            .await
            .unwrap()
            .value
            .unwrap();
        assert_eq!(
            changed,
            Account {
                username: "ada".into(),
                imap: moved,
                smtp: sending.clone(),
                imap_capabilities: vec!["IMAP4rev1".into()],
                ..added
            }
        );
        let none = store
            .write(move |tx| set_login(tx, "nobody", "x", &sending, &sending, &[]))
            .await
            .unwrap()
            .value;
        assert_eq!(none, None);
    }

    #[tokio::test]
    async fn the_same_address_cannot_be_added_twice_in_any_case() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        store
            .write(|tx| insert(tx, new_account("ada@example.test")))
            .await
            .unwrap();
        let again = store.write(|tx| insert(tx, new_account("ADA@Example.test"))).await;
        assert!(matches!(again, Err(StoreError::Conflict(_))), "got {again:?}");
        assert_eq!(store.read(list).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_schema_rejects_plaintext_connections() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let result = store
            .write(|tx| {
                let mut account = new_account("ada@example.test");
                account.imap.security = Security::Tls;
                insert(tx, account)?;
                tx.execute("UPDATE accounts SET imap_security = 'none'", [])?;
                Ok(())
            })
            .await;
        assert!(result.is_err(), "the CHECK constraint must refuse 'none'");
    }
}
