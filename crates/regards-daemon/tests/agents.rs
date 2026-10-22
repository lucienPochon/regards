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
    AgentDecision, ClientKind, DraftCreateParams, DraftListParams, DraftListResult, DraftRef, DraftSendParams,
    DraftUpdateParams, DraftView, MailboxListResult, MessageIds, MessageRef, Policy, RpcError, SearchParams,
    SearchResult, SenderSetParams, SenderSetResult, UndoParams, View, ViewParams, codes, methods,
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

    // Reading is all: writing needs triage, and syncing stays with the app.
    let drafted = agent
        .call::<_, Value>(
            methods::DRAFT_CREATE,
            DraftCreateParams {
                account_id: account.id.clone(),
                ..DraftCreateParams::default()
            },
        )
        .await;
    assert_eq!(refusal(drafted).code, codes::FORBIDDEN);
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
    let drafted = agent
        .call::<_, Value>(
            methods::DRAFT_CREATE,
            DraftCreateParams {
                account_id: account.id.clone(),
                ..DraftCreateParams::default()
            },
        )
        .await;
    assert_eq!(refusal(drafted).code, codes::FORBIDDEN);
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
            (methods::DRAFT_CREATE, AgentDecision::Denied),
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
    assert_eq!(mine.entries.len(), 2, "the folders listed, the draft refused");

    // Agents do not read it.
    let read = agent
        .call::<_, Value>(methods::AGENT_ACTIVITY, AgentActivityParams::default())
        .await;
    assert_eq!(refusal(read).code, codes::FORBIDDEN);
    handle.abort();
}

#[tokio::test]
async fn at_triage_agents_write_drafts_and_sort_senders() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    set(&mut desktop, &account.id, AgentAccess::Read).await;
    let mut agent = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();
    let new_draft = || DraftCreateParams {
        account_id: account.id.clone(),
        subject: "Plans".into(),
        body: "First thoughts".into(),
        ..DraftCreateParams::default()
    };

    // Read is not enough to write: the refusal says what is needed.
    let refused = refusal(agent.call::<_, DraftView>(methods::DRAFT_CREATE, new_draft()).await);
    assert_eq!(refused.code, codes::FORBIDDEN);
    assert!(
        refused.message.contains("ada@example.test allows agents only to read"),
        "{}",
        refused.message
    );
    assert_eq!(refused.data.unwrap()["needed"], "triage");

    set(&mut desktop, &account.id, AgentAccess::Triage).await;
    let draft: DraftView = agent.call(methods::DRAFT_CREATE, new_draft()).await.unwrap();
    let _: DraftView = agent
        .call(
            methods::DRAFT_UPDATE,
            DraftUpdateParams {
                id: draft.id,
                to: vec![],
                cc: vec![],
                bcc: vec![],
                subject: "Better plans".into(),
                body: "Second thoughts".into(),
                html: None,
            },
        )
        .await
        .unwrap();
    let listed: DraftListResult = agent
        .call(methods::DRAFT_LIST, DraftListParams { account_id: None })
        .await
        .unwrap();
    assert_eq!(listed.drafts.len(), 1);
    assert_eq!(listed.drafts[0].subject, "Better plans");

    // Senders sorted (blocking, which moves mail to Junk, and undo are in live.rs).
    let _: SenderSetResult = agent
        .call(
            methods::SENDER_SET,
            SenderSetParams {
                account_id: account.id.clone(),
                address: "news@shop.example".into(),
                policy: Some(Policy::Newsletters),
                priority: false,
            },
        )
        .await
        .unwrap();

    // Still not: unknown mail, too much at once, and sending.
    let unknown = refusal(
        agent
            .call::<_, Value>(methods::MESSAGE_ARCHIVE, MessageIds { ids: vec![999] })
            .await,
    );
    assert_eq!(
        (unknown.code, unknown.message.as_str()),
        (codes::NOT_FOUND, "no such message: 999")
    );
    let bulk = refusal(
        agent
            .call::<_, Value>(
                methods::MESSAGE_ARCHIVE,
                MessageIds {
                    ids: (1..=51).collect(),
                },
            )
            .await,
    );
    // Many at once waits for the person (tests/holds.rs, live.rs), once the messages are known.
    assert_eq!(bulk.code, codes::NOT_FOUND);
    let sent = agent
        .call::<_, Value>(
            methods::DRAFT_SEND,
            DraftSendParams {
                id: draft.id,
                undo_seconds: None,
                send_at: None,
                remind_at: None,
            },
        )
        .await;
    assert_eq!(refusal(sent).code, codes::FORBIDDEN);

    // What it changed is kept with how it stood.
    let activity: AgentActivityResult = desktop
        .call(methods::AGENT_ACTIVITY, AgentActivityParams::default())
        .await
        .unwrap();
    let entry = |method: &str| activity.entries.iter().find(|e| e.method == method).unwrap();
    assert_eq!(
        entry(methods::DRAFT_UPDATE).before.as_ref().unwrap()["subject"],
        "Plans",
        "the draft before"
    );
    assert_eq!(
        entry(methods::SENDER_SET).before.as_ref().unwrap()["policy"],
        Value::Null
    );
    assert!(
        entry(methods::MESSAGE_ARCHIVE).before.is_none(),
        "a refusal changed nothing"
    );

    // Agents do not undo: that stays with the person.
    let undone = agent.call::<_, Value>(methods::UNDO_APPLY, UndoParams { id: 1 }).await;
    assert_eq!(refusal(undone).code, codes::FORBIDDEN);

    // In an account closed to agents, its drafts are not there for them.
    set(&mut desktop, &account.id, AgentAccess::Off).await;
    let hidden = refusal(
        agent
            .call::<_, DraftView>(methods::DRAFT_GET, DraftRef { id: draft.id })
            .await,
    );
    assert_eq!(
        (hidden.code, hidden.message.as_str()),
        (codes::NOT_FOUND, "no such draft")
    );
    set(&mut desktop, &account.id, AgentAccess::Triage).await;

    let _: Value = agent
        .call(methods::DRAFT_DELETE, DraftRef { id: draft.id })
        .await
        .unwrap();
    handle.abort();
}

