//! One client connection: handshake, then request dispatch.

use crate::{
    State, access, accounts, actions, agents, assistant, attachments, audit, compose, context, discover, draft_sync,
    erase, folders, holds, identities, labels, mail, notes, notices, oauth, people, prefetch, saved, signatures, smart,
    templates, unsubscribe, voice,
};
use regards_core::agents::Access;
use regards_core::build_info::{BUILD_ID, VERSION};
use regards_ipc::framing::{self, FrameError};
use regards_ipc::protocol::{
    AccountRef, AsideParams, AttachmentRef, ChangesNotification, ClientKind, DraftCreateParams, DraftForwardParams,
    DraftListParams, DraftRef, DraftReplyParams, DraftSendParams, DraftSetFromParams, DraftUpdateParams, HealthResult,
    HelloParams, HelloResult, HoldRef, JSONRPC, MemoryParams, MessageHtmlParams, MessageIds, MessageListParams,
    MessageRef, MoveParams, NoteAddParams, NoteListParams, NoteRef, Notification, PROTOCOL_VERSIONS,
    PeopleSuggestParams, PinParams, Principal, RemindParams, Request, Response, RpcError, SearchParams,
    SenderBlockParams, SenderSetParams, SetFlagsParams, SnoozeParams, ViewParams, VoiceSuggestParams, codes, methods,
    negotiate, notifications,
};
use regards_ipc::transport::Stream;
use regards_store::holds::Reason;
use regards_store::notes::Author;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// The most messages an agent may change in one request; more is a bulk
/// change, which waits for the person (ADR-0012 §3).
const MAX_AGENT_CHANGES: usize = 50;
/// A client must say `hello` within this time or the connection is closed.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

struct Session {
    state: Arc<State>,
    principal: Option<Principal>,
    client: Option<ClientKind>,
    /// An agent's connection, as its activity names it (ADR-0012 §4).
    agent_session: Option<String>,
    /// The account the current request was checked against, and its level.
    checked: Mutex<Option<(String, Access)>>,
    /// For an agent's change: how what it changes stood before.
    before: Mutex<Option<Value>>,
    /// Asked to stop the daemon: done once the reply is sent.
    stop_after_reply: bool,
    /// What an agent may still ask (ADR-0012 §5).
    limits: crate::limits::Limits,
    /// Set by `changes.subscribe`: changes to forward as notifications.
    feed: Option<watch::Receiver<ChangesNotification>>,
    /// For an agent that said which app it runs in: listed while connected.
    app: Option<agents::AppSeen>,
}

/// What woke the session up.
enum Next {
    Request(Option<Result<Value, FrameError>>),
    Change,
    FeedClosed,
}

pub(crate) async fn run(stream: Stream, state: Arc<State>) -> Result<(), FrameError> {
    let mut framed = framing::framed(stream);
    let mut session = Session {
        state,
        principal: None,
        client: None,
        agent_session: None,
        checked: Mutex::new(None),
        before: Mutex::new(None),
        stop_after_reply: false,
        limits: crate::limits::Limits::new(),
        feed: None,
        app: None,
    };

    // Set once an agent has said hello: it changes when agents are sent away.
    let mut kick: Option<watch::Receiver<u64>> = None;
    loop {
        if kick.is_none() && session.agent_session.is_some() {
            kick = Some(session.state.agents.subscribe());
        }
        let incoming = if let Some(feed) = session.feed.as_mut() {
            // Sent away first: a request that came at the same time is not served.
            let next = tokio::select! {
                biased;
                () = kicked(&mut kick) => return Ok(()),
                incoming = framing::recv::<_, Value>(&mut framed) => Next::Request(incoming),
                changed = feed.changed() => if changed.is_ok() { Next::Change } else { Next::FeedClosed },
            };
            match next {
                Next::Request(incoming) => incoming,
                Next::Change => {
                    let change = *feed.borrow_and_update();
                    let notification = Notification {
                        jsonrpc: JSONRPC.to_owned(),
                        method: notifications::CHANGES.to_owned(),
                        params: to_value(change),
                    };
                    framing::send(&mut framed, &notification).await?;
                    continue;
                }
                Next::FeedClosed => {
                    session.feed = None;
                    continue;
                }
            }
        } else if session.principal.is_none() {
            match tokio::time::timeout(HANDSHAKE_TIMEOUT, framing::recv::<_, Value>(&mut framed)).await {
                Ok(incoming) => incoming,
                Err(_) => {
                    tracing::debug!("closing a connection that did not complete the handshake");
                    return Ok(());
                }
            }
        } else {
            tokio::select! {
                biased;
                () = kicked(&mut kick) => return Ok(()),
                incoming = framing::recv::<_, Value>(&mut framed) => incoming,
            }
        };

        let response = match incoming {
            None => return Ok(()),
            Some(Err(FrameError::Json(error))) => Response::failure(
                None,
                RpcError::new(codes::PARSE_ERROR, format!("invalid JSON: {error}")),
            ),
            Some(Err(error)) => return Err(error),
            Some(Ok(value)) => session.handle(value).await,
        };

        framing::send(&mut framed, &response).await?;

        if session.stop_after_reply {
            let _ = session.state.stop.send(true);
        }
        if session.state.stop.borrow().to_owned() {
            return Ok(());
        }
    }
}

