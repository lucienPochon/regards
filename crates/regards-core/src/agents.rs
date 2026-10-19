//! What agents may do with an account (ADR-0012): its agent access level,
//! Off by default. Each level allows what the one before it does, and more.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// Nothing: agents do not see the account.
    #[default]
    Off,
    /// List, search and read mail. Reading never marks it read.
    Read,
    /// Read, and drafts and organising: archive, move, flags, pin, snooze, senders.
    Triage,
    /// Triage, and sending.
    Send,
}

impl Access {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Read => "read",
            Self::Triage => "triage",
            Self::Send => "send",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "off" => Some(Self::Off),
            "read" => Some(Self::Read),
            "triage" => Some(Self::Triage),
            "send" => Some(Self::Send),
            _ => None,
        }
    }

    /// As the app names it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Read => "Read",
            Self::Triage => "Triage",
            Self::Send => "Send",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_rise_and_read_back() {
        assert!(Access::Off < Access::Read && Access::Read < Access::Triage && Access::Triage < Access::Send);
        assert_eq!(Access::default(), Access::Off);
        for access in [Access::Off, Access::Read, Access::Triage, Access::Send] {
            assert_eq!(Access::parse(access.as_str()), Some(access));
        }
        assert_eq!(Access::parse("admin"), None);
    }
}
