//! The durable run kernel: common lifecycle reducer plus algorithm dispatch.
//!
//! Extracted from `service::commit_validated_events`. Persistence still lives
//! in CoreRuntime; this module decides what the committed facts mean.

use serde::{Deserialize, Serialize};

use super::admission::AdmissionCommit;
use super::algorithm::{AlgorithmProjection, AlgorithmResult};
use super::error::{KernelError, KernelErrorCode, KernelResult};
use super::evidence::{EvidenceState, SealedTerminal, UsageCompleteness};
use super::sequences::{
    assign_aggregate_sequences, plan_producer_batch, CommittedEvent, DurableProducerLog,
    ProducerEvent, ProducerVerdict,
};
use super::types::{
    AlgorithmKind, ExecutionPlacement, RunCondition, RunLifecycle, RunPhase, TerminalKind,
    TerminalReason,
};
use super::work::WorkSummary;
use super::KERNEL_SCHEMA_VERSION;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunKernelState {
    pub schema_version: String,
    pub run_id: String,
    pub algorithm: AlgorithmKind,
    pub lifecycle: RunLifecycle,
    #[serde(default)]
    pub phase: Option<RunPhase>,
    pub condition: RunCondition,
    pub placement: ExecutionPlacement,
    pub aggregate_sequence: u64,
    pub projection_revision: u64,
    pub spec_digest: String,
    #[serde(default)]
    pub terminal: Option<SealedTerminal>,
    #[serde(default)]
    pub failure_ref: Option<String>,
    /// Current evidence may include append-only facts recorded after the
    /// terminal sequence. The sealed terminal keeps its historical evidence.
    #[serde(default)]
    pub current_evidence: Option<EvidenceState>,
    pub projection: AlgorithmProjection,
}

impl RunKernelState {
    pub fn from_admission(commit: &AdmissionCommit) -> Self {
        Self::new(
            commit.run_id.clone(),
            commit.algorithm,
            commit.placement,
            commit.spec_digest.clone(),
        )
    }

    pub fn new(
        run_id: impl Into<String>,
        algorithm: AlgorithmKind,
        placement: ExecutionPlacement,
        spec_digest: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: KERNEL_SCHEMA_VERSION.into(),
            run_id: run_id.into(),
            algorithm,
            lifecycle: RunLifecycle::Queued,
            phase: None,
            condition: RunCondition::Healthy,
            placement,
            aggregate_sequence: 0,
            projection_revision: 0,
            spec_digest: spec_digest.into(),
            terminal: None,
            failure_ref: None,
            current_evidence: None,
            projection: AlgorithmProjection::new(algorithm),
        }
    }

    pub fn work_summary(&self) -> WorkSummary {
        self.projection.work_summary()
    }

    pub fn evidence_state(&self) -> EvidenceState {
        self.current_evidence
            .clone()
            .unwrap_or_else(|| self.projection.evidence_state())
    }

    pub fn usage(&self) -> UsageCompleteness {
        self.projection.usage()
    }
}

#[derive(Clone, Debug)]
pub struct CommitPlan {
    pub events: Vec<CommittedEvent>,
    pub replayed: usize,
    pub state: RunKernelState,
}

