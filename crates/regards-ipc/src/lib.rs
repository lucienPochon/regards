//! Local IPC between Regards clients (desktop app, CLI, MCP server) and the
//! Regards daemon (ADR-0006).
//!
//! - **Transport:** a per-user Windows named pipe restricted to the current
//!   user, or a Unix domain socket in a `0700` directory with a peer-UID check.
//!   There is no TCP listener.
//! - **Framing:** 4-byte big-endian length prefix + one JSON document.
//! - **Protocol:** JSON-RPC 2.0. The first request on every connection must be
//!   [`methods::HELLO`], which negotiates the protocol version and exchanges
//!   build identities.

pub mod client;
pub mod endpoint;
pub mod framing;
pub mod protocol;
pub mod transport;

pub use client::{Client, ClientError};
pub use endpoint::Endpoint;
