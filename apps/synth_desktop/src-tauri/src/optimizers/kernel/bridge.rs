//! Convert legacy `OptimizerEventEnvelope` batches into kernel producer events.

use super::error::KernelResult;
use super::sequences::ProducerEvent;
use super::types::{AlgorithmKind, ExecutionPlacement, PRODUCER_EVENT_SCHEMA_VERSION};
use crate::optimizers::models::OptimizerEventEnvelope;

pub fn envelope_to_producer(
    envelope: &OptimizerEventEnvelope,
    producer_id: &str,
) -> KernelResult<ProducerEvent> {
    let algorithm = AlgorithmKind::parse_wire(&envelope.algorithm_id)?;
    let payload = envelope_payload(envelope);
    Ok(ProducerEvent {
        producer_id: producer_id.to_string(),
        producer_sequence: envelope.sequence_number,
        idempotency_key: envelope
            .event_id
            .clone()
            .unwrap_or_else(|| format!("{}:{}", envelope.event_type, envelope.sequence_number)),
        schema_version: PRODUCER_EVENT_SCHEMA_VERSION.into(),
        algorithm_id: algorithm.wire_id().to_string(),
        event_type: envelope.event_type.clone(),
        occurred_at: envelope.occurred_at.clone(),
        payload_digest: String::new(),
        payload,
    }
    .with_computed_digest())
}

fn envelope_payload(envelope: &OptimizerEventEnvelope) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    if let Some(item) = envelope.item.as_ref().and_then(|value| value.as_object()) {
        map.extend(item.clone());
    }
    if let Some(snapshot) = &envelope.snapshot {
        map.extend(snapshot.clone());
    }
    map.extend(envelope.delta.clone());
    // Reconciliation items carry authoritative totals. Its usage_delta is
    // intentionally only the remainder for additive legacy consumers and
    // must not overwrite those totals in the kernel payload.
    if let Some(usage) = envelope
        .usage_delta
        .as_ref()
        .filter(|_| envelope.event_type != "optimizer.usage.reconciled")
    {
        copy_usage_field(&mut map, usage, "costUsd", &["costUsd", "cost_usd"]);
        copy_usage_field(&mut map, usage, "calls", &["calls"]);
        copy_usage_field(
            &mut map,
            usage,
            "promptTokens",
            &["promptTokens", "prompt_tokens"],
        );
        copy_usage_field(
            &mut map,
            usage,
            "completionTokens",
            &["completionTokens", "completion_tokens"],
        );
        copy_usage_field(&mut map, usage, "steps", &["steps", "step_count"]);
    }
    if !envelope.artifact_refs.is_empty() {
        map.insert(
            "artifactRefs".into(),
            serde_json::Value::Array(envelope.artifact_refs.clone()),
        );
    }
    if let Some(error) = &envelope.error {
        map.insert("error".into(), error.clone());
    }
    serde_json::Value::Object(map)
}

fn copy_usage_field(
    target: &mut serde_json::Map<String, serde_json::Value>,
    usage: &serde_json::Map<String, serde_json::Value>,
    canonical: &str,
    aliases: &[&str],
) {
    if let Some(value) = aliases.iter().find_map(|key| usage.get(*key)) {
        target.insert(canonical.to_string(), value.clone());
    }
}

pub fn placement_from_run_source(algorithm: AlgorithmKind, source: &str) -> ExecutionPlacement {
    match (algorithm, source) {
        (AlgorithmKind::Eval, _) => ExecutionPlacement::DirectContainerEvaluation,
        (AlgorithmKind::GoEx, _) => ExecutionPlacement::HostedOptimizersService,
        (AlgorithmKind::Sft | AlgorithmKind::Cispo, "hosted") => {
            ExecutionPlacement::RemoteTrainingService
        }
        (AlgorithmKind::Sft | AlgorithmKind::Cispo, _) => ExecutionPlacement::LocalTrainingSidecar,
        (_, "hosted") => ExecutionPlacement::HostedOptimizersService,
        _ => ExecutionPlacement::LocalPythonProcess,
    }
}