#[tokio::test]
async fn agents_are_kept_to_a_pace_and_to_a_few_requests_waiting() {
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    let _: Value = desktop
        .call(
            methods::ACCOUNT_SET_AGENT_ACCESS,
            AgentAccessParams {
                account_id: account.id.clone(),
                access: AgentAccess::Send,
            },
        )
        .await
        .unwrap();

    // Changes: a burst of 30, then refused with when to try again.
    let mut agent = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();
    for n in 0..30 {
        let _: DraftView = agent
            .call(methods::DRAFT_CREATE, new_draft(&account.id, n))
            .await
            .unwrap();
    }
    let refused = refusal(
        agent
            .call::<_, DraftView>(methods::DRAFT_CREATE, new_draft(&account.id, 30))
            .await,
    );
    assert_eq!(refused.code, codes::RATE_LIMITED);
    assert!(refused.data.unwrap()["retry_after_ms"].as_u64().unwrap() <= 1_000);
    // Reading goes on, and the app is never limited.
    let _: DraftListResult = agent
        .call(methods::DRAFT_LIST, DraftListParams { account_id: None })
        .await
        .unwrap();
    for n in 0..40 {
        let _: DraftView = desktop
            .call(methods::DRAFT_CREATE, new_draft(&account.id, n))
            .await
            .unwrap();
    }

    // At most 20 requests wait for the person at once.
    let drafts: DraftListResult = desktop
        .call(methods::DRAFT_LIST, DraftListParams { account_id: None })
        .await
        .unwrap();
    let mut sender = Client::connect(&endpoint, ClientKind::Cli).await.unwrap();
    let send = |id| DraftSendParams {
        id,
        undo_seconds: Some(0),
        send_at: None,
        remind_at: None,
    };
    for draft in &drafts.drafts[..20] {
        let held = refusal(sender.call::<_, Value>(methods::DRAFT_SEND, send(draft.id)).await);
        assert_eq!(held.code, codes::HELD);
    }
    let crowded = refusal(
        sender
            .call::<_, Value>(methods::DRAFT_SEND, send(drafts.drafts[20].id))
            .await,
    );
    assert_eq!(crowded.code, codes::RATE_LIMITED);
    assert!(
        crowded.message.contains("20 requests already wait"),
        "{}",
        crowded.message
    );
    handle.abort();
}