/// Validate producer order, assign aggregate sequences, apply the common
/// lifecycle reducer and the registered algorithm reducer.
pub fn commit(
    mut state: RunKernelState,
    log: &DurableProducerLog,
    batch: &[ProducerEvent],
    committed_at: &str,
) -> KernelResult<CommitPlan> {
    if state.lifecycle.is_terminal() && state.terminal.is_some() {
        // Replays of already-committed producer events are allowed; new facts
        // that would move a sealed run are not. Evidence amendments are the
        // sole append-only lane after sealing and retain the original terminal
        // sequence.
        let plan = plan_producer_batch(log, batch)?;
        if batch.iter().zip(&plan).any(|(event, verdict)| {
            *verdict == ProducerVerdict::Append && event.event_type != "optimizer.evidence.amended"
        }) {
            return Err(KernelError::new(
                KernelErrorCode::TerminalAlreadySealed,
                format!(
                    "run {} is sealed; refusing new producer facts",
                    state.run_id
                ),
            ));
        }
        if plan
            .iter()
            .all(|verdict| *verdict == ProducerVerdict::ConfirmedReplay)
        {
            return Ok(CommitPlan {
                events: Vec::new(),
                replayed: plan.len(),
                state,
            });
        }
    }

    let verdicts = plan_producer_batch(log, batch)?;
    let committed =
        assign_aggregate_sequences(state.aggregate_sequence, committed_at, batch, &verdicts)?;
    let replayed = verdicts
        .iter()
        .filter(|verdict| **verdict == ProducerVerdict::ConfirmedReplay)
        .count();
    for event in &committed {
        if state.terminal.is_some() && event.producer.event_type != "optimizer.evidence.amended" {
            return Err(KernelError::new(
                KernelErrorCode::TerminalAlreadySealed,
                format!(
                    "run {} sealed at sequence {}; refusing {}",
                    state.run_id,
                    state
                        .terminal
                        .as_ref()
                        .map(|terminal| terminal.final_sequence)
                        .unwrap_or(state.aggregate_sequence),
                    event.producer.event_type
                ),
            ));
        }
        if event.producer.event_type == "optimizer.evidence.amended" {
            let terminal = state.terminal.as_ref().ok_or_else(|| {
                KernelError::new(
                    KernelErrorCode::TerminalPrerequisitesUnmet,
                    "evidence amendment requires a sealed terminal",
                )
            })?;
            let linked_sequence = event
                .producer
                .payload
                .get("terminalSequence")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    KernelError::new(
                        KernelErrorCode::EventSchemaMismatch,
                        "evidence amendment is missing typed terminalSequence",
                    )
                })?;
            if linked_sequence != terminal.final_sequence {
                return Err(KernelError::new(
                    KernelErrorCode::TerminalAlreadySealed,
                    format!(
                        "evidence amendment must link to terminal sequence {}, not {linked_sequence}",
                        terminal.final_sequence
                    ),
                ));
            }
        }
        let terminal_outcome = apply_lifecycle(&mut state, event)?;
        state.projection.apply(event)?;
        state.aggregate_sequence = event.aggregate_sequence;
        state.projection_revision += 1;
        state.phase = state.projection.phase().or(state.phase);
        if let Some((kind, reason)) = terminal_outcome {
            seal_terminal(&mut state, event, kind, reason)?;
        }
    }
    Ok(CommitPlan {
        events: committed,
        replayed,
        state,
    })
}

fn apply_lifecycle(
    state: &mut RunKernelState,
    event: &CommittedEvent,
) -> KernelResult<Option<(TerminalKind, Option<TerminalReason>)>> {
    if event
        .producer
        .payload
        .get("kernelLifecycleFact")
        .and_then(|value| value.as_bool())
        == Some(false)
    {
        return Ok(None);
    }
    let next = match event.producer.event_type.as_str() {
        "optimizer.run.queued" => Some(RunLifecycle::Queued),
        "optimizer.run.starting" => Some(RunLifecycle::Starting),
        "optimizer.run.created" | "optimizer.run.started" | "run.started" => {
            Some(RunLifecycle::Running)
        }
        "optimizer.run.paused" => Some(RunLifecycle::Paused),
        "optimizer.run.resumed" => Some(RunLifecycle::Running),
        "optimizer.run.cancelling" => Some(RunLifecycle::Cancelling),
        "optimizer.run.completed"
        | "gepa.run.finished"
        | "goex.run_finished"
        | "go-ex.run.finished"
        | "run.completed" => Some(RunLifecycle::Terminal),
        "optimizer.run.failed" | "run.failed" => Some(RunLifecycle::Terminal),
        "optimizer.run.degraded" | "run.degraded" => Some(RunLifecycle::Terminal),
        "optimizer.run.cancelled" | "run.cancelled" => Some(RunLifecycle::Terminal),
        _ => None,
    };
    let mut terminal_outcome = None;
    if let Some(next) = next {
        state.lifecycle = state.lifecycle.transition_to(next)?;
        if next == RunLifecycle::Terminal {
            terminal_outcome = Some(match event.producer.event_type.as_str() {
                "optimizer.run.failed" | "run.failed" => {
                    (TerminalKind::Failed, Some(TerminalReason::ProducerFailed))
                }
                "optimizer.run.cancelled" | "run.cancelled" => {
                    // A typed request on the event names its cause; user and
                    // agent causes settle operator_cancelled, systemic causes
                    // settle interrupted. Absent provenance (legacy events)
                    // keeps the operator reading.
                    let reason = event
                        .producer
                        .payload
                        .pointer("/cancellation/cause")
                        .and_then(serde_json::Value::as_str)
                        .and_then(super::types::CancellationCause::parse)
                        .map(super::types::CancellationCause::terminal_reason)
                        .unwrap_or(TerminalReason::OperatorCancelled);
                    (TerminalKind::Cancelled, Some(reason))
                }
                "optimizer.run.degraded" | "run.degraded" => (TerminalKind::Degraded, None),
                _ => (TerminalKind::Completed, None),
            });
        }
    }
    if event.producer.event_type == "optimizer.condition.environment_unreachable" {
        state.condition = RunCondition::EnvironmentUnreachable;
    }
    if event.producer.event_type == "optimizer.condition.healthy" {
        state.condition = RunCondition::Healthy;
    }
    Ok(terminal_outcome)
}

