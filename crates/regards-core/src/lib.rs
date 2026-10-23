//! Regards core library.
//!
//! Holds what every Regards component shares: product identity, the build
//! identity used by the client/daemon handshake (ADR-0005), and the per-user
//! data locations.

pub mod agents;
pub mod assistant;
pub mod attachment;
pub mod autoconfig;
pub mod body;
pub mod build_info;
pub mod compose;
pub mod encoding;
pub mod executor;
pub mod headers;
pub mod html;
pub mod mailbox;
pub mod notes;
pub mod oauth;
pub mod paths;
pub mod search;
pub mod secret;
pub mod smart;
pub mod unsubscribe;

/// Human-readable product name.
pub const PRODUCT_NAME: &str = "Regards";
