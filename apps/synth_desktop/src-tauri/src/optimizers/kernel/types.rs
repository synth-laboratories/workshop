//! Canonical enums and identities for the Optimizers run kernel.
//!
//! Algorithm identity and execution placement are orthogonal. Local and hosted
//! SFT share one SFT contract; local and hosted CISPO share one CISPO contract.
//! GO-EX is the canonical algorithm id; GELO is its display label and recipe
//! name. Wire aliases are rejected, never silently folded.

use serde::{Deserialize, Serialize};

use super::error::{KernelError, KernelErrorCode, KernelResult};

pub const KERNEL_SCHEMA_VERSION: &str = "optimizer_run_kernel.v1";
pub const PRODUCER_EVENT_SCHEMA_VERSION: &str = "optimizer_producer_event.v1";
pub const RUN_VIEW_SCHEMA_VERSION: &str = "optimizer_run_view.v2";
pub const GELO_HOSTED_RECIPE_ID: &str = "gelo.craftax.hosted.v1";

/// Closed set of algorithms the kernel will reduce. External producers send
/// versioned wire strings that must decode to one of these before they can
/// mutate state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum AlgorithmKind {
    Eval,
    Gepa,
    /// Canonical algorithm id `go-ex`. Display label is GELO. Recipe id is
    /// [`GELO_HOSTED_RECIPE_ID`]. `gelo` and `hosted_gelo` are not this value.
    GoEx,
    Sft,
    Cispo,
}

impl AlgorithmKind {
    pub const ALL: &'static [Self] = &[Self::Eval, Self::Gepa, Self::GoEx, Self::Sft, Self::Cispo];

    pub const fn wire_id(self) -> &'static str {
        match self {
            Self::Eval => "eval",
            Self::Gepa => "gepa",
            Self::GoEx => "go-ex",
            Self::Sft => "sft",
            Self::Cispo => "cispo",
        }
    }

    pub const fn display_label(self) -> &'static str {
        match self {
            Self::Eval => "Eval",
            Self::Gepa => "GEPA",
            Self::GoEx => "GELO",
            Self::Sft => "SFT",
            Self::Cispo => "CISPO",
        }
    }

    pub const fn reducer_version(self) -> &'static str {
        match self {
            Self::Eval => "eval.projection.v1",
            Self::Gepa => "gepa.projection.v1",
            Self::GoEx => "go_ex.projection.v1",
            Self::Sft => "sft.projection.v1",
            Self::Cispo => "cispo.projection.v1",
        }
    }

    /// Stable typed-result schema owned by the algorithm registry.
    pub const fn result_schema(self) -> &'static str {
        match self {
            Self::Eval => "eval_run_result.v1",
            Self::Gepa => "gepa_run_result.v1",
            Self::GoEx => "go_ex_run_result.v1",
            Self::Sft => "sft_run_result.v1",
            Self::Cispo => "cispo_run_result.v1",
        }
    }

    /// Decode a producer wire string. Exact canonical spelling only.
    ///
    /// `gelo`, `hosted_gelo`, `go_ex`, and case variants are rejected with
    /// [`KernelErrorCode::AlgorithmAliasRejected`] so a producer cannot acquire
    /// GO-EX authority by sharing a nearby name.
    pub fn parse_wire(value: &str) -> KernelResult<Self> {
        match value {
            "eval" => Ok(Self::Eval),
            "gepa" => Ok(Self::Gepa),
            "go-ex" => Ok(Self::GoEx),
            "sft" => Ok(Self::Sft),
            "cispo" => Ok(Self::Cispo),
            "gelo" | "hosted_gelo" | "go_ex" | "goex" | "GO-EX" | "GELO" => Err(KernelError::new(
                KernelErrorCode::AlgorithmAliasRejected,
                format!(
                    "{value:?} is not a canonical algorithm id; GO-EX is wire `go-ex`, \
                         display GELO, recipe `{GELO_HOSTED_RECIPE_ID}`"
                ),
            )),
            other => Err(KernelError::new(
                KernelErrorCode::UnknownAlgorithm,
                format!("{other:?} is not a registered optimizer algorithm"),
            )),
        }
    }
}

