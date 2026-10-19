//! The Regards daemon (ADR-0005).
//!
//! One daemon runs per data directory, normally one per OS user. It is the only
//! process that owns the user's mail data; the desktop app, the CLI and the MCP
//! server talk to it over the local API (ADR-0006).

mod accounts;
mod actions;
mod backfill;
mod compose;
mod details;
mod feed;
mod identities;
mod mail;
mod notices;
mod push;
mod queue;
mod scheduler;
mod sender;
mod session;
mod smart;
mod sync;

use regards_core::paths::DataDirs;
use regards_imap::TlsOptions;
use regards_ipc::UiToken;
use regards_ipc::endpoint::Endpoint;
use regards_ipc::transport::Listener;
use regards_store::blobs::Blobs;
use regards_store::{Store, StoreError};
use regards_vault::Vault;
use std::fs::{File, TryLockError};
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{OnceCell, watch};

/// Errors starting the daemon.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("another Regards daemon is already running for {0}")]
    AlreadyRunning(PathBuf),
    #[error("could not prepare the data directory {path}: {source}")]
    DataDir { path: PathBuf, source: io::Error },
    #[error("could not listen on {endpoint}: {source}")]
    Listen { endpoint: String, source: io::Error },
    #[error("could not open the mail database: {0}")]
    Store(#[from] StoreError),
}

/// Shared state of a running daemon.
pub(crate) struct State {
    pub(crate) started: Instant,
    pub(crate) stop: watch::Sender<bool>,
    /// The desktop bridge's per-launch secret, when one was handed off.
    pub(crate) ui_token: Option<UiToken>,
    /// The mail database.
    pub(crate) store: Store,
    /// The credential store, opened on first use so a missing Secret Service
    /// on Linux does not stop the daemon from starting.
    pub(crate) vault: OnceCell<Result<Vault, String>>,
    /// Which certificates IMAP and SMTP connections trust.
    pub(crate) tls: TlsOptions,
    /// Sync progress per account.
    pub(crate) sync: sync::Tracker,
    /// The IDLE connections, one per account.
    pub(crate) watchers: push::Watchers,
    /// Change notifications for subscribed clients (ADR-0007).
    pub(crate) feed: Arc<feed::Feed>,
    /// Downloaded messages (ADR-0008).
    pub(crate) blobs: Blobs,
    /// Runners sending queued actions to the servers (ADR-0010).
    pub(crate) queue: queue::Queue,
    /// Tasks fetching older mail, one per account.
    pub(crate) backfills: backfill::Backfills,
    /// Sends what is queued in the outbox (ADR-0010 §3).
    pub(crate) outbox: sender::Outbox,
    /// Brings snoozed messages back when their time comes (ADR-0011).
    pub(crate) scheduler: scheduler::Scheduler,
    pub(crate) limits: SyncLimits,
    /// A scheduled send found later than this is held for the person.
    pub(crate) late_send_grace: Duration,
}

/// How much mail is fetched at a time (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncLimits {
    /// The first sync of a folder fetches this many of its newest messages.
    pub initial_window: u32,
    /// Older messages are then fetched in the background, this many at a time.
    pub backfill_batch: u32,
}

impl Default for SyncLimits {
    fn default() -> Self {
        Self {
            initial_window: 2000,
            backfill_batch: 500,
        }
    }
}

/// A daemon that holds its data-directory lock and is listening, but not yet serving.
pub struct Daemon {
    dirs: DataDirs,
    endpoint: Endpoint,
    listener: Listener,
    lock: File,
    store: Store,
    ui_token: Option<UiToken>,
    vault: Option<Vault>,
    tls: TlsOptions,
    limits: SyncLimits,
    late_send_grace: Duration,
}

