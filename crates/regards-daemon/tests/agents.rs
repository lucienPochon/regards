//! Agents (ADR-0012): what each account lets them do, enforced in the daemon
//! whatever program they come through. Off by default; only the app opens
//! an account to them.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{
    AccountRef, AccountSummary, AgentAccess, AgentAccessParams, AgentActivityParams, AgentActivityResult,
    AgentDecision, ClientKind, MailboxListResult, MessageIds, MessageRef, RpcError, SearchParams, SearchResult, View,
    ViewParams, codes, methods,
};
use regards_ipc::{Client, ClientError};
use serde_json::Value;
use world::{Running, World};

fn refusal<T: std::fmt::Debug>(result: Result<T, ClientError>) -> RpcError {
    match result {
        Err(ClientError::Rpc(error)) => error,
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

async fn accounts(client: &mut Client) -> Vec<AccountSummary> {
    client.call(methods::ACCOUNT_LIST, Value::Null).await.unwrap()
}

async fn set(desktop: &mut Client, account_id: &str, access: AgentAccess) {
    let _: Value = desktop
        .call(
            methods::ACCOUNT_SET_AGENT_ACCESS,
            AgentAccessParams {
                account_id: account_id.into(),
                access,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn agents_see_and_read_only_what_an_account_allows() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    let mut agent = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();
    let mine = || AccountRef {
        account_id: account.id.clone(),
    };

    // Off: the account is not listed, and naming it is refused without naming it.
    assert_eq!(accounts(&mut desktop).await[0].agent_access, AgentAccess::Off);
    assert!(accounts(&mut agent).await.is_empty());
    let refused = refusal(agent.call::<_, MailboxListResult>(methods::MAILBOX_LIST, mine()).await);
    assert_eq!(refused.code, codes::FORBIDDEN);
    assert!(refused.message.contains("Settings › Agents"), "{}", refused.message);
    assert!(
        !refused.message.contains("ada@example.test"),
        "an account closed to agents is not named"
    );
    let data = refused.data.unwrap();
    assert_eq!(
        (data["setting"].as_str(), data["needed"].as_str()),
        (Some("agent_access"), Some("read"))
    );
    let search = |account_id: Option<&str>| SearchParams {
        query: "garden".into(),
        account_id: account_id.map(str::to_owned),
        ..SearchParams::default()
    };
    let found: SearchResult = agent.call(methods::SEARCH, search(None)).await.unwrap();
    assert!(found.messages.is_empty(), "nothing open to search");
    assert_eq!(
        refusal(
            agent
                .call::<_, SearchResult>(methods::SEARCH, search(Some(&account.id)))
                .await
        )
        .code,
        codes::FORBIDDEN
    );

    // Agents cannot open it themselves.
    let opened = agent
        .call::<_, Value>(
            methods::ACCOUNT_SET_AGENT_ACCESS,
            AgentAccessParams {
                account_id: account.id.clone(),
                access: AgentAccess::Send,
            },
        )
        .await;
    assert_eq!(refusal(opened).code, codes::FORBIDDEN);
    assert_eq!(accounts(&mut desktop).await[0].agent_access, AgentAccess::Off);

    // Read: listed, its folders, views and search; mail they name must exist.
    set(&mut desktop, &account.id, AgentAccess::Read).await;
    let listed = accounts(&mut agent).await;
    assert_eq!((listed.len(), listed[0].agent_access), (1, AgentAccess::Read));
    let _: MailboxListResult = agent.call(methods::MAILBOX_LIST, mine()).await.unwrap();
    let _: Value = agent
        .call(
            methods::MESSAGE_VIEW,
            ViewParams {
                account_id: account.id.clone(),
                view: View::Pinned,
                limit: None,
                before: None,
            },
        )
        .await
        .unwrap();
    let _: SearchResult = agent.call(methods::SEARCH, search(Some(&account.id))).await.unwrap();
    assert_eq!(
        refusal(
            agent
                .call::<_, Value>(methods::MESSAGE_GET, MessageRef { id: 999 })
                .await
        )
        .code,
        codes::NOT_FOUND
    );

    // Reading is all: organising, sending and settings stay with the app for now.
    let archived = agent
        .call::<_, Value>(methods::MESSAGE_ARCHIVE, MessageIds { ids: vec![1] })
        .await;
    assert_eq!(refusal(archived).code, codes::FORBIDDEN);
    assert_eq!(
        refusal(agent.call::<_, Value>(methods::ACCOUNT_SYNC, mine()).await).code,
        codes::FORBIDDEN
    );

    // Closed again: gone from the agent's view at once.
    set(&mut desktop, &account.id, AgentAccess::Off).await;
    assert!(accounts(&mut agent).await.is_empty());
    let unknown = agent
        .call::<_, Value>(
            methods::ACCOUNT_SET_AGENT_ACCESS,
            AgentAccessParams {
                account_id: "nobody".into(),
                access: AgentAccess::Read,
            },
        )
        .await;
    assert_eq!(refusal(unknown).code, codes::FORBIDDEN, "agents learn nothing from it");
    let unknown = desktop
        .call::<_, Value>(
            methods::ACCOUNT_SET_AGENT_ACCESS,
            AgentAccessParams {
                account_id: "nobody".into(),
                access: AgentAccess::Read,
            },
        )
        .await;
    assert_eq!(refusal(unknown).code, codes::NOT_FOUND);
    handle.abort();
}

#[tokio::test]
async fn what_agents_do_is_kept_for_the_app_to_see_without_their_secrets() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    set(&mut desktop, &account.id, AgentAccess::Read).await;
    let mut agent = Client::connect(&endpoint, ClientKind::Cli).await.unwrap();

    let _: MailboxListResult = agent
        .call(
            methods::MAILBOX_LIST,
            AccountRef {
                account_id: account.id.clone(),
            },
        )
        .await
        .unwrap();
    let archived = agent
        .call::<_, Value>(methods::MESSAGE_ARCHIVE, MessageIds { ids: vec![7] })
        .await;
    assert_eq!(refusal(archived).code, codes::FORBIDDEN);
    // Asking for what is not allowed, with a secret in it: refused, and the secret not kept.
    let added = agent
        .call::<_, Value>(
            methods::ACCOUNT_ADD,
            serde_json::json!({ "email": "eve@example.test", "password": "hunter2" }),
        )
        .await;
    assert_eq!(refusal(added).code, codes::FORBIDDEN);
    // What the app does is not an agent's.
    let _: Vec<AccountSummary> = desktop.call(methods::ACCOUNT_LIST, Value::Null).await.unwrap();

    let activity: AgentActivityResult = desktop
        .call(methods::AGENT_ACTIVITY, AgentActivityParams::default())
        .await
        .unwrap();
    let seen: Vec<(&str, AgentDecision)> = activity
        .entries
        .iter()
        .map(|entry| (entry.method.as_str(), entry.decision))
        .collect();
    assert_eq!(
        seen,
        [
            (methods::ACCOUNT_ADD, AgentDecision::Denied),
            (methods::MESSAGE_ARCHIVE, AgentDecision::Denied),
            (methods::MAILBOX_LIST, AgentDecision::Allowed),
        ],
        "newest first"
    );
    let listed = &activity.entries[2];
    assert_eq!(listed.account_id.as_deref(), Some(account.id.as_str()));
    assert_eq!(
        (listed.access, listed.client.as_str()),
        (Some(AgentAccess::Read), "cli")
    );
    assert!(listed.error.is_none());
    let added = &activity.entries[0];
    assert_eq!(added.args["password"], "[removed]");
    assert_eq!(added.args["email"], "eve@example.test");
    assert_eq!(added.error_code, Some(codes::FORBIDDEN));
    assert!(
        activity.entries.iter().all(|entry| entry.session == listed.session),
        "one connection"
    );

    // Pages, and one account's.
    let first: AgentActivityResult = desktop
        .call(
            methods::AGENT_ACTIVITY,
            AgentActivityParams {
                limit: Some(2),
                ..AgentActivityParams::default()
            },
        )
        .await
        .unwrap();
    let rest: AgentActivityResult = desktop
        .call(
            methods::AGENT_ACTIVITY,
            AgentActivityParams {
                before: first.next,
                ..AgentActivityParams::default()
            },
        )
        .await
        .unwrap();
    assert_eq!((first.entries.len(), rest.entries.len()), (2, 1));
    let mine: AgentActivityResult = desktop
        .call(
            methods::AGENT_ACTIVITY,
            AgentActivityParams {
                account_id: Some(account.id.clone()),
                ..AgentActivityParams::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(mine.entries.len(), 1);

    // Agents do not read it.
    let read = agent
        .call::<_, Value>(methods::AGENT_ACTIVITY, AgentActivityParams::default())
        .await;
    assert_eq!(refusal(read).code, codes::FORBIDDEN);
    handle.abort();
}
