//! CISPO: warm-start, rollout groups, advantages, clipping, checkpoints.
//!
//! Local MLX and hosted slime share this projection. Backend details stay in
//! bindings and the driver.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::optimizers::kernel::error::{KernelError, KernelErrorCode, KernelResult};
use crate::optimizers::kernel::evidence::{EvidenceState, UsageCompleteness};
use crate::optimizers::kernel::sequences::CommittedEvent;
use crate::optimizers::kernel::types::{
    EvidenceCompleteness, RunPhase, TerminalKind, WorkItemKind, WorkItemLifecycle,
};
use crate::optimizers::kernel::work::{close_open_items, WorkItem, WorkSummary};

use super::training::{MetricSeries, TrainingEvaluationSummary, TrainingMetricPoint};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CispoProjection {
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub experiment: Value,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub checkpoint_details: BTreeMap<String, Value>,
    pub work_items: Vec<WorkItem>,
    pub phase: Option<RunPhase>,
    pub usage: UsageCompleteness,
    pub warm_start_id: Option<String>,
    pub clip_identity: Option<String>,
    pub mean_advantage: Option<f64>,
    #[serde(default)]
    pub advantage_std: Option<f64>,
    #[serde(default)]
    pub reward_variance: Option<f64>,
    /// Number of distinct rollout groups whose rewards were non-uniform.
    /// This is a run-level learning-signal count, not a sample count.
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub learning_signal_groups: u64,
    /// Uniform groups are locally uninformative, but do not imply the whole
    /// run lacked a learning signal.
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub zero_advantage_groups: u64,
    /// Distinct rollout groups observed by the reducer. This compact scalar
    /// survives the bounded first-paint wire view after detailed work items
    /// are removed and is therefore the canonical UI count.
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub rollout_group_count: u64,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub group_size: Option<u64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub optimizer_steps: u64,
    #[serde(default)]
    pub clipped_token_fraction: Option<f64>,
    #[serde(default)]
    pub importance_ratio_mean: Option<f64>,
    #[serde(default)]
    pub kl_proxy: Option<f64>,
    pub checkpoints: Vec<String>,
    #[serde(default)]
    pub selected_checkpoint_id: Option<String>,
    pub child_eval_run_ids: Vec<String>,
    pub no_learning_signal: bool,
    pub policy_checkpoint_id: Option<String>,
    /// Checkpoint evaluation scorecards; see `SftProjection::evaluations`.
    #[serde(default)]
    pub evaluations: Vec<TrainingEvaluationSummary>,
    /// Bounded reward/advantage/loss curve keyed by training step.
    #[serde(default)]
    pub metrics: MetricSeries,
    /// Clip configuration as reported by the producer. Compact facts only.
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub clip_config: serde_json::Value,
}

