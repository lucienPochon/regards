//! Templates (plan §5.9): text to start a message with, by name, kept on
//! this device for every account. The composer puts one into a draft.

use crate::StoreError;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub id: i64,
    pub name: String,
    pub subject: String,
    /// The text; with `html`, its plain-text version.
    pub body: String,
    pub html: Option<String>,
    pub updated_at: i64,
}

/// What a template says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    pub name: String,
    pub subject: String,
    pub body: String,
    pub html: Option<String>,
}

const COLUMNS: &str = "id, name, subject, body, html, updated_at";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Template> {
    Ok(Template {
        id: row.get(0)?,
        name: row.get(1)?,
        subject: row.get(2)?,
        body: row.get(3)?,
        html: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

/// Every template, by name.
pub fn list(conn: &Connection) -> Result<Vec<Template>, StoreError> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM templates ORDER BY name COLLATE NOCASE, id"
    ))?;
    let rows = statement.query_map([], from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Template>, StoreError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM templates WHERE id = ?1"),
            [id],
            from_row,
        )
        .optional()?)
}

pub fn add(tx: &Transaction<'_>, fields: &Fields, now: i64) -> Result<i64, StoreError> {
    tx.execute(
        "INSERT INTO templates (name, subject, body, html, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![fields.name, fields.subject, fields.body, fields.html, now],
    )?;
    Ok(tx.last_insert_rowid())
}

/// `false` if there is no such template.
pub fn update(tx: &Transaction<'_>, id: i64, fields: &Fields, now: i64) -> Result<bool, StoreError> {
    Ok(tx.execute(
        "UPDATE templates SET name = ?2, subject = ?3, body = ?4, html = ?5, updated_at = ?6 WHERE id = ?1",
        params![id, fields.name, fields.subject, fields.body, fields.html, now],
    )? == 1)
}

pub fn remove(tx: &Transaction<'_>, id: i64) -> Result<bool, StoreError> {
    Ok(tx.execute("DELETE FROM templates WHERE id = ?1", [id])? == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use regards_core::paths::DataDirs;

    #[tokio::test]
    async fn templates_are_kept_by_name_changed_and_removed() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let fields = |name: &str| Fields {
            name: name.into(),
            subject: format!("About {name}"),
            body: "Thanks for writing.".into(),
            html: None,
        };
        let (b, a) = (fields("booking"), fields("Apology"));
        let ids = store
            .write(move |tx| Ok((add(tx, &b, 1)?, add(tx, &a, 2)?)))
            .await
            .unwrap()
            .value;
        let names = |list: Vec<Template>| list.into_iter().map(|t| t.name).collect::<Vec<_>>();
        assert_eq!(
            names(store.read(list).await.unwrap()),
            ["Apology", "booking"],
            "by name, in any case"
        );

        let changed = Fields {
            html: Some("<p>Thanks for <b>writing</b>.</p>".into()),
            ..fields("Booking")
        };
        let id = ids.0;
        assert!(store.write(move |tx| update(tx, id, &changed, 3)).await.unwrap().value);
        let got = store.read(move |c| get(c, id)).await.unwrap().unwrap();
        assert_eq!((got.name.as_str(), got.updated_at), ("Booking", 3));
        assert!(got.html.is_some());

        assert!(store.write(move |tx| remove(tx, id)).await.unwrap().value);
        assert!(
            !store.write(move |tx| remove(tx, id)).await.unwrap().value,
            "gone already"
        );
        assert_eq!(names(store.read(list).await.unwrap()), ["Apology"]);
    }
}
