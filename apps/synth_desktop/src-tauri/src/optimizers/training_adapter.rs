//! Maps native training facts onto the persisted Workshop envelope.
//!
//! Trainers own `training.event.v1`. This adapter is the only place that
//! turns those facts into `optimizer_event.v1`. Workshop then ingests the
//! mapped envelope; visuals never read a provider wire format.
//!
//! Identity rules:
//! - provider `event_id`, `attempt_id`, and source sequence are retained
//! - sequence gaps and collisions fail visibly
//! - `job.succeeded` is `training.job.completed`, never `optimizer.run.completed`
//! - the optimizer terminal is emitted only after an explicit mapping receipt,
//!   and for local MLX only after the adapter artifact is materialized

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};

use super::events::OptimizerEventDraft;
use super::training::{TrainingEvent, TRAINING_EVENT_SCHEMA_VERSION};

pub const TRAINING_TERMINAL_MAPPED: &str = "training.terminal.mapped";
pub const TRAINING_JOB_COMPLETED: &str = "training.job.completed";
pub const TRAINING_JOB_FAILED: &str = "training.job.failed";
pub const TRAINING_JOB_CANCELLED: &str = "training.job.cancelled";
pub const TRAINING_ARTIFACT_MATERIALIZED: &str = "training.artifact.materialized";

#[derive(Clone, Debug)]
pub struct AdaptedTrainingEvent {
    pub draft: OptimizerEventDraft,
    pub source_sequence: u64,
    pub source_event_id: String,
    pub attempt_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalMapping {
    pub source_status: &'static str,
    pub mapped_to: &'static str,
    pub reason: String,
}

impl TerminalMapping {
    pub fn completed_after_artifact(artifact_id: &str) -> Self {
        Self {
            source_status: "succeeded",
            mapped_to: "optimizer.run.completed",
            reason: format!("artifact {artifact_id} materialized"),
        }
    }

    pub fn completed_without_local_artifact() -> Self {
        Self {
            source_status: "succeeded",
            mapped_to: "optimizer.run.completed",
            reason: "hosted training succeeded; no local mlx-lora artifact".into(),
        }
    }

    pub fn failed(reason: &str) -> Self {
        Self {
            source_status: "failed",
            mapped_to: "optimizer.run.failed",
            reason: reason.to_string(),
        }
    }

    pub fn cancelled() -> Self {
        Self {
            source_status: "cancelled",
            mapped_to: "optimizer.run.cancelled",
            reason: "training job cancelled".into(),
        }
    }

