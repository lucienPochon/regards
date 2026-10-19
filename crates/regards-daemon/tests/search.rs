//! Search end to end (plan §5.10), what runs without a mail server: the
//! method's checks. What is found is tested in the store, and against a real
//! server in `live.rs`.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{ClientKind, SearchParams, SearchResult, codes, methods};
use regards_ipc::{Client, ClientError};
use world::{Running, World};

fn code<T: std::fmt::Debug>(result: Result<T, ClientError>) -> i32 {
    match result {
        Err(ClientError::Rpc(error)) => error.code,
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

fn search(query: &str) -> SearchParams {
    SearchParams {
        query: query.into(),
        ..SearchParams::default()
    }
}

#[tokio::test]
async fn search_answers_the_app_and_checks_what_it_is_asked() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    world.add_account(&mut desktop).await;

    let found: SearchResult = desktop
        .call(methods::SEARCH, search("garden from:lena is:unread"))
        .await
        .unwrap();
    assert!(found.messages.is_empty() && found.top.is_empty() && found.next.is_none());
    let nothing: SearchResult = desktop.call(methods::SEARCH, search("   ")).await.unwrap();
    assert!(nothing.messages.is_empty(), "nothing asked, nothing found");

    let too_many = SearchParams {
        limit: Some(501),
        ..search("garden")
    };
    assert_eq!(
        code(desktop.call::<_, SearchResult>(methods::SEARCH, too_many).await),
        codes::INVALID_PARAMS
    );
    let rambling = search(&"word ".repeat(300));
    assert_eq!(
        code(desktop.call::<_, SearchResult>(methods::SEARCH, rambling).await),
        codes::INVALID_PARAMS
    );

    // Agents get access levels later (ADR-0012); until then, not mail.
    let mut agent = Client::connect(&endpoint, ClientKind::Cli).await.unwrap();
    assert_eq!(
        code(agent.call::<_, SearchResult>(methods::SEARCH, search("garden")).await),
        codes::FORBIDDEN
    );
    handle.abort();
}