/// Where a run executes. Orthogonal to [`AlgorithmKind`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPlacement {
    LocalPythonProcess,
    DirectContainerEvaluation,
    LocalTrainingSidecar,
    HostedOptimizersService,
    RemoteTrainingService,
}

impl ExecutionPlacement {
    pub const ALL: &'static [Self] = &[
        Self::LocalPythonProcess,
        Self::DirectContainerEvaluation,
        Self::LocalTrainingSidecar,
        Self::HostedOptimizersService,
        Self::RemoteTrainingService,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalPythonProcess => "local_python_process",
            Self::DirectContainerEvaluation => "direct_container_evaluation",
            Self::LocalTrainingSidecar => "local_training_sidecar",
            Self::HostedOptimizersService => "hosted_optimizers_service",
            Self::RemoteTrainingService => "remote_training_service",
        }
    }

    pub fn parse(value: &str) -> KernelResult<Self> {
        match value {
            "local_python_process" => Ok(Self::LocalPythonProcess),
            "direct_container_evaluation" => Ok(Self::DirectContainerEvaluation),
            "local_training_sidecar" => Ok(Self::LocalTrainingSidecar),
            "hosted_optimizers_service" => Ok(Self::HostedOptimizersService),
            "remote_training_service" => Ok(Self::RemoteTrainingService),
            other => Err(KernelError::new(
                KernelErrorCode::DriverPlacementUnsupported,
                format!("{other:?} is not an execution placement"),
            )),
        }
    }
}

/// Common execution lifecycle. Algorithm phase and execution health are not
/// peers of these variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RunLifecycle {
    Queued,
    Starting,
    Running,
    Paused,
    Cancelling,
    Terminal,
}

impl RunLifecycle {
    pub const ALL: &'static [Self] = &[
        Self::Queued,
        Self::Starting,
        Self::Running,
        Self::Paused,
        Self::Cancelling,
        Self::Terminal,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Cancelling => "cancelling",
            Self::Terminal => "terminal",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Terminal)
    }

    pub fn parse(value: &str) -> KernelResult<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "starting" => Ok(Self::Starting),
            "running" => Ok(Self::Running),
            "paused" => Ok(Self::Paused),
            "cancelling" => Ok(Self::Cancelling),
            "terminal" => Ok(Self::Terminal),
            other => Err(KernelError::new(
                KernelErrorCode::EventSchemaMismatch,
                format!("{other:?} is not a run lifecycle"),
            )),
        }
    }

    fn permitted_successors(self) -> &'static [Self] {
        match self {
            Self::Queued => &[
                Self::Starting,
                Self::Running,
                Self::Cancelling,
                Self::Terminal,
            ],
            Self::Starting => &[Self::Running, Self::Cancelling, Self::Terminal],
            Self::Running => &[Self::Paused, Self::Cancelling, Self::Terminal],
            Self::Paused => &[Self::Running, Self::Cancelling, Self::Terminal],
            Self::Cancelling => &[Self::Terminal],
            Self::Terminal => &[],
        }
    }

    pub fn may_transition_to(self, next: Self) -> bool {
        self == next || self.permitted_successors().contains(&next)
    }

    pub fn transition_to(self, next: Self) -> KernelResult<Self> {
        if self.may_transition_to(next) {
            return Ok(next);
        }
        Err(KernelError::new(
            KernelErrorCode::LifecycleTransitionInvalid,
            format!(
                "lifecycle `{}` may not transition to `{}` (permitted: {})",
                self.as_str(),
                next.as_str(),
                self.permitted_successors()
                    .iter()
                    .map(|state| state.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ))
    }
}

/// Structured terminal outcome. These are not lifecycle peers of `running`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TerminalKind {
    Completed,
    Failed,
    Cancelled,
    Degraded,
}

impl TerminalKind {
    pub const ALL: &'static [Self] = &[
        Self::Completed,
        Self::Failed,
        Self::Cancelled,
        Self::Degraded,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Degraded => "degraded",
        }
    }
}

/// Typed optional reason attached to a terminal outcome. These used to be
/// extra `OptimizerRunStatus` variants, which let a phase overwrite lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TerminalReason {
    EvidenceUnusable,
    Interrupted,
    InfrastructureLost,
    CapReached,
    ProducerFailed,
    OperatorCancelled,
    AdmissionRejected,
}

