//! GO-EX / GELO: themes, candidates, proposers, child evals, frontier.
//!
//! Hosted remote status is an observed producer fact, not a second Workshop run
//! status. Canonical algorithm id is `go-ex`.

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
pub struct GoExProjection {
    pub work_items: Vec<WorkItem>,
    pub phase: Option<RunPhase>,
    pub usage: UsageCompleteness,
    pub themes: Vec<String>,
    pub candidate_ids: Vec<String>,
    pub selected_candidate_id: Option<String>,
    pub remote_status: Option<String>,
    pub child_eval_run_ids: Vec<String>,
}

impl GoExProjection {
    pub fn apply(&mut self, event: &CommittedEvent) -> KernelResult<()> {
        let payload = &event.producer.payload;
        match event.producer.event_type.as_str() {
            "go-ex.theme.updated" | "goex.theme.updated" => {
                if let Some(theme) = payload
                    .get("themeId")
                    .or_else(|| payload.get("id"))
                    .and_then(|v| v.as_str())
                {
                    if !self.themes.iter().any(|existing| existing == theme) {
                        self.themes.push(theme.to_string());
                    }
                }
            }
            "go-ex.candidate.registered" | "candidate.registered" => {
                if let Some(id) = payload.get("candidate_id").and_then(|v| v.as_str()) {
                    if !self.candidate_ids.iter().any(|existing| existing == id) {
                        self.candidate_ids.push(id.to_string());
                    }
                }
                self.phase = Some(RunPhase::Selection);
            }
            "go-ex.child_eval.attached" => {
                let child = payload
                    .get("optimizerRunId")
                    .or_else(|| payload.get("childEvalRunId"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        KernelError::new(
                            KernelErrorCode::EventSchemaMismatch,
                            "GO-EX child eval is missing an optimizer run id",
                        )
                    })?;
                self.child_eval_run_ids.push(child.to_string());
                let mut item = WorkItem::planned(
                    format!("go-ex:child-eval:{child}"),
                    WorkItemKind::CandidateEvaluation,
                )?;
                item.transition(WorkItemLifecycle::Queued)?;
                self.work_items.push(item);
            }
            "go-ex.remote.status" => {
                self.remote_status = payload
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
            }
            "go-ex.frontier.updated" | "frontier.updated" => {
                self.selected_candidate_id = payload
                    .get("best_candidate_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
            }
            "goex.run_finished" | "go-ex.run.finished" => {
                self.selected_candidate_id = self.selected_candidate_id.clone().or_else(|| {
                    payload
                        .get("selected_candidate_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                });
                self.phase = Some(RunPhase::Materializing);
                for item in &mut self.work_items {
                    if item.lifecycle != WorkItemLifecycle::Terminal {
                        item.seal_terminal(TerminalKind::Completed)?;
                    }
                }
            }
            _ => {}
        }
        apply_usage(&mut self.usage, event);
        Ok(())
    }

    pub fn work_summary(&self) -> WorkSummary {
        WorkSummary::from_items(&self.work_items, "child_evals", false)
    }

    /// Terminal seal closes interrupted children as `cancelled`, never failed.
    pub fn close_open_work(&mut self) -> KernelResult<usize> {
        close_open_items(&mut self.work_items)
    }

    pub fn evidence_state(&self) -> EvidenceState {
        let completeness = if self.selected_candidate_id.is_some() {
            EvidenceCompleteness::Complete
        } else if !self.candidate_ids.is_empty() {
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

    pub fn settle(&self) -> KernelResult<GoExResult> {
        if self.candidate_ids.is_empty() {
            return Err(KernelError::new(
                KernelErrorCode::EvidenceMissing,
                "GO-EX cannot settle without candidates",
            ));
        }
        Ok(GoExResult {
            selected_candidate_id: self.selected_candidate_id.clone(),
            themes: self.themes.len() as u64,
            candidates: self.candidate_ids.len() as u64,
            child_eval_run_ids: self.child_eval_run_ids.clone(),
            remote_status: self.remote_status.clone(),
            usage: self.usage.clone(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GoExResult {
    #[serde(default)]
    pub selected_candidate_id: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub themes: u64,
    #[specta(type = specta_typescript::Number)]
    pub candidates: u64,
    pub child_eval_run_ids: Vec<String>,
    #[serde(default)]
    pub remote_status: Option<String>,
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
