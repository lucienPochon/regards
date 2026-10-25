//! Templates (plan §5.9): a message to start from, by name, kept on this
//! device for every account: its subject, text, recipients and files. The
//! composer puts one into a draft.

use crate::StoreError;
use crate::drafts;
use regards_core::headers::Person;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub id: i64,
    pub name: String,
    pub subject: String,
    /// The text; with `html`, its plain-text version.
    pub body: String,
    pub html: Option<String>,
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
    pub bcc: Vec<Person>,
    pub updated_at: i64,
}

/// What a template says (its files are kept apart: [`files`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    pub name: String,
    pub subject: String,
    pub body: String,
    pub html: Option<String>,
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
    pub bcc: Vec<Person>,
}

/// A file a template keeps; its bytes are the blob `blob_hash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub id: i64,
    pub blob_hash: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
    /// An image in the template's text, which its HTML names as `cid:`.
    pub content_id: Option<String>,
}

const COLUMNS: &str = "id, name, subject, body, html, updated_at, to_json, cc_json, bcc_json";

fn from_row(row: &Row<'_>) -> rusqlite::Result<Template> {
    let people = |at: usize| -> rusqlite::Result<Vec<Person>> {
        let text: String = row.get(at)?;
        serde_json::from_str(&text).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(at, rusqlite::types::Type::Text, Box::new(error))
        })
    };
    Ok(Template {
        id: row.get(0)?,
        name: row.get(1)?,
        subject: row.get(2)?,
        body: row.get(3)?,
        html: row.get(4)?,
        updated_at: row.get(5)?,
        to: people(6)?,
        cc: people(7)?,
        bcc: people(8)?,
    })
}

fn json(people: &[Person]) -> String {
    serde_json::to_string(people).expect("people always serialize")
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
        "INSERT INTO templates (name, subject, body, html, to_json, cc_json, bcc_json, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            fields.name,
            fields.subject,
            fields.body,
            fields.html,
            json(&fields.to),
            json(&fields.cc),
            json(&fields.bcc),
            now
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// `false` if there is no such template.
pub fn update(tx: &Transaction<'_>, id: i64, fields: &Fields, now: i64) -> Result<bool, StoreError> {
    Ok(tx.execute(
        "UPDATE templates SET name = ?2, subject = ?3, body = ?4, html = ?5, to_json = ?6, cc_json = ?7, \
         bcc_json = ?8, updated_at = ?9 WHERE id = ?1",
        params![
            id,
            fields.name,
            fields.subject,
            fields.body,
            fields.html,
            json(&fields.to),
            json(&fields.cc),
            json(&fields.bcc),
            now
        ],
    )? == 1)
}

/// Its files go with it; their bytes once nothing else names them.
pub fn remove(tx: &Transaction<'_>, id: i64) -> Result<bool, StoreError> {
    Ok(tx.execute("DELETE FROM templates WHERE id = ?1", [id])? == 1)
}

/// The files a template keeps, as they were added.
pub fn files(conn: &Connection, template_id: i64) -> Result<Vec<File>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT id, blob_hash, name, mime, size, content_id FROM template_attachments WHERE template_id = ?1 ORDER BY id",
    )?;
    let rows = statement.query_map([template_id], |row| {
        Ok(File {
            id: row.get(0)?,
            blob_hash: row.get(1)?,
            name: row.get(2)?,
            mime: row.get(3)?,
            size: row.get::<_, i64>(4)?.max(0) as u64,
            content_id: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Keep a draft's files (and the images in its text) with a template: the
/// same blobs, not copies of the bytes. How many.
pub fn keep_draft_files(tx: &Transaction<'_>, template_id: i64, draft_id: i64) -> Result<usize, StoreError> {
    Ok(tx.execute(
        "INSERT INTO template_attachments (template_id, blob_hash, name, mime, size, content_id) \
         SELECT ?1, blob_hash, name, mime, size, content_id FROM draft_attachments WHERE draft_id = ?2 ORDER BY id",
        [template_id, draft_id],
    )?)
}

/// `false` if the template has no such file.
pub fn remove_file(tx: &Transaction<'_>, template_id: i64, file_id: i64) -> Result<bool, StoreError> {
    Ok(tx.execute(
        "DELETE FROM template_attachments WHERE id = ?1 AND template_id = ?2",
        [file_id, template_id],
    )? == 1)
}

/// Put a template's files on a draft, but those it has already: an image
/// with the same Content-ID, a file with the same bytes and name. How many
/// went on. Refused while the draft is being sent.
pub fn put_files_on(tx: &Transaction<'_>, template_id: i64, draft_id: i64) -> Result<usize, StoreError> {
    let has = drafts::attachments(tx, draft_id)?;
    let mut put = 0;
    for file in files(tx, template_id)? {
        let already = has
            .iter()
            .any(|attached| match (&file.content_id, &attached.content_id) {
                (Some(wanted), Some(there)) => wanted == there,
                (None, None) => attached.blob_hash == file.blob_hash && attached.name == file.name,
                _ => false,
            });
        if already {
            continue;
        }
        match &file.content_id {
            Some(content_id) => drafts::attach_inline(
                tx,
                draft_id,
                (&file.blob_hash, &file.name, &file.mime, file.size),
                content_id,
            )?,
            None => drafts::attach(tx, draft_id, &file.blob_hash, &file.name, &file.mime, file.size)?,
        };
        put += 1;
    }
    Ok(put)
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
            ..Fields::default()
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

    #[tokio::test]
    async fn a_template_keeps_recipients_and_files_and_puts_them_on_a_draft_once() {
        use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
        use crate::drafts::Content;
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let (kept, put, again, referenced) = store
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
                        display_name: "Ada".into(),
                        email: "ada@example.test".into(),
                        auth_kind: AuthKind::Password,
                        username: "ada".into(),
                        secret_key: "k".into(),
                        imap: server.clone(),
                        smtp: server,
                        imap_capabilities: vec![],
                    },
                )?;
                let written = drafts::create(tx, "a1", &Content::default(), 1)?;
                drafts::attach(tx, written, "h1", "terms.pdf", "application/pdf", 10)?;
                drafts::attach_inline(tx, written, ("h2", "logo.png", "image/png", 20), "logo@regards")?;
                let fields = Fields {
                    name: "Booking".into(),
                    to: vec![Person {
                        name: Some("Desk".into()),
                        address: Some("desk@example.test".into()),
                    }],
                    ..Fields::default()
                };
                let template = add(tx, &fields, 2)?;
                let kept = keep_draft_files(tx, template, written)?;
                drafts::delete(tx, written)?;
                let fresh = drafts::create(tx, "a1", &Content::default(), 3)?;
                let put = put_files_on(tx, template, fresh)?;
                let again = put_files_on(tx, template, fresh)?;
                let to = get(tx, template)?.expect("kept").to;
                assert_eq!(to[0].address.as_deref(), Some("desk@example.test"));
                Ok((kept, put, again, crate::blobs::referenced(tx)?))
            })
            .await
            .unwrap()
            .value;
        assert_eq!((kept, put, again), (2, 2, 0), "kept, put on, and not twice");
        assert!(referenced.contains("h1") && referenced.contains("h2"), "{referenced:?}");
    }
}
