//! Messages: one row per message per mailbox (IMAP mailbox and UID).

use crate::StoreError;
use regards_core::headers::{Headers, Person};
use regards_core::smart::Section;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageFlags {
    pub seen: bool,
    pub answered: bool,
    pub flagged: bool,
    pub draft: bool,
    pub deleted: bool,
    pub keywords: Vec<String>,
}

/// A message fetched from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMessage {
    pub uid: u32,
    pub modseq: Option<u64>,
    pub flags: MessageFlags,
    /// INTERNALDATE, Unix time in milliseconds.
    pub received_at: Option<i64>,
    pub size: Option<u32>,
    pub headers: Headers,
}

/// New flags for a stored message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagChange {
    pub uid: u32,
    pub modseq: Option<u64>,
    pub flags: MessageFlags,
}

/// A stored message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub mailbox_id: i64,
    pub uid: u32,
    pub modseq: Option<u64>,
    pub flags: MessageFlags,
    pub received_at: i64,
    pub size: Option<u32>,
    pub sent_at: Option<i64>,
    pub subject: String,
    pub from: Option<Person>,
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
    pub message_id: Option<String>,
    /// Hash of the downloaded source in `blobs/`, once it has been fetched.
    pub blob_hash: Option<String>,
    /// The folder this message is being moved to, until the server has done it.
    pub moving_to: Option<i64>,
    /// Its conversation (ADR-0009 §5); `None` until threaded.
    pub thread_id: Option<i64>,
    /// Its Smart Inbox section; `None` until sorted.
    pub section: Option<Section>,
    /// It has attached files; `None` until its structure is known.
    pub has_attachments: Option<bool>,
    /// The start of its text, whitespace collapsed; `None` until fetched.
    pub preview: Option<String>,
    /// Its place in the list it was read from: `received_at`, except in the
    /// inbox for what came back to it (a snooze that ended, a reminder).
    pub listed_at: i64,
}

/// Where a page of a message list ends: list strictly older messages next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub received_at: i64,
    pub id: i64,
}

/// Insert messages, or replace them if a row for the same UID exists.
pub fn upsert(tx: &Transaction<'_>, mailbox_id: i64, messages: &[NewMessage]) -> Result<usize, StoreError> {
    let account_id: String = tx.query_row("SELECT account_id FROM mailboxes WHERE id = ?1", [mailbox_id], |r| {
        r.get(0)
    })?;
    let mut statement = tx.prepare_cached(
        "INSERT INTO messages (mailbox_id, uid, modseq, seen, answered, flagged, draft, deleted, keywords, \
         received_at, size, sent_at, subject, from_name, from_address, to_json, cc_json, message_id_header, \
         in_reply_to, references_ids, traits) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21) \
         ON CONFLICT (mailbox_id, uid) DO UPDATE SET modseq = excluded.modseq, seen = excluded.seen, \
         answered = excluded.answered, flagged = excluded.flagged, draft = excluded.draft, \
         deleted = excluded.deleted, keywords = excluded.keywords, received_at = excluded.received_at, \
         size = excluded.size, sent_at = excluded.sent_at, subject = excluded.subject, \
         from_name = excluded.from_name, from_address = excluded.from_address, to_json = excluded.to_json, \
         cc_json = excluded.cc_json, message_id_header = excluded.message_id_header, \
         in_reply_to = excluded.in_reply_to, references_ids = excluded.references_ids, traits = excluded.traits \
         RETURNING id",
    )?;
    let own = crate::smart::own_addresses(tx, &account_id)?;
    for message in messages {
        let headers = &message.headers;
        let received_at = message.received_at.or(headers.date).unwrap_or(0);
        let row: i64 = statement.query_row(
            params![
                mailbox_id,
                message.uid,
                message.modseq.map(|m| m as i64),
                message.flags.seen,
                message.flags.answered,
                message.flags.flagged,
                message.flags.draft,
                message.flags.deleted,
                json(&message.flags.keywords),
                received_at,
                message.size,
                headers.date,
                headers.subject,
                headers.from.as_ref().and_then(|p| p.name.clone()),
                headers.from.as_ref().and_then(|p| p.address.clone()),
                json(&headers.to),
                json(&headers.cc),
                headers.message_id,
                json(&headers.in_reply_to),
                json(&headers.references),
                headers.traits,
            ],
            |r| r.get(0),
        )?;
        let mut references = headers.in_reply_to.clone();
        references.extend(headers.references.iter().cloned());
        crate::threads::assign(tx, &account_id, row, headers.message_id.as_deref(), &references)?;
        // Mail from the account itself: everyone it went to is someone it writes to.
        let from = headers.from.as_ref().and_then(|p| p.address.as_deref());
        if from.is_some_and(|address| own.iter().any(|mine| mine.eq_ignore_ascii_case(address))) {
            let recipients: Vec<String> = headers
                .to
                .iter()
                .chain(&headers.cc)
                .filter_map(|p| p.address.clone())
                .collect();
            crate::smart::note_written_to(tx, &account_id, &recipients, received_at)?;
        }
        crate::smart::reclassify(tx, &account_id, &own, row)?;
    }
    Ok(messages.len())
}

/// Apply flag changes; returns how many stored messages actually changed.
pub fn apply_flags(tx: &Transaction<'_>, mailbox_id: i64, changes: &[FlagChange]) -> Result<usize, StoreError> {
    let mut statement = tx.prepare_cached(
        "UPDATE messages SET seen = ?3, answered = ?4, flagged = ?5, draft = ?6, deleted = ?7, keywords = ?8, \
         modseq = COALESCE(?9, modseq) \
         WHERE mailbox_id = ?1 AND uid = ?2 AND (seen, answered, flagged, draft, deleted, keywords) \
         IS NOT (?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    let mut changed = 0;
    for change in changes {
        changed += statement.execute(params![
            mailbox_id,
            change.uid,
            change.flags.seen,
            change.flags.answered,
            change.flags.flagged,
            change.flags.draft,
            change.flags.deleted,
            json(&change.flags.keywords),
            change.modseq.map(|m| m as i64),
        ])?;
    }
    Ok(changed)
}

/// Unread messages per folder of an account, by folder id, where it shows
/// (a message being moved counts in its target). The inbox leaves out what
/// is set aside or snoozed past `now`, as it does in its list.
/// The unread messages filed in a folder (not those on their way out of
/// it), of one Smart Inbox section if asked: for "Mark all as read".
pub fn unread_in(conn: &Connection, mailbox_id: i64, section: Option<Section>) -> Result<Vec<i64>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT id FROM messages WHERE mailbox_id = ?1 AND seen = 0 AND deleted = 0 AND moving_to IS NULL \
         AND (?2 IS NULL OR section = ?2) ORDER BY uid",
    )?;
    let ids = statement
        .query_map(params![mailbox_id, section.map(Section::as_str)], |row| row.get(0))?
        .collect::<Result<Vec<i64>, _>>()?;
    Ok(ids)
}

