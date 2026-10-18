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