/// Resolves when agents are sent away; never for the app's connection.
async fn kicked(kick: &mut Option<watch::Receiver<u64>>) {
    match kick {
        Some(kick) => {
            let _ = kick.changed().await;
            tracing::debug!("an agent's connection was ended");
        }
        None => std::future::pending().await,
    }
}

impl Session {
    async fn handle(&mut self, value: Value) -> Response {
        let id = value.get("id").and_then(Value::as_u64);
        let request: Request = match serde_json::from_value(value) {
            Ok(request) => request,
            Err(error) => {
                return Response::failure(
                    id,
                    RpcError::new(codes::INVALID_REQUEST, format!("invalid request: {error}")),
                );
            }
        };
        if request.jsonrpc != "2.0" {
            return Response::failure(
                Some(request.id),
                RpcError::new(codes::INVALID_REQUEST, "jsonrpc must be \"2.0\""),
            );
        }
        let started = Instant::now();
        let outcome = match self.agent_session {
            Some(_) => match self.limits.take(&request.method) {
                Ok(()) => self.dispatch(&request).await,
                Err(refused) => Err(refused),
            },
            None => self.dispatch(&request).await,
        };
        // Everything an agent asks, but the handshake, is kept (ADR-0012 §4).
        if let (Some(session), Some(client)) = (&self.agent_session, self.client)
            && request.method != methods::HELLO
        {
            let checked = self
                .checked
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            let before = self
                .before
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            // What reverses it, when it changed something that can be undone.
            let undo_id = outcome
                .as_ref()
                .ok()
                .and_then(|result| result.get("undo")?.get("id")?.as_i64());
            let answered = audit::Answered {
                session,
                client,
                method: &request.method,
                params: &request.params,
                checked,
                error: outcome.as_ref().err(),
                duration: started.elapsed(),
                before,
                undo_id,
            };
            audit::record(&self.state, answered).await;
        }
        match outcome {
            Ok(result) => Response::success(request.id, result),
            Err(error) => Response::failure(Some(request.id), error),
        }
    }

