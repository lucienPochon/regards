//! Per-user data locations.
//!
//! One daemon owns one data directory (ADR-0005). The default is the platform's
//! per-user local data directory; `REGARDS_DATA_DIR` overrides it for
//! development and tests, so isolated instances never touch real data.

use std::path::{Path, PathBuf};

/// Environment variable that overrides the data directory.
pub const DATA_DIR_ENV: &str = "REGARDS_DATA_DIR";

/// Directory name used inside the platform data directory.
const APP_DIR_NAME: &str = "Regards";

/// Locations used by one Regards instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDirs {
    data: PathBuf,
}

impl DataDirs {
    /// Resolve the data directory from `REGARDS_DATA_DIR` or the platform default:
    /// `%LOCALAPPDATA%\Regards` on Windows, `~/Library/Application Support/Regards`
    /// on macOS and `$XDG_DATA_HOME/Regards` (usually `~/.local/share/Regards`) on Linux.
    pub fn resolve() -> Result<Self, PathError> {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::at(dir));
        }
        let base = dirs::data_local_dir().ok_or(PathError::NoDataDir)?;
        Ok(Self::at(base.join(APP_DIR_NAME)))
    }

    /// Use an explicit data directory.
    pub fn at(data: impl Into<PathBuf>) -> Self {
        Self { data: data.into() }
    }

    /// Root of the instance's data.
    pub fn data(&self) -> &Path {
        &self.data
    }

    /// File locked exclusively by the running daemon.
    pub fn lock_file(&self) -> PathBuf {
        self.data.join("daemon.lock")
    }

    /// Directory for runtime files such as the daemon socket on Unix.
    pub fn runtime(&self) -> PathBuf {
        self.data.join("run")
    }

    /// The main database (ADR-0008).
    pub fn mail_db(&self) -> PathBuf {
        self.data.join("mail.db")
    }

    /// Content-addressed files: downloaded messages (ADR-0008).
    pub fn blobs(&self) -> PathBuf {
        self.data.join("blobs")
    }

    /// Copies of `mail.db` taken before each schema migration.
    pub fn backups(&self) -> PathBuf {
        self.data.join("backups")
    }

    /// Log of a daemon started in the background by a client. It never
    /// contains message content or secrets.
    pub fn daemon_log(&self) -> PathBuf {
        self.data.join("logs").join("daemon.log")
    }

    /// Short identifier derived from the data directory. It names per-instance
    /// resources such as the Windows named pipe; it is not a secret.
    pub fn scope_id(&self) -> String {
        let normalized = self.data.to_string_lossy().replace('\\', "/");
        #[cfg(any(windows, target_os = "macos"))]
        let normalized = normalized.to_lowercase();
        format!("{:016x}", fnv1a64(normalized.trim_end_matches('/').as_bytes()))
    }
}

/// Errors resolving data locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// The platform reports no per-user data directory.
    NoDataDir,
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDataDir => write!(
                f,
                "no per-user data directory is available; set {DATA_DIR_ENV} to choose one"
            ),
        }
    }
}

impl std::error::Error for PathError {}

/// 64-bit FNV-1a: small, stable across platforms and Rust versions.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_paths_live_inside_the_data_dir() {
        let dirs = DataDirs::at("/tmp/regards-test");
        assert_eq!(dirs.lock_file(), Path::new("/tmp/regards-test/daemon.lock"));
        assert_eq!(dirs.runtime(), Path::new("/tmp/regards-test/run"));
    }

    #[test]
    fn scope_id_is_stable_and_distinct() {
        let a = DataDirs::at("/tmp/a");
        let b = DataDirs::at("/tmp/b");
        assert_eq!(a.scope_id(), DataDirs::at("/tmp/a/").scope_id());
        assert_ne!(a.scope_id(), b.scope_id());
        assert_eq!(a.scope_id().len(), 16);
    }

    #[test]
    fn fnv_matches_reference_vector() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
