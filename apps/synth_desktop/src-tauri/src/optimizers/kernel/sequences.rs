//! Producer sequence versus Workshop aggregate sequence.
//!
//! A producer supplies content and its own replay order. Workshop assigns the
//! canonical total order. A reconnect replays producer events: confirmed
//! events acknowledge idempotently; conflicting digests fail; a gap blocks
//! reduction.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use super::error::{KernelError, KernelErrorCode, KernelResult};
use super::types::{AlgorithmKind, PRODUCER_EVENT_SCHEMA_VERSION};

/// One producer-offered fact. Workshop has not yet assigned aggregate order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProducerEvent {
    pub producer_id: String,
    pub producer_sequence: u64,
    pub idempotency_key: String,
    pub schema_version: String,
    pub algorithm_id: String,
    pub event_type: String,
    pub occurred_at: String,
    pub payload_digest: String,
    pub payload: serde_json::Value,
}

impl ProducerEvent {
    pub fn compute_payload_digest(payload: &serde_json::Value) -> String {
        let canonical = serde_json::to_string(payload).unwrap_or_else(|_| "null".into());
        format!("sha256:{:x}", Sha256::digest(canonical.as_bytes()))
    }

    pub fn with_computed_digest(mut self) -> Self {
        self.payload_digest = Self::compute_payload_digest(&self.payload);
        self
    }

    pub fn algorithm(&self) -> KernelResult<AlgorithmKind> {
        AlgorithmKind::parse_wire(&self.algorithm_id)
    }

    pub fn validate_shape(&self) -> KernelResult<()> {
        if self.producer_id.trim().is_empty() {
            return Err(KernelError::new(
                KernelErrorCode::EventSchemaMismatch,
                "producer_id is empty",
            ));
        }
        if self.producer_sequence == 0 {
            return Err(KernelError::new(
                KernelErrorCode::EventSchemaMismatch,
                "producer_sequence starts at 1",
            ));
        }
        if self.idempotency_key.trim().is_empty() {
            return Err(KernelError::new(
                KernelErrorCode::EventSchemaMismatch,
                "idempotency_key is empty",
            ));
        }
        if self.schema_version != PRODUCER_EVENT_SCHEMA_VERSION {
            return Err(KernelError::new(
                KernelErrorCode::EventSchemaUnknown,
                format!(
                    "unsupported producer event schema {} (expected {PRODUCER_EVENT_SCHEMA_VERSION})",
                    self.schema_version
                ),
            ));
        }
        if self.event_type.trim().is_empty() {
            return Err(KernelError::new(
                KernelErrorCode::EventSchemaMismatch,
                "event_type is empty",
            ));
        }
        let _ = self.algorithm()?;
        if chrono::DateTime::parse_from_rfc3339(&self.occurred_at).is_err() {
            return Err(KernelError::new(
                KernelErrorCode::EventSchemaMismatch,
                format!("unparseable occurred_at {:?}", self.occurred_at),
            ));
        }
        let expected = Self::compute_payload_digest(&self.payload);
        if self.payload_digest != expected {
            return Err(KernelError::new(
                KernelErrorCode::PayloadDigestMismatch,
                format!(
                    "payload digest {} does not match content {expected}",
                    self.payload_digest
                ),
            ));
        }
        Ok(())
    }
}

/// Durable producer log keyed by (producer_id, producer_sequence).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DurableProducerLog {
    /// Highest contiguous producer sequence per producer.
    pub cursors: BTreeMap<String, u64>,
    /// (producer_id, producer_sequence) -> (idempotency_key, payload_digest)
    pub entries: BTreeMap<(String, u64), (String, String)>,
    /// idempotency_key -> (producer_id, producer_sequence, payload_digest)
    pub by_key: BTreeMap<String, (String, u64, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProducerVerdict {
    Append,
    ConfirmedReplay,
}

/// Workshop-assigned envelope. The product record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommittedEvent {
    pub aggregate_sequence: u64,
    pub committed_at: String,
    pub producer: ProducerEvent,
}

