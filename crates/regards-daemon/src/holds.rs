//! Holds (ADR-0012 §3): what an agent asks that waits for the person.
//!
//! - **Held:** an agent's send (Send access), changing more messages at once
//!   than an agent may, and deleting for good. The agent gets
//!   [`codes::HELD`] with the hold's id, and follows it with `hold.get`.
//! - **The person** is told (a notice), and approves or rejects it in the
//!   app; only the app decides. Approving runs the request as the agent
//!   made it: a send only if the draft is still as it was when held.
//! - **After a day** an undecided hold expires.

use crate::{State, actions, compose};
use regards_core::headers::Person;
use regards_ipc::protocol::{
    DraftSendParams, HoldListParams, HoldListResult, HoldReason, HoldRef, HoldStatus, HoldView, RpcError, codes,
    methods,
};
use regards_store::audit::{self, Decision};
use regards_store::holds::{self, Hold, NewHold, Reason, Status};
use regards_store::notices::{self, Kind, NewNotice};
use regards_store::{StoreError, drafts};
use serde_json::{Value, json};
use std::sync::Arc;

/// How long a hold waits for the person.
const HOLD_FOR_MS: i64 = 24 * 60 * 60 * 1000;
/// The most holds waiting at once: more would bury the person in requests.
const MAX_WAITING: u32 = 20;
const DEFAULT_PAGE: u32 = 50;
const MAX_PAGE: u32 = 200;

/// The agent request being held.
pub(crate) struct Asker<'a> {
    pub session: &'a str,
    pub client: &'a str,
    pub method: &'a str,
    pub params: &'a Value,
}

/// What a hold is about, as the person is told.
pub(crate) struct About {
    pub account_id: String,
    pub reason: Reason,
    pub description: String,
    /// For a send: the draft and its version.
    pub draft: Option<(i64, i64)>,
    /// The notice's who and what.
    pub people: Vec<Person>,
    pub subject: String,
}

/// Hold an agent's request for the person; the answer the agent gets.
pub(crate) async fn hold(state: &State, asker: Asker<'_>, about: About) -> RpcError {
    let now = compose::now_ms();
    match state.store.read(move |conn| holds::waiting(conn, now)).await {
        Ok(waiting) if waiting >= MAX_WAITING => {
            return RpcError::new(
                codes::RATE_LIMITED,
                format!("{waiting} requests already wait for the person; ask again once they have decided some"),
            );
        }
        Ok(_) => {}
        Err(error) => return crate::session::internal(error),
    }
    let new = NewHold {
        created_at: now,
        expires_at: now + HOLD_FOR_MS,
        session: asker.session.to_owned(),
        client: asker.client.to_owned(),
        account_id: about.account_id.clone(),
        method: asker.method.to_owned(),
        params: asker.params.to_string(),
        description: about.description.clone(),
        reason: about.reason,
        draft: about.draft,
    };
    let notice = NewNotice {
        account_id: about.account_id,
        kind: Kind::AgentAsks,
        message_id: None,
        outbox_id: None,
        people: about.people,
        subject: about.subject,
    };
    let held = state
        .store
        .write(move |tx| {
            let id = holds::add(tx, &new)?;
            notices::add(tx, &notice, now)?;
            Ok(id)
        })
        .await;
    match held {
        Ok(committed) => {
            let expires_at = now + HOLD_FOR_MS;
            let mut error = RpcError::new(
                codes::HELD,
                format!(
                    "waiting for the person to approve it in Regards: {}. Follow it with hold.get.",
                    about.description
                ),
            );
            error.data = Some(json!({
                "hold_id": committed.value,
                "expires_at": expires_at,
                "description": about.description,
            }));
            error
        }
        Err(error) => crate::session::internal(error),
    }
}

/// Hold an agent's send: the person sees the draft as it is now, and only
/// that is sent on their approval.
pub(crate) async fn hold_send(state: &State, asker: Asker<'_>, params: &DraftSendParams) -> RpcError {
    let id = params.id;
    let found = state.store.read(move |conn| drafts::get(conn, id)).await;
    let draft = match found {
        Ok(Some(draft)) => draft,
        Ok(None) => return RpcError::new(codes::NOT_FOUND, "no such draft"),
        Err(error) => return crate::session::internal(error),
    };
    if draft.sending {
        return RpcError::new(codes::CONFLICT, "this message is already being sent");
    }
    let people: Vec<Person> = draft.to.iter().chain(&draft.cc).chain(&draft.bcc).cloned().collect();
    if people.is_empty() {
        return RpcError::new(codes::INVALID_PARAMS, "the draft has no recipients");
    }
    let subject = if draft.subject.trim().is_empty() {
        "(no subject)".to_owned()
    } else {
        draft.subject.clone()
    };
    let verb = if params.send_at.is_some() { "Schedule" } else { "Send" };
    let about = About {
        account_id: draft.account_id.clone(),
        reason: Reason::Sending,
        description: format!("{verb} “{subject}” to {}", names(&people)),
        draft: Some((draft.id, draft.updated_at)),
        people,
        subject,
    };
    hold(state, asker, about).await
}