    async fn dispatch(&mut self, request: &Request) -> Result<Value, RpcError> {
        if request.method == methods::HELLO {
            return self.hello(&request.params);
        }
        if self.principal.is_none() {
            return Err(RpcError::new(
                codes::HANDSHAKE_REQUIRED,
                "the first request must be \"hello\"",
            ));
        }
        if self.agent_session.is_some() && !self.state.agents.enabled() {
            return Err(agents::turned_off());
        }
        match request.method.as_str() {
            methods::DAEMON_HEALTH => Ok(to_value(HealthResult {
                status: "ok".to_owned(),
                version: VERSION.to_owned(),
                build_id: BUILD_ID.to_owned(),
                pid: std::process::id(),
                uptime_secs: self.state.started.elapsed().as_secs(),
                revision: self.state.store.revision().await.map_err(internal)?,
            })),
            methods::DAEMON_ERASE => {
                self.require_ui()?;
                let erased = erase::erase(&self.state, params(request)?).await?;
                // Once the reply is out: its files can go when nothing holds them.
                self.stop_after_reply = true;
                Ok(erased)
            }
            methods::DAEMON_SHUTDOWN => {
                tracing::info!("shutdown requested by a client");
                // Once the reply is out: the daemon may be gone right after.
                self.stop_after_reply = true;
                Ok(json!({ "stopping": true }))
            }
            methods::ACCOUNT_LIST => accounts::list(&self.state, !self.is_ui()).await,
            methods::ACCOUNT_SET_AGENT_ACCESS => {
                self.require_ui()?;
                accounts::set_agent_access(&self.state, params(request)?).await
            }
            methods::ACCOUNT_SET_AGENT_SEND => {
                self.require_ui()?;
                accounts::set_agent_send(&self.state, params(request)?).await
            }
            methods::AGENTS_SETTINGS => {
                self.require_ui()?;
                Ok(agents::settings(&self.state))
            }
            methods::AGENTS_SET_ENABLED => {
                self.require_ui()?;
                agents::set_enabled(&self.state, params(request)?).await
            }
            methods::AGENTS_DISCONNECT => {
                self.require_ui()?;
                Ok(agents::disconnect(&self.state))
            }
            methods::AGENT_ACTIVITY => {
                self.require_ui()?;
                audit::activity(&self.state, params(request)?).await
            }
            methods::HOLD_LIST => {
                self.require_ui()?;
                holds::list(&self.state, params(request)?).await
            }
            methods::HOLD_GET => {
                let params: HoldRef = params(request)?;
                let scope = self.scope(Access::Read, None).await?;
                holds::get(&self.state, params, scope).await
            }
            methods::HOLD_APPROVE => {
                self.require_ui()?;
                holds::approve(&self.state, params(request)?).await
            }
            methods::HOLD_REJECT => {
                self.require_ui()?;
                holds::reject(&self.state, params(request)?).await
            }
            methods::ACCOUNT_ADD => {
                self.require_ui()?;
                accounts::add(&self.state, params(request)?).await
            }
            methods::ACCOUNT_REMOVE => {
                self.require_ui()?;
                accounts::remove(&self.state, params(request)?).await
            }
            methods::ACCOUNT_UPDATE => {
                self.require_ui()?;
                accounts::update(&self.state, params(request)?).await
            }
            methods::MAILBOX_CREATE => {
                self.require_ui()?;
                folders::create(&self.state, params(request)?).await
            }
            methods::MAILBOX_RENAME => {
                self.require_ui()?;
                folders::rename(&self.state, params(request)?).await
            }
            methods::MAILBOX_DELETE => {
                self.require_ui()?;
                folders::delete(&self.state, params(request)?).await
            }
            methods::MAILBOX_EMPTY => {
                self.require_ui()?;
                folders::empty(&self.state, params(request)?).await
            }
            methods::ACCOUNT_SET_LOGIN => {
                self.require_ui()?;
                accounts::set_login(&self.state, params(request)?).await
            }
            methods::ACCOUNT_SET_AUTO_COPY => {
                self.require_ui()?;
                accounts::set_auto_copy(&self.state, params(request)?).await
            }
            methods::ACCOUNT_DISCOVER => {
                self.require_ui()?;
                discover::discover(&self.state, params(request)?).await
            }
            methods::ACCOUNT_CHECK_SERVER => {
                self.require_ui()?;
                discover::check_server(&self.state, params(request)?).await
            }
            methods::ACCOUNT_OAUTH_PROVIDERS => {
                self.require_ui()?;
                Ok(oauth::providers(&self.state))
            }
            methods::ACCOUNT_OAUTH_START => {
                self.require_ui()?;
                oauth::start(&self.state, params(request)?).await
            }
            methods::ACCOUNT_OAUTH_FINISH => {
                self.require_ui()?;
                oauth::finish(&self.state, params(request)?).await
            }
            methods::ACCOUNT_OAUTH_CANCEL => {
                self.require_ui()?;
                Ok(oauth::cancel(&self.state, params(request)?))
            }
            methods::ACCOUNT_SYNC => {
                self.require_ui()?;
                accounts::sync_now(&self.state, params(request)?).await
            }
            methods::ACCOUNT_RESYNC => {
                self.require_ui()?;
                accounts::resync(&self.state, params(request)?).await
            }
            methods::MAILBOX_LIST => {
                let params: AccountRef = params(request)?;
                self.allow_account(Access::Read, &params.account_id).await?;
                accounts::mailboxes(&self.state, params).await
            }
            methods::MESSAGE_LIST => {
                let params: MessageListParams = params(request)?;
                self.allow_mailbox(Access::Read, params.mailbox_id).await?;
                mail::list(&self.state, params).await
            }
            methods::SEARCH => {
                let params: SearchParams = params(request)?;
                let scope = self.search_scope(params.account_id.as_deref()).await?;
                mail::search(&self.state, params, scope).await
            }
            methods::SEARCH_SERVER => {
                let params: SearchParams = params(request)?;
                let scope = self.search_scope(params.account_id.as_deref()).await?;
                mail::search_server(&self.state, params, scope).await
            }
            methods::MESSAGE_GET => {
                let params: MessageRef = params(request)?;
                self.allow_message(Access::Read, params.id).await?;
                let of = Some(context::Of::Message(params.id));
                let result = mail::get(&self.state, params).await?;
                Ok(self.with_context(result, of).await)
            }
            methods::MESSAGE_SOURCE => {
                let params: MessageRef = params(request)?;
                self.allow_message(Access::Read, params.id).await?;
                mail::source(&self.state, params).await
            }
            methods::ATTACHMENT_PREVIEW => {
                let params: AttachmentRef = params(request)?;
                self.allow_message(Access::Read, params.message_id).await?;
                attachments::preview(&self.state, params).await
            }
            methods::ATTACHMENT_SAVE => {
                self.require_ui()?;
                attachments::save(&self.state, params(request)?).await
            }
            methods::ATTACHMENT_SAVE_ALL => {
                self.require_ui()?;
                attachments::save_all(&self.state, params(request)?).await
            }
            methods::ATTACHMENT_OPEN => {
                self.require_ui()?;
                attachments::open(&self.state, params(request)?).await
            }
            methods::MESSAGE_MARK_ALL_READ => {
                self.require_ui()?;
                actions::mark_all_read(&self.state, params(request)?).await
            }
            methods::MESSAGE_HTML => {
                let params: MessageHtmlParams = params(request)?;
                self.allow_message(Access::Read, params.id).await?;
                mail::html(&self.state, params).await
            }
            methods::MESSAGE_DELETE => {
                let params: MessageIds = params(request)?;
                if self.is_ui() {
                    return actions::delete_forever(&self.state, params).await;
                }
                // Deleting for good always waits for the person, if it can be done.
                let account = self.allow_targets(Access::Triage, &params.ids).await?;
                access::deletable(&self.state, &params.ids).await?;
                Err(self
                    .hold_change(request, account, Reason::Delete, params.ids.len())
                    .await)
            }
            methods::DRAFT_CREATE => {
                let params: DraftCreateParams = params(request)?;
                self.allow_account(Access::Triage, &params.account_id).await?;
                let result = compose::create(&self.state, params).await?;
                let of = context::Of::draft_in(&result);
                Ok(self.with_context(result, of).await)
            }
            methods::DRAFT_REPLY => {
                let params: DraftReplyParams = params(request)?;
                self.allow_message(Access::Triage, params.message_id).await?;
                let result = compose::reply(&self.state, params).await?;
                let of = context::Of::draft_in(&result);
                Ok(self.with_context(result, of).await)
            }
            methods::DRAFT_FORWARD => {
                let params: DraftForwardParams = params(request)?;
                self.allow_message(Access::Triage, params.id).await?;
                let result = compose::forward(&self.state, params).await?;
                let of = context::Of::draft_in(&result);
                Ok(self.with_context(result, of).await)
            }
            methods::DRAFT_GET => {
                let params: DraftRef = params(request)?;
                self.allow_draft(Access::Triage, params.id).await?;
                let result = compose::get(&self.state, params).await?;
                let of = context::Of::draft_in(&result);
                Ok(self.with_context(result, of).await)
            }
            methods::DRAFT_CHECK => {
                let params: DraftRef = params(request)?;
                self.allow_draft(Access::Triage, params.id).await?;
                compose::check(&self.state, params).await
            }
            methods::DRAFT_LIST => {
                let params: DraftListParams = params(request)?;
                let scope = self.scope(Access::Triage, params.account_id.as_deref()).await?;
                compose::list(&self.state, params, scope).await
            }
            methods::DRAFT_UPDATE => {
                let params: DraftUpdateParams = params(request)?;
                self.allow_draft(Access::Triage, params.id).await?;
                let result = compose::update(&self.state, params).await?;
                let of = context::Of::draft_in(&result);
                Ok(self.with_context(result, of).await)
            }
            methods::DRAFT_DELETE => {
                let params: DraftRef = params(request)?;
                self.allow_draft(Access::Triage, params.id).await?;
                compose::delete(&self.state, params).await
            }
            methods::DRAFT_ATTACH_FILE => {
                self.require_ui()?;
                compose::attach_file(&self.state, params(request)?).await
            }
            methods::DRAFT_DETACH => {
                self.require_ui()?;
                compose::detach(&self.state, params(request)?).await
            }
            methods::DRAFT_ELSEWHERE => {
                self.require_ui()?;
                draft_sync::elsewhere(&self.state, params(request)?).await
            }
            methods::DRAFT_ADOPT => {
                self.require_ui()?;
                draft_sync::adopt(&self.state, params(request)?).await
            }
            methods::SAVED_SEARCH_LIST => {
                self.require_ui()?;
                saved::list(&self.state).await
            }
            methods::SAVED_SEARCH_SAVE => {
                self.require_ui()?;
                saved::save(&self.state, params(request)?).await
            }
            methods::SAVED_SEARCH_REORDER => {
                self.require_ui()?;
                saved::reorder(&self.state, params(request)?).await
            }
            methods::SAVED_SEARCH_REMOVE => {
                self.require_ui()?;
                saved::remove(&self.state, params(request)?).await
            }
            methods::TEMPLATE_LIST => {
                self.require_ui()?;
                templates::list(&self.state).await
            }
            methods::TEMPLATE_SAVE => {
                self.require_ui()?;
                templates::save(&self.state, params(request)?).await
            }
            methods::TEMPLATE_REMOVE => {
                self.require_ui()?;
                templates::remove(&self.state, params(request)?).await
            }
            methods::IDENTITY_LIST => {
                self.require_ui()?;
                identities::list(&self.state, params(request)?).await
            }
            methods::IDENTITY_SAVE => {
                self.require_ui()?;
                identities::save(&self.state, params(request)?).await
            }
            methods::IDENTITY_REMOVE => {
                self.require_ui()?;
                identities::remove(&self.state, params(request)?).await
            }
            methods::IDENTITY_MAKE_DEFAULT => {
                self.require_ui()?;
                identities::make_default(&self.state, params(request)?).await
            }
            methods::IDENTITY_SET_SIGNATURE => {
                self.require_ui()?;
                identities::set_signature(&self.state, params(request)?).await
            }
            methods::SIGNATURE_LIST => {
                self.require_ui()?;
                signatures::list(&self.state).await
            }
            methods::SIGNATURE_SAVE => {
                self.require_ui()?;
                signatures::save(&self.state, params(request)?).await
            }
            methods::SIGNATURE_REMOVE => {
                self.require_ui()?;
                signatures::remove(&self.state, params(request)?).await
            }
            methods::IDENTITY_SET_VOICE => {
                self.require_ui()?;
                voice::set(&self.state, params(request)?).await
            }
            methods::VOICE_SUGGEST => {
                let params: VoiceSuggestParams = params(request)?;
                if self.is_ui() {
                    return voice::apply(&self.state, params, None).await;
                }
                // Only the person changes a voice: an agent's suggestion waits (ADR-0020).
                let scope = self.scope(Access::Triage, params.account_id.as_deref()).await?;
                let about = voice::held(&self.state, &params, scope).await?;
                self.allow_account(Access::Triage, &about.account_id).await?;
                Err(holds::hold(&self.state, self.asker(request), about).await)
            }
            methods::NOTE_LIST => {
                let params: NoteListParams = params(request)?;
                if self.is_ui() {
                    return notes::list(&self.state, params, regards_store::notes::PER_PERSON_MAX).await;
                }
                self.allow_account(Access::Read, &params.account_id).await?;
                notes::require_on(&self.state).await?;
                notes::list(&self.state, params, notes::FOR_AGENTS).await
            }
            methods::NOTE_ADD => {
                let params: NoteAddParams = params(request)?;
                if self.is_ui() {
                    return notes::add(&self.state, params, Author::Person, None).await;
                }
                self.allow_account(Access::Triage, &params.account_id).await?;
                notes::require_on(&self.state).await?;
                let text = notes::checked(&params.text)?;
                // A note that could plant something for later waits for the person.
                if let Some(review) = regards_core::notes::needs_review(&text) {
                    let about = notes::held(&self.state, &params, review).await?;
                    return Err(holds::hold(&self.state, self.asker(request), about).await);
                }
                notes::add(&self.state, params, Author::Agent, self.agent_session.clone()).await
            }
            methods::NOTE_UPDATE => {
                self.require_ui()?;
                notes::update(&self.state, params(request)?).await
            }
            methods::NOTE_REMOVE => {
                let params: NoteRef = params(request)?;
                if !self.is_ui() {
                    let note = notes::find(&self.state, params.id).await?;
                    self.allow_within(Access::Triage, &note.account_id, "note").await?;
                    notes::require_on(&self.state).await?;
                    if note.author != Author::Agent {
                        return Err(RpcError::new(
                            codes::FORBIDDEN,
                            "agents can only remove notes an agent wrote; the person's own notes are theirs",
                        ));
                    }
                    self.note_before(serde_json::to_value(notes::view(note)).unwrap_or(Value::Null));
                }
                notes::remove(&self.state, params).await
            }
            methods::NOTE_SETTINGS => {
                self.require_ui()?;
                notes::settings(&self.state).await
            }
            methods::NOTE_SET_ENABLED => {
                self.require_ui()?;
                notes::set_enabled(&self.state, params(request)?).await
            }
            methods::NOTE_FORGET_ALL => {
                self.require_ui()?;
                notes::forget_all(&self.state).await
            }
            methods::MEMORY_GET => {
                let params: MemoryParams = params(request)?;
                let scope = self.scope(Access::Read, params.account_id.as_deref()).await?;
                if scope.as_ref().is_some_and(Vec::is_empty) {
                    return Err(RpcError::new(codes::NOT_FOUND, "no account is open to agents"));
                }
                notes::memory(&self.state, params, scope).await
            }
            methods::DRAFT_ATTACH_IMAGE => {
                self.require_ui()?;
                compose::attach_image(&self.state, params(request)?).await
            }
            methods::DRAFT_IMAGES => {
                self.require_ui()?;
                compose::images(&self.state, params(request)?).await
            }
            methods::DRAFT_SET_FROM => {
                let params: DraftSetFromParams = params(request)?;
                self.allow_draft(Access::Triage, params.id).await?;
                // Another account's address moves the draft there: that account too.
                if let Some(account) = compose::identity_account(&self.state, params.identity_id).await? {
                    self.allow_account(Access::Triage, &account).await?;
                }
                compose::set_from(&self.state, params).await
            }
            methods::DRAFT_SEND => {
                let params: DraftSendParams = params(request)?;
                if self.is_ui() {
                    return compose::send(&self.state, params).await;
                }
                // An agent's send waits for the person (ADR-0012 §3), unless its
                // account lets it go after an undo window.
                self.allow_draft(Access::Send, params.id).await?;
                if let Some(window) = compose::agent_send_window(&self.state, params.id).await? {
                    return compose::send_for_agent(&self.state, params, window).await;
                }
                Err(holds::hold_send(&self.state, self.asker(request), &params).await)
            }
            methods::OUTBOX_LIST => {
                self.require_ui()?;
                compose::outbox_list(&self.state).await
            }
            methods::OUTBOX_UNDO => {
                self.require_ui()?;
                compose::undo(&self.state, params(request)?).await
            }
            methods::NOTICE_LIST => {
                self.require_ui()?;
                notices::list(&self.state).await
            }
            methods::NOTICE_SHOWN => {
                self.require_ui()?;
                notices::shown(&self.state, params(request)?).await
            }
            methods::PEOPLE_SUGGEST => {
                let params: PeopleSuggestParams = params(request)?;
                let scope = self.search_scope(params.account_id.as_deref()).await?;
                people::suggest(&self.state, params, scope).await
            }
            methods::PEOPLE_FORGET => {
                self.require_ui()?;
                people::forget(&self.state, params(request)?).await
            }
            methods::READING_SETTINGS => {
                self.require_ui()?;
                mail::reading_settings(&self.state).await
            }
            methods::READING_SET_SETTINGS => {
                self.require_ui()?;
                mail::set_reading_settings(&self.state, params(request)?).await
            }
            methods::ASSISTANT_SETTINGS => {
                self.require_ui()?;
                assistant::settings(&self.state).await
            }
            methods::ASSISTANT_SET_SETTINGS => {
                self.require_ui()?;
                assistant::set_settings(&self.state, params(request)?).await
            }
            methods::ASSISTANT_PREVIEW => {
                self.require_ui()?;
                assistant::preview(&self.state, params(request)?).await
            }
            methods::ASSISTANT_RUN => {
                self.require_ui()?;
                assistant::run(&self.state, params(request)?).await
            }
            methods::CONNECTION_LOG_GET => {
                self.require_ui()?;
                Ok(mail::connection_log(&self.state))
            }
            methods::CONNECTION_LOG_SET => {
                self.require_ui()?;
                mail::set_connection_log(&self.state, params(request)?).await
            }
            methods::PREFETCH_GET => {
                self.require_ui()?;
                prefetch::get(&self.state).await
            }
            methods::PREFETCH_SET => {
                self.require_ui()?;
                prefetch::set(&self.state, params(request)?).await
            }
            methods::MAIL_ARRIVED => {
                self.require_ui()?;
                notices::arrived(&self.state).await
            }
            methods::MAIL_ARRIVED_SHOWN => {
                self.require_ui()?;
                notices::arrived_shown(&self.state, params(request)?).await
            }
            methods::OUTBOX_RESCHEDULE => {
                self.require_ui()?;
                compose::reschedule(&self.state, params(request)?).await
            }
            methods::OUTBOX_SEND_AGAIN => {
                self.require_ui()?;
                compose::send_again(&self.state, params(request)?).await
            }
            methods::OUTBOX_DISMISS => {
                self.require_ui()?;
                compose::dismiss(&self.state, params(request)?).await
            }
            methods::MESSAGE_PIN => {
                let params: PinParams = params(request)?;
                self.allow_change(Access::Triage, &params.ids, request).await?;
                actions::pin(&self.state, params).await
            }
            methods::MESSAGE_SET_ASIDE => {
                let params: AsideParams = params(request)?;
                self.allow_change(Access::Triage, &params.ids, request).await?;
                actions::set_aside(&self.state, params).await
            }
            methods::SMART_OVERVIEW => {
                let params: AccountRef = params(request)?;
                self.allow_account(Access::Read, &params.account_id).await?;
                smart::overview(&self.state, params).await
            }
            methods::MESSAGE_EXPLAIN => {
                let params: MessageRef = params(request)?;
                self.allow_message(Access::Read, params.id).await?;
                smart::explain(&self.state, params).await
            }
            methods::SENDER_SET_IMAGES => {
                self.require_ui()?;
                mail::set_sender_images(&self.state, params(request)?).await
            }
            methods::SENDER_IMAGES => {
                self.require_ui()?;
                mail::sender_images_list(&self.state, params(request)?).await
            }
            methods::SENDER_BLOCK => {
                let params: SenderBlockParams = params(request)?;
                self.allow_sender(&params.account_id, &params.address).await?;
                actions::block(&self.state, params).await
            }
            methods::SMART_SCREENING => {
                self.require_ui()?;
                smart::screening(&self.state).await
            }
            methods::SMART_SET_SCREENING => {
                self.require_ui()?;
                smart::set_screening(&self.state, params(request)?).await
            }
            methods::SENDER_SET => {
                let params: SenderSetParams = params(request)?;
                self.allow_sender(&params.account_id, &params.address).await?;
                smart::set_sender(&self.state, params).await
            }
            methods::MESSAGE_REMIND => {
                let params: RemindParams = params(request)?;
                self.allow_change(Access::Triage, &params.ids, request).await?;
                actions::remind(&self.state, params).await
            }
            methods::MESSAGE_UNSUBSCRIBE => {
                self.require_ui()?;
                unsubscribe::unsubscribe(&self.state, params(request)?).await
            }
            methods::MESSAGE_SNOOZE => {
                let params: SnoozeParams = params(request)?;
                self.allow_change(Access::Triage, &params.ids, request).await?;
                actions::snooze(&self.state, params).await
            }
            methods::MESSAGE_VIEW => {
                let params: ViewParams = params(request)?;
                self.allow_account(Access::Read, &params.account_id).await?;
                mail::view(&self.state, params).await
            }
            methods::MESSAGE_CONVERSATION_IDS => {
                let params: MessageIds = params(request)?;
                if !self.is_ui() {
                    self.allow_targets(Access::Read, &params.ids).await?;
                }
                mail::conversation_ids(&self.state, params).await
            }
            methods::MESSAGE_LABELS => {
                let params: MessageIds = params(request)?;
                if !self.is_ui() {
                    self.allow_targets(Access::Read, &params.ids).await?;
                }
                labels::labels(&self.state, params).await
            }
            methods::MESSAGE_SET_LABELS => {
                self.require_ui()?;
                labels::set(&self.state, params(request)?).await
            }
            methods::THREAD_GET => {
                let params: MessageRef = params(request)?;
                self.allow_message(Access::Read, params.id).await?;
                let of = Some(context::Of::Thread(params.id));
                let result = mail::thread(&self.state, params).await?;
                Ok(self.with_context(result, of).await)
            }
            methods::MESSAGE_SET_FLAGS => {
                let params: SetFlagsParams = params(request)?;
                self.allow_change(Access::Triage, &params.ids, request).await?;
                actions::set_flags(&self.state, params).await
            }
            methods::MESSAGE_MOVE => {
                let params: MoveParams = params(request)?;
                self.allow_change(Access::Triage, &params.ids, request).await?;
                self.allow_mailbox(Access::Triage, params.mailbox_id).await?;
                actions::move_to(&self.state, params).await
            }
            methods::MESSAGE_ARCHIVE => {
                let params: MessageIds = params(request)?;
                self.allow_change(Access::Triage, &params.ids, request).await?;
                actions::archive(&self.state, params).await
            }
            methods::MESSAGE_TRASH => {
                let params: MessageIds = params(request)?;
                self.allow_change(Access::Triage, &params.ids, request).await?;
                actions::trash(&self.state, params).await
            }
            methods::UNDO_APPLY => {
                self.require_ui()?;
                actions::apply_undo(&self.state, params(request)?).await
            }
            methods::CHANGES_SUBSCRIBE => {
                self.require_ui()?;
                let mut feed = self.state.feed.subscribe();
                let current = *feed.borrow_and_update();
                self.feed = Some(feed);
                Ok(to_value(current))
            }
            other => Err(RpcError::new(
                codes::METHOD_NOT_FOUND,
                format!("unknown method \"{other}\""),
            )),
        }
    }

