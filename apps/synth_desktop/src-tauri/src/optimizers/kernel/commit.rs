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

