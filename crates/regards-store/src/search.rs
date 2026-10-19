//! The keyword index (ADR-0008 §4, plan §5.10): SQLite FTS5 in `search.db`,
//! next to `mail.db`. Like any index it can be deleted; it is then rebuilt.
//!
//! - **A row per stored message** (`rowid` = `messages.id`): its subject,
//!   sender, recipients and text. The text is the whole of it once the
//!   message was downloaded, its start (`preview`) until then.
//! - **Changes are queued** (`dirty`) by TEMP triggers on the writer
//!   connection, so every write to `messages` (sync, moves, a removed
//!   account) is seen, with nothing to remember at each call site. The
//!   queue is worked off in batches ([`index`]), in the background: a new
//!   message, whose preview follows its headers, is indexed once, and sync's
//!   writes stay short.
//! - **Full texts are kept by account and Message-ID** (`texts`), not by row:
//!   a moved message arrives in its new folder as a new row, and is found by
//!   its text at once.
//! - **Its own copy of the text:** FTS5 can take its text from another table
//!   (external content), but only one in its own database, not `mail.db`.
//! - **Another schema** ([`VERSION`]) drops the index and queues every
//!   message; search works on what is done meanwhile, newest first.
//!
//! The two databases commit separately in WAL mode: after a crash the index
//! may lag a write, which only affects what is found, never the mail.

use crate::StoreError;
use crate::messages::{self, Cursor, Message};
use regards_core::search::Query;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::path::Path;

/// The index's schema; another one rebuilds it.
const VERSION: i64 = 1;
/// The most of a message's text that is indexed, in bytes.
const MAX_TEXT: usize = 64 * 1024;
/// Best matches put first on a search's first page.
const TOP: u32 = 3;
/// Up to this many matches, a search reads them all (and ranks them);
/// beyond, it walks the mail newest first until it has a page.
const FEW: u32 = 2_000;

/// Attach `search.db` as `search`; read-only connections attach it read-only.
pub(crate) fn attach(conn: &Connection, path: &Path) -> Result<(), StoreError> {
    conn.execute("ATTACH DATABASE ?1 AS search", [path.to_string_lossy()])?;
    Ok(())
}

/// On the writer: the schema (new, or rebuilt when another version), and
/// the triggers that queue changes.
pub(crate) fn prepare(conn: &Connection) -> Result<(), StoreError> {
    conn.pragma_update(Some("search"), "journal_mode", "WAL")?;
    conn.pragma_update(Some("search"), "synchronous", "NORMAL")?;
    let version: i64 = conn.pragma_query_value(Some("search"), "user_version", |row| row.get(0))?;
    if version != VERSION {
        conn.execute_batch(&format!(
            "BEGIN; \
             DROP TABLE IF EXISTS search.mail; \
             DROP TABLE IF EXISTS search.texts; \
             DROP TABLE IF EXISTS search.dirty; \
             CREATE VIRTUAL TABLE search.mail USING fts5(subject, sender, recipients, body, \
                 tokenize = 'unicode61 remove_diacritics 2', prefix = '2 3'); \
             CREATE TABLE search.texts (account_id TEXT NOT NULL, message_id TEXT NOT NULL, \
                 body TEXT NOT NULL, stored_at INTEGER NOT NULL, \
                 PRIMARY KEY (account_id, message_id)) WITHOUT ROWID; \
             CREATE TABLE search.dirty (id INTEGER PRIMARY KEY); \
             INSERT INTO search.dirty SELECT id FROM main.messages; \
             PRAGMA search.user_version = {VERSION}; \
             COMMIT;"
        ))?;
    }
    // Not INSERT OR IGNORE: an upsert overrides how a trigger it fires resolves conflicts.
    conn.execute_batch(
        "CREATE TEMP TRIGGER search_added AFTER INSERT ON main.messages BEGIN \
             INSERT INTO search.dirty (id) SELECT new.id WHERE NOT EXISTS (SELECT 1 FROM search.dirty WHERE id = new.id); \
         END; \
         CREATE TEMP TRIGGER search_changed \
             AFTER UPDATE OF subject, from_name, from_address, to_json, cc_json, message_id_header, preview \
             ON main.messages \
             WHEN old.subject IS NOT new.subject OR old.from_name IS NOT new.from_name \
             OR old.from_address IS NOT new.from_address OR old.to_json IS NOT new.to_json \
             OR old.cc_json IS NOT new.cc_json OR old.message_id_header IS NOT new.message_id_header \
             OR old.preview IS NOT new.preview \
         BEGIN \
             INSERT INTO search.dirty (id) SELECT new.id WHERE NOT EXISTS (SELECT 1 FROM search.dirty WHERE id = new.id); \
         END; \
         CREATE TEMP TRIGGER search_removed AFTER DELETE ON main.messages BEGIN \
             INSERT INTO search.dirty (id) SELECT old.id WHERE NOT EXISTS (SELECT 1 FROM search.dirty WHERE id = old.id); \
         END;",
    )?;
    Ok(())
}

