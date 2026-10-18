//! Where a daemon listens.

use regards_core::paths::DataDirs;
use std::fmt;
#[cfg(unix)]
use std::path::PathBuf;

/// The daemon's address for one data directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    #[cfg(windows)]
    pipe_name: String,
    #[cfg(unix)]
    socket_path: PathBuf,
}

impl Endpoint {
    /// Endpoint of the daemon that owns `dirs`.
    ///
    /// - Windows: `\\.\pipe\regards-<scope id>`
    /// - Unix: `<data dir>/run/daemon.sock`
    pub fn for_dirs(dirs: &DataDirs) -> Self {
        #[cfg(windows)]
        {
            Self {
                pipe_name: format!(r"\\.\pipe\regards-{}", dirs.scope_id()),
            }
        }
        #[cfg(unix)]
        {
            Self {
                socket_path: dirs.runtime().join("daemon.sock"),
            }
        }
    }

    #[cfg(windows)]
    pub fn pipe_name(&self) -> &str {
        &self.pipe_name
    }

    #[cfg(unix)]
    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[cfg(windows)]
        {
            f.write_str(&self.pipe_name)
        }
        #[cfg(unix)]
        {
            write!(f, "{}", self.socket_path.display())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_depends_on_data_dir() {
        let a = Endpoint::for_dirs(&DataDirs::at("/tmp/one"));
        let b = Endpoint::for_dirs(&DataDirs::at("/tmp/two"));
        assert_ne!(a, b);
        #[cfg(windows)]
        assert!(a.pipe_name().starts_with(r"\\.\pipe\regards-"));
        #[cfg(unix)]
        assert!(a.socket_path().ends_with("run/daemon.sock"));
    }
}
