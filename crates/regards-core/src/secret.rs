//! A secret value (password, token) that is wiped from memory on drop and
//! never printed by `Debug`.

use std::fmt;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// The secret text. Hand it straight to the protocol code; never log or store it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// Serialised as the plain string. The only use is the local IPC request that
/// carries a password the user just typed to the daemon (ADR-0006).
impl serde::Serialize for Secret {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.expose())
    }
}

impl<'de> serde::Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::new)
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn debug_output_is_redacted() {
        assert_eq!(format!("{:?}", super::Secret::new("hunter2")), "Secret(<redacted>)");
    }
}
