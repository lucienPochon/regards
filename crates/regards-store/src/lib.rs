//! Local storage for one Regards data directory (ADR-0008).
//!
//! - **`mail.db`:** SQLite in WAL mode with foreign keys on, migrated to the
//!   latest schema when opened.
//! - **One writer:** a dedicated thread owns the only write connection and runs
//!   transactions one at a time. Each committed write increments the database
//!   `revision`, the change feed's cursor (ADR-0007).
//! - **Readers:** a small pool of read-only connections serves queries
//!   concurrently with the writer.
//!
//! The daemon is the only process that opens the database (ADR-0005).

pub mod accounts;
pub mod blobs;
pub mod drafts;
pub mod mailboxes;
pub mod messages;
mod migrate;
pub mod operations;
pub mod outbox;
pub mod threads;
pub mod undo;

use regards_core::paths::DataDirs;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use tokio::sync::{Semaphore, oneshot, watch};

/// Read-only connections kept open.
const READERS: usize = 4;
/// How long a connection waits for a lock before failing.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Errors from the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database migration failed: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("the database was created by a newer version of Regards; update Regards to open it")]
    TooNew,
    #[error("could not access the data directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Conflict(String),
    #[error("stored data is invalid: {0}")]
    Corrupt(String),
    #[error("a database task failed unexpectedly")]
    Panicked,
    #[error("the store is closed")]
    Closed,
}

/// The value returned by a write transaction, and the revision it committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed<T> {
    pub value: T,
    pub revision: i64,
}

type WriteJob = Box<dyn FnOnce(&mut Connection) + Send>;

/// Handle to an open store. Cheap to clone.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

struct Inner {
    writer: mpsc::Sender<WriteJob>,
    readers: Mutex<Vec<Connection>>,
    read_permits: Arc<Semaphore>,
    /// The latest committed revision, for change notifications (ADR-0007).
    revisions: watch::Sender<i64>,
}

impl Store {
    /// Open (creating if needed) and migrate the database of `dirs`.
    pub async fn open(dirs: &DataDirs) -> Result<Self, StoreError> {
        let dirs = dirs.clone();
        tokio::task::spawn_blocking(move || Self::open_blocking(&dirs.mail_db(), &dirs.backups()))
            .await
            .map_err(|_| StoreError::Panicked)?
    }

    fn open_blocking(path: &Path, backups: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut writer = Connection::open(path)?;
        writer.busy_timeout(BUSY_TIMEOUT)?;
        writer.pragma_update(None, "journal_mode", "WAL")?;
        writer.pragma_update(None, "synchronous", "NORMAL")?;
        writer.pragma_update(None, "foreign_keys", true)?;
        migrate::to_latest(&mut writer, backups)?;

        let readers = (0..READERS)
            .map(|_| {
                let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                conn.busy_timeout(BUSY_TIMEOUT)?;
                conn.pragma_update(None, "foreign_keys", true)?;
                Ok(conn)
            })
            .collect::<Result<Vec<_>, StoreError>>()?;

        let revision: i64 = writer.query_row("SELECT value FROM revision", [], |row| row.get(0))?;
        let (sender, jobs) = mpsc::channel::<WriteJob>();
        std::thread::Builder::new()
            .name("regards-store-writer".into())
            .spawn(move || {
                let mut conn = writer;
                // Ends when every `Store` handle is dropped.
                for job in jobs {
                    job(&mut conn);
                }
            })?;

        Ok(Self {
            inner: Arc::new(Inner {
                writer: sender,
                readers: Mutex::new(readers),
                read_permits: Arc::new(Semaphore::new(READERS)),
                revisions: watch::Sender::new(revision),
            }),
        })
    }

