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
    pub body: String,
    /// The Message-ID it answers.
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// The stored message it answers, to mark answered once sent.
    pub replying_to: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    /// An outbox entry exists: the draft is frozen.
    pub sending: bool,
}

/// What a draft says; everything but its identity and times.
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
}

pub fn create(tx: &Transaction<'_>, account_id: &str, content: &Content, now: i64) -> Result<i64, StoreError> {
    tx.execute(
        "INSERT INTO drafts (account_id, to_json, cc_json, bcc_json, subject, body, in_reply_to, references_json, \
         replying_to, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            account_id,
            json(&content.to),
            json(&content.cc),
            json(&content.bcc),
            content.subject,
            content.body,
            content.in_reply_to,
            json(&content.references),
            content.replying_to,
            now,
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

/// What the person writing can change: recipients, subject and text.
#[derive(Debug, Clone, Copy)]
pub struct Edit<'a> {
    pub to: &'a [Person],
    pub cc: &'a [Person],
    pub bcc: &'a [Person],
    pub subject: &'a str,
    pub body: &'a str,
}

/// Change what the draft says. Refused while it is being sent.
pub fn update(tx: &Transaction<'_>, id: i64, edit: &Edit<'_>, now: i64) -> Result<(), StoreError> {
    let Edit {
        to,
        cc,
        bcc,
        subject,
        body,
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
        "UPDATE drafts SET to_json = ?2, cc_json = ?3, bcc_json = ?4, subject = ?5, body = ?6, updated_at = ?7 \
         WHERE id = ?1",
        params![id, json(to), json(cc), json(bcc), subject, body, now],
    )?;
    Ok(())
}

const COLUMNS: &str = "d.id, d.account_id, d.to_json, d.cc_json, d.bcc_json, d.subject, d.body, d.in_reply_to, \
    d.references_json, d.replying_to, d.created_at, d.updated_at, \
    EXISTS (SELECT 1 FROM outbox o WHERE o.draft_id = d.id)";

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
