//! Algorithm-specific SFT result materialization.
//!
//! SFT results are typed from the durable event stream / run directory. They
//! never read `best_candidate.json` and never invent GEPA-shaped winners.
//! Missing metrics serialize as `null` plus coverage, never numeric zero.

use super::models::{OptimizerEventEnvelope, OptimizerRunRecord, TrainingJobStatus};
use super::training_adapter::is_step_metrics_event;
use super::OptimizerService;
use anyhow::Result;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub async fn materialize_sft_result(
    service: &OptimizerService,
    run: &OptimizerRunRecord,
) -> Result<Value> {
    let mut events = service.events_after(run.id.clone(), 0, Some(2_000)).await?;
    if events.is_empty() {
        events.extend(events_from_run_dir(run));
    }
    let manifest = manifest_from_run_dir(run);
    Ok(project_sft_result(run, &events, manifest.as_ref()))
}

pub fn project_sft_result(
    run: &OptimizerRunRecord,
    events: &[OptimizerEventEnvelope],
    manifest: Option<&Value>,
) -> Value {
    let dataset = dataset_from(run, events, manifest);
    let config = config_from(run, events, manifest);
    let metrics = metrics_from(events);
    let checkpoints = checkpoints_from(events, manifest);
    let evaluations = evaluations_from(events);
    let selected = selected_checkpoint_from(events, manifest);
    let verdict = improvement_verdict_from(events, manifest, &evaluations);
    let artifacts = artifacts_from(events, manifest);
    let usage = usage_from(run, events);
    json!({
        "schemaVersion": "optimizer_result.v1",
        "resultType": "sft",
        "optimizerRunId": run.id,
        "algorithmId": "sft",
        "status": run.status,
        "finalCursor": run.cursor_seq,
        "dataset": dataset,
        "config": config,
        "metrics": metrics,
        "checkpoints": checkpoints,
        "evaluations": evaluations,
        "selectedCheckpoint": selected,
        "improvementVerdict": verdict,
        "terminalLabel": terminal_label(&run.status, verdict.as_str()),
        "artifacts": artifacts,
        "usage": usage,
        "reconciliationStatus": {
            "status": if events.is_empty() { "incomplete" } else { "aligned" },
            "authorities": {
                "events": "optimizer_event.v1",
                "manifest": manifest.is_some(),
            }
        },
        "completionReceiptId": format!("optimizer_completion_{}", run.id)
    })
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub fn validate_dataset_digest(path: &std::path::Path, claimed: &str) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let computed = sha256_bytes(&bytes);
    let claimed = normalize_digest(claimed);
    if claimed != computed {
        anyhow::bail!("metadata.dataset_digest {claimed} does not match staged bytes {computed}");
    }
    Ok(computed)
}

fn normalize_digest(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(hex) = trimmed
        .strip_prefix("sha256:")
        .or_else(|| trimmed.strip_prefix("SHA256:"))
    {
        format!("sha256:{}", hex.trim().to_ascii_lowercase())
    } else {
        format!("sha256:{}", trimmed.to_ascii_lowercase())
    }
}

fn events_from_run_dir(run: &OptimizerRunRecord) -> Vec<OptimizerEventEnvelope> {
    let Some(dir) = run_dir(run) else {
        return Vec::new();
    };
    let path = dir.join("events.jsonl");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<OptimizerEventEnvelope>(line).ok())
        .collect()
}

fn manifest_from_run_dir(run: &OptimizerRunRecord) -> Option<Value> {
    let dir = run_dir(run)?;
    serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).ok()?).ok()
}

fn run_dir(run: &OptimizerRunRecord) -> Option<std::path::PathBuf> {
    run.summary
        .get("runDirectory")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
}

