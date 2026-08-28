//! The typed settlement command: one entry for ending a run.
//!
//! `SettleCause` names how a run ends. The service's `settle_run` appends the
//! matching terminal event through the same transactional append path every
//! other event uses, so locking, idempotency, the sealed-run refusal, manifest
//! sealing, and projection persistence all apply unchanged. A second
//! settlement with the same terminal kind is idempotent; a different kind is
//! a typed refusal, never a silent overwrite.

use std::sync::Arc;

use super::error::{KernelError, KernelErrorCode, KernelResult};
use super::types::{CancellationRequest, TerminalKind};

#[derive(Clone, Debug)]
pub enum SettleCause {
    Completed,
    Failed { detail: String },
    Degraded { detail: String },
    Cancelled { request: Arc<CancellationRequest> },
}

impl SettleCause {
    pub const fn kind(&self) -> TerminalKind {
        match self {
            Self::Completed => TerminalKind::Completed,
            Self::Failed { .. } => TerminalKind::Failed,
            Self::Degraded { .. } => TerminalKind::Degraded,
            Self::Cancelled { .. } => TerminalKind::Cancelled,
        }
    }

    pub const fn event_type(&self) -> &'static str {
        match self {
            Self::Completed => "optimizer.run.completed",
            Self::Failed { .. } => "optimizer.run.failed",
            Self::Degraded { .. } => "optimizer.run.degraded",
            Self::Cancelled { .. } => "optimizer.run.cancelled",
        }
    }

    pub const fn status(&self) -> &'static str {
        self.kind().as_str()
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Failed { detail } | Self::Degraded { detail } => Some(detail.as_str()),
            _ => None,
        }
    }

    pub fn cancellation(&self) -> Option<&Arc<CancellationRequest>> {
        match self {
            Self::Cancelled { request } => Some(request),
            _ => None,
        }
    }

    /// Judge this cause against an already-sealed terminal. The same kind is
    /// a confirmed replay; a different kind is refused — a sealed record can
    /// never be re-decided by a later, different opinion.
    pub fn accept_sealed(&self, sealed: TerminalKind, run_id: &str) -> KernelResult<()> {
        if sealed == self.kind() {
            return Ok(());
        }
        Err(KernelError::new(
            KernelErrorCode::TerminalAlreadySealed,
            format!(
                "run {run_id} is sealed {}; refusing a second settlement as {}",
                sealed.as_str(),
                self.kind().as_str()
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::kernel::types::CancellationCause;

    #[test]
    fn a_second_compatible_settlement_is_idempotent_and_a_conflict_is_typed() {
        let cancelled = SettleCause::Cancelled {
            request: Arc::new(CancellationRequest::new(
                CancellationCause::UserRequested,
                "user:test",
                "run:r1",
            )),
        };
        cancelled.accept_sealed(TerminalKind::Cancelled, "r1").unwrap();
        let error = cancelled
            .accept_sealed(TerminalKind::Failed, "r1")
            .unwrap_err();
        assert_eq!(error.code, KernelErrorCode::TerminalAlreadySealed);

        let failed = SettleCause::Failed {
            detail: "worker died".into(),
        };
        assert_eq!(failed.event_type(), "optimizer.run.failed");
        failed.accept_sealed(TerminalKind::Failed, "r1").unwrap();
    }
}
