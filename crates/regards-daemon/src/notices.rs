//! What to tell the person (ADR-0011): the scheduler and the outbox runner
//! record notices; the desktop lists the ones not shown yet, shows them as
//! notifications, and marks them shown.

use crate::State;
use regards_ipc::protocol::{
    ArrivalView, ArrivedResult, NoticeKind, NoticeListResult, NoticeShownParams, NoticeView, RpcError,
};
use regards_store::notices::{self, Kind, Notice};
use regards_store::{arrivals, messages};
use serde_json::{Value, json};

/// At most this many at a time; more become a summary anyway.
const PAGE: u32 = 50;

pub(crate) async fn list(state: &State) -> Result<Value, RpcError> {
    let notices = state
        .store
        .read(|conn| notices::unshown(conn, PAGE))
        .await
        .map_err(crate::session::internal)?;
    Ok(json!(NoticeListResult {
        notices: notices.into_iter().map(view).collect(),
    }))
}

/// `mail.arrived`: new mail not told of yet, with what the app filters by.
pub(crate) async fn arrived(state: &State) -> Result<Value, RpcError> {
    let found = state
        .store
        .read(|conn| {
            let mut views = Vec::new();
            for (id, message_id) in arrivals::unshown(conn, PAGE)? {
                let Some(message) = messages::get(conn, message_id)? else {
                    continue;
                };
                let Some(mailbox) = regards_store::mailboxes::get(conn, message.mailbox_id)? else {
                    continue;
                };
                views.push(ArrivalView {
                    id,
                    message_id,
                    account_id: mailbox.account_id,
                    from: message.from,
                    subject: message.subject,
                    section: message.section,
                    received_at: message.received_at,
                });
            }
            Ok(views)
        })
        .await
        .map_err(crate::session::internal)?;
    Ok(json!(ArrivedResult { arrivals: found }))
}

/// `mail.arrived_shown`.
pub(crate) async fn arrived_shown(state: &State, params: NoticeShownParams) -> Result<Value, RpcError> {
    let up_to = params.up_to;
    let pending = state
        .store
        .read(|conn| arrivals::unshown(conn, 1))
        .await
        .map_err(crate::session::internal)?;
    // Read first: a write when nothing changes would wake every client for nothing.
    let marked = if pending.first().is_some_and(|(id, _)| *id <= up_to) {
        state
            .store
            .write(move |tx| arrivals::mark_shown(tx, up_to))
            .await
            .map_err(crate::session::internal)?
            .value
    } else {
        0
    };
    Ok(json!({ "shown": marked }))
}

pub(crate) async fn shown(state: &State, params: NoticeShownParams) -> Result<Value, RpcError> {
    let up_to = params.up_to;
    let changed = state
        .store
        .read(move |conn| Ok(notices::unshown(conn, 1)?.first().is_some_and(|n| n.id <= up_to)))
        .await
        .map_err(crate::session::internal)?;
    // Read first: a write when nothing changes would wake every client for nothing.
    let marked = if changed {
        state
            .store
            .write(move |tx| notices::mark_shown(tx, up_to))
            .await
            .map_err(crate::session::internal)?
            .value
    } else {
        0
    };
    Ok(json!({ "shown": marked }))
}

fn view(notice: Notice) -> NoticeView {
    NoticeView {
        id: notice.id,
        account_id: notice.account_id,
        kind: match notice.kind {
            Kind::SnoozeEnded => NoticeKind::SnoozeEnded,
            Kind::ReminderDue => NoticeKind::ReminderDue,
            Kind::SendHeld => NoticeKind::SendHeld,
            Kind::AgentAsks => NoticeKind::AgentAsks,
            Kind::AgentSending => NoticeKind::AgentSending,
            Kind::SendFailed => NoticeKind::SendFailed,
            Kind::SendUncertain => NoticeKind::SendUncertain,
        },
        message_id: notice.message_id,
        outbox_id: notice.outbox_id,
        people: notice.people,
        subject: notice.subject,
        created_at: notice.created_at,
    }
}