/// "Bob, Ada and 2 others".
fn names(people: &[Person]) -> String {
    let name = |person: &Person| {
        person
            .name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .or_else(|| person.address.clone())
            .unwrap_or_default()
    };
    match people {
        [] => String::new(),
        [one] => name(one),
        [one, two] => format!("{} and {}", name(one), name(two)),
        [one, two, rest @ ..] => format!("{}, {} and {} others", name(one), name(two), rest.len()),
    }
}

/// What a held change to many messages, or deleting them, would do.
pub(crate) fn describe(method: &str, count: usize) -> String {
    let messages = if count == 1 {
        "1 message".to_owned()
    } else {
        format!("{count} messages")
    };
    match method {
        methods::MESSAGE_DELETE => format!("Delete {messages} for good"),
        methods::MESSAGE_ARCHIVE => format!("Archive {messages}"),
        methods::MESSAGE_TRASH => format!("Move {messages} to the Trash"),
        methods::MESSAGE_MOVE => format!("Move {messages} to another folder"),
        methods::MESSAGE_SET_FLAGS => format!("Change the flags of {messages}"),
        methods::MESSAGE_PIN => format!("Pin or unpin {messages}"),
        methods::MESSAGE_SET_ASIDE => format!("Set aside or bring back {messages}"),
        methods::MESSAGE_SNOOZE => format!("Snooze {messages}"),
        methods::MESSAGE_REMIND => format!("Set reminders on {messages}"),
        other => format!("{other} on {messages}"),
    }
}

/// Holds, newest first; only those waiting when asked. Only the app gets here.
pub(crate) async fn list(state: &State, params: HoldListParams) -> Result<Value, RpcError> {
    let limit = params.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=MAX_PAGE).contains(&limit) {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!("limit must be 1 to {MAX_PAGE}"),
        ));
    }
    let (pending, now) = (params.pending, compose::now_ms());
    let holds = state
        .store
        .read(move |conn| holds::list(conn, pending, now, limit))
        .await
        .map_err(crate::session::internal)?;
    Ok(json!(HoldListResult {
        holds: holds.into_iter().map(view).collect(),
    }))
}

/// One hold: for an agent, if its account is in `scope`.
pub(crate) async fn get(state: &State, params: HoldRef, scope: Option<Vec<String>>) -> Result<Value, RpcError> {
    let (id, now) = (params.id, compose::now_ms());
    let found = state
        .store
        .read(move |conn| holds::get(conn, id, now))
        .await
        .map_err(crate::session::internal)?
        .filter(|hold| scope.is_none_or(|open| open.contains(&hold.hold.account_id)))
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such hold"))?;
    Ok(json!(view(found)))
}

/// The person approves: it runs, as the agent asked.
pub(crate) async fn approve(state: &Arc<State>, params: HoldRef) -> Result<Value, RpcError> {
    let (id, now) = (params.id, compose::now_ms());
    let decided = state
        .store
        .write(move |tx| {
            let hold = holds::decide(tx, id, true, now)?;
            if hold.is_some() {
                audit::decided(tx, id, Decision::Approved)?;
            }
            Ok(hold)
        })
        .await
        .map_err(crate::session::internal)?
        .value;
    let Some(hold) = decided else {
        return Err(not_waiting(state, id).await);
    };
    let outcome = run(state, &hold).await;
    if let Err(error) = &outcome {
        tracing::info!(hold = id, error = %error.message, "an approved request could not run");
    }
    let outcome = outcome.map(|result| result.to_string()).map_err(|error| error.message);
    state
        .store
        .write(move |tx| holds::finish(tx, id, outcome.as_deref().map_err(String::as_str)))
        .await
        .map_err(crate::session::internal)?;
    get(state, HoldRef { id }, None).await
}

