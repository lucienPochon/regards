//! The desktop bridge's per-launch secret (ADR-0006).
//!
//! A daemon started with `--ui-handoff` generates one token and hands it to the
//! process that started it over the daemon's stdout pipe. Only that process can
//! read it, and it is never written to disk. A connection that presents the
//! token in `hello` gets the `ui` principal.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Random bytes in a token (256 bits).
const TOKEN_BYTES: usize = 32;

/// A per-launch secret. `Debug` never prints the value.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UiToken(String);

impl UiToken {
    /// A fresh token from the operating system's secure random source.
    pub fn generate() -> Result<Self, getrandom::Error> {
        let mut bytes = [0u8; TOKEN_BYTES];
        getrandom::fill(&mut bytes)?;
        Ok(Self(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// Compare in constant time, so the comparison does not leak how many characters matched.
    pub fn matches(&self, other: &UiToken) -> bool {
        let (a, b) = (self.0.as_bytes(), other.0.as_bytes());
        a.len() == b.len() && a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
    }
}

impl PartialEq for UiToken {
    fn eq(&self, other: &Self) -> bool {
        self.matches(other)
    }
}

impl Eq for UiToken {}

impl fmt::Debug for UiToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UiToken(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_hex_and_compare_by_value() {
        let a = UiToken::generate().unwrap();
        let b = UiToken::generate().unwrap();
        assert_eq!(a.0.len(), TOKEN_BYTES * 2);
        assert!(a.0.bytes().all(|c| c.is_ascii_hexdigit()));
        assert!(a.matches(&a.clone()));
        assert!(!a.matches(&b));
        assert!(!a.matches(&UiToken(a.0[..10].to_owned())));
    }

    #[test]
    fn debug_output_hides_the_value() {
        let token = UiToken::generate().unwrap();
        let debug = format!("{token:?}");
        assert!(!debug.contains(&token.0));
    }
}
