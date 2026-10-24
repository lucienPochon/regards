//! How much an agent may ask, per connection (ADR-0012 §5): a steady rate
//! with some room for bursts, tighter for changes than for reading. Past it,
//! requests are refused with [`codes::RATE_LIMITED`] and when to try again.

use regards_ipc::protocol::{RpcError, codes, methods};
use serde_json::json;
use std::time::Instant;

/// Any request: a burst of 120, then 10 a second.
const REQUESTS: (f64, f64) = (120.0, 10.0);
/// Changes: a burst of 30, then 1 a second.
const CHANGES: (f64, f64) = (30.0, 1.0);

/// Requests that change nothing.
const READING: &[&str] = &[
    methods::HELLO,
    methods::DAEMON_HEALTH,
    methods::ACCOUNT_LIST,
    methods::MAILBOX_LIST,
    methods::MESSAGE_LIST,
    methods::SEARCH,
    methods::SEARCH_SERVER,
    methods::PEOPLE_SUGGEST,
    methods::MESSAGE_GET,
    methods::MESSAGE_HTML,
    methods::MESSAGE_SOURCE,
    methods::ATTACHMENT_PREVIEW,
    methods::THREAD_GET,
    methods::MESSAGE_CONVERSATION_IDS,
    methods::MESSAGE_LABELS,
    methods::MESSAGE_VIEW,
    methods::SMART_OVERVIEW,
    methods::MESSAGE_EXPLAIN,
    methods::DRAFT_LIST,
    methods::DRAFT_GET,
    methods::DRAFT_CHECK,
    methods::HOLD_GET,
    methods::NOTE_LIST,
    methods::MEMORY_GET,
];

/// A token bucket: `capacity` at most, refilled at `per_second`.
#[derive(Debug, Clone)]
struct Bucket {
    capacity: f64,
    per_second: f64,
    tokens: f64,
    at: Instant,
}

impl Bucket {
    fn new((capacity, per_second): (f64, f64), now: Instant) -> Self {
        Self {
            capacity,
            per_second,
            tokens: capacity,
            at: now,
        }
    }

    fn refill(&mut self, now: Instant) {
        let seconds = now.saturating_duration_since(self.at).as_secs_f64();
        self.tokens = (self.tokens + seconds * self.per_second).min(self.capacity);
        self.at = now;
    }

    /// How long until one is free, in ms, if none is now.
    fn wait(&self) -> Option<u64> {
        (self.tokens < 1.0).then(|| (((1.0 - self.tokens) / self.per_second) * 1000.0).ceil() as u64)
    }
}

/// One agent connection's allowance.
#[derive(Debug, Clone)]
pub(crate) struct Limits {
    requests: Bucket,
    changes: Bucket,
}

impl Limits {
    pub(crate) fn new() -> Self {
        let now = Instant::now();
        Self {
            requests: Bucket::new(REQUESTS, now),
            changes: Bucket::new(CHANGES, now),
        }
    }

    /// Take one request of `method`, or say when to try again.
    pub(crate) fn take(&mut self, method: &str) -> Result<(), RpcError> {
        self.take_at(method, Instant::now())
    }

    fn take_at(&mut self, method: &str, now: Instant) -> Result<(), RpcError> {
        let change = !READING.contains(&method);
        self.requests.refill(now);
        self.changes.refill(now);
        let wait = self
            .requests
            .wait()
            .or_else(|| if change { self.changes.wait() } else { None });
        if let Some(retry_after_ms) = wait {
            let mut error = RpcError::new(
                codes::RATE_LIMITED,
                format!(
                    "too many requests: try again in {:.1} s",
                    retry_after_ms as f64 / 1000.0
                ),
            );
            error.data = Some(json!({ "retry_after_ms": retry_after_ms }));
            return Err(error);
        }
        self.requests.tokens -= 1.0;
        if change {
            self.changes.tokens -= 1.0;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn bursts_then_a_steady_rate_tighter_for_changes() {
        let start = Instant::now();
        let mut limits = Limits::new();
        for _ in 0..30 {
            limits.take_at(methods::MESSAGE_ARCHIVE, start).unwrap();
        }
        let refused = limits.take_at(methods::MESSAGE_ARCHIVE, start).unwrap_err();
        assert_eq!(refused.code, codes::RATE_LIMITED);
        assert_eq!(refused.data.unwrap()["retry_after_ms"], 1000);
        // Reading goes on meanwhile.
        limits.take_at(methods::SEARCH, start).unwrap();
        // A second later, one more change.
        let later = start + Duration::from_secs(1);
        limits.take_at(methods::MESSAGE_ARCHIVE, later).unwrap();
        assert!(limits.take_at(methods::MESSAGE_ARCHIVE, later).is_err());

        // Reading has its own, larger allowance.
        let mut reader = Limits::new();
        for _ in 0..120 {
            reader.take_at(methods::MESSAGE_GET, start).unwrap();
        }
        let refused = reader.take_at(methods::MESSAGE_GET, start).unwrap_err();
        assert_eq!(refused.data.unwrap()["retry_after_ms"], 100);
        assert!(
            reader
                .take_at(methods::MESSAGE_GET, start + Duration::from_millis(100))
                .is_ok()
        );
    }
}