    fn is_ui(&self) -> bool {
        self.principal == Some(Principal::Ui)
    }

    /// For an agent: the result with its context, the voice and the people
    /// in it (ADR-0020 §4). The app shows those its own way.
    async fn with_context(&self, result: Value, of: Option<context::Of>) -> Value {
        if self.is_ui() {
            return result;
        }
        context::attach(&self.state, result, of).await
    }

    /// The app may; an agent, if the account allows it `needed` (ADR-0012).
    async fn allow_account(&self, needed: Access, account_id: &str) -> Result<(), RpcError> {
        if self.is_ui() {
            return Ok(());
        }
        let (email, current) = access::current(&self.state, account_id).await?;
        *self.checked.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((account_id.to_owned(), current));
        access::permit(needed, account_id, &email, current)
    }

    /// [`allow_account`](Self::allow_account) for something of the account
    /// (`what`: "message", "mailbox", "draft"). In an account closed to
    /// agents it is not there for them: they cannot tell it from nothing.
    async fn allow_within(&self, needed: Access, account_id: &str, what: &str) -> Result<(), RpcError> {
        let (_, current) = access::current(&self.state, account_id).await?;
        if current == Access::Off {
            *self.checked.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
                Some((account_id.to_owned(), current));
            return Err(RpcError::new(codes::NOT_FOUND, format!("no such {what}")));
        }
        self.allow_account(needed, account_id).await
    }

