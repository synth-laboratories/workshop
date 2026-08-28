//! Eval: immutable trial plan, per-trial evidence, scorecards, selection.

use serde::{Deserialize, Serialize};

use crate::optimizers::kernel::error::{KernelError, KernelErrorCode, KernelResult};
use crate::optimizers::kernel::evidence::{EvidenceRef, EvidenceState, UsageCompleteness};
use crate::optimizers::kernel::sequences::CommittedEvent;
use crate::optimizers::kernel::types::{
    EvidenceCompleteness, RunPhase, TerminalKind, WorkItemKind, WorkItemLifecycle,
};
use crate::optimizers::kernel::work::{close_open_items, WorkItem, WorkSummary};

/// Durable evidence state for one admitted rollout/work item.
///
/// This is deliberately separate from the run-level `kernel::evidence::EvidenceState`:
/// the run receipt is a fold of this ledger, while these entries retain which
/// rollout was open, partially sealed, aborted, or never produced evidence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RolloutEvidenceState {
    Open,
    SealedComplete,
    SealedPartial,
    Aborted,
    #[default]
    Missing,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RolloutEvidenceEntry {
    pub work_item_id: String,
    #[serde(default)]
    pub rollout_id: Option<String>,
    #[serde(default)]
    pub trial_id: Option<String>,
    pub state: RolloutEvidenceState,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub last_observed_step: Option<u64>,
    #[serde(default)]
    pub cancellation_request_id: Option<String>,
    #[serde(default)]
    pub refs: Vec<EvidenceRef>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EvalProjection {
    pub candidates: Vec<String>,
    #[specta(type = Vec<specta_typescript::Number>)]
    pub seeds: Vec<i64>,
    pub scenarios: Vec<String>,
    pub work_items: Vec<WorkItem>,
    pub phase: Option<RunPhase>,
    pub usage: UsageCompleteness,
    pub mean_reward: Option<f64>,
    #[specta(type = specta_typescript::Number)]
    pub scored_trials: u64,
    /// Terminal trials carrying an evaluator-produced measurement. This is
    /// separate from terminal work: a process can finish without producing a
    /// score, and that must not make evidence complete.
    #[specta(type = specta_typescript::Number)]
    pub evaluator_evidence: u64,
    pub promotion_applicable: bool,
    #[specta(type = specta_typescript::Number)]
    pub traces: usize,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    /// Per-rollout evidence truth. Added with a default so persisted v1/v2
    /// projections replay forward instead of becoming unreadable.
    #[serde(default)]
    pub evidence_ledger: Vec<RolloutEvidenceEntry>,
}

impl EvalProjection {
    pub fn plan_trials(
        &mut self,
        candidates: Vec<String>,
        seeds: Vec<i64>,
        scenarios: Vec<String>,
    ) -> KernelResult<()> {
        if !self.work_items.is_empty() {
            return Err(KernelError::new(
                KernelErrorCode::EventSchemaMismatch,
                "eval trial plan is immutable once created",
            ));
        }
        let mut items = Vec::new();
        for (ci, candidate) in candidates.iter().enumerate() {
            for (si, seed) in seeds.iter().enumerate() {
                for (xi, scenario) in scenarios.iter().enumerate() {
                    let id = format!("eval:{candidate}:{seed}:{scenario}:{ci}:{si}:{xi}");
                    items.push(WorkItem::planned(id, WorkItemKind::EvalTrial)?);
                }
            }
        }
        self.candidates = candidates;
        self.seeds = seeds;
        self.scenarios = scenarios;
        self.work_items = items;
        self.evidence_ledger = self
            .work_items
            .iter()
            .map(|item| RolloutEvidenceEntry {
                work_item_id: item.work_item_id.clone(),
                ..RolloutEvidenceEntry::default()
            })
            .collect();
        self.promotion_applicable = self.candidates.len() > 1;
        Ok(())
    }

    pub fn apply(&mut self, event: &CommittedEvent) -> KernelResult<()> {
        match event.producer.event_type.as_str() {
            "eval.run.planned" => {
                let payload = &event.producer.payload;
                let work_item_ids = string_list(payload, "workItemIds");
                if !work_item_ids.is_empty() {
                    if !self.work_items.is_empty() {
                        return Err(KernelError::new(
                            KernelErrorCode::EventSchemaMismatch,
                            "eval trial plan is immutable once created",
                        ));
                    }
                    for id in work_item_ids {
                        self.work_items
                            .push(WorkItem::planned(id.clone(), WorkItemKind::EvalTrial)?);
                        self.ensure_evidence_entry(&id);
                    }
                    self.promotion_applicable = false;
                    apply_usage(&mut self.usage, event);
                    return Ok(());
                }
                let candidates = candidate_list(payload);
                let seeds = i64_list(payload, "seeds");
                let scenarios = string_list(payload, "scenarios");
                let planned = payload
                    .get("plannedTrials")
                    .or_else(|| payload.get("planned_trials"))
                    .and_then(|v| v.as_u64());
                if let Some(planned) = planned {
                    self.candidates = candidates;
                    self.seeds = seeds;
                    self.scenarios = scenarios;
                    for index in 0..planned {
                        let id = format!("eval:trial:{index}");
                        self.work_items
                            .push(WorkItem::planned(id.clone(), WorkItemKind::EvalTrial)?);
                        self.ensure_evidence_entry(&id);
                    }
                    self.promotion_applicable = self.candidates.len() > 1;
                } else if candidates.is_empty() || seeds.is_empty() || scenarios.is_empty() {
                    return Err(KernelError::new(
                        KernelErrorCode::EventSchemaMismatch,
                        "eval.run.planned must declare a trial count or complete candidate, seed, and scenario dimensions",
                    ));
                } else {
                    self.plan_trials(candidates, seeds, scenarios)?;
                }
            }
            "eval.trial.queued" => {
                self.advance_named(event, WorkItemLifecycle::Queued)?;
            }
            "eval.trial.started" | "optimizer.work.started" => {
                self.advance_named(event, WorkItemLifecycle::Running)?;
                if let Some(id) = work_id(event) {
                    let entry = self.ensure_evidence_entry(&id);
                    entry.state = RolloutEvidenceState::Open;
                    entry.rollout_id = string_field(&event.producer.payload, "rolloutId", "rollout_id");
                    entry.trial_id = string_field(&event.producer.payload, "trialId", "trial_id");
                }
            }
            "eval.trial.event" => {
                self.observe_trial_evidence(&event.producer.payload);
            }
            "eval.trial.terminal" => {
                // A cancelled trial is neither valid nor invalid: it was
                // interrupted, and `valid` is a claim about work that ran to
                // its own end. It settles `cancelled`, never `failed`.
                let cancelled = event
                    .producer
                    .payload
                    .get("status")
                    .and_then(|v| v.as_str())
                    == Some("cancelled")
                    || event
                        .producer
                        .payload
                        .get("cancelled")
                        .and_then(|v| v.as_bool())
                        == Some(true);
                let valid = if cancelled {
                    false
                } else {
                    event
                        .producer
                        .payload
                        .get("valid")
                        .and_then(|v| v.as_bool())
                        .ok_or_else(|| {
                            KernelError::new(
                                KernelErrorCode::EventSchemaMismatch,
                                "eval.trial.terminal is missing typed `valid`",
                            )
                        })?
                };
                let reward = event
                    .producer
                    .payload
                    .get("reward")
                    .and_then(|v| v.as_f64());
                let id = work_id(event);
                let work_item_id = if let Some(id) = id {
                    id
                } else {
                    self.work_items
                        .iter_mut()
                        .find(|item| item.lifecycle != WorkItemLifecycle::Terminal)
                        .ok_or_else(|| {
                            KernelError::new(
                                KernelErrorCode::WorkItemIdentityMissing,
                                "eval.trial.terminal arrived with no work item identity and no planned trial left",
                            )
                        })?
                        .work_item_id
                        .clone()
                };
                let item = self.named_work_item(&work_item_id)?;
                if item.lifecycle == WorkItemLifecycle::Planned {
                    item.transition(WorkItemLifecycle::Queued)?;
                    item.transition(WorkItemLifecycle::Starting)?;
                    item.transition(WorkItemLifecycle::Running)?;
                } else if item.lifecycle == WorkItemLifecycle::Queued {
                    item.transition(WorkItemLifecycle::Starting)?;
                    item.transition(WorkItemLifecycle::Running)?;
                } else if item.lifecycle == WorkItemLifecycle::Starting {
                    item.transition(WorkItemLifecycle::Running)?;
                }
                item.seal_terminal(if cancelled {
                    TerminalKind::Cancelled
                } else if valid {
                    TerminalKind::Completed
                } else {
                    TerminalKind::Failed
                })?;
                if let Some(reward) = reward {
                    self.mean_reward = Some(match (self.mean_reward, self.scored_trials) {
                        (Some(mean), n) => (mean * n as f64 + reward) / (n as f64 + 1.0),
                        (None, _) => reward,
                    });
                    self.scored_trials += 1;
                }
                if evaluator_measurement(&event.producer.payload) {
                    self.evaluator_evidence += 1;
                }
                let terminal_refs = evidence_refs(&event.producer.payload);
                for reference in &terminal_refs {
                    if reference.kind.contains("trace") {
                        self.traces += 1;
                    }
                    if !self
                        .evidence_refs
                        .iter()
                        .any(|present| present.kind == reference.kind && present.id == reference.id)
                    {
                        self.evidence_refs.push(reference.clone());
                    }
                }
                let entry = self.ensure_evidence_entry(&work_item_id);
                entry.rollout_id = string_field(
                    &event.producer.payload,
                    "rolloutId",
                    "rollout_id",
                )
                .or_else(|| entry.rollout_id.clone());
                entry.trial_id = string_field(&event.producer.payload, "trialId", "trial_id")
                    .or_else(|| entry.trial_id.clone());
                entry.last_observed_step = event
                    .producer
                    .payload
                    .get("lastObservedStep")
                    .or_else(|| event.producer.payload.get("steps"))
                    .and_then(|value| value.as_u64())
                    .or(entry.last_observed_step);
                entry.cancellation_request_id = event
                    .producer
                    .payload
                    .pointer("/cancellation/requestId")
                    .or_else(|| {
                        event
                            .producer
                            .payload
                            .pointer("/cancellationReceipt/requestId")
                    })
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                entry.refs = terminal_refs;
                entry.state = rollout_evidence_state(&event.producer.payload, cancelled, valid);
            }
            _ => {}
        }
        apply_usage(&mut self.usage, event);
        Ok(())
    }

    fn advance_named(
        &mut self,
        event: &CommittedEvent,
        next: WorkItemLifecycle,
    ) -> KernelResult<()> {
        let Some(id) = work_id(event) else {
            return Ok(());
        };
        let item = self.named_work_item(&id)?;
        if next == WorkItemLifecycle::Running {
            if item.lifecycle == WorkItemLifecycle::Planned {
                item.transition(WorkItemLifecycle::Queued)?;
                item.transition(WorkItemLifecycle::Starting)?;
            } else if item.lifecycle == WorkItemLifecycle::Queued {
                item.transition(WorkItemLifecycle::Starting)?;
            }
        }
        item.transition(next)
    }

    fn named_work_item(&mut self, id: &str) -> KernelResult<&mut WorkItem> {
        if let Some(index) = self
            .work_items
            .iter()
            .position(|item| item.work_item_id == id)
        {
            return Ok(&mut self.work_items[index]);
        }

        // A legacy producer may declare only a count, then assign the stable
        // trial identity in its queued/terminal fact. Replace one undispatched
        // reserved slot; never add work beyond a declared fixed plan.
        if let Some(index) = self.work_items.iter().position(|item| {
            item.lifecycle == WorkItemLifecycle::Planned
                && item
                    .work_item_id
                    .strip_prefix("eval:trial:")
                    .is_some_and(|suffix| suffix.parse::<u64>().is_ok())
        }) {
            let reserved_id = self.work_items[index].work_item_id.clone();
            self.work_items[index].work_item_id = id.to_string();
            if let Some(entry) = self
                .evidence_ledger
                .iter_mut()
                .find(|entry| entry.work_item_id == reserved_id)
            {
                entry.work_item_id = id.to_string();
            }
            return Ok(&mut self.work_items[index]);
        }
        if self.work_items.is_empty() {
            self.work_items
                .push(WorkItem::planned(id, WorkItemKind::EvalTrial)?);
            return self.work_items.last_mut().ok_or_else(|| {
                KernelError::new(
                    KernelErrorCode::WorkItemIdentityMissing,
                    "identified eval work item was not retained",
                )
            });
        }
        Err(KernelError::new(
            KernelErrorCode::WorkItemIdentityMissing,
            format!("unknown eval work item {id}"),
        ))
    }

    fn ensure_evidence_entry(&mut self, work_item_id: &str) -> &mut RolloutEvidenceEntry {
        if let Some(index) = self
            .evidence_ledger
            .iter()
            .position(|entry| entry.work_item_id == work_item_id)
        {
            return &mut self.evidence_ledger[index];
        }
        self.evidence_ledger.push(RolloutEvidenceEntry {
            work_item_id: work_item_id.to_string(),
            ..RolloutEvidenceEntry::default()
        });
        self.evidence_ledger
            .last_mut()
            .expect("evidence entry was just appended")
    }

    fn observe_trial_evidence(&mut self, payload: &serde_json::Value) {
        let rollout_id = payload
            .pointer("/container_event/rollout_id")
            .or_else(|| payload.pointer("/containerEvent/rolloutId"))
            .and_then(|value| value.as_str());
        let trial_id = string_field(payload, "trialId", "trial_id");
        let entry = self.evidence_ledger.iter_mut().find(|entry| {
            rollout_id.is_some_and(|id| entry.rollout_id.as_deref() == Some(id))
                || trial_id
                    .as_deref()
                    .is_some_and(|id| entry.trial_id.as_deref() == Some(id))
        });
        let Some(entry) = entry else {
            return;
        };
        if entry.state == RolloutEvidenceState::Missing {
            entry.state = RolloutEvidenceState::Open;
        }
        let step = payload
            .pointer("/container_event/payload/step")
            .or_else(|| payload.pointer("/containerEvent/payload/step"))
            .and_then(|value| value.as_u64());
        if let Some(step) = step {
            entry.last_observed_step = Some(entry.last_observed_step.unwrap_or(0).max(step));
        }
    }

    pub fn work_summary(&self) -> WorkSummary {
        WorkSummary::from_items(&self.work_items, "trials", true)
    }

    /// Terminal seal closes interrupted children as `cancelled`, never failed.
    pub fn close_open_work(&mut self) -> KernelResult<usize> {
        let closed = close_open_items(&mut self.work_items)?;
        for entry in &mut self.evidence_ledger {
            if entry.state == RolloutEvidenceState::Open {
                entry.state = RolloutEvidenceState::Aborted;
            }
        }
        Ok(closed)
    }

    pub fn evidence_state(&self) -> EvidenceState {
        if self.work_items.is_empty() {
            return EvidenceState::absent();
        }
        let terminal = self
            .work_items
            .iter()
            .filter(|item| item.lifecycle == WorkItemLifecycle::Terminal)
            .count();
        let sealed_complete = self
            .evidence_ledger
            .iter()
            .filter(|entry| entry.state == RolloutEvidenceState::SealedComplete)
            .count();
        let sealed_partial = self
            .evidence_ledger
            .iter()
            .filter(|entry| entry.state == RolloutEvidenceState::SealedPartial)
            .count();
        let aborted = self
            .evidence_ledger
            .iter()
            .filter(|entry| entry.state == RolloutEvidenceState::Aborted)
            .count();
        let missing = self
            .evidence_ledger
            .iter()
            .filter(|entry| entry.state == RolloutEvidenceState::Missing)
            .count();
        let completeness = if !self.evidence_ledger.is_empty()
            && sealed_complete == self.evidence_ledger.len()
        {
            EvidenceCompleteness::Complete
        } else if terminal > 0 || sealed_complete > 0 || sealed_partial > 0 || aborted > 0 {
            EvidenceCompleteness::Partial
        } else {
            EvidenceCompleteness::Absent
        };
        EvidenceState {
            completeness,
            reason: (completeness != EvidenceCompleteness::Complete).then(|| {
                format!(
                    "{terminal}/{} trials terminal; evidence ledger: {sealed_complete} complete, {sealed_partial} partial, {aborted} aborted, {missing} missing",
                    self.work_items.len(),
                )
            }),
            refs: self.evidence_refs.clone(),
        }
    }

    pub fn settle(&self) -> KernelResult<EvalResult> {
        if self.work_items.is_empty() {
            return Err(KernelError::new(
                KernelErrorCode::EvidenceMissing,
                "eval cannot settle without a trial plan",
            ));
        }
        let unfinished = self
            .work_items
            .iter()
            .filter(|item| item.lifecycle != WorkItemLifecycle::Terminal)
            .count();
        if unfinished != 0 {
            return Err(KernelError::new(
                KernelErrorCode::EvidenceMissing,
                format!("eval cannot settle while {unfinished} planned trials are unfinished"),
            ));
        }
        let summary = self.work_summary();
        let failed = summary.failed.unwrap_or(0);
        if failed != 0 {
            return Err(KernelError::new(
                KernelErrorCode::EvidenceMissing,
                format!("eval cannot complete with {failed} failed trials"),
            ));
        }
        let cancelled = summary.cancelled.unwrap_or(0);
        if cancelled != 0 {
            return Err(KernelError::new(
                KernelErrorCode::EvidenceMissing,
                format!("eval cannot complete with {cancelled} cancelled trials"),
            ));
        }
        let succeeded = summary.succeeded.unwrap_or(0);
        if self.evaluator_evidence != succeeded {
            return Err(KernelError::new(
                KernelErrorCode::EvidenceMissing,
                format!(
                    "eval cannot complete: {}/{} completed trials reported an evaluator measurement",
                    self.evaluator_evidence, succeeded
                ),
            ));
        }
        if self.evidence_refs.is_empty() {
            return Err(KernelError::new(
                KernelErrorCode::EvidenceMissing,
                "eval cannot complete without immutable evidence references",
            ));
        }
        let selection = if self.promotion_applicable {
            EvalSelection::Inconclusive
        } else {
            EvalSelection::PromotionNotApplicable
        };
        Ok(EvalResult {
            trials: self.work_summary(),
            mean_reward: self.mean_reward,
            selection,
            usage: self.usage.clone(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum EvalSelection {
    PromotionNotApplicable,
    Inconclusive,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EvalResult {
    pub trials: WorkSummary,
    #[serde(default)]
    pub mean_reward: Option<f64>,
    pub selection: EvalSelection,
    pub usage: UsageCompleteness,
}

fn work_id(event: &CommittedEvent) -> Option<String> {
    event
        .producer
        .payload
        .get("workItemId")
        .or_else(|| event.producer.payload.get("work_item_id"))
        .or_else(|| event.producer.payload.get("trialId"))
        .or_else(|| event.producer.payload.get("trial_id"))
        // `OptimizerEventEnvelope.item.id` is flattened into the producer
        // payload by the bridge. It is the legacy eval trial identity.
        .or_else(|| event.producer.payload.get("id"))
        .and_then(|v| v.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn string_list(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn candidate_list(value: &serde_json::Value) -> Vec<String> {
    value
        .get("candidates")
        .and_then(|value| value.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    row.as_str()
                        .or_else(|| row.get("id").and_then(|value| value.as_str()))
                        .or_else(|| row.get("candidateId").and_then(|value| value.as_str()))
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn i64_list(value: &serde_json::Value, key: &str) -> Vec<i64> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|rows| rows.iter().filter_map(|row| row.as_i64()).collect())
        .unwrap_or_default()
}

fn apply_usage(usage: &mut UsageCompleteness, event: &CommittedEvent) {
    let payload = &event.producer.payload;
    usage.add_reported(
        payload.get("costUsd").and_then(|v| v.as_f64()),
        payload.get("promptTokens").and_then(|v| v.as_u64()),
        payload.get("completionTokens").and_then(|v| v.as_u64()),
    );
}

fn evaluator_measurement(payload: &serde_json::Value) -> bool {
    payload.get("reward").is_some_and(|value| !value.is_null())
        || payload
            .get("metrics")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|metrics| metrics.values().any(|value| !value.is_null()))
}

fn evidence_refs(payload: &serde_json::Value) -> Vec<EvidenceRef> {
    payload
        .get("artifactRefs")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let kind = value.get("kind")?.as_str()?.trim();
            let id = value
                .get("id")
                .or_else(|| value.get("path"))?
                .as_str()?
                .trim();
            if kind.is_empty() || id.is_empty() {
                return None;
            }
            Some(EvidenceRef {
                kind: kind.to_string(),
                id: id.to_string(),
                digest: value
                    .get("digest")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

fn string_field(value: &serde_json::Value, camel: &str, snake: &str) -> Option<String> {
    value
        .get(camel)
        .or_else(|| value.get(snake))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn rollout_evidence_state(
    payload: &serde_json::Value,
    cancelled: bool,
    valid: bool,
) -> RolloutEvidenceState {
    let explicit = payload
        .get("evidenceState")
        .or_else(|| payload.get("evidence_state"))
        .and_then(serde_json::Value::as_str);
    match explicit {
        Some("open") => RolloutEvidenceState::Open,
        Some("sealed_complete") => RolloutEvidenceState::SealedComplete,
        Some("sealed_partial") => RolloutEvidenceState::SealedPartial,
        Some("aborted") => RolloutEvidenceState::Aborted,
        Some("missing") => RolloutEvidenceState::Missing,
        _ if cancelled
            && (payload.get("lastObservedStep").is_some()
                || payload.get("partialSeal").is_some()) =>
        {
            RolloutEvidenceState::SealedPartial
        }
        _ if cancelled => RolloutEvidenceState::Aborted,
        _ if valid && !evidence_refs(payload).is_empty() => RolloutEvidenceState::SealedComplete,
        _ if !evidence_refs(payload).is_empty() => RolloutEvidenceState::SealedPartial,
        _ => RolloutEvidenceState::Missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::kernel::sequences::ProducerEvent;
    use crate::optimizers::kernel::types::PRODUCER_EVENT_SCHEMA_VERSION;
    use serde_json::json;

    fn committed(event_type: &str, payload: serde_json::Value, seq: u64) -> CommittedEvent {
        let producer = ProducerEvent {
            producer_id: "eval-local".into(),
            producer_sequence: seq,
            idempotency_key: format!("{event_type}-{seq}"),
            schema_version: PRODUCER_EVENT_SCHEMA_VERSION.into(),
            algorithm_id: "eval".into(),
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
    fn baseline_eval_settles_promotion_not_applicable() {
        let mut projection = EvalProjection::default();
        projection
            .apply(&committed(
                "eval.run.planned",
                json!({"plannedTrials": 1, "candidates": ["baseline"], "seeds": [1], "scenarios": ["default"]}),
                1,
            ))
            .unwrap();
        let id = projection.work_items[0].work_item_id.clone();
        projection.work_items[0]
            .transition(WorkItemLifecycle::Queued)
            .unwrap();
        projection.work_items[0]
            .transition(WorkItemLifecycle::Starting)
            .unwrap();
        projection.work_items[0]
            .transition(WorkItemLifecycle::Running)
            .unwrap();
        projection
            .apply(&committed(
                "eval.trial.terminal",
                json!({
                    "workItemId": id,
                    "valid": true,
                    "reward": 1.0,
                    "artifactRefs": [{"kind": "evaluator_result", "id": "eval:trial:1"}]
                }),
                2,
            ))
            .unwrap();
        let result = projection.settle().unwrap();
        assert_eq!(result.selection, EvalSelection::PromotionNotApplicable);
        assert_eq!(result.mean_reward, Some(1.0));
        assert_eq!(result.trials.planned, Some(1));
    }

    #[test]
    fn missing_valid_is_typed_not_a_success() {
        let mut projection = EvalProjection::default();
        projection
            .plan_trials(vec!["a".into()], vec![1], vec!["s".into()])
            .unwrap();
        projection.work_items[0]
            .transition(WorkItemLifecycle::Queued)
            .unwrap();
        projection.work_items[0]
            .transition(WorkItemLifecycle::Starting)
            .unwrap();
        projection.work_items[0]
            .transition(WorkItemLifecycle::Running)
            .unwrap();
        let id = projection.work_items[0].work_item_id.clone();
        let error = projection
            .apply(&committed(
                "eval.trial.terminal",
                json!({"workItemId": id}),
                2,
            ))
            .unwrap_err();
        assert_eq!(error.code, KernelErrorCode::EventSchemaMismatch);
    }

    #[test]
    fn an_identified_legacy_terminal_retains_its_work_item_without_a_plan_event() {
        let mut projection = EvalProjection::default();
        projection
            .apply(&committed(
                "eval.trial.terminal",
                json!({
                    "id": "trial:7",
                    "valid": true,
                    "reward": 0.5,
                    "artifactRefs": [{"kind": "evaluator_result", "id": "eval:trial:7"}]
                }),
                1,
            ))
            .unwrap();
        assert_eq!(projection.work_items.len(), 1);
        assert_eq!(projection.work_items[0].work_item_id, "trial:7");
        assert_eq!(
            projection.work_items[0].lifecycle,
            WorkItemLifecycle::Terminal
        );
    }

    #[test]
    fn terminal_work_without_measurement_or_evidence_cannot_settle() {
        let mut projection = EvalProjection::default();
        projection
            .apply(&committed(
                "eval.run.planned",
                json!({"plannedTrials": 1}),
                1,
            ))
            .unwrap();
        projection
            .apply(&committed(
                "eval.trial.terminal",
                json!({"workItemId": "eval:trial:0", "valid": true}),
                2,
            ))
            .unwrap();

        assert_eq!(
            projection.evidence_state().completeness,
            EvidenceCompleteness::Partial
        );
        let error = projection.settle().unwrap_err();
        assert_eq!(error.code, KernelErrorCode::EvidenceMissing);
        assert!(error.message.contains("evaluator measurement"));
    }

    #[test]
    fn cancellation_seals_partial_rollout_evidence_and_leaves_undispatched_work_missing() {
        let mut projection = EvalProjection::default();
        projection
            .apply(&committed(
                "eval.run.planned",
                json!({"plannedTrials": 2}),
                1,
            ))
            .unwrap();
        projection
            .apply(&committed(
                "eval.trial.started",
                json!({
                    "workItemId": "eval:trial:0",
                    "trialId": "trial:0",
                    "rolloutId": "rollout:0"
                }),
                2,
            ))
            .unwrap();
        projection
            .apply(&committed(
                "eval.trial.event",
                json!({
                    "trial_id": "trial:0",
                    "container_event": {"kind": "frame", "payload": {"step": 4}}
                }),
                3,
            ))
            .unwrap();
        projection
            .apply(&committed(
                "eval.trial.terminal",
                json!({
                    "workItemId": "eval:trial:0",
                    "trialId": "trial:0",
                    "rolloutId": "rollout:0",
                    "status": "cancelled",
                    "cancelled": true,
                    "lastObservedStep": 4,
                    "evidenceState": "sealed_partial",
                    "cancellationReceipt": {"requestId": "cancel:1"},
                    "artifactRefs": [{"kind": "trace_v5_partial", "id": "trace:0"}]
                }),
                4,
            ))
            .unwrap();
        projection.close_open_work().unwrap();

        assert_eq!(projection.evidence_ledger.len(), 2);
        assert_eq!(
            projection.evidence_ledger[0].state,
            RolloutEvidenceState::SealedPartial
        );
        assert_eq!(
            projection.evidence_ledger[0].rollout_id.as_deref(),
            Some("rollout:0")
        );
        assert_eq!(projection.evidence_ledger[0].last_observed_step, Some(4));
        assert_eq!(
            projection.evidence_ledger[0]
                .cancellation_request_id
                .as_deref(),
            Some("cancel:1")
        );
        assert_eq!(
            projection.evidence_ledger[1].state,
            RolloutEvidenceState::Missing
        );
        assert_eq!(
            projection.evidence_state().completeness,
            EvidenceCompleteness::Partial
        );
    }
}
