//! Saved searches (Spark's Smart Folders) end to end: kept by name, in order,
//! of every account or one; checked; only the app keeps them.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{
    ClientKind, SavedSearchListResult, SavedSearchOrderParams, SavedSearchRef, SavedSearchSaveParams, SavedSearchView,
    codes, methods,
};
use regards_ipc::{Client, ClientError};
use serde_json::Value;
use world::{Running, World};

fn save(id: Option<i64>, name: &str, query: &str) -> SavedSearchSaveParams {
    SavedSearchSaveParams {
        id,
        name: name.into(),
        query: query.into(),
        account_id: None,
        color: None,
    }
}

fn code<T: std::fmt::Debug>(result: Result<T, ClientError>) -> i32 {
    match result {
        Err(ClientError::Rpc(error)) => error.code,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn searches_are_kept_by_name_in_order_and_checked() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;

    let bills: SavedSearchView = desktop
        .call(methods::SAVED_SEARCH_SAVE, save(None, " Bills ", "invoice last month "))
        .await
        .unwrap();
    assert_eq!(
        (bills.name.as_str(), bills.query.as_str()),
        ("Bills", "invoice last month")
    );
    let trips: SavedSearchView = desktop
        .call(
            methods::SAVED_SEARCH_SAVE,
            SavedSearchSaveParams {
                account_id: Some(account.id.clone()),
                color: Some("#1D7A61".into()),
                ..save(None, "Trips", "has:attachment flight")
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (trips.account_id.as_deref(), trips.color.as_deref()),
        (Some(account.id.as_str()), Some("#1d7a61"))
    );

    let names = |listed: SavedSearchListResult| listed.searches.into_iter().map(|s| s.name).collect::<Vec<_>>();
    let listed: SavedSearchListResult = desktop.call(methods::SAVED_SEARCH_LIST, Value::Null).await.unwrap();
    assert_eq!(names(listed), ["Bills", "Trips"]);
    let reordered: SavedSearchListResult = desktop
        .call(
            methods::SAVED_SEARCH_REORDER,
            SavedSearchOrderParams { ids: vec![trips.id] },
        )
        .await
        .unwrap();
    assert_eq!(names(reordered), ["Trips", "Bills"]);

    // Changed in place; nonsense refused.
    let renamed: SavedSearchView = desktop
        .call(methods::SAVED_SEARCH_SAVE, save(Some(bills.id), "Invoices", "invoice"))
        .await
        .unwrap();
    assert_eq!((renamed.id, renamed.name.as_str()), (bills.id, "Invoices"));
    for bad in [
        save(None, "", "invoice"),
        save(None, "Empty", "   "),
        SavedSearchSaveParams {
            color: Some("green".into()),
            ..save(None, "Coloured", "x")
        },
    ] {
        assert_eq!(
            code(desktop.call::<_, Value>(methods::SAVED_SEARCH_SAVE, bad).await),
            codes::INVALID_PARAMS
        );
    }
    let elsewhere = SavedSearchSaveParams {
        account_id: Some("no-such-account".into()),
        ..save(None, "Lost", "x")
    };
    assert_eq!(
        code(desktop.call::<_, Value>(methods::SAVED_SEARCH_SAVE, elsewhere).await),
        codes::NOT_FOUND
    );
    let missing = save(Some(9_999), "Gone", "x");
    assert_eq!(
        code(desktop.call::<_, Value>(methods::SAVED_SEARCH_SAVE, missing).await),
        codes::NOT_FOUND
    );

    let _: Value = desktop
        .call(methods::SAVED_SEARCH_REMOVE, SavedSearchRef { id: trips.id })
        .await
        .unwrap();
    let listed: SavedSearchListResult = desktop.call(methods::SAVED_SEARCH_LIST, Value::Null).await.unwrap();
    assert_eq!(names(listed), ["Invoices"]);

    // Agents never keep them.
    let mut agent = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();
    let refused = agent.call::<_, Value>(methods::SAVED_SEARCH_LIST, Value::Null).await;
    assert_eq!(code(refused), codes::FORBIDDEN);
    handle.abort();
}
