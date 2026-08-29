//! CISPO: warm-start, rollout groups, advantages, clipping, checkpoints.
//!
//! Local MLX and hosted slime share this projection. Backend details stay in
//! bindings and the driver.

use serde::{Deserialize, Serialize};

use crate::optimizers::kernel::error::{KernelError, KernelErrorCode, KernelResult};
use crate::optimizers::kernel::evidence::{EvidenceState, UsageCompleteness};
use crate::optimizers::kernel::sequences::CommittedEvent;
use crate::optimizers::kernel::types::{
    EvidenceCompleteness, RunPhase, TerminalKind, WorkItemKind, WorkItemLifecycle,
};
use crate::optimizers::kernel::work::{close_open_items, WorkItem, WorkSummary};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CispoProjection {
    pub work_items: Vec<WorkItem>,
    pub phase: Option<RunPhase>,
    pub usage: UsageCompleteness,
    pub warm_start_id: Option<String>,
    pub clip_identity: Option<String>,
    pub mean_advantage: Option<f64>,
    pub checkpoints: Vec<String>,
    pub child_eval_run_ids: Vec<String>,
    pub no_learning_signal: bool,
    pub policy_checkpoint_id: Option<String>,
}

impl CispoProjection {
    pub fn apply(&mut self, event: &CommittedEvent) -> KernelResult<()> {
        let payload = &event.producer.payload;
        match event.producer.event_type.as_str() {
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
                }
                if let Some(adv) = payload.get("meanAdvantage").and_then(|v| v.as_f64()) {
                    self.mean_advantage = Some(adv);
                    if adv == 0.0 {
                        self.no_learning_signal = true;
                    }
                }
            }
            "cispo.no_learning_signal" => {
                self.no_learning_signal = true;
            }
            "cispo.checkpoint.ready" | "sft.checkpoint.ready" => {
                if let Some(id) = payload
                    .get("checkpointId")
                    .or_else(|| payload.get("checkpoint_id"))
                    .or_else(|| payload.get("id"))
                    .and_then(|v| v.as_str())
                {
                    self.checkpoints.push(id.to_string());
                    self.policy_checkpoint_id = Some(id.to_string());
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::kernel::sequences::ProducerEvent;
    use crate::optimizers::kernel::types::PRODUCER_EVENT_SCHEMA_VERSION;
    use serde_json::json;

    fn committed(event_type: &str, payload: serde_json::Value, seq: u64) -> CommittedEvent {
        let producer = ProducerEvent {
            producer_id: "cispo".into(),
            producer_sequence: seq,
            idempotency_key: format!("{event_type}-{seq}"),
            schema_version: PRODUCER_EVENT_SCHEMA_VERSION.into(),
            algorithm_id: "cispo".into(),
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
    fn zero_advantage_is_a_typed_no_learning_signal() {
        let mut projection = CispoProjection::default();
        projection
            .apply(&committed(
                "cispo.rollout_group.completed",
                json!({"groupId": "g1", "meanAdvantage": 0.0}),
                1,
            ))
            .unwrap();
        let result = projection.settle().unwrap();
        assert!(result.no_learning_signal);
        assert_eq!(result.mean_advantage, Some(0.0));
    }

    #[test]
    fn sidecar_checkpoint_item_id_settles() {
        let mut projection = CispoProjection::default();
        projection
            .apply(&committed(
                "sft.checkpoint.ready",
                json!({"id": "cispo_mlx_job:step-1", "path": "/tmp/adapter", "sha256": "abc"}),
                1,
            ))
            .unwrap();
        let result = projection.settle().unwrap();
        assert_eq!(
            result.policy_checkpoint_id.as_deref(),
            Some("cispo_mlx_job:step-1")
        );
        assert!(!result.no_learning_signal);
    }
}
