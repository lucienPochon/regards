//! One client connection: handshake, then request dispatch.

use crate::State;
use regards_core::build_info::{BUILD_ID, VERSION};
use regards_ipc::framing::{self, FrameError};
use regards_ipc::protocol::{
    HealthResult, HelloParams, HelloResult, PROTOCOL_VERSIONS, Principal, Request, Response, RpcError, codes, methods,
    negotiate,
};
use regards_ipc::transport::Stream;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// A client must say `hello` within this time or the connection is closed.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

struct Session {
    state: Arc<State>,
    principal: Option<Principal>,
}

pub(crate) async fn run(stream: Stream, state: Arc<State>) -> Result<(), FrameError> {
    let mut framed = framing::framed(stream);
    let mut session = Session { state, principal: None };

    loop {
        let incoming = if session.principal.is_none() {
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
            other => Err(RpcError::new(
                codes::METHOD_NOT_FOUND,
                format!("unknown method \"{other}\""),
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
fn internal(error: impl std::fmt::Display) -> RpcError {
    tracing::error!(%error, "request failed");
    RpcError::new(codes::INTERNAL_ERROR, "internal error; see the daemon log")
}