fn seal_terminal(
    state: &mut RunKernelState,
    event: &CommittedEvent,
    kind: TerminalKind,
    reason: Option<TerminalReason>,
) -> KernelResult<()> {
    if state.terminal.is_some() {
        return Err(KernelError::new(
            KernelErrorCode::TerminalAlreadySealed,
            format!("run {} already has a sealed terminal", state.run_id),
        ));
    }
    let (kind, reason) = if kind == TerminalKind::Completed {
        match state.projection.settle() {
            Ok(_) => (kind, reason),
            // The producer's terminal fact is authoritative for execution
            // lifecycle. Missing evaluation evidence must therefore seal as a
            // typed failed-evidence outcome instead of rolling back the whole
            // batch and leaving a durable run looking live forever.
            Err(error) if error.code == KernelErrorCode::EvidenceMissing => {
                (TerminalKind::Failed, Some(TerminalReason::EvidenceUnusable))
            }
            Err(error) => return Err(error),
        }
    } else {
        (kind, reason)
    };
    // Any terminal kind closes the run's open work: interrupted children
    // settle `cancelled`, never `failed`. Judged after `settle()` so a
    // `completed` fact over unfinished work still seals as failed evidence,
    // and before the evidence snapshot so the sealed evidence is closed-world.
    // Pure over the terminal event, so replay reproduces the closure.
    state.projection.close_open_work()?;
    let evidence = state.projection.evidence_state();
    state.terminal = Some(SealedTerminal {
        kind,
        reason,
        final_sequence: event.aggregate_sequence,
        evidence,
        failure_ref: state.failure_ref.clone(),
        sealed_at: event.committed_at.clone(),
    });
    Ok(())
}

