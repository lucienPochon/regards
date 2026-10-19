//! The outbox runner (ADR-0010 §3): sends queued messages once their undo
//! window is over, or at their time (send later), at most once each. It is the
//! `device` executor: entries for other executors are not its to send.
//!
//! - **Claim first:** an entry becomes `sending` in one guarded write, so an
//!   undo arriving at the same moment either wins (nothing is sent) or finds
//!   it already gone.
//! - **Outcomes** follow the SMTP client: not sent and retryable (back to
//!   `queued` with backoff from 5 s to 10 min), not sent for good (`failed`),
//!   or perhaps sent (`uncertain`, never retried without the user).
//! - **Afterwards:** the message it answers is marked `\Answered`, and a copy
//!   goes to the Sent folder unless the provider keeps its own (Gmail,
//!   Microsoft).
//! - **At startup**, an entry left `sending` by a crash becomes `uncertain`.
//! - **Late** (ADR-0011 §2): a scheduled message found more than the grace
//!   (an hour by default) past its time, because the computer was off or
//!   asleep, is held for the person instead of going out unasked.
//! - **Wall clock, checked often:** it naps at most [`MAX_NAP`], so a send is
//!   on time after the computer sleeps or its clock changes.

use crate::State;
use crate::compose::now_ms;
use crate::sync;
use regards_core::mailbox::Role;
use regards_imap::SystemFlag;
use regards_smtp::{Credentials, Envelope, Security, SmtpServer, SubmitError, Submitted};
use regards_store::accounts::{self, Account};
use regards_store::notices::{self, Kind, NewNotice};
use regards_store::outbox::{self, Entry, State as EntryState};
use regards_store::{StoreError, mailboxes};
use regards_vault::SecretKey;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

const FIRST_RETRY: Duration = Duration::from_secs(5);
const MAX_RETRY: Duration = Duration::from_secs(10 * 60);
/// Copying to Sent is given up after this many tries; the message did go.
const MAX_COPY_ATTEMPTS: u32 = 10;
/// Monotonic timers may stop while the computer sleeps; the wall clock does not.
const MAX_NAP: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct Outbox {
    notify: Notify,
}

impl Outbox {
    /// Something was queued or sent again: look now.
    pub(crate) fn wake(&self) {
        self.notify.notify_one();
    }
}

pub(crate) async fn run(state: Arc<State>) {
    // Read first: a write at every start would notify every client for nothing.
    let stranded = state
        .store
        .read(|conn| Ok(outbox::list(conn)?.iter().any(|e| e.state == EntryState::Sending)))
        .await;
    if let Ok(true) = stranded {
        match state.store.write(outbox::recover).await {
            Ok(done) => tracing::warn!(
                entries = done.value,
                "messages left mid-send by a crash are now uncertain"
            ),
            Err(error) => tracing::error!(%error, "cannot recover the outbox"),
        }
    }
    loop {
        if let Err(error) = process(&state).await {
            tracing::warn!(%error, "the outbox paused");
        }
        let next = state.store.read(outbox::next_due_at).await.ok().flatten();
        let wait = next.map(|at| Duration::from_millis(u64::try_from(at - now_ms()).unwrap_or(0)).min(MAX_NAP));
        tokio::select! {
            () = async {
                match wait {
                    Some(wait) => tokio::time::sleep(wait).await,
                    None => std::future::pending().await,
                }
            } => {}
            () = state.outbox.notify.notified() => {}
        }
    }
}

async fn process(state: &Arc<State>) -> Result<(), StoreError> {
    let now = now_ms();
    let due = state.store.read(move |conn| outbox::due(conn, now)).await?;
    let grace = i64::try_from(state.late_send_grace.as_millis()).unwrap_or(i64::MAX);
    for entry in due {
        match entry.state {
            EntryState::Queued if late(entry.send_at, now_ms(), grace) => {
                let id = entry.id;
                let notice = notice_about(&entry, Kind::SendHeld);
                let held = state
                    .store
                    .write(move |tx| {
                        let held = outbox::hold(tx, id)?;
                        if held {
                            notices::add(tx, &notice, now_ms())?;
                        }
                        Ok(held)
                    })
                    .await?
                    .value;
                if held {
                    tracing::info!(
                        outbox = id,
                        "a scheduled message is well past its time; held for the person"
                    );
                }
            }
            EntryState::Queued => send_one(state, &entry).await?,
            EntryState::Sent => copy_to_sent(state, &entry).await?,
            _ => {}
        }
    }
    Ok(())
}

