use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::definition::{FailureDefinition, FailureDisposition, FailureId, FailureKind};
use super::lifecycle::FailureLifecycleState;
use crate::platform::operations::{OperationKind, OperationPhase, OperationContext};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FailureCause {
    Failure(FailureId),
    Detail(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OperationalFailure {
    pub failure_id: FailureId,
    pub kind: FailureKind,
    pub operation: OperationKind,
    pub phase: OperationPhase,
    pub disposition: FailureDisposition,
    pub lifecycle_state: FailureLifecycleState,
    pub context: OperationContext,
    pub safe_facts: serde_json::Value,
    pub cause: Option<FailureCause>,
    pub raised_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OperationalFailure {
    pub fn new(
        kind: FailureKind,
        context: OperationContext,
        operation: OperationKind,
        phase: OperationPhase,
        cause: Option<FailureCause>,
        now: DateTime<Utc>,
    ) -> Self {
        let disposition = kind.disposition();
        Self {
            failure_id: FailureId::generate(),
            kind,
            operation,
            phase,
            disposition,
            lifecycle_state: FailureLifecycleState::initial_for(disposition),
            context,
            safe_facts: serde_json::Value::Null,
            cause,
            raised_at: now,
            updated_at: now,
        }
    }

    pub fn with_facts(mut self, facts: serde_json::Value) -> Self {
        self.safe_facts = facts;
        self
    }

    pub fn code(&self) -> &'static str {
        self.kind.code()
    }

    pub fn message(&self) -> String {
        self.kind.message()
    }
}
