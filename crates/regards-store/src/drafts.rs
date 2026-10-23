//! Drafts: messages being written (Phase 2). A draft with an outbox entry is
//! being sent and cannot change until that entry is taken back.

use crate::StoreError;
use regards_core::headers::Person;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub id: i64,
    pub account_id: String,
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
    pub bcc: Vec<Person>,
    pub subject: String,
    /// The text; with `html`, its plain-text version.
    pub body: String,
    /// The formatted version, sanitised, if it was written with formatting.
    pub html: Option<String>,
    /// The Message-ID it answers.
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// The stored message it answers, to mark answered once sent.
    pub replying_to: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    /// An outbox entry exists: the draft is frozen.
    pub sending: bool,
    /// Who it is from; the account's default identity when `None`.
    pub identity_id: Option<i64>,
}

/// What a draft says; everything but its id and times.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Content {
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
    pub bcc: Vec<Person>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub replying_to: Option<i64>,
    pub identity_id: Option<i64>,
    /// Its formatted version, `body` being the plain one; none for plain text.
    pub html: Option<String>,
}

/// A new draft. It gets the account's automatic Cc or Bcc, if it has one
/// and the address is not addressed already.
pub fn create(tx: &Transaction<'_>, account_id: &str, content: &Content, now: i64) -> Result<i64, StoreError> {
    let (mut cc, mut bcc) = (content.cc.clone(), content.bcc.clone());
    if let Some(copy) = crate::accounts::auto_copy(tx, account_id)? {
        copy.apply(&content.to, &mut cc, &mut bcc);
    }
    tx.execute(
        "INSERT INTO drafts (account_id, to_json, cc_json, bcc_json, subject, body, in_reply_to, references_json, \
         replying_to, created_at, updated_at, identity_id, html) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?11, ?12)",
        params![
            account_id,
            json(&content.to),
            json(&cc),
            json(&bcc),
            content.subject,
            content.body,
            content.in_reply_to,
            json(&content.references),
            content.replying_to,
            now,
            content.identity_id,
            content.html,
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Send it from another identity, with `body` (its signature swapped).
/// Refused while it is being sent.
pub fn set_identity(tx: &Transaction<'_>, id: i64, identity_id: i64, body: &str, now: i64) -> Result<(), StoreError> {
    if get(tx, id)?.is_none_or(|draft| draft.sending) {
        return Err(StoreError::Conflict(
            "this message is being sent; undo or cancel it first".into(),
        ));
    }
    tx.execute(
        "UPDATE drafts SET identity_id = ?2, body = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, identity_id, body, now],
    )?;
    Ok(())
}

/// Send a draft from another account: `identity_id` is one of its
/// addresses. Its copy on the old account's server is to be deleted, and
/// the new account's Drafts folder gets one in time. Refused while it is
/// being sent.
pub fn move_to(
    tx: &Transaction<'_>,
    id: i64,
    account_id: &str,
    identity_id: i64,
    body: &str,
    now: i64,
) -> Result<(), StoreError> {
    let Some(draft) = get(tx, id)?.filter(|draft| !draft.sending) else {
        return Err(StoreError::Conflict(
            "this message is being sent; undo or cancel it first".into(),
        ));
    };
    let copy: (Option<i64>, Option<u32>) = tx.query_row(
        "SELECT server_mailbox_id, server_uid FROM drafts WHERE id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if let (Some(mailbox_id), Some(uid)) = copy {
        crate::draft_sync::queue_delete(tx, &draft.account_id, mailbox_id, uid)?;
    }
    tx.execute(
        "UPDATE drafts SET account_id = ?2, identity_id = ?3, body = ?4, updated_at = ?5, \
         server_message_id = NULL, server_mailbox_id = NULL, server_uid = NULL, uploaded_at = NULL \
         WHERE id = ?1",
        params![id, account_id, identity_id, body, now],
    )?;
    Ok(())
}

/// What the person writing can change: recipients, subject and text.
#[derive(Debug, Clone, Copy)]
pub struct Edit<'a> {
    pub to: &'a [Person],
    pub cc: &'a [Person],
    pub bcc: &'a [Person],
    pub subject: &'a str,
    pub body: &'a str,
    pub html: Option<&'a str>,
}

/// Change what the draft says. Refused while it is being sent.
pub fn update(tx: &Transaction<'_>, id: i64, edit: &Edit<'_>, now: i64) -> Result<(), StoreError> {
    let Edit {
        to,
        cc,
        bcc,
        subject,
        body,
        html,
    } = *edit;
    let Some(draft) = get(tx, id)? else {
        return Err(StoreError::Conflict("no such draft".into()));
    };
    if draft.sending {
        return Err(StoreError::Conflict(
            "this message is being sent; undo or cancel it first".into(),
        ));
    }
    tx.execute(
        "UPDATE drafts SET to_json = ?2, cc_json = ?3, bcc_json = ?4, subject = ?5, body = ?6, updated_at = ?7, \
         html = ?8 WHERE id = ?1",
        params![id, json(to), json(cc), json(bcc), subject, body, now, html],
    )?;
    Ok(())
}

const COLUMNS: &str = "d.id, d.account_id, d.to_json, d.cc_json, d.bcc_json, d.subject, d.body, d.in_reply_to, \
    d.references_json, d.replying_to, d.created_at, d.updated_at, \
    EXISTS (SELECT 1 FROM outbox o WHERE o.draft_id = d.id), d.html, d.identity_id";

pub fn get(conn: &Connection, id: i64) -> Result<Option<Draft>, StoreError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM drafts d WHERE d.id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .transpose()
}

/// Drafts being written (not those in the outbox), most recently changed
/// first: one account's, or every account's.
pub fn list(conn: &Connection, account_id: Option<&str>) -> Result<Vec<Draft>, StoreError> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM drafts d WHERE (?1 IS NULL OR d.account_id = ?1) \
         AND NOT EXISTS (SELECT 1 FROM outbox o WHERE o.draft_id = d.id) \
         ORDER BY d.updated_at DESC, d.id DESC"
    ))?;
    let drafts = statement
        .query_map([account_id], from_row)?
        .map(|row| row?)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(drafts)
}

