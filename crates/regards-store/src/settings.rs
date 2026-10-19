//! Small choices the daemon keeps, by key (the `settings` table, migration
//! 0019), for those that need nothing more than a word.

use crate::StoreError;
use rusqlite::{Connection, OptionalExtension, Transaction};

pub fn get(conn: &Connection, key: &str) -> Result<Option<String>, StoreError> {
    Ok(conn
        .prepare_cached("SELECT value FROM settings WHERE key = ?1")?
        .query_row([key], |row| row.get(0))
        .optional()?)
}

pub fn set(tx: &Transaction<'_>, key: &str, value: &str) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2",
        [key, value],
    )?;
    Ok(())
}