    /// Run `f` in an immediate write transaction and commit it, incrementing the revision.
    /// If `f` fails, nothing is written.
    pub async fn write<T, F>(&self, f: F) -> Result<Committed<T>, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> Result<T, StoreError> + Send + 'static,
    {
        let (done, result) = oneshot::channel();
        let job: WriteJob = Box::new(move |conn| {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let value = f(&tx)?;
                let revision = tx.query_row("UPDATE revision SET value = value + 1 RETURNING value", [], |row| {
                    row.get(0)
                })?;
                tx.commit()?;
                Ok(Committed { value, revision })
            }))
            .unwrap_or(Err(StoreError::Panicked));
            let _ = done.send(outcome);
        });
        self.inner.writer.send(job).map_err(|_| StoreError::Closed)?;
        let committed = result.await.map_err(|_| StoreError::Closed)??;
        // Writes finish in order, but callers may resume out of order: keep the highest.
        self.inner.revisions.send_if_modified(|latest| {
            let newer = committed.revision > *latest;
            if newer {
                *latest = committed.revision;
            }
            newer
        });
        Ok(committed)
    }

    /// Follow the latest committed revision: the receiver wakes after each commit.
    pub fn subscribe(&self) -> watch::Receiver<i64> {
        self.inner.revisions.subscribe()
    }

    /// Run `f` on a read-only connection.
    pub async fn read<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, StoreError> + Send + 'static,
    {
        let permit = Arc::clone(&self.inner.read_permits)
            .acquire_owned()
            .await
            .map_err(|_| StoreError::Closed)?;
        let inner = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || {
            let conn = inner
                .readers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .pop()
                .expect("a permit guarantees a free connection");
            let outcome = catch_unwind(AssertUnwindSafe(|| f(&conn))).unwrap_or(Err(StoreError::Panicked));
            inner
                .readers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(conn);
            drop(permit);
            outcome
        })
        .await
        .map_err(|_| StoreError::Panicked)?
    }

    /// The revision of the latest committed write.
    pub async fn revision(&self) -> Result<i64, StoreError> {
        self.read(|conn| Ok(conn.query_row("SELECT value FROM revision", [], |row| row.get(0))?))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn open_temp() -> (tempfile::TempDir, Store) {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        (temp, store)
    }

    #[tokio::test]
    async fn opens_in_wal_mode_with_foreign_keys_and_fts5() {
        let (_temp, store) = open_temp().await;
        let (mode, fk, fts5) = store
            .read(|conn| {
                let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
                let fk: bool = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
                let fts5: bool = conn.query_row("SELECT sqlite_compileoption_used('ENABLE_FTS5')", [], |r| r.get(0))?;
                Ok((mode, fk, fts5))
            })
            .await
            .unwrap();
        assert_eq!(mode, "wal");
        assert!(fk);
        assert!(fts5, "search (ADR-0008 §4) needs FTS5 in the bundled SQLite");
    }

    #[tokio::test]
    async fn each_committed_write_increments_the_revision() {
        let (_temp, store) = open_temp().await;
        assert_eq!(store.revision().await.unwrap(), 0);
        let first = store.write(|_| Ok("a")).await.unwrap();
        let second = store.write(|_| Ok("b")).await.unwrap();
        assert_eq!((first.value, first.revision), ("a", 1));
        assert_eq!((second.value, second.revision), ("b", 2));
        assert_eq!(store.revision().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn a_failed_or_panicking_write_changes_nothing_and_the_writer_survives() {
        let (_temp, store) = open_temp().await;
        let failed = store
            .write(|tx| {
                tx.execute("UPDATE revision SET value = 99", [])?;
                Err::<(), _>(StoreError::Conflict("no".into()))
            })
            .await;
        assert!(matches!(failed, Err(StoreError::Conflict(_))));

        let panicked = store
            .write(|_| -> Result<(), StoreError> { panic!("bug in a write") })
            .await;
        assert!(matches!(panicked, Err(StoreError::Panicked)));

        assert_eq!(store.revision().await.unwrap(), 0);
        assert_eq!(store.write(|_| Ok(())).await.unwrap().revision, 1);
    }

    #[tokio::test]
    async fn subscribers_see_each_new_revision() {
        let (_temp, store) = open_temp().await;
        let mut revisions = store.subscribe();
        assert_eq!(*revisions.borrow(), 0);
        store.write(|_| Ok(())).await.unwrap();
        revisions.changed().await.unwrap();
        assert_eq!(*revisions.borrow_and_update(), 1);
        // A failed write commits nothing and wakes nobody.
        let _ = store.write(|_| Err::<(), _>(StoreError::Conflict("no".into()))).await;
        assert!(!revisions.has_changed().unwrap());
    }

    #[tokio::test]
    async fn readers_cannot_write() {
        let (_temp, store) = open_temp().await;
        let result = store
            .read(|conn| Ok(conn.execute("UPDATE revision SET value = 5", [])?))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn reopening_keeps_data_and_does_not_migrate_again() {
        let temp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::at(temp.path());
        let store = Store::open(&dirs).await.unwrap();
        store.write(|_| Ok(())).await.unwrap();
        drop(store);
        let store = Store::open(&dirs).await.unwrap();
        assert_eq!(store.revision().await.unwrap(), 1);
        assert!(!dirs.backups().exists(), "no migration ran, so no backup was taken");
    }
}