    pub fn draft(&self, algorithm: &str) -> OptimizerEventDraft {
        OptimizerEventDraft::new(TRAINING_TERMINAL_MAPPED, algorithm).delta(Map::from_iter([
            ("sourceStatus".into(), json!(self.source_status)),
            ("mappedTo".into(), json!(self.mapped_to)),
            ("reason".into(), json!(self.reason)),
        ]))
    }
}

/// Ingest a page of provider events. Replays (`sequence <= cursor`) are
/// skipped; a hole (`sequence > cursor + 1`) is a hard error.
pub fn ingest_ordered_events(
    cursor: u64,
    events: impl IntoIterator<Item = Value>,
) -> Result<(u64, Vec<Value>)> {
    let mut cursor = cursor;
    let mut accepted = Vec::new();
    for event in events {
        let sequence =
            source_sequence(&event).ok_or_else(|| anyhow!("training event omitted sequence"))?;
        if sequence == 0 {
            bail!("training event sequence must be >= 1");
        }
        if sequence <= cursor {
            continue;
        }
        if sequence != cursor + 1 {
            bail!("training event sequence gap after {cursor}: {sequence}");
        }
        accepted.push(event);
        cursor = sequence;
    }
    Ok((cursor, accepted))
}

pub fn adapt_source_fact(algorithm: &str, event: &Value) -> Result<AdaptedTrainingEvent> {
    let fact = coerce_training_fact(event)?;
    let source_sequence = fact.sequence;
    let source_event_id = fact.event_id.clone();
    let attempt_id = fact.attempt_id.clone();
    let mut draft = mapped_event_draft(algorithm, &fact);
    draft = attach_identity(draft, &fact);
    Ok(AdaptedTrainingEvent {
        draft,
        source_sequence,
        source_event_id,
        attempt_id,
    })
}

pub fn mapped_event_type(algorithm: &str, kind: &str) -> String {
    mapped_event_draft(
        algorithm,
        &CoercedFact {
            schema_version: TRAINING_EVENT_SCHEMA_VERSION.into(),
            event_id: "probe".into(),
            job_id: "probe".into(),
            attempt_id: "attempt-1".into(),
            sequence: 1,
            occurred_at: "1970-01-01T00:00:00Z".into(),
            kind: kind.into(),
            payload: json!({}),
            producer: json!({}),
            native: json!({}),
        },
    )
    .event_type
}

struct CoercedFact {
    schema_version: String,
    event_id: String,
    job_id: String,
    attempt_id: String,
    sequence: u64,
    occurred_at: String,
    kind: String,
    payload: Value,
    producer: Value,
    native: Value,
}

fn coerce_training_fact(event: &Value) -> Result<CoercedFact> {
    if let Some(schema) = event.get("schema_version").and_then(Value::as_str) {
        if schema.starts_with("training.event.") && schema != TRAINING_EVENT_SCHEMA_VERSION {
            bail!("unsupported training event schema {schema:?}");
        }
    }
    if event.get("schema_version").and_then(Value::as_str) == Some(TRAINING_EVENT_SCHEMA_VERSION) {
        let parsed: TrainingEvent = serde_json::from_value(event.clone())
            .map_err(|error| anyhow!("invalid training.event.v1: {error}"))?;
        parsed.validate().map_err(|error| anyhow!("{error}"))?;
        return Ok(CoercedFact {
            schema_version: parsed.schema_version,
            event_id: parsed.event_id,
            job_id: parsed.job_id,
            attempt_id: parsed.attempt_id,
            sequence: parsed.sequence,
            occurred_at: parsed.occurred_at,
            kind: parsed.kind,
            payload: parsed.payload,
            producer: serde_json::to_value(&parsed.producer).unwrap_or(json!({})),
            native: event.clone(),
        });
    }

    let sequence =
        source_sequence(event).ok_or_else(|| anyhow!("training event omitted sequence"))?;
    if sequence == 0 {
        bail!("training event sequence must be >= 1");
    }
    let kind = event
        .get("kind")
        .or_else(|| event.get("type"))
        .or_else(|| event.get("event_type"))
        .and_then(Value::as_str)
        .unwrap_or("job.event")
        .to_string();
    let payload = event.get("payload").cloned().unwrap_or_else(|| json!({}));
    let job_id = event
        .get("job_id")
        .or_else(|| payload.get("job_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let event_id = event
        .get("event_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            if job_id.is_empty() {
                format!("{kind}:{sequence}")
            } else {
                format!("{job_id}:{sequence}")
            }
        });
    let attempt_id = event
        .get("attempt_id")
        .or_else(|| payload.get("attempt_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("attempt-1")
        .to_string();
    let occurred_at = event
        .get("occurred_at")
        .or_else(|| event.get("timestamp"))
        .and_then(Value::as_str)
        .unwrap_or("1970-01-01T00:00:00Z")
        .to_string();
    Ok(CoercedFact {
        schema_version: event
            .get("schema_version")
            .and_then(Value::as_str)
            .unwrap_or("sidecar.training.event")
            .to_string(),
        event_id,
        job_id,
        attempt_id,
        sequence,
        occurred_at,
        kind,
        payload,
        producer: event.get("producer").cloned().unwrap_or(json!({})),
        native: event.clone(),
    })
}

fn source_sequence(event: &Value) -> Option<u64> {
    event
        .get("sequence")
        .or_else(|| event.get("sequence_number"))
        .and_then(Value::as_u64)
        .or_else(|| {
            event
                .get("payload")
                .and_then(|payload| {
                    payload
                        .get("sequence")
                        .or_else(|| payload.get("sequence_number"))
                })
                .and_then(Value::as_u64)
        })
}

fn attach_identity(draft: OptimizerEventDraft, fact: &CoercedFact) -> OptimizerEventDraft {
    let mut delta = draft.delta.clone();
    delta.insert("attemptId".into(), json!(fact.attempt_id));
    delta.insert("sourceEventId".into(), json!(fact.event_id));
    delta.insert("sourceSequence".into(), json!(fact.sequence));
    delta.insert("sourceSchema".into(), json!(fact.schema_version));
    delta.insert(
        "trainingEventType".into(),
        json!(training_vocabulary(&fact.kind)),
    );
    draft
        .delta(delta)
        .occurred_at(fact.occurred_at.clone())
        .idempotency_key(format!("training:{}", fact.event_id))
        .raw(json!({
            "source": "training.event.v1",
            "sourceEventId": fact.event_id,
            "attemptId": fact.attempt_id,
            "sourceSequence": fact.sequence,
            "jobId": fact.job_id,
            "producer": fact.producer,
            "native": fact.native,
        }))
}

fn training_vocabulary(kind: &str) -> &'static str {
    match kind {
        "job.queued" => "training.job.queued",
        "job.started" | "job.resumed" => "training.job.started",
        "job.succeeded" | "job.completed" => TRAINING_JOB_COMPLETED,
        "job.failed" => TRAINING_JOB_FAILED,
        "job.cancelled" => TRAINING_JOB_CANCELLED,
        "training.metric" | "metric" => "training.metrics",
        "training.compute" | "compute.updated" => "training.compute.updated",
        "checkpoint.created" => "training.checkpoint.created",
        "checkpoint.ready" => "training.checkpoint.ready",
        "evaluation.completed" | "heldout_eval.completed" => "training.evaluation.completed",
        "training.dataset.validated" | "dataset.validated" => "training.dataset.validated",
        "sft.dataset.validated" => "training.dataset.validated",
        "cispo.rollout_group.completed" => "training.rollout_group.completed",
        "cispo.group_advantage.computed" => "training.group_advantage.computed",
        "cispo.zero_advantage.detected" => "training.no_learning_signal",
        _ => "training.event",
    }
}

fn mapped_event_draft(algorithm: &str, fact: &CoercedFact) -> OptimizerEventDraft {
    let kind = fact.kind.as_str();
    let payload = &fact.payload;
    match kind {
        "job.queued" => OptimizerEventDraft::new("optimizer.run.queued", algorithm)
            .delta(Map::from_iter([("status".into(), json!("queued"))])),
        "job.started" => OptimizerEventDraft::new("optimizer.run.started", algorithm)
            .delta(Map::from_iter([("status".into(), json!("running"))])),
        "job.resumed" => OptimizerEventDraft::new("optimizer.run.resumed", algorithm)
            .delta(Map::from_iter([("status".into(), json!("running"))])),
        "job.succeeded" | "job.completed" => {
            OptimizerEventDraft::new(TRAINING_JOB_COMPLETED, algorithm)
                .delta(Map::from_iter([("status".into(), json!("succeeded"))]))
        }
        "job.failed" => OptimizerEventDraft::new(TRAINING_JOB_FAILED, algorithm)
            .level("error")
            .delta(Map::from_iter([("status".into(), json!("failed"))]))
            .error(payload.clone()),
        "job.cancelled" => OptimizerEventDraft::new(TRAINING_JOB_CANCELLED, algorithm)
            .delta(Map::from_iter([("status".into(), json!("cancelled"))])),
        "training.metric" | "metric" => {
            let event_type = if algorithm == "cispo" {
                "training.metrics"
            } else {
                "sft.training.metrics"
            };
            let mut delta = if algorithm == "cispo" {
                cispo_metric_delta(payload)
            } else {
                sft_metric_delta(payload)
            };
            if delta.get("train_loss").is_none_or(|value| value.is_null()) {
                delta.insert("train_loss".into(), payload["loss"].clone());
            }
            OptimizerEventDraft::new(event_type, algorithm).delta(delta)
        }
        "training.dataset.validated" | "dataset.validated" | "sft.dataset.validated" => {
            let mut delta = payload.as_object().cloned().unwrap_or_default();
            if !delta.contains_key("dataset_digest") {
                if let Some(digest) = payload
                    .get("dataset_sha256")
                    .or_else(|| payload.get("sha256"))
                    .or_else(|| payload.get("digest"))
                    .or_else(|| payload.pointer("/manifest/digest"))
                {
                    delta.insert("dataset_digest".into(), digest.clone());
                }
            }
            OptimizerEventDraft::new("sft.dataset.validated", algorithm).delta(delta)
        }
        "checkpoint.created" | "checkpoint.ready" => checkpoint_ready_draft(algorithm, payload),
        "evaluation.completed" | "heldout_eval.completed" => {
            OptimizerEventDraft::new("sft.heldout_evaluation.completed", algorithm)
                .delta(Map::from_iter([
                    ("kind".into(), json!(kind)),
                    ("evaluation".into(), payload.clone()),
                ]))
                .item(payload.clone())
        }
        "training.clip" | "cispo.clip.identity" => {
            OptimizerEventDraft::new("cispo.clip.identity", algorithm).delta(clip_delta(payload))
        }
        "cispo.no_learning_signal" => {
            OptimizerEventDraft::new("cispo.no_learning_signal", algorithm)
                .level("error")
                .error(payload.clone())
        }
        "sft.step.metrics" | "sft.training.metrics" | "training.step.metrics" => {
            OptimizerEventDraft::new("sft.training.metrics", algorithm)
                .delta(sft_metric_delta(payload))
        }
        "cispo.update.completed"
        | "cispo.step.metrics"
        | "cispo.training.metrics"
        | "cispo.importance_ratio.measured" => {
            OptimizerEventDraft::new("training.metrics", algorithm)
                .delta(cispo_metric_delta(payload))
        }
        "cispo.rollout_group.completed" => {
            let mut delta = payload.as_object().cloned().unwrap_or_default();
            if let Some(group_id) = payload.get("group_id").or_else(|| payload.get("groupId")) {
                delta.insert("groupId".into(), group_id.clone());
                delta.insert("workItemId".into(), group_id.clone());
            }
            OptimizerEventDraft::new("cispo.rollout_group.completed", algorithm).delta(delta)
        }
        "cispo.group_advantage.computed" => {
            let mut delta = payload.as_object().cloned().unwrap_or_default();
            if let Some(group_id) = payload.get("group_id").or_else(|| payload.get("groupId")) {
                delta.insert("groupId".into(), group_id.clone());
                delta.insert("workItemId".into(), group_id.clone());
            }
            if let Some(advantages) = payload.get("advantages").and_then(Value::as_array) {
                let values = advantages
                    .iter()
                    .filter_map(Value::as_f64)
                    .collect::<Vec<_>>();
                if !values.is_empty() {
                    delta.insert(
                        "meanAdvantage".into(),
                        json!(values.iter().sum::<f64>() / values.len() as f64),
                    );
                }
            }
            OptimizerEventDraft::new("cispo.rollout_group.completed", algorithm).delta(delta)
        }
        "cispo.zero_advantage.detected" => {
            OptimizerEventDraft::new("cispo.no_learning_signal", algorithm)
                .delta(payload.as_object().cloned().unwrap_or_default())
        }
        "sft.checkpoint.created" | "sft.checkpoint.ready" | "cispo.checkpoint.created" => {
            checkpoint_ready_draft(algorithm, payload)
        }
        "sft.checkpoint.promoted" | "cispo.checkpoint.promoted" => {
            let mut delta = payload.as_object().cloned().unwrap_or_default();
            if !delta.contains_key("checkpointId") {
                if let Some(id) = payload.get("checkpoint_id").or_else(|| payload.get("id")) {
                    delta.insert("checkpointId".into(), id.clone());
                }
            }
            // SFT and CISPO share the checkpoint/selection projection. Keep a
            // single canonical event name so the visual can show a selected
            // checkpoint without treating selection as an uplift claim.
            OptimizerEventDraft::new("sft.checkpoint.promoted", algorithm).delta(delta)
        }
        "sft.checkpoint_eval.completed"
        | "sft.heldout_eval.completed"
        | "cispo.checkpoint_eval.completed"
        | "cispo.heldout_eval.completed"
        | "sft.checkpoint_evaluation.completed" => {
            OptimizerEventDraft::new("sft.heldout_evaluation.completed", algorithm)
                .delta(Map::from_iter([
                    ("kind".into(), json!(kind)),
                    ("evaluation".into(), payload.clone()),
                ]))
                .item(payload.clone())
        }
        "sft.completed" | "cispo.completed" => {
            OptimizerEventDraft::new(TRAINING_JOB_COMPLETED, algorithm)
                .delta(Map::from_iter([("status".into(), json!("succeeded"))]))
        }
        "sft.model.materialized" | "sft.adapter.materialized" | "cispo.model.materialized" => {
            let mut delta = payload.as_object().cloned().unwrap_or_default();
            if !delta.contains_key("adapterId") {
                if let Some(id) = payload
                    .get("adapter_id")
                    .or_else(|| payload.get("artifact_id"))
                    .or_else(|| payload.get("checkpoint_id"))
                    .or_else(|| payload.get("id"))
                {
                    delta.insert("adapterId".into(), id.clone());
                }
            }
            OptimizerEventDraft::new("sft.model.materialized", algorithm).delta(delta)
        }
        "sft.failed" | "cispo.failed" => OptimizerEventDraft::new(TRAINING_JOB_FAILED, algorithm)
            .level("error")
            .delta(Map::from_iter([("status".into(), json!("failed"))]))
            .error(payload.clone()),
        "sft.cancelled" | "cispo.cancelled" => {
            OptimizerEventDraft::new(TRAINING_JOB_CANCELLED, algorithm)
                .delta(Map::from_iter([("status".into(), json!("cancelled"))]))
        }
        _ => OptimizerEventDraft::new(format!("training.{kind}"), algorithm),
    }
}

fn sft_metric_delta(payload: &Value) -> Map<String, Value> {
    Map::from_iter([
        ("step".into(), metric_step(payload)),
        (
            "train_loss".into(),
            metric_number(payload, &["train_loss", "trainLoss", "loss"]),
        ),
        (
            "learning_rate".into(),
            metric_number(payload, &["learning_rate", "learningRate", "lr"]),
        ),
        (
            "throughput".into(),
            metric_number(payload, &["tokens_per_second", "tokensPerSecond"]),
        ),
    ])
}

fn cispo_metric_delta(payload: &Value) -> Map<String, Value> {
    Map::from_iter([
        ("step".into(), metric_step(payload)),
        (
            "train_loss".into(),
            metric_number(payload, &["train_loss", "trainLoss", "loss"]),
        ),
        (
            "reward".into(),
            metric_number(payload, &["reward_mean", "mean_reward", "reward"]),
        ),
        (
            "mean_reward".into(),
            metric_number(payload, &["reward_mean", "mean_reward", "reward"]),
        ),
        (
            "reward_variance".into(),
            metric_number(payload, &["reward_variance", "rewardVariance"]),
        ),
        (
            "advantage_mean".into(),
            metric_number(payload, &["advantage_mean", "mean_advantage", "advantage"]),
        ),
        (
            "advantage_std".into(),
            metric_number(payload, &["advantage_std", "advantageStd"]),
        ),
        (
            "group_size".into(),
            metric_number(payload, &["group_size", "groupSize", "group_count"]),
        ),
        (
            "optimizer_step".into(),
            metric_number(
                payload,
                &["optimizer_step", "optimizerStep", "update", "step"],
            ),
        ),
    ])
}

fn clip_delta(payload: &Value) -> Map<String, Value> {
    let clip = payload
        .get("clip")
        .cloned()
        .unwrap_or_else(|| payload.clone());
    Map::from_iter([
        ("clip".into(), clip.clone()),
        (
            "identity".into(),
            payload
                .get("identity")
                .cloned()
                .unwrap_or_else(|| json!("cispo.slime.v1")),
        ),
        ("config".into(), clip),
    ])
}

fn checkpoint_ready_draft(algorithm: &str, payload: &Value) -> OptimizerEventDraft {
    let checkpoint_id = payload
        .get("checkpoint_id")
        .or_else(|| payload.get("checkpointId"))
        .cloned()
        .unwrap_or(Value::Null);
    let digest = payload
        .get("sha256")
        .or_else(|| payload.get("digest"))
        .cloned()
        .unwrap_or(Value::Null);
    OptimizerEventDraft::new("sft.checkpoint.ready", algorithm)
        .delta(Map::from_iter([
            ("checkpointId".into(), checkpoint_id.clone()),
            ("checkpoint_id".into(), checkpoint_id.clone()),
        ]))
        .item(json!({
            "id": checkpoint_id,
            "step": payload.get("step").cloned().unwrap_or_else(|| payload["update"].clone()),
            "status": "ready",
            "ready": true,
            "path": payload["path"],
            "sha256": digest,
            "bytes": payload["bytes"],
            "kind": payload.get("kind").cloned().unwrap_or_else(|| json!("mlx-lora.v1")),
            "baseModel": payload.get("base_model"),
            "raw": payload
        }))
        .artifact_refs(vec![json!({
            "kind": "checkpoint",
            "id": checkpoint_id,
            "uri": payload["path"],
            "digest": digest
        })])
}

fn metric_step(payload: &Value) -> Value {
    payload
        .get("step")
        .or_else(|| payload.get("update"))
        .cloned()
        .unwrap_or(Value::Null)
}

fn metric_number(payload: &Value, keys: &[&str]) -> Value {
    for key in keys {
        if let Some(value) = payload.get(*key) {
            if !value.is_null() {
                return value.clone();
            }
        }
    }
    if let Some(metrics) = payload.get("metrics") {
        for key in keys {
            if let Some(value) = metrics.get(*key) {
                if !value.is_null() {
                    return value.clone();
                }
            }
        }
    }
    Value::Null
}

pub fn promote_hosted_fact(event: Value) -> Result<Value> {
    if event.get("schema_version").and_then(Value::as_str) == Some(TRAINING_EVENT_SCHEMA_VERSION) {
        return Ok(event);
    }
    let sequence =
        source_sequence(&event).ok_or_else(|| anyhow!("hosted training event omitted sequence"))?;
    let event_id = event.get("event_id").cloned();
    let attempt_id = event.get("attempt_id").cloned();
    let kind = event
        .get("kind")
        .cloned()
        .or_else(|| event.get("type").cloned())
        .or_else(|| event.get("event_type").cloned())
        .unwrap_or_else(|| json!("hosted.event"));
    Ok(json!({
        "sequence": sequence,
        "event_id": event_id,
        "attempt_id": attempt_id,
        "type": kind,
        "kind": kind,
        "occurred_at": event.get("occurred_at").or_else(|| event.get("timestamp")).cloned(),
        "payload": event.get("payload").cloned().unwrap_or_else(|| event.clone()),
        "producer": event.get("producer").cloned().unwrap_or(json!({})),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_event(sequence: u64, kind: &str, payload: Value) -> Value {
        json!({
            "schema_version": "training.event.v1",
            "event_id": format!("evt-{sequence}"),
            "job_id": "job-1",
            "attempt_id": "attempt-7",
            "sequence": sequence,
            "occurred_at": "2026-08-20T16:00:00Z",
            "kind": kind,
            "phase": "running",
            "payload": payload,
            "producer": {
                "service": "synth-mlx-rl",
                "version": "0.1.0",
                "commit": "deadbeef"
            }
        })
    }

    #[test]
    fn native_identity_is_retained_on_the_mapped_envelope() {
        let adapted = adapt_source_fact(
            "sft",
            &native_event(3, "training.metric", json!({"step": 2, "loss": 0.4})),
        )
        .unwrap();
        assert_eq!(adapted.source_event_id, "evt-3");
        assert_eq!(adapted.attempt_id, "attempt-7");
        assert_eq!(adapted.source_sequence, 3);
        assert_eq!(adapted.draft.event_type, "sft.training.metrics");
        assert_eq!(adapted.draft.delta["sourceEventId"], "evt-3");
        assert_eq!(adapted.draft.delta["attemptId"], "attempt-7");
        assert_eq!(adapted.draft.delta["sourceSequence"], 3);
        assert_eq!(adapted.draft.delta["trainingEventType"], "training.metrics");
        assert_eq!(
            adapted.draft.idempotency_key.as_deref(),
            Some("training:evt-3")
        );
        let sealed = adapted.draft.seal("run-1", 3, "2026-08-20T16:00:01Z");
        assert_eq!(sealed.schema_version, "optimizer_event.v1");
        assert_eq!(sealed.event_id.as_deref(), Some("run-1:training:evt-3"));
        assert_eq!(sealed.occurred_at, "2026-08-20T16:00:00Z");
    }

    #[test]
    fn job_succeeded_is_not_optimizer_run_completed() {
        let adapted =
            adapt_source_fact("sft", &native_event(8, "job.succeeded", json!({}))).unwrap();
        assert_eq!(adapted.draft.event_type, TRAINING_JOB_COMPLETED);
        assert_ne!(adapted.draft.event_type, "optimizer.run.completed");
    }

    #[test]
    fn cispo_metrics_use_the_shared_training_vocabulary() {
        let adapted = adapt_source_fact(
            "cispo",
            &json!({
                "sequence": 2,
                "type": "training.metric",
                "timestamp": "2026-08-20T16:00:00Z",
                "event_id": "mlx:2",
                "attempt_id": "attempt-1",
                "payload": {"step": 1, "loss": 0.9}
            }),
        )
        .unwrap();
        assert_eq!(adapted.draft.event_type, "training.metrics");
        assert_eq!(adapted.draft.delta["trainingEventType"], "training.metrics");
    }

    #[test]
    fn public_cispo_evidence_maps_to_kernel_event_shapes() {
        let rollout = adapt_source_fact(
            "cispo",
            &native_event(
                2,
                "cispo.rollout_group.completed",
                json!({"group_id": "1:0", "rewards": [0.0, 0.0]}),
            ),
        )
        .unwrap();
        assert_eq!(rollout.draft.event_type, "cispo.rollout_group.completed");
        assert_eq!(rollout.draft.delta["groupId"], "1:0");
        assert_eq!(rollout.draft.delta["workItemId"], "1:0");

        let checkpoint = adapt_source_fact(
            "cispo",
            &native_event(
                6,
                "cispo.checkpoint.created",
                json!({
                    "checkpoint_id": "ckpt_1_inference",
                    "digest": "sha256:abc",
                    "step": 1
                }),
            ),
        )
        .unwrap();
        assert_eq!(checkpoint.draft.event_type, "sft.checkpoint.ready");
        assert_eq!(checkpoint.draft.delta["checkpointId"], "ckpt_1_inference");
        assert_eq!(
            checkpoint.draft.item.as_ref().unwrap()["sha256"],
            "sha256:abc"
        );
    }

    #[test]
    fn public_sft_dataset_digest_maps_to_kernel_event_shape() {
        let dataset = adapt_source_fact(
            "sft",
            &native_event(
                1,
                "sft.dataset.validated",
                json!({"dataset_sha256": "sha256:banking77"}),
            ),
        )
        .unwrap();
        assert_eq!(dataset.draft.event_type, "sft.dataset.validated");
        assert_eq!(dataset.draft.delta["dataset_digest"], "sha256:banking77");

        let nested = adapt_source_fact(
            "sft",
            &native_event(
                1,
                "sft.dataset.validated",
                json!({"manifest": {"digest": "sha256:manifest"}}),
            ),
        )
        .unwrap();
        assert_eq!(nested.draft.delta["dataset_digest"], "sha256:manifest");
    }

    #[test]
    fn public_sft_terminal_artifacts_map_to_kernel_event_shapes() {
        let promoted = adapt_source_fact(
            "sft",
            &native_event(
                39,
                "sft.checkpoint.promoted",
                json!({"checkpoint_id": "ckpt_10_inference", "digest": "sha256:chosen"}),
            ),
        )
        .unwrap();
        assert_eq!(promoted.draft.event_type, "sft.checkpoint.promoted");
        assert_eq!(promoted.draft.delta["checkpointId"], "ckpt_10_inference");

        let materialized = adapt_source_fact(
            "sft",
            &native_event(
                41,
                "sft.model.materialized",
                json!({"checkpoint_id": "ckpt_10_inference", "digest": "sha256:model"}),
            ),
        )
        .unwrap();
        assert_eq!(materialized.draft.event_type, "sft.model.materialized");
        assert_eq!(materialized.draft.delta["adapterId"], "ckpt_10_inference");
    }

    #[test]
    fn public_cispo_selection_keeps_checkpoint_evidence() {
        let promoted = adapt_source_fact(
            "cispo",
            &native_event(
                8,
                "cispo.checkpoint.promoted",
                json!({
                    "checkpoint_id": "ckpt_1_inference",
                    "calibration_accuracy": 0.0,
                    "digest": "sha256:chosen"
                }),
            ),
        )
        .unwrap();
        assert_eq!(promoted.draft.event_type, "sft.checkpoint.promoted");
        assert_eq!(promoted.draft.delta["checkpointId"], "ckpt_1_inference");
        assert_eq!(promoted.draft.delta["calibration_accuracy"], 0.0);

        let materialized = adapt_source_fact(
            "cispo",
            &native_event(
                10,
                "cispo.model.materialized",
                json!({"checkpoint_id": "ckpt_1_inference", "digest": "sha256:model"}),
            ),
        )
        .unwrap();
        assert_eq!(materialized.draft.event_type, "sft.model.materialized");
        assert_eq!(materialized.draft.delta["adapterId"], "ckpt_1_inference");
    }

    #[test]
    fn sequence_gaps_fail_visibly() {
        let error = ingest_ordered_events(
            2,
            vec![
                json!({"sequence": 3, "type": "training.metric"}),
                json!({"sequence": 5, "type": "training.metric"}),
            ],
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("sequence gap after 3: 5"), "{error}");
    }

    #[test]
    fn sequence_replays_are_skipped_not_rewritten() {
        let (cursor, accepted) = ingest_ordered_events(
            2,
            vec![
                json!({"sequence": 1, "type": "job.started"}),
                json!({"sequence": 2, "type": "training.metric"}),
                json!({"sequence": 3, "type": "checkpoint.created"}),
            ],
        )
        .unwrap();
        assert_eq!(cursor, 3);
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0]["type"], "checkpoint.created");
    }

    #[test]
    fn hosted_wrap_keeps_provider_identity() {
        let promoted = promote_hosted_fact(json!({
            "event_id": "hosted-evt-4",
            "attempt_id": "att-2",
            "sequence_number": 4,
            "type": "sft.training.metrics",
            "payload": {"step": 4}
        }))
        .unwrap();
        let adapted = adapt_source_fact("sft", &promoted).unwrap();
        assert_eq!(adapted.source_event_id, "hosted-evt-4");
        assert_eq!(adapted.attempt_id, "att-2");
        assert_eq!(adapted.source_sequence, 4);
    }

    #[test]
    fn public_sft_step_metrics_flatten_nested_loss() {
        let adapted = adapt_source_fact(
            "sft",
            &native_event(
                4,
                "sft.step.metrics",
                json!({"step": 2, "metrics": {"loss": 0.31}, "tokens": 16}),
            ),
        )
        .unwrap();
        assert_eq!(adapted.draft.event_type, "sft.training.metrics");
        assert_eq!(adapted.draft.delta["train_loss"], 0.31);
        assert_eq!(adapted.draft.delta["step"], 2);
    }

    #[test]
    fn public_cispo_update_maps_to_training_metrics() {
        let adapted = adapt_source_fact(
            "cispo",
            &json!({
                "schema_version": "training.event.v1",
                "event_id": "evt-9",
                "job_id": "cispo_public",
                "attempt_id": "attempt-1",
                "sequence": 9,
                "occurred_at": "2026-09-02T14:00:00Z",
                "kind": "cispo.update.completed",
                "phase": "running",
                "payload": {
                    "update": 1,
                    "reward_mean": 0.5,
                    "reward_variance": 0.08,
                    "group_count": 2
                },
                "producer": {"service": "synth-optimizers", "version": "v1", "commit": "local"}
            }),
        )
        .unwrap();
        assert_eq!(adapted.draft.event_type, "training.metrics");
        assert_eq!(adapted.draft.delta["step"], 1);
        assert_eq!(adapted.draft.delta["reward_variance"], 0.08);
    }

    #[test]
    fn public_completed_is_not_optimizer_run_completed() {
        let adapted =
            adapt_source_fact("sft", &native_event(8, "sft.completed", json!({}))).unwrap();
        assert_eq!(adapted.draft.event_type, TRAINING_JOB_COMPLETED);
        assert_ne!(adapted.draft.event_type, "optimizer.run.completed");
    }

    #[test]
    fn terminal_mapping_names_the_optimizer_event() {
        let mapping = TerminalMapping::completed_after_artifact("art_abc");
        let draft = mapping.draft("sft");
        assert_eq!(draft.event_type, TRAINING_TERMINAL_MAPPED);
        assert_eq!(draft.delta["mappedTo"], "optimizer.run.completed");
        assert!(draft.delta["reason"].as_str().unwrap().contains("art_abc"));
    }

    #[test]
    fn native_schema_mismatch_is_a_visible_error() {
        let mut event = native_event(1, "job.started", json!({}));
        event["schema_version"] = json!("training.event.v0");
        let error = adapt_source_fact("sft", &event).unwrap_err().to_string();
        assert!(
            error.contains("unsupported training event schema"),
            "{error}"
        );
    }
}
