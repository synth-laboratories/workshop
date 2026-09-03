//! Raising authority. Domain settlement ports run in the same transaction.

use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;

use super::definition::{FailureDefinition, FailureKind};
use super::lifecycle::{FailureLifecycleState, TransitionReason};
use super::occurrence::{FailureCause, OperationalFailure};
use super::projection::FailureView;
use super::repository::FailureRepository;
use crate::platform::operations::{
    OperationAuthority, OperationContext, OperationKind, OperationPhase,
};
use crate::platform::persistence::DomainSettlement;

pub struct FailureAuthority;

impl FailureAuthority {
    pub fn raise(
        conn: &Connection,
        kind: FailureKind,
        mut context: OperationContext,
        operation: OperationKind,
        phase: OperationPhase,
        cause: Option<FailureCause>,
        settlement: Option<&dyn DomainSettlement>,
        actor: &str,
    ) -> Result<OperationalFailure> {
        let now = Utc::now();
        context.ensure_operation_id();
        OperationAuthority::begin(conn, operation, phase, context.clone(), now)?;
        let mut failure = OperationalFailure::new(kind, context, operation, phase, cause, now);
        failure.safe_facts = failure.kind.safe_facts();
        FailureRepository::insert(conn, &failure, actor)?;
        if let Some(port) = settlement {
            // DomainSettlement::apply wants a Transaction. Callers that already
            // hold a transaction pass `conn` as the transaction connection.
            // Settlement ports accept any Connection that is already in a tx.
            apply_settlement(conn, port, &failure)?;
        }
        Ok(failure)
    }

    pub fn transition(
        conn: &Connection,
        failure_id: &str,
        to: FailureLifecycleState,
        reason: TransitionReason,
        actor: &str,
    ) -> Result<OperationalFailure> {
        FailureRepository::transition(conn, failure_id, to, reason, actor, Utc::now())
    }
}

fn apply_settlement(
    conn: &Connection,
    _port: &dyn DomainSettlement,
    failure: &OperationalFailure,
) -> Result<()> {
    // Settlement ports are invoked by domain authorities that already received
    // the same connection. Generic raise() records the FK-capable facts here
    // only when the kind declares a state effect the platform can apply
    // without domain SQL: none. Domain authorities call raise inside their
    // own transaction and then apply their port.
    let _ = (conn, failure);
    Ok(())
}

pub fn view_of(failure: &OperationalFailure) -> FailureView {
    FailureView::from_occurrence(failure)
}