async fn send_one(state: &Arc<State>, entry: &Entry) -> Result<(), StoreError> {
    let (id, now) = (entry.id, now_ms());
    if !state.store.write(move |tx| outbox::claim(tx, id, now)).await?.value {
        return Ok(()); // Taken back, or claimed already.
    }
    let attempts = entry.attempts + 1;
    let outcome = deliver(state, entry).await;
    match outcome {
        Ok(submitted) => {
            tracing::info!(outbox = id, "message sent");
            let answering = entry.answering;
            let account = state
                .store
                .write(move |tx| {
                    outbox::sent(tx, id, &submitted.reply, now_ms())?;
                    match answering {
                        Some(message) => crate::actions::mark_answered(tx, message, now_ms()),
                        None => Ok(None),
                    }
                })
                .await?
                .value;
            if let Some(account) = account {
                state.queue.wake(state, &account);
            }
            if let Some(sent) = state.store.read(move |conn| outbox::get(conn, id)).await? {
                copy_to_sent(state, &sent).await?;
            }
        }
        Err(SubmitError::NotSent { reason, retry: true }) => {
            let delay = FIRST_RETRY
                .saturating_mul(2u32.saturating_pow(attempts.saturating_sub(1).min(16)))
                .min(MAX_RETRY);
            tracing::info!(outbox = id, retry_in = ?delay, "message not sent yet");
            let next = now_ms() + delay.as_millis() as i64;
            state
                .store
                .write(move |tx| outbox::retry_later(tx, id, &reason, next))
                .await?;
        }
        Err(SubmitError::NotSent { reason, retry: false } | SubmitError::SignIn(reason)) => {
            tracing::warn!(outbox = id, "message not sent");
            let notice = notice_about(entry, Kind::SendFailed);
            state
                .store
                .write(move |tx| {
                    outbox::failed(tx, id, &reason)?;
                    notices::add(tx, &notice, now_ms()).map(drop)
                })
                .await?;
        }
        Err(SubmitError::Uncertain(reason)) => {
            tracing::warn!(outbox = id, "message may or may not have been sent");
            let notice = notice_about(entry, Kind::SendUncertain);
            state
                .store
                .write(move |tx| {
                    outbox::uncertain(tx, id, &reason)?;
                    notices::add(tx, &notice, now_ms()).map(drop)
                })
                .await?;
        }
    }
    Ok(())
}

/// A notice about an outbox entry: its recipients and subject.
fn notice_about(entry: &Entry, kind: Kind) -> NewNotice {
    NewNotice {
        account_id: entry.account_id.clone(),
        kind,
        message_id: None,
        outbox_id: Some(entry.id),
        people: entry.recipients.clone(),
        subject: entry.subject.clone(),
    }
}

/// Scheduled, and found more than `grace` ms past its time.
fn late(send_at: Option<i64>, now: i64, grace: i64) -> bool {
    send_at.is_some_and(|at| now.saturating_sub(at) > grace)
}

/// Hand the frozen bytes to the account's SMTP server.
async fn deliver(state: &State, entry: &Entry) -> Result<Submitted, SubmitError> {
    let account_id = entry.account_id.clone();
    let id = entry.id;
    let found = state
        .store
        .read(move |conn| Ok((accounts::get(conn, &account_id)?, outbox::mime(conn, id)?)))
        .await
        .map_err(|error| not_sent(&format!("could not read the message: {error}"), true))?;
    let (Some(account), Some(mime)) = found else {
        return Err(not_sent("the account or the message no longer exists", false));
    };
    let server = smtp_server(&account);
    let envelope = Envelope {
        from: entry.envelope_from.clone(),
        to: entry.envelope_to.clone(),
    };
    if crate::oauth::provider_of(account.auth_kind).is_some() {
        let token_error = |error: crate::oauth::TokenError| not_sent(&error.to_string(), error.passes());
        let (token, kept) = crate::oauth::access_token(state, &account, None)
            .await
            .map_err(token_error)?;
        let credentials = Credentials {
            username: &account.username,
            password: &token,
            oauth: true,
        };
        return match regards_smtp::submit(&server, &credentials, &envelope, &mime, &state.tls).await {
            // Turned down before anything was sent: renew the token, once.
            Err(SubmitError::SignIn(_)) if kept => {
                let (token, _) = crate::oauth::access_token(state, &account, Some(&token))
                    .await
                    .map_err(token_error)?;
                let credentials = Credentials {
                    password: &token,
                    ..credentials
                };
                regards_smtp::submit(&server, &credentials, &envelope, &mime, &state.tls).await
            }
            other => other,
        };
    }
    let vault = crate::accounts::vault(state)
        .await
        .map_err(|error| not_sent(&error.message, true))?;
    let password = vault
        .get(&SecretKey::from_stored(account.secret_key.clone()))
        .await
        .map_err(|error| not_sent(&format!("the system credential store is not available: {error}"), true))?
        .ok_or_else(|| {
            not_sent(
                "the saved password is missing; remove the account and add it again",
                false,
            )
        })?;
    let credentials = Credentials {
        username: &account.username,
        password: &password,
        oauth: false,
    };
    regards_smtp::submit(&server, &credentials, &envelope, &mime, &state.tls).await
}