pub fn unread_counts(
    conn: &Connection,
    account_id: &str,
    inbox_id: Option<i64>,
    now: i64,
) -> Result<std::collections::HashMap<i64, u32>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT COALESCE(m.moving_to, m.mailbox_id) AS shown, COUNT(*) FROM messages m \
         WHERE m.seen = 0 AND m.deleted = 0 AND m.mailbox_id IN (SELECT id FROM mailboxes WHERE account_id = ?1) \
         AND NOT (COALESCE(m.moving_to, m.mailbox_id) IS ?2 AND EXISTS (SELECT 1 FROM marks k \
             WHERE k.account_id = ?1 AND k.message_id = m.message_id_header \
             AND (k.aside_at IS NOT NULL OR k.snoozed_until > ?3))) \
         GROUP BY shown",
    )?;
    let rows = statement.query_map(params![account_id, inbox_id, now], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Drafts on the server that were not written here (on another device, in
/// webmail): messages in Drafts folders that are no draft's copy, newest
/// first; of one account, or of all.
pub fn drafts_elsewhere(conn: &Connection, account_id: Option<&str>) -> Result<Vec<Message>, StoreError> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM messages WHERE deleted = 0 AND moving_to IS NULL AND mailbox_id IN ( \
             SELECT id FROM mailboxes WHERE role = 'drafts' AND (?1 IS NULL OR account_id = ?1)) \
         AND NOT EXISTS (SELECT 1 FROM drafts d WHERE d.server_mailbox_id = messages.mailbox_id \
             AND d.server_uid = messages.uid) \
         AND NOT EXISTS (SELECT 1 FROM drafts d JOIN mailboxes b ON b.id = messages.mailbox_id \
             WHERE d.account_id = b.account_id AND d.server_message_id = messages.message_id_header) \
         ORDER BY received_at DESC, id DESC LIMIT 200"
    ))?;
    let rows = statement.query_map([account_id], from_row)?;
    rows.map(|row| row?).collect()
}

/// What a list row shows beyond the headers: whether the message carries
/// files, and the start of its text.
pub fn set_details(
    tx: &Transaction<'_>,
    mailbox_id: i64,
    uid: u32,
    has_attachments: bool,
    preview: Option<&str>,
) -> Result<(), StoreError> {
    tx.prepare_cached("UPDATE messages SET has_attachments = ?3, preview = ?4 WHERE mailbox_id = ?1 AND uid = ?2")?
        .execute(params![mailbox_id, uid, has_attachments, preview])?;
    Ok(())
}

/// Messages of a folder whose details were not fetched yet, newest first.
pub fn details_unknown(conn: &Connection, mailbox_id: i64, limit: u32) -> Result<Vec<u32>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT uid FROM messages WHERE mailbox_id = ?1 AND has_attachments IS NULL ORDER BY uid DESC LIMIT ?2",
    )?;
    let rows = statement.query_map(params![mailbox_id, limit], |row| row.get(0))?;
    Ok(rows.collect::<Result<Vec<u32>, _>>()?)
}

/// How many messages each conversation has (one per Message-ID, not deleted).
pub fn thread_sizes(conn: &Connection, threads: &[i64]) -> Result<std::collections::HashMap<i64, u32>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT COUNT(DISTINCT COALESCE(message_id_header, '#' || id)) FROM messages \
         WHERE thread_id = ?1 AND deleted = 0",
    )?;
    let mut sizes = std::collections::HashMap::new();
    for thread in threads {
        sizes.insert(*thread, statement.query_row([thread], |row| row.get(0))?);
    }
    Ok(sizes)
}

/// UIDs stored for a mailbox, ascending.
pub fn uids(conn: &Connection, mailbox_id: i64) -> Result<Vec<u32>, StoreError> {
    let mut statement = conn.prepare_cached("SELECT uid FROM messages WHERE mailbox_id = ?1 ORDER BY uid")?;
    let uids = statement
        .query_map([mailbox_id], |row| row.get(0))?
        .collect::<Result<Vec<u32>, _>>()?;
    Ok(uids)
}

/// Remove the given UIDs; returns how many were removed.
pub fn delete_uids(tx: &Transaction<'_>, mailbox_id: i64, uids: &[u32]) -> Result<usize, StoreError> {
    let mut statement = tx.prepare_cached("DELETE FROM messages WHERE mailbox_id = ?1 AND uid = ?2")?;
    let mut removed = 0;
    for uid in uids {
        removed += statement.execute(params![mailbox_id, uid])?;
    }
    Ok(removed)
}

/// Mark `uids` of `mailbox_id` as being moved to `target` (or no longer, with `None`).
pub fn set_moving_to(
    tx: &Transaction<'_>,
    mailbox_id: i64,
    uids: &[u32],
    target: Option<i64>,
) -> Result<(), StoreError> {
    let mut statement = tx.prepare_cached("UPDATE messages SET moving_to = ?3 WHERE mailbox_id = ?1 AND uid = ?2")?;
    for uid in uids {
        statement.execute(params![mailbox_id, uid, target])?;
    }
    Ok(())
}

/// The server moved `uids` from `source` to `target`. With the new UIDs
/// (COPYUID) the rows move in place and keep their ids, so undo still finds
/// them; without, they are removed and the next sync of `target` brings them.
pub fn complete_move(
    tx: &Transaction<'_>,
    source: i64,
    target: i64,
    uids: &[u32],
    new_uids: Option<&[(u32, u32)]>,
) -> Result<(), StoreError> {
    if let Some(pairs) = new_uids {
        let mut statement = tx.prepare_cached(
            "UPDATE OR IGNORE messages SET mailbox_id = ?2, uid = ?4, moving_to = NULL \n             WHERE mailbox_id = ?1 AND uid = ?3 AND moving_to = ?2",
        )?;
        for (old, new) in pairs {
            statement.execute(params![source, target, old, new])?;
        }
    }
    // Whatever did not move in place (no COPYUID, or a sync already fetched it there).
    let mut statement =
        tx.prepare_cached("DELETE FROM messages WHERE mailbox_id = ?1 AND uid = ?2 AND moving_to = ?3")?;
    for uid in uids {
        statement.execute(params![source, uid, target])?;
    }
    Ok(())
}

