//! Holds (ADR-0012 §3): what an agent asks that waits for the person. Sends
//! never go unasked: the person approves what they saw, or rejects it.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_core::headers::{self, Person};
use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{
    AccountSummary, AgentAccess, AgentAccessParams, AgentActivityParams, AgentActivityResult, AgentDecision,
    AgentSendParams, ClientKind, DraftCreateParams, DraftSendParams, DraftSendResult, DraftUpdateParams, DraftView,
    HoldListParams, HoldListResult, HoldReason, HoldRef, HoldStatus, HoldView, MessageIds, NoticeKind,
    NoticeListResult, OutboxRef, RpcError, codes, methods,
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

fn bob() -> Person {
    Person {
        name: Some("Bob".into()),
        address: Some("bob@example.test".into()),
    }
}

async fn draft(agent: &mut Client, account_id: &str, subject: &str) -> DraftView {
    agent
        .call(
            methods::DRAFT_CREATE,
            DraftCreateParams {
                account_id: account_id.into(),
                to: vec![bob()],
                subject: subject.into(),
                body: "Hello".into(),
                ..DraftCreateParams::default()
            },
        )
        .await
        .unwrap()
}

fn send_now(draft: &DraftView) -> DraftSendParams {
    DraftSendParams {
        id: draft.id,
        undo_seconds: Some(0),
        send_at: None,
        remind_at: None,
    }
}

/// The agent asks to send at once (no undo window): the id of its hold.
async fn ask_to_send(agent: &mut Client, draft: &DraftView) -> i64 {
    let held = refusal(agent.call::<_, Value>(methods::DRAFT_SEND, send_now(draft)).await);
    assert_eq!(held.code, codes::HELD, "{}", held.message);
    held.data.unwrap()["hold_id"].as_i64().unwrap()
}

async fn decide(desktop: &mut Client, method: &str, id: i64) -> HoldView {
    desktop.call(method, HoldRef { id }).await.unwrap()
}

#[tokio::test]
async fn an_agents_send_waits_for_the_person_who_sees_what_goes() {
    let mut world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        endpoint,
        mut desktop,
        handle,
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    let level = |access| AgentAccessParams {
        account_id: account.id.clone(),
        access,
    };
    let _: Value = desktop
        .call(methods::ACCOUNT_SET_AGENT_ACCESS, level(AgentAccess::Triage))
        .await
        .unwrap();
    let mut agent = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();

    // Triage drafts, but does not send.
    let plans = draft(&mut agent, &account.id, "Plans").await;
    let refused = refusal(agent.call::<_, Value>(methods::DRAFT_SEND, send_now(&plans)).await);
    assert_eq!(
        (refused.code, refused.data.unwrap()["needed"].as_str()),
        (codes::FORBIDDEN, Some("send"))
    );

    // Send: it waits, and the person is told.
    let _: Value = desktop
        .call(methods::ACCOUNT_SET_AGENT_ACCESS, level(AgentAccess::Send))
        .await
        .unwrap();
    let id = ask_to_send(&mut agent, &plans).await;
    assert!(world.nothing_sent(), "not before the person says so");
    let waiting: HoldListResult = desktop
        .call(
            methods::HOLD_LIST,
            HoldListParams {
                pending: true,
                limit: None,
            },
        )
        .await
        .unwrap();
    let hold = &waiting.holds[0];
    assert_eq!(hold.description, "Send \u{201c}Plans\u{201d} to Bob");
    assert_eq!(
        (hold.reason, hold.status, hold.draft_id),
        (HoldReason::Sending, HoldStatus::Pending, Some(plans.id))
    );
    let told: NoticeListResult = desktop.call(methods::NOTICE_LIST, Value::Null).await.unwrap();
    assert!(
        told.notices
            .iter()
            .any(|n| n.kind == NoticeKind::AgentAsks && n.subject == "Plans")
    );
    let followed: HoldView = agent.call(methods::HOLD_GET, HoldRef { id }).await.unwrap();
    assert_eq!(followed.status, HoldStatus::Pending);

    // Agents do not decide, not even their own.
    let approved = agent.call::<_, Value>(methods::HOLD_APPROVE, HoldRef { id }).await;
    assert_eq!(refusal(approved).code, codes::FORBIDDEN);

    // Approved: it goes, as the person saw it, once.
    let approved = decide(&mut desktop, methods::HOLD_APPROVE, id).await;
    assert_eq!(approved.status, HoldStatus::Approved);
    assert!(
        approved.result.as_ref().unwrap()["outbox_id"].as_i64().is_some(),
        "{approved:?}"
    );
    let message = world.next_message().await;
    assert_eq!(headers::parse(&message).subject, "Plans");
    let again = desktop.call::<_, HoldView>(methods::HOLD_APPROVE, HoldRef { id }).await;
    assert_eq!(refusal(again).code, codes::CONFLICT);

    // Changed after it was held: approving does not send it.
    let rota = draft(&mut agent, &account.id, "Rota").await;
    let changed = ask_to_send(&mut agent, &rota).await;
    let _: DraftView = agent
        .call(
            methods::DRAFT_UPDATE,
            DraftUpdateParams {
                id: rota.id,
                to: vec![bob()],
                cc: vec![],
                bcc: vec![],
                subject: "Rota (other)".into(),
                body: "Not what was seen".into(),
                html: None,
            },
        )
        .await
        .unwrap();
    let failed = decide(&mut desktop, methods::HOLD_APPROVE, changed).await;
    assert_eq!(failed.status, HoldStatus::Failed);
    assert!(failed.error.as_deref().unwrap().contains("changed"), "{failed:?}");

    // Rejected: nothing goes; the agent sees it.
    let lunch = draft(&mut agent, &account.id, "Lunch").await;
    let refused = ask_to_send(&mut agent, &lunch).await;
    let rejected = decide(&mut desktop, methods::HOLD_REJECT, refused).await;
    assert_eq!(rejected.status, HoldStatus::Rejected);
    let followed: HoldView = agent.call(methods::HOLD_GET, HoldRef { id: refused }).await.unwrap();
    assert_eq!(followed.status, HoldStatus::Rejected);
    assert!(world.nothing_sent(), "only the approved one went");

    // The activity follows the decisions (newest first).
    let activity: AgentActivityResult = desktop
        .call(methods::AGENT_ACTIVITY, AgentActivityParams::default())
        .await
        .unwrap();
    let sends: Vec<AgentDecision> = activity
        .entries
        .iter()
        .filter(|e| e.method == methods::DRAFT_SEND && e.error_code != Some(codes::FORBIDDEN))
        .map(|e| e.decision)
        .collect();
    assert_eq!(
        sends,
        [
            AgentDecision::Rejected,
            AgentDecision::Approved,
            AgentDecision::Approved
        ]
    );

    // Deleting for good waits too, once the messages are known.
    let unknown = agent
        .call::<_, Value>(methods::MESSAGE_DELETE, MessageIds { ids: vec![999] })
        .await;
    assert_eq!(refusal(unknown).code, codes::NOT_FOUND);

    // Closed to agents: its holds are not theirs to follow.
    let _: Value = desktop
        .call(methods::ACCOUNT_SET_AGENT_ACCESS, level(AgentAccess::Off))
        .await
        .unwrap();
    let hidden = agent.call::<_, HoldView>(methods::HOLD_GET, HoldRef { id }).await;
    assert_eq!(refusal(hidden).code, codes::NOT_FOUND);
    handle.abort();
}

#[tokio::test]
async fn with_an_undo_window_an_agents_send_goes_unless_the_person_calls_it_back() {
    let mut world = World::new(DEFAULT_LATE_SEND_GRACE).await;
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
    let window = |seconds: Option<u32>| AgentSendParams {
        account_id: account.id.clone(),
        window_seconds: seconds,
    };
    let mut agent = Client::connect(&endpoint, ClientKind::Mcp).await.unwrap();

    // Only the person sets it, and not too short.
    let refused = agent
        .call::<_, Value>(methods::ACCOUNT_SET_AGENT_SEND, window(Some(10)))
        .await;
    assert_eq!(refusal(refused).code, codes::FORBIDDEN);
    let refused = desktop
        .call::<_, Value>(methods::ACCOUNT_SET_AGENT_SEND, window(Some(5)))
        .await;
    assert_eq!(refusal(refused).code, codes::INVALID_PARAMS);
    let _: Value = desktop
        .call(methods::ACCOUNT_SET_AGENT_SEND, window(Some(10)))
        .await
        .unwrap();
    let listed: Vec<AccountSummary> = desktop.call(methods::ACCOUNT_LIST, Value::Null).await.unwrap();
    assert_eq!(listed[0].agent_send_window, Some(10));

    // Queued, not held; the person is told and can call it back, the agent cannot.
    let started = std::time::Instant::now();
    let regret = draft(&mut agent, &account.id, "Regret").await;
    let queued: DraftSendResult = agent.call(methods::DRAFT_SEND, send_now(&regret)).await.unwrap();
    assert!(queued.undo_until >= now_ms() + 8_000, "the window, not the agent's 0 s");
    let told: NoticeListResult = desktop.call(methods::NOTICE_LIST, Value::Null).await.unwrap();
    assert!(
        told.notices.iter().any(|n| n.kind == NoticeKind::AgentSending
            && n.subject == "Regret"
            && n.outbox_id == Some(queued.outbox_id))
    );
    let undo = OutboxRef { id: queued.outbox_id };
    assert_eq!(
        refusal(agent.call::<_, Value>(methods::OUTBOX_UNDO, undo).await).code,
        codes::FORBIDDEN
    );
    let _: Value = desktop.call(methods::OUTBOX_UNDO, undo).await.unwrap();

    // Left alone: it goes once the window is over.
    let fine = draft(&mut agent, &account.id, "Fine").await;
    let _: DraftSendResult = agent.call(methods::DRAFT_SEND, send_now(&fine)).await.unwrap();
    let message = world.next_message().await;
    assert_eq!(headers::parse(&message).subject, "Fine");
    assert!(
        started.elapsed() >= std::time::Duration::from_secs(9),
        "not before the window"
    );
    assert!(world.nothing_sent(), "the one called back never went");

    // Past 20 an hour, sends wait for approval again.
    for n in 2..20 {
        let more = draft(&mut desktop, &account.id, &format!("More {n}")).await;
        let _: DraftSendResult = agent.call(methods::DRAFT_SEND, send_now(&more)).await.unwrap();
    }
    let one_too_many = draft(&mut desktop, &account.id, "One too many").await;
    ask_to_send(&mut agent, &one_too_many).await;

    // Back to approval for each.
    let _: Value = desktop
        .call(methods::ACCOUNT_SET_AGENT_SEND, window(None))
        .await
        .unwrap();
    let listed: Vec<AccountSummary> = desktop.call(methods::ACCOUNT_LIST, Value::Null).await.unwrap();
    assert_eq!(listed[0].agent_send_window, None);
    handle.abort();
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
