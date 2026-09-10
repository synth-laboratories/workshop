//! GO-EX / GELO: themes, candidates, proposers, child evals, frontier.
//!
//! Hosted remote status is an observed producer fact, not a second Workshop run
//! status. Canonical algorithm id is `go-ex`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::optimizers::kernel::error::{KernelError, KernelErrorCode, KernelResult};
use crate::optimizers::kernel::evidence::{EvidenceState, UsageCompleteness};
use crate::optimizers::kernel::sequences::CommittedEvent;
use crate::optimizers::kernel::types::{
    EvidenceCompleteness, RunPhase, TerminalKind, WorkItemKind, WorkItemLifecycle,
};
use crate::optimizers::kernel::work::{close_open_items, WorkItem, WorkSummary};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GoExCandidateSummary {
    pub id: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
    pub selected: bool,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub values: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub details: Value,
    #[specta(type = specta_typescript::Number)]
    pub sequence: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GoExProposerSummary {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[specta(type = specta_typescript::Unknown)]
    pub details: Value,
    #[specta(type = specta_typescript::Number)]
    pub sequence: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GoExChildSummary {
    pub id: String,
    #[serde(default)]
    pub candidate_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub reward: Option<f64>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[specta(type = specta_typescript::Unknown)]
    pub details: Value,
    #[specta(type = specta_typescript::Number)]
    pub sequence: u64,
}

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
    #[serde(default)]
    pub candidates: Vec<GoExCandidateSummary>,
    #[serde(default)]
    pub proposer_calls: Vec<GoExProposerSummary>,
    #[serde(default)]
    pub child_rollouts: Vec<GoExChildSummary>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub board: Value,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub frontier: Value,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub data_engine: Value,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub agents: Value,
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
                if let Some(id) = string_at(payload, &["candidate_id", "candidateId", "id"]) {
                    if !self.candidate_ids.iter().any(|existing| existing == &id) {
                        self.candidate_ids.push(id.to_string());
                    }
                }
                self.update_candidate(payload, "registered", event.aggregate_sequence);
                self.phase = Some(RunPhase::Selection);
            }
            "goex.seed_candidate_registered" => {
                self.update_candidate(payload, "registered", event.aggregate_sequence);
            }
            "candidate.evaluated" => {
                self.update_candidate(payload, "evaluated", event.aggregate_sequence);
            }
            "goex.best_base_decided" => {
                self.update_candidate(payload, "best_base", event.aggregate_sequence);
            }
            "goex.acceptance_completed" => {
                if let Some(champion) =
                    string_at(payload, &["champion_candidate_id", "championCandidateId"])
                {
                    self.mark_candidate(&champion, "accepted", true, event.aggregate_sequence);
                    self.selected_candidate_id = Some(champion);
                }
                if let Some(baseline) =
                    string_at(payload, &["baseline_candidate_id", "baselineCandidateId"])
                {
                    if self.selected_candidate_id.as_deref() != Some(&baseline) {
                        self.mark_candidate(&baseline, "rejected", false, event.aggregate_sequence);
                    }
                }
            }
            "goex.tick_transition" => {
                merge_object(&mut self.board, payload);
            }
            "goex.core_proposer_started" => {
                self.update_proposer(payload, "running", event.aggregate_sequence);
            }
            "goex.core_proposer_finished" => {
                self.update_proposer(payload, "completed", event.aggregate_sequence);
            }
            "proposer.delta" => {
                // Deliberately retain identity/status only. Streaming token
                // chunks belong in evidence/artifacts, not this projection.
                self.update_proposer(payload, "running", event.aggregate_sequence);
            }
            "goex.theme_state_changed" => {
                if let Some(theme) = string_at(payload, &["theme_id", "themeId", "id"]) {
                    if !self.themes.iter().any(|existing| existing == &theme) {
                        self.themes.push(theme);
                    }
                }
            }
            event_type if event_type.starts_with("child.rollout.") => {
                self.update_child_rollout(payload, event.aggregate_sequence);
            }
            "goex.state.batch.updated" => {
                self.apply_state_batch(payload, event.aggregate_sequence);
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
                self.frontier = payload.clone();
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

    fn update_candidate(&mut self, payload: &Value, default_status: &str, sequence: u64) {
        let Some(id) = string_at(payload, &["candidate_id", "candidateId", "id"]) else {
            return;
        };
        if !self.candidate_ids.iter().any(|existing| existing == &id) {
            self.candidate_ids.push(id.clone());
        }
        let status =
            string_at(payload, &["status", "decision"]).or_else(|| Some(default_status.into()));
        let score = number_at(payload, &["score", "reward", "objective", "mean_reward"]);
        let values = payload
            .get("values")
            .or_else(|| payload.get("value"))
            .cloned()
            .unwrap_or(Value::Null);
        let parent_id = string_at(payload, &["parent_id", "parentId", "base_candidate_id"]);
        let selected = payload
            .get("selected")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let next = GoExCandidateSummary {
            id: id.clone(),
            status,
            score,
            selected,
            parent_id,
            values,
            details: payload.clone(),
            sequence,
        };
        match self.candidates.iter_mut().find(|row| row.id == id) {
            Some(existing) => {
                if !next.values.is_null() {
                    existing.values = next.values.clone();
                }
                existing.status = next.status.or_else(|| existing.status.clone());
                existing.score = next.score.or(existing.score);
                existing.selected |= next.selected;
                existing.parent_id = next.parent_id.or_else(|| existing.parent_id.clone());
                existing.details = next.details;
                existing.sequence = sequence;
            }
            None => self.candidates.push(next),
        }
    }

    fn mark_candidate(&mut self, id: &str, status: &str, selected: bool, sequence: u64) {
        if !self.candidate_ids.iter().any(|existing| existing == id) {
            self.candidate_ids.push(id.into());
        }
        if let Some(candidate) = self.candidates.iter_mut().find(|row| row.id == id) {
            candidate.status = Some(status.into());
            candidate.selected = selected;
            candidate.sequence = sequence;
        } else {
            self.candidates.push(GoExCandidateSummary {
                id: id.into(),
                status: Some(status.into()),
                selected,
                sequence,
                values: Value::Null,
                details: json!({ "candidateId": id }),
                ..Default::default()
            });
        }
    }

    fn update_proposer(&mut self, payload: &Value, status: &str, sequence: u64) {
        let id = string_at(payload, &["call_id", "callId", "proposer_id", "id"])
            .unwrap_or_else(|| "core_proposer".into());
        let next = GoExProposerSummary {
            id: id.clone(),
            status: status.into(),
            model: string_at(payload, &["model", "model_id"]),
            cost_usd: number_at(payload, &["costUsd", "cost_usd"]),
            // Never persist token deltas into the read model.
            details: if payload.get("text").is_some() {
                json!({ "channel": payload.get("channel") })
            } else {
                payload.clone()
            },
            sequence,
        };
        match self.proposer_calls.iter_mut().find(|row| row.id == id) {
            Some(existing) => {
                existing.status = next.status;
                existing.model = next.model.or_else(|| existing.model.clone());
                existing.cost_usd = next.cost_usd.or(existing.cost_usd);
                if payload.get("text").is_none() {
                    existing.details = next.details;
                }
                existing.sequence = sequence;
            }
            None => self.proposer_calls.push(next),
        }
    }

    fn update_child_rollout(&mut self, payload: &Value, sequence: u64) {
        let resource = payload.get("resource_ref").and_then(Value::as_object);
        let id = resource
            .and_then(|value| value.get("rollout_id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| string_at(payload, &["rollout_id", "rolloutId", "id"]));
        let Some(id) = id else {
            return;
        };
        let next = GoExChildSummary {
            id: id.clone(),
            candidate_id: string_at(payload, &["candidate_id", "candidateId"]),
            status: string_at(payload, &["status"]),
            reward: number_at(payload, &["reward"]),
            cost_usd: number_at(payload, &["costUsd", "cost_usd"]),
            details: payload.clone(),
            sequence,
        };
        match self.child_rollouts.iter_mut().find(|row| row.id == id) {
            Some(existing) => *existing = next,
            None => self.child_rollouts.push(next),
        }
    }

    fn apply_state_batch(&mut self, payload: &Value, sequence: u64) {
        let Some(slices) = payload.get("slices").and_then(Value::as_object) else {
            return;
        };
        let data = |name: &str| slices.get(name).and_then(|value| value.get("data"));
        if let Some(value) = data("board") {
            self.board = value.clone();
        }
        if let Some(value) = data("frontier") {
            self.frontier = value.clone();
        }
        if let Some(value) = data("data-engine") {
            self.data_engine = value.clone();
        }
        if let Some(value) = data("agents") {
            self.agents = value.clone();
        }
        if let Some(rows) = data("themes")
            .and_then(|value| value.get("themes").or(Some(value)))
            .and_then(Value::as_array)
        {
            self.themes = rows
                .iter()
                .filter_map(|row| string_at(row, &["theme_id", "id"]))
                .collect();
        }
        if let Some(rows) = data("candidates")
            .and_then(|value| value.get("candidates").or(Some(value)))
            .and_then(Value::as_array)
        {
            for row in rows {
                self.update_candidate(row, "registered", sequence);
            }
        }
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

fn string_at(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn number_at(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(Value::as_f64))
}

fn merge_object(target: &mut Value, source: &Value) {
    if !target.is_object() {
        *target = json!({});
    }
    let Some(target) = target.as_object_mut() else {
        return;
    };
    let Some(source) = source.as_object() else {
        return;
    };
    for (key, value) in source {
        target.insert(key.clone(), value.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::kernel::sequences::ProducerEvent;
    use crate::optimizers::kernel::types::PRODUCER_EVENT_SCHEMA_VERSION;

    fn committed(event_type: &str, payload: Value, sequence: u64) -> CommittedEvent {
        CommittedEvent {
            aggregate_sequence: sequence,
            committed_at: "2026-09-01T20:00:00Z".into(),
            producer: ProducerEvent {
                producer_id: "goex".into(),
                producer_sequence: sequence,
                idempotency_key: format!("{event_type}:{sequence}"),
                schema_version: PRODUCER_EVENT_SCHEMA_VERSION.into(),
                algorithm_id: "go-ex".into(),
                event_type: event_type.into(),
                occurred_at: "2026-09-01T20:00:00Z".into(),
                payload_digest: String::new(),
                payload,
            }
            .with_computed_digest(),
        }
    }

    #[test]
    fn candidate_proposer_and_child_rows_are_durable_projection_facts() {
        let mut projection = GoExProjection::default();
        projection.apply(&committed(
            "candidate.registered",
            json!({"candidate_id": "cand-a", "values": {"prompt": "durable"}, "parent_id": "seed"}),
            1,
        )).unwrap();
        projection
            .apply(&committed(
                "candidate.evaluated",
                json!({"candidate_id": "cand-a", "score": 0.82, "status": "evaluated"}),
                2,
            ))
            .unwrap();
        projection
            .apply(&committed(
                "goex.core_proposer_finished",
                json!({"call_id": "proposal-1", "model": "luna", "costUsd": 0.04}),
                3,
            ))
            .unwrap();
        projection.apply(&committed(
            "child.rollout.completed",
            json!({"resource_ref": {"rollout_id": "roll-1"}, "candidate_id": "cand-a", "status": "completed", "reward": 1.0}),
            4,
        )).unwrap();

        assert_eq!(projection.candidates[0].values["prompt"], "durable");
        assert_eq!(projection.candidates[0].score, Some(0.82));
        assert_eq!(projection.proposer_calls[0].model.as_deref(), Some("luna"));
        assert_eq!(
            projection.child_rollouts[0].candidate_id.as_deref(),
            Some("cand-a")
        );
    }

    #[test]
    fn proposer_token_deltas_do_not_accumulate_in_the_projection() {
        let mut projection = GoExProjection::default();
        for sequence in 1..=100 {
            projection.apply(&committed(
                "proposer.delta",
                json!({"call_id": "proposal-1", "channel": "content", "text": "a very long token chunk"}),
                sequence,
            )).unwrap();
        }
        let encoded = serde_json::to_vec(&projection.proposer_calls[0]).unwrap();
        assert!(
            encoded.len() < 512,
            "streaming text leaked into the read model"
        );
        assert!(projection.proposer_calls[0].details.get("text").is_none());
    }
}