impl TerminalReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EvidenceUnusable => "evidence_unusable",
            Self::Interrupted => "interrupted",
            Self::InfrastructureLost => "infrastructure_lost",
            Self::CapReached => "cap_reached",
            Self::ProducerFailed => "producer_failed",
            Self::OperatorCancelled => "operator_cancelled",
            Self::AdmissionRejected => "admission_rejected",
        }
    }
}

/// Algorithm-owned phase while lifecycle is `running` (or `starting`). Not a
/// lifecycle peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RunPhase {
    Validating,
    Provisioning,
    WaitingForViewer,
    Training,
    Selection,
    CheckpointEvaluation,
    HeldoutEvaluation,
    Materializing,
}

impl RunPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Validating => "validating",
            Self::Provisioning => "provisioning",
            Self::WaitingForViewer => "waiting_for_viewer",
            Self::Training => "training",
            Self::Selection => "selection",
            Self::CheckpointEvaluation => "checkpoint_evaluation",
            Self::HeldoutEvaluation => "heldout_evaluation",
            Self::Materializing => "materializing",
        }
    }

    pub fn parse(value: &str) -> KernelResult<Self> {
        match value {
            "validating" => Ok(Self::Validating),
            "provisioning" => Ok(Self::Provisioning),
            "waiting_for_viewer" => Ok(Self::WaitingForViewer),
            "training" => Ok(Self::Training),
            "selection" => Ok(Self::Selection),
            "checkpoint_evaluation" => Ok(Self::CheckpointEvaluation),
            "heldout_evaluation" => Ok(Self::HeldoutEvaluation),
            "materializing" => Ok(Self::Materializing),
            other => Err(KernelError::new(
                KernelErrorCode::EventSchemaMismatch,
                format!("{other:?} is not a run phase"),
            )),
        }
    }
}

/// Execution health, stored beside lifecycle rather than as a status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RunCondition {
    Healthy,
    EnvironmentUnreachable,
    WaitingForProducer,
    ProducerSequenceBlocked,
}

impl RunCondition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::EnvironmentUnreachable => "environment_unreachable",
            Self::WaitingForProducer => "waiting_for_producer",
            Self::ProducerSequenceBlocked => "producer_sequence_blocked",
        }
    }

    pub fn parse(value: &str) -> KernelResult<Self> {
        match value {
            "healthy" => Ok(Self::Healthy),
            "environment_unreachable" => Ok(Self::EnvironmentUnreachable),
            "waiting_for_producer" => Ok(Self::WaitingForProducer),
            "producer_sequence_blocked" => Ok(Self::ProducerSequenceBlocked),
            other => Err(KernelError::new(
                KernelErrorCode::EventSchemaMismatch,
                format!("{other:?} is not a run condition"),
            )),
        }
    }
}

/// Common work-item lifecycle. External task/rollout/job ids are references.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemLifecycle {
    Planned,
    Queued,
    Starting,
    Running,
    Terminal,
}

impl WorkItemLifecycle {
    pub const ALL: &'static [Self] = &[
        Self::Planned,
        Self::Queued,
        Self::Starting,
        Self::Running,
        Self::Terminal,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Terminal => "terminal",
        }
    }

    fn permitted_successors(self) -> &'static [Self] {
        match self {
            Self::Planned => &[Self::Queued, Self::Terminal],
            Self::Queued => &[Self::Starting, Self::Terminal],
            Self::Starting => &[Self::Running, Self::Terminal],
            Self::Running => &[Self::Terminal],
            Self::Terminal => &[],
        }
    }

    pub fn may_transition_to(self, next: Self) -> bool {
        self == next || self.permitted_successors().contains(&next)
    }

    pub fn transition_to(self, next: Self) -> KernelResult<Self> {
        if self.may_transition_to(next) {
            return Ok(next);
        }
        Err(KernelError::new(
            KernelErrorCode::WorkItemTransitionInvalid,
            format!(
                "work item `{}` may not transition to `{}`",
                self.as_str(),
                next.as_str()
            ),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemKind {
    EvalTrial,
    ContainerRollout,
    ProposerJob,
    CandidateEvaluation,
    TrainingStep,
    CheckpointEvaluation,
    HeldoutEvaluation,
}

impl WorkItemKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EvalTrial => "eval_trial",
            Self::ContainerRollout => "container_rollout",
            Self::ProposerJob => "proposer_job",
            Self::CandidateEvaluation => "candidate_evaluation",
            Self::TrainingStep => "training_step",
            Self::CheckpointEvaluation => "checkpoint_evaluation",
            Self::HeldoutEvaluation => "heldout_evaluation",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceCompleteness {
    Absent,
    Partial,
    Complete,
    Unusable,
}

impl EvidenceCompleteness {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Partial => "partial",
            Self::Complete => "complete",
            Self::Unusable => "unusable",
        }
    }
}

/// Pre-run admission. A draft is not an optimizer run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionState {
    Draft,
    Validating,
    AwaitingApproval,
    Approved,
    NotRequired,
    Rejected,
    Expired,
    Consumed,
}

