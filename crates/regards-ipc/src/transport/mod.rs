//! Per-user local transports (ADR-0006).
//!
//! - Windows: a named pipe whose security descriptor grants access to the
//!   current user only, created as the first instance (anti-squatting) and
//!   rejecting remote clients.
//! - Unix: a socket in a `0700` directory, mode `0600`, accepting only peers
//!   with the daemon's effective UID.

use crate::endpoint::Endpoint;
use std::io;
use tokio::io::{AsyncRead, AsyncWrite};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as platform;
#[cfg(windows)]
use windows as platform;

/// A bidirectional byte stream to a peer.
pub trait IoStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> IoStream for T {}

/// A connected peer.
pub type Stream = Box<dyn IoStream>;

/// The daemon side of the transport.
pub struct Listener(platform::Listener);

impl Listener {
    /// Start listening on `endpoint`. Fails if another daemon already listens there.
    pub async fn bind(endpoint: &Endpoint) -> io::Result<Self> {
        platform::Listener::bind(endpoint).await.map(Self)
    }

    /// Wait for the next connection from the same OS user.
    pub async fn accept(&mut self) -> io::Result<Stream> {
        self.0.accept().await
    }
}

/// Connect to the daemon at `endpoint`.
pub async fn connect(endpoint: &Endpoint) -> io::Result<Stream> {
    platform::connect(endpoint).await
}

/// Whether a connection error means that no daemon is listening.
pub fn is_not_running(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing;
    use regards_core::paths::DataDirs;
    use serde_json::{Value, json};

    #[tokio::test]
    async fn listener_and_client_exchange_frames() {
        let temp = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::for_dirs(&DataDirs::at(temp.path()));
        let mut listener = Listener::bind(&endpoint).await.unwrap();

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.unwrap();
            let mut framed = framing::framed(stream);
            let message: Value = framing::recv(&mut framed).await.unwrap().unwrap();
            framing::send(&mut framed, &json!({ "echo": message })).await.unwrap();
        });

        let mut client = framing::framed(connect(&endpoint).await.unwrap());
        framing::send(&mut client, &json!("ping")).await.unwrap();
        let reply: Value = framing::recv(&mut client).await.unwrap().unwrap();
        assert_eq!(reply, json!({ "echo": "ping" }));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn connecting_without_a_daemon_reports_not_running() {
        let temp = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::for_dirs(&DataDirs::at(temp.path()));
        let error = connect(&endpoint).await.err().expect("no daemon is listening");
        assert!(is_not_running(&error), "unexpected error: {error:?}");
    }

    #[tokio::test]
    async fn a_second_listener_on_the_same_endpoint_fails() {
        let temp = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::for_dirs(&DataDirs::at(temp.path()));
        let _first = Listener::bind(&endpoint).await.unwrap();
        assert!(Listener::bind(&endpoint).await.is_err());
    }
}
