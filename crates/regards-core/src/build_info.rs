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

/// Whether a release tag (`v1.2.3` or `1.2.3`) is a later version than
/// `current`. A pre-release (`v1.3.0-beta`) never is: only releases are offered.
pub fn is_newer(tag: &str, current: &str) -> bool {
    let numbers = |text: &str, release_only: bool| -> Option<[u64; 3]> {
        let text = text.trim().trim_start_matches('v');
        if release_only && text.contains('-') {
            return None;
        }
        // Build metadata (`+dev`) and a pre-release of the running version do not count.
        let core = text.split(['-', '+']).next().unwrap_or(text);
        let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
        let found = [parts.next()??, parts.next()??, parts.next()??];
        parts.next().is_none().then_some(found)
    };
    match (numbers(tag, true), numbers(current, false)) {
        (Some(tag), Some(current)) => tag > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_releases_only() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.12"));
        assert!(is_newer("v0.1.10", "0.1.9"), "numbers, not text");
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(!is_newer("v0.2.0-beta.1", "0.1.0"), "never a pre-release");
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(is_newer("v0.2.0", "0.1.0+dev"), "a local build is its version");
    }

    #[test]
    fn build_id_starts_with_version_for_local_builds() {
        if option_env!("REGARDS_BUILD_ID").is_none() {
            assert_eq!(BUILD_ID, format!("{VERSION}+dev"));
        }
    }
}