/// A scheduled send found more than this late, because the computer was off
/// or asleep at its time, waits for the person instead of going out unasked:
/// a "Monday 9:00" message may no longer fit at 15:00 (ADR-0011 §2).
pub const DEFAULT_LATE_SEND_GRACE: Duration = Duration::from_secs(60 * 60);

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
        restrict_to_owner(dirs.data()).map_err(data_error)?;

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

        // Only the lock holder opens the database, so two daemons never migrate it at once.
        let store = Store::open(&dirs).await?;

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
            store,
            ui_token: None,
            vault: None,
            tls: default_tls(),
            limits: SyncLimits::default(),
            late_send_grace: DEFAULT_LATE_SEND_GRACE,
        })
    }

    /// Grant the `ui` principal to connections that present `token` (ADR-0006).
    /// Without a token, every connection is an `agent`.
    pub fn with_ui_token(mut self, token: UiToken) -> Self {
        self.ui_token = Some(token);
        self
    }

    /// Use this credential store instead of the operating system's (tests).
    pub fn with_vault(mut self, vault: Vault) -> Self {
        self.vault = Some(vault);
        self
    }

    /// Trust these certificates for IMAP and SMTP instead of the defaults (tests).
    pub fn with_tls(mut self, tls: TlsOptions) -> Self {
        self.tls = tls;
        self
    }

    /// Fetch mail in other amounts (tests).
    pub fn with_sync_limits(mut self, limits: SyncLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Hold scheduled sends found later than this, instead of after
    /// [`DEFAULT_LATE_SEND_GRACE`].
    pub fn with_late_send_grace(mut self, grace: Duration) -> Self {
        self.late_send_grace = grace;
        self
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
            ui_token,
            store,
            vault,
            tls,
            dirs,
            limits,
            late_send_grace,
        } = self;
        let (stop, mut stopped) = watch::channel(false);
        let feed = Arc::new(feed::Feed::new(*store.subscribe().borrow()));
        let state = Arc::new(State {
            started: Instant::now(),
            stop,
            ui_token,
            store,
            vault: OnceCell::new_with(vault.map(Ok)),
            tls,
            sync: sync::Tracker::new(Arc::clone(&feed)),
            watchers: push::Watchers::default(),
            feed,
            blobs: Blobs::new(dirs.blobs()),
            queue: queue::Queue::default(),
            backfills: backfill::Backfills::default(),
            outbox: sender::Outbox::default(),
            scheduler: scheduler::Scheduler::default(),
            limits,
            late_send_grace,
        });
        state.queue.attach(&state);
        queue::Queue::wake_all(&state);
        let follow = tokio::spawn({
            let state = Arc::clone(&state);
            async move { state.feed.follow(&state.store).await }
        });
        sync::spawn_all(Arc::clone(&state));
        let threading = tokio::spawn(mail::thread_backfill(Arc::clone(&state)));
        let sorting = tokio::spawn(smart::sort_stored(Arc::clone(&state)));
        let sending = tokio::spawn(sender::run(Arc::clone(&state)));
        let scheduling = tokio::spawn(scheduler::run(Arc::clone(&state)));
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
        state.watchers.stop_all();
        state.backfills.stop_all();
        follow.abort();
        threading.abort();
        sorting.abort();
        sending.abort();
        scheduling.abort();
        drop(listener);
        // Releasing the lock last lets a replacement daemon start only after we stopped listening.
        drop(lock);
    }
}

/// Make the data directory private to its owner (`0700`) on Unix. On Windows,
/// the per-user application-data directory is already restricted to the user.
#[cfg(unix)]
fn restrict_to_owner(dir: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_to_owner(_dir: &std::path::Path) -> io::Result<()> {
    Ok(())
}

/// Debug builds only: also trust the CA certificates in this PEM file for IMAP,
/// to develop against a local test server. Release builds ignore it.
#[cfg(debug_assertions)]
const DEV_EXTRA_CA_ENV: &str = "REGARDS_DEV_EXTRA_CA";

fn default_tls() -> TlsOptions {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os(DEV_EXTRA_CA_ENV) {
        match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|pem| {
            TlsOptions::platform()
                .with_extra_roots_pem(&pem)
                .map_err(|e| e.to_string())
        }) {
            Ok(tls) => {
                tracing::warn!(path = %std::path::Path::new(&path).display(), "development build: trusting an extra IMAP CA");
                return tls;
            }
            Err(error) => tracing::error!(%error, "ignoring {DEV_EXTRA_CA_ENV}"),
        }
    }
    TlsOptions::platform()
}
