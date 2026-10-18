//! End-to-end tests of the daemon over the real per-user transport.

use futures_util::SinkExt;
use regards_core::paths::DataDirs;
use regards_daemon::{Daemon, DaemonError};
use regards_ipc::protocol::{ClientKind, Principal, Request, ServerMessage, codes, methods};
use regards_ipc::{Client, Endpoint, framing, transport};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::task::JoinHandle;

async fn start(dirs: DataDirs) -> (Endpoint, JoinHandle<()>) {
    let daemon = Daemon::bind(dirs).await.expect("daemon binds");
    let endpoint = daemon.endpoint().clone();
    let handle = tokio::spawn(daemon.serve(std::future::pending()));
    (endpoint, handle)
}

async fn raw_request(endpoint: &Endpoint, request: Value) -> ServerMessage {
    let mut framed = framing::framed(transport::connect(endpoint).await.unwrap());
    framing::send(&mut framed, &request).await.unwrap();
    framing::recv(&mut framed).await.unwrap().unwrap()
}

fn error_code(message: ServerMessage) -> i32 {
    match message {
        ServerMessage::Response(response) => response.error.expect("an error response").code,
        ServerMessage::Notification(notification) => panic!("unexpected notification {notification:?}"),
    }
}

#[tokio::test]
async fn handshake_health_and_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let (endpoint, handle) = start(DataDirs::at(temp.path())).await;

    let mut client = Client::connect(&endpoint, ClientKind::Cli).await.unwrap();
    assert_eq!(client.hello().protocol_version, 1);
    assert_eq!(client.hello().principal, Principal::Agent);
    assert!(client.is_same_build());

    let health = client.health().await.unwrap();
    assert_eq!(health.status, "ok");
    assert_eq!(health.pid, std::process::id());

    client.shutdown().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("daemon stops after shutdown")
        .unwrap();

    // The lock is released, so a new daemon can start on the same data directory.
    let (_endpoint, again) = start(DataDirs::at(temp.path())).await;
    again.abort();
}

#[tokio::test]
async fn a_second_daemon_for_the_same_data_dir_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let _first = Daemon::bind(DataDirs::at(temp.path())).await.unwrap();
    let second = Daemon::bind(DataDirs::at(temp.path())).await;
    assert!(
        matches!(second, Err(DaemonError::AlreadyRunning(_))),
        "got {:?}",
        second.err()
    );
}

#[tokio::test]
async fn requests_before_hello_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let (endpoint, handle) = start(DataDirs::at(temp.path())).await;
    let request = serde_json::to_value(Request::new(1, methods::DAEMON_HEALTH, Value::Null)).unwrap();
    assert_eq!(
        error_code(raw_request(&endpoint, request).await),
        codes::HANDSHAKE_REQUIRED
    );
    handle.abort();
}

#[tokio::test]
async fn hello_without_a_common_protocol_version_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let (endpoint, handle) = start(DataDirs::at(temp.path())).await;
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "hello",
        "params": { "protocol_versions": [999], "build_id": "test", "client_kind": "cli" }
    });
    assert_eq!(
        error_code(raw_request(&endpoint, request).await),
        codes::UNSUPPORTED_PROTOCOL
    );
    handle.abort();
}

#[tokio::test]
async fn unknown_methods_and_malformed_json_get_errors() {
    let temp = tempfile::tempdir().unwrap();
    let (endpoint, handle) = start(DataDirs::at(temp.path())).await;

    let mut client = Client::connect(&endpoint, ClientKind::Cli).await.unwrap();
    let unknown = client
        .call::<_, Value>("no.such.method", Value::Null)
        .await
        .unwrap_err();
    assert!(matches!(unknown, regards_ipc::ClientError::Rpc(ref e) if e.code == codes::METHOD_NOT_FOUND));

    let mut framed = framing::framed(transport::connect(&endpoint).await.unwrap());
    framed
        .send(bytes::Bytes::from_static(b"this is not json"))
        .await
        .unwrap();
    let reply: ServerMessage = framing::recv(&mut framed).await.unwrap().unwrap();
    assert_eq!(error_code(reply), codes::PARSE_ERROR);

    handle.abort();
}