/// Remove every message of a mailbox, for example after a UIDVALIDITY change.
pub fn clear(tx: &Transaction<'_>, mailbox_id: i64) -> Result<usize, StoreError> {
    Ok(tx.execute("DELETE FROM messages WHERE mailbox_id = ?1", [mailbox_id])?)
}

/// The columns [`from_row`] reads, in its order.
const COLUMNS: &str = "id, mailbox_id, uid, modseq, seen, answered, flagged, draft, deleted, keywords, \
    received_at, size, sent_at, subject, from_name, from_address, to_json, cc_json, message_id_header, blob_hash, moving_to, thread_id, \
    section, has_attachments, preview";
/// How many [`COLUMNS`] there are: what a query selects after them starts here.
pub(crate) const COLUMN_COUNT: usize = 25;

/// [`COLUMNS`] of the table aliased `alias`.
pub(crate) fn columns(alias: &str) -> String {
    COLUMNS
        .split(',')
        .map(|column| format!("{alias}.{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Record where the downloaded source of message `id` is stored.
/// Inbox messages received since `since` (Unix ms), not downloaded yet and
/// at most `max_size` bytes, newest first: (id, account) to download ahead.
pub fn to_prefetch(conn: &Connection, since: i64, max_size: i64, limit: u32) -> Result<Vec<(i64, String)>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT m.id, b.account_id FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id \
         WHERE b.role = 'inbox' AND m.blob_hash IS NULL AND m.deleted = 0 AND m.moving_to IS NULL \
         AND m.received_at >= ?1 AND COALESCE(m.size, 0) <= ?2 \
         ORDER BY m.received_at DESC, m.id DESC LIMIT ?3",
    )?;
    let rows = statement.query_map(params![since, max_size, limit], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn set_blob(tx: &Transaction<'_>, id: i64, hash: &str) -> Result<(), StoreError> {
    tx.execute("UPDATE messages SET blob_hash = ?2 WHERE id = ?1", params![id, hash])?;
    Ok(())
}

/// The page query of [`list`]. A message being moved shows in its target
/// folder at once; one marked deleted (being deleted for good, or by another
/// client) is not shown. The two halves, the folder's own messages and those
/// on their way into it, are separate so that each walks its own index: with
/// an OR, a 100k-message folder took 55 ms per page instead of well under 1.
fn list_sql() -> String {
    format!(
        "SELECT * FROM (SELECT {COLUMNS} FROM messages \
             WHERE mailbox_id = ?1 AND moving_to IS NULL AND deleted = 0 AND (received_at, id) < (?2, ?3) \
             ORDER BY received_at DESC, id DESC LIMIT ?4) \
         UNION ALL \
         SELECT * FROM (SELECT {COLUMNS} FROM messages \
             WHERE moving_to = ?1 AND deleted = 0 AND (received_at, id) < (?2, ?3) \
             ORDER BY received_at DESC, id DESC LIMIT ?4) \
         ORDER BY received_at DESC, id DESC LIMIT ?4"
    )
}

/// The inbox's page query: [`list_sql`] without what account ?5 has set
/// aside or snoozed past ?6 (now), and with what came back to the inbox (a
/// snooze that ended, a reminder come due) listed by when it came back, so it
/// is on top. A reminder shows its message from wherever it is filed (Sent,
/// usually). Rows end with `listed_at`, the time they are listed by.
///
/// The third half walks the marks of what came back, newest first; a copy in
/// the inbox is preferred over one elsewhere, and copies on their way out are
/// left out.
///
/// With `section`, only that Smart Inbox section (?7): the first half then
/// walks `messages_by_section`.
fn inbox_sql(sections: &Sections<'_>) -> String {
    // Listed by its own time: not parked, and not come back since it arrived.
    const PLAIN: &str = "NOT EXISTS (SELECT 1 FROM marks k WHERE k.account_id = ?5 \
        AND k.message_id = messages.message_id_header AND (k.aside_at IS NOT NULL OR k.snoozed_until > ?6 \
        OR k.reminded_at IS NOT NULL OR k.surfaced_at > messages.received_at))";
    // Sections from ?7 on: those shown, then the one left out.
    let mut only = String::new();
    if !sections.only.is_empty() {
        let places: Vec<String> = (0..sections.only.len()).map(|i| format!("?{}", 7 + i)).collect();
        only = format!("AND messages.section IN ({})", places.join(", "));
    }
    if sections.except.is_some() {
        only.push_str(&format!(" AND messages.section IS NOT ?{}", 7 + sections.only.len()));
    }
    // A literal, so that unread pages can use the `messages_unseen` index.
    match sections.seen {
        Some(true) => only.push_str(" AND messages.seen = 1"),
        Some(false) => only.push_str(" AND messages.seen = 0"),
        None => {}
    }
    format!(
        "SELECT * FROM (SELECT {COLUMNS}, received_at AS listed_at FROM messages \
             WHERE mailbox_id = ?1 AND moving_to IS NULL AND deleted = 0 AND (received_at, id) < (?2, ?3) {only} \
             AND {PLAIN} ORDER BY received_at DESC, id DESC LIMIT ?4) \
         UNION ALL \
         SELECT * FROM (SELECT {COLUMNS}, received_at AS listed_at FROM messages \
             WHERE moving_to = ?1 AND deleted = 0 AND (received_at, id) < (?2, ?3) {only} \
             AND {PLAIN} ORDER BY received_at DESC, id DESC LIMIT ?4) \
         UNION ALL \
         SELECT * FROM (SELECT {COLUMNS}, k.surfaced_at AS listed_at FROM marks k \
             JOIN messages ON messages.id = ( \
                 SELECT m.id FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id \
                 WHERE b.account_id = k.account_id AND m.message_id_header = k.message_id AND m.deleted = 0 \
                 AND (m.moving_to IS NULL OR m.moving_to = ?1) \
                 AND (COALESCE(m.moving_to, m.mailbox_id) = ?1 OR k.reminded_at IS NOT NULL) \
                 ORDER BY COALESCE(m.moving_to, m.mailbox_id) = ?1 DESC, m.id LIMIT 1) \
             WHERE k.account_id = ?5 AND k.surfaced_at IS NOT NULL AND k.aside_at IS NULL \
             AND (k.snoozed_until IS NULL OR k.snoozed_until <= ?6) \
             AND (k.reminded_at IS NOT NULL OR k.surfaced_at > messages.received_at) \
             AND (k.surfaced_at, messages.id) < (?2, ?3) {only} \
             ORDER BY k.surfaced_at DESC, messages.id DESC LIMIT ?4) \
         ORDER BY listed_at DESC, id DESC LIMIT ?4"
    )
}

/// A page of a mailbox's messages, newest first, older than `before` if given.
pub fn list(
    conn: &Connection,
    mailbox_id: i64,
    limit: u32,
    before: Option<Cursor>,
) -> Result<Vec<Message>, StoreError> {
    list_without_parked(conn, mailbox_id, limit, before, None)
}

/// [`list`], as the inbox of `parked` (an account, and the time now) is
/// shown: without what is set aside or snoozed, and with what came back on
/// top ([`inbox_sql`]). Each message's `listed_at` is its place in the list,
/// and the cursor's `received_at` is the last one's `listed_at`.
pub fn list_without_parked(
    conn: &Connection,
    mailbox_id: i64,
    limit: u32,
    before: Option<Cursor>,
    parked: Option<(&str, i64)>,
) -> Result<Vec<Message>, StoreError> {
    let Some((account, now)) = parked else {
        let (received_at, id) = before.map_or((i64::MAX, i64::MAX), |c| (c.received_at, c.id));
        let messages = conn
            .prepare_cached(&list_sql())?
            .query_map(params![mailbox_id, received_at, id, limit], from_row)?
            .map(|row| row?)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(messages);
    };
    inbox_page(conn, mailbox_id, limit, before, account, now, &Sections::default())
}

/// A page of an inbox, as [`list_without_parked`] shows it, of one Smart
/// Inbox section or all.
pub fn inbox_page(
    conn: &Connection,
    mailbox_id: i64,
    limit: u32,
    before: Option<Cursor>,
    account: &str,
    now: i64,
    sections: &Sections<'_>,
) -> Result<Vec<Message>, StoreError> {
    use rusqlite::types::Value;
    let (received_at, id) = before.map_or((i64::MAX, i64::MAX), |c| (c.received_at, c.id));
    let listed = |row: &Row<'_>| {
        let listed_at: i64 = row.get(LISTED_AT)?;
        Ok(from_row(row)?.map(|message| Message { listed_at, ..message }))
    };
    let mut values: Vec<Value> = vec![
        mailbox_id.into(),
        received_at.into(),
        id.into(),
        i64::from(limit).into(),
        account.to_owned().into(),
        now.into(),
    ];
    values.extend(sections.only.iter().map(|s| Value::from(s.as_str().to_owned())));
    values.extend(sections.except.map(|s| Value::from(s.as_str().to_owned())));
    let mut statement = conn.prepare_cached(&inbox_sql(sections))?;
    let rows = statement.query_map(rusqlite::params_from_iter(values), listed)?;
    rows.map(|row| row?).collect()
}

/// Which Smart Inbox sections an inbox page shows: those in `only` (all
/// when empty), less `except`; and with `seen`, only mail read or unread.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sections<'a> {
    pub only: &'a [Section],
    pub except: Option<Section>,
    pub seen: Option<bool>,
}

/// Where `listed_at` follows [`COLUMNS`] in [`inbox_sql`].
const LISTED_AT: usize = COLUMN_COUNT;

/// The account's messages carrying `mark` (snoozed: until after `now`), one per
/// Message-ID, newest first: the Pinned, Set aside, Snoozed and Reminders views.
pub fn marked(
    conn: &Connection,
    account_id: &str,
    mark: crate::marks::Mark,
    now: i64,
    limit: u32,
    before: Option<Cursor>,
) -> Result<Vec<Message>, StoreError> {
    let condition = match mark {
        crate::marks::Mark::Pinned => "k.pinned_at IS NOT NULL",
        crate::marks::Mark::Aside => "k.aside_at IS NOT NULL",
        crate::marks::Mark::Snoozed => "k.snoozed_until > ?2",
        crate::marks::Mark::Remind => "k.remind_at IS NOT NULL",
        crate::marks::Mark::Reminded => "k.reminded_at IS NOT NULL",
        crate::marks::Mark::Surfaced => "k.surfaced_at IS NOT NULL",
    };
    let (received_at, id) = before.map_or((i64::MAX, i64::MAX), |c| (c.received_at, c.id));
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM messages WHERE id IN ( \
             SELECT MIN(m.id) FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id \
             JOIN marks k ON k.account_id = b.account_id AND k.message_id = m.message_id_header \
             WHERE b.account_id = ?1 AND m.deleted = 0 AND {condition} GROUP BY m.message_id_header) \
         AND (received_at, id) < (?3, ?4) ORDER BY received_at DESC, id DESC LIMIT ?5"
    ))?;
    let messages = statement
        .query_map(params![account_id, now, received_at, id, limit], from_row)?
        .map(|row| row?)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(messages)
}

