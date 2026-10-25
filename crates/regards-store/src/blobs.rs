//! Content-addressed files under `blobs/` (ADR-0008): a downloaded message is
//! stored once, named by the SHA-256 of its bytes. Writes are atomic (a
//! temporary file renamed into place), so a crash never leaves half a file.
//! Files nothing points to any more are removed by [`Blobs::remove_unused`].

use crate::StoreError;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
pub struct Blobs {
    root: PathBuf,
    /// Held while a stored file is reused or removed, so one is never
    /// handed out again just as a sweep takes it away.
    using: Arc<Mutex<()>>,
}

impl Blobs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            using: Arc::default(),
        }
    }

    /// Store `bytes`; returns their hash, the name to keep in the database.
    pub fn put(&self, bytes: &[u8]) -> std::io::Result<String> {
        let hash = hex(&Sha256::digest(bytes));
        let path = self.path(&hash).expect("a SHA-256 hex digest is a valid name");
        if self.reuse(&path) {
            return Ok(hash);
        }
        let dir = path.parent().expect("blob paths have a parent");
        std::fs::create_dir_all(dir)?;
        let mut temp = tempfile_in(dir)?;
        temp.1.write_all(bytes)?;
        temp.1.sync_all()?;
        drop(temp.1);
        match std::fs::rename(&temp.0, &path) {
            Ok(()) => Ok(hash),
            // Someone stored the same bytes meanwhile; theirs is identical.
            Err(_) if path.exists() => {
                let _ = std::fs::remove_file(&temp.0);
                Ok(hash)
            }
            Err(error) => {
                let _ = std::fs::remove_file(&temp.0);
                Err(error)
            }
        }
    }

    /// The bytes stored under `hash`, or `None` if there are none (or the hash is invalid).
    pub fn get(&self, hash: &str) -> std::io::Result<Option<Vec<u8>>> {
        let Some(path) = self.path(hash) else {
            return Ok(None);
        };
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Remove the files whose names are not in `keep`, once older than
    /// `grace` (one just stored may not be in the database yet), and the
    /// temporary files a crash left behind. Returns the files and bytes removed.
    pub fn remove_unused(&self, keep: &HashSet<String>, grace: Duration) -> std::io::Result<(usize, u64)> {
        let mut removed = (0, 0);
        let folders = match std::fs::read_dir(&self.root) {
            Ok(folders) => folders,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(removed),
            Err(error) => return Err(error),
        };
        for folder in folders.flatten() {
            let Ok(files) = std::fs::read_dir(folder.path()) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                let name = file.file_name();
                let Some(name) = name.to_str() else { continue };
                let unused = self.path(name).is_some_and(|stored| stored == path) && !keep.contains(name);
                if !unused && !name.starts_with(".tmp-") {
                    continue;
                }
                let _using = self.lock();
                // Read again now (a listing may cache it): `put` may just have reused it.
                let Ok(metadata) = std::fs::metadata(&path) else {
                    continue;
                };
                let old = metadata
                    .modified()
                    .ok()
                    .and_then(|modified| modified.elapsed().ok())
                    .is_some_and(|age| age >= grace);
                if old && metadata.is_file() && std::fs::remove_file(&path).is_ok() {
                    removed.0 += 1;
                    removed.1 += metadata.len();
                }
            }
        }
        Ok(removed)
    }

    /// Mark a stored file as just used, so a sweep running now leaves it;
    /// `false` if there is none (or it cannot be marked: it is written again).
    fn reuse(&self, path: &Path) -> bool {
        let _using = self.lock();
        std::fs::File::options()
            .write(true)
            .open(path)
            .and_then(|file| file.set_modified(SystemTime::now()))
            .is_ok()
    }

    fn lock(&self) -> MutexGuard<'_, ()> {
        self.using.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `blobs/ab/abcdef…`; `None` for anything that is not a lower-case SHA-256 hex digest,
    /// so a stored value can never point outside the blob directory.
    fn path(&self, hash: &str) -> Option<PathBuf> {
        let valid = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        valid.then(|| self.root.join(&hash[..2]).join(hash))
    }
}

