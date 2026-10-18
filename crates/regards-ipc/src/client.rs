//! Client used by the CLI, the MCP server and the desktop bridge.

use crate::endpoint::Endpoint;
use crate::framing::{self, FrameError};
use crate::protocol::{
    ClientKind, HealthResult, HelloParams, HelloResult, PROTOCOL_VERSIONS, Request, RpcError, ServerMessage, methods,
};
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
    /// Connect and perform the `hello` handshake.
    pub async fn connect(endpoint: &Endpoint, kind: ClientKind) -> Result<Self, ClientError> {
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

    /// Call a method and decode its result.
    pub async fn call<P: Serialize, R: DeserializeOwned>(&mut self, method: &str, params: P) -> Result<R, ClientError> {
        self.connection.call(method, params).await
    }

    pub async fn health(&mut self) -> Result<HealthResult, ClientError> {
        self.call(methods::DAEMON_HEALTH, Value::Null).await
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
