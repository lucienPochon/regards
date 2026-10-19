//! Who sends a queued message at its time (plan §5.7, ADR-0019).
//!
//! Regards 1.0 has one executor: this computer. The outbox and the API
//! carry the executor from 1.0 on, so 1.1 adds those that send while the
//! computer is off without a schema or API change. A remote executor takes
//! the frozen message when it is scheduled (the entry is then `handed_off`),
//! and taking it back or moving it must reach that executor first.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Executor {
    /// This computer, which must be on, with Regards running, at the time.
    #[default]
    Device,
    /// The mail provider's own scheduled delivery: SMTP FUTURERELEASE, or
    /// Exchange-held delayed delivery (1.1).
    Provider,
    /// The person's own Send Service, in personal mode (1.1).
    SelfHosted,
    /// The Regards Send Service (Pro, 1.1).
    SendService,
}

impl Executor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Device => "device",
            Self::Provider => "provider",
            Self::SelfHosted => "self_hosted",
            Self::SendService => "send_service",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "device" => Self::Device,
            "provider" => Self::Provider,
            "self_hosted" => Self::SelfHosted,
            "send_service" => Self::SendService,
            _ => return None,
        })
    }

    /// It sends from this computer: for a scheduled message, the computer
    /// must be on at that time (the UI and agents say so). One found well
    /// past its time waits for the person (ADR-0011 §2).
    pub fn needs_this_computer(self) -> bool {
        self == Self::Device
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_those_the_outbox_allows() {
        // migration 0013's CHECK: device, provider, self_hosted, send_service.
        for executor in [
            Executor::Device,
            Executor::Provider,
            Executor::SelfHosted,
            Executor::SendService,
        ] {
            assert_eq!(Executor::parse(executor.as_str()), Some(executor));
            assert_eq!(
                serde_json::to_value(executor).unwrap(),
                serde_json::Value::String(executor.as_str().into())
            );
        }
        assert_eq!(Executor::parse("cloud"), None);
        assert!(Executor::Device.needs_this_computer());
        assert!(!Executor::SendService.needs_this_computer());
    }
}