/// The names the database still points to: message sources and files
/// attached to drafts.
pub fn referenced(conn: &Connection) -> Result<HashSet<String>, StoreError> {
    let mut statement = conn.prepare(
        "SELECT blob_hash FROM messages WHERE blob_hash IS NOT NULL \
         UNION SELECT blob_hash FROM draft_attachments",
    )?;
    let names = statement.query_map([], |row| row.get(0))?;
    Ok(names.collect::<Result<_, _>>()?)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A new file next to the final location, so the rename stays on one volume.
fn tempfile_in(dir: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    for attempt in 0u32..100 {
        let path = dir.join(format!(".tmp-{}-{attempt}", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(ErrorKind::AlreadyExists, "no free temporary name"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_and_deduplicate() {
        let temp = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(temp.path());
        let hash = blobs.put(b"Subject: hi\r\n\r\nbody").unwrap();
        assert_eq!(hash.len(), 64);
        assert_eq!(blobs.get(&hash).unwrap().unwrap(), b"Subject: hi\r\n\r\nbody");
        assert_eq!(
            blobs.put(b"Subject: hi\r\n\r\nbody").unwrap(),
            hash,
            "same bytes, same name"
        );
        assert!(temp.path().join(&hash[..2]).join(&hash).is_file());
    }

    #[test]
    fn unknown_or_malformed_hashes_find_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(temp.path());
        assert!(blobs.get(&"0".repeat(64)).unwrap().is_none());
        assert!(blobs.get("../../etc/passwd").unwrap().is_none());
        assert!(blobs.get(&"A".repeat(64)).unwrap().is_none());
    }

    #[test]
    fn unused_files_go_once_old_and_used_or_new_ones_stay() {
        let temp = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(temp.path());
        let kept = blobs.put(b"kept").unwrap();
        let unused = blobs.put(b"unused").unwrap();
        let reused = blobs.put(b"reused").unwrap();
        let crashed = temp.path().join(&unused[..2]).join(".tmp-1-0");
        std::fs::write(&crashed, b"half").unwrap();
        let other = temp.path().join(&kept[..2]).join("notes.txt");
        std::fs::write(&other, b"not a blob").unwrap();
        let keep = HashSet::from([kept.clone()]);
        let grace = Duration::from_secs(3600);

        assert_eq!(blobs.remove_unused(&keep, grace).unwrap(), (0, 0), "too new to tell");

        let long_ago = SystemTime::now() - 2 * grace;
        for path in [&kept, &unused, &reused]
            .map(|hash| blobs.path(hash).unwrap())
            .into_iter()
            .chain([crashed.clone(), other.clone()])
        {
            let file = std::fs::File::options().write(true).open(path).unwrap();
            file.set_modified(long_ago).unwrap();
        }
        // Stored again just now: it may be about to go in the database.
        assert_eq!(blobs.put(b"reused").unwrap(), reused);

        assert_eq!(
            blobs.remove_unused(&keep, grace).unwrap(),
            (2, 10),
            "the unused file and the temporary one"
        );
        assert!(blobs.get(&kept).unwrap().is_some());
        assert!(blobs.get(&reused).unwrap().is_some());
        assert!(blobs.get(&unused).unwrap().is_none());
        assert!(!crashed.exists());
        assert!(other.exists(), "only files the store made are removed");
    }

    #[tokio::test]
    async fn message_sources_and_draft_files_are_in_use() {
        use crate::Store;
        use crate::accounts::{self, AuthKind, NewAccount, Security, Server};
        use crate::drafts::{self, Content};
        use crate::mailboxes::{self, RemoteMailbox};
        use crate::messages::{self, MessageFlags, NewMessage};
        use regards_core::headers::Headers;
        use regards_core::mailbox::Role;
        use regards_core::paths::DataDirs;

        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&DataDirs::at(temp.path())).await.unwrap();
        let (source, attached) = ("a".repeat(64), "b".repeat(64));
        let used = store
            .write(move |tx| {
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
                mailboxes::reconcile(
                    tx,
                    "a1",
                    &[RemoteMailbox {
                        remote_name: b"INBOX".to_vec(),
                        name: "INBOX".into(),
                        delimiter: Some('/'),
                        role: Some(Role::Inbox),
                        selectable: true,
                        attributes: vec![],
                    }],
                )?;
                let inbox = mailboxes::list(tx, "a1")?[0].id;
                let message = |uid| NewMessage {
                    uid,
                    modseq: None,
                    flags: MessageFlags::default(),
                    received_at: Some(1_000),
                    size: Some(100),
                    headers: Headers::default(),
                };
                messages::upsert(tx, inbox, &[message(1), message(2)])?;
                let first: i64 = tx.query_row("SELECT id FROM messages WHERE uid = 1", [], |row| row.get(0))?;
                messages::set_blob(tx, first, &source)?;
                let draft = drafts::create(tx, "a1", &Content::default(), 1_000)?;
                drafts::attach(tx, draft, &attached, "plan.pdf", "application/pdf", 10)?;
                referenced(tx)
            })
            .await
            .unwrap()
            .value;
        assert_eq!(
            used,
            HashSet::from(["a".repeat(64), "b".repeat(64)]),
            "not the message without a source"
        );
    }
}