/// What the index holds of the message row `m`, as `INSERT … SELECT` values.
fn indexed(m: &str) -> String {
    let people = |column: &str| {
        format!(
            "COALESCE((SELECT group_concat(COALESCE(p.value ->> 'name', '') || ' ' \
                 || COALESCE(p.value ->> 'address', ''), ' ') \
             FROM json_each(CASE WHEN json_valid({m}.{column}) THEN {m}.{column} END) p), '')"
        )
    };
    format!(
        "{m}.id, COALESCE({m}.subject, ''), COALESCE({m}.from_name, '') || ' ' || COALESCE({m}.from_address, ''), \
         {to} || ' ' || {cc}, \
         COALESCE(NULLIF((SELECT t.body FROM search.texts t \
             WHERE t.account_id = (SELECT b.account_id FROM main.mailboxes b WHERE b.id = {m}.mailbox_id) \
             AND t.message_id = {key}), ''), {m}.preview, '')",
        to = people("to_json"),
        cc = people("cc_json"),
        key = text_key(m),
    )
}

/// What a message's full text is kept by: its Message-ID, or its row.
fn text_key(m: &str) -> String {
    format!("COALESCE({m}.message_id_header, '#' || {m}.id)")
}

/// Whether the index is there (a broken `search.db` is left out, see `Store::open`).
fn available(conn: &Connection) -> Result<bool, StoreError> {
    Ok(conn
        .query_row("SELECT 1 FROM pragma_database_list WHERE name = 'search'", [], |_| {
            Ok(())
        })
        .optional()?
        .is_some())
}