impl AdmissionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Validating => "validating",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Approved => "approved",
            Self::NotRequired => "not_required",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
            Self::Consumed => "consumed",
        }
    }

    pub fn parse(value: &str) -> KernelResult<Self> {
        match value {
            "draft" => Ok(Self::Draft),
            "validating" => Ok(Self::Validating),
            "awaiting_approval" => Ok(Self::AwaitingApproval),
            "approved" => Ok(Self::Approved),
            "not_required" => Ok(Self::NotRequired),
            "rejected" => Ok(Self::Rejected),
            "expired" => Ok(Self::Expired),
            "consumed" => Ok(Self::Consumed),
            other => Err(KernelError::new(
                KernelErrorCode::EventSchemaMismatch,
                format!("{other:?} is not an admission state"),
            )),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Rejected | Self::Expired | Self::Consumed)
    }

    fn permitted_successors(self) -> &'static [Self] {
        match self {
            Self::Draft => &[
                Self::Validating,
                Self::NotRequired,
                Self::Rejected,
                Self::Expired,
            ],
            Self::Validating => &[
                Self::AwaitingApproval,
                Self::Approved,
                Self::Rejected,
                Self::Expired,
            ],
            Self::AwaitingApproval => &[Self::Approved, Self::Rejected, Self::Expired],
            Self::Approved | Self::NotRequired => &[Self::Consumed, Self::Expired],
            Self::Rejected | Self::Expired | Self::Consumed => &[],
        }
    }

    pub fn may_transition_to(self, next: Self) -> bool {
        self == next || self.permitted_successors().contains(&next)
    }

    pub fn transition_to(self, next: Self) -> KernelResult<Self> {
        if self.may_transition_to(next) {
            return Ok(next);
        }
        Err(KernelError::new(
            KernelErrorCode::DraftNotApproved,
            format!(
                "admission `{}` may not transition to `{}`",
                self.as_str(),
                next.as_str()
            ),
        ))
    }
}

