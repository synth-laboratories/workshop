//! Bounded, non-blocking diagnostic queue.
//!
//! Producer paths (chat send, rendering, MCP dispatch, rollout polling,
//! optimizer control) call [`DiagnosticBus::enqueue`] and continue. There is no
//! await, no I/O, and no unbounded growth: enqueue takes a short mutex, pushes
//! into a fixed-capacity lane, and wakes the writer.
//!
//! Two lanes, because a saturated queue must not be the thing that discards the
//! error you are debugging. Errors have their own reservation; informational
//! events are dropped first and counted by severity and component so the loss
//! is visible rather than silent.

use super::event::{DiagnosticEvent, Severity};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// Reserved slots for errors. Sized for a burst of correlated failures (a
/// ten-lane rollout failing at once) without letting a stuck writer grow the
/// process.
pub const ERROR_CAPACITY: usize = 2_048;

/// Slots shared by debug/info/warn.
pub const NORMAL_CAPACITY: usize = 8_192;

/// Batch size handed to the journal writer in one transaction.
pub const DRAIN_BATCH: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enqueued {
    Accepted,
    /// The queue was full: this event was dropped (non-error lane).
    DroppedIncoming,
    /// The queue was full: the oldest error was evicted to make room.
    EvictedOldest,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaturationReport {
    pub dropped: u64,
    /// `"severity/component"` → count. Bounded by the closed component list.
    pub by_severity_component: BTreeMap<String, u64>,
}

#[derive(Default)]
struct BusState {
    errors: VecDeque<DiagnosticEvent>,
    normal: VecDeque<DiagnosticEvent>,
    dropped_total: u64,
    dropped_by: BTreeMap<String, u64>,
}

/// Queue shared by every emitter in the process.
pub struct DiagnosticBus {
    state: Mutex<BusState>,
    notify: Notify,
    enqueued_total: AtomicU64,
    error_capacity: usize,
    normal_capacity: usize,
}

impl Default for DiagnosticBus {
    fn default() -> Self {
        Self::with_capacity(ERROR_CAPACITY, NORMAL_CAPACITY)
    }
}

impl DiagnosticBus {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn with_capacity(error_capacity: usize, normal_capacity: usize) -> Self {
        Self {
            state: Mutex::new(BusState::default()),
            notify: Notify::new(),
            enqueued_total: AtomicU64::new(0),
            error_capacity,
            normal_capacity,
        }
    }

    /// Never blocks on I/O and never awaits. Safe to call from any hot path.
    pub fn enqueue(&self, event: DiagnosticEvent) -> Enqueued {
        let outcome = {
            let mut state = self.state.lock().expect("diagnostic bus lock");
            if event.severity.is_preserved() {
                let evicted = if state.errors.len() >= self.error_capacity {
                    state.errors.pop_front().map(|old| {
                        record_drop(&mut state, old.severity, &old.component);
                        Enqueued::EvictedOldest
                    })
                } else {
                    None
                };
                state.errors.push_back(event);
                evicted.unwrap_or(Enqueued::Accepted)
            } else if state.normal.len() >= self.normal_capacity {
                record_drop(&mut state, event.severity, &event.component);
                Enqueued::DroppedIncoming
            } else {
                state.normal.push_back(event);
                Enqueued::Accepted
            }
        };
        if outcome != Enqueued::DroppedIncoming {
            self.enqueued_total.fetch_add(1, Ordering::Relaxed);
        }
        self.notify.notify_one();
        outcome
    }

    /// Take up to `max` queued events in timestamp order.
    ///
    /// Errors are taken first so a persistently saturated queue still makes
    /// progress on the events that matter, then the batch is sorted so the
    /// journal keeps a sensible chronology.
    pub fn drain(&self, max: usize) -> Vec<DiagnosticEvent> {
        let mut state = self.state.lock().expect("diagnostic bus lock");
        let mut batch = Vec::with_capacity(max.min(state.errors.len() + state.normal.len()));
        while batch.len() < max {
            if let Some(event) = state.errors.pop_front() {
                batch.push(event);
                continue;
            }
            match state.normal.pop_front() {
                Some(event) => batch.push(event),
                None => break,
            }
        }
        batch.sort_by(|left, right| left.timestamp.cmp(&right.timestamp));
        batch
    }

    /// Consume the accumulated drop accounting. The caller emits exactly one
    /// bounded saturation diagnostic per recovery, not one per dropped event.
    pub fn take_saturation(&self) -> Option<SaturationReport> {
        let mut state = self.state.lock().expect("diagnostic bus lock");
        if state.dropped_total == 0 {
            return None;
        }
        Some(SaturationReport {
            dropped: std::mem::take(&mut state.dropped_total),
            by_severity_component: std::mem::take(&mut state.dropped_by),
        })
    }

    pub fn depth(&self) -> usize {
        let state = self.state.lock().expect("diagnostic bus lock");
        state.errors.len() + state.normal.len()
    }

    pub fn enqueued_total(&self) -> u64 {
        self.enqueued_total.load(Ordering::Relaxed)
    }

    pub fn capacity(&self) -> usize {
        self.error_capacity + self.normal_capacity
    }

    pub async fn wait_for_work(&self) {
        self.notify.notified().await;
    }
}

fn record_drop(state: &mut BusState, severity: Severity, component: &str) {
    state.dropped_total = state.dropped_total.saturating_add(1);
    *state
        .dropped_by
        .entry(format!("{}/{component}", severity.as_str()))
        .or_default() += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::event::{validate, DiagnosticInput};

    fn event(severity: Severity, component: &str, timestamp: &str) -> DiagnosticEvent {
        let mut input = DiagnosticInput::new(severity, component, "test.event", "test_code", "m");
        input.timestamp = Some(timestamp.into());
        validate(input).unwrap()
    }

    #[test]
    fn accepts_until_capacity_then_drops_informational_events() {
        let bus = DiagnosticBus::with_capacity(2, 2);
        for index in 0..2 {
            assert_eq!(
                bus.enqueue(event(
                    Severity::Info,
                    "renderer",
                    &format!("2026-08-16T00:00:0{index}Z")
                )),
                Enqueued::Accepted
            );
        }
        assert_eq!(
            bus.enqueue(event(Severity::Info, "renderer", "2026-08-16T00:00:09Z")),
            Enqueued::DroppedIncoming
        );
        assert_eq!(bus.depth(), 2);
    }

    #[test]
    fn errors_have_their_own_reservation_and_survive_an_info_flood() {
        let bus = DiagnosticBus::with_capacity(4, 2);
        for index in 0..20 {
            bus.enqueue(event(
                Severity::Info,
                "renderer",
                &format!("2026-08-16T00:00:{:02}Z", index),
            ));
        }
        assert_eq!(
            bus.enqueue(event(Severity::Error, "visual-host", "2026-08-16T00:01:00Z")),
            Enqueued::Accepted
        );
        let drained = bus.drain(64);
        assert!(drained.iter().any(|item| item.severity == Severity::Error));
    }

    #[test]
    fn a_full_error_lane_evicts_the_oldest_error_not_the_newest() {
        let bus = DiagnosticBus::with_capacity(2, 2);
        for index in 0..3 {
            bus.enqueue(event(
                Severity::Error,
                "containers",
                &format!("2026-08-16T00:00:0{index}Z"),
            ));
        }
        let drained = bus.drain(64);
        let timestamps: Vec<&str> = drained.iter().map(|item| item.timestamp.as_str()).collect();
        assert_eq!(timestamps.len(), 2);
        assert!(!timestamps[0].contains("00:00:00"), "{timestamps:?}");
    }

    #[test]
    fn drops_are_counted_by_severity_and_component() {
        let bus = DiagnosticBus::with_capacity(1, 1);
        bus.enqueue(event(Severity::Info, "renderer", "2026-08-16T00:00:00Z"));
        bus.enqueue(event(Severity::Info, "renderer", "2026-08-16T00:00:01Z"));
        bus.enqueue(event(Severity::Warn, "containers", "2026-08-16T00:00:02Z"));
        let report = bus.take_saturation().expect("saturation report");
        assert_eq!(report.dropped, 2);
        assert_eq!(report.by_severity_component["info/renderer"], 1);
        assert_eq!(report.by_severity_component["warn/containers"], 1);
        assert!(bus.take_saturation().is_none(), "report must be consumed once");
    }

    #[test]
    fn drain_returns_a_bounded_batch_in_timestamp_order() {
        let bus = DiagnosticBus::default();
        bus.enqueue(event(Severity::Info, "renderer", "2026-08-16T00:00:05Z"));
        bus.enqueue(event(Severity::Error, "renderer", "2026-08-16T00:00:01Z"));
        bus.enqueue(event(Severity::Info, "renderer", "2026-08-16T00:00:03Z"));
        let batch = bus.drain(2);
        assert_eq!(batch.len(), 2);
        assert!(batch[0].timestamp <= batch[1].timestamp);
        assert_eq!(bus.depth(), 1);
    }

    #[test]
    fn enqueue_stays_off_the_producer_path_budget() {
        // Not a benchmark — a regression guard. Enqueue must remain a lock,
        // a push, and a notify; anything that starts doing I/O here will blow
        // this bound by orders of magnitude.
        let bus = DiagnosticBus::default();
        let sample = event(Severity::Info, "renderer", "2026-08-16T00:00:00Z");
        let started = std::time::Instant::now();
        for _ in 0..1_000 {
            bus.enqueue(sample.clone());
        }
        let per_event = started.elapsed() / 1_000;
        assert!(
            per_event < std::time::Duration::from_micros(500),
            "enqueue took {per_event:?} per event"
        );
    }
}
