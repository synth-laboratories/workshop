//! GEPA: candidates, generations, proposer jobs, evaluations, frontier, selection.
//!
//! A rollout ceiling is a budget, not a fixed work denominator.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::optimizers::kernel::error::{KernelError, KernelErrorCode, KernelResult};
use crate::optimizers::kernel::evidence::{EvidenceState, UsageCompleteness};
use crate::optimizers::kernel::sequences::CommittedEvent;
use crate::optimizers::kernel::types::{
    EvidenceCompleteness, RunPhase, TerminalKind, WorkItemKind, WorkItemLifecycle,
};
use crate::optimizers::kernel::work::{close_open_items, WorkItem, WorkSummary};

const STAGE_HELDOUT: &str = "heldout";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GepaCandidate {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub generation: Option<u64>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub digest: Option<String>,
    /// Durable candidate levers (for GEPA, normally the proposed prompt).
    /// This is bounded by the candidate count and lets the live visual render
    /// content/diffs without replaying the entire optimizer journal.
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub values: Value,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub proposal_index: Option<u64>,
    #[serde(default)]
    pub heldout_reward: Option<f64>,
    #[serde(default)]
    pub train_reward: Option<f64>,
    #[serde(default)]
    pub minibatch_reward: Option<f64>,
    #[serde(default)]
    pub gate_accepted: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GepaProjection {
    pub work_items: Vec<WorkItem>,
    pub phase: Option<RunPhase>,
    pub usage: UsageCompleteness,
    pub candidates: BTreeMap<String, GepaCandidate>,
    pub candidate_order: Vec<String>,
    pub seed_candidate_id: Option<String>,
    pub selected_candidate_id: Option<String>,
    pub frontier_history: Vec<String>,
    pub incumbent_id: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub rollouts_allocated: u64,
    #[specta(type = specta_typescript::Number)]
    pub rollouts_scored: u64,
    #[specta(type = specta_typescript::Number)]
    pub rollouts_failed: u64,
    #[specta(type = specta_typescript::Number)]
    pub proposals_requested: Option<u64>,
    #[specta(type = specta_typescript::Number)]
    pub proposals_returned: Option<u64>,
    #[specta(type = specta_typescript::Number)]
    pub max_active_workers: Option<u64>,
    #[specta(type = specta_typescript::Number)]
    pub rollout_budget: Option<u64>,
    /// Durable, bounded summaries used by the live visual. These are not raw
    /// traces; the journal remains the authority for full inspection.
    #[serde(default)]
    pub evaluations: Vec<GepaEvaluationSummary>,
    #[serde(default)]
    pub proposer_calls: Vec<GepaProposerCallSummary>,
    /// Compact product-facing setup facts reduced from the durable journal.
    /// This deliberately excludes task rows, prompts, and credentials.
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub contract: Value,
    /// Latest observed execution shape. Capacity is kept distinct from
    /// measured parallelism and throughput.
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub runtime: Value,
    #[serde(default)]
    #[specta(skip)]
    seen_evaluations: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GepaEvaluationSummary {
    pub id: String,
    #[serde(default)]
    pub candidate_id: Option<String>,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub example_id: Option<String>,
    #[serde(default)]
    pub rollout_id: Option<String>,
    #[serde(default)]
    pub reward: Option<f64>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GepaProposerCallSummary {
    #[specta(type = specta_typescript::Number)]
    pub generation: u64,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub proposal_count: u64,
    #[serde(default)]
    pub cost_usd: Option<f64>,
}

impl GepaProjection {
    pub fn apply(&mut self, event: &CommittedEvent) -> KernelResult<()> {
        let payload = &event.producer.payload;
        match event.producer.event_type.as_str() {
            "gepa.run.planned" | "optimizer.run.queued" => {
                self.rollout_budget = payload
                    .get("maxTotalRollouts")
                    .or_else(|| payload.get("max_total_rollouts"))
                    .and_then(|v| v.as_u64())
                    .or(self.rollout_budget);
                self.proposals_requested = payload
                    .get("proposalsPerGeneration")
                    .and_then(|v| v.as_u64())
                    .or(self.proposals_requested);
            }
            "candidate.registered" => {
                let id = required_str(payload, "candidate_id")?;
                let entry = self.candidates.entry(id.clone()).or_insert_with(|| {
                    self.candidate_order.push(id.clone());
                    GepaCandidate {
                        id: id.clone(),
                        ..GepaCandidate::default()
                    }
                });
                if entry.parent_id.is_none() {
                    entry.parent_id = payload
                        .get("parent_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                }
                if entry.generation.is_none() {
                    entry.generation = payload.get("generation").and_then(|v| v.as_u64());
                }
                if entry.source.is_none() {
                    entry.source = payload
                        .get("source")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                }
                if entry.values.is_null() {
                    entry.values = payload.get("values").cloned().unwrap_or(Value::Null);
                }
                if entry.digest.is_none() {
                    entry.digest = payload
                        .get("digest")
                        .or_else(|| payload.get("candidate_digest"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                if entry.proposal_index.is_none() {
                    entry.proposal_index = payload.get("proposal_index").and_then(Value::as_u64);
                }
                if entry.source.as_deref() == Some("seed") {
                    self.seed_candidate_id.get_or_insert(id);
                }
                self.phase = Some(RunPhase::Selection);
            }
            "gepa.run.started" => {
                let container = nested_object_mut(&mut self.contract, "container");
                copy_string(payload, "container_url", container, "url");
            }
            "container.task_info.loaded" => {
                let task = payload.get("task").and_then(Value::as_object);
                if let Some(task) = task {
                    let target = nested_object_mut(&mut self.contract, "task");
                    copy_string_map(task, "id", target, "id");
                    copy_string_map(task, "name", target, "name");
                    copy_string_map(task, "description", target, "description");
                    copy_string_map(task, "task_family", target, "family");
                    copy_string_map(task, "version", target, "version");
                }
                copy_dataset(payload.get("dataset"), &mut self.contract);
            }
            "container.program.loaded" => {
                let program = nested_object_mut(&mut self.contract, "program");
                copy_string(payload, "program_id", program, "id");
                if let Some(fields) = payload.get("mutable_fields").and_then(Value::as_array) {
                    program.insert("mutableFields".into(), Value::Array(fields.clone()));
                }
            }
            "objective_set.declared" => {
                let objective = nested_object_mut(&mut self.contract, "objectiveSet");
                copy_string(payload, "objective_set_id", objective, "id");
                copy_string(payload, "objective_set_hash", objective, "hash");
                copy_string(payload, "frontier_type", objective, "frontierType");
                copy_string(
                    payload,
                    "selection_objective",
                    objective,
                    "selectionObjective",
                );
                if let Some(items) = payload.get("objectives").and_then(Value::as_array) {
                    objective.insert("objectives".into(), Value::Array(items.clone()));
                }
            }
            "taskset.tasks.loaded" => {
                let splits = nested_object_mut(&mut self.contract, "splits");
                copy_number(payload, "minibatch_rows", splits, "minibatch");
                copy_number(payload, "reflection_rows", splits, "reflection");
                copy_number(payload, "pareto_rows", splits, "pareto");
                copy_number(payload, "heldout_rows", splits, "heldout");
                if let Some(rows) = payload.get("pareto_rows").and_then(Value::as_u64) {
                    splits.insert("train".into(), json!(rows));
                }
            }
            "container.contract.verified" => {
                copy_dataset(payload.get("dataset"), &mut self.contract);
                let container = nested_object_mut(&mut self.contract, "container");
                container.insert("verified".into(), Value::Bool(true));
                copy_string(payload, "container_spec_id", container, "specId");
                copy_string(payload, "workshop_instance", container, "workshopInstance");
                copy_string(payload, "credential_mode", container, "credentialMode");
                copy_string(payload, "runtime_family", container, "runtimeFamily");
                copy_string(payload, "target_id", container, "targetId");
                copy_string(payload, "reward_authority", container, "rewardAuthority");
                copy_string(payload, "retention", container, "retention");
                copy_number(payload, "scale_leases", container, "scaleLeases");
                let evaluator_id = payload
                    .get("evaluator_id")
                    .or_else(|| payload.get("evaluation_plan_ref"))
                    .and_then(Value::as_str);
                if let Some(evaluator_id) = evaluator_id {
                    container.insert("evaluatorId".into(), json!(evaluator_id));
                }
                if let Some(policy) = payload
                    .get("policy_refs")
                    .and_then(Value::as_array)
                    .and_then(|refs| refs.first())
                    .and_then(Value::as_object)
                {
                    copy_string_map(policy, "harness", container, "policyHarness");
                    copy_string_map(policy, "config", container, "policyConfig");
                    copy_string_map(policy, "model", container, "policyModel");
                }
            }
            "runtime.job.completed" | "runtime.throughput.warning" => {
                let runtime = object_mut(&mut self.runtime);
                copy_number(
                    payload,
                    "configured_rollout_workers",
                    runtime,
                    "configuredRolloutWorkers",
                );
                copy_number(
                    payload,
                    "static_rollout_workers",
                    runtime,
                    "staticRolloutWorkers",
                );
                copy_number(
                    payload,
                    "estimated_effective_concurrency",
                    runtime,
                    "estimatedEffectiveConcurrency",
                );
                copy_string(
                    payload,
                    "rollout_submission_mode",
                    runtime,
                    "rolloutSubmissionMode",
                );
                copy_number(
                    payload,
                    "max_dispatch_chunk_size",
                    runtime,
                    "maxDispatchChunkSize",
                );
                copy_number(payload, "wall_seconds", runtime, "latestWallSeconds");
                let observed = payload
                    .get("observed_uncached_rollouts_per_second")
                    .and_then(Value::as_f64)
                    .map(|per_second| per_second * 60.0)
                    .or_else(|| {
                        let misses = payload.get("cache_misses").and_then(Value::as_f64)?;
                        let seconds = payload.get("wall_seconds").and_then(Value::as_f64)?;
                        (misses > 0.0 && seconds > 0.0).then_some(misses * 60.0 / seconds)
                    });
                if let Some(rollouts_per_minute) = observed {
                    runtime.insert("rolloutsPerMinute".into(), json!(rollouts_per_minute));
                }
            }
            "optimizer.candidate_evaluation.allocated" => {
                self.rollouts_allocated += 1;
                let candidate = payload.get("candidate_id").and_then(|v| v.as_str());
                let stage = payload.get("stage").and_then(|v| v.as_str());
                let work_id = payload
                    .get("workItemId")
                    .or_else(|| payload.get("evaluation_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .or_else(|| {
                        candidate.map(|candidate| {
                            format!(
                                "gepa:{candidate}:{}:{}",
                                stage.unwrap_or("unspecified"),
                                event.aggregate_sequence
                            )
                        })
                    })
                    .ok_or_else(|| {
                        KernelError::new(
                            KernelErrorCode::WorkItemIdentityMissing,
                            "GEPA allocation is missing a stable work identity",
                        )
                    })?;
                if !self
                    .work_items
                    .iter()
                    .any(|item| item.work_item_id == work_id)
                {
                    let mut item = WorkItem::planned(work_id, WorkItemKind::CandidateEvaluation)?;
                    item.transition(WorkItemLifecycle::Queued)?;
                    item.transition(WorkItemLifecycle::Starting)?;
                    item.transition(WorkItemLifecycle::Running)?;
                    self.work_items.push(item);
                }
            }
            "optimizer.evaluation_result.received" => {
                let evaluation_key = payload
                    // A producer may emit an early partial result with an
                    // evaluation id and later finalize the same provider call
                    // without that field. Both records retain rollout_id, so
                    // it is the durable identity for exactly-once scoring.
                    .get("rollout_id")
                    .or_else(|| payload.get("evaluation_id"))
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .or_else(|| {
                        Some(format!(
                            "{}:{}:{}",
                            payload.get("candidate_id")?.as_str()?,
                            payload.get("stage")?.as_str()?,
                            payload.get("example_id")?.as_str()?
                        ))
                    })
                    .unwrap_or_else(|| format!("event:{}", event.aggregate_sequence));
                if !self.seen_evaluations.insert(evaluation_key) {
                    return Ok(());
                }
                match payload.get("reward").and_then(|v| v.as_f64()) {
                    Some(_) => self.rollouts_scored += 1,
                    None => self.rollouts_failed += 1,
                }
                let evaluation_id = payload
                    .get("evaluation_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("evaluation:{}", event.aggregate_sequence));
                self.evaluations.push(GepaEvaluationSummary {
                    id: evaluation_id,
                    candidate_id: payload
                        .get("candidate_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    stage: payload
                        .get("stage")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    example_id: payload
                        .get("example_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    rollout_id: payload
                        .get("rollout_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    reward: payload.get("reward").and_then(|v| v.as_f64()),
                    cost_usd: payload.get("cost_usd").and_then(|v| v.as_f64()),
                });
                if let Some(active) = payload.get("active_workers").and_then(|v| v.as_u64()) {
                    self.max_active_workers =
                        Some(self.max_active_workers.unwrap_or(0).max(active));
                }
                if let Some(id) = payload.get("candidate_id").and_then(|v| v.as_str()) {
                    if payload.get("stage").and_then(|v| v.as_str()) == Some(STAGE_HELDOUT) {
                        if let Some(reward) = payload.get("reward").and_then(|v| v.as_f64()) {
                            if let Some(candidate) = self.candidates.get_mut(id) {
                                candidate.heldout_reward = Some(reward);
                            }
                        }
                    }
                }
                if let Some(work_id) = payload
                    .get("workItemId")
                    .or_else(|| payload.get("evaluation_id"))
                    .and_then(|v| v.as_str())
                {
                    if let Some(item) = self
                        .work_items
                        .iter_mut()
                        .find(|item| item.work_item_id == work_id)
                    {
                        let valid = payload
                            .get("valid")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true);
                        item.seal_terminal(if valid {
                            TerminalKind::Completed
                        } else {
                            TerminalKind::Failed
                        })?;
                    }
                }
            }
            "proposer.completed" => {
                let returned = payload
                    .get("count")
                    .or_else(|| payload.get("proposal_count"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                self.proposals_returned = Some(self.proposals_returned.unwrap_or(0) + returned);
                self.proposer_calls.push(GepaProposerCallSummary {
                    generation: payload
                        .get("generation")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    model: payload
                        .get("model")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    provider: payload
                        .get("provider")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    proposal_count: returned,
                    cost_usd: payload.get("cost_usd").and_then(|v| v.as_f64()),
                });
                let work_id = format!("proposer:{}", event.aggregate_sequence);
                let mut item = WorkItem::planned(work_id, WorkItemKind::ProposerJob)?;
                item.transition(WorkItemLifecycle::Queued)?;
                item.transition(WorkItemLifecycle::Starting)?;
                item.transition(WorkItemLifecycle::Running)?;
                item.seal_terminal(TerminalKind::Completed)?;
                self.work_items.push(item);
            }
            "candidate.evaluated" | "candidate.full_train_evaluated" => {
                if let Some(id) = observed_candidate_id(payload).map(str::to_string) {
                    let is_seed =
                        self.seed_candidate_id.is_none() && observed_parent_id(payload).is_none();
                    let candidate = self.candidates.entry(id.clone()).or_insert_with(|| {
                        self.candidate_order.push(id.clone());
                        GepaCandidate {
                            id: id.clone(),
                            parent_id: observed_parent_id(payload).map(str::to_string),
                            ..GepaCandidate::default()
                        }
                    });
                    candidate.gate_accepted = payload.get("accepted").and_then(|v| v.as_bool());
                    candidate.train_reward = payload
                        .get("train_reward")
                        .or_else(|| payload.get("trainReward"))
                        .or_else(|| payload.get("reward"))
                        .and_then(|value| value.as_f64())
                        .or(candidate.train_reward);
                    if is_seed {
                        self.seed_candidate_id = Some(id);
                    }
                }
            }
            "candidate.minibatch_evaluated" | "candidate.rejected" => {
                if let Some(id) = observed_candidate_id(payload).map(str::to_string) {
                    let candidate = self.candidates.entry(id.clone()).or_insert_with(|| {
                        self.candidate_order.push(id.clone());
                        GepaCandidate {
                            id,
                            ..GepaCandidate::default()
                        }
                    });
                    if candidate.parent_id.is_none() {
                        candidate.parent_id = observed_parent_id(payload).map(str::to_string);
                    }
                    candidate.minibatch_reward = payload
                        .get("minibatch_reward")
                        .or_else(|| payload.get("candidate_minibatch_reward"))
                        .and_then(|value| value.as_f64())
                        .or(candidate.minibatch_reward);
                    candidate.gate_accepted = payload
                        .get("accepted_minibatch")
                        .or_else(|| payload.get("accepted"))
                        .and_then(|value| value.as_bool())
                        .or(candidate.gate_accepted);
                }
            }
            "candidate.accepted" => {
                if let Some(id) = observed_candidate_id(payload).map(str::to_string) {
                    let candidate = self.candidates.entry(id.clone()).or_insert_with(|| {
                        self.candidate_order.push(id.clone());
                        GepaCandidate {
                            id,
                            parent_id: observed_parent_id(payload).map(str::to_string),
                            ..GepaCandidate::default()
                        }
                    });
                    candidate.gate_accepted = Some(true);
                }
            }
            "frontier.snapshot" | "frontier.updated" => {
                if let Some(id) = payload
                    .get("best_candidate_id")
                    .or_else(|| payload.get("incumbentId"))
                    .and_then(|v| v.as_str())
                {
                    self.incumbent_id = Some(id.to_string());
                    self.frontier_history.push(id.to_string());
                }
            }
            "heldout.completed" => {
                self.phase = Some(RunPhase::HeldoutEvaluation);
                if let Some(id) = payload.get("candidate_id").and_then(|v| v.as_str()) {
                    let candidate = self.candidates.entry(id.to_string()).or_insert_with(|| {
                        self.candidate_order.push(id.to_string());
                        GepaCandidate {
                            id: id.to_string(),
                            ..GepaCandidate::default()
                        }
                    });
                    candidate.heldout_reward = payload
                        .get("heldout_reward")
                        .or_else(|| payload.get("reward"))
                        .and_then(|v| v.as_f64());
                    candidate.train_reward = payload
                        .get("train_reward")
                        .and_then(|v| v.as_f64())
                        .or(candidate.train_reward);
                }
            }
            "gepa.run.finished" => {
                self.selected_candidate_id = payload
                    .get("selected_candidate_id")
                    .or_else(|| payload.get("best_candidate_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .or_else(|| self.incumbent_id.clone());
                self.phase = Some(RunPhase::Materializing);
            }
            _ => {}
        }
        apply_usage(&mut self.usage, event);
        Ok(())
    }

    pub fn work_summary(&self) -> WorkSummary {
        // A rollout ceiling is a budget, not a planned denominator.
        let mut summary = WorkSummary {
            unit: Some("rollouts".into()),
            fixed_denominator: false,
            ..WorkSummary::default()
        };
        if self.rollouts_allocated == 0 && self.rollouts_scored == 0 && self.rollouts_failed == 0 {
            return summary;
        }
        summary.succeeded = Some(self.rollouts_scored);
        summary.failed = Some(self.rollouts_failed);
        summary
    }

    /// Terminal seal closes interrupted children as `cancelled`, never failed.
    pub fn close_open_work(&mut self) -> KernelResult<usize> {
        close_open_items(&mut self.work_items)
    }

    pub fn evidence_state(&self) -> EvidenceState {
        if self.candidates.is_empty() && self.work_items.is_empty() {
            return EvidenceState::absent();
        }
        let completeness = if self.selected_candidate_id.is_some() || self.incumbent_id.is_some() {
            EvidenceCompleteness::Complete
        } else if !self.candidates.is_empty() {
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

    pub fn settle(&self) -> KernelResult<GepaResult> {
        if self.candidates.is_empty() {
            return Err(KernelError::new(
                KernelErrorCode::EvidenceMissing,
                "GEPA cannot settle without candidates",
            ));
        }
        let seed_heldout = self
            .seed_candidate_id
            .as_ref()
            .and_then(|id| self.candidates.get(id))
            .and_then(|c| c.heldout_reward);
        let selected_id = self
            .selected_candidate_id
            .clone()
            .or_else(|| self.incumbent_id.clone());
        let selected_heldout = selected_id
            .as_ref()
            .and_then(|id| self.candidates.get(id))
            .and_then(|c| c.heldout_reward);
        let verdict = match (seed_heldout, selected_heldout, selected_id.as_ref()) {
            (Some(seed), Some(selected), Some(id))
                if Some(id.as_str()) != self.seed_candidate_id.as_deref() && selected > seed =>
            {
                GepaVerdict::MeasuredImprovement
            }
            (Some(_), Some(_), _) => GepaVerdict::NoMeasuredImprovement,
            (None, _, _) | (_, None, _) => GepaVerdict::Inconclusive,
        };
        Ok(GepaResult {
            verdict,
            seed_candidate_id: self.seed_candidate_id.clone(),
            selected_candidate_id: selected_id,
            candidates: self.candidate_order.len() as u64,
            work: self.work_summary(),
            usage: self.usage.clone(),
        })
    }
}

fn object_mut(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("object initialized above")
}

fn nested_object_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    let root = object_mut(value);
    object_mut(root.entry(key.to_string()).or_insert_with(|| json!({})))
}

fn copy_string(
    source: &Value,
    source_key: &str,
    target: &mut Map<String, Value>,
    target_key: &str,
) {
    if let Some(value) = source.get(source_key).and_then(Value::as_str) {
        target.insert(target_key.to_string(), json!(value));
    }
}

fn copy_string_map(
    source: &Map<String, Value>,
    source_key: &str,
    target: &mut Map<String, Value>,
    target_key: &str,
) {
    if let Some(value) = source.get(source_key).and_then(Value::as_str) {
        target.insert(target_key.to_string(), json!(value));
    }
}

fn copy_number(
    source: &Value,
    source_key: &str,
    target: &mut Map<String, Value>,
    target_key: &str,
) {
    if let Some(value) = source.get(source_key).and_then(Value::as_f64) {
        target.insert(target_key.to_string(), json!(value));
    }
}

fn copy_dataset(source: Option<&Value>, contract: &mut Value) {
    let Some(source) = source.and_then(Value::as_object) else {
        return;
    };
    let dataset = nested_object_mut(contract, "dataset");
    copy_string_map(source, "source", dataset, "source");
    copy_string_map(source, "config", dataset, "config");
    copy_string_map(source, "revision", dataset, "revision");
    copy_string_map(source, "dataset_digest", dataset, "digest");
    if let Some(value) = source.get("row_count").and_then(Value::as_u64) {
        dataset.insert("rowCount".into(), json!(value));
    }
    if let Some(value) = source.get("label_count").and_then(Value::as_u64) {
        dataset.insert("labelCount".into(), json!(value));
    }
    if let Some(source_splits) = source.get("splits").and_then(Value::as_object) {
        let splits = object_mut(dataset.entry("splits").or_insert_with(|| json!({})));
        for name in ["train", "selection", "heldout"] {
            if let Some(count) = source_splits
                .get(name)
                .and_then(Value::as_object)
                .and_then(|split| split.get("count"))
                .and_then(Value::as_u64)
            {
                splits.insert(name.into(), json!(count));
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum GepaVerdict {
    MeasuredImprovement,
    NoMeasuredImprovement,
    Inconclusive,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GepaResult {
    pub verdict: GepaVerdict,
    #[serde(default)]
    pub seed_candidate_id: Option<String>,
    #[serde(default)]
    pub selected_candidate_id: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub candidates: u64,
    pub work: WorkSummary,
    pub usage: UsageCompleteness,
}

fn required_str(payload: &serde_json::Value, key: &str) -> KernelResult<String> {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            KernelError::new(
                KernelErrorCode::EventSchemaMismatch,
                format!("GEPA event missing {key}"),
            )
        })
}

/// Historical sidecars placed candidate identity on the envelope item while
/// newer producers use the canonical snake-case field.
fn observed_candidate_id(value: &serde_json::Value) -> Option<&str> {
    value
        .get("candidate_id")
        .or_else(|| value.get("candidateId"))
        .or_else(|| value.get("id"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
}

fn observed_parent_id(value: &serde_json::Value) -> Option<&str> {
    value
        .get("parent_id")
        .or_else(|| value.get("parentId"))
        .or_else(|| value.pointer("/raw/parent_id"))
        .or_else(|| value.pointer("/raw/parentId"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
}

fn apply_usage(usage: &mut UsageCompleteness, event: &CommittedEvent) {
    let payload = &event.producer.payload;
    usage.add_reported(
        payload.get("costUsd").and_then(|v| v.as_f64()),
        payload.get("promptTokens").and_then(|v| v.as_u64()),
        payload.get("completionTokens").and_then(|v| v.as_u64()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::kernel::sequences::ProducerEvent;
    use crate::optimizers::kernel::types::PRODUCER_EVENT_SCHEMA_VERSION;
    use serde_json::json;

    fn committed(event_type: &str, payload: serde_json::Value, seq: u64) -> CommittedEvent {
        let producer = ProducerEvent {
            producer_id: "gepa-local".into(),
            producer_sequence: seq,
            idempotency_key: format!("{event_type}-{seq}"),
            schema_version: PRODUCER_EVENT_SCHEMA_VERSION.into(),
            algorithm_id: "gepa".into(),
            event_type: event_type.into(),
            occurred_at: "2026-08-27T18:00:00Z".into(),
            payload_digest: String::new(),
            payload,
        }
        .with_computed_digest();
        CommittedEvent {
            aggregate_sequence: seq,
            committed_at: "2026-08-27T18:00:01Z".into(),
            producer,
        }
    }

    #[test]
    fn seed_win_is_no_measured_improvement() {
        let mut projection = GepaProjection::default();
        projection
            .apply(&committed(
                "candidate.registered",
                json!({"candidate_id": "seed", "source": "seed"}),
                1,
            ))
            .unwrap();
        projection
            .apply(&committed(
                "candidate.registered",
                json!({"candidate_id": "child", "source": "proposer", "parent_id": "seed"}),
                2,
            ))
            .unwrap();
        projection
            .apply(&committed(
                "heldout.completed",
                json!({"candidate_id": "seed", "reward": 0.8}),
                3,
            ))
            .unwrap();
        projection
            .apply(&committed(
                "heldout.completed",
                json!({"candidate_id": "child", "reward": 0.4}),
                4,
            ))
            .unwrap();
        projection
            .apply(&committed(
                "gepa.run.finished",
                json!({"selected_candidate_id": "seed"}),
                5,
            ))
            .unwrap();
        let result = projection.settle().unwrap();
        assert_eq!(result.verdict, GepaVerdict::NoMeasuredImprovement);
        assert_eq!(result.selected_candidate_id.as_deref(), Some("seed"));
        assert!(!result.work.fixed_denominator);
    }

    #[test]
    fn allocation_without_identity_is_typed() {
        let mut projection = GepaProjection::default();
        let error = projection
            .apply(&committed(
                "optimizer.candidate_evaluation.allocated",
                json!({}),
                1,
            ))
            .unwrap_err();
        assert_eq!(error.code, KernelErrorCode::WorkItemIdentityMissing);
    }

    #[test]
    fn scored_rollouts_are_the_work_unit_not_a_fixed_plan() {
        let mut projection = GepaProjection::default();
        for seq in 1..=2 {
            projection
                .apply(&committed(
                    "optimizer.candidate_evaluation.allocated",
                    json!({"candidate_id": "seed", "stage": "heldout"}),
                    seq,
                ))
                .unwrap();
            projection
                .apply(&committed(
                    "optimizer.evaluation_result.received",
                    json!({
                        "candidate_id": "seed",
                        "stage": "heldout",
                        "evaluation_id": format!("seed:heldout:{seq}"),
                        "reward": 0.5
                    }),
                    seq + 2,
                ))
                .unwrap();
        }
        let summary = projection.work_summary();
        assert_eq!(summary.planned, None);
        assert_eq!(summary.succeeded, Some(2));
        assert!(!summary.fixed_denominator);
        assert_eq!(summary.unit.as_deref(), Some("rollouts"));
    }

    #[test]
    fn duplicate_result_deduplication_survives_projection_persistence() {
        let payload = json!({
            "candidate_id": "seed",
            "stage": "seed_full_train",
            "example_id": "train:0",
            "evaluation_id": "seed:train:0",
            "rollout_id": "roll_0",
            "reward": 1.0
        });
        let mut projection = GepaProjection::default();
        projection
            .apply(&committed(
                "optimizer.evaluation_result.received",
                payload.clone(),
                1,
            ))
            .unwrap();
        let mut restored: GepaProjection =
            serde_json::from_value(serde_json::to_value(projection).unwrap()).unwrap();
        let mut finalized = payload;
        finalized.as_object_mut().unwrap().remove("evaluation_id");
        restored
            .apply(&committed(
                "optimizer.evaluation_result.received",
                finalized,
                2,
            ))
            .unwrap();
        assert_eq!(restored.rollouts_scored, 1);
        assert_eq!(restored.evaluations.len(), 1);
    }

    #[test]
    fn current_gepa_events_preserve_minibatch_proposer_and_heldout_evidence() {
        let mut projection = GepaProjection::default();
        let events = [
            (
                "candidate.registered",
                json!({"candidate_id":"seed","source":"seed","values":{"classification_system_prompt":"Classify into one label."}}),
            ),
            (
                "candidate.evaluated",
                json!({"candidate_id":"seed","train_reward":0.8025}),
            ),
            (
                "candidate.registered",
                json!({"candidate_id":"child","source":"reflector:parent_variation","parent_id":"seed","generation":0,"proposal_index":0,"values":{"classification_system_prompt":"Classify precisely into one canonical label."}}),
            ),
            (
                "proposer.completed",
                json!({"generation":0,"proposal_count":1,"model":"openai/gpt-5.6-luna","provider":"openrouter","cost_usd":0.01}),
            ),
            (
                "candidate.minibatch_evaluated",
                json!({"candidate_id":"child","parent_id":"seed","minibatch_reward":0.7845,"accepted_minibatch":false}),
            ),
            (
                "candidate.rejected",
                json!({"candidate_id":"child","parent_id":"seed","candidate_minibatch_reward":0.7845,"accepted_minibatch":false}),
            ),
            (
                "heldout.completed",
                json!({"candidate_id":"seed","heldout_reward":0.7985,"train_reward":0.8025}),
            ),
        ];
        for (index, (kind, payload)) in events.into_iter().enumerate() {
            projection
                .apply(&committed(kind, payload, index as u64 + 1))
                .unwrap();
        }
        assert_eq!(projection.proposals_returned, Some(1));
        assert_eq!(projection.proposer_calls.len(), 1);
        assert_eq!(
            projection.candidates["child"].minibatch_reward,
            Some(0.7845)
        );
        assert_eq!(projection.candidates["child"].gate_accepted, Some(false));
        assert_eq!(projection.candidates["seed"].train_reward, Some(0.8025));
        assert_eq!(projection.candidates["seed"].heldout_reward, Some(0.7985));
        assert_eq!(
            projection.candidates["child"].values["classification_system_prompt"],
            json!("Classify precisely into one canonical label.")
        );
        assert_eq!(projection.candidates["child"].proposal_index, Some(0));
    }

    #[test]
    fn live_projection_keeps_setup_and_measured_runtime_facts() {
        let mut projection = GepaProjection::default();
        let events = [
            (
                "gepa.run.started",
                json!({"container_url":"http://127.0.0.1:8127"}),
            ),
            (
                "container.task_info.loaded",
                json!({
                    "task":{"id":"banking77-intents-v1","name":"Banking77 intent classification","task_family":"banking77","version":"v1"},
                    "dataset":{"source":"PolyAI/banking77","config":"test","revision":"evals:abc","dataset_digest":"sha256:data","row_count":3080,"label_count":77,"splits":{"train":{"count":2114},"selection":{"count":623},"heldout":{"count":343}}}
                }),
            ),
            (
                "container.program.loaded",
                json!({"program_id":"banking77-classifier-v1","mutable_fields":["classification_system_prompt"]}),
            ),
            (
                "taskset.tasks.loaded",
                json!({"minibatch_rows":40,"reflection_rows":100,"pareto_rows":100,"heldout_rows":100}),
            ),
            (
                "container.contract.verified",
                json!({"container_spec_id":"banking77-gepa-b-v6","workshop_instance":"B","credential_mode":"workshop_ephemeral_proxy","runtime_family":"banking77","evaluator_id":"banking77-evaluator-v1","retention":"run","policy_refs":[{"model":"openai/gpt-5.6-luna","config":"chatgpt_proxy"}]}),
            ),
            (
                "runtime.job.completed",
                json!({"configured_rollout_workers":50,"static_rollout_workers":50,"estimated_effective_concurrency":17.5,"cache_misses":50,"wall_seconds":5.0,"rollout_submission_mode":"async","max_dispatch_chunk_size":50}),
            ),
        ];
        for (index, (kind, payload)) in events.into_iter().enumerate() {
            projection
                .apply(&committed(kind, payload, index as u64 + 1))
                .unwrap();
        }
        assert_eq!(
            projection.contract["task"]["id"],
            json!("banking77-intents-v1")
        );
        assert_eq!(projection.contract["dataset"]["labelCount"], json!(77));
        assert_eq!(projection.contract["splits"]["heldout"], json!(100.0));
        assert_eq!(
            projection.contract["container"]["url"],
            json!("http://127.0.0.1:8127")
        );
        assert_eq!(
            projection.contract["container"]["workshopInstance"],
            json!("B")
        );
        assert_eq!(projection.runtime["configuredRolloutWorkers"], json!(50.0));
        assert_eq!(
            projection.runtime["estimatedEffectiveConcurrency"],
            json!(17.5)
        );
        assert_eq!(projection.runtime["rolloutsPerMinute"], json!(600.0));
    }

    #[test]
    fn pre_summary_projection_deserializes_with_empty_durable_summaries() {
        let projection: GepaProjection = serde_json::from_value(json!({
            "workItems": [],
            "usage": {},
            "candidates": {},
            "candidateOrder": [],
            "frontierHistory": [],
            "rolloutsAllocated": 0,
            "rolloutsScored": 0,
            "rolloutsFailed": 0
        }))
        .unwrap();
        assert!(projection.evaluations.is_empty());
        assert!(projection.proposer_calls.is_empty());
    }
}
