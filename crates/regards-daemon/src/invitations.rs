//! Invitations by email (iMIP, RFC 6047): what an invitation in a message
//! asks, and answering it. The answer is a message to the organizer with an
//! iCalendar REPLY, queued in the outbox like any other (it marks the
//! invitation answered once sent). What the person answered is kept, per
//! version of the invitation, to show beside it. `ui` only.

use crate::State;
use crate::compose::{now_ms, outgoing_from};

use regards_core::calendar::{self, Answer, Invitation, When};
use regards_core::executor::Executor;
use regards_core::headers::Person;
use regards_ipc::protocol::{
    CalendarTime, InvitationAnswer, InvitationRespondParams, InvitationView, MessageRef, RpcError, codes,
};
use regards_store::outbox::{self, NewEntry};
use regards_store::{StoreError, accounts, identities, mailboxes, settings};
use serde_json::{Value, json};

/// The invitation in a message, or null; its body is downloaded if needed.
pub(crate) async fn get(state: &State, params: MessageRef) -> Result<Value, RpcError> {
    let Some((invitation, account_id)) = read(state, params.id).await? else {
        return Ok(Value::Null);
    };
    let view = state
        .store
        .read(move |conn| view(conn, &account_id, &invitation))
        .await
        .map_err(crate::session::internal)?;
    Ok(json!(view))
}

/// Answer the invitation in a message: a reply to its organizer, from the
/// address it was sent to (or the account's own), at once.
pub(crate) async fn respond(state: &State, params: InvitationRespondParams) -> Result<Value, RpcError> {
    let (message_id, answer) = (params.message_id, params.answer);
    let (invitation, account_id) = read(state, message_id)
        .await?
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "this message holds no invitation"))?;
    if invitation.method != "REQUEST" {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            "this is not an invitation to answer (it may have been cancelled)",
        ));
    }
    let organizer = invitation
        .organizer
        .clone()
        .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, "the invitation names no organizer to answer"))?;
    let now = now_ms();
    let view = state
        .store
        .write(move |tx| {
            let account =
                accounts::get(tx, &account_id)?.ok_or_else(|| StoreError::Conflict("no such account".into()))?;
            let identity = answering_identity(tx, &account, &invitation)?;
            let me = Person {
                name: Some(identity.name.clone()).filter(|name| !name.trim().is_empty() && name != &identity.email),
                address: Some(identity.email.clone()),
            };
            let message = regards_store::messages::get(tx, message_id)?
                .ok_or_else(|| StoreError::Conflict("no such message".into()))?;
            let summary = if invitation.summary.trim().is_empty() {
                "(no title)"
            } else {
                invitation.summary.as_str()
            };
            let who = me.name.clone().unwrap_or_else(|| identity.email.clone());
            let said = match answer {
                InvitationAnswer::Accepted => "accepted",
                InvitationAnswer::Tentative => "tentatively accepted",
                InvitationAnswer::Declined => "declined",
            };
            let reply = invitation.reply(&me, core_answer(answer), now);
            let outgoing = outgoing_from(
                &identity,
                vec![organizer.clone()],
                format!("{}: {summary}", core_answer(answer).word()),
                format!("{who} {said} the invitation to {summary}.\n"),
                message.message_id.clone(),
                now,
                reply,
            );
            let mime = outgoing
                .build()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
            outbox::queue(
                tx,
                &NewEntry {
                    account_id: account.id.clone(),
                    draft_id: None,
                    message_id: outgoing.message_id.clone(),
                    subject: outgoing.subject.clone(),
                    recipients: vec![organizer],
                    envelope_from: identity.email.clone(),
                    envelope_to: outgoing.recipients(),
                    mime,
                    undo_until: now,
                    send_at: None,
                    answering: Some(message_id),
                    executor: Executor::Device,
                    by_agent: false,
                },
                now,
            )?;
            settings::set(
                tx,
                &answer_key(&account.id, &invitation.uid),
                &format!("{}:{}", invitation.sequence, answer_word(answer)),
            )?;
            view(tx, &account.id, &invitation)
        })
        .await
        .map_err(|error| match error {
            StoreError::Conflict(message) => RpcError::new(codes::CONFLICT, message),
            other => crate::session::internal(other),
        })?
        .value;
    state.outbox.wake();
    Ok(json!(view))
}