    async fn allow_mailbox(&self, needed: Access, mailbox_id: i64) -> Result<(), RpcError> {
        if self.is_ui() {
            return Ok(());
        }
        let account = access::account_of_mailbox(&self.state, mailbox_id).await?;
        self.allow_within(needed, &account, "mailbox").await
    }

    async fn allow_message(&self, needed: Access, message_id: i64) -> Result<(), RpcError> {
        if self.is_ui() {
            return Ok(());
        }
        let account = access::account_of_message(&self.state, message_id).await?;
        self.allow_within(needed, &account, "message").await
    }

    /// The accounts a search may look in: for the app, those asked for; for
    /// an agent, those of them open to it.
    async fn search_scope(&self, account_id: Option<&str>) -> Result<Option<Vec<String>>, RpcError> {
        self.scope(Access::Read, account_id).await
    }

    /// The accounts a request about many may cover: for the app, those asked
    /// for (`None`: all); for an agent, those of them that allow `needed`.
    async fn scope(&self, needed: Access, account_id: Option<&str>) -> Result<Option<Vec<String>>, RpcError> {
        if self.is_ui() {
            return Ok(None);
        }
        match account_id {
            Some(account) => {
                self.allow_account(needed, account).await?;
                Ok(Some(vec![account.to_owned()]))
            }
            None => access::open_to_agents(&self.state, needed).await.map(Some),
        }
    }

