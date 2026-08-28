//! SFT: dataset/config identity, training, checkpoints, child evals, artifacts.
//!
//! Local MLX and hosted Tinker share this projection. Checkpoint evaluations
//! reference child eval runs; they do not create eval_campaign aggregates.

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
pub struct SftProjection {
    pub work_items: Vec<WorkItem>,
    pub phase: Option<RunPhase>,
    pub usage: UsageCompleteness,
    pub dataset_digest: Option<String>,
    pub config_digest: Option<String>,
    pub checkpoints: Vec<String>,
    pub selected_checkpoint_id: Option<String>,
    pub child_eval_run_ids: Vec<String>,
    pub produced_adapter: Option<String>,
    pub train_loss: Option<f64>,
}

impl SftProjection {
    pub fn apply(&mut self, event: &CommittedEvent) -> KernelResult<()> {
        let payload = &event.producer.payload;
        match event.producer.event_type.as_str() {
            "sft.dataset.validated" => {
                self.dataset_digest = payload
                    .get("dataset_digest")
                    .or_else(|| payload.get("digest"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                self.phase = Some(RunPhase::Validating);
            }
            "sft.training.metrics" | "sft.step.metrics" => {
                self.phase = Some(RunPhase::Training);
                self.train_loss = payload
                    .get("trainLoss")
                    .or_else(|| payload.get("train_loss"))
                    .or_else(|| payload.get("loss"))
                    .and_then(|v| v.as_f64());
                if let Some(step) = payload.get("step").and_then(|v| v.as_u64()) {
                    self.usage.steps = Some(step);
                }
            }
            "sft.checkpoint.ready" | "sft.checkpoint.created" => {
                let id = payload
                    .get("checkpointId")
                    .or_else(|| payload.get("checkpoint_id"))
                    .or_else(|| payload.get("id"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        KernelError::new(
                            KernelErrorCode::EventSchemaMismatch,
                            "sft.checkpoint.ready is missing checkpoint identity",
                        )
                    })?;
                self.checkpoints.push(id.to_string());
            }
            "sft.checkpoint.selected" | "sft.checkpoint.promoted" => {
                self.selected_checkpoint_id = payload
                    .get("checkpointId")
                    .or_else(|| payload.get("checkpoint_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
            }
            "sft.checkpoint_evaluation.started" | "training.evaluation.started" => {
                self.phase = Some(RunPhase::CheckpointEvaluation);
                let child = payload
                    .get("optimizerRunId")
                    .or_else(|| payload.get("childEvalRunId"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        KernelError::new(
                            KernelErrorCode::EventSchemaMismatch,
                            "SFT checkpoint evaluation must reference a child eval run",
                        )
                    })?;
                self.child_eval_run_ids.push(child.to_string());
                let mut item = WorkItem::planned(
                    format!("sft:ckpt-eval:{child}"),
                    WorkItemKind::CheckpointEvaluation,
                )?;
                item.transition(WorkItemLifecycle::Queued)?;
                item.transition(WorkItemLifecycle::Starting)?;
                item.transition(WorkItemLifecycle::Running)?;
                self.work_items.push(item);
            }
            "sft.checkpoint_evaluation.completed"
            | "sft.heldout_evaluation.completed"
            | "training.evaluation.completed" => {
                if let Some(child) = payload
                    .get("optimizerRunId")
                    .or_else(|| payload.get("childEvalRunId"))
                    .and_then(|v| v.as_str())
                {
                    if let Some(item) = self
                        .work_items
                        .iter_mut()
                        .find(|item| item.work_item_id == format!("sft:ckpt-eval:{child}"))
                    {
                        item.seal_terminal(TerminalKind::Completed)?;
                    }
                }
                self.phase = Some(RunPhase::HeldoutEvaluation);
            }
            "sft.adapter.materialized" | "sft.model.materialized" => {
                self.produced_adapter = payload
                    .get("adapterId")
                    .or_else(|| payload.get("artifactId"))
                    .or_else(|| payload.get("id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                self.phase = Some(RunPhase::Materializing);
            }
            _ => {}
        }
        apply_usage(&mut self.usage, event);
        Ok(())
    }

    pub fn work_summary(&self) -> WorkSummary {
        WorkSummary::from_items(&self.work_items, "checkpoint_evals", true)
    }

    /// Terminal seal closes interrupted children as `cancelled`, never failed.
    pub fn close_open_work(&mut self) -> KernelResult<usize> {
        close_open_items(&mut self.work_items)
    }

    pub fn evidence_state(&self) -> EvidenceState {
        let completeness =
            if self.produced_adapter.is_some() || self.selected_checkpoint_id.is_some() {
                EvidenceCompleteness::Complete
            } else if self.dataset_digest.is_some() {
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

    pub fn settle(&self) -> KernelResult<SftResult> {
        if self.dataset_digest.is_none() {
            return Err(KernelError::new(
                KernelErrorCode::EvidenceMissing,
                "SFT cannot settle without a dataset digest",
            ));
        }
        Ok(SftResult {
            dataset_digest: self.dataset_digest.clone(),
            selected_checkpoint_id: self.selected_checkpoint_id.clone(),
            produced_adapter: self.produced_adapter.clone(),
            child_eval_run_ids: self.child_eval_run_ids.clone(),
            train_loss: self.train_loss,
            usage: self.usage.clone(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SftResult {
    #[serde(default)]
    pub dataset_digest: Option<String>,
    #[serde(default)]
    pub selected_checkpoint_id: Option<String>,
    #[serde(default)]
    pub produced_adapter: Option<String>,
    pub child_eval_run_ids: Vec<String>,
    #[serde(default)]
    pub train_loss: Option<f64>,
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

