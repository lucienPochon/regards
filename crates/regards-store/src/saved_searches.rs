//! Saved searches (Spark's Smart Folders): a search kept by name and run
//! again whenever it is opened, of every account or of one. Kept on this
//! device; the sidebar lists them in their order.

use crate::StoreError;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSearch {
    pub id: i64,
    pub name: String,
    /// What was typed, in the search language.
    pub query: String,
    /// `None`: every account.
    pub account_id: Option<String>,
    /// `#rrggbb`, or none.
    pub color: Option<String>,
    pub position: i64,
}

/// What a saved search is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    pub name: String,
    pub query: String,
    pub account_id: Option<String>,
    pub color: Option<String>,
}

const COLUMNS: &str = "id, name, query, account_id, color, position";

fn from_row(row: &Row<'_>) -> rusqlite::Result<SavedSearch> {
    Ok(SavedSearch {
        id: row.get(0)?,
        name: row.get(1)?,
        query: row.get(2)?,
        account_id: row.get(3)?,
        color: row.get(4)?,
        position: row.get(5)?,
    })
}

/// Every saved search, in the sidebar's order.
pub fn list(conn: &Connection) -> Result<Vec<SavedSearch>, StoreError> {
    let mut statement = conn.prepare_cached(&format!("SELECT {COLUMNS} FROM saved_searches ORDER BY position, id"))?;
    let rows = statement.query_map([], from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<SavedSearch>, StoreError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM saved_searches WHERE id = ?1"),
            [id],
            from_row,
        )
        .optional()?)
}

/// A new one, last in the order.
pub fn add(tx: &Transaction<'_>, fields: &Fields, now: i64) -> Result<i64, StoreError> {
    tx.execute(
        "INSERT INTO saved_searches (name, query, account_id, color, position, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, (SELECT COALESCE(MAX(position), -1) + 1 FROM saved_searches), ?5, ?5)",
        params![fields.name, fields.query, fields.account_id, fields.color, now],
    )?;
    Ok(tx.last_insert_rowid())
}

/// `false` if there is no such saved search.
pub fn update(tx: &Transaction<'_>, id: i64, fields: &Fields, now: i64) -> Result<bool, StoreError> {
    Ok(tx.execute(
        "UPDATE saved_searches SET name = ?2, query = ?3, account_id = ?4, color = ?5, updated_at = ?6 WHERE id = ?1",
        params![id, fields.name, fields.query, fields.account_id, fields.color, now],
    )? == 1)
}

/// Put them in this order; ids not given keep theirs, after these.
pub fn reorder(tx: &Transaction<'_>, ids: &[i64]) -> Result<(), StoreError> {
    let others: Vec<i64> = list(tx)?
        .into_iter()
        .map(|saved| saved.id)
        .filter(|id| !ids.contains(id))
        .collect();
    let mut statement = tx.prepare_cached("UPDATE saved_searches SET position = ?2 WHERE id = ?1")?;
    for (position, id) in ids.iter().chain(&others).enumerate() {
        statement.execute(params![id, position as i64])?;
    }
    Ok(())
}

/// `false` if there is no such saved search. The mail is not touched.
pub fn remove(tx: &Transaction<'_>, id: i64) -> Result<bool, StoreError> {
    Ok(tx.execute("DELETE FROM saved_searches WHERE id = ?1", [id])? == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use regards_core::paths::DataDirs;

    fn fields(name: &str, query: &str) -> Fields {
        Fields {
            name: name.into(),
            query: query.into(),
            ..Fields::default()
        }
    }

    #[tokio::test]
    async fn saved_searches_keep_their_order_and_can_change_or_go() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let (bills, trips) = store
            .write(|tx| {
                Ok((
                    add(tx, &fields("Bills", "invoice"), 1)?,
                    add(tx, &fields("Trips", "has:attachment flight"), 2)?,
                ))
            })
            .await
            .unwrap()
            .value;
        let names = |all: Vec<SavedSearch>| all.into_iter().map(|saved| saved.name).collect::<Vec<_>>();
        assert_eq!(names(store.read(list).await.unwrap()), ["Bills", "Trips"]);

        store.write(move |tx| reorder(tx, &[trips])).await.unwrap();
        assert_eq!(names(store.read(list).await.unwrap()), ["Trips", "Bills"]);

        let changed = Fields {
            color: Some("#1d7a61".into()),
            ..fields("Invoices", "invoice last month")
        };
        assert!(
            store
                .write(move |tx| update(tx, bills, &changed, 3))
                .await
                .unwrap()
                .value
        );
        let found = store.read(move |conn| get(conn, bills)).await.unwrap().unwrap();
        assert_eq!(
            (found.name.as_str(), found.query.as_str(), found.color.as_deref()),
            ("Invoices", "invoice last month", Some("#1d7a61"))
        );

        assert!(store.write(move |tx| remove(tx, trips)).await.unwrap().value);
        assert!(!store.write(move |tx| remove(tx, trips)).await.unwrap().value);
        assert_eq!(names(store.read(list).await.unwrap()), ["Invoices"]);
    }
}
