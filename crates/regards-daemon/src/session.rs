//! One client connection: handshake, then request dispatch.

use crate::{State, accounts, actions, mail};
use regards_core::build_info::{BUILD_ID, VERSION};
use regards_ipc::framing::{self, FrameError};
use regards_ipc::protocol::{
    ChangesNotification, HealthResult, HelloParams, HelloResult, JSONRPC, Notification, PROTOCOL_VERSIONS, Principal,
    Request, Response, RpcError, codes, methods, negotiate, notifications,
};
use regards_ipc::transport::Stream;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// A client must say `hello` within this time or the connection is closed.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

struct Session {
    state: Arc<State>,
    principal: Option<Principal>,
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
        match self.dispatch(&request).await {
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
                let _ = self.state.stop.send(true);
                Ok(json!({ "stopping": true }))
            }
            methods::ACCOUNT_LIST => {
                self.require_ui()?;
                accounts::list(&self.state).await
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
                self.require_ui()?;
                accounts::mailboxes(&self.state, params(request)?).await
            }
            methods::MESSAGE_LIST => {
                self.require_ui()?;
                mail::list(&self.state, params(request)?).await
            }
            methods::MESSAGE_GET => {
                self.require_ui()?;
                mail::get(&self.state, params(request)?).await
            }
            methods::MESSAGE_HTML => {
                self.require_ui()?;
                mail::html(&self.state, params(request)?).await
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
