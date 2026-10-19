//! Regards core library.
//!
//! Holds what every Regards component shares: product identity, the build
//! identity used by the client/daemon handshake (ADR-0005), and the per-user
//! data locations.

pub mod agents;
pub mod body;
pub mod build_info;
pub mod compose;
pub mod encoding;
pub mod headers;
pub mod html;
pub mod mailbox;
pub mod paths;
pub mod search;
pub mod secret;
pub mod smart;

/// Human-readable product name.
pub const PRODUCT_NAME: &str = "Regards";
