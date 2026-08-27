//! Typed kernel failures. The code is the signal; prose is explanation only.
//!
//! Missing state, unknown algorithms, sequence gaps, and digest conflicts are
//! errors. They are never coerced into a default, a no-op, or a zero.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable public vocabulary for run-kernel failures. These strings appear in
/// logs, tool results, and the UI; they are API surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum KernelErrorCode {
    UnknownAlgorithm,
    AlgorithmAliasRejected,
    EventSchemaUnknown,
    EventSchemaMismatch,
    ProducerSequenceGap,
    ProducerIdempotencyConflict,
    AggregateSequenceCollision,
    LifecycleTransitionInvalid,
    WorkItemTransitionInvalid,
    WorkItemIdentityMissing,
    RunDoesNotExist,
    RunExistsBeforeAdmission,
    DraftNotApproved,
    ProjectionMissing,
    ProjectionReducerMismatch,
    TelemetryUnavailable,
    EvidenceMissing,
    TerminalAlreadySealed,
    TerminalPrerequisitesUnmet,
    DriverNotWired,
    DriverPlacementUnsupported,
    PayloadDigestMismatch,
}

impl KernelErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownAlgorithm => "unknown_algorithm",
            Self::AlgorithmAliasRejected => "algorithm_alias_rejected",
            Self::EventSchemaUnknown => "event_schema_unknown",
            Self::EventSchemaMismatch => "event_schema_mismatch",
            Self::ProducerSequenceGap => "producer_sequence_gap",
            Self::ProducerIdempotencyConflict => "producer_idempotency_conflict",
            Self::AggregateSequenceCollision => "aggregate_sequence_collision",
            Self::LifecycleTransitionInvalid => "lifecycle_transition_invalid",
            Self::WorkItemTransitionInvalid => "work_item_transition_invalid",
            Self::WorkItemIdentityMissing => "work_item_identity_missing",
            Self::RunDoesNotExist => "run_does_not_exist",
            Self::RunExistsBeforeAdmission => "run_exists_before_admission",
            Self::DraftNotApproved => "draft_not_approved",
            Self::ProjectionMissing => "projection_missing",
            Self::ProjectionReducerMismatch => "projection_reducer_mismatch",
            Self::TelemetryUnavailable => "telemetry_unavailable",
            Self::EvidenceMissing => "evidence_missing",
            Self::TerminalAlreadySealed => "terminal_already_sealed",
            Self::TerminalPrerequisitesUnmet => "terminal_prerequisites_unmet",
            Self::DriverNotWired => "driver_not_wired",
            Self::DriverPlacementUnsupported => "driver_placement_unsupported",
            Self::PayloadDigestMismatch => "payload_digest_mismatch",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KernelError {
    pub code: KernelErrorCode,
    pub message: String,
}

impl KernelError {
    pub fn new(code: KernelErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for KernelError {}

pub type KernelResult<T> = Result<T, KernelError>;
