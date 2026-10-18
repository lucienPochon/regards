//! Version and build identity.
//!
//! Clients and the daemon exchange their build identity on connect (ADR-0005).
//! A client that finds a daemon from a different build asks it to stop and
//! starts its own, so an update never talks to a stale daemon.

/// Semantic version of this workspace.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build identity. Release pipelines set `REGARDS_BUILD_ID` at compile time
/// (for example to the commit hash); local builds report `<version>+dev`.
pub const BUILD_ID: &str = match option_env!("REGARDS_BUILD_ID") {
    Some(id) => id,
    None => concat!(env!("CARGO_PKG_VERSION"), "+dev"),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_id_starts_with_version_for_local_builds() {
        if option_env!("REGARDS_BUILD_ID").is_none() {
            assert_eq!(BUILD_ID, format!("{VERSION}+dev"));
        }
    }
}