/// The person says no.
pub(crate) async fn reject(state: &State, params: HoldRef) -> Result<Value, RpcError> {
    let (id, now) = (params.id, compose::now_ms());
    let decided = state
        .store
        .write(move |tx| {
            let hold = holds::decide(tx, id, false, now)?;
            if hold.is_some() {
                audit::decided(tx, id, Decision::Rejected)?;
            }
            Ok(hold)
        })
        .await
        .map_err(crate::session::internal)?
        .value;
    let Some(hold) = decided else {
        return Err(not_waiting(state, id).await);
    };
    Ok(json!(view(hold)))
}

async fn not_waiting(state: &State, id: i64) -> RpcError {
    let now = compose::now_ms();
    match state.store.read(move |conn| holds::get(conn, id, now)).await {
        Ok(Some(_)) => RpcError::new(
            codes::CONFLICT,
            "this is not waiting any more: it was decided, or expired",
        ),
        Ok(None) => RpcError::new(codes::NOT_FOUND, "no such hold"),
        Err(error) => crate::session::internal(error),
    }
}

/// Run an approved request as the agent made it.
async fn run(state: &Arc<State>, hold: &Hold) -> Result<Value, RpcError> {
    let params: Value = serde_json::from_str(&hold.hold.params)
        .map_err(|error| crate::session::internal(StoreError::Corrupt(format!("held request: {error}"))))?;
    fn parse<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, RpcError> {
        serde_json::from_value(params).map_err(|error| RpcError::new(codes::INVALID_PARAMS, error.to_string()))
    }
    match hold.hold.method.as_str() {
        methods::DRAFT_SEND => {
            let version = hold.hold.draft.map(|(_, version)| version);
            compose::send_unchanged(state, parse(params)?, version).await
        }
        methods::MESSAGE_DELETE => actions::delete_forever(state, parse(params)?).await,
        methods::MESSAGE_ARCHIVE => actions::archive(state, parse(params)?).await,
        methods::MESSAGE_TRASH => actions::trash(state, parse(params)?).await,
        methods::MESSAGE_MOVE => actions::move_to(state, parse(params)?).await,
        methods::MESSAGE_SET_FLAGS => actions::set_flags(state, parse(params)?).await,
        methods::MESSAGE_PIN => actions::pin(state, parse(params)?).await,
        methods::MESSAGE_SET_ASIDE => actions::set_aside(state, parse(params)?).await,
        methods::MESSAGE_SNOOZE => actions::snooze(state, parse(params)?).await,
        methods::MESSAGE_REMIND => actions::remind(state, parse(params)?).await,
        other => Err(RpcError::new(codes::INTERNAL_ERROR, format!("{other} cannot be held"))),
    }
}

fn view(hold: Hold) -> HoldView {
    let Hold {
        id,
        hold,
        status,
        decided_at,
        result,
        error,
    } = hold;
    HoldView {
        id,
        created_at: hold.created_at,
        expires_at: hold.expires_at,
        session: hold.session,
        client: hold.client,
        account_id: hold.account_id,
        method: hold.method,
        params: serde_json::from_str(&hold.params).unwrap_or(Value::Null),
        description: hold.description,
        reason: match hold.reason {
            Reason::Sending => HoldReason::Sending,
            Reason::Bulk => HoldReason::Bulk,
            Reason::Delete => HoldReason::Delete,
        },
        draft_id: hold.draft.map(|(id, _)| id),
        status: match status {
            Status::Pending => HoldStatus::Pending,
            Status::Approved => HoldStatus::Approved,
            Status::Rejected => HoldStatus::Rejected,
            Status::Expired => HoldStatus::Expired,
            Status::Failed => HoldStatus::Failed,
        },
        decided_at,
        result: result.and_then(|text| serde_json::from_str(&text).ok()),
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: Option<&str>, address: &str) -> Person {
        Person {
            name: name.map(str::to_owned),
            address: Some(address.into()),
        }
    }

    #[test]
    fn people_are_named_briefly() {
        let bob = person(Some("Bob"), "bob@example.test");
        let ada = person(None, "ada@example.test");
        assert_eq!(names(std::slice::from_ref(&bob)), "Bob");
        assert_eq!(names(&[bob.clone(), ada.clone()]), "Bob and ada@example.test");
        assert_eq!(
            names(&[bob.clone(), ada.clone(), bob, ada]),
            "Bob, ada@example.test and 2 others"
        );
        assert_eq!(describe(methods::MESSAGE_DELETE, 1), "Delete 1 message for good");
        assert_eq!(describe(methods::MESSAGE_ARCHIVE, 120), "Archive 120 messages");
    }
}