/// The messages of a conversation, oldest first, one per Message-ID (copies of
/// one message in several folders are listed once). Deleted ones are left out.
pub fn in_thread(conn: &Connection, thread_id: i64) -> Result<Vec<Message>, StoreError> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM messages WHERE thread_id = ?1 AND deleted = 0 \
         ORDER BY COALESCE(sent_at, received_at), id"
    ))?;
    let rows = statement
        .query_map([thread_id], from_row)?
        .map(|row| row?)
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = std::collections::HashSet::new();
    Ok(rows
        .into_iter()
        .filter(|m| m.message_id.as_ref().is_none_or(|id| seen.insert(id.clone())))
        .collect())
}

/// Every stored copy of a message in an account (Inbox, All Mail, …), by Message-ID.
pub fn copies(conn: &Connection, account_id: &str, message_id: &str) -> Result<Vec<Message>, StoreError> {
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM messages WHERE message_id_header = ?2 AND deleted = 0 \
         AND mailbox_id IN (SELECT id FROM mailboxes WHERE account_id = ?1)"
    ))?;
    let rows = statement
        .query_map(params![account_id, message_id], from_row)?
        .map(|row| row?)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The stored messages of a folder with these UIDs (those not stored left out).
pub fn by_uids(conn: &Connection, mailbox_id: i64, uids: &[u32]) -> Result<Vec<Message>, StoreError> {
    let uids = serde_json::to_string(uids).expect("numbers always serialize");
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM messages WHERE mailbox_id = ?1 AND uid IN (SELECT value FROM json_each(?2))"
    ))?;
    let rows = statement.query_map(params![mailbox_id, uids], from_row)?;
    rows.map(|row| row?).collect()
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Message>, StoreError> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM messages WHERE id = ?1"), [id], from_row)
        .optional()?
        .transpose()
}

pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Result<Message, StoreError>> {
    let keywords: String = row.get(9)?;
    let to: String = row.get(16)?;
    let cc: String = row.get(17)?;
    let from_name: Option<String> = row.get(14)?;
    let from_address: Option<String> = row.get(15)?;
    let modseq: Option<i64> = row.get(3)?;
    let message = (|| {
        Ok(Message {
            id: row.get(0)?,
            mailbox_id: row.get(1)?,
            uid: row.get(2)?,
            modseq: modseq.map(|m| m as u64),
            flags: MessageFlags {
                seen: row.get(4)?,
                answered: row.get(5)?,
                flagged: row.get(6)?,
                draft: row.get(7)?,
                deleted: row.get(8)?,
                keywords: parse_json(&keywords, "keywords")?,
            },
            received_at: row.get(10)?,
            size: row.get(11)?,
            sent_at: row.get(12)?,
            subject: row.get(13)?,
            from: (from_name.is_some() || from_address.is_some()).then_some(Person {
                name: from_name,
                address: from_address,
            }),
            to: parse_json(&to, "recipients")?,
            cc: parse_json(&cc, "recipients")?,
            message_id: row.get(18)?,
            blob_hash: row.get(19)?,
            moving_to: row.get(20)?,
            thread_id: row.get(21)?,
            section: row.get::<_, Option<String>>(22)?.as_deref().and_then(Section::parse),
            has_attachments: row.get(23)?,
            preview: row.get(24)?,
            listed_at: row.get(10)?,
        })
    })();
    Ok(message)
}

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("plain data always serializes")
}

