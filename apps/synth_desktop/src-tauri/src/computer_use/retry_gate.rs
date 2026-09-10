//! Bound helper launch attempts, including failures, independently of UI polling.
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct RetryGate {
    last_attempt: Option<Instant>,
}

impl RetryGate {
    pub(super) fn admit(&mut self, now: Instant) -> bool {
        if self
            .last_attempt
            .is_some_and(|last| now.saturating_duration_since(last) < Duration::from_secs(30))
        {
            return false;
        }
        // Record before launching: cancellation and failures must also back off.
        self.last_attempt = Some(now);
        true
    }
}

