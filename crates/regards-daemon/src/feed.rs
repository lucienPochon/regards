//! The change feed (ADR-0007): one version that moves whenever anything a
//! client may show changes, either a committed database write or sync status.
//! Subscribers get the latest value; bursts collapse into one wake-up.

use regards_ipc::protocol::ChangesNotification;
use regards_store::Store;
use tokio::sync::watch;

pub(crate) struct Feed {
    current: watch::Sender<ChangesNotification>,
}

impl Feed {
    pub(crate) fn new(revision: i64) -> Self {
        Self {
            current: watch::Sender::new(ChangesNotification { version: 0, revision }),
        }
    }

    /// Something other than the database changed, such as sync status.
    pub(crate) fn bump(&self) {
        self.current.send_modify(|change| change.version += 1);
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<ChangesNotification> {
        self.current.subscribe()
    }

    /// Follow the store's revisions until it closes.
    pub(crate) async fn follow(&self, store: &Store) {
        let mut revisions = store.subscribe();
        while revisions.changed().await.is_ok() {
            let revision = *revisions.borrow_and_update();
            self.current.send_modify(|change| {
                change.version += 1;
                change.revision = revision;
            });
        }
    }
}
