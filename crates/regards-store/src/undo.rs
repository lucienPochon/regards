//! Undo (ADR-0007): each undoable action stores its inverse, the intent
//! that reverses it, for a limited time.

use crate::StoreError;
use rusqlite::{OptionalExtension, Transaction, params};

/// Store an inverse; returns the id the client uses to undo.
pub fn record(
    tx: &Transaction<'_>,
    description: &str,
    inverse: &str,
    now: i64,
    expires_at: i64,
) -> Result<i64, StoreError> {
    // Old entries are of no use to anyone.
    tx.execute("DELETE FROM undo_log WHERE expires_at < ?1", [now])?;
    tx.execute(
        "INSERT INTO undo_log (created_at, expires_at, description, inverse) VALUES (?1, ?2, ?3, ?4)",
        params![now, expires_at, description, inverse],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Take an inverse to apply it. Each can be used once, and only before it expires.
pub fn take(tx: &Transaction<'_>, id: i64, now: i64) -> Result<Option<String>, StoreError> {
    let inverse: Option<String> = tx
        .query_row(
            "SELECT inverse FROM undo_log WHERE id = ?1 AND expires_at >= ?2",
            params![id, now],
            |row| row.get(0),
        )
        .optional()?;
    tx.execute("DELETE FROM undo_log WHERE id = ?1", [id])?;
    Ok(inverse)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use regards_core::paths::DataDirs;

    #[tokio::test]
    async fn an_inverse_can_be_taken_once_and_only_before_it_expires() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let id = store
            .write(|tx| record(tx, "Archived", "{}", 100, 200))
            .await
            .unwrap()
            .value;
        assert_eq!(
            store.write(move |tx| take(tx, id, 150)).await.unwrap().value.as_deref(),
            Some("{}")
        );
        assert_eq!(
            store.write(move |tx| take(tx, id, 150)).await.unwrap().value,
            None,
            "only once"
        );

        let late = store
            .write(|tx| record(tx, "Archived", "{}", 100, 200))
            .await
            .unwrap()
            .value;
        assert_eq!(
            store.write(move |tx| take(tx, late, 201)).await.unwrap().value,
            None,
            "expired"
        );
    }
}
