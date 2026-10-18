//! Regards core library.
//!
//! Holds what every Regards component shares: product identity, the build
//! identity used by the client/daemon handshake (ADR-0005), and the per-user
//! data locations.

pub mod body;
pub mod build_info;
pub mod headers;
pub mod mailbox;
pub mod paths;
pub mod secret;

/// Human-readable product name.
pub const PRODUCT_NAME: &str = "Regards";