fn new_draft(account_id: &str, n: u32) -> DraftCreateParams {
    DraftCreateParams {
        account_id: account_id.into(),
        to: vec![regards_core::headers::Person {
            name: None,
            address: Some("bob@example.test".into()),
        }],
        subject: format!("Note {n}"),
        ..DraftCreateParams::default()
    }
}

#[tokio::test]
async fn every_agent_can_be_turned_off_at_once_and_sent_away() {
    use regards_ipc::protocol::{AgentsDisconnected, AgentsEnabledParams, AgentsSettings};
    let world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    set(&mut desktop, &account.id, AgentAccess::Read).await;
    // The app it runs in is listed while it is connected, each name once.
    let mut agent = Client::connect_as(&endpoint, ClientKind::Mcp, None, Some(" claude-code "))
        .await
        .unwrap();
    assert_eq!(accounts(&mut agent).await.len(), 1);
    let on: AgentsSettings = desktop.call(methods::AGENTS_SETTINGS, Value::Null).await.unwrap();
    assert_eq!(
        on,
        AgentsSettings {
            enabled: true,
            connected: 1,
            apps: vec!["claude-code".into()],
        }
    );

    // Only the app may turn them off.
    let refused = refusal(
        agent
            .call::<_, AgentsSettings>(methods::AGENTS_SET_ENABLED, AgentsEnabledParams { enabled: false })
            .await,
    );
    assert_eq!(refused.code, codes::FORBIDDEN);

    // Off: the connected agent is sent away, and a new one gets nothing but hello.
    let off: AgentsSettings = desktop
        .call(methods::AGENTS_SET_ENABLED, AgentsEnabledParams { enabled: false })
        .await
        .unwrap();
    assert!(!off.enabled);
    assert!(
        matches!(
            agent.call::<_, Value>(methods::ACCOUNT_LIST, Value::Null).await,
            Err(ClientError::Closed | ClientError::Io(_) | ClientError::Frame(_))
        ),
        "its connection was ended"
    );
    let mut again = Client::connect(&endpoint, ClientKind::Cli).await.unwrap();
    let refused = refusal(again.call::<_, Value>(methods::ACCOUNT_LIST, Value::Null).await);
    assert_eq!(refused.code, codes::FORBIDDEN);
    assert!(refused.message.contains("turned off"), "{}", refused.message);

    // On again: the account's own level was kept.
    let _: AgentsSettings = desktop
        .call(methods::AGENTS_SET_ENABLED, AgentsEnabledParams { enabled: true })
        .await
        .unwrap();
    assert_eq!(accounts(&mut again).await.len(), 1);

    // Sent away without turning them off: they may come back.
    let sent: AgentsDisconnected = desktop.call(methods::AGENTS_DISCONNECT, Value::Null).await.unwrap();
    assert_eq!(sent.disconnected, 1);
    assert!(
        again
            .call::<_, Value>(methods::ACCOUNT_LIST, Value::Null)
            .await
            .is_err()
    );
    let mut back = Client::connect(&endpoint, ClientKind::Cli).await.unwrap();
    assert_eq!(accounts(&mut back).await.len(), 1);
    handle.abort();
}
