//! Failure lifecycle machine. Illegal transitions are rejected, not coerced.
//!
//! See `notes/specifications/workshop/failure_runtime.md`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureLifecycleState {
    Open,
    AwaitingApproval,
    Repairing,
    RetryScheduled,
    Retrying,
    Resolved,
    Terminalized,
    Superseded,
}

impl FailureLifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Repairing => "repairing",
            Self::RetryScheduled => "retry_scheduled",
            Self::Retrying => "retrying",
            Self::Resolved => "resolved",
            Self::Terminalized => "terminalized",
            Self::Superseded => "superseded",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "open" => Ok(Self::Open),
            "awaiting_approval" => Ok(Self::AwaitingApproval),
            "repairing" => Ok(Self::Repairing),
            "retry_scheduled" => Ok(Self::RetryScheduled),
            "retrying" => Ok(Self::Retrying),
            "resolved" => Ok(Self::Resolved),
            "terminalized" => Ok(Self::Terminalized),
            "superseded" => Ok(Self::Superseded),
            other => Err(format!("unknown failure lifecycle state `{other}`")),
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Resolved | Self::Terminalized | Self::Superseded)
    }

    pub fn initial_for(disposition: super::definition::FailureDisposition) -> Self {
        use super::definition::FailureDisposition::*;
        match disposition {
            ApprovalRequired => Self::AwaitingApproval,
            RepairRequired => Self::Open,
            Retryable => Self::RetryScheduled,
            Terminal => Self::Terminalized,
            Cancelled => Self::Terminalized,
            ProgrammerError => Self::Open,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionReason {
    Raised,
    ApprovalRequested,
    ApprovalGranted,
    ApprovalDenied,
    RepairStarted,
    RetryScheduled,
    RetryStarted,
    Resolved,
    Terminalized,
    Superseded,
}

impl TransitionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Raised => "raised",
            Self::ApprovalRequested => "approval_requested",
            Self::ApprovalGranted => "approval_granted",
            Self::ApprovalDenied => "approval_denied",
            Self::RepairStarted => "repair_started",
            Self::RetryScheduled => "retry_scheduled",
            Self::RetryStarted => "retry_started",
            Self::Resolved => "resolved",
            Self::Terminalized => "terminalized",
            Self::Superseded => "superseded",
        }
    }
}

pub fn allowed(
    from: FailureLifecycleState,
    to: FailureLifecycleState,
) -> Result<(), String> {
    use FailureLifecycleState::*;
    let ok = match (from, to) {
        (Open, AwaitingApproval | Repairing | RetryScheduled | Resolved | Terminalized | Superseded) => true,
        (AwaitingApproval, Repairing | Retrying | Terminalized | Open | Resolved) => true,
        (Repairing, Resolved | Terminalized | Open | AwaitingApproval) => true,
        (RetryScheduled, Retrying | Terminalized | Superseded | Open) => true,
        (Retrying, Resolved | Terminalized | Open | RetryScheduled) => true,
        (Resolved | Terminalized | Superseded, _) => false,
        (from, to) if from == to => true,
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "illegal failure lifecycle transition {} → {}",
            from.as_str(),
            to.as_str()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_states_never_return_to_live() {
        for from in [
            FailureLifecycleState::Resolved,
            FailureLifecycleState::Terminalized,
            FailureLifecycleState::Superseded,
        ] {
            for to in [
                FailureLifecycleState::Open,
                FailureLifecycleState::Repairing,
                FailureLifecycleState::Retrying,
                FailureLifecycleState::AwaitingApproval,
            ] {
                assert!(allowed(from, to).is_err(), "{from:?} → {to:?}");
            }
        }
    }

    #[test]
    fn approval_required_starts_awaiting_approval() {
        assert_eq!(
            FailureLifecycleState::initial_for(
                super::super::definition::FailureDisposition::ApprovalRequired
            ),
            FailureLifecycleState::AwaitingApproval
        );
    }
}
