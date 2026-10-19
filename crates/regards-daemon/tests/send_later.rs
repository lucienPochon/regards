//! Send later end to end (ADR-0011 §2, plan §5.7), against fake IMAP and SMTP
//! servers: a scheduled message goes at its time and not before, dated for
//! then; it can be moved, sent now or taken back; and after the daemon was
//! down, one well past its time waits for the person while one a little late
//! goes.

#[path = "../../regards-imap/tests/support/fake_imap.rs"]
mod fake_imap;
#[path = "../../regards-smtp/tests/support/fake_smtp.rs"]
mod fake_smtp;
#[path = "support/world.rs"]
mod world;

use regards_core::headers::{self, Person};
use regards_daemon::DEFAULT_LATE_SEND_GRACE;
use regards_ipc::protocol::{
    AccountSummary, DraftCreateParams, DraftRef, DraftSendParams, DraftSendResult, DraftView, NoticeKind,
    NoticeListResult, NoticeShownParams, OutboxEntryView, OutboxListResult, OutboxRef, OutboxRescheduleParams,
    OutboxState, OutboxUndoResult, codes, methods,
};
use regards_ipc::{Client, ClientError};
use serde_json::Value;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use world::{Running, World};

async fn draft(desktop: &mut Client, account: &AccountSummary, subject: &str) -> DraftView {
    desktop
        .call(
            methods::DRAFT_CREATE,
            DraftCreateParams {
                account_id: account.id.clone(),
                to: vec![Person {
                    name: None,
                    address: Some("bob@example.test".into()),
                }],
                subject: subject.into(),
                body: "Hi".into(),
                ..DraftCreateParams::default()
            },
        )
        .await
        .unwrap()
}

async fn send(desktop: &mut Client, id: i64, send_at: Option<i64>) -> Result<DraftSendResult, ClientError> {
    desktop
        .call(
            methods::DRAFT_SEND,
            DraftSendParams {
                id,
                undo_seconds: Some(0),
                send_at,
                remind_at: None,
            },
        )
        .await
}

async fn reschedule(desktop: &mut Client, id: i64, send_at: Option<i64>) -> Result<Value, ClientError> {
    desktop
        .call(methods::OUTBOX_RESCHEDULE, OutboxRescheduleParams { id, send_at })
        .await
}

async fn outbox(desktop: &mut Client) -> Vec<OutboxEntryView> {
    desktop
        .call::<_, OutboxListResult>(methods::OUTBOX_LIST, Value::Null)
        .await
        .unwrap()
        .entries
}

async fn outbox_becomes(desktop: &mut Client, check: impl Fn(&[OutboxEntryView]) -> bool) -> Vec<OutboxEntryView> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let entries = outbox(desktop).await;
        if check(&entries) {
            return entries;
        }
        assert!(Instant::now() < deadline, "the outbox stayed {entries:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn code<T: std::fmt::Debug>(result: Result<T, ClientError>) -> i32 {
    match result {
        Err(ClientError::Rpc(error)) => error.code,
        other => panic!("expected an RPC error, got {other:?}"),
    }
}

