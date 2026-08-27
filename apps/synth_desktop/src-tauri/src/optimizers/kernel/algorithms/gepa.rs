//! GEPA: candidates, generations, proposer jobs, evaluations, frontier, selection.
//!
//! A rollout ceiling is a budget, not a fixed work denominator.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::optimizers::kernel::error::{KernelError, KernelErrorCode, KernelResult};
use crate::optimizers::kernel::evidence::{EvidenceState, UsageCompleteness};
use crate::optimizers::kernel::sequences::CommittedEvent;
use crate::optimizers::kernel::types::{
    EvidenceCompleteness, RunPhase, TerminalKind, WorkItemKind, WorkItemLifecycle,
};
use crate::optimizers::kernel::work::{WorkItem, WorkSummary};

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
    #[serde(default)]
    pub heldout_reward: Option<f64>,
    #[serde(default)]
    pub train_reward: Option<f64>,
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
    #[serde(skip)]
    #[specta(skip)]
    seen_evaluations: BTreeSet<String>,
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
                if entry.source.as_deref() == Some("seed") {
                    self.seed_candidate_id.get_or_insert(id);
                }
                self.phase = Some(RunPhase::Selection);
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
                if let Some(evaluation_id) = payload.get("evaluation_id").and_then(|v| v.as_str()) {
                    if !self.seen_evaluations.insert(evaluation_id.to_string()) {
                        return Ok(());
                    }
                }
                match payload.get("reward").and_then(|v| v.as_f64()) {
                    Some(_) => self.rollouts_scored += 1,
                    None => self.rollouts_failed += 1,
                }
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
                self.proposals_returned = payload
                    .get("count")
                    .or_else(|| payload.get("proposal_count"))
                    .and_then(|v| v.as_u64());
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
                    if let Some(candidate) = self.candidates.get_mut(id) {
                        candidate.heldout_reward = payload
                            .get("heldout_reward")
                            .or_else(|| payload.get("reward"))
                            .and_then(|v| v.as_f64());
                    }
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
}