pub fn envelopes_to_producer(
    envelopes: &[OptimizerEventEnvelope],
    producer_id: &str,
) -> KernelResult<Vec<ProducerEvent>> {
    let mut events = envelopes
        .iter()
        .enumerate()
        .map(|(index, envelope)| {
            let mut event = envelope_to_producer(envelope, producer_id)?;
            event.producer_sequence = index as u64 + 1;
            if envelope.event_id.is_none() {
                event.idempotency_key =
                    format!("{}:{}", envelope.event_type, event.producer_sequence);
            }
            Ok(event)
        })
        .collect::<KernelResult<Vec<_>>>()?;

    // Some historical feeds emitted an algorithm settlement followed by the
    // canonical optimizer terminal fact. Preserve the first as algorithm
    // evidence, but let only the canonical fact own lifecycle termination.
    let last_canonical_terminal = events.iter().rposition(|event| {
        matches!(
            event.event_type.as_str(),
            "optimizer.run.completed"
                | "optimizer.run.failed"
                | "optimizer.run.degraded"
                | "optimizer.run.cancelled"
        )
    });
    if let Some(last_terminal) = last_canonical_terminal {
        for event in events.iter_mut().take(last_terminal) {
            if matches!(
                event.event_type.as_str(),
                "gepa.run.finished" | "goex.run_finished" | "go-ex.run.finished" | "run.completed"
            ) {
                if let Some(payload) = event.payload.as_object_mut() {
                    payload.insert("kernelLifecycleFact".into(), serde_json::Value::Bool(false));
                }
                *event = event.clone().with_computed_digest();
            }
        }
    }
    Ok(events)
}

/// Fold a batch of envelopes onto an existing kernel state.
///
/// The incremental twin of [`reduce_envelopes`], which always starts from
/// `RunKernelState::new` and replays everything. `commit` has always taken a
/// prior state and a batch; this is simply the call that uses it that way.
///
/// The producer log is empty for the same reason it is empty in
/// `reduce_envelopes`: the service has already decided which envelopes are new
/// — a confirmed replay never reaches here — so the batch is numbered from 1
/// and every event is an append. Cross-batch ordering rules are the caller's
/// responsibility; `can_fold_incrementally` in the service keeps the events
/// that have them on the replay path.
pub fn fold_envelopes(
    state: super::commit::RunKernelState,
    run_id: &str,
    envelopes: &[OptimizerEventEnvelope],
) -> KernelResult<super::commit::RunKernelState> {
    let events = envelopes_to_producer(envelopes, run_id)?;
    if events.is_empty() {
        return Ok(state);
    }
    let committed_at = events
        .last()
        .map(|event| event.occurred_at.as_str())
        .unwrap_or("1970-01-01T00:00:00Z");
    let plan = super::commit::commit(
        state,
        &super::sequences::DurableProducerLog::default(),
        &events,
        committed_at,
    )?;
    Ok(plan.state)
}

pub fn reduce_envelopes(
    run_id: &str,
    algorithm: AlgorithmKind,
    placement: ExecutionPlacement,
    spec_digest: &str,
    envelopes: &[OptimizerEventEnvelope],
) -> KernelResult<super::commit::RunKernelState> {
    let events = envelopes_to_producer(envelopes, run_id)?;
    let state = super::commit::RunKernelState::new(run_id, algorithm, placement, spec_digest);
    if events.is_empty() {
        return Ok(state);
    }
    let committed_at = events
        .last()
        .map(|event| event.occurred_at.as_str())
        .unwrap_or("1970-01-01T00:00:00Z");
    let plan = super::commit::commit(
        state,
        &super::sequences::DurableProducerLog::default(),
        &events,
        committed_at,
    )?;
    Ok(plan.state)
}
