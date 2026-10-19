//! Daemon and CLI logging to stderr, filtered by `REGARDS_LOG`.
//!
//! Logs never contain secrets or mail (ADR-0015). imap-next logs the raw
//! protocol bytes of every command and response at TRACE level, passwords
//! included, so its target is capped at INFO whatever `REGARDS_LOG` asks for.

use tracing_subscriber::Layer;
use tracing_subscriber::filter::{EnvFilter, FilterExt, LevelFilter, Targets};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;

/// Environment variable with the log filter, for example `debug` or `regards_daemon=trace`.
pub const LOG_ENV: &str = "REGARDS_LOG";

/// Targets that log protocol data, and the most they may log.
const CAPPED: &[(&str, LevelFilter)] = &[("imap_next", LevelFilter::INFO)];

pub fn init() {
    let requested = EnvFilter::try_from_env(LOG_ENV).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(layer(requested, std::io::stderr))
        .init();
}

fn layer<S, W>(requested: EnvFilter, writer: W) -> impl Layer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    let privacy = CAPPED.iter().fold(
        Targets::new().with_default(LevelFilter::TRACE),
        |targets, (target, level)| targets.with_target(*target, *level),
    );
    // Both filters must pass, so REGARDS_LOG cannot lift the cap.
    tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_filter(requested.and(privacy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn protocol_traces_never_reach_the_log_even_at_trace_level() {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::registry().with(layer(EnvFilter::new("trace"), move || writer.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::trace!(target: "imap_next::fragment", data = "a1 LOGIN ada hunter2", "write");
            tracing::info!(target: "imap_next", "imap-next info is kept");
            tracing::trace!(target: "regards_daemon", "our own trace is kept");
        });
        let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        assert!(!log.contains("hunter2"), "a password reached the log:\n{log}");
        assert!(log.contains("imap-next info is kept"));
        assert!(log.contains("our own trace is kept"));
    }
}
