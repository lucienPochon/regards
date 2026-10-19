//! A daemon on a scratch data directory, its account served by fake IMAP and
//! SMTP servers: for tests that run everywhere, without a mail server. The
//! test crate declares `fake_imap` and `fake_smtp` at its root.

// Each test crate uses a part of it.
#![allow(dead_code)]

use regards_core::paths::DataDirs;
use regards_core::secret::Secret;
use regards_daemon::Daemon;
use regards_imap::TlsOptions;
use regards_ipc::protocol::{
    AccountAddParams, AccountSummary, ClientKind, ConnectionSecurity, ServerSettings, methods,
};
use regards_ipc::{Client, Endpoint, UiToken};
use regards_vault::Vault;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

/// One data directory and its fake servers; [`World::start`] starts (or
/// restarts) a daemon on it.
pub struct World {
    pub temp: tempfile::TempDir,
    pub vault: Vault,
    pub token: UiToken,
    pub tls: TlsOptions,
    pub grace: Duration,
    pub imap_port: u16,
    pub smtp_port: u16,
    pub sent: crate::fake_smtp::Sessions,
}

pub struct Running {
    pub endpoint: Endpoint,
    pub desktop: Client,
    pub handle: JoinHandle<()>,
}

impl World {
    pub async fn new(grace: Duration) -> Self {
        let imap_pki = crate::fake_imap::pki(&["localhost"]);
        let (imap_port, _) =
            crate::fake_imap::fake_server(&imap_pki, "* OK ready\r\n", crate::fake_imap::PASSWORD).await;
        let smtp_pki = crate::fake_smtp::pki();
        let (smtp_port, sent) = crate::fake_smtp::serve(
            &smtp_pki,
            crate::fake_smtp::Script {
                password: crate::fake_imap::PASSWORD,
                ..crate::fake_smtp::Script::new(crate::fake_smtp::Mode::StartTls {
                    offer: true,
                    inject: false,
                })
            },
        )
        .await;
        let tls = TlsOptions::platform()
            .with_extra_roots_pem(imap_pki.ca_pem.as_bytes())
            .unwrap()
            .with_extra_roots_pem(smtp_pki.ca_pem.as_bytes())
            .unwrap();
        Self {
            temp: tempfile::tempdir().unwrap(),
            vault: Vault::in_memory(),
            token: UiToken::generate().unwrap(),
            tls,
            grace,
            imap_port,
            smtp_port,
            sent,
        }
    }

    pub async fn start(&self) -> Running {
        let daemon = Daemon::bind(DataDirs::at(self.temp.path()))
            .await
            .unwrap()
            .with_ui_token(self.token.clone())
            .with_vault(self.vault.clone())
            .with_tls(self.tls.clone())
            .with_late_send_grace(self.grace);
        let endpoint = daemon.endpoint().clone();
        let handle = tokio::spawn(daemon.serve(std::future::pending()));
        let desktop = Client::connect_with(&endpoint, ClientKind::Desktop, Some(&self.token))
            .await
            .unwrap();
        Running {
            endpoint,
            desktop,
            handle,
        }
    }

    pub async fn add_account(&self, desktop: &mut Client) -> AccountSummary {
        desktop
            .call(
                methods::ACCOUNT_ADD,
                AccountAddParams {
                    display_name: "Ada".into(),
                    email: "ada@example.test".into(),
                    username: None,
                    password: Secret::new(crate::fake_imap::PASSWORD),
                    imap: ServerSettings {
                        host: "localhost".into(),
                        port: self.imap_port,
                        security: ConnectionSecurity::Tls,
                    },
                    smtp: ServerSettings {
                        host: "127.0.0.1".into(),
                        port: self.smtp_port,
                        security: ConnectionSecurity::Starttls,
                    },
                },
            )
            .await
            .unwrap()
    }

    /// The next message the SMTP server received.
    pub async fn next_message(&mut self) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let session = tokio::time::timeout(left, self.sent.next())
                .await
                .expect("nothing was sent");
            if let Some(data) = session.data {
                return data;
            }
        }
    }

    /// Nothing reached the SMTP server so far.
    pub fn nothing_sent(&mut self) -> bool {
        std::iter::from_fn(|| self.sent.try_next()).all(|session| session.data.is_none())
    }
}