fn dataset_from(
    run: &OptimizerRunRecord,
    events: &[OptimizerEventEnvelope],
    manifest: Option<&Value>,
) -> Value {
    let digest = events
        .iter()
        .rev()
        .find(|event| event.event_type == "sft.dataset.validated")
        .and_then(|event| {
            event
                .delta
                .get("dataset_digest")
                .or_else(|| event.item.as_ref().and_then(|item| item.get("id")))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| {
            manifest
                .and_then(|value| value.get("dataset_digest"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| {
            run.summary
                .get("datasetDigest")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let rows = last_u64(events, &["dataset_rows", "rows"]);
    json!({
        "digest": digest,
        "rows": coverage_num(rows),
        "tokens": coverage_num(training_tokens(events)),
    })
}

fn config_from(
    run: &OptimizerRunRecord,
    events: &[OptimizerEventEnvelope],
    manifest: Option<&Value>,
) -> Value {
    let created = events
        .iter()
        .find(|event| event.event_type == "optimizer.run.created");
    let summary = created
        .and_then(|event| event.snapshot.as_ref())
        .and_then(|snapshot| snapshot.get("summary"))
        .cloned()
        .unwrap_or_else(|| run.summary.clone());
    let rank = summary
        .get("rank")
        .and_then(Value::as_u64)
        .or_else(|| {
            manifest
                .and_then(|value| value.get("rank"))
                .and_then(Value::as_u64)
        })
        .or_else(|| parse_lora_rank(summary.get("adapter").and_then(Value::as_str)))
        .unwrap_or(8);
    let adapter = summary
        .get("adapter")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("lora_r{rank}"));
    json!({
        "baseModel": summary.get("baseModel").cloned().unwrap_or(Value::Null),
        "adapter": adapter,
        "rank": rank,
        "backend": summary.get("backend").cloned().unwrap_or(Value::Null),
    })
}

fn parse_lora_rank(adapter: Option<&str>) -> Option<u64> {
    adapter?
        .strip_prefix("lora_r")?
        .parse::<u64>()
        .ok()
        .filter(|rank| *rank > 0)
}

fn metrics_from(events: &[OptimizerEventEnvelope]) -> Value {
    let mut train_loss = None;
    let mut train_coverage = "missing";
    let mut validation_loss = None;
    let mut validation_coverage = "missing";
    for event in events {
        // Not a name list. `training_adapter` owns the one rule that names a
        // step-metrics event and the closed set of names one can carry, so a
        // CISPO run reaches this reader on either placement.
        if !is_step_metrics_event(&event.event_type) {
            continue;
        }
        if let Some(value) = json_f64(event.delta.get("train_loss")) {
            train_loss = Some(value);
            train_coverage = "present";
        } else if event.delta.get("train_loss").is_some() {
            train_coverage = event
                .delta
                .get("train_loss_coverage")
                .and_then(Value::as_str)
                .unwrap_or("missing");
        }
        if let Some(value) = json_f64(event.delta.get("validation_loss"))
            .or_else(|| json_f64(event.delta.get("valid_loss")))
        {
            validation_loss = Some(value);
            validation_coverage = "present";
        } else if event
            .delta
            .get("validation_loss_coverage")
            .and_then(Value::as_str)
            == Some("unsupported")
        {
            validation_coverage = "unsupported";
        } else if event.delta.get("validation_loss").is_some() {
            validation_coverage = "missing";
        }
    }
    json!({
        "trainLoss": coverage_f64(train_loss, train_coverage),
        "validationLoss": coverage_f64(validation_loss, validation_coverage),
    })
}

fn checkpoints_from(events: &[OptimizerEventEnvelope], manifest: Option<&Value>) -> Value {
    let mut by_id = Map::new();
    for event in events {
        if !event.event_type.starts_with("sft.checkpoint.") {
            continue;
        }
        let id = event
            .delta
            .get("checkpoint_id")
            .and_then(Value::as_str)
            .or_else(|| {
                event
                    .item
                    .as_ref()
                    .and_then(|item| item.get("id"))
                    .and_then(Value::as_str)
            });
        let Some(id) = id else {
            continue;
        };
        let entry = by_id.entry(id.to_string()).or_insert_with(|| {
            json!({
                "id": id,
                "step": event.delta.get("step"),
                "digest": event.delta.get("digest"),
                "ready": false,
                "selected": false,
                "promoted": false,
            })
        });
        if let Some(object) = entry.as_object_mut() {
            if event.event_type == "sft.checkpoint.ready" {
                object.insert("ready".into(), json!(true));
            }
            if event.event_type == "sft.checkpoint.selected" {
                object.insert("selected".into(), json!(true));
            }
            if event.event_type == "sft.checkpoint.promoted" {
                let claimed = event
                    .delta
                    .get("uplift_claimed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                object.insert("promoted".into(), json!(claimed));
                object.insert("selected".into(), json!(true));
                object.insert(
                    "improvementVerdict".into(),
                    event
                        .delta
                        .get("improvement_verdict")
                        .cloned()
                        .unwrap_or(Value::Null),
                );
            }
        }
    }
    if by_id.is_empty() {
        if let Some(rows) = manifest.and_then(|value| value.get("checkpoints")) {
            return rows.clone();
        }
    }
    Value::Array(by_id.into_values().collect())
}

fn evaluations_from(events: &[OptimizerEventEnvelope]) -> Vec<Value> {
    events
        .iter()
        .filter(|event| {
            event.event_type == "sft.checkpoint_evaluation.completed"
                || event.event_type == "sft.baseline.completed"
        })
        .map(|event| {
            json!({
                "checkpointId": event.delta.get("checkpoint_id"),
                "splitRole": event.delta.get("split_role"),
                "evaluatorVersion": event.delta.get("evaluator_version"),
                "score": event.delta.get("score").cloned().unwrap_or(Value::Null),
                "scored": event.delta.get("scored"),
                "total": event.delta.get("total"),
            })
        })
        .collect()
}

fn selected_checkpoint_from(events: &[OptimizerEventEnvelope], manifest: Option<&Value>) -> Value {
    if let Some(event) = events.iter().rev().find(|event| {
        event.event_type == "sft.checkpoint.selected"
            || event.event_type == "sft.checkpoint.promoted"
    }) {
        return json!({
            "id": event.delta.get("checkpoint_id"),
            "rule": event.delta.get("rule"),
            "tieBreak": event.delta.get("tie_break"),
        });
    }
    manifest
        .and_then(|value| value.get("selected_checkpoint"))
        .cloned()
        .unwrap_or(Value::Null)
}

fn improvement_verdict_from(
    events: &[OptimizerEventEnvelope],
    manifest: Option<&Value>,
    evaluations: &[Value],
) -> String {
    if let Some(value) = events.iter().rev().find_map(|event| {
        event
            .delta
            .get("improvement_verdict")
            .and_then(Value::as_str)
            .map(str::to_string)
    }) {
        return value;
    }
    if let Some(value) = manifest
        .and_then(|value| value.get("improvement_verdict"))
        .and_then(Value::as_str)
    {
        return value.to_string();
    }
    let scores: Vec<f64> = evaluations
        .iter()
        .filter_map(|row| json_f64(row.get("score")))
        .collect();
    if scores.iter().all(|score| *score == 0.0) {
        return "no_measured_improvement".into();
    }
    "no_measured_improvement".into()
}

fn artifacts_from(events: &[OptimizerEventEnvelope], manifest: Option<&Value>) -> Vec<Value> {
    let mut artifacts = Vec::new();
    for event in events {
        for artifact in &event.artifact_refs {
            if !artifacts.contains(artifact) {
                artifacts.push(artifact.clone());
            }
        }
    }
    if artifacts.is_empty() {
        if let Some(rows) = manifest.and_then(|value| value.get("artifacts")) {
            if let Some(array) = rows.as_array() {
                return array.clone();
            }
        }
    }
    artifacts
}

fn usage_from(run: &OptimizerRunRecord, events: &[OptimizerEventEnvelope]) -> Value {
    let mut training_tokens = None;
    let mut inference_tokens = None;
    let mut inference_unsupported = false;
    for event in events {
        if let Some(delta) = event.usage_delta.as_ref() {
            if let Some(value) = json_u64(delta.get("training_tokens"))
                .or_else(|| json_u64(delta.get("batch_tokens")))
            {
                training_tokens = Some(training_tokens.unwrap_or(0u64).saturating_add(value));
            }
            if delta
                .get("coverage")
                .and_then(|coverage| coverage.get("inference_tokens"))
                .and_then(Value::as_str)
                == Some("unsupported")
            {
                inference_unsupported = true;
            }
            if let Some(value) = json_u64(delta.get("inference_tokens")) {
                inference_tokens = Some(inference_tokens.unwrap_or(0u64).saturating_add(value));
            }
        }
    }
    json!({
        "trainingTokens": coverage_num(training_tokens),
        "inferenceTokens": if inference_tokens.is_some() {
            coverage_num(inference_tokens)
        } else if inference_unsupported {
            json!({"value": null, "coverage": "unsupported"})
        } else {
            coverage_num(None)
        },
        "costUsd": run.usage.cost_usd,
        "ledger": {
            "promptTokens": if run.usage.prompt_tokens == 0 { Value::Null } else { json!(run.usage.prompt_tokens) },
            "completionTokens": if run.usage.completion_tokens == 0 { Value::Null } else { json!(run.usage.completion_tokens) },
        }
    })
}

fn terminal_label(status: &str, verdict: &str) -> String {
    let job_status = TrainingJobStatus::parse(status);
    if job_status == Some(TrainingJobStatus::Failed) {
        return "Failed".into();
    }
    if job_status == Some(TrainingJobStatus::Cancelled) {
        return "Cancelled".into();
    }
    match verdict {
        "improvement_demonstrated" => "Completed · improvement demonstrated".into(),
        "inconclusive" => "Completed · evaluation inconclusive".into(),
        _ => "Completed · no measured improvement".into(),
    }
}

fn training_tokens(events: &[OptimizerEventEnvelope]) -> Option<u64> {
    events.iter().rev().find_map(|event| {
        json_u64(event.delta.get("tokens")).or_else(|| {
            event
                .usage_delta
                .as_ref()
                .and_then(|delta| json_u64(delta.get("training_tokens")))
        })
    })
}

fn last_u64(events: &[OptimizerEventEnvelope], keys: &[&str]) -> Option<u64> {
    events
        .iter()
        .rev()
        .find_map(|event| keys.iter().find_map(|key| json_u64(event.delta.get(*key))))
}

fn coverage_num(value: Option<u64>) -> Value {
    match value {
        Some(value) => json!({"value": value, "coverage": "present"}),
        None => json!({"value": null, "coverage": "missing"}),
    }
}

fn coverage_f64(value: Option<f64>, coverage: &str) -> Value {
    json!({
        "value": value,
        "coverage": coverage,
    })
}

fn json_f64(value: Option<&Value>) -> Option<f64> {
    value.and_then(|value| {
        if value.is_null() {
            None
        } else {
            value.as_f64().or_else(|| value.as_u64().map(|n| n as f64))
        }
    })
}

fn json_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(|value| {
        if value.is_null() {
            None
        } else {
            value.as_u64()
        }
    })
}

pub fn sft_milestone_kind(event_type: &str, level: Option<&str>) -> Option<&'static str> {
    if level == Some("warning") {
        return Some("warning");
    }
    match event_type {
        "sft.dataset.validated" | "sft.dataset.validation_started" => Some("validation"),
        "sft.training.queued" | "optimizer.run.queued" => Some("queue_transition"),
        "sft.checkpoint.created"
        | "sft.checkpoint.ready"
        | "sft.checkpoint.selected"
        | "sft.checkpoint.promoted" => Some("checkpoint"),
        "sft.checkpoint_evaluation.allocated"
        | "sft.checkpoint_evaluation.completed"
        | "sft.checkpoint_rollout.allocated"
        | "sft.checkpoint_rollout.completed" => Some("eval_phase"),
        "optimizer.run.failed" | "sft.training.failed" => Some("failure"),
        "optimizer.run.completed" | "optimizer.run.cancelled" | "sft.training.completed" => {
            Some("terminal")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::models::{
        OptimizerCapabilities, OptimizerRunRecord, OPTIMIZER_EVENT_SCHEMA_VERSION,
        OPTIMIZER_RUN_SCHEMA_VERSION,
    };

    fn run() -> OptimizerRunRecord {
        OptimizerRunRecord {
            schema_version: OPTIMIZER_RUN_SCHEMA_VERSION.into(),
            id: "sft_materialize".into(),
            algorithm_id: "sft".into(),
            algorithm_version: None,
            status: "completed".into(),
            source: "hosted".into(),
            objective: None,
            project_ref: None,
            session_ref: None,
            created_at: "2026-08-17T00:00:00Z".into(),
            started_at: None,
            finished_at: None,
            cursor_seq: 4,
            capabilities: OptimizerCapabilities::for_algorithm("sft"),
            execution_bindings: vec![],
            input_refs: vec![],
            output_refs: vec![],
            visual_refs: vec![],
            summary: json!({"baseModel": "openai/gpt-oss-20b", "adapter": "lora_r8", "rank": 8}),
            usage: Default::default(),
            error: None,
        }
    }

    fn evt(
        event_type: &str,
        seq: u64,
        delta: Value,
        usage: Option<Map<String, Value>>,
    ) -> OptimizerEventEnvelope {
        OptimizerEventEnvelope {
            schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: Some(format!("sft_materialize:{seq}")),
            event_type: event_type.into(),
            sequence_number: seq,
            occurred_at: "2026-08-17T00:00:00Z".into(),
            optimizer_run_id: "sft_materialize".into(),
            algorithm_id: "sft".into(),
            level: Some("info".into()),
            item: None,
            delta: delta.as_object().cloned().unwrap_or_default(),
            snapshot: None,
            usage_delta: usage,
            artifact_refs: vec![],
            error: None,
            raw: json!({}),
        }
    }

    #[test]
    fn sft_result_does_not_require_best_candidate_and_nulls_missing_metrics() {
        let events = vec![
            evt(
                "sft.dataset.validated",
                1,
                json!({"dataset_digest": "sha256:abc"}),
                None,
            ),
            evt(
                "sft.training.metrics",
                2,
                json!({
                    "train_loss": 1.1,
                    "validation_loss": null,
                    "validation_loss_coverage": "unsupported"
                }),
                Some(Map::from_iter([
                    ("training_tokens".into(), json!(64)),
                    (
                        "coverage".into(),
                        json!({"training_tokens": "present", "inference_tokens": "unsupported"}),
                    ),
                ])),
            ),
            evt(
                "sft.checkpoint.promoted",
                3,
                json!({
                    "checkpoint_id": "ckpt_20",
                    "rule": "retain_latest_checkpoint",
                    "tie_break": "latest_checkpoint",
                    "improvement_verdict": "no_measured_improvement",
                    "uplift_claimed": false
                }),
                None,
            ),
        ];
        let result = project_sft_result(&run(), &events, None);
        assert_eq!(result["algorithmId"], "sft");
        assert_eq!(result["resultType"], "sft");
        assert!(result.get("selectedCandidate").is_none());
        assert_eq!(result["dataset"]["digest"], "sha256:abc");
        assert_eq!(result["metrics"]["trainLoss"]["value"], 1.1);
        assert!(result["metrics"]["validationLoss"]["value"].is_null());
        assert_eq!(
            result["metrics"]["validationLoss"]["coverage"],
            "unsupported"
        );
        assert_eq!(result["improvementVerdict"], "no_measured_improvement");
        assert_eq!(
            result["terminalLabel"],
            "Completed · no measured improvement"
        );
        assert_eq!(result["config"]["adapter"], "lora_r8");
        assert_eq!(result["config"]["rank"], 8);
        assert_eq!(result["usage"]["trainingTokens"]["value"], 64);
        assert_eq!(
            result["usage"]["inferenceTokens"]["coverage"],
            "unsupported"
        );
        let encoded = result.to_string();
        assert!(!encoded.contains("best_candidate.json"));
    }

    #[test]
    fn zero_score_evaluations_cannot_claim_improvement() {
        let events = vec![evt(
            "sft.checkpoint_evaluation.completed",
            1,
            json!({"checkpoint_id": "ckpt_a", "score": 0.0, "split_role": "selection"}),
            None,
        )];
        let result = project_sft_result(&run(), &events, None);
        assert_eq!(result["improvementVerdict"], "no_measured_improvement");
        assert_ne!(result["improvementVerdict"], "improvement_demonstrated");
    }

    #[test]
    fn identical_bytes_have_stable_dataset_identity() {
        let a = sha256_bytes(b"{\"messages\":[]}\n");
        let b = sha256_bytes(b"{\"messages\":[]}\n");
        let c = sha256_bytes(b"{\"messages\":[1]}\n");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("sha256:"));
    }

    #[test]
    fn adapter_rank_agrees_across_summary_and_result() {
        let result = project_sft_result(&run(), &[], None);
        assert_eq!(result["config"]["adapter"], "lora_r8");
        assert_eq!(result["config"]["rank"], 8);
    }

    #[test]
    fn cispo_step_metrics_reach_the_result_metrics_on_both_placements() {
        // The hosted/adapter arm and the sidecar/MLX arm now name a CISPO step
        // identically, so this fixture covers both. Before one rule owned the
        // name, this reader matched `sft.training.metrics` alone and every
        // CISPO run reported `null` train loss with `missing` coverage.
        let name = crate::optimizers::training_adapter::step_metrics_event("cispo");
        assert_eq!(name, "training.metrics");
        let mut cispo = run();
        cispo.id = "cispo_materialize".into();
        cispo.algorithm_id = "cispo".into();
        let events = vec![
            evt(name, 1, json!({"step": 1, "train_loss": 0.9}), None),
            evt(
                name,
                2,
                json!({"step": 2, "train_loss": 0.4, "validation_loss": 0.5}),
                None,
            ),
        ];
        let result = project_sft_result(&cispo, &events, None);
        assert_eq!(result["metrics"]["trainLoss"]["value"], 0.4);
        assert_eq!(result["metrics"]["trainLoss"]["coverage"], "present");
        assert_eq!(result["metrics"]["validationLoss"]["value"], 0.5);
        assert_eq!(result["metrics"]["validationLoss"]["coverage"], "present");
    }

    #[test]
    fn every_persisted_step_metrics_spelling_still_reaches_the_result() {
        // A persisted row keeps its original name forever; the reader accepts
        // the whole set the producers can have written.
        for name in [
            "sft.training.metrics",
            "sft.step.metrics",
            "training.metrics",
        ] {
            let events = vec![evt(name, 1, json!({"step": 1, "train_loss": 1.25}), None)];
            let result = project_sft_result(&run(), &events, None);
            assert_eq!(
                result["metrics"]["trainLoss"]["value"], 1.25,
                "`{name}` was dropped by the result reader"
            );
        }
    }
}
