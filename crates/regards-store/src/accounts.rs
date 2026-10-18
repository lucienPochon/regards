//! Mail accounts. Secrets are not here: `secret_key` names an entry in the OS
//! credential store (`regards-vault`).

use crate::StoreError;
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
