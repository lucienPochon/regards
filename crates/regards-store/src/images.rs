//! Senders whose remote images load (ADR-0015 §4). Remote images can tell a
//! sender their message was opened, so they are blocked by default; the
//! person allows them per message, or per sender from then on: an address,
//! or a whole `@domain`.

use crate::StoreError;
use regards_core::smart::domain_key;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

/// Whether `address`'s remote images load for an account: allowed for the
/// address, or for its domain.
pub fn allowed(conn: &Connection, account_id: &str, address: &str) -> Result<bool, StoreError> {
    let mut keys = vec![address.trim().to_lowercase()];
    if let Some(domain) = domain_key(address).filter(|_| !address.trim().starts_with('@')) {
        keys.push(domain);
    }
    let mut statement =
        conn.prepare_cached("SELECT 1 FROM senders WHERE account_id = ?1 AND address = ?2 AND images = 1")?;
    for key in keys {
        if statement
            .query_row(params![account_id, key], |_| Ok(()))
            .optional()?
            .is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Let `key`'s remote images load (an address, or `@domain`), or block them again.
pub fn set(tx: &Transaction<'_>, account_id: &str, key: &str, allow: bool) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO senders (account_id, address, images) VALUES (?1, lower(?2), ?3) \
         ON CONFLICT (account_id, address) DO UPDATE SET images = ?3",
        params![account_id, key.trim(), allow],
    )?;
    Ok(())
}

/// The senders an account loads images from, for its settings.
pub fn list(conn: &Connection, account_id: &str) -> Result<Vec<String>, StoreError> {
    let mut statement =
        conn.prepare_cached("SELECT address FROM senders WHERE account_id = ?1 AND images = 1 ORDER BY address")?;
    let rows = statement.query_map([account_id], |row| row.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use regards_core::paths::DataDirs;

    #[tokio::test]
    async fn blocked_until_allowed_for_the_address_or_its_domain() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        store
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
                )
                .map(drop)
            })
            .await
            .unwrap();
        let allowed_for = |address: &'static str| {
            let store = &store;
            async move { store.read(move |conn| allowed(conn, "a1", address)).await.unwrap() }
        };
        assert!(!allowed_for("news@shop.example").await, "blocked by default");

        store
            .write(|tx| set(tx, "a1", "News@Shop.example", true))
            .await
            .unwrap();
        assert!(allowed_for("news@shop.example").await);
        assert!(!allowed_for("deals@shop.example").await, "that address only");

        store.write(|tx| set(tx, "a1", "@shop.example", true)).await.unwrap();
        assert!(allowed_for("deals@SHOP.example").await, "the whole domain");
        assert_eq!(
            store.read(|conn| list(conn, "a1")).await.unwrap(),
            ["@shop.example", "news@shop.example"]
        );

        store.write(|tx| set(tx, "a1", "@shop.example", false)).await.unwrap();
        store
            .write(|tx| set(tx, "a1", "news@shop.example", false))
            .await
            .unwrap();
        assert!(!allowed_for("news@shop.example").await, "blocked again");
        assert!(
            !store
                .read(|conn| allowed(conn, "other", "news@shop.example"))
                .await
                .unwrap()
        );
    }
}