    /// An agent may change these messages: each one's account allows
    /// `needed`, and they are not too many at once (ADR-0012 §3).
    /// More than that is a bulk change, which waits for the person.
    async fn allow_change(&self, needed: Access, ids: &[i64], request: &Request) -> Result<(), RpcError> {
        if self.is_ui() {
            return Ok(());
        }
        let account = self.allow_targets(needed, ids).await?;
        if ids.len() > MAX_AGENT_CHANGES {
            return Err(self.hold_change(request, account, Reason::Bulk, ids.len()).await);
        }
        Ok(())
    }

    /// Each message's account allows an agent `needed`; the first account.
    async fn allow_targets(&self, needed: Access, ids: &[i64]) -> Result<String, RpcError> {
        if ids.is_empty() {
            return Err(RpcError::new(codes::INVALID_PARAMS, "give at least one message id"));
        }
        let (accounts, before) = access::messages_before(&self.state, ids).await?;
        self.note_before(before);
        for account in &accounts {
            self.allow_within(needed, account, "message").await?;
        }
        Ok(accounts.into_iter().next().expect("one account per message"))
    }

    /// Hold an agent's change to messages for the person (ADR-0012 §3).
    async fn hold_change(&self, request: &Request, account_id: String, reason: Reason, count: usize) -> RpcError {
        let description = holds::describe(&request.method, count);
        let about = holds::About {
            account_id,
            reason,
            description: description.clone(),
            draft: None,
            people: Vec::new(),
            subject: description,
        };
        holds::hold(&self.state, self.asker(request), about).await
    }

