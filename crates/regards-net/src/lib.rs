//! Network connections for mail protocols (ADR-0009): TCP that tries IPv6
//! and IPv4 side by side, and TLS with rustls and the operating system's
//! certificate verifier, either from the first byte or after STARTTLS.
//! Validation is always on; there is no way to accept an invalid certificate.

pub mod connlog;
pub mod https;
pub mod loopback;

use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, crypto};

pub use tokio_rustls::client::TlsStream;

/// How long TCP connect, or a TLS handshake, may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Why a connection could not be made, in words for the account owner.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    #[error("{0}")]
    Connect(String),
    #[error("{0}")]
    Tls(String),
    #[error("the server did not answer in time ({0})")]
    Timeout(&'static str),
}

/// Which certificates to trust. The default trusts what the operating system trusts.
#[derive(Clone, Default)]
pub struct TlsOptions {
    extra_roots: Vec<CertificateDer<'static>>,
}

impl TlsOptions {
    /// Trust the operating system's root certificates only.
    pub fn platform() -> Self {
        Self::default()
    }

    /// Also trust the CA certificates in `pem`. For tests and local
    /// development servers only; never offered in the UI.
    pub fn with_extra_roots_pem(mut self, pem: &[u8]) -> Result<Self, NetError> {
        for cert in CertificateDer::pem_slice_iter(pem) {
            let cert = cert.map_err(|error| NetError::Tls(format!("invalid extra root certificate: {error}")))?;
            self.extra_roots.push(cert);
        }
        Ok(self)
    }

    fn client_config(&self) -> Result<Arc<ClientConfig>, NetError> {
        let provider = Arc::new(crypto::ring::default_provider());
        let verifier = if self.extra_roots.is_empty() {
            rustls_platform_verifier::Verifier::new(Arc::clone(&provider))
        } else {
            rustls_platform_verifier::Verifier::new_with_extra_roots(self.extra_roots.clone(), Arc::clone(&provider))
        }
        .map_err(|error| NetError::Tls(format!("cannot load the system certificates: {error}")))?;
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|error| NetError::Tls(error.to_string()))?
            .dangerous() // Only to install the platform verifier, which does full validation.
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        Ok(Arc::new(config))
    }
}

/// Open TCP to `host:port` and complete a TLS handshake that verifies `host`.
pub async fn connect_tls(host: &str, port: u16, options: &TlsOptions) -> Result<TlsStream<TcpStream>, NetError> {
    let tcp = connect(host, port).await?;
    start_tls(tcp, host, port, options).await
}

/// Open TCP to `host:port`, for a protocol that upgrades with STARTTLS.
pub async fn connect(host: &str, port: u16) -> Result<TcpStream, NetError> {
    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, connect_tcp(host, port))
        .await
        .map_err(|_| NetError::Timeout("connecting"))?
        .map_err(|error| NetError::Connect(format!("cannot reach {host}:{port}: {error}")))?;
    tcp.set_nodelay(true).ok();
    Ok(tcp)
}

/// TLS on an open connection, verifying `host` (after STARTTLS, or right away).
pub async fn start_tls(
    tcp: TcpStream,
    host: &str,
    port: u16,
    options: &TlsOptions,
) -> Result<TlsStream<TcpStream>, NetError> {
    let name = ServerName::try_from(host.to_owned())
        .map_err(|_| NetError::Connect(format!("{host:?} is not a valid server name")))?;
    let connector = TlsConnector::from(options.client_config()?);
    let handshake = connector.connect(name, tcp);
    tokio::time::timeout(CONNECT_TIMEOUT, handshake)
        .await
        .map_err(|_| NetError::Timeout("starting TLS"))?
        .map_err(|error| {
            // rustls reports certificate problems as an io::Error wrapping its own error.
            match error
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<tokio_rustls::rustls::Error>())
            {
                Some(tls) => NetError::Tls(tls.to_string()),
                None => NetError::Connect(format!("connection to {host}:{port} failed: {error}")),
            }
        })
}

/// Delay before also trying the next address while earlier attempts are pending (RFC 8305 §5).
const ATTEMPT_DELAY: Duration = Duration::from_millis(250);

/// Connect to the first address that answers ("Happy Eyeballs", RFC 8305):
/// addresses alternate between IPv6 and IPv4, and a new attempt starts every
/// 250 ms until one succeeds, so a broken IPv6 route costs 250 ms, not seconds.
async fn connect_tcp(host: &str, port: u16) -> std::io::Result<TcpStream> {
    use futures_util::stream::{FuturesUnordered, StreamExt};

    let addresses = interleave(tokio::net::lookup_host((host, port)).await?.collect());
    let mut remaining = addresses.into_iter();
    let Some(first) = remaining.next() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the name has no addresses",
        ));
    };
    let mut attempts = FuturesUnordered::new();
    attempts.push(TcpStream::connect(first));
    loop {
        let more = remaining.len() > 0;
        tokio::select! {
            Some(result) = attempts.next() => match result {
                Ok(stream) => return Ok(stream),
                Err(error) => {
                    if attempts.is_empty() {
                        match remaining.next() {
                            Some(next) => attempts.push(TcpStream::connect(next)),
                            None => return Err(error),
                        }
                    }
                }
            },
            () = tokio::time::sleep(ATTEMPT_DELAY), if more => {
                if let Some(next) = remaining.next() {
                    attempts.push(TcpStream::connect(next));
                }
            }
        }
    }
}

/// Alternate address families, starting with the resolver's first choice.
fn interleave(addresses: Vec<std::net::SocketAddr>) -> Vec<std::net::SocketAddr> {
    let Some(first) = addresses.first() else {
        return addresses;
    };
    let first_is_v6 = first.is_ipv6();
    let (mut preferred, mut other): (Vec<_>, Vec<_>) = addresses
        .iter()
        .copied()
        .partition(|address| address.is_ipv6() == first_is_v6);
    let mut ordered = Vec::with_capacity(addresses.len());
    preferred.reverse();
    other.reverse();
    while let Some(address) = preferred.pop() {
        ordered.push(address);
        if let Some(address) = other.pop() {
            ordered.push(address);
        }
    }
    ordered.extend(other.into_iter().rev());
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    #[test]
    fn families_alternate_from_the_first_choice() {
        let parse = |list: &[&str]| {
            list.iter()
                .map(|a| a.parse::<SocketAddr>().unwrap())
                .collect::<Vec<_>>()
        };
        let input = parse(&["[::1]:993", "[::2]:993", "[::3]:993", "127.0.0.1:993", "127.0.0.2:993"]);
        let expected = parse(&["[::1]:993", "127.0.0.1:993", "[::2]:993", "127.0.0.2:993", "[::3]:993"]);
        assert_eq!(interleave(input), expected);
        let v4_first = parse(&["127.0.0.1:993", "[::1]:993", "[::2]:993"]);
        assert_eq!(
            interleave(v4_first.clone()),
            parse(&["127.0.0.1:993", "[::1]:993", "[::2]:993"])
        );
        assert!(interleave(vec![]).is_empty());
    }

    #[tokio::test]
    async fn localhost_connects_quickly_when_only_ipv4_listens() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let started = std::time::Instant::now();
        connect_tcp("localhost", port).await.unwrap();
        // Sequential connects take about 2 s on Windows when ::1 is tried first.
        assert!(
            started.elapsed() < Duration::from_millis(1500),
            "{:?}",
            started.elapsed()
        );
    }
}
