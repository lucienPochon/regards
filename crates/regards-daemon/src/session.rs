//! One client connection: handshake, then request dispatch.

use crate::{
    State, access, accounts, actions, audit, compose, draft_sync, identities, mail, notices, smart, templates,
};
use regards_core::agents::Access;
use regards_core::build_info::{BUILD_ID, VERSION};
use regards_ipc::framing::{self, FrameError};
use regards_ipc::protocol::{
    AccountRef, ChangesNotification, ClientKind, HealthResult, HelloParams, HelloResult, JSONRPC, MessageHtmlParams,
    MessageListParams, MessageRef, Notification, PROTOCOL_VERSIONS, Principal, Request, Response, RpcError,
    SearchParams, ViewParams, codes, methods, negotiate, notifications,
};
use regards_ipc::transport::Stream;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

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
    /// Asked to stop the daemon: done once the reply is sent.
    stop_after_reply: bool,
    /// Set by `changes.subscribe`: changes to forward as notifications.
    feed: Option<watch::Receiver<ChangesNotification>>,
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
        stop_after_reply: false,
        feed: None,
    };

    loop {
        let incoming = if let Some(feed) = session.feed.as_mut() {
            let next = tokio::select! {
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
            framing::recv::<_, Value>(&mut framed).await
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
        let outcome = self.dispatch(&request).await;
        // Everything an agent asks, but the handshake, is kept (ADR-0012 §4).
        if let (Some(session), Some(client)) = (&self.agent_session, self.client)
            && request.method != methods::HELLO
        {
            let checked = self
                .checked
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            let answered = audit::Answered {
                session,
                client,
                method: &request.method,
                params: &request.params,
                checked,
                error: outcome.as_ref().err(),
                duration: started.elapsed(),
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
        match request.method.as_str() {
            methods::DAEMON_HEALTH => Ok(to_value(HealthResult {
                status: "ok".to_owned(),
                version: VERSION.to_owned(),
                build_id: BUILD_ID.to_owned(),
                pid: std::process::id(),
                uptime_secs: self.state.started.elapsed().as_secs(),
                revision: self.state.store.revision().await.map_err(internal)?,
            })),
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
            methods::AGENT_ACTIVITY => {
                self.require_ui()?;
                audit::activity(&self.state, params(request)?).await
            }
            methods::ACCOUNT_ADD => {
                self.require_ui()?;
                accounts::add(&self.state, params(request)?).await
            }
            methods::ACCOUNT_REMOVE => {
                self.require_ui()?;
                accounts::remove(&self.state, params(request)?).await
            }
            methods::ACCOUNT_SYNC => {
                self.require_ui()?;
                accounts::sync_now(&self.state, params(request)?).await
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
                mail::get(&self.state, params).await
            }
            methods::MESSAGE_HTML => {
                let params: MessageHtmlParams = params(request)?;
                self.allow_message(Access::Read, params.id).await?;
                mail::html(&self.state, params).await
            }
            methods::MESSAGE_DELETE => {
                self.require_ui()?;
                actions::delete_forever(&self.state, params(request)?).await
            }
            methods::DRAFT_CREATE => {
                self.require_ui()?;
                compose::create(&self.state, params(request)?).await
            }
            methods::DRAFT_REPLY => {
                self.require_ui()?;
                compose::reply(&self.state, params(request)?).await
            }
            methods::DRAFT_FORWARD => {
                self.require_ui()?;
                compose::forward(&self.state, params(request)?).await
            }
            methods::DRAFT_GET => {
                self.require_ui()?;
                compose::get(&self.state, params(request)?).await
            }
            methods::DRAFT_LIST => {
                self.require_ui()?;
                compose::list(&self.state, params(request)?).await
            }
            methods::DRAFT_UPDATE => {
                self.require_ui()?;
                compose::update(&self.state, params(request)?).await
            }
            methods::DRAFT_DELETE => {
                self.require_ui()?;
                compose::delete(&self.state, params(request)?).await
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
            methods::DRAFT_SET_FROM => {
                self.require_ui()?;
                compose::set_from(&self.state, params(request)?).await
            }
            methods::DRAFT_SEND => {
                self.require_ui()?;
                compose::send(&self.state, params(request)?).await
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
                self.require_ui()?;
                actions::pin(&self.state, params(request)?).await
            }
            methods::MESSAGE_SET_ASIDE => {
                self.require_ui()?;
                actions::set_aside(&self.state, params(request)?).await
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
            methods::SENDER_BLOCK => {
                self.require_ui()?;
                actions::block(&self.state, params(request)?).await
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
                self.require_ui()?;
                smart::set_sender(&self.state, params(request)?).await
            }
            methods::MESSAGE_REMIND => {
                self.require_ui()?;
                actions::remind(&self.state, params(request)?).await
            }
            methods::MESSAGE_SNOOZE => {
                self.require_ui()?;
                actions::snooze(&self.state, params(request)?).await
            }
            methods::MESSAGE_VIEW => {
                let params: ViewParams = params(request)?;
                self.allow_account(Access::Read, &params.account_id).await?;
                mail::view(&self.state, params).await
            }
            methods::THREAD_GET => {
                let params: MessageRef = params(request)?;
                self.allow_message(Access::Read, params.id).await?;
                mail::thread(&self.state, params).await
            }
            methods::MESSAGE_SET_FLAGS => {
                self.require_ui()?;
                actions::set_flags(&self.state, params(request)?).await
            }
            methods::MESSAGE_MOVE => {
                self.require_ui()?;
                actions::move_to(&self.state, params(request)?).await
            }
            methods::MESSAGE_ARCHIVE => {
                self.require_ui()?;
                actions::archive(&self.state, params(request)?).await
            }
            methods::MESSAGE_TRASH => {
                self.require_ui()?;
                actions::trash(&self.state, params(request)?).await
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

    /// The app may; an agent, if the account allows it `needed` (ADR-0012).
    async fn allow_account(&self, needed: Access, account_id: &str) -> Result<(), RpcError> {
        if self.is_ui() {
            return Ok(());
        }
        let (email, current) = access::current(&self.state, account_id).await?;
        *self.checked.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((account_id.to_owned(), current));
        access::permit(needed, account_id, &email, current)
    }

    async fn allow_mailbox(&self, needed: Access, mailbox_id: i64) -> Result<(), RpcError> {
        if self.is_ui() {
            return Ok(());
        }
        let account = access::account_of_mailbox(&self.state, mailbox_id).await?;
        self.allow_account(needed, &account).await
    }

    async fn allow_message(&self, needed: Access, message_id: i64) -> Result<(), RpcError> {
        if self.is_ui() {
            return Ok(());
        }
        let account = access::account_of_message(&self.state, message_id).await?;
        self.allow_account(needed, &account).await
    }

    /// The accounts a search may look in: for the app, those asked for; for
    /// an agent, those of them open to it.
    async fn search_scope(&self, account_id: Option<&str>) -> Result<Option<Vec<String>>, RpcError> {
        if self.is_ui() {
            return Ok(None);
        }
        match account_id {
            Some(account) => {
                self.allow_account(Access::Read, account).await?;
                Ok(Some(vec![account.to_owned()]))
            }
            None => access::open_to_agents(&self.state, Access::Read).await.map(Some),
        }
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