    /// The agent request at hand, as a hold keeps it.
    fn asker<'a>(&'a self, request: &'a Request) -> holds::Asker<'a> {
        holds::Asker {
            session: self.agent_session.as_deref().unwrap_or_default(),
            client: self.client.map_or("agent", audit::client_name),
            method: &request.method,
            params: &request.params,
        }
    }

    /// An agent may touch a draft if its account allows `needed`.
    async fn allow_draft(&self, needed: Access, draft_id: i64) -> Result<(), RpcError> {
        if self.is_ui() {
            return Ok(());
        }
        let (account, before) = access::draft_before(&self.state, draft_id).await?;
        self.note_before(before);
        self.allow_within(needed, &account, "draft").await
    }

    /// An agent may sort a sender if the account allows triage.
    async fn allow_sender(&self, account_id: &str, address: &str) -> Result<(), RpcError> {
        if self.is_ui() {
            return Ok(());
        }
        self.allow_account(Access::Triage, account_id).await?;
        let before = access::sender_before(&self.state, account_id, address).await?;
        self.note_before(before);
        Ok(())
    }

    fn note_before(&self, before: Value) {
        *self.before.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(before);
    }

    /// Accounts, credentials and access levels may only be changed from the desktop UI (ADR-0006).
    fn require_ui(&self) -> Result<(), RpcError> {
        match self.principal {
            Some(Principal::Ui) => Ok(()),
            _ => Err(RpcError::new(
                codes::FORBIDDEN,
                "only the Regards app can do this; agents and the CLI cannot",
            )),
        }
    }

