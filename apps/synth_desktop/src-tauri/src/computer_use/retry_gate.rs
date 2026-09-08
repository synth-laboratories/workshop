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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_failed_polls_do_not_retry() {
        let now = Instant::now();
        let mut gate = RetryGate::default();
        assert!(gate.admit(now));
        for millis in 1..30_000 {
            assert!(!gate.admit(now + Duration::from_millis(millis)));
        }
        assert!(gate.admit(now + Duration::from_secs(30)));
        assert!(!gate.admit(now + Duration::from_secs(31)));
    }

    #[test]
    fn backwards_time_does_not_bypass_gate() {
        let now = Instant::now();
        let mut gate = RetryGate::default();
        assert!(gate.admit(now));
        assert!(!gate.admit(now - Duration::from_secs(1)));
    }
}
