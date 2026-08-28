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
        _ => "training.event",
    }
}

/// Every field a training runtime may report on a `training.metric` fact.
///
/// Left is the persisted delta key the projection reads; right is every wire
/// spelling a producer has been seen to use. This is the single widening point
/// for both mapping paths — `sidecar_training::mapped_event_draft` calls it too,
/// so the sidecar and hosted arms cannot drift apart field by field.
///
/// Absence survives the pipeline. A field the runtime did not report is not
/// inserted at all, so it reaches the renderer as a gap rather than as a
/// fabricated zero; a reported `0.0` is forwarded as `0.0` and means zero.
///
/// The CISPO aggregates below have no producer in this repository today
/// (`synth-mlx-rl` is a pinned external wheel; the Tinker and slime lanes are
/// dark). They are forwarded so that when a runtime starts reporting them the
/// panel lights up without a Desktop release — never so that the panel can
/// claim them before then.
const TRAINING_METRIC_FIELDS: &[(&str, &[&str])] = &[
    ("step", &["step", "global_step"]),
    ("epoch", &["epoch"]),
    ("train_loss", &["loss", "train_loss"]),
    ("train_loss_coverage", &["train_loss_coverage"]),
    ("validation_loss", &["validation_loss", "valid_loss"]),
    ("validation_loss_coverage", &["validation_loss_coverage"]),
    ("learning_rate", &["learning_rate"]),
    ("throughput", &["tokens_per_second", "throughput"]),
    // CISPO aggregates — read by `projectEvents.ts` into `projected.cispo`.
    ("group_size", &["group_size"]),
    ("reward_variance", &["reward_variance"]),
    ("advantage_mean", &["advantage_mean"]),
    ("advantage_std", &["advantage_std", "advantage_sd"]),
    ("optimizer_step", &["optimizer_step"]),
];

/// Map one `training.metric` payload onto the persisted delta.
pub(super) fn training_metric_delta(payload: &Value) -> Map<String, Value> {
    let mut delta = Map::new();
    for (key, aliases) in TRAINING_METRIC_FIELDS {
        if let Some(value) = training_metric_field(payload, aliases) {
            delta.insert((*key).into(), value);
        }
    }
    delta
}

/// Payload root first, then the `metrics` sub-object that `training.event.v1`
/// nests them under (`fixtures/training_event_v1.json`). A JSON `null` is an
/// absent field, not a value.
fn training_metric_field(payload: &Value, aliases: &[&str]) -> Option<Value> {
    let nested = payload.get("metrics");
    for alias in aliases {
        for source in [Some(payload), nested].into_iter().flatten() {
            if let Some(value) = source.get(*alias) {
                if !value.is_null() {
                    return Some(value.clone());
                }
            }
        }
    }
    None
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
            OptimizerEventDraft::new(event_type, algorithm).delta(training_metric_delta(payload))
        }
        "checkpoint.created" | "checkpoint.ready" => {
            OptimizerEventDraft::new("sft.checkpoint.ready", algorithm)
                .item(json!({
                    "id": payload["checkpoint_id"],
                    "step": payload["step"],
                    "status": "ready",
                    "ready": true,
                    "path": payload["path"],
                    "sha256": payload["sha256"],
                    "bytes": payload["bytes"],
                    "kind": payload.get("kind").cloned().unwrap_or_else(|| json!("mlx-lora.v1")),
                    "baseModel": payload.get("base_model"),
                    "raw": payload
                }))
                .artifact_refs(vec![json!({
                    "kind": "checkpoint",
                    "id": payload["checkpoint_id"],
                    "uri": payload["path"],
                    "digest": payload["sha256"]
                })])
        }
        "evaluation.completed" | "heldout_eval.completed" => {
            OptimizerEventDraft::new("sft.heldout_evaluation.completed", algorithm)
                .delta(Map::from_iter([
                    ("kind".into(), json!(kind)),
                    ("evaluation".into(), payload.clone()),
                ]))
                .item(payload.clone())
        }
        "training.clip" => OptimizerEventDraft::new("cispo.clip.identity", algorithm)
            .delta(Map::from_iter([("clip".into(), payload.clone())])),
        "cispo.no_learning_signal" => {
            OptimizerEventDraft::new("cispo.no_learning_signal", algorithm)
                .level("error")
                .error(payload.clone())
        }
        _ => OptimizerEventDraft::new(format!("training.{kind}"), algorithm),
    }
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
    fn cispo_step_aggregates_survive_the_metric_mapping() {
        let adapted = adapt_source_fact(
            "cispo",
            &native_event(
                4,
                "training.metric",
                json!({
                    "step": 7,
                    "epoch": 1,
                    "loss": 0.4,
                    "learning_rate": 0.00005,
                    "tokens_per_second": 64.0,
                    "group_size": 16,
                    "reward_variance": 0.12,
                    "advantage_mean": 0.08,
                    "advantage_std": 0.31,
                    "optimizer_step": 7
                }),
            ),
        )
        .unwrap();
        let delta = &adapted.draft.delta;
        assert_eq!(delta["step"], json!(7));
        assert_eq!(delta["epoch"], json!(1));
        assert_eq!(delta["train_loss"], json!(0.4));
        assert_eq!(delta["learning_rate"], json!(0.00005));
        assert_eq!(delta["throughput"], json!(64.0));
        assert_eq!(delta["group_size"], json!(16));
        assert_eq!(delta["reward_variance"], json!(0.12));
        assert_eq!(delta["advantage_mean"], json!(0.08));
        assert_eq!(delta["advantage_std"], json!(0.31));
        assert_eq!(delta["optimizer_step"], json!(7));
    }

    #[test]
    fn a_field_the_runtime_never_reported_stays_absent_not_zero() {
        // Today's real MLX `training.metric` payload. No runtime in this repo
        // reports the CISPO aggregates yet; the mapping must not invent them.
        let adapted = adapt_source_fact(
            "cispo",
            &native_event(
                5,
                "training.metric",
                json!({
                    "step": 2,
                    "epoch": 1,
                    "loss": 0.42,
                    "learning_rate": 0.00005,
                    "tokens": 128.0,
                    "step_seconds": 2.0,
                    "tokens_per_second": 64.0,
                    "memory_bytes": 1048576
                }),
            ),
        )
        .unwrap();
        let delta = &adapted.draft.delta;
        for absent in [
            "group_size",
            "reward_variance",
            "advantage_mean",
            "advantage_std",
            "optimizer_step",
            "validation_loss",
        ] {
            assert!(
                delta.get(absent).is_none(),
                "{absent} must be absent, not a fabricated value: {:?}",
                delta.get(absent)
            );
        }
        assert_eq!(delta["step"], json!(2));
        assert_eq!(delta["train_loss"], json!(0.42));
    }

    #[test]
    fn an_explicit_null_is_absence_and_a_reported_zero_is_a_value() {
        let delta = training_metric_delta(&json!({
            "step": 3,
            "loss": null,
            "reward_variance": 0.0
        }));
        assert!(delta.get("train_loss").is_none(), "null is not a value");
        assert_eq!(delta["reward_variance"], json!(0.0), "0.0 is a value");
    }

    #[test]
    fn nested_metric_payloads_are_read_too() {
        // `training.event.v1` nests them: fixtures/training_event_v1.json.
        let delta = training_metric_delta(&json!({
            "metrics": {"train_loss": 0.75, "advantage_mean": 0.02},
            "policy_version": "checkpoint:step-4"
        }));
        assert_eq!(delta["train_loss"], json!(0.75));
        assert_eq!(delta["advantage_mean"], json!(0.02));
        assert!(delta.get("step").is_none());
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
