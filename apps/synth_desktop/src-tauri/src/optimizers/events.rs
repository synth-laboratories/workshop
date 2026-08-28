//! Event identity, ordering, and batch validation — the one place that decides
//! what a producer is allowed to put into the durable log.
//!
//! The rule this module exists to enforce: **a producer supplies event content,
//! never authoritative order**. Before it existed, every worker computed its own
//! sequence numbers from a `service.get()` snapshot and handed them to
//! `append_events`, which skipped anything at or below the run's cursor and
//! inserted the rest with `INSERT OR IGNORE`. Two writers — or one writer plus a
//! stale `persist_run` that rewound `cursor_seq` — was enough to make a whole
//! batch vanish while the run still advanced its cursor and reported success.
//! That is exactly how `opt_eval_banking77_81d51f81b59f` finished ten of ten
//! rollouts and left `optimizer.run.started` as its entire history.
//!
//! So: [`OptimizerEventDraft`] carries content only, and the service seals it
//! into an envelope inside the same transaction that advances the cursor.
//! [`plan_batch`] validates a *complete* batch before a single row is inserted,
//! and every outcome is explicit — appended, or a confirmed byte-for-byte
//! replay, or an error. Nothing is silently skipped.

use anyhow::{bail, Result};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

use super::models::{OptimizerEventEnvelope, OPTIMIZER_EVENT_SCHEMA_VERSION};

fn strip_frame_body(container: &mut Map<String, Value>) {
    if let Some(frame) = container.get_mut("frame").and_then(Value::as_object_mut) {
        frame.remove("data_url");
        frame.remove("dataUrl");
    }
    if let Some(payload) = container.get_mut("payload").and_then(Value::as_object_mut) {
        payload.remove("data_url");
        payload.remove("dataUrl");
    }
}

fn mutable_container_event(
    event: &mut OptimizerEventEnvelope,
    raw: bool,
) -> Option<&mut Map<String, Value>> {
    if raw {
        let object = event.raw.as_object_mut()?;
        let value = if object.contains_key("container_event") {
            object.get_mut("container_event")
        } else {
            object.get_mut("containerEvent")
        }?;
        value.as_object_mut()
    } else {
        let value = if event.delta.contains_key("container_event") {
            event.delta.get_mut("container_event")
        } else {
            // COMPAT_CONTAINER_EVENT_CAMEL_CASE_THROUGH: legacy read only;
            // remove after the release following 2026-08.
            event.delta.get_mut("containerEvent")
        }?;
        value.as_object_mut()
    }
}

/// Event subscriptions carry telemetry only. Native frame bodies have their
/// own cursor and content APIs; allowing even one base64 PNG per event page to
/// enter the shared run store makes memory grow with the number of pages and
/// surfaces. Legacy rows may still contain inline bodies, so strip them here.
pub fn strip_frame_bodies_for_ipc(events: &mut [OptimizerEventEnvelope]) {
    for event in events.iter_mut() {
        if let Some(container) = mutable_container_event(event, true) {
            strip_frame_body(container);
        }
        if let Some(container) = mutable_container_event(event, false) {
            strip_frame_body(container);
        }
    }
}

/// Event content, without identity or order.
///
/// Producers build these; the service assigns `sequence_number`, `event_id`, and
/// the durable `occurred_at`, inside the append transaction.
#[derive(Clone, Debug)]
pub(super) struct OptimizerEventDraft {
    pub event_type: String,
    pub algorithm_id: String,
    /// When the producer observed the fact. Defaults to seal time.
    pub occurred_at: Option<String>,
    pub level: Option<String>,
    pub item: Option<Value>,
    pub delta: Map<String, Value>,
    pub snapshot: Option<Map<String, Value>>,
    pub usage_delta: Option<Map<String, Value>>,
    pub artifact_refs: Vec<Value>,
    pub error: Option<Value>,
    pub raw: Value,
    /// Stable identity for an event a producer may legitimately re-offer (a
    /// retried terminal settlement, a resumed worker). Two drafts with the same
    /// key in one run are the same event; the second seals to nothing.
    pub idempotency_key: Option<String>,
}

impl OptimizerEventDraft {
    pub fn new(event_type: impl Into<String>, algorithm_id: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            algorithm_id: algorithm_id.into(),
            occurred_at: None,
            level: None,
            item: None,
            delta: Map::new(),
            snapshot: None,
            usage_delta: None,
            artifact_refs: Vec::new(),
            error: None,
            raw: Value::Null,
            idempotency_key: None,
        }
    }

    pub fn level(mut self, level: impl Into<String>) -> Self {
        self.level = Some(level.into());
        self
    }

    pub fn item(mut self, item: Value) -> Self {
        self.item = Some(item);
        self
    }

    pub fn delta(mut self, delta: Map<String, Value>) -> Self {
        self.delta = delta;
        self
    }

    pub fn snapshot(mut self, snapshot: Map<String, Value>) -> Self {
        self.snapshot = Some(snapshot);
        self
    }

    pub fn usage_delta(mut self, usage: Map<String, Value>) -> Self {
        self.usage_delta = Some(usage);
        self
    }

    pub fn error(mut self, error: Value) -> Self {
        self.error = Some(error);
        self
    }

    pub fn artifact_refs(mut self, refs: Vec<Value>) -> Self {
        self.artifact_refs = refs;
        self
    }

    pub fn raw(mut self, raw: Value) -> Self {
        self.raw = raw;
        self
    }

    pub fn occurred_at(mut self, occurred_at: impl Into<String>) -> Self {
        self.occurred_at = Some(occurred_at.into());
        self
    }

    /// Producer timestamp when there is one; seal time when there is not.
    /// Relayed events carry the container's `ts`, which is the moment the
    /// environment actually observed the fact rather than the moment Workshop
    /// got around to reading it.
    pub fn occurred_at_opt(self, occurred_at: Option<&str>) -> Self {
        match occurred_at.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => self.occurred_at(value),
            None => self,
        }
    }

    pub fn idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    /// Seal into a durable envelope at `sequence_number`. Called by the service
    /// only, inside the append transaction.
    pub fn seal(
        self,
        run_id: &str,
        sequence_number: u64,
        sealed_at: &str,
    ) -> OptimizerEventEnvelope {
        let event_id = self
            .idempotency_key
            .map(|key| format!("{run_id}:{key}"))
            .unwrap_or_else(|| format!("{run_id}:{sequence_number}"));
        OptimizerEventEnvelope {
            schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: Some(event_id),
            event_type: self.event_type,
            sequence_number,
            occurred_at: self.occurred_at.unwrap_or_else(|| sealed_at.to_string()),
            optimizer_run_id: run_id.into(),
            algorithm_id: self.algorithm_id,
            level: self.level,
            item: self.item,
            delta: self.delta,
            snapshot: self.snapshot,
            usage_delta: self.usage_delta,
            artifact_refs: self.artifact_refs,
            error: self.error,
            raw: self.raw,
        }
    }
}

