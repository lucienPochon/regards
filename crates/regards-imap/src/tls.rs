//! TLS for IMAP connections (ADR-0009): rustls with the operating system's
//! certificate verifier. Validation is always on; there is no way to accept an
//! invalid certificate.

use crate::ImapError;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, crypto};

/// How long TCP connect plus the TLS handshake may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

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
    pub fn with_extra_roots_pem(mut self, pem: &[u8]) -> Result<Self, ImapError> {
        for cert in CertificateDer::pem_slice_iter(pem) {
            let cert = cert.map_err(|error| ImapError::Tls(format!("invalid extra root certificate: {error}")))?;
            self.extra_roots.push(cert);
        }
        Ok(self)
    }

    fn client_config(&self) -> Result<Arc<ClientConfig>, ImapError> {
        let provider = Arc::new(crypto::ring::default_provider());
        let verifier = if self.extra_roots.is_empty() {
            rustls_platform_verifier::Verifier::new(Arc::clone(&provider))
        } else {
            rustls_platform_verifier::Verifier::new_with_extra_roots(self.extra_roots.clone(), Arc::clone(&provider))
        }
        .map_err(|error| ImapError::Tls(format!("cannot load the system certificates: {error}")))?;
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|error| ImapError::Tls(error.to_string()))?
            .dangerous() // Only to install the platform verifier, which does full validation.
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        Ok(Arc::new(config))
    }
}

/// Open TCP to `host:port` and complete a TLS handshake that verifies `host`.
pub(crate) async fn connect(host: &str, port: u16, options: &TlsOptions) -> Result<TlsStream<TcpStream>, ImapError> {
    let name = ServerName::try_from(host.to_owned())
        .map_err(|_| ImapError::Connect(format!("{host:?} is not a valid server name")))?;
    let connector = TlsConnector::from(options.client_config()?);
    let handshake = async {
        let tcp = TcpStream::connect((host, port))
            .await
            .map_err(|error| ImapError::Connect(format!("cannot reach {host}:{port}: {error}")))?;
        tcp.set_nodelay(true).ok();
        connector.connect(name, tcp).await.map_err(|error| {
            // rustls reports certificate problems as an io::Error wrapping its own error.
            match error
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<tokio_rustls::rustls::Error>())
            {
                Some(tls) => ImapError::Tls(tls.to_string()),
                None => ImapError::Connect(format!("connection to {host}:{port} failed: {error}")),
            }
        })
    };
    tokio::time::timeout(CONNECT_TIMEOUT, handshake)
        .await
        .map_err(|_| ImapError::Timeout("connecting"))?
}