impl CispoProjection {
    pub fn apply(&mut self, event: &CommittedEvent) -> KernelResult<()> {
        let payload = &event.producer.payload;
        match event.producer.event_type.as_str() {
            "cispo.checkpoint.registered" | "cispo.checkpoint.publication_changed"
            | "cispo.checkpoint.availability_checked" | "cispo.checkpoint.save_recorded" => {
                if let (Some(id), Some(snapshot)) = (payload.get("checkpoint_id").and_then(Value::as_str), payload.get("checkpoint_snapshot")) {
                    if !self.checkpoints.iter().any(|known| known == id) {
                        self.checkpoints.push(id.to_string());
                    }
                    self.checkpoint_details.insert(id.to_string(), snapshot.clone());
                }
            }
            kind if kind.starts_with("cispo.experiment.") || kind.starts_with("cispo.phase.") || kind.starts_with("cispo.budget.") => {
                if !self.experiment.is_object() { self.experiment = serde_json::json!({}); }
                if kind.starts_with("cispo.budget.") {
                    self.experiment["budget"] = payload.clone();
                    if let (Some(lane), Some(seconds)) = (payload.get("lane").and_then(Value::as_str), payload.get("duration_seconds").and_then(Value::as_f64)) {
                        if seconds.is_finite() && seconds >= 0.0 {
                            if !self.experiment["operation_latency"].is_object() { self.experiment["operation_latency"] = serde_json::json!({}); }
                            let prior = &self.experiment["operation_latency"][lane];
                            let count = prior["count"].as_u64().unwrap_or(0) + 1;
                            let total = prior["total_seconds"].as_f64().unwrap_or(0.0) + seconds;
                            self.experiment["operation_latency"][lane] = serde_json::json!({"count":count,"total_seconds":total,"mean_seconds":total/count as f64});
                        }
                    }
                    if !self.experiment["cost_points"].is_array() { self.experiment["cost_points"] = serde_json::json!([]); }
                    let points = self.experiment["cost_points"].as_array_mut().unwrap();
                    if points.len() >= 128 {
                        *points = points.iter().step_by(2).cloned().collect();
                    }
                    points.push(serde_json::json!({"sequence": event.aggregate_sequence, "usd": payload["counted_or_reserved_usd"]}));
                } else if kind == "cispo.phase.started" {
                    self.experiment["phase"] = payload["phase"].clone();
                    self.experiment["status"] = Value::String("running".to_string());
                    self.experiment["blocked_reason"] = Value::Null;
                } else if kind == "cispo.phase.completed" || kind == "cispo.phase.reconciled" {
                    if let Some(result) = payload.get("result") {
                        if result.get("admitted_examples_per_second").is_some() {
                            self.experiment["throughput"] = result.clone();
                        }
                        if let Some(rule) = result.get("rule") {
                            self.selected_checkpoint_id = result.get("checkpoint_id").and_then(Value::as_str).map(str::to_string);
                            self.experiment["selection"] = serde_json::json!({"rule": rule, "checkpoint_id": self.selected_checkpoint_id});
                        }
                        if let Some(update) = result.get("target_update").and_then(Value::as_u64) {
                            self.optimizer_steps = self.optimizer_steps.max(update);
                        }
                        if result.get("evaluation_id").is_some() && result.get("trained_mean").is_some() {
                            if !self.experiment["evaluations"].is_array() { self.experiment["evaluations"] = serde_json::json!([]); }
                            let mut summary = result.clone();
                            if let Some(object) = summary.as_object_mut() { object.remove("rows"); }
                            let rows = self.experiment["evaluations"].as_array_mut().unwrap();
                            rows.retain(|row| row["evaluation_id"] != summary["evaluation_id"]);
                            rows.push(summary);
                        }
                    }
                } else {
                    self.experiment["status"] = Value::String(kind.trim_start_matches("cispo.experiment.").to_string());
                    self.experiment["blocked_reason"] = payload.get("reason").cloned().unwrap_or(Value::Null);
                }
            }
            "cispo.warm_start.bound" => {
                self.warm_start_id = payload
                    .get("checkpointId")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
            }
            "cispo.clip.identity" => {
                self.clip_identity = payload
                    .get("identity")
                    .or_else(|| payload.get("clipIdentity"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                if let Some(config) = payload
                    .get("config")
                    .or_else(|| payload.get("clipConfig"))
                    .or_else(|| payload.get("clip_config"))
                {
                    self.clip_config = config.clone();
                }
            }
            "cispo.training.metrics"
            | "cispo.step.metrics"
            | "training.step.metrics"
            | "training.metrics" => {
                self.phase = Some(RunPhase::Training);
                if let Some(step) = payload.get("step").and_then(|v| v.as_u64()) {
                    self.usage.steps = Some(step);
                }
                if let Some(point) =
                    TrainingMetricPoint::from_payload(payload, event.aggregate_sequence)
                {
                    if let Some(advantage) = point.advantage {
                        self.mean_advantage = Some(advantage);
                    }
                    self.advantage_std = point.advantage_std.or(self.advantage_std);
                    self.reward_variance = point.reward_variance.or(self.reward_variance);
                    if let Some(size) = point.group_size {
                        self.group_size = Some(self.group_size.unwrap_or(0).max(size));
                    }
                    self.optimizer_steps = self
                        .optimizer_steps
                        .max(point.optimizer_step.unwrap_or(point.step));
                    self.metrics.push(point);
                }
            }
            "cispo.checkpoint_evaluation.completed"
            | "training.evaluation.completed"
            | "sft.heldout_evaluation.completed" => {
                if let Some(child) = payload
                    .get("childEvalRunId")
                    .or_else(|| payload.get("optimizerRunId"))
                    .and_then(|v| v.as_str())
                {
                    if let Some(item) = self
                        .work_items
                        .iter_mut()
                        .find(|item| item.work_item_id == format!("cispo:ckpt-eval:{child}"))
                    {
                        if item.lifecycle != WorkItemLifecycle::Terminal {
                            if item.lifecycle == WorkItemLifecycle::Queued {
                                item.transition(WorkItemLifecycle::Starting)?;
                                item.transition(WorkItemLifecycle::Running)?;
                            }
                            item.seal_terminal(TerminalKind::Completed)?;
                        }
                    }
                }
                if let Some(summary) =
                    TrainingEvaluationSummary::from_payload(payload, event.aggregate_sequence)
                {
                    match self
                        .evaluations
                        .iter_mut()
                        .find(|existing| existing.id == summary.id)
                    {
                        Some(existing) => *existing = summary,
                        None => self.evaluations.push(summary),
                    }
                }
                self.phase = Some(RunPhase::HeldoutEvaluation);
            }
            "cispo.rollout_group.completed" => {
                let id = payload
                    .get("workItemId")
                    .or_else(|| payload.get("groupId"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        KernelError::new(
                            KernelErrorCode::WorkItemIdentityMissing,
                            "CISPO rollout group is missing a stable work identity",
                        )
                    })?;
                if !self.work_items.iter().any(|item| item.work_item_id == id) {
                    let mut item = WorkItem::planned(id, WorkItemKind::TrainingStep)?;
                    item.transition(WorkItemLifecycle::Queued)?;
                    item.transition(WorkItemLifecycle::Starting)?;
                    item.transition(WorkItemLifecycle::Running)?;
                    item.seal_terminal(TerminalKind::Completed)?;
                    self.work_items.push(item);
                    self.rollout_group_count += 1;
                }
                if let Some(adv) = payload.get("meanAdvantage").and_then(|v| v.as_f64()) {
                    self.mean_advantage = Some(adv);
                }
                let observed_group_size = payload
                    .get("rewards")
                    .or_else(|| payload.get("advantages"))
                    .and_then(|value| value.as_array())
                    .map(|values| values.len() as u64);
                if let Some(size) = observed_group_size {
                    self.group_size = Some(self.group_size.unwrap_or(0).max(size));
                }
                if let Some(variance) = payload
                    .get("rewardVariance")
                    .or_else(|| payload.get("reward_variance"))
                    .and_then(Value::as_f64)
                {
                    self.reward_variance = Some(variance);
                    if variance > 0.0 {
                        self.learning_signal_groups += 1;
                    }
                }
            }
            "cispo.zero_advantage.detected" => {
                self.zero_advantage_groups += 1;
            }
            "cispo.importance_ratio.measured" => {
                self.clipped_token_fraction = payload
                    .get("clipped_token_fraction")
                    .or_else(|| payload.get("clippedTokenFraction"))
                    .and_then(Value::as_f64)
                    .or(self.clipped_token_fraction);
                self.importance_ratio_mean = payload
                    .get("mean_ratio")
                    .or_else(|| payload.get("meanRatio"))
                    .and_then(Value::as_f64)
                    .or(self.importance_ratio_mean);
                self.kl_proxy = payload
                    .get("kl_proxy")
                    .or_else(|| payload.get("klProxy"))
                    .and_then(Value::as_f64)
                    .or(self.kl_proxy);
            }
            "cispo.no_learning_signal" => {
                // Older hosted producers used this name for one uniform
                // rollout group. A group identity makes it a local diagnostic;
                // only an unscoped event is a run-level stop condition.
                if payload
                    .get("groupId")
                    .or_else(|| payload.get("group_id"))
                    .is_some()
                {
                    self.zero_advantage_groups += 1;
                } else {
                    self.no_learning_signal = true;
                }
            }
            "cispo.checkpoint.ready" | "sft.checkpoint.ready" => {
                if let Some(id) = payload
                    .get("checkpointId")
                    .or_else(|| payload.get("checkpoint_id"))
                    .and_then(|v| v.as_str())
                {
                    self.checkpoints.push(id.to_string());
                    self.policy_checkpoint_id = Some(id.to_string());
                }
            }
            "cispo.checkpoint.promoted" | "sft.checkpoint.promoted" | "sft.checkpoint.selected" => {
                self.selected_checkpoint_id = payload
                    .get("checkpointId")
                    .or_else(|| payload.get("checkpoint_id"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            "cispo.checkpoint_evaluation.started" => {
                self.phase = Some(RunPhase::CheckpointEvaluation);
                let child = payload
                    .get("childEvalRunId")
                    .or_else(|| payload.get("optimizerRunId"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        KernelError::new(
                            KernelErrorCode::EventSchemaMismatch,
                            "CISPO checkpoint evaluation must reference a child eval run",
                        )
                    })?;
                self.child_eval_run_ids.push(child.to_string());
                let mut item = WorkItem::planned(
                    format!("cispo:ckpt-eval:{child}"),
                    WorkItemKind::CheckpointEvaluation,
                )?;
                item.transition(WorkItemLifecycle::Queued)?;
                self.work_items.push(item);
            }
            _ => {}
        }
        apply_usage(&mut self.usage, event);
        Ok(())
    }

    pub fn work_summary(&self) -> WorkSummary {
        WorkSummary::from_items(&self.work_items, "rollout_groups", false)
    }

    /// Terminal seal closes interrupted children as `cancelled`, never failed.
    pub fn close_open_work(&mut self) -> KernelResult<usize> {
        close_open_items(&mut self.work_items)
    }

    pub fn evidence_state(&self) -> EvidenceState {
        let completeness = if self.policy_checkpoint_id.is_some() || self.no_learning_signal {
            EvidenceCompleteness::Complete
        } else if !self.work_items.is_empty() {
            EvidenceCompleteness::Partial
        } else {
            EvidenceCompleteness::Absent
        };
        EvidenceState {
            completeness,
            reason: None,
            refs: Vec::new(),
        }
    }

    pub fn settle(&self) -> KernelResult<CispoResult> {
        if self.work_items.is_empty() && self.policy_checkpoint_id.is_none() {
            return Err(KernelError::new(
                KernelErrorCode::EvidenceMissing,
                "CISPO cannot settle without rollout groups or a policy checkpoint",
            ));
        }
        Ok(CispoResult {
            warm_start_id: self.warm_start_id.clone(),
            clip_identity: self.clip_identity.clone(),
            mean_advantage: self.mean_advantage,
            no_learning_signal: self.no_learning_signal,
            policy_checkpoint_id: self.policy_checkpoint_id.clone(),
            child_eval_run_ids: self.child_eval_run_ids.clone(),
            usage: self.usage.clone(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CispoResult {
    #[serde(default)]
    pub warm_start_id: Option<String>,
    #[serde(default)]
    pub clip_identity: Option<String>,
    #[serde(default)]
    pub mean_advantage: Option<f64>,
    pub no_learning_signal: bool,
    #[serde(default)]
    pub policy_checkpoint_id: Option<String>,
    pub child_eval_run_ids: Vec<String>,
    pub usage: UsageCompleteness,
}

fn apply_usage(usage: &mut UsageCompleteness, event: &CommittedEvent) {
    let payload = &event.producer.payload;
    usage.add_reported(
        payload.get("costUsd").and_then(|v| v.as_f64()),
        payload.get("promptTokens").and_then(|v| v.as_u64()),
        payload.get("completionTokens").and_then(|v| v.as_u64()),
    );
}