pub fn settle_result(state: &RunKernelState) -> KernelResult<AlgorithmResult> {
    if !state.lifecycle.is_terminal() {
        return Err(KernelError::new(
            KernelErrorCode::TerminalPrerequisitesUnmet,
            format!("run {} is {}", state.run_id, state.lifecycle.as_str()),
        ));
    }
    state.projection.settle()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::kernel::admission::{AdmissionCommit, RunDraft};
    use crate::optimizers::kernel::types::{AdmissionState, PRODUCER_EVENT_SCHEMA_VERSION};
    use serde_json::json;

    fn admit_gepa() -> RunKernelState {
        let mut draft = RunDraft::new("draft-g", AlgorithmKind::Gepa, "sha256:spec", "{}", "now");
        draft.transition(AdmissionState::Validating, "now").unwrap();
        draft
            .transition(AdmissionState::AwaitingApproval, "now")
            .unwrap();
        draft.transition(AdmissionState::Approved, "now").unwrap();
        let commit = AdmissionCommit::from_approved_draft(
            &draft,
            "run-g",
            ExecutionPlacement::LocalPythonProcess,
            "now",
        )
        .unwrap();
        RunKernelState::from_admission(&commit)
    }

    fn admit_eval() -> RunKernelState {
        let mut draft = RunDraft::new("draft-e", AlgorithmKind::Eval, "sha256:spec", "{}", "now");
        draft.transition(AdmissionState::Validating, "now").unwrap();
        draft
            .transition(AdmissionState::AwaitingApproval, "now")
            .unwrap();
        draft.transition(AdmissionState::Approved, "now").unwrap();
        let commit = AdmissionCommit::from_approved_draft(
            &draft,
            "run-e",
            ExecutionPlacement::DirectContainerEvaluation,
            "now",
        )
        .unwrap();
        RunKernelState::from_admission(&commit)
    }

    fn event(seq: u64, event_type: &str, payload: serde_json::Value) -> ProducerEvent {
        ProducerEvent {
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
        .with_computed_digest()
    }

    fn eval_event(seq: u64, event_type: &str, payload: serde_json::Value) -> ProducerEvent {
        let mut event = event(seq, event_type, payload);
        event.algorithm_id = "eval".into();
        event.with_computed_digest()
    }

    #[test]
    fn queued_run_starts_and_algorithm_phase_does_not_overwrite_lifecycle() {
        let state = admit_gepa();
        assert_eq!(state.lifecycle, RunLifecycle::Queued);
        let plan = commit(
            state,
            &DurableProducerLog::default(),
            &[
                event(1, "optimizer.run.started", json!({})),
                event(
                    2,
                    "candidate.registered",
                    json!({"candidate_id": "seed", "source": "seed"}),
                ),
            ],
            "now",
        )
        .unwrap();
        assert_eq!(plan.state.lifecycle, RunLifecycle::Running);
        assert_eq!(plan.state.phase, Some(RunPhase::Selection));
        assert_eq!(plan.state.aggregate_sequence, 2);
        assert_eq!(plan.events.len(), 2);
    }

    #[test]
    fn sealed_run_refuses_new_facts() {
        let state = admit_gepa();
        let first = commit(
            state,
            &DurableProducerLog::default(),
            &[
                event(1, "optimizer.run.started", json!({})),
                event(
                    2,
                    "candidate.registered",
                    json!({"candidate_id": "seed", "source": "seed"}),
                ),
                event(3, "optimizer.run.completed", json!({})),
            ],
            "now",
        )
        .unwrap();
        assert!(first.state.terminal.is_some());
        let mut log = DurableProducerLog::default();
        for event in &first.events {
            log.cursors.insert(
                event.producer.producer_id.clone(),
                event.producer.producer_sequence,
            );
            log.entries.insert(
                (
                    event.producer.producer_id.clone(),
                    event.producer.producer_sequence,
                ),
                (
                    event.producer.idempotency_key.clone(),
                    event.producer.payload_digest.clone(),
                ),
            );
        }
        let amendment = event(
            4,
            "optimizer.evidence.amended",
            json!({"terminalSequence": 3}),
        );
        let amended = commit(first.state.clone(), &log, &[amendment], "later").unwrap();
        assert_eq!(amended.state.aggregate_sequence, 4);
        assert_eq!(
            amended
                .state
                .terminal
                .as_ref()
                .expect("terminal remains sealed")
                .final_sequence,
            3,
            "an amendment advances the aggregate without rewriting termination"
        );
        let mismatched = event(
            4,
            "optimizer.evidence.amended",
            json!({"terminalSequence": 2}),
        );
        let error = commit(first.state.clone(), &log, &[mismatched], "later").unwrap_err();
        assert_eq!(error.code, KernelErrorCode::TerminalAlreadySealed);
        let extra = event(4, "candidate.registered", json!({"candidate_id": "late"}));
        let error = commit(first.state, &log, &[extra], "later").unwrap_err();
        assert_eq!(error.code, KernelErrorCode::TerminalAlreadySealed);
    }

    #[test]
    fn completed_eval_with_missing_measurements_seals_failed_evidence() {
        let plan = commit(
            admit_eval(),
            &DurableProducerLog::default(),
            &[
                eval_event(1, "optimizer.run.started", json!({})),
                eval_event(2, "eval.run.planned", json!({"plannedTrials": 1})),
                eval_event(
                    3,
                    "eval.trial.terminal",
                    json!({"workItemId": "eval:trial:0", "valid": true}),
                ),
                eval_event(4, "optimizer.run.completed", json!({})),
            ],
            "now",
        )
        .unwrap();

        let terminal = plan.state.terminal.expect("terminal must seal");
        assert_eq!(terminal.kind, TerminalKind::Failed);
        assert_eq!(terminal.reason, Some(TerminalReason::EvidenceUnusable));
        assert_eq!(terminal.final_sequence, 4);
        assert_eq!(
            terminal.evidence.completeness,
            crate::optimizers::kernel::types::EvidenceCompleteness::Partial
        );
    }

    #[test]
    fn sealing_closes_every_open_child_as_cancelled_and_replay_reproduces_it() {
        use crate::optimizers::kernel::types::WorkItemLifecycle;
        let events = vec![
            eval_event(1, "optimizer.run.started", json!({})),
            eval_event(2, "eval.run.planned", json!({"plannedTrials": 5})),
            eval_event(3, "eval.trial.started", json!({"workItemId": "eval:trial:0"})),
            eval_event(4, "eval.trial.started", json!({"workItemId": "eval:trial:1"})),
            eval_event(5, "optimizer.run.cancelled", json!({})),
        ];
        let batch = commit(admit_eval(), &DurableProducerLog::default(), &events, "now").unwrap();
        let terminal = batch.state.terminal.as_ref().expect("terminal must seal");
        assert_eq!(terminal.kind, TerminalKind::Cancelled);
        let summary = batch.state.work_summary();
        assert_eq!(summary.planned, Some(5));
        assert_eq!(summary.running, Some(0));
        assert_eq!(summary.queued, Some(0));
        assert_eq!(summary.failed, Some(0), "interrupted work never fails");
        assert_eq!(summary.cancelled, Some(5));
        assert!(batch
            .state
            .projection
            .work_items()
            .iter()
            .all(|item| item.lifecycle == WorkItemLifecycle::Terminal));

        // The restart path re-reduces the same durable events one at a time;
        // closure is a function of the terminal event, so it must reproduce.
        let mut log = DurableProducerLog::default();
        let mut restarted = admit_eval();
        for event in &events {
            let plan = commit(restarted, &log, std::slice::from_ref(event), "now").unwrap();
            log.cursors
                .insert(event.producer_id.clone(), event.producer_sequence);
            log.entries.insert(
                (event.producer_id.clone(), event.producer_sequence),
                (event.idempotency_key.clone(), event.payload_digest.clone()),
            );
            log.by_key.insert(
                event.idempotency_key.clone(),
                (
                    event.producer_id.clone(),
                    event.producer_sequence,
                    event.payload_digest.clone(),
                ),
            );
            restarted = plan.state;
        }
        assert_eq!(restarted.work_summary(), batch.state.work_summary());
        assert_eq!(
            restarted.terminal.as_ref().map(|terminal| terminal.kind),
            Some(TerminalKind::Cancelled)
        );
        assert_eq!(
            restarted.projection.work_items(),
            batch.state.projection.work_items()
        );
    }

    #[test]
    fn restart_replay_matches_uninterrupted_ingestion() {
        let events = vec![
            event(1, "optimizer.run.started", json!({})),
            event(
                2,
                "candidate.registered",
                json!({"candidate_id": "seed", "source": "seed"}),
            ),
            event(
                3,
                "candidate.registered",
                json!({"candidate_id": "child", "parent_id": "seed", "source": "proposer"}),
            ),
        ];
        let uninterrupted =
            commit(admit_gepa(), &DurableProducerLog::default(), &events, "now").unwrap();
        let mut log = DurableProducerLog::default();
        let mut restarted = admit_gepa();
        for (index, event) in events.iter().enumerate() {
            let plan = commit(restarted, &log, std::slice::from_ref(event), "now").unwrap();
            log.cursors
                .insert(event.producer_id.clone(), event.producer_sequence);
            log.entries.insert(
                (event.producer_id.clone(), event.producer_sequence),
                (event.idempotency_key.clone(), event.payload_digest.clone()),
            );
            log.by_key.insert(
                event.idempotency_key.clone(),
                (
                    event.producer_id.clone(),
                    event.producer_sequence,
                    event.payload_digest.clone(),
                ),
            );
            restarted = plan.state;
            assert_eq!(index + 1, restarted.aggregate_sequence as usize);
        }
        assert_eq!(
            restarted.projection.work_summary(),
            uninterrupted.state.projection.work_summary()
        );
        assert_eq!(restarted.lifecycle, uninterrupted.state.lifecycle);
        assert_eq!(
            restarted.aggregate_sequence,
            uninterrupted.state.aggregate_sequence
        );
    }
}
