//! Versioned backend projection the renderer formats. Raw events are diagnostic.

use serde::{Deserialize, Serialize};

use super::algorithm::AlgorithmResult;
use super::commit::RunKernelState;
use super::evidence::{EvidenceState, SealedTerminal, UsageCompleteness};
use super::types::{
    AlgorithmKind, ExecutionPlacement, RunCondition, RunLifecycle, RunPhase,
    RUN_VIEW_SCHEMA_VERSION,
};
use super::work::WorkSummary;
use crate::optimizers::models::{
    EffectiveContract, OptimizerExecutionBinding, OptimizerResourceRef, OptimizerRunArtifact,
    OptimizerRunRecord,
};

/// Product metadata stored on the admitted run record rather than re-derived
/// from producer events. Keeping it beside the kernel projection makes the V2
/// view sufficient for every ordinary renderer surface.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunViewContext {
    pub execution_bindings: Vec<OptimizerExecutionBinding>,
    pub input_refs: Vec<OptimizerResourceRef>,
    pub output_refs: Vec<OptimizerResourceRef>,
    pub visual_refs: Vec<OptimizerResourceRef>,
    pub artifacts: Vec<OptimizerRunArtifact>,
    pub effective_contract: Option<EffectiveContract>,
}

impl From<&OptimizerRunRecord> for RunViewContext {
    fn from(run: &OptimizerRunRecord) -> Self {
        Self {
            execution_bindings: run.execution_bindings.clone(),
            input_refs: run.input_refs.clone(),
            output_refs: run.output_refs.clone(),
            visual_refs: run.visual_refs.clone(),
            artifacts: vec![],
            effective_contract: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerRunHeader {
    pub schema_version: String,
    pub run_id: String,
    pub algorithm: AlgorithmKind,
    pub lifecycle: RunLifecycle,
    #[serde(default)]
    pub phase: Option<RunPhase>,
    pub condition: RunCondition,
    pub placement: ExecutionPlacement,
    /// The admitted spec is one-to-one with the run in the current schema.
    pub spec_id: String,
    pub spec_digest: String,
    pub execution_bindings: Vec<OptimizerExecutionBinding>,
    pub input_refs: Vec<OptimizerResourceRef>,
    pub output_refs: Vec<OptimizerResourceRef>,
    pub visual_refs: Vec<OptimizerResourceRef>,
    pub artifacts: Vec<OptimizerRunArtifact>,
    #[serde(default)]
    pub effective_contract: Option<EffectiveContract>,
    pub usage: UsageCompleteness,
    pub work: WorkSummary,
    pub evidence: EvidenceState,
    #[serde(default)]
    pub failure_ref: Option<String>,
    #[serde(default)]
    pub terminal: Option<SealedTerminal>,
    pub projection_schema_version: String,
    #[specta(type = specta_typescript::Number)]
    pub as_of_sequence: u64,
    #[specta(type = specta_typescript::Number)]
    pub projection_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "algorithm", rename_all = "kebab-case")]
pub enum OptimizerRunViewV2 {
    Eval(EvalRunView),
    Gepa(GepaRunView),
    GoEx(GoExRunView),
    Sft(SftRunView),
    Cispo(CispoRunView),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EvalRunView {
    pub header: OptimizerRunHeader,
    pub projection: super::algorithms::eval::EvalProjection,
    pub aggregate: super::algorithms::eval::EvalAggregate,
    pub result: Option<super::algorithms::eval::EvalResult>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GepaRunView {
    pub header: OptimizerRunHeader,
    pub projection: super::algorithms::gepa::GepaProjection,
    pub result: Option<super::algorithms::gepa::GepaResult>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GoExRunView {
    pub header: OptimizerRunHeader,
    pub projection: super::algorithms::go_ex::GoExProjection,
    pub result: Option<super::algorithms::go_ex::GoExResult>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SftRunView {
    pub header: OptimizerRunHeader,
    pub projection: super::algorithms::sft::SftProjection,
    pub result: Option<super::algorithms::sft::SftResult>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CispoRunView {
    pub header: OptimizerRunHeader,
    pub projection: super::algorithms::cispo::CispoProjection,
    pub result: Option<super::algorithms::cispo::CispoResult>,
}

pub fn project_view(state: &RunKernelState) -> OptimizerRunViewV2 {
    project_view_with_context(state, &RunViewContext::default())
}

pub fn project_view_with_context(
    state: &RunKernelState,
    context: &RunViewContext,
) -> OptimizerRunViewV2 {
    let header = OptimizerRunHeader {
        schema_version: RUN_VIEW_SCHEMA_VERSION.into(),
        run_id: state.run_id.clone(),
        algorithm: state.algorithm,
        lifecycle: state.lifecycle,
        phase: state.phase,
        condition: state.condition,
        placement: state.placement,
        spec_id: state.run_id.clone(),
        spec_digest: state.spec_digest.clone(),
        execution_bindings: context.execution_bindings.clone(),
        input_refs: context.input_refs.clone(),
        output_refs: context.output_refs.clone(),
        visual_refs: context.visual_refs.clone(),
        artifacts: context.artifacts.clone(),
        effective_contract: context.effective_contract.clone(),
        usage: state.usage(),
        work: state.work_summary(),
        evidence: state.evidence_state(),
        failure_ref: state.failure_ref.clone(),
        terminal: state.terminal.clone(),
        projection_schema_version: state.algorithm.reducer_version().into(),
        as_of_sequence: state.aggregate_sequence,
        projection_revision: state.projection_revision,
    };
    let result = state.projection.settle().ok();
    match (&state.projection, result) {
        (
            super::algorithm::AlgorithmProjection::Eval(projection),
            Some(AlgorithmResult::Eval(result)),
        ) => OptimizerRunViewV2::Eval(eval_run_view(
            header,
            eval_projection_with_artifacts(projection, &context.artifacts),
            Some(result),
        )),
        (super::algorithm::AlgorithmProjection::Eval(projection), _) => {
            OptimizerRunViewV2::Eval(eval_run_view(
                header,
                eval_projection_with_artifacts(projection, &context.artifacts),
                None,
            ))
        }
        (
            super::algorithm::AlgorithmProjection::Gepa(projection),
            Some(AlgorithmResult::Gepa(result)),
        ) => OptimizerRunViewV2::Gepa(GepaRunView {
            header,
            projection: projection.clone(),
            result: Some(result),
        }),
        (super::algorithm::AlgorithmProjection::Gepa(projection), _) => {
            OptimizerRunViewV2::Gepa(GepaRunView {
                header,
                projection: projection.clone(),
                result: None,
            })
        }
        (
            super::algorithm::AlgorithmProjection::GoEx(projection),
            Some(AlgorithmResult::GoEx(result)),
        ) => OptimizerRunViewV2::GoEx(GoExRunView {
            header,
            projection: projection.clone(),
            result: Some(result),
        }),
        (super::algorithm::AlgorithmProjection::GoEx(projection), _) => {
            OptimizerRunViewV2::GoEx(GoExRunView {
                header,
                projection: projection.clone(),
                result: None,
            })
        }
        (
            super::algorithm::AlgorithmProjection::Sft(projection),
            Some(AlgorithmResult::Sft(result)),
        ) => OptimizerRunViewV2::Sft(SftRunView {
            header,
            projection: projection.clone(),
            result: Some(result),
        }),
        (super::algorithm::AlgorithmProjection::Sft(projection), _) => {
            OptimizerRunViewV2::Sft(SftRunView {
                header,
                projection: projection.clone(),
                result: None,
            })
        }
        (
            super::algorithm::AlgorithmProjection::Cispo(projection),
            Some(AlgorithmResult::Cispo(result)),
        ) => OptimizerRunViewV2::Cispo(CispoRunView {
            header,
            projection: projection.clone(),
            result: Some(result),
        }),
        (super::algorithm::AlgorithmProjection::Cispo(projection), _) => {
            OptimizerRunViewV2::Cispo(CispoRunView {
                header,
                projection: projection.clone(),
                result: None,
            })
        }
    }
}

fn eval_run_view(
    header: OptimizerRunHeader,
    projection: super::algorithms::eval::EvalProjection,
    result: Option<super::algorithms::eval::EvalResult>,
) -> EvalRunView {
    let aggregate = super::algorithms::eval::EvalAggregate {
        schema_version: super::algorithms::eval::EVAL_AGGREGATE_SCHEMA_VERSION.into(),
        run_id: header.run_id.clone(),
        as_of_sequence: header.as_of_sequence,
        projection_revision: header.projection_revision,
        lifecycle: header.lifecycle,
        work: header.work.clone(),
        evidence: header.evidence.clone(),
        selection: projection.selection_outcome(),
        mean_reward: projection.mean_reward,
        scored_trials: projection.scored_trials,
        evaluator_evidence: projection.evaluator_evidence,
        trace_count: projection.traces,
        evidence_ref_count: projection.evidence_refs.len(),
    };
    EvalRunView {
        header,
        projection,
        aggregate,
        result,
    }
}

fn eval_projection_with_artifacts(
    projection: &super::algorithms::eval::EvalProjection,
    artifacts: &[OptimizerRunArtifact],
) -> super::algorithms::eval::EvalProjection {
    let mut projection = projection.clone();
    for item in &mut projection.work_items {
        let evidence_identity = projection
            .evidence_ledger
            .iter()
            .find(|entry| entry.work_item_id == item.work_item_id);
        item.artifact_refs = artifacts
            .iter()
            .filter(|artifact| {
                artifact.work_item_id.as_deref() == Some(item.work_item_id.as_str())
                    || item
                        .external_ref
                        .as_deref()
                        .is_some_and(|external| artifact.rollout_id.as_deref() == Some(external))
                    || evidence_identity.is_some_and(|entry| {
                        artifact.rollout_id.as_deref() == entry.rollout_id.as_deref()
                            || artifact.rollout_id.as_deref() == entry.trial_id.as_deref()
                    })
            })
            .cloned()
            .collect();
    }
    projection
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::kernel::admission::{AdmissionCommit, RunDraft};
    use crate::optimizers::kernel::commit::commit;
    use crate::optimizers::kernel::sequences::{DurableProducerLog, ProducerEvent};
    use crate::optimizers::kernel::types::{AdmissionState, PRODUCER_EVENT_SCHEMA_VERSION};
    use serde_json::json;

    #[test]
    fn view_is_discriminated_by_algorithm_not_raw_events() {
        let mut draft = RunDraft::new("d", AlgorithmKind::Eval, "sha256:s", "{}", "now");
        draft.transition(AdmissionState::Validating, "now").unwrap();
        draft
            .transition(AdmissionState::AwaitingApproval, "now")
            .unwrap();
        draft.transition(AdmissionState::Approved, "now").unwrap();
        let commit_spec = AdmissionCommit::from_approved_draft(
            &draft,
            "run-e",
            ExecutionPlacement::DirectContainerEvaluation,
            "now",
        )
        .unwrap();
        let state = RunKernelState::from_admission(&commit_spec);
        let planned = ProducerEvent {
            producer_id: "eval".into(),
            producer_sequence: 1,
            idempotency_key: "plan".into(),
            schema_version: PRODUCER_EVENT_SCHEMA_VERSION.into(),
            algorithm_id: "eval".into(),
            event_type: "eval.run.planned".into(),
            occurred_at: "2026-08-27T18:00:00Z".into(),
            payload_digest: String::new(),
            payload: json!({"plannedTrials": 2, "candidates": ["baseline"], "seeds": [1], "scenarios": ["s"]}),
        }
        .with_computed_digest();
        let plan = commit(state, &DurableProducerLog::default(), &[planned], "now").unwrap();
        match project_view(&plan.state) {
            OptimizerRunViewV2::Eval(view) => {
                assert_eq!(view.header.algorithm, AlgorithmKind::Eval);
                assert_eq!(view.header.as_of_sequence, 1);
                assert!(
                    view.result.is_none(),
                    "a planned live run has no settled result"
                );
            }
            other => panic!("expected eval view, got {other:?}"),
        }
    }

    #[test]
    fn rollout_ledger_identity_joins_declared_artifacts_to_trial_rows() {
        use crate::optimizers::kernel::algorithms::eval::{
            EvalProjection, RolloutEvidenceEntry, RolloutEvidenceState,
        };
        use crate::optimizers::kernel::{WorkItem, WorkItemKind};

        let projection = EvalProjection {
            work_items: vec![WorkItem::planned("trial-1", WorkItemKind::EvalTrial).unwrap()],
            evidence_ledger: vec![RolloutEvidenceEntry {
                work_item_id: "trial-1".into(),
                rollout_id: Some("rollout-1".into()),
                state: RolloutEvidenceState::Open,
                ..RolloutEvidenceEntry::default()
            }],
            ..EvalProjection::default()
        };
        let artifact = OptimizerRunArtifact {
            schema_version: "optimizer_run_artifact.v1".into(),
            optimizer_run_id: "run-1".into(),
            artifact_id: "video-1".into(),
            sequence: 3,
            work_item_id: None,
            rollout_id: Some("rollout-1".into()),
            kind: "rollout_video".into(),
            locator: "/tmp/video-1.mp4".into(),
            digest: None,
            media_type: Some("video/mp4".into()),
            byte_size: None,
            metadata: json!({}),
            declared_at: "2026-08-27T18:00:00Z".into(),
        };

        let joined = eval_projection_with_artifacts(&projection, &[artifact]);
        assert_eq!(joined.work_items[0].artifact_refs.len(), 1);
        assert_eq!(joined.work_items[0].artifact_refs[0].artifact_id, "video-1");
    }
}
