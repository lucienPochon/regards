//! TLS for IMAP connections, from `regards-net` (ADR-0009): TLS from the
//! first byte, with the operating system's certificate verifier.

use crate::ImapError;
use regards_net::{NetError, TlsStream};
use tokio::net::TcpStream;

pub use regards_net::TlsOptions;

/// Open TCP to `host:port` and complete a TLS handshake that verifies `host`.
pub(crate) async fn connect(host: &str, port: u16, options: &TlsOptions) -> Result<TlsStream<TcpStream>, ImapError> {
    regards_net::connect_tls(host, port, options).await.map_err(Into::into)
}

impl From<NetError> for ImapError {
    fn from(error: NetError) -> Self {
        match error {
            NetError::Connect(message) => ImapError::Connect(message),
            NetError::Tls(message) => ImapError::Tls(message),
            NetError::Timeout(what) => ImapError::Timeout(what),
        }
    }
}