fn subject(message: &[u8]) -> String {
    headers::parse(message).subject
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

async fn sleep_until(at: i64) {
    tokio::time::sleep(Duration::from_millis(u64::try_from(at - now_ms()).unwrap_or(0))).await;
}

#[tokio::test]
async fn a_scheduled_message_goes_at_its_time_dated_for_then() {
    let mut world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        mut desktop, handle, ..
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;

    let past = draft(&mut desktop, &account, "Too late").await;
    assert_eq!(
        code(send(&mut desktop, past.id, Some(now_ms() - 1_000)).await),
        codes::INVALID_PARAMS,
        "a time that has passed is refused"
    );

    let on_time = draft(&mut desktop, &account, "On time").await;
    let at = now_ms() + 2_500;
    let queued = send(&mut desktop, on_time.id, Some(at)).await.unwrap();
    assert_eq!(queued.send_at, Some(at));
    let listed = outbox(&mut desktop).await;
    assert_eq!((listed[0].state, listed[0].send_at), (OutboxState::Queued, Some(at)));

    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert!(world.nothing_sent(), "not before its time");
    let message = world.next_message().await;
    assert!(now_ms() >= at);
    assert_eq!(subject(&message), "On time");
    assert_eq!(
        headers::parse(&message).date,
        Some(at / 1000 * 1000),
        "dated for when it goes, not when it was written"
    );
    handle.abort();
}

#[tokio::test]
async fn a_scheduled_message_can_be_moved_sent_now_or_taken_back() {
    let mut world = World::new(DEFAULT_LATE_SEND_GRACE).await;
    let Running {
        mut desktop, handle, ..
    } = world.start().await;
    let account = world.add_account(&mut desktop).await;
    let in_an_hour = now_ms() + 3_600_000;

    // Moved: still waiting, at the new time.
    let later = draft(&mut desktop, &account, "Later").await;
    let queued = send(&mut desktop, later.id, Some(in_an_hour)).await.unwrap();
    reschedule(&mut desktop, queued.outbox_id, Some(in_an_hour + 60_000))
        .await
        .unwrap();
    assert_eq!(outbox(&mut desktop).await[0].send_at, Some(in_an_hour + 60_000));
    assert_eq!(
        code(reschedule(&mut desktop, queued.outbox_id, Some(now_ms() - 1_000)).await),
        codes::INVALID_PARAMS
    );

    // Sent now: it goes, dated now.
    let before = now_ms() / 1000 * 1000;
    reschedule(&mut desktop, queued.outbox_id, None).await.unwrap();
    let message = world.next_message().await;
    assert_eq!(subject(&message), "Later");
    assert!(headers::parse(&message).date.unwrap() >= before);
    outbox_becomes(&mut desktop, <[_]>::is_empty).await;
    let gone = code(reschedule(&mut desktop, queued.outbox_id, Some(in_an_hour)).await);
    assert!(gone == codes::CONFLICT || gone == codes::NOT_FOUND, "gone: {gone}");

    // Taken back: the draft can be edited again, and nothing goes.
    let back = draft(&mut desktop, &account, "Not after all").await;
    let queued = send(&mut desktop, back.id, Some(in_an_hour)).await.unwrap();
    let undone: OutboxUndoResult = desktop
        .call(methods::OUTBOX_UNDO, OutboxRef { id: queued.outbox_id })
        .await
        .unwrap();
    assert_eq!(undone.draft_id, Some(back.id));
    let editable: DraftView = desktop
        .call(methods::DRAFT_GET, DraftRef { id: back.id })
        .await
        .unwrap();
    assert!(!editable.sending);
    assert!(outbox(&mut desktop).await.is_empty());
    handle.abort();
}

#[tokio::test]
async fn after_downtime_one_well_past_its_time_waits_and_one_a_little_late_goes() {
    // A grace of 4 s stands in for the hour.
    let mut world = World::new(Duration::from_secs(4)).await;
    let first = world.start().await;
    let mut desktop = first.desktop;
    let account = world.add_account(&mut desktop).await;
    let well_past = draft(&mut desktop, &account, "Well past its time").await;
    let a_little_late = draft(&mut desktop, &account, "A little late").await;
    let start = now_ms();
    let held = send(&mut desktop, well_past.id, Some(start + 1_500)).await.unwrap();
    send(&mut desktop, a_little_late.id, Some(start + 6_000)).await.unwrap();

    // The computer goes off before either is due, and is back at +8 s: the
    // first is then 6.5 s late, the second 2 s.
    desktop.shutdown().await.unwrap();
    first.handle.await.unwrap();
    drop(first.endpoint);
    assert!(world.nothing_sent());
    sleep_until(start + 8_000).await;
    let Running {
        mut desktop, handle, ..
    } = world.start().await;

    let message = world.next_message().await;
    assert_eq!(subject(&message), "A little late");
    let entries = outbox_becomes(&mut desktop, |e| e.len() == 1 && e[0].state == OutboxState::Held).await;
    assert_eq!(
        (entries[0].id, entries[0].send_at),
        (held.outbox_id, Some(start + 1_500))
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(world.nothing_sent(), "never sent unasked");

    // ...and the person is told, once.
    let told: NoticeListResult = desktop.call(methods::NOTICE_LIST, Value::Null).await.unwrap();
    assert_eq!(told.notices.len(), 1, "{told:?}");
    let notice = &told.notices[0];
    assert_eq!(
        (notice.kind, notice.outbox_id, notice.subject.as_str()),
        (NoticeKind::SendHeld, Some(held.outbox_id), "Well past its time")
    );
    assert_eq!(notice.people[0].address.as_deref(), Some("bob@example.test"));
    let _: Value = desktop
        .call(methods::NOTICE_SHOWN, NoticeShownParams { up_to: notice.id })
        .await
        .unwrap();
    let told: NoticeListResult = desktop.call(methods::NOTICE_LIST, Value::Null).await.unwrap();
    assert!(told.notices.is_empty());

    // The person sends it now.
    reschedule(&mut desktop, held.outbox_id, None).await.unwrap();
    let message = world.next_message().await;
    assert_eq!(subject(&message), "Well past its time");
    handle.abort();
}
