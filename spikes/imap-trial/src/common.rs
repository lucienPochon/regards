//! Shared pieces: configuration, TLS, test messages and the results table.

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use tokio::net::TcpStream;
use tokio_rustls::{
    TlsConnector,
    client::TlsStream,
    rustls::{
        ClientConfig, RootCertStore,
        pki_types::{CertificateDer, ServerName, pem::PemObject},
    },
};

pub struct Config {
    pub addr: String,
    pub ca: PathBuf,
    pub pass: String,
    run_id: u64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            addr: std::env::var("TRIAL_ADDR").unwrap_or_else(|_| "127.0.0.1:31993".into()),
            ca: std::env::var("TRIAL_CA")
                .context("TRIAL_CA must point to the test CA (PEM)")?
                .into(),
            pass: std::env::var("TRIAL_PASS").unwrap_or_else(|_| "pass".into()),
            run_id: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        })
    }

    /// A fresh mailbox per library and run, so every run starts empty.
    pub fn user(&self, lib: &str) -> String {
        format!("{lib}-{}", self.run_id)
    }
}

pub async fn connect_tls(cfg: &Config) -> Result<TlsStream<TcpStream>> {
    let mut roots = RootCertStore::empty();
    for cert in CertificateDer::pem_file_iter(&cfg.ca).context("reading TRIAL_CA")? {
        roots.add(cert?)?;
    }
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tcp = TcpStream::connect(&cfg.addr).await.context("TCP connect")?;
    tcp.set_nodelay(true)?;
    let name = ServerName::try_from("localhost")?.to_owned();
    Ok(TlsConnector::from(Arc::new(config)).connect(name, tcp).await?)
}

pub fn tls_version(tls: &TlsStream<TcpStream>) -> String {
    tls.get_ref()
        .1
        .protocol_version()
        .map(|v| format!("{v:?}"))
        .unwrap_or_else(|| "?".into())
}

pub fn message(i: usize) -> Vec<u8> {
    format!(
        "From: Trial <trial@example.test>\r\n\
         To: Me <me@example.test>\r\n\
         Subject: Trial message {i}\r\n\
         Message-ID: <trial-{i}@example.test>\r\n\
         Date: Sun, 18 Oct 2026 10:00:00 +0000\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         Hello {i}\r\n"
    )
    .into_bytes()
}

pub const WANTED_CAPS: &[&str] = &[
    "IMAP4rev1",
    "CONDSTORE",
    "QRESYNC",
    "MOVE",
    "UIDPLUS",
    "IDLE",
    "SPECIAL-USE",
    "ENABLE",
    "LITERAL+",
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail,
    /// The server did it, but the library gives the caller no way to see the result.
    Gap,
}

pub struct Row {
    pub step: String,
    pub outcome: Outcome,
    pub note: String,
    pub elapsed: Duration,
}

pub struct Report {
    pub lib: &'static str,
    pub rows: Vec<Row>,
}

impl Report {
    pub fn new(lib: &'static str) -> Self {
        Self { lib, rows: Vec::new() }
    }

    pub fn add(&mut self, step: &str, started: Instant, outcome: Outcome, note: impl Into<String>) {
        self.rows.push(Row {
            step: step.into(),
            outcome,
            note: note.into(),
            elapsed: started.elapsed(),
        });
    }

    pub fn pass(&mut self, step: &str, started: Instant, note: impl Into<String>) {
        self.add(step, started, Outcome::Pass, note);
    }

    pub fn check(&mut self, step: &str, started: Instant, ok: bool, note: impl Into<String>) {
        self.add(step, started, if ok { Outcome::Pass } else { Outcome::Fail }, note);
    }

    pub fn gap(&mut self, step: &str, note: impl Into<String>) {
        self.add(step, Instant::now(), Outcome::Gap, note);
    }

    pub fn print(&self) {
        println!("\n## {}\n", self.lib);
        println!("| Step | Result | ms | Note |");
        println!("| --- | --- | ---: | --- |");
        for row in &self.rows {
            let result = match row.outcome {
                Outcome::Pass => "pass",
                Outcome::Fail => "**FAIL**",
                Outcome::Gap => "gap",
            };
            println!(
                "| {} | {} | {:.1} | {} |",
                row.step,
                result,
                row.elapsed.as_secs_f64() * 1000.0,
                row.note.replace('|', "\\|")
            );
        }
    }
}