    fn hello(&mut self, params: &Value) -> Result<Value, RpcError> {
        if self.principal.is_some() {
            return Err(RpcError::new(
                codes::INVALID_REQUEST,
                "the handshake was already completed",
            ));
        }
        let params: HelloParams = serde_json::from_value(params.clone())
            .map_err(|error| RpcError::new(codes::INVALID_PARAMS, format!("invalid hello parameters: {error}")))?;
        let Some(protocol_version) = negotiate(&params.protocol_versions) else {
            let mut error = RpcError::new(
                codes::UNSUPPORTED_PROTOCOL,
                "no protocol version in common; update Regards",
            );
            error.data = Some(json!({ "supported": PROTOCOL_VERSIONS }));
            return Err(error);
        };
        if params.build_id != BUILD_ID {
            tracing::info!(client_build = %params.build_id, "client from a different build connected");
        }

        // Only the desktop bridge holds the per-launch secret (ADR-0006). A wrong
        // token is refused instead of downgraded, so a stale desktop notices
        // that the daemon was replaced.
        let principal = match (&params.ui_token, &self.state.ui_token) {
            (None, _) => Principal::Agent,
            (Some(offered), Some(expected)) if expected.matches(offered) => Principal::Ui,
            (Some(_), _) => {
                tracing::warn!(client_kind = ?params.client_kind, "rejected a connection with an unknown desktop token");
                return Err(RpcError::new(
                    codes::FORBIDDEN,
                    "the desktop access token was not accepted",
                ));
            }
        };
        self.principal = Some(principal);
        self.client = Some(params.client_kind);
        if principal == Principal::Agent {
            self.agent_session = Some(regards_store::accounts::new_id());
            self.app = params
                .app
                .as_deref()
                .and_then(|app| agents::Gate::seen(&self.state, app));
        }
        tracing::debug!(client_kind = ?params.client_kind, ?principal, "handshake completed");

        Ok(to_value(HelloResult {
            protocol_version,
            daemon_version: VERSION.to_owned(),
            daemon_build_id: BUILD_ID.to_owned(),
            principal,
        }))
    }
}

fn to_value<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).expect("protocol types always serialize")
}

/// An internal failure, reported without details that could leak data.
pub(crate) fn internal(error: impl std::fmt::Display) -> RpcError {
    tracing::error!(%error, "request failed");
    RpcError::new(codes::INTERNAL_ERROR, "internal error; see the daemon log")
}

/// Decode a request's parameters.
fn params<T: serde::de::DeserializeOwned>(request: &Request) -> Result<T, RpcError> {
    serde_json::from_value(request.params.clone())
        .map_err(|error| RpcError::new(codes::INVALID_PARAMS, format!("invalid parameters: {error}")))
}
