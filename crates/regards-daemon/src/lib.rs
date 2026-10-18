//! The Regards daemon (ADR-0005).
//!
//! One daemon runs per data directory, normally one per OS user. It is the only
//! process that owns the user's mail data; the desktop app, the CLI and the MCP
//! server talk to it over the local API (ADR-0006).

mod session;

use regards_core::paths::DataDirs;
use regards_ipc::endpoint::Endpoint;
use regards_ipc::transport::Listener;
use std::fs::{File, TryLockError};
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Errors starting the daemon.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("another Regards daemon is already running for {0}")]
    AlreadyRunning(PathBuf),
    #[error("could not prepare the data directory {path}: {source}")]
    DataDir { path: PathBuf, source: io::Error },
    #[error("could not listen on {endpoint}: {source}")]
    Listen { endpoint: String, source: io::Error },
}

/// Shared state of a running daemon.
pub(crate) struct State {
    pub(crate) started: Instant,
    pub(crate) stop: watch::Sender<bool>,
}

/// A daemon that holds its data-directory lock and is listening, but not yet serving.
pub struct Daemon {
    dirs: DataDirs,
    endpoint: Endpoint,
    listener: Listener,
    lock: File,
}

impl Daemon {
    /// Take the data-directory lock and start listening.
    ///
    /// Fails with [`DaemonError::AlreadyRunning`] when another daemon owns `dirs`.
    pub async fn bind(dirs: DataDirs) -> Result<Self, DaemonError> {
        let data_error = |source| DaemonError::DataDir {
            path: dirs.data().to_path_buf(),
            source,
        };
        std::fs::create_dir_all(dirs.data()).map_err(data_error)?;

        let lock = File::options()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(dirs.lock_file())
            .map_err(data_error)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(DaemonError::AlreadyRunning(dirs.data().to_path_buf())),
            Err(TryLockError::Error(source)) => return Err(data_error(source)),
        }

        let endpoint = Endpoint::for_dirs(&dirs);
        let listener = Listener::bind(&endpoint).await.map_err(|source| DaemonError::Listen {
            endpoint: endpoint.to_string(),
            source,
        })?;
        Ok(Self {
            dirs,
            endpoint,
            listener,
            lock,
        })
    }

    /// Where clients can reach this daemon.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// The data directory this daemon owns.
    pub fn dirs(&self) -> &DataDirs {
        &self.dirs
    }

    /// Serve connections until `shutdown` completes or a client calls `daemon.shutdown`.
    pub async fn serve(self, shutdown: impl Future<Output = ()>) {
        let Self {
            mut listener,
            lock,
            endpoint,
            ..
        } = self;
        let (stop, mut stopped) = watch::channel(false);
        let state = Arc::new(State {
            started: Instant::now(),
            stop,
        });
        tracing::info!(%endpoint, "daemon listening");

        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                () = &mut shutdown => break,
                _ = stopped.changed() => break,
                accepted = listener.accept() => match accepted {
                    Ok(stream) => {
                        let state = Arc::clone(&state);
                        tokio::spawn(async move {
                            if let Err(error) = session::run(stream, state).await {
                                tracing::debug!(%error, "connection ended with an error");
                            }
                        });
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to accept a connection");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
            }
        }

        tracing::info!("daemon stopping");
        drop(listener);
        // Releasing the lock last lets a replacement daemon start only after we stopped listening.
        drop(lock);
    }
}
