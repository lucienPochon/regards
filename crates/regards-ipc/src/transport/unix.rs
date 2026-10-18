//! Unix domain socket transport.
#![allow(unsafe_code)]

use super::Stream;
use crate::endpoint::Endpoint;
use std::fs::{self, Permissions};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tokio::net::{UnixListener, UnixStream};

pub struct Listener {
    inner: UnixListener,
    path: PathBuf,
}

impl Listener {
    pub async fn bind(endpoint: &Endpoint) -> io::Result<Self> {
        let path = endpoint.socket_path().to_path_buf();
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "socket path has no parent directory"))?;
        fs::create_dir_all(dir)?;
        fs::set_permissions(dir, Permissions::from_mode(0o700))?;

        if path.exists() {
            // A live socket means another daemon is serving this data directory.
            // A dead one is left over from a crash and is safe to replace.
            if UnixStream::connect(&path).await.is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another Regards daemon is listening on this socket",
                ));
            }
            fs::remove_file(&path)?;
        }

        let inner = UnixListener::bind(&path)?;
        fs::set_permissions(&path, Permissions::from_mode(0o600))?;
        Ok(Self { inner, path })
    }

    pub async fn accept(&mut self) -> io::Result<Stream> {
        loop {
            let (stream, _) = self.inner.accept().await?;
            match stream.peer_cred() {
                Ok(cred) if cred.uid() == current_uid() => return Ok(Box::new(stream)),
                Ok(cred) => tracing::warn!(uid = cred.uid(), "rejected a connection from another user"),
                Err(error) => tracing::warn!(%error, "rejected a connection without peer credentials"),
            }
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub async fn connect(endpoint: &Endpoint) -> io::Result<Stream> {
    Ok(Box::new(UnixStream::connect(endpoint.socket_path()).await?))
}

fn current_uid() -> libc::uid_t {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}