/// Discard a draft. Refused while it is being sent.
pub fn delete(tx: &Transaction<'_>, id: i64) -> Result<bool, StoreError> {
    if get(tx, id)?.is_some_and(|draft| draft.sending) {
        return Err(StoreError::Conflict(
            "this message is being sent; undo or cancel it first".into(),
        ));
    }
    Ok(tx.execute("DELETE FROM drafts WHERE id = ?1", [id])? > 0)
}

/// A file attached to a draft; its bytes are the blob `blob_hash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub id: i64,
    pub blob_hash: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
    /// An image in the text, which the HTML refers to as `cid:`.
    pub content_id: Option<String>,
}

/// Attach a stored blob. Refused while the draft is being sent.
pub fn attach(
    tx: &Transaction<'_>,
    draft_id: i64,
    blob_hash: &str,
    name: &str,
    mime: &str,
    size: u64,
) -> Result<i64, StoreError> {
    attach_as(tx, draft_id, blob_hash, name, mime, size, None)
}

/// [`attach`] an image for the draft's text, by its Content-ID.
pub fn attach_inline(
    tx: &Transaction<'_>,
    draft_id: i64,
    file: (&str, &str, &str, u64),
    content_id: &str,
) -> Result<i64, StoreError> {
    let (blob_hash, name, mime, size) = file;
    attach_as(tx, draft_id, blob_hash, name, mime, size, Some(content_id))
}

