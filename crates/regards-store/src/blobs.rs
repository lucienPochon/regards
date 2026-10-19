//! Content-addressed files under `blobs/` (ADR-0008): a downloaded message is
//! stored once, named by the SHA-256 of its bytes. Writes are atomic (a
//! temporary file renamed into place), so a crash never leaves half a file.

use sha2::{Digest, Sha256};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Blobs {
    root: PathBuf,
}

impl Blobs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Store `bytes`; returns their hash, the name to keep in the database.
    pub fn put(&self, bytes: &[u8]) -> std::io::Result<String> {
        let hash = hex(&Sha256::digest(bytes));
        let path = self.path(&hash).expect("a SHA-256 hex digest is a valid name");
        if path.exists() {
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

    /// `blobs/ab/abcdef…`; `None` for anything that is not a lower-case SHA-256 hex digest,
    /// so a stored value can never point outside the blob directory.
    fn path(&self, hash: &str) -> Option<PathBuf> {
        let valid = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        valid.then(|| self.root.join(&hash[..2]).join(hash))
    }
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
}