/// The invitation a message carries, and its account; `None` without one.
async fn read(state: &State, message_id: i64) -> Result<Option<(Invitation, String)>, RpcError> {
    let (message, source) = crate::mail::load(state, message_id).await?;
    let mailbox = message.mailbox_id;
    let account = state
        .store
        .read(move |conn| mailboxes::get(conn, mailbox))
        .await
        .map_err(crate::session::internal)?
        .map(|mailbox| mailbox.account_id)
        .ok_or_else(|| RpcError::new(codes::NOT_FOUND, "no such message"))?;
    Ok(calendar::in_message(&source)
        .and_then(|ics| calendar::parse(&ics))
        .map(|invitation| (invitation, account)))
}

/// The identity it was sent to, among the account's (an attendee); else the account's own.
fn answering_identity(
    conn: &rusqlite::Connection,
    account: &accounts::Account,
    invitation: &Invitation,
) -> Result<identities::Identity, StoreError> {
    let own = identities::list(conn, &account.id)?;
    match own
        .into_iter()
        .find(|identity| invitation.status_of(&identity.email).is_some())
    {
        Some(found) => Ok(found),
        None => crate::compose::identity_of(conn, account, None),
    }
}

fn view(conn: &rusqlite::Connection, account_id: &str, invitation: &Invitation) -> Result<InvitationView, StoreError> {
    let me = identities::list(conn, account_id)?
        .into_iter()
        .find(|identity| invitation.status_of(&identity.email).is_some())
        .map(|identity| identity.email);
    // Answered here, to this version of it (a changed invitation asks again).
    let answered = settings::get(conn, &answer_key(account_id, &invitation.uid))?.and_then(|kept| {
        let (sequence, word) = kept.split_once(':')?;
        (sequence.parse::<u32>().ok()? >= invitation.sequence).then_some(())?;
        answer_of(word)
    });
    let time = |when: &Option<When>| {
        when.as_ref().map(|when| CalendarTime {
            value: when.value.clone(),
            zone: when.zone.clone(),
            utc: when.utc,
            all_day: when.all_day,
        })
    };
    Ok(InvitationView {
        method: invitation.method.to_ascii_lowercase(),
        summary: invitation.summary.clone(),
        location: invitation.location.clone(),
        start: time(&invitation.start),
        end: time(&invitation.end),
        organizer: invitation.organizer.clone(),
        attendees: invitation.attendees.len(),
        repeats: invitation.repeats,
        status: me
            .as_deref()
            .and_then(|address| invitation.status_of(address))
            .map(str::to_ascii_lowercase),
        me,
        answered,
    })
}

/// The settings key an answer is kept under (no table of its own: one line each).
fn answer_key(account_id: &str, uid: &str) -> String {
    format!("invitation.answer.{account_id}.{uid}")
}

fn answer_word(answer: InvitationAnswer) -> &'static str {
    match answer {
        InvitationAnswer::Accepted => "accepted",
        InvitationAnswer::Tentative => "tentative",
        InvitationAnswer::Declined => "declined",
    }
}

fn answer_of(word: &str) -> Option<InvitationAnswer> {
    match word {
        "accepted" => Some(InvitationAnswer::Accepted),
        "tentative" => Some(InvitationAnswer::Tentative),
        "declined" => Some(InvitationAnswer::Declined),
        _ => None,
    }
}

fn core_answer(answer: InvitationAnswer) -> Answer {
    match answer {
        InvitationAnswer::Accepted => Answer::Accepted,
        InvitationAnswer::Tentative => Answer::Tentative,
        InvitationAnswer::Declined => Answer::Declined,
    }
}