pub(crate) fn smtp_server(account: &Account) -> SmtpServer {
    SmtpServer {
        host: account.smtp.host.clone(),
        port: account.smtp.port,
        security: match account.smtp.security {
            accounts::Security::Tls => Security::Tls,
            accounts::Security::Starttls => Security::StartTls,
        },
    }
}

fn not_sent(reason: &str, retry: bool) -> SubmitError {
    SubmitError::NotSent {
        reason: reason.to_owned(),
        retry,
    }
}

/// Providers that file what is sent through them in Sent themselves; a copy
/// from us would be a duplicate (ADR-0010 §3).
fn keeps_sent_copies(imap_host: &str) -> bool {
    let host = imap_host.to_ascii_lowercase();
    [
        "gmail.com",
        "googlemail.com",
        "office365.com",
        "outlook.com",
        "hotmail.com",
        "live.com",
    ]
    .iter()
    .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

/// Put a sent message into the Sent folder (marked read), then forget the entry.
async fn copy_to_sent(state: &Arc<State>, entry: &Entry) -> Result<(), StoreError> {
    let (id, account_id) = (entry.id, entry.account_id.clone());
    let found = state
        .store
        .read(move |conn| {
            let account = accounts::get(conn, &account_id)?;
            let sent = match &account {
                Some(account) => mailboxes::list(conn, &account.id)?
                    .into_iter()
                    .find(|m| m.role == Some(Role::Sent) && m.selectable),
                None => None,
            };
            Ok((account, sent, outbox::mime(conn, id)?))
        })
        .await?;
    let (account, sent, mime) = found;
    let needs_copy = account.as_ref().is_some_and(|a| !keeps_sent_copies(&a.imap.host));
    let (true, Some(sent), Some(mime)) = (needs_copy, sent, mime) else {
        state.store.write(move |tx| outbox::finish(tx, id)).await?;
        return Ok(());
    };
    let copied: Result<(), sync::SyncError> = async {
        let mut session = sync::login(state, &entry.account_id).await?;
        session.append(&sent.remote_name, &[SystemFlag::Seen], &mime).await?;
        // The copy is made: whatever happens next must not make another one.
        // Showing it in Sent (and its conversation) at once is best effort;
        // the next sync brings it otherwise.
        if let Err(error) = sync::sync_mailbox(state, &mut session, &sent).await {
            tracing::info!(outbox = id, %error, "the Sent folder will show the copy at its next sync");
        }
        session.logout().await;
        Ok(())
    }
    .await;
    match copied {
        Ok(()) => {
            state.store.write(move |tx| outbox::finish(tx, id)).await?;
        }
        Err(error) if entry.attempts >= MAX_COPY_ATTEMPTS => {
            tracing::warn!(outbox = id, %error, "giving up copying a sent message to Sent");
            state.store.write(move |tx| outbox::finish(tx, id)).await?;
        }
        Err(error) => {
            tracing::info!(outbox = id, %error, "copying a sent message to Sent failed; trying later");
            let next = now_ms() + MAX_RETRY.min(FIRST_RETRY * 12).as_millis() as i64;
            state.store.write(move |tx| outbox::copy_later(tx, id, next)).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_scheduled_sends_well_past_their_time_are_late() {
        let hour = 3_600_000;
        // Sent now, however long it waited (offline, say): never late.
        assert!(!late(None, 10 * hour, hour));
        // On time, or a little late after a short sleep: it goes.
        assert!(!late(Some(5 * hour), 5 * hour, hour));
        assert!(!late(Some(5 * hour), 6 * hour, hour));
        // More than the grace: held.
        assert!(late(Some(5 * hour), 6 * hour + 1, hour));
        assert!(!late(Some(i64::MIN), i64::MAX, i64::MAX), "no overflow");
    }

    #[test]
    fn providers_that_file_sent_mail_themselves() {
        assert!(keeps_sent_copies("imap.gmail.com"));
        assert!(keeps_sent_copies("outlook.office365.com"));
        assert!(keeps_sent_copies("imap-mail.outlook.com"));
        assert!(!keeps_sent_copies("mail.infomaniak.com"));
        assert!(!keeps_sent_copies("notgmail.com"));
        assert!(!keeps_sent_copies("localhost"));
    }
}
