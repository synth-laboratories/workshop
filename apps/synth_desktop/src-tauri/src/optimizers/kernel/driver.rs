//! Execution drivers. Placement is orthogonal to algorithm identity.
//!
//! The central service starts, polls, cancels, and recovers through this
//! registry rather than per-algorithm reconciliation branches.

use super::error::{KernelError, KernelErrorCode, KernelResult};
use super::types::{AlgorithmKind, ExecutionPlacement};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedRunSpec {
    pub run_id: String,
    pub algorithm: AlgorithmKind,
    pub placement: ExecutionPlacement,
    pub spec_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalRunRef {
    pub placement: ExecutionPlacement,
    pub external_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverKind {
    LocalPythonProcess,
    DirectContainerEvaluation,
    LocalTrainingSidecar,
    HostedOptimizersService,
    RemoteTrainingService,
}

impl DriverKind {
    pub const fn placement(self) -> ExecutionPlacement {
        match self {
            Self::LocalPythonProcess => ExecutionPlacement::LocalPythonProcess,
            Self::DirectContainerEvaluation => ExecutionPlacement::DirectContainerEvaluation,
            Self::LocalTrainingSidecar => ExecutionPlacement::LocalTrainingSidecar,
            Self::HostedOptimizersService => ExecutionPlacement::HostedOptimizersService,
            Self::RemoteTrainingService => ExecutionPlacement::RemoteTrainingService,
        }
    }

    pub const fn as_str(self) -> &'static str {
        self.placement().as_str()
    }
}

/// Resolve the driver for an algorithm + placement. Unknown combinations fail
/// closed; they are not coerced onto a nearby driver.
pub fn resolve(
    algorithm: AlgorithmKind,
    placement: ExecutionPlacement,
) -> KernelResult<DriverKind> {
    let allowed = match algorithm {
        AlgorithmKind::Eval => &[
            ExecutionPlacement::LocalPythonProcess,
            ExecutionPlacement::DirectContainerEvaluation,
        ][..],
        AlgorithmKind::Gepa => &[
            ExecutionPlacement::LocalPythonProcess,
            ExecutionPlacement::HostedOptimizersService,
        ][..],
        AlgorithmKind::GoEx => &[ExecutionPlacement::HostedOptimizersService][..],
        AlgorithmKind::Sft => &[
            ExecutionPlacement::LocalTrainingSidecar,
            ExecutionPlacement::RemoteTrainingService,
        ][..],
        AlgorithmKind::Cispo => &[
            ExecutionPlacement::LocalTrainingSidecar,
            ExecutionPlacement::RemoteTrainingService,
        ][..],
    };
    if !allowed.contains(&placement) {
        return Err(KernelError::new(
            KernelErrorCode::DriverPlacementUnsupported,
            format!(
                "{} cannot run at placement {}",
                algorithm.wire_id(),
                placement.as_str()
            ),
        ));
    }
    Ok(match placement {
        ExecutionPlacement::LocalPythonProcess => DriverKind::LocalPythonProcess,
        ExecutionPlacement::DirectContainerEvaluation => DriverKind::DirectContainerEvaluation,
        ExecutionPlacement::LocalTrainingSidecar => DriverKind::LocalTrainingSidecar,
        ExecutionPlacement::HostedOptimizersService => DriverKind::HostedOptimizersService,
        ExecutionPlacement::RemoteTrainingService => DriverKind::RemoteTrainingService,
    })
}

