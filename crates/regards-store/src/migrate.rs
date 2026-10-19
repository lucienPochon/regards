//! Schema migrations: forward-only SQL embedded in the binary, with a backup
//! of the database before any migration of existing data (ADR-0008 §3).

use crate::StoreError;
use rusqlite::Connection;
use rusqlite_migration::{M, MigrationDefinitionError, Migrations};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Every migration, in order. Append only; never edit a merged migration.
const MIGRATIONS: &[M<'static>] = &[
    M::up(include_str!("../migrations/0001_accounts.sql")),
    M::up(include_str!("../migrations/0002_mailboxes.sql")),
    M::up(include_str!("../migrations/0003_messages.sql")),
    M::up(include_str!("../migrations/0004_message_blobs.sql")),
    M::up(include_str!("../migrations/0005_operations.sql")),
    M::up(include_str!("../migrations/0006_moves_and_undo.sql")),
    M::up(include_str!("../migrations/0007_threads.sql")),
    M::up(include_str!("../migrations/0008_delete_forever.sql")),
    M::up(include_str!("../migrations/0009_drafts_outbox.sql")),
    M::up(include_str!("../migrations/0010_draft_attachments.sql")),
    M::up(include_str!("../migrations/0011_draft_html.sql")),
    M::up(include_str!("../migrations/0012_marks.sql")),
    M::up(include_str!("../migrations/0013_send_later.sql")),
    M::up(include_str!("../migrations/0014_reminders.sql")),
    M::up(include_str!("../migrations/0015_notices.sql")),
    M::up(include_str!("../migrations/0016_identities.sql")),
    M::up(include_str!("../migrations/0017_smart_inbox.sql")),
    M::up(include_str!("../migrations/0018_list_details.sql")),
    M::up(include_str!("../migrations/0019_screening.sql")),
    M::up(include_str!("../migrations/0020_templates.sql")),
    M::up(include_str!("../migrations/0021_draft_sync.sql")),
    M::up(include_str!("../migrations/0022_search.sql")),
    M::up(include_str!("../migrations/0023_agent_access.sql")),
    M::up(include_str!("../migrations/0024_agent_audit.sql")),
    M::up(include_str!("../migrations/0025_agent_changes.sql")),
    M::up(include_str!("../migrations/0026_holds.sql")),
    M::up(include_str!("../migrations/0027_inline_images.sql")),
    M::up(include_str!("../migrations/0028_agent_send_window.sql")),
    M::up(include_str!("../migrations/0029_sender_images.sql")),
    M::up(include_str!("../migrations/0030_arrivals.sql")),
    M::up(include_str!("../migrations/0031_auto_copy.sql")),
];

/// Backups kept before older ones are deleted.
const BACKUPS_KEPT: usize = 3;

pub(crate) fn migrations() -> Migrations<'static> {
    Migrations::from_slice(MIGRATIONS)
}

/// Bring `conn` to the latest schema, backing it up first when it already holds data.
pub(crate) fn to_latest(conn: &mut Connection, backups: &Path) -> Result<(), StoreError> {
    run(&migrations(), conn, backups)
}

pub(crate) fn run(migrations: &Migrations<'_>, conn: &mut Connection, backups: &Path) -> Result<(), StoreError> {
    let pending = migrations.pending_migrations(conn)?;
    if pending < 0 {
        return Err(StoreError::TooNew);
    }
    if pending == 0 {
        return Ok(());
    }
    let from: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if from > 0 {
        let backup = backup(conn, backups, from)?;
        tracing::info!(from, pending, backup = %backup.display(), "migrating the database");
    }
    migrations.to_latest(conn).map_err(|error| match error {
        rusqlite_migration::Error::MigrationDefinition(MigrationDefinitionError::DatabaseTooFarAhead) => {
            StoreError::TooNew
        }
        other => other.into(),
    })
}