fn parse_json<T: serde::de::DeserializeOwned>(text: &str, what: &str) -> Result<T, StoreError> {
    serde_json::from_str(text).map_err(|error| StoreError::Corrupt(format!("message {what}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use crate::mailboxes::{self, RemoteMailbox};
    use regards_core::mailbox::Role;
    use regards_core::paths::DataDirs;

    #[tokio::test]
    async fn pages_walk_indexes_instead_of_scanning_the_mailbox() {
        let (_temp, store, _inbox) = store_with_inbox().await;
        let (plain, inbox): (Vec<String>, Vec<String>) = store
            .read(|conn| {
                let plan = |sql: String, inbox: bool| -> Result<Vec<String>, StoreError> {
                    let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
                    let step = |row: &Row<'_>| row.get::<_, String>(3);
                    let details = if inbox {
                        statement.query_map(params![1, i64::MAX, i64::MAX, 50, "account", 0], step)?
                    } else {
                        statement.query_map(params![1, i64::MAX, i64::MAX, 50], step)?
                    };
                    Ok(details.collect::<Result<_, _>>()?)
                };
                // One section's page walks the section index, not the whole inbox.
                let mut statement = conn.prepare(&format!(
                    "EXPLAIN QUERY PLAN {}",
                    inbox_sql(&Sections {
                        only: &[Section::People],
                        except: None,
                        seen: None,
                    })
                ))?;
                let section: Vec<String> = statement
                    .query_map(params![1, i64::MAX, i64::MAX, 50, "account", 0, "people"], |row| {
                        row.get::<_, String>(3)
                    })?
                    .collect::<Result<_, _>>()?;
                assert!(
                    section
                        .iter()
                        .any(|step| step.contains("USING INDEX messages_by_section")),
                    "{section:#?}"
                );
                Ok((plan(list_sql(), false)?, plan(inbox_sql(&Sections::default()), true)?))
            })
            .await
            .unwrap();
        // With an OR, SQLite reads both indexes ("MULTI-INDEX OR") and sorts the
        // whole folder for every page: 55 ms at 100k messages (tests/scale.rs).
        for plan in [&plain, &inbox] {
            assert!(
                plan.iter().any(|step| step.contains("USING INDEX messages_by_date"))
                    && plan.iter().any(|step| step.contains("USING INDEX messages_moving_to"))
                    && !plan
                        .iter()
                        .any(|step| step.starts_with("SCAN messages") || step.contains("MULTI-INDEX OR")),
                "{plan:#?}"
            );
        }
        // What came back is found from its marks, newest first; the only scans
        // are of each half's own page.
        assert!(
            inbox.iter().any(|step| step.contains("USING INDEX marks_surfaced"))
                && inbox
                    .iter()
                    .all(|step| !step.starts_with("SCAN") || step.starts_with("SCAN (subquery")),
            "{inbox:#?}"
        );
    }

    #[tokio::test]
    async fn forgetting_an_accounts_mail_keeps_the_account_and_its_marks() {
        use crate::marks::{self, Mark};
        let (_temp, store, mailbox) = store_with_inbox().await;
        let batch: Vec<NewMessage> = (1..=2)
            .map(|uid| {
                let mut new = message(uid, i64::from(uid) * 1000, &format!("m{uid}"));
                new.headers.message_id = Some(format!("m{uid}@x"));
                new
            })
            .collect();
        let account = store
            .write(move |tx| {
                upsert(tx, mailbox, &batch)?;
                tx.execute(
                    "UPDATE mailboxes SET uid_validity = 7, uid_next = 3 WHERE id = ?1",
                    [mailbox],
                )?;
                let account: String =
                    tx.query_row("SELECT account_id FROM mailboxes WHERE id = ?1", [mailbox], |r| {
                        r.get(0)
                    })?;
                marks::set(tx, &account, "m1@x", Mark::Pinned, Some(10))?;
                Ok(account)
            })
            .await
            .unwrap()
            .value;
        let a = account.clone();
        let dropped = store
            .write(move |tx| accounts::forget_mail(tx, &a))
            .await
            .unwrap()
            .value;
        assert_eq!(dropped, 2);
        let a = account.clone();
        let (left, folder, pinned, still) = store
            .read(move |c| {
                Ok((
                    c.query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, i64>(0))?,
                    mailboxes::get(c, mailbox)?.unwrap(),
                    marks::get(c, &a, "m1@x")?.pinned_at,
                    accounts::get(c, &a)?.is_some(),
                ))
            })
            .await
            .unwrap();
        assert_eq!(left, 0);
        assert_eq!(
            (folder.uid_validity, folder.uid_next),
            (None, None),
            "the next sync starts afresh"
        );
        assert_eq!(pinned, Some(10), "a pin stays for when the message is back");
        assert!(still, "the account stays");
    }

    async fn store_with_inbox() -> (tempfile::TempDir, Store, i64) {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let id = accounts::new_id();
        let server = Server {
            host: "h".into(),
            port: 993,
            security: Security::Tls,
        };
        let account = NewAccount {
            id: id.clone(),
            display_name: "Ada".into(),
            email: "ada@example.test".into(),
            auth_kind: AuthKind::Password,
            username: "ada".into(),
            secret_key: "k".into(),
            imap: server.clone(),
            smtp: server,
            imap_capabilities: vec![],
        };
        let mailbox_id = store
            .write(move |tx| {
                accounts::insert(tx, account)?;
                mailboxes::reconcile(
                    tx,
                    &id,
                    &[RemoteMailbox {
                        remote_name: b"INBOX".to_vec(),
                        name: "INBOX".into(),
                        delimiter: Some('/'),
                        role: Some(Role::Inbox),
                        selectable: true,
                        attributes: vec![],
                    }],
                )?;
                Ok(mailboxes::list(tx, &id)?[0].id)
            })
            .await
            .unwrap()
            .value;
        (temp, store, mailbox_id)
    }

    fn message(uid: u32, received_at: i64, subject: &str) -> NewMessage {
        NewMessage {
            uid,
            modseq: Some(u64::from(uid)),
            flags: MessageFlags::default(),
            received_at: Some(received_at),
            size: Some(100),
            headers: Headers {
                subject: subject.into(),
                from: Some(Person {
                    name: Some("Ada".into()),
                    address: Some("ada@example.test".into()),
                }),
                ..Headers::default()
            },
        }
    }

    #[tokio::test]
    async fn mark_all_read_finds_the_unread_of_a_folder_or_of_one_section() {
        let (_temp, store, mailbox) = store_with_inbox().await;
        let batch: Vec<NewMessage> = (1..=4)
            .map(|uid| {
                let mut new = message(uid, i64::from(uid) * 1000, &format!("m{uid}"));
                new.flags.seen = uid == 2;
                new.flags.deleted = uid == 4;
                new
            })
            .collect();
        store.write(move |tx| upsert(tx, mailbox, &batch)).await.unwrap();
        let ids = store.read(move |c| unread_in(c, mailbox, None)).await.unwrap();
        assert_eq!(ids.len(), 2, "not the read one, nor the deleted one");
        let first = ids[0];
        store
            .write(move |tx| {
                tx.execute("UPDATE messages SET section = 'notifications'", [])?;
                tx.execute("UPDATE messages SET section = 'people' WHERE id = ?1", [first])?;
                Ok(())
            })
            .await
            .unwrap();
        let people = store
            .read(move |c| unread_in(c, mailbox, Some(Section::People)))
            .await
            .unwrap();
        assert_eq!(people, [first]);
    }

    #[tokio::test]
    async fn inbox_pages_can_hold_only_read_or_only_unread_mail() {
        let (_temp, store, mailbox) = store_with_inbox().await;
        let batch: Vec<NewMessage> = (1..=5)
            .map(|uid| {
                let mut new = message(uid, i64::from(uid) * 1000, &format!("m{uid}"));
                new.flags.seen = uid % 2 == 0;
                new
            })
            .collect();
        store.write(move |tx| upsert(tx, mailbox, &batch)).await.unwrap();
        store
            .write(|tx| Ok(tx.execute("UPDATE messages SET section = 'people'", [])?))
            .await
            .unwrap();
        let page = |seen: Option<bool>, only: &'static [Section]| {
            store.read(move |c| {
                let sections = Sections {
                    only,
                    except: None,
                    seen,
                };
                let rows = inbox_page(c, mailbox, 50, None, "account", 0, &sections)?;
                Ok(rows.into_iter().map(|m| m.subject).collect::<Vec<_>>())
            })
        };
        assert_eq!(page(Some(false), &[]).await.unwrap(), ["m5", "m3", "m1"]);
        assert_eq!(page(Some(true), &[]).await.unwrap(), ["m4", "m2"]);
        assert_eq!(page(Some(false), &[Section::People]).await.unwrap(), ["m5", "m3", "m1"]);
        assert!(page(Some(false), &[Section::Newsletters]).await.unwrap().is_empty());
        assert_eq!(page(None, &[]).await.unwrap().len(), 5);
    }

    #[tokio::test]
    async fn the_inbox_leaves_out_what_is_set_aside_or_snoozed_and_views_show_it() {
        use crate::marks::{self, Mark};
        let (_temp, store, mailbox) = store_with_inbox().await;
        let account: String = store
            .read(move |c| {
                Ok(
                    c.query_row("SELECT account_id FROM mailboxes WHERE id = ?1", [mailbox], |r| {
                        r.get(0)
                    })?,
                )
            })
            .await
            .unwrap();
        let batch: Vec<NewMessage> = (1..=4)
            .map(|uid| {
                let mut new = message(uid, i64::from(uid) * 1000, &format!("m{uid}"));
                new.headers.message_id = Some(format!("m{uid}@x"));
                new
            })
            .collect();
        store.write(move |tx| upsert(tx, mailbox, &batch)).await.unwrap();
        let a = account.clone();
        store
            .write(move |tx| {
                marks::set(tx, &a, "m1@x", Mark::Pinned, Some(10))?;
                marks::set(tx, &a, "m2@x", Mark::Aside, Some(10))?;
                marks::set(tx, &a, "m3@x", Mark::Snoozed, Some(5_000))?;
                marks::set(tx, &a, "m4@x", Mark::Snoozed, Some(500))
            })
            .await
            .unwrap();
        let subjects = |list: Vec<Message>| list.into_iter().map(|m| m.subject).collect::<Vec<_>>();
        let a = account.clone();
        let inbox = store
            .read(move |c| list_without_parked(c, mailbox, 10, None, Some((&a, 1_000))))
            .await
            .unwrap();
        // m2 is set aside and m3 snoozed until later; m4's snooze is over; pinned m1 stays.
        assert_eq!(subjects(inbox), ["m4", "m1"]);
        assert_eq!(
            subjects(store.read(move |c| list(c, mailbox, 10, None)).await.unwrap()),
            ["m4", "m3", "m2", "m1"],
            "other folders show everything"
        );
        for (mark, expected) in [(Mark::Pinned, "m1"), (Mark::Aside, "m2"), (Mark::Snoozed, "m3")] {
            let a = account.clone();
            let view = store.read(move |c| marked(c, &a, mark, 1_000, 10, None)).await.unwrap();
            assert_eq!(subjects(view), [expected], "{mark:?}");
        }
    }

    #[tokio::test]
    async fn snoozes_come_due_and_a_reply_from_someone_else_ends_them() {
        use crate::marks;
        let (_temp, store, mailbox) = store_with_inbox().await;
        let account: String = store
            .read(move |c| {
                Ok(
                    c.query_row("SELECT account_id FROM mailboxes WHERE id = ?1", [mailbox], |r| {
                        r.get(0)
                    })?,
                )
            })
            .await
            .unwrap();
        let with = |uid: u32, at: i64, id: &str, parent: Option<&str>, from: &str| {
            let mut new = message(uid, at, id);
            new.headers.message_id = Some(id.into());
            new.headers.in_reply_to = parent.map(str::to_owned).into_iter().collect();
            new.headers.from = Some(Person {
                name: None,
                address: Some(from.into()),
            });
            new
        };
        let first = vec![
            with(1, 1_000, "plan@x", None, "bob@example.test"),
            with(2, 1_000, "alone@x", None, "bob@example.test"),
        ];
        store.write(move |tx| upsert(tx, mailbox, &first)).await.unwrap();
        let a = account.clone();
        store
            .write(move |tx| {
                marks::set(tx, &a, "plan@x", marks::Mark::Snoozed, Some(50_000))?;
                marks::set(tx, &a, "alone@x", marks::Mark::Snoozed, Some(9_000))
            })
            .await
            .unwrap();
        assert_eq!(store.read(marks::next_wake).await.unwrap(), Some(9_000));
        assert!(store.read(|c| marks::due_snoozes(c, 8_999)).await.unwrap().is_empty());
        assert_eq!(
            store.read(|c| marks::due_snoozes(c, 9_000)).await.unwrap(),
            [(account.clone(), "alone@x".to_owned())]
        );

        // A reply joins the conversation that holds a snoozed message.
        let reply = vec![with(4, 4_000, "reply@x", Some("plan@x"), "bob@example.test")];
        store.write(move |tx| upsert(tx, mailbox, &reply)).await.unwrap();
        let a = account.clone();
        assert_eq!(
            store
                .read(move |c| marks::snoozed_in_thread_of(c, &a, "reply@x"))
                .await
                .unwrap(),
            ["plan@x"]
        );
        let a = account.clone();
        assert!(
            store
                .read(move |c| marks::snoozed_in_thread_of(c, &a, "plan@x"))
                .await
                .unwrap()
                .is_empty(),
            "a message does not end its own snooze"
        );
        // "Someday" never comes due by itself.
        let a = account.clone();
        store
            .write(move |tx| marks::set(tx, &a, "alone@x", marks::Mark::Snoozed, Some(marks::SOMEDAY)))
            .await
            .unwrap();
        assert_eq!(store.read(marks::next_wake).await.unwrap(), Some(50_000));
    }

    #[tokio::test]
    async fn what_comes_back_is_on_top_of_the_inbox_and_a_reminder_shows_from_sent() {
        use crate::marks::{self, Mark};
        let (_temp, store, inbox) = store_with_inbox().await;
        let (account, sent) = store
            .write(move |tx| {
                let account: String =
                    tx.query_row("SELECT account_id FROM mailboxes WHERE id = ?1", [inbox], |r| r.get(0))?;
                let folder = |name: &str, role| RemoteMailbox {
                    remote_name: name.as_bytes().to_vec(),
                    name: name.into(),
                    delimiter: Some('/'),
                    role: Some(role),
                    selectable: true,
                    attributes: vec![],
                };
                mailboxes::reconcile(
                    tx,
                    &account,
                    &[folder("INBOX", Role::Inbox), folder("Sent", Role::Sent)],
                )?;
                let sent = mailboxes::list(tx, &account)?
                    .into_iter()
                    .find(|m| m.role == Some(Role::Sent))
                    .unwrap()
                    .id;
                Ok((account, sent))
            })
            .await
            .unwrap()
            .value;
        let with = |uid: u32, at: i64, id: &str, parent: Option<&str>, from: &str| {
            let mut new = message(uid, at, id);
            new.headers.message_id = Some(id.into());
            new.headers.in_reply_to = parent.map(str::to_owned).into_iter().collect();
            new.headers.from = Some(Person {
                name: None,
                address: Some(from.into()),
            });
            new
        };
        let received: Vec<NewMessage> = (1..=4)
            .map(|uid| {
                with(
                    uid,
                    i64::from(uid) * 1000,
                    &format!("m{uid}@x"),
                    None,
                    "bob@example.test",
                )
            })
            .collect();
        store.write(move |tx| upsert(tx, inbox, &received)).await.unwrap();
        // Ada wrote at 2.5 s and asked to be reminded; nobody answered.
        let mine = vec![with(1, 2_500, "mine@x", None, "ada@example.test")];
        store.write(move |tx| upsert(tx, sent, &mine)).await.unwrap();
        let a = account.clone();
        store
            .write(move |tx| {
                // m1's snooze ended at 10 s; the reminder came due at 9 s.
                marks::set(tx, &a, "m1@x", Mark::Surfaced, Some(10_000))?;
                marks::set(tx, &a, "mine@x", Mark::Reminded, Some(9_000))?;
                marks::set(tx, &a, "mine@x", Mark::Surfaced, Some(9_000))?;
                // Came back before it arrived (clocks): listed by its own time, once.
                marks::set(tx, &a, "m3@x", Mark::Surfaced, Some(100))
            })
            .await
            .unwrap();

        let page = |before: Option<Cursor>, limit: u32| {
            let a = account.clone();
            let store = store.clone();
            async move {
                store
                    .read(move |c| list_without_parked(c, inbox, limit, before, Some((&a, 20_000))))
                    .await
                    .unwrap()
            }
        };
        let shown = |list: &[Message]| {
            list.iter()
                .map(|m| (m.subject.clone(), m.listed_at))
                .collect::<Vec<_>>()
        };
        let all = page(None, 10).await;
        assert_eq!(
            shown(&all),
            [
                ("m1@x".to_owned(), 10_000),
                ("mine@x".to_owned(), 9_000),
                ("m4@x".to_owned(), 4_000),
                ("m3@x".to_owned(), 3_000),
                ("m2@x".to_owned(), 2_000),
            ]
        );
        assert_eq!(all[1].mailbox_id, sent, "the reminder shows its copy in Sent");
        // Pages follow the same order: the cursor is the last one's place.
        let first = page(None, 2).await;
        let cursor = |m: &Message| Cursor {
            received_at: m.listed_at,
            id: m.id,
        };
        let second = page(Some(cursor(&first[1])), 2).await;
        let third = page(Some(cursor(&second[1])), 2).await;
        let walked: Vec<_> = [first, second, third].iter().flat_map(|p| shown(p)).collect();
        assert_eq!(walked, shown(&all));
        // Other folders keep their own order.
        let sent_list = store.read(move |c| list(c, sent, 10, None)).await.unwrap();
        assert_eq!(sent_list[0].listed_at, 2_500);

        // Snoozed again: out of the inbox, whatever came back before.
        let a = account.clone();
        store
            .write(move |tx| marks::set(tx, &a, "m1@x", Mark::Snoozed, Some(50_000)))
            .await
            .unwrap();
        assert_eq!(page(None, 10).await[0].subject, "mine@x");

        // A reply from someone else, after Ada's message: replied, and the
        // reminder is found from the reply's conversation.
        let a = account.clone();
        let own = store
            .read(move |c| marks::replied_after(c, &a, "mine@x", "Ada@Example.test"))
            .await
            .unwrap();
        assert_eq!(own, Some(false), "nobody has answered yet");
        let follow_up = vec![with(5, 3_000, "again@x", Some("mine@x"), "ada@example.test")];
        store.write(move |tx| upsert(tx, sent, &follow_up)).await.unwrap();
        let a = account.clone();
        assert_eq!(
            store
                .read(move |c| marks::replied_after(c, &a, "mine@x", "ada@example.test"))
                .await
                .unwrap(),
            Some(false),
            "Ada's own follow-up is not a reply"
        );
        let reply = vec![with(6, 5_000, "reply@x", Some("mine@x"), "bob@example.test")];
        store.write(move |tx| upsert(tx, inbox, &reply)).await.unwrap();
        let a = account.clone();
        assert_eq!(
            store
                .read(move |c| marks::replied_after(c, &a, "mine@x", "ada@example.test"))
                .await
                .unwrap(),
            Some(true)
        );
        let a = account.clone();
        assert_eq!(
            store
                .read(move |c| marks::reminders_in_thread_of(c, &a, "reply@x"))
                .await
                .unwrap(),
            ["mine@x"]
        );
        let a = account.clone();
        assert_eq!(
            store
                .read(move |c| marks::replied_after(c, &a, "unknown@x", "ada@example.test"))
                .await
                .unwrap(),
            None,
            "no copy, no answer"
        );

        // A waiting reminder comes due like a snooze, and shows in its view.
        let a = account.clone();
        store
            .write(move |tx| marks::set(tx, &a, "m2@x", Mark::Remind, Some(30_000)))
            .await
            .unwrap();
        assert_eq!(store.read(marks::next_wake).await.unwrap(), Some(30_000));
        assert_eq!(
            store.read(|c| marks::due_reminders(c, 30_000)).await.unwrap(),
            [(account.clone(), "m2@x".to_owned())]
        );
        let a = account.clone();
        let view = store
            .read(move |c| marked(c, &a, Mark::Remind, 20_000, 10, None))
            .await
            .unwrap();
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].subject, "m2@x");
    }

    #[tokio::test]
    async fn pages_continue_through_messages_that_arrived_in_the_same_second() {
        let (_temp, store, mailbox) = store_with_inbox().await;
        let batch: Vec<NewMessage> = (1..=5).map(|uid| message(uid, 7000, &format!("m{uid}"))).collect();
        store.write(move |tx| upsert(tx, mailbox, &batch)).await.unwrap();
        let mut seen = Vec::new();
        let mut before = None;
        loop {
            let page = store.read(move |conn| list(conn, mailbox, 2, before)).await.unwrap();
            let Some(last) = page.last() else { break };
            before = Some(Cursor {
                received_at: last.received_at,
                id: last.id,
            });
            seen.extend(page.into_iter().map(|m| m.uid));
        }
        assert_eq!(seen, [5, 4, 3, 2, 1], "every message once, none skipped");
    }

    #[tokio::test]
    async fn upsert_list_page_flags_and_delete() {
        let (_temp, store, mailbox) = store_with_inbox().await;
        let batch: Vec<NewMessage> = (1..=5)
            .map(|uid| message(uid, i64::from(uid) * 1000, &format!("m{uid}")))
            .collect();
        store.write(move |tx| upsert(tx, mailbox, &batch)).await.unwrap();

        // Newest first, two per page.
        let page = store.read(move |conn| list(conn, mailbox, 2, None)).await.unwrap();
        assert_eq!(
            page.iter().map(|m| m.subject.as_str()).collect::<Vec<_>>(),
            ["m5", "m4"]
        );
        let cursor = Cursor {
            received_at: page[1].received_at,
            id: page[1].id,
        };
        let next = store
            .read(move |conn| list(conn, mailbox, 2, Some(cursor)))
            .await
            .unwrap();
        assert_eq!(
            next.iter().map(|m| m.subject.as_str()).collect::<Vec<_>>(),
            ["m3", "m2"]
        );
        assert_eq!(next[0].from.as_ref().unwrap().name.as_deref(), Some("Ada"));

        let seen = MessageFlags {
            seen: true,
            ..MessageFlags::default()
        };
        let changes = vec![
            FlagChange {
                uid: 1,
                modseq: Some(9),
                flags: seen.clone(),
            },
            FlagChange {
                uid: 2,
                modseq: None,
                flags: MessageFlags::default(), // unchanged
            },
        ];
        let changed = store
            .write(move |tx| apply_flags(tx, mailbox, &changes))
            .await
            .unwrap()
            .value;
        assert_eq!(changed, 1, "only real changes count");

        let removed = store
            .write(move |tx| delete_uids(tx, mailbox, &[2, 4, 99]))
            .await
            .unwrap()
            .value;
        assert_eq!(removed, 2);
        assert_eq!(store.read(move |conn| uids(conn, mailbox)).await.unwrap(), [1, 3, 5]);

        let oldest = store.read(move |conn| list(conn, mailbox, 10, None)).await.unwrap();
        let first = oldest.iter().find(|m| m.uid == 1).unwrap();
        assert!(first.flags.seen);
        assert_eq!(first.modseq, Some(9));

        // Upserting an existing UID replaces it rather than duplicating it.
        let again = vec![message(1, 1000, "m1 again")];
        store.write(move |tx| upsert(tx, mailbox, &again)).await.unwrap();
        assert_eq!(store.read(move |conn| uids(conn, mailbox)).await.unwrap(), [1, 3, 5]);

        let cleared = store.write(move |tx| clear(tx, mailbox)).await.unwrap().value;
        assert_eq!(cleared, 3);
    }
}
