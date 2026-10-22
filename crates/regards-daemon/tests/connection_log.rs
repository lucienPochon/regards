//! The connection log (Settings › Support): off by default, on at once, and
//! never a password. A test binary of its own: the log is one per process.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{ClientKind, ConnectionLogSettings, methods};
use regards_ipc::{Client, ClientError};
use serde_json::Value;
use world::{Running, World};

#[tokio::test]
async fn connections_are_logged_once_turned_on_and_never_a_password() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let off: ConnectionLogSettings = desktop.call(methods::CONNECTION_LOG_GET, Value::Null).await.unwrap();
    assert!(!off.on, "off by default");
    let mut agent = Client::connect(&endpoint, ClientKind::Cli).await.unwrap();
    assert!(matches!(
        agent
            .call::<_, ConnectionLogSettings>(methods::CONNECTION_LOG_SET, ConnectionLogSettings { on: true })
            .await,
        Err(ClientError::Rpc(_))
    ));

    let on: ConnectionLogSettings = desktop
        .call(methods::CONNECTION_LOG_SET, ConnectionLogSettings { on: true })
        .await
        .unwrap();
    assert!(on.on);
    // Adding an account signs in to check it: that conversation is logged.
    world.add_account(&mut desktop).await;
    let _: ConnectionLogSettings = desktop
        .call(methods::CONNECTION_LOG_SET, ConnectionLogSettings { on: false })
        .await
        .unwrap();

    let log = std::fs::read_to_string(world.temp.path().join("logs").join("connections.log")).unwrap();
    assert!(log.contains("[imap#") && log.contains(" LOGIN [redacted]"), "{log}");
    assert!(!log.contains(fake_imap::PASSWORD), "{log}");
    assert!(!log.contains("ada@example.test"), "{log}");
    handle.abort();
}