/// What the service must do with one event of a validated batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EventVerdict {
    /// Not durable yet. Insert it and advance the cursor to its sequence.
    Append,
    /// Already durable at this sequence under the same identity: a replayed
    /// import or a re-offered idempotent settlement. Insert nothing, advance
    /// nothing, and do not treat it as loss.
    ConfirmedReplay,
}

/// How strictly a batch's own sequence numbers are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SequenceContract {
    /// The service allocated the numbers: they must be contiguous from the
    /// cursor with no holes. Every local worker is on this contract.
    ServiceAllocated,
    /// The numbers mirror a remote log (hosted campaigns, imported sidecar
    /// feeds). Forward holes are the remote's business, but a number already
    /// durable under a different identity is still a collision, not a skip.
    Mirrored,
}

/// Validate a complete batch against what is already durable.
///
/// Returns one verdict per event, in order, or an error naming the first
/// violation. No caller may insert any event of a batch this rejects: an event
/// plan that cannot be executed as a whole must not be executed in part.
pub(super) fn plan_batch(
    run_id: &str,
    cursor_seq: u64,
    durable: &HashMap<u64, String>,
    events: &[OptimizerEventEnvelope],
    contract: SequenceContract,
) -> Result<Vec<EventVerdict>> {
    if events.is_empty() {
        bail!("event batch is empty");
    }
    let mut verdicts = Vec::with_capacity(events.len());
    let mut previous: Option<u64> = None;
    let mut highest_appended = cursor_seq;
    for event in events {
        validate_shape(run_id, event)?;
        let seq = event.sequence_number;
        if let Some(previous) = previous {
            if seq <= previous {
                bail!(
                    "event batch is not ordered: {} follows {previous} in {}",
                    seq,
                    event.event_type
                );
            }
        }
        previous = Some(seq);

        match durable.get(&seq) {
            Some(existing) => {
                // Someone already owns this sequence. It is the same event only
                // if it carries the same identity; otherwise the producer is
                // writing from a stale snapshot and its content would be lost.
                let offered = event.event_id.as_deref();
                if offered.is_some_and(|id| id != existing) {
                    bail!(
                        "sequence {seq} of {run_id} already holds event {existing}; \
                         refusing to drop {} offered as {}",
                        event.event_type,
                        offered.unwrap_or("<unnamed>")
                    );
                }
                verdicts.push(EventVerdict::ConfirmedReplay);
            }
            None => {
                if seq <= cursor_seq {
                    // Below the cursor with nothing durable there: the log has a
                    // hole, and appending into it would leave the hole in place.
                    bail!(
                        "sequence {seq} of {run_id} is at or below cursor {cursor_seq} but no \
                         event is durable there; the event log is holed"
                    );
                }
                if contract == SequenceContract::ServiceAllocated && seq != highest_appended + 1 {
                    bail!(
                        "service-allocated batch is not contiguous: expected {} for {}, got {seq}",
                        highest_appended + 1,
                        event.event_type
                    );
                }
                highest_appended = seq;
                verdicts.push(EventVerdict::Append);
            }
        }
    }
    Ok(verdicts)
}

/// Shape rules a producer can compile past but a projector cannot survive.
fn validate_shape(run_id: &str, event: &OptimizerEventEnvelope) -> Result<()> {
    if event.optimizer_run_id != run_id {
        bail!(
            "event optimizer_run_id mismatch: {} is not {run_id}",
            event.optimizer_run_id
        );
    }
    if event.schema_version != OPTIMIZER_EVENT_SCHEMA_VERSION {
        bail!(
            "unsupported optimizer event schema {} (expected {OPTIMIZER_EVENT_SCHEMA_VERSION})",
            event.schema_version
        );
    }
    if event.event_type.trim().is_empty() {
        bail!("event {} has no type", event.sequence_number);
    }
    if event.algorithm_id.trim().is_empty() {
        bail!("event {} has no algorithm id", event.event_type);
    }
    if event.sequence_number == 0 {
        bail!(
            "event {} has sequence 0; sequences start at 1",
            event.event_type
        );
    }
    if chrono::DateTime::parse_from_rfc3339(&event.occurred_at).is_err() {
        bail!(
            "event {} has an unparseable occurred_at {:?}",
            event.event_type,
            event.occurred_at
        );
    }
    Ok(())
}

