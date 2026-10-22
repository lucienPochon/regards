//! Client used by the CLI, the MCP server and the desktop bridge.

use crate::endpoint::Endpoint;
use crate::framing::{self, FrameError};
use crate::protocol::{
    ChangesNotification, ClientKind, HealthResult, HelloParams, HelloResult, PROTOCOL_VERSIONS, Principal, Request,
    RpcError, ServerMessage, methods, notifications,
};
use crate::token::UiToken;
use crate::transport::{self, Stream};
use regards_core::build_info::BUILD_ID;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

/// Errors talking to the daemon.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("the Regards daemon is not running ({endpoint})")]
    NotRunning { endpoint: String },
    #[error("could not reach the Regards daemon: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error("the daemon closed the connection")]
    Closed,
    #[error("daemon error: {0}")]
    Rpc(#[from] RpcError),
    #[error("unexpected reply from the daemon: {0}")]
    Protocol(String),
}

/// A handshaken connection to the daemon.
pub struct Client {
    connection: Connection,
    hello: HelloResult,
}

impl Client {
    /// Connect and perform the `hello` handshake as an `agent` connection.
    pub async fn connect(endpoint: &Endpoint, kind: ClientKind) -> Result<Self, ClientError> {
        Self::connect_with(endpoint, kind, None).await
    }

    /// Connect, presenting the desktop bridge's token when there is one.
    ///
    /// A token the daemon does not recognise fails with a [`codes::FORBIDDEN`]
    /// RPC error rather than silently downgrading the connection.
    ///
    /// [`codes::FORBIDDEN`]: crate::protocol::codes::FORBIDDEN
    pub async fn connect_with(
        endpoint: &Endpoint,
        kind: ClientKind,
        ui_token: Option<&UiToken>,
    ) -> Result<Self, ClientError> {
        Self::connect_as(endpoint, kind, ui_token, None).await
    }

    /// [`connect_with`](Self::connect_with), for an agent saying which app it
    /// runs in (`app`, as that app names itself).
    pub async fn connect_as(
        endpoint: &Endpoint,
        kind: ClientKind,
        ui_token: Option<&UiToken>,
        app: Option<&str>,
    ) -> Result<Self, ClientError> {
        let stream = transport::connect(endpoint).await.map_err(|error| {
            if transport::is_not_running(&error) {
                ClientError::NotRunning {
                    endpoint: endpoint.to_string(),
                }
            } else {
                ClientError::Io(error)
            }
        })?;
        let mut connection = Connection {
            framed: framing::framed(stream),
            next_id: 1,
        };
        let hello = connection
            .call(
                methods::HELLO,
                HelloParams {
                    protocol_versions: PROTOCOL_VERSIONS.to_vec(),
                    build_id: BUILD_ID.to_owned(),
                    client_kind: kind,
                    ui_token: ui_token.cloned(),
                    app: app.map(str::to_owned),
                },
            )
            .await?;
        Ok(Self { connection, hello })
    }

    /// What the daemon reported during the handshake.
    pub fn hello(&self) -> &HelloResult {
        &self.hello
    }

    /// Whether the daemon was built from the same source as this client.
    pub fn is_same_build(&self) -> bool {
        self.hello.daemon_build_id == BUILD_ID
    }

    /// The principal the daemon assigned to this connection.
    pub fn principal(&self) -> Principal {
        self.hello.principal
    }

    /// Call a method and decode its result.
    pub async fn call<P: Serialize, R: DeserializeOwned>(&mut self, method: &str, params: P) -> Result<R, ClientError> {
        self.connection.call(method, params).await
    }

    pub async fn health(&mut self) -> Result<HealthResult, ClientError> {
        self.call(methods::DAEMON_HEALTH, Value::Null).await
    }

    /// Turn this connection into a change feed (ADR-0007). The connection is
    /// then used for notifications only; open another one for requests.
    pub async fn subscribe_changes(mut self) -> Result<ChangeFeed, ClientError> {
        let current: ChangesNotification = self.call(methods::CHANGES_SUBSCRIBE, Value::Null).await?;
        Ok(ChangeFeed {
            connection: self.connection,
            current,
        })
    }

    /// Ask the daemon to stop. Returns once the daemon has acknowledged.
    pub async fn shutdown(&mut self) -> Result<(), ClientError> {
        let _: Value = self.call(methods::DAEMON_SHUTDOWN, Value::Null).await?;
        Ok(())
    }
}

struct Connection {
    framed: Framed<Stream, LengthDelimitedCodec>,
    next_id: u64,
}

impl Connection {
    async fn call<P: Serialize, R: DeserializeOwned>(&mut self, method: &str, params: P) -> Result<R, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        let params = serde_json::to_value(params).map_err(FrameError::from)?;
        framing::send(&mut self.framed, &Request::new(id, method, params)).await?;

        loop {
            let message: ServerMessage = match framing::recv(&mut self.framed).await {
                None => return Err(ClientError::Closed),
                Some(message) => message?,
            };
            match message {
                // Change-feed notifications are consumed by subscribers (ADR-0007), not by calls.
                ServerMessage::Notification(_) => {}
                ServerMessage::Response(response) if response.id == Some(id) || response.id.is_none() => {
                    if let Some(error) = response.error {
                        return Err(error.into());
                    }
                    let result = response.result.unwrap_or(Value::Null);
                    return serde_json::from_value(result).map_err(|error| ClientError::Protocol(error.to_string()));
                }
                ServerMessage::Response(_) => {}
            }
        }
    }
}

/// Change notifications from the daemon (ADR-0007).
pub struct ChangeFeed {
    connection: Connection,
    current: ChangesNotification,
}

impl ChangeFeed {
    /// The state when the subscription started, or the latest one seen.
    pub fn current(&self) -> ChangesNotification {
        self.current
    }

    /// Wait for the next change. Several quick changes may arrive as one.
    pub async fn next(&mut self) -> Result<ChangesNotification, ClientError> {
        loop {
            let message: ServerMessage = match framing::recv(&mut self.connection.framed).await {
                None => return Err(ClientError::Closed),
                Some(message) => message?,
            };
            if let ServerMessage::Notification(notification) = message
                && notification.method == notifications::CHANGES
            {
                let change: ChangesNotification = serde_json::from_value(notification.params)
                    .map_err(|error| ClientError::Protocol(error.to_string()))?;
                self.current = change;
                return Ok(change);
            }
        }
    }
}