/// Copy the database with `VACUUM INTO`, then keep only the newest backups.
fn backup(conn: &Connection, dir: &Path, version: i64) -> Result<PathBuf, StoreError> {
    std::fs::create_dir_all(dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = dir.join(format!("mail-v{version}-{stamp}.db"));
    conn.execute("VACUUM INTO ?1", [path.to_string_lossy()])?;

    let mut backups: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("mail-v") && n.ends_with(".db"))
        })
        .collect();
    // Oldest first by modification time; anything beyond the newest few is removed, best effort.
    backups.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    for old in backups.iter().rev().skip(BACKUPS_KEPT) {
        let _ = std::fs::remove_file(old);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        migrations().validate().unwrap();
    }

    #[test]
    fn a_version_1_database_upgrades_with_a_backup_and_keeps_its_accounts() {
        let temp = tempfile::tempdir().unwrap();
        let backups = temp.path().join("backups");
        let mut conn = Connection::open(temp.path().join("mail.db")).unwrap();
        run(&Migrations::from_slice(&MIGRATIONS[..1]), &mut conn, &backups).unwrap();
        conn.execute(
            "INSERT INTO accounts (id, display_name, email, auth_kind, username, secret_key, imap_host, imap_port, \
             imap_security, smtp_host, smtp_port, smtp_security, created_at, position) \
             VALUES ('a', 'Ada', 'ada@example.test', 'password', 'ada', 'a/mail-password', 'h', 993, 'tls', 'h', 587, \
             'starttls', 0, 0)",
            [],
        )
        .unwrap();

        to_latest(&mut conn, &backups).unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
        let accounts: i64 = conn
            .query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(accounts, 1);
        assert_eq!(
            std::fs::read_dir(&backups).unwrap().count(),
            1,
            "a backup of the v1 database"
        );
    }

    #[test]
    fn the_outbox_rebuild_keeps_queued_messages() {
        // Migration 13 rebuilds the outbox; foreign keys are on, as in the store.
        let temp = tempfile::tempdir().unwrap();
        let backups = temp.path().join("backups");
        let mut conn = Connection::open(temp.path().join("mail.db")).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        run(&Migrations::from_slice(&MIGRATIONS[..12]), &mut conn, &backups).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (id, display_name, email, auth_kind, username, secret_key, imap_host, imap_port, \
             imap_security, smtp_host, smtp_port, smtp_security, created_at, position) \
             VALUES ('a', 'Ada', 'ada@example.test', 'password', 'ada', 'a/mail-password', 'h', 993, 'tls', 'h', 587, \
             'starttls', 0, 0); \
             INSERT INTO drafts (id, account_id, created_at, updated_at) VALUES (7, 'a', 0, 0); \
             INSERT INTO outbox (id, account_id, draft_id, message_id, subject, recipients_json, envelope_from, \
             envelope_to, mime, mime_sha256, state, undo_until, next_attempt_at, attempts, created_at) \
             VALUES (3, 'a', 7, 'm@example.test', 'Hi', '[]', 'ada@example.test', '[\"b@example.test\"]', \
             x'00', 'h', 'failed', 5, 6, 2, 1);",
        )
        .unwrap();

        to_latest(&mut conn, &backups).unwrap();
        let row: (i64, Option<i64>, String, Option<i64>, String, i64) = conn
            .query_row(
                "SELECT id, draft_id, state, send_at, executor, attempts FROM outbox",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .unwrap();
        assert_eq!(row, (3, Some(7), "failed".into(), None, "device".into(), 2));
        let broken: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(broken, 0);
        // The rebuilt table's foreign keys act as before.
        conn.execute("DELETE FROM drafts WHERE id = 7", []).unwrap();
        let draft: Option<i64> = conn.query_row("SELECT draft_id FROM outbox", [], |r| r.get(0)).unwrap();
        assert_eq!(draft, None, "ON DELETE SET NULL still holds");
    }

    #[test]
    fn the_smart_inbox_knows_who_the_account_wrote_to_from_mail_already_stored() {
        let temp = tempfile::tempdir().unwrap();
        let backups = temp.path().join("backups");
        let mut conn = Connection::open(temp.path().join("mail.db")).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        run(&Migrations::from_slice(&MIGRATIONS[..16]), &mut conn, &backups).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (id, display_name, email, auth_kind, username, secret_key, imap_host, imap_port, \
             imap_security, smtp_host, smtp_port, smtp_security, created_at, position) \
             VALUES ('a', 'Ada', 'ada@example.test', 'password', 'ada', 'a/mail-password', 'h', 993, 'tls', 'h', 587, \
             'starttls', 0, 0); \
             INSERT INTO identities (account_id, name, email, is_default, created_at) \
             VALUES ('a', 'Ada', 'ada@example.test', 1, 0), ('a', 'Work', 'work@example.test', 0, 0); \
             INSERT INTO mailboxes (id, account_id, remote_name, name, selectable) VALUES (1, 'a', x'53656e74', 'Sent', 1); \
             INSERT INTO messages (mailbox_id, uid, received_at, from_address, to_json, cc_json) VALUES \
             (1, 1, 50, 'Ada@example.test', '[{\"name\":null,\"address\":\"Lena@example.net\"}]', \
              '[{\"name\":null,\"address\":\"bob@example.org\"}]'), \
             (1, 2, 30, 'work@example.test', '[{\"name\":null,\"address\":\"lena@example.net\"}]', '[]'), \
             (1, 3, 10, 'someone@else.test', '[{\"name\":null,\"address\":\"cy@example.org\"}]', '[]');",
        )
        .unwrap();

        to_latest(&mut conn, &backups).unwrap();
        let mut statement = conn
            .prepare("SELECT address, written_to_at FROM senders ORDER BY address")
            .unwrap();
        let senders: Vec<(String, i64)> = statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        // Lena first written to at 30 (from the alias), Bob at 50; not Cy.
        assert_eq!(
            senders,
            [("bob@example.org".to_owned(), 50), ("lena@example.net".to_owned(), 30)]
        );
    }

    #[test]
    fn a_new_database_is_not_backed_up_but_an_existing_one_is() {
        let temp = tempfile::tempdir().unwrap();
        let backups = temp.path().join("backups");
        let mut conn = Connection::open(temp.path().join("mail.db")).unwrap();

        let v1 = Migrations::new(vec![M::up("CREATE TABLE a (x INTEGER) STRICT;")]);
        run(&v1, &mut conn, &backups).unwrap();
        assert!(!backups.exists(), "nothing to back up in a new database");
        conn.execute("INSERT INTO a VALUES (42)", []).unwrap();

        let v2 = Migrations::new(vec![
            M::up("CREATE TABLE a (x INTEGER) STRICT;"),
            M::up("CREATE TABLE b (y INTEGER) STRICT;"),
        ]);
        run(&v2, &mut conn, &backups).unwrap();
        let files: Vec<_> = std::fs::read_dir(&backups)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        let copy = Connection::open(&files[0]).unwrap();
        let x: i64 = copy.query_row("SELECT x FROM a", [], |r| r.get(0)).unwrap();
        assert_eq!(x, 42, "the backup holds the pre-migration data");

        // Running an older build against the newer database is refused.
        assert!(matches!(run(&v1, &mut conn, &backups), Err(StoreError::TooNew)));
    }
}