fn attach_as(
    tx: &Transaction<'_>,
    draft_id: i64,
    blob_hash: &str,
    name: &str,
    mime: &str,
    size: u64,
    content_id: Option<&str>,
) -> Result<i64, StoreError> {
    editable(tx, draft_id)?;
    tx.execute(
        "INSERT INTO draft_attachments (draft_id, blob_hash, name, mime, size, content_id, added_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, CAST(unixepoch('subsec') * 1000 AS INTEGER))",
        params![draft_id, blob_hash, name, mime, size as i64, content_id],
    )?;
    tx.execute(
        "UPDATE drafts SET updated_at = updated_at + 1 WHERE id = ?1",
        [draft_id],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Take a file off a draft. Refused while the draft is being sent.
pub fn detach(tx: &Transaction<'_>, draft_id: i64, attachment_id: i64) -> Result<bool, StoreError> {
    editable(tx, draft_id)?;
    let removed = tx.execute(
        "DELETE FROM draft_attachments WHERE id = ?1 AND draft_id = ?2",
        params![attachment_id, draft_id],
    )? > 0;
    if removed {
        tx.execute(
            "UPDATE drafts SET updated_at = updated_at + 1 WHERE id = ?1",
            [draft_id],
        )?;
    }
    Ok(removed)
}

/// Take off the draft's images its text no longer has (`html`: `None`
/// when it has no formatting, so no images), if added before `before`
/// (Unix ms): a save begun before an image was put in the text does not
/// have it yet. How many went.
pub fn drop_unused_images(
    tx: &Transaction<'_>,
    draft_id: i64,
    html: Option<&str>,
    before: i64,
) -> Result<usize, StoreError> {
    let old: Vec<(i64, String)> = {
        let mut statement = tx.prepare_cached(
            "SELECT id, content_id FROM draft_attachments \
             WHERE draft_id = ?1 AND content_id IS NOT NULL AND (added_at IS NULL OR added_at < ?2)",
        )?;
        let rows = statement.query_map(params![draft_id, before], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let unused: Vec<i64> = old
        .into_iter()
        .filter(|(_, cid)| !html.is_some_and(|html| html.contains(&format!("cid:{cid}"))))
        .map(|(id, _)| id)
        .collect();
    let mut statement = tx.prepare_cached("DELETE FROM draft_attachments WHERE id = ?1")?;
    for id in &unused {
        statement.execute([id])?;
    }
    Ok(unused.len())
}

/// A draft's files, in the order they were attached.
pub fn attachments(conn: &Connection, draft_id: i64) -> Result<Vec<Attachment>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT id, blob_hash, name, mime, size, content_id FROM draft_attachments WHERE draft_id = ?1 ORDER BY id",
    )?;
    let rows = statement
        .query_map([draft_id], |row| {
            Ok(Attachment {
                id: row.get(0)?,
                blob_hash: row.get(1)?,
                name: row.get(2)?,
                mime: row.get(3)?,
                size: row.get::<_, i64>(4)? as u64,
                content_id: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn editable(tx: &Transaction<'_>, draft_id: i64) -> Result<(), StoreError> {
    match get(tx, draft_id)? {
        None => Err(StoreError::Conflict("no such draft".into())),
        Some(draft) if draft.sending => Err(StoreError::Conflict(
            "this message is being sent; undo or cancel it first".into(),
        )),
        Some(_) => Ok(()),
    }
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Result<Draft, StoreError>> {
    let texts: [String; 4] = [row.get(2)?, row.get(3)?, row.get(4)?, row.get(8)?];
    let parse = || -> Result<Draft, StoreError> {
        Ok(Draft {
            id: row.get(0)?,
            account_id: row.get(1)?,
            to: from_json(&texts[0])?,
            cc: from_json(&texts[1])?,
            bcc: from_json(&texts[2])?,
            subject: row.get(5)?,
            body: row.get(6)?,
            in_reply_to: row.get(7)?,
            references: from_json(&texts[3])?,
            replying_to: row.get(9)?,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
            sending: row.get(12)?,
            html: row.get(13)?,
            identity_id: row.get(14)?,
        })
    };
    Ok(parse())
}

fn json<T: serde::Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).expect("plain data always serializes")
}

fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, StoreError> {
    serde_json::from_str(text).map_err(|error| StoreError::Corrupt(format!("draft: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use regards_core::paths::DataDirs;

    #[tokio::test]
    async fn images_taken_out_of_the_text_go_once_they_have_been_there_a_while() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let draft = store
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
                let draft = create(tx, "a1", &Content::default(), 1_000)?;
                attach(tx, draft, "h1", "plan.pdf", "application/pdf", 10)?;
                attach_inline(tx, draft, ("h2", "kept.png", "image/png", 20), "kept@regards")?;
                attach_inline(tx, draft, ("h3", "gone.png", "image/png", 30), "gone@regards")?;
                Ok(draft)
            })
            .await
            .unwrap()
            .value;
        let names = |files: Vec<Attachment>| files.into_iter().map(|f| (f.name, f.content_id)).collect::<Vec<_>>();
        let html = r#"<p><img src="cid:kept@regards" alt=""></p>"#;

        // Just added: kept, though the save does not have it yet.
        let dropped = store
            .write(move |tx| drop_unused_images(tx, draft, Some(html), 0))
            .await
            .unwrap()
            .value;
        assert_eq!(dropped, 0);
        // A while later: the one the text no longer has goes; files stay.
        let dropped = store
            .write(move |tx| drop_unused_images(tx, draft, Some(html), i64::MAX))
            .await
            .unwrap()
            .value;
        assert_eq!(dropped, 1);
        let left = store.read(move |conn| attachments(conn, draft)).await.unwrap();
        assert_eq!(
            names(left),
            [
                ("plan.pdf".to_owned(), None),
                ("kept.png".to_owned(), Some("kept@regards".to_owned()))
            ]
        );
        // No formatting: no text for images.
        store
            .write(move |tx| drop_unused_images(tx, draft, None, i64::MAX))
            .await
            .unwrap();
        let left = store.read(move |conn| attachments(conn, draft)).await.unwrap();
        assert_eq!(names(left), [("plan.pdf".to_owned(), None)]);
    }
}