/// Map a legacy stored `optimizer_runs.status` onto kernel lifecycle + phase +
/// condition. This is a read adapter for migration, not a second authority.
pub fn classify_legacy_status(
    status: &str,
) -> Option<(
    RunLifecycle,
    Option<RunPhase>,
    RunCondition,
    Option<(TerminalKind, Option<TerminalReason>)>,
)> {
    use crate::optimizers::OptimizerRunStatus;
    let status = OptimizerRunStatus::parse(status)?;
    Some(match status {
        OptimizerRunStatus::Queued => (RunLifecycle::Queued, None, RunCondition::Healthy, None),
        OptimizerRunStatus::Validating => (
            RunLifecycle::Starting,
            Some(RunPhase::Validating),
            RunCondition::Healthy,
            None,
        ),
        OptimizerRunStatus::Provisioning => (
            RunLifecycle::Starting,
            Some(RunPhase::Provisioning),
            RunCondition::Healthy,
            None,
        ),
        OptimizerRunStatus::Starting => (RunLifecycle::Starting, None, RunCondition::Healthy, None),
        OptimizerRunStatus::WaitingForViewer => (
            RunLifecycle::Running,
            Some(RunPhase::WaitingForViewer),
            RunCondition::Healthy,
            None,
        ),
        OptimizerRunStatus::Running => (RunLifecycle::Running, None, RunCondition::Healthy, None),
        OptimizerRunStatus::Paused => (RunLifecycle::Paused, None, RunCondition::Healthy, None),
        OptimizerRunStatus::Cancelling => {
            (RunLifecycle::Cancelling, None, RunCondition::Healthy, None)
        }
        OptimizerRunStatus::EnvUnreachable => (
            RunLifecycle::Running,
            None,
            RunCondition::EnvironmentUnreachable,
            None,
        ),
        OptimizerRunStatus::Completed => (
            RunLifecycle::Terminal,
            None,
            RunCondition::Healthy,
            Some((TerminalKind::Completed, None)),
        ),
        OptimizerRunStatus::Failed => (
            RunLifecycle::Terminal,
            None,
            RunCondition::Healthy,
            Some((TerminalKind::Failed, Some(TerminalReason::ProducerFailed))),
        ),
        OptimizerRunStatus::FailedEvidence => (
            RunLifecycle::Terminal,
            None,
            RunCondition::Healthy,
            Some((TerminalKind::Failed, Some(TerminalReason::EvidenceUnusable))),
        ),
        OptimizerRunStatus::Cancelled => (
            RunLifecycle::Terminal,
            None,
            RunCondition::Healthy,
            Some((
                TerminalKind::Cancelled,
                Some(TerminalReason::OperatorCancelled),
            )),
        ),
        OptimizerRunStatus::Degraded => (
            RunLifecycle::Terminal,
            None,
            RunCondition::Healthy,
            Some((TerminalKind::Degraded, None)),
        ),
        OptimizerRunStatus::Interrupted => (
            RunLifecycle::Terminal,
            None,
            RunCondition::Healthy,
            Some((TerminalKind::Failed, Some(TerminalReason::Interrupted))),
        ),
        OptimizerRunStatus::InfrastructureLost => (
            RunLifecycle::Terminal,
            None,
            RunCondition::Healthy,
            Some((
                TerminalKind::Failed,
                Some(TerminalReason::InfrastructureLost),
            )),
        ),
        OptimizerRunStatus::CapReached => (
            RunLifecycle::Terminal,
            None,
            RunCondition::Healthy,
            Some((TerminalKind::Degraded, Some(TerminalReason::CapReached))),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_ex_is_canonical_and_gelo_aliases_are_rejected() {
        assert_eq!(
            AlgorithmKind::parse_wire("go-ex").unwrap(),
            AlgorithmKind::GoEx
        );
        assert_eq!(AlgorithmKind::GoEx.display_label(), "GELO");
        assert_eq!(AlgorithmKind::GoEx.wire_id(), "go-ex");
        for alias in ["gelo", "hosted_gelo", "go_ex", "goex", "GO-EX", "GELO"] {
            let error = AlgorithmKind::parse_wire(alias).unwrap_err();
            assert_eq!(
                error.code,
                KernelErrorCode::AlgorithmAliasRejected,
                "{alias}"
            );
        }
        let unknown = AlgorithmKind::parse_wire("ppo").unwrap_err();
        assert_eq!(unknown.code, KernelErrorCode::UnknownAlgorithm);
    }

    #[test]
    fn every_legacy_status_classifies_without_becoming_a_lifecycle_peer() {
        use crate::optimizers::OptimizerRunStatus;
        for status in OptimizerRunStatus::ALL {
            let (lifecycle, phase, condition, terminal) =
                classify_legacy_status(status.as_str()).expect(status.as_str());
            match status {
                OptimizerRunStatus::Validating
                | OptimizerRunStatus::Provisioning
                | OptimizerRunStatus::WaitingForViewer
                | OptimizerRunStatus::EnvUnreachable => {
                    assert_ne!(lifecycle, RunLifecycle::Terminal, "{}", status.as_str());
                    assert!(
                        phase.is_some() || condition != RunCondition::Healthy,
                        "{}",
                        status.as_str()
                    );
                    assert!(terminal.is_none(), "{}", status.as_str());
                }
                _ if status.is_terminal() => {
                    assert_eq!(lifecycle, RunLifecycle::Terminal);
                    assert!(terminal.is_some());
                }
                _ => {}
            }
        }
    }
}