/// Validate a complete producer batch against the durable producer log.
///
/// Returns one verdict per event, or the first typed violation. A gap blocks
/// the whole batch: partial reduction would leave the aggregate lying about
/// what the producer has proven.
pub fn plan_producer_batch(
    log: &DurableProducerLog,
    events: &[ProducerEvent],
) -> KernelResult<Vec<ProducerVerdict>> {
    if events.is_empty() {
        return Err(KernelError::new(
            KernelErrorCode::EventSchemaMismatch,
            "producer batch is empty",
        ));
    }
    let mut verdicts = Vec::with_capacity(events.len());
    let mut cursors = log.cursors.clone();
    let mut seen_keys: BTreeMap<String, (String, u64, String)> = log.by_key.clone();
    for event in events {
        event.validate_shape()?;
        let producer_id = event.producer_id.clone();
        let seq = event.producer_sequence;
        let expected = cursors.get(&producer_id).copied().unwrap_or(0) + 1;

        if let Some((existing_key, existing_digest)) = log.entries.get(&(producer_id.clone(), seq))
        {
            if existing_key != &event.idempotency_key || existing_digest != &event.payload_digest {
                return Err(KernelError::new(
                    KernelErrorCode::ProducerIdempotencyConflict,
                    format!(
                        "producer {producer_id} sequence {seq} already holds key {existing_key} \
                         digest {existing_digest}; offered key {} digest {}",
                        event.idempotency_key, event.payload_digest
                    ),
                ));
            }
            verdicts.push(ProducerVerdict::ConfirmedReplay);
            continue;
        }

        if seq != expected {
            return Err(KernelError::new(
                KernelErrorCode::ProducerSequenceGap,
                format!("producer {producer_id} offered sequence {seq}, expected {expected}"),
            ));
        }

        if let Some((existing_producer, existing_seq, existing_digest)) =
            seen_keys.get(&event.idempotency_key)
        {
            if existing_producer != &producer_id
                || *existing_seq != seq
                || existing_digest != &event.payload_digest
            {
                return Err(KernelError::new(
                    KernelErrorCode::ProducerIdempotencyConflict,
                    format!(
                        "idempotency key {} already bound to {existing_producer}#{existing_seq}",
                        event.idempotency_key
                    ),
                ));
            }
            verdicts.push(ProducerVerdict::ConfirmedReplay);
            continue;
        }

        seen_keys.insert(
            event.idempotency_key.clone(),
            (producer_id.clone(), seq, event.payload_digest.clone()),
        );
        cursors.insert(producer_id, seq);
        verdicts.push(ProducerVerdict::Append);
    }
    Ok(verdicts)
}

pub fn assign_aggregate_sequences(
    next_aggregate: u64,
    committed_at: &str,
    events: &[ProducerEvent],
    verdicts: &[ProducerVerdict],
) -> KernelResult<Vec<CommittedEvent>> {
    let mut seq = next_aggregate;
    let mut out = Vec::new();
    for (event, verdict) in events.iter().zip(verdicts) {
        if *verdict == ProducerVerdict::ConfirmedReplay {
            continue;
        }
        seq += 1;
        out.push(CommittedEvent {
            aggregate_sequence: seq,
            committed_at: committed_at.to_string(),
            producer: event.clone(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(producer: &str, seq: u64, key: &str, payload: serde_json::Value) -> ProducerEvent {
        ProducerEvent {
            producer_id: producer.into(),
            producer_sequence: seq,
            idempotency_key: key.into(),
            schema_version: PRODUCER_EVENT_SCHEMA_VERSION.into(),
            algorithm_id: "gepa".into(),
            event_type: "optimizer.run.started".into(),
            occurred_at: "2026-08-27T18:00:00Z".into(),
            payload_digest: String::new(),
            payload,
        }
        .with_computed_digest()
    }

    #[test]
    fn contiguous_append_assigns_aggregate_order() {
        let batch = vec![
            event("local-1", 1, "a", json!({"n": 1})),
            event("local-1", 2, "b", json!({"n": 2})),
        ];
        let plan = plan_producer_batch(&DurableProducerLog::default(), &batch).unwrap();
        assert_eq!(plan, vec![ProducerVerdict::Append, ProducerVerdict::Append]);
        let committed =
            assign_aggregate_sequences(0, "2026-08-27T18:00:01Z", &batch, &plan).unwrap();
        assert_eq!(committed[0].aggregate_sequence, 1);
        assert_eq!(committed[1].aggregate_sequence, 2);
        assert_eq!(committed[0].producer.producer_sequence, 1);
    }

    #[test]
    fn confirmed_replay_is_idempotent() {
        let first = event("local-1", 1, "a", json!({"n": 1}));
        let mut log = DurableProducerLog::default();
        log.cursors.insert("local-1".into(), 1);
        log.entries.insert(
            ("local-1".into(), 1),
            (first.idempotency_key.clone(), first.payload_digest.clone()),
        );
        log.by_key.insert(
            first.idempotency_key.clone(),
            ("local-1".into(), 1, first.payload_digest.clone()),
        );
        let plan = plan_producer_batch(&log, &[first.clone()]).unwrap();
        assert_eq!(plan, vec![ProducerVerdict::ConfirmedReplay]);
        let committed = assign_aggregate_sequences(1, "now", &[first], &plan).unwrap();
        assert!(committed.is_empty());
    }

    #[test]
    fn digest_conflict_is_typed_not_a_skip() {
        let original = event("local-1", 1, "a", json!({"n": 1}));
        let mut conflicted = original.clone();
        conflicted.payload = json!({"n": 2});
        conflicted.payload_digest = ProducerEvent::compute_payload_digest(&conflicted.payload);
        let mut log = DurableProducerLog::default();
        log.cursors.insert("local-1".into(), 1);
        log.entries.insert(
            ("local-1".into(), 1),
            (
                original.idempotency_key.clone(),
                original.payload_digest.clone(),
            ),
        );
        let error = plan_producer_batch(&log, &[conflicted]).unwrap_err();
        assert_eq!(error.code, KernelErrorCode::ProducerIdempotencyConflict);
    }

    #[test]
    fn sequence_gap_blocks_the_batch() {
        let error = plan_producer_batch(
            &DurableProducerLog::default(),
            &[event("local-1", 2, "a", json!({}))],
        )
        .unwrap_err();
        assert_eq!(error.code, KernelErrorCode::ProducerSequenceGap);
    }
}
