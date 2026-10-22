//! Signatures (ADR-0021): a list of their own, for every account, each
//! named for the person. Each identity has one as its default, or none;
//! deleting one leaves the identities that used it with none.

use crate::StoreError;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

/// Characters a signature may have at most.
pub const BODY_MAX: usize = 5_000;
/// Characters a signature's name may have at most.
pub const NAME_MAX: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub id: i64,
    pub name: String,
    /// Plain text, added below what is written and above a quote.
    pub body: String,
    pub updated_at: i64,
}

/// What a signature says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    pub name: String,
    pub body: String,
}

const COLUMNS: &str = "id, name, body, updated_at";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Signature> {
    Ok(Signature {
        id: row.get(0)?,
        name: row.get(1)?,
        body: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

/// Every signature, by name.
pub fn list(conn: &Connection) -> Result<Vec<Signature>, StoreError> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM signatures ORDER BY name COLLATE NOCASE, id"
    ))?;
    let rows = statement.query_map([], from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Signature>, StoreError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM signatures WHERE id = ?1"),
            [id],
            from_row,
        )
        .optional()?)
}

/// The signature with this exact text, if there is one.
pub fn with_body(conn: &Connection, body: &str) -> Result<Option<Signature>, StoreError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM signatures WHERE body = ?1 ORDER BY id LIMIT 1"),
            [body],
            from_row,
        )
        .optional()?)
}

pub fn add(tx: &Transaction<'_>, fields: &Fields, now: i64) -> Result<i64, StoreError> {
    tx.execute(
        "INSERT INTO signatures (name, body, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![fields.name, fields.body, now],
    )?;
    Ok(tx.last_insert_rowid())
}

/// `false` if there is no such signature.
pub fn update(tx: &Transaction<'_>, id: i64, fields: &Fields, now: i64) -> Result<bool, StoreError> {
    Ok(tx.execute(
        "UPDATE signatures SET name = ?2, body = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, fields.name, fields.body, now],
    )? == 1)
}

/// `false` if there is no such signature. The identities that used it
/// have none from then on.
pub fn remove(tx: &Transaction<'_>, id: i64) -> Result<bool, StoreError> {
    Ok(tx.execute("DELETE FROM signatures WHERE id = ?1", [id])? == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use regards_core::paths::DataDirs;

    #[tokio::test]
    async fn signatures_are_kept_by_name_found_by_text_changed_and_removed() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let fields = |name: &str, body: &str| Fields {
            name: name.into(),
            body: body.into(),
        };
        let (short, formal) = (fields("short", "Ada"), fields("Formal", "Ada Lovelace\nEngines Ltd"));
        let (short, formal) = store
            .write(move |tx| Ok((add(tx, &short, 1)?, add(tx, &formal, 2)?)))
            .await
            .unwrap()
            .value;
        let names = |list: Vec<Signature>| list.into_iter().map(|s| s.name).collect::<Vec<_>>();
        assert_eq!(
            names(store.read(list).await.unwrap()),
            ["Formal", "short"],
            "by name, in any case"
        );
        assert_eq!(
            store.read(|c| with_body(c, "Ada")).await.unwrap().map(|s| s.id),
            Some(short)
        );

        let changed = fields("Short", "Ada L.");
        assert!(
            store
                .write(move |tx| update(tx, short, &changed, 3))
                .await
                .unwrap()
                .value
        );
        let got = store.read(move |c| get(c, short)).await.unwrap().unwrap();
        assert_eq!(
            (got.name.as_str(), got.body.as_str(), got.updated_at),
            ("Short", "Ada L.", 3)
        );

        assert!(store.write(move |tx| remove(tx, formal)).await.unwrap().value);
        assert!(
            !store.write(move |tx| remove(tx, formal)).await.unwrap().value,
            "gone already"
        );
        assert_eq!(names(store.read(list).await.unwrap()), ["Short"]);
    }
}