/// Index the next `batch` queued changes, the newest messages first; how
/// many are left.
pub fn index(tx: &Transaction<'_>, batch: u32) -> Result<u64, StoreError> {
    if !available(tx)? {
        return Ok(0);
    }
    let ids: Vec<i64> = {
        let mut statement = tx.prepare_cached("SELECT id FROM search.dirty ORDER BY id DESC LIMIT ?1")?;
        let rows = statement.query_map([batch], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    // In ascending order: FTS5 writes a batch of rising rowids in one go.
    let mut forget = tx.prepare_cached("DELETE FROM search.mail WHERE rowid = ?1")?;
    let mut add = tx.prepare_cached(&format!(
        "INSERT INTO search.mail (rowid, subject, sender, recipients, body) \
         SELECT {} FROM main.messages m WHERE m.id = ?1",
        indexed("m")
    ))?;
    let mut done = tx.prepare_cached("DELETE FROM search.dirty WHERE id = ?1")?;
    for id in ids.iter().rev() {
        forget.execute([id])?;
        add.execute([id])?;
        done.execute([id])?;
    }
    pending(tx)
}

/// Queued changes not indexed yet; 0 when the index is current.
pub fn pending(conn: &Connection) -> Result<u64, StoreError> {
    if !available(conn)? {
        return Ok(0);
    }
    let left: i64 = conn.query_row("SELECT count(*) FROM search.dirty", [], |row| row.get(0))?;
    Ok(left as u64)
}

/// Message `row` was downloaded: index all of its text, for it and the
/// account's other copies of it.
pub fn set_text(
    tx: &Transaction<'_>,
    account_id: &str,
    row: i64,
    message_id: Option<&str>,
    text: &str,
    now: i64,
) -> Result<(), StoreError> {
    if !available(tx)? {
        return Ok(());
    }
    let key = message_id.map_or_else(|| format!("#{row}"), str::to_owned);
    let mut end = text.len().min(MAX_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    tx.prepare_cached(
        "INSERT INTO search.texts (account_id, message_id, body, stored_at) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT (account_id, message_id) DO UPDATE SET body = excluded.body, stored_at = excluded.stored_at",
    )?
    .execute(params![account_id, key, &text[..end], now])?;
    tx.prepare_cached(
        "INSERT OR IGNORE INTO search.dirty (id) \
         SELECT id FROM main.messages WHERE id = ?1 UNION \
         SELECT m.id FROM main.messages m JOIN main.mailboxes b ON b.id = m.mailbox_id \
         WHERE m.message_id_header = ?2 AND b.account_id = ?3",
    )?
    .execute(params![row, message_id, account_id])?;
    Ok(())
}

/// A downloaded message whose full text is not indexed yet: after a rebuild,
/// or downloaded before there was an index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unindexed {
    pub id: i64,
    pub account_id: String,
    pub message_id: Option<String>,
    pub blob_hash: String,
}

pub fn texts_missing(conn: &Connection, limit: u32) -> Result<Vec<Unindexed>, StoreError> {
    if !available(conn)? {
        return Ok(Vec::new());
    }
    let mut statement = conn.prepare_cached(&format!(
        "SELECT m.id, b.account_id, m.message_id_header, m.blob_hash \
         FROM main.messages m JOIN main.mailboxes b ON b.id = m.mailbox_id \
         WHERE m.blob_hash IS NOT NULL AND NOT EXISTS (SELECT 1 FROM search.texts t \
             WHERE t.account_id = b.account_id AND t.message_id = {}) \
         ORDER BY m.id DESC LIMIT ?1",
        text_key("m")
    ))?;
    let rows = statement.query_map([limit], |row| {
        Ok(Unindexed {
            id: row.get(0)?,
            account_id: row.get(1)?,
            message_id: row.get(2)?,
            blob_hash: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Forget full texts kept longer than until `before` for messages no longer
/// stored (kept a while: a moved message's new row comes with the next sync).
pub fn forget_texts(tx: &Transaction<'_>, before: i64) -> Result<usize, StoreError> {
    if !available(tx)? {
        return Ok(0);
    }
    Ok(tx.execute(
        "DELETE FROM search.texts WHERE stored_at < ?1 \
         AND NOT EXISTS (SELECT 1 FROM main.messages m JOIN main.mailboxes b ON b.id = m.mailbox_id \
             WHERE b.account_id = texts.account_id AND m.message_id_header = texts.message_id) \
         AND NOT (substr(texts.message_id, 1, 1) = '#' \
             AND EXISTS (SELECT 1 FROM main.messages WHERE id = CAST(substr(texts.message_id, 2) AS INTEGER)))",
        [before],
    )?)
}

/// What a search found: a page, newest first, and on the first page of a
/// search with few matches, the best of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hits {
    pub page: Vec<Message>,
    pub top: Vec<Message>,
}

/// Search `account_id` (every account when `None`): one copy of each message
/// (the inbox's, if it has one), newest first, older than `before`.
///
/// Trash and Junk are left out, unless `in:` names them (or `in:anywhere`).
/// Copies of a message are taken to have the same text, so a copy is left
/// out when a better one passes the filters.
pub fn search(
    conn: &Connection,
    query: &Query,
    account_id: Option<&str>,
    limit: u32,
    before: Option<Cursor>,
) -> Result<Hits, StoreError> {
    if query.is_empty() {
        return Ok(Hits::default());
    }
    if !available(conn)? {
        return Err(StoreError::Conflict("search is not available".into()));
    }
    let folders = folders(conn, account_id, query.folder.as_deref())?;
    if folders.is_empty() {
        return Ok(Hits::default());
    }
    let folders = serde_json::to_string(&folders).expect("numbers always serialize");
    let (received_at, id) = before.map_or((i64::MAX, i64::MAX), |c| (c.received_at, c.id));
    let (filters, theirs) = (filters(query, "m"), filters(query, "o"));
    let alone = format!(
        "NOT EXISTS (SELECT 1 FROM main.messages o CROSS JOIN main.mailboxes ob \
             ON ob.id = COALESCE(o.moving_to, o.mailbox_id) \
         WHERE o.message_id_header = m.message_id_header AND o.id <> m.id AND o.deleted = 0 \
         AND ob.account_id = b.account_id AND ob.id IN (SELECT value FROM json_each(?1)){theirs} \
         AND (ob.role IS NOT 'inbox', ob.role IS 'all', o.id) < (b.role IS NOT 'inbox', b.role IS 'all', m.id))"
    );
    let columns = messages::columns("m");
    // On the message, not its folder: a walk then tests it from the index.
    let inside = "AND COALESCE(m.moving_to, m.mailbox_id) IN (SELECT value FROM json_each(?1))";
    let few = match &query.text {
        Some(text) => conn
            .prepare_cached("SELECT count(*) FROM (SELECT 1 FROM search.mail WHERE mail MATCH ?1 LIMIT ?2)")?
            .query_row(params![text, FEW + 1], |row| row.get::<_, u32>(0))?
            .le(&FEW),
        None => false,
    };
    let mut hits = Hits::default();
    if let (true, Some(text)) = (few, &query.text) {
        // Few: read them all, newest first, and the best of them by BM25.
        let sql = format!(
            "WITH found AS MATERIALIZED (SELECT m.id AS id, m.received_at AS received_at, \
                 bm25(f.mail, 5.0, 3.0, 2.0, 1.0) AS rank \
             FROM search.mail AS f CROSS JOIN main.messages m ON m.id = f.rowid \
             CROSS JOIN main.mailboxes b ON b.id = COALESCE(m.moving_to, m.mailbox_id) \
             WHERE f.mail MATCH ?5 AND m.deleted = 0 {inside}{filters} AND {alone}) \
             SELECT * FROM (SELECT {columns}, 0 FROM found CROSS JOIN main.messages m ON m.id = found.id \
                 WHERE (found.received_at, found.id) < (?2, ?3) \
                 ORDER BY found.received_at DESC, found.id DESC LIMIT ?4) \
             UNION ALL \
             SELECT * FROM (SELECT {columns}, 1 FROM found CROSS JOIN main.messages m ON m.id = found.id \
                 WHERE ?6 ORDER BY found.rank LIMIT {TOP})"
        );
        let mut statement = conn.prepare_cached(&sql)?;
        let mut rows = statement.query(params![folders, received_at, id, limit, text, before.is_none()])?;
        while let Some(row) = rows.next()? {
            let top: bool = row.get(messages::COLUMN_COUNT)?;
            let message = messages::from_row(row)??;
            if top {
                hits.top.push(message)
            } else {
                hits.page.push(message)
            }
        }
    } else {
        // Many, or only filters: newest first until the page is full.
        let matching = match query.text {
            Some(_) => " AND m.id IN (SELECT rowid FROM search.mail WHERE mail MATCH ?5)",
            None => "",
        };
        let sql = format!(
            "SELECT {columns} FROM main.messages m INDEXED BY messages_by_time \
             CROSS JOIN main.mailboxes b ON b.id = COALESCE(m.moving_to, m.mailbox_id) \
             WHERE (m.received_at, m.id) < (?2, ?3) AND m.deleted = 0 {inside}{filters}{matching} \
             AND {alone} \
             ORDER BY m.received_at DESC, m.id DESC LIMIT ?4"
        );
        let mut statement = conn.prepare_cached(&sql)?;
        let mut rows = match &query.text {
            Some(text) => statement.query(params![folders, received_at, id, limit, text])?,
            None => statement.query(params![folders, received_at, id, limit])?,
        };
        while let Some(row) = rows.next()? {
            hits.page.push(messages::from_row(row)??);
        }
    }
    Ok(hits)
}

/// The query's filters on the message row `m`, as SQL (only numbers in it).
fn filters(query: &Query, m: &str) -> String {
    let mut sql = String::new();
    if query.has_attachment {
        sql.push_str(&format!(" AND {m}.has_attachments = 1"));
    }
    match query.unread {
        Some(true) => sql.push_str(&format!(" AND {m}.seen = 0")),
        Some(false) => sql.push_str(&format!(" AND {m}.seen = 1")),
        None => {}
    }
    if query.flagged {
        sql.push_str(&format!(" AND {m}.flagged = 1"));
    }
    if let Some(before) = query.before {
        sql.push_str(&format!(" AND {m}.received_at < {before}"));
    }
    if let Some(after) = query.after {
        sql.push_str(&format!(" AND {m}.received_at >= {after}"));
    }
    sql
}

/// The folders a search looks in.
fn folders(conn: &Connection, account_id: Option<&str>, folder: Option<&str>) -> Result<Vec<i64>, StoreError> {
    let mut statement =
        conn.prepare_cached("SELECT id, name, delimiter, role FROM mailboxes WHERE ?1 IS NULL OR account_id = ?1")?;
    let rows = statement.query_map([account_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut found = Vec::new();
    for row in rows {
        let (id, name, delimiter, role) = row?;
        let role = role.as_deref();
        let wanted = match folder {
            None => !matches!(role, Some("trash" | "junk")),
            Some("anywhere") => true,
            Some(folder) => {
                let folder = if folder == "spam" { "junk" } else { folder };
                let name = name.to_lowercase();
                let leaf = delimiter
                    .as_deref()
                    .and_then(|d| d.chars().next())
                    .and_then(|d| name.rsplit(d).next())
                    .unwrap_or(&name);
                role == Some(folder) || name == folder || leaf == folder
            }
        };
        if wanted {
            found.push(id);
        }
    }
    Ok(found)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
    use crate::mailboxes::{self, RemoteMailbox};
    use crate::messages::{MessageFlags, NewMessage};
    use regards_core::headers::{Headers, Person};
    use regards_core::mailbox::Role;
    use regards_core::paths::DataDirs;
    use regards_core::search::parse;

    fn person(name: &str, address: &str) -> Person {
        Person {
            name: Some(name.into()),
            address: Some(address.into()),
        }
    }

    fn mail(uid: u32, subject: &str, from: Person, received_at: i64) -> NewMessage {
        NewMessage {
            uid,
            modseq: None,
            flags: MessageFlags::default(),
            received_at: Some(received_at),
            size: Some(100),
            headers: Headers {
                subject: subject.into(),
                from: Some(from),
                to: vec![person("Me", "me@example.org")],
                message_id: Some(format!("m{uid}-{subject}@example.org")),
                ..Headers::default()
            },
        }
    }

    /// Account `a1` with its Inbox, Archive (in a folder) and Trash.
    fn seed(tx: &Transaction<'_>) -> Result<[i64; 3], StoreError> {
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
        )?;
        let remote = |name: &str, role| RemoteMailbox {
            remote_name: name.as_bytes().to_vec(),
            name: name.into(),
            delimiter: Some('/'),
            role,
            selectable: true,
            attributes: vec![],
        };
        mailboxes::reconcile(
            tx,
            "a1",
            &[
                remote("INBOX", Some(Role::Inbox)),
                remote("Projects/Archive", Some(Role::Archive)),
                remote("Trash", Some(Role::Trash)),
            ],
        )?;
        let all = mailboxes::list(tx, "a1")?;
        let with = |role| all.iter().find(|m| m.role == Some(role)).map(|m| m.id).unwrap();
        Ok([with(Role::Inbox), with(Role::Archive), with(Role::Trash)])
    }

    async fn store() -> (tempfile::TempDir, Store, [i64; 3]) {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let folders = store.write(seed).await.unwrap().value;
        (temp, store, folders)
    }

    /// Index what is queued.
    async fn settle(store: &Store) {
        while store.write(|tx| index(tx, 7)).await.unwrap().value > 0 {}
    }

    async fn find(store: &Store, query: &str) -> Vec<String> {
        settle(store).await;
        found(store, query).await
    }

    /// As it is now, without indexing what is queued.
    async fn found(store: &Store, query: &str) -> Vec<String> {
        let query = parse(query);
        store
            .read(move |conn| search(conn, &query, None, 50, None))
            .await
            .unwrap()
            .page
            .into_iter()
            .map(|m| m.subject)
            .collect()
    }

    #[tokio::test]
    async fn finds_by_subject_people_and_text_as_they_are_written() {
        let (_temp, store, [inbox, archive, _]) = store().await;
        store
            .write(move |tx| {
                messages::upsert(
                    tx,
                    inbox,
                    &[
                        mail(1, "Garden plot", person("Léna Müller", "lena@garden.example"), 1_000),
                        mail(2, "Train tickets", person("Bob", "bob@rail.example"), 2_000),
                        mail(3, "Café opening", person("Cy", "cy@example.org"), 3_000),
                    ],
                )?;
                messages::upsert(
                    tx,
                    archive,
                    &[mail(4, "Old plans", person("Bob", "bob@rail.example"), 500)],
                )?;
                messages::set_details(tx, inbox, 2, true, Some("Your seats are booked for Lugano"))
            })
            .await
            .unwrap();
        assert_eq!(find(&store, "garden").await, ["Garden plot"]);
        assert_eq!(
            find(&store, "gard").await,
            ["Garden plot"],
            "the last word is typed so far"
        );
        assert_eq!(find(&store, "lena").await, ["Garden plot"], "accents are not needed");
        assert_eq!(find(&store, "muller").await, ["Garden plot"]);
        assert_eq!(find(&store, "cafe").await, ["Café opening"]);
        assert_eq!(find(&store, "lugano").await, ["Train tickets"], "the preview is text");
        assert_eq!(
            find(&store, "from:bob").await,
            ["Train tickets", "Old plans"],
            "newest first"
        );
        assert_eq!(find(&store, "bob -tickets").await, ["Old plans"]);
        assert_eq!(
            find(&store, "to:me@example.org has:attachment").await,
            ["Train tickets"]
        );
        assert_eq!(
            find(&store, "in:archive").await,
            ["Old plans"],
            "the folder's last name part"
        );
        assert_eq!(
            find(&store, r#""plot garden""#).await,
            Vec::<String>::new(),
            "a phrase keeps its order"
        );
        assert_eq!(
            find(&store, "address").await,
            Vec::<String>::new(),
            "recipients are names, not JSON"
        );
        assert_eq!(
            find(&store, "after:1970-01-01 before:1970-01-01").await,
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    async fn whatever_is_typed_is_a_search_never_an_error() {
        let (_temp, store, [inbox, ..]) = store().await;
        store
            .write(move |tx| messages::upsert(tx, inbox, &[mail(1, "A + B: the plan", person("Ann", "ann@x.org"), 1)]))
            .await
            .unwrap();
        settle(&store).await;
        for typed in [
            "\"",
            "-",
            ":",
            "from:",
            "\"unterminated plan",
            "a:b:c",
            "***",
            "NEAR(a b)",
            "^plan",
            "subject:plan*",
            "plan OR",
            "AND",
            "(plan",
            "plan)",
            "-\"\"",
            "in:",
            "before:2026-02-31",
            "ⅷ",
            "👍 plan",
            "plan -",
            "{subject sender}: plan",
            "sender:ann",
            "rowid:1",
        ] {
            let query = parse(typed);
            let found = store.read(move |conn| search(conn, &query, None, 10, None)).await;
            assert!(found.is_ok(), "{typed:?}: {found:?}");
        }
    }

    #[tokio::test]
    async fn the_index_follows_changes_moves_and_deletions() {
        let (_temp, store, [inbox, archive, trash]) = store().await;
        store
            .write(move |tx| {
                messages::upsert(
                    tx,
                    inbox,
                    &[mail(1, "Quarterly report", person("Ann", "ann@example.org"), 1_000)],
                )
            })
            .await
            .unwrap();
        // Downloaded: all of its text.
        let row = store
            .read(move |conn| messages::list(conn, inbox, 1, None))
            .await
            .unwrap()[0]
            .clone();
        let message_id = row.message_id.clone();
        store
            .write(move |tx| {
                set_text(
                    tx,
                    "a1",
                    row.id,
                    message_id.as_deref(),
                    "Figures attached. Margins improved in Zug.",
                    5,
                )
            })
            .await
            .unwrap();
        assert_eq!(find(&store, "zug").await, ["Quarterly report"]);

        // Moved: the archived copy is a new row, deleted from the inbox first.
        store
            .write(move |tx| {
                messages::delete_uids(tx, inbox, &[1])?;
                let mut moved = mail(1, "Quarterly report", person("Ann", "ann@example.org"), 1_000);
                moved.uid = 7;
                messages::upsert(tx, archive, &[moved])
            })
            .await
            .unwrap();
        assert_eq!(
            find(&store, "zug margins").await,
            ["Quarterly report"],
            "its text came along"
        );

        // Changed on the server (a new subject), then thrown away.
        let renamed = |uid| {
            let mut renamed = mail(1, "Annual report", person("Ann", "ann@example.org"), 1_000);
            renamed.uid = uid;
            renamed.headers.message_id = Some("m1-Quarterly report@example.org".into());
            renamed
        };
        store
            .write(move |tx| messages::upsert(tx, archive, &[renamed(7)]))
            .await
            .unwrap();
        assert_eq!(find(&store, "annual zug").await, ["Annual report"]);
        assert!(find(&store, "quarterly").await.is_empty());
        store
            .write(move |tx| {
                messages::delete_uids(tx, archive, &[7])?;
                messages::upsert(tx, trash, &[renamed(8)])
            })
            .await
            .unwrap();
        assert!(find(&store, "annual").await.is_empty(), "Trash is left out");
        assert_eq!(find(&store, "annual in:trash").await, ["Annual report"]);
        assert_eq!(find(&store, "annual in:anywhere").await, ["Annual report"]);

        // The account goes: so does what it had in the index.
        store.write(|tx| accounts::delete(tx, "a1")).await.unwrap();
        settle(&store).await;
        let left: i64 = store
            .read(|conn| Ok(conn.query_row("SELECT count(*) FROM search.mail_docsize", [], |r| r.get(0))?))
            .await
            .unwrap();
        assert_eq!(left, 0);
        // And its kept text, once a while has passed.
        let forgotten = store.write(|tx| forget_texts(tx, 10)).await.unwrap().value;
        assert_eq!(forgotten, 1);
    }

    #[tokio::test]
    async fn one_copy_of_each_message_and_the_best_matches_first() {
        let (_temp, store, [inbox, archive, _]) = store().await;
        store
            .write(move |tx| {
                messages::upsert(
                    tx,
                    archive,
                    &[
                        mail(1, "Budget", person("Ann", "ann@example.org"), 1_000),
                        mail(2, "Budget budget budget", person("Ann", "ann@example.org"), 1_500),
                    ],
                )?;
                // The same message in the inbox too (a Gmail label, say).
                messages::upsert(tx, inbox, &[mail(1, "Budget", person("Ann", "ann@example.org"), 1_000)])?;
                messages::upsert(
                    tx,
                    inbox,
                    &(10..18)
                        .map(|uid| {
                            mail(
                                uid,
                                "Lunch",
                                person("Budget Office", "office@example.org"),
                                2_000 + i64::from(uid),
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .await
            .unwrap();
        settle(&store).await;
        let query = parse("budget");
        let hits = store
            .read(move |conn| search(conn, &query, None, 4, None))
            .await
            .unwrap();
        assert_eq!(hits.page.len(), 4);
        assert_eq!(hits.top[0].subject, "Budget budget budget", "most about it");
        let all = find(&store, "budget").await;
        assert_eq!(all.len(), 10, "one of the two copies");
        let copy = store
            .read(|conn| search(conn, &parse("subject:budget -budget"), None, 50, None))
            .await
            .unwrap();
        assert!(copy.page.is_empty());
        let shown = store
            .read(|conn| search(conn, &parse("subject:budget"), None, 50, None))
            .await
            .unwrap()
            .page;
        let budget = shown.iter().find(|m| m.subject == "Budget").unwrap();
        assert_eq!(budget.mailbox_id, inbox, "the inbox's copy is shown");

        // Pages go on where the last ended.
        let first = store
            .read(|conn| search(conn, &parse("budget"), None, 6, None))
            .await
            .unwrap()
            .page;
        let last = first.last().unwrap();
        let cursor = Cursor {
            received_at: last.received_at,
            id: last.id,
        };
        let rest = store
            .read(move |conn| search(conn, &parse("budget"), None, 6, Some(cursor)))
            .await
            .unwrap();
        assert_eq!(rest.page.len(), 4);
        assert!(rest.top.is_empty(), "only on the first page");
    }

    #[tokio::test]
    async fn a_new_schema_rebuilds_the_index_while_search_goes_on() {
        // As an older Regards left it: messages, and an index of another version.
        let temp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::at(temp.path());
        {
            let store = Store::open(&dirs).await.unwrap();
            store
                .write(|tx| {
                    let [inbox, ..] = seed(tx)?;
                    let batch: Vec<NewMessage> = (1..=25)
                        .map(|uid| {
                            mail(
                                uid,
                                &format!("Note {uid}"),
                                person("Ann", "ann@example.org"),
                                i64::from(uid),
                            )
                        })
                        .collect();
                    messages::upsert(tx, inbox, &batch)?;
                    tx.execute_batch("PRAGMA search.user_version = 0")?;
                    Ok(())
                })
                .await
                .unwrap();
        }
        let store = Store::open(&dirs).await.unwrap();
        assert_eq!(store.read(pending).await.unwrap(), 25);
        assert!(found(&store, "note").await.is_empty(), "nothing indexed yet");
        assert_eq!(store.write(|tx| index(tx, 10)).await.unwrap().value, 15);
        let newest = found(&store, "note").await;
        assert_eq!(newest.len(), 10);
        assert_eq!(newest[0], "Note 25", "the newest first");
        assert_eq!(found(&store, "note 16").await, ["Note 16"]);
        assert!(found(&store, "note 15").await.is_empty());
        assert_eq!(find(&store, "note").await.len(), 25);
        assert_eq!(store.read(pending).await.unwrap(), 0);
        assert_eq!(store.write(|tx| index(tx, 10)).await.unwrap().value, 0, "done");
    }
}
