use anyhow::Result;
use rusqlite::{params, Connection};

use crate::platform::failure::{
    FailureKind, OperationalFailure, SessionFailure, TransitionReason,
};
use crate::platform::operations::{OperationContext, OperationKind, OperationPhase};
use crate::recovery::RecoveryNotice;

pub fn from_notice(notice: &RecoveryNotice) -> SessionFailure {
    if notice.reason == "lease_expired" {
        SessionFailure::LeaseExpired {
            session_id: notice.session_id.clone(),
        }
    } else {
        SessionFailure::Detached {
            session_id: notice.session_id.clone(),
            reason: notice.reason.clone(),
        }
    }
}

pub fn raise_from_notice(conn: &Connection, notice: &RecoveryNotice) -> Result<OperationalFailure> {
    let mut context = OperationContext::bootstrap(crate::instance::boot_epoch())
        .for_session(notice.session_id.clone());
    context.turn_id = notice.run_id.clone();
    let raised = crate::platform::failure::FailureRuntime::raise_in_tx(
        conn,
        FailureKind::Session(from_notice(notice)),
        context,
        OperationKind::SessionRecover,
        OperationPhase::Recover,
        None,
        "session_authority",
    )?;
    if let Some(run_id) = &notice.run_id {
        conn.execute(
            "UPDATE runs SET terminal_failure_id = ?1 WHERE id = ?2",
            params![raised.failure_id.as_str(), run_id],
        )?;
    }
    Ok(raised)
}

pub fn resolve_resume(conn: &Connection, failure_id: &str, actor: &str) -> Result<OperationalFailure> {
    crate::platform::failure::FailureAuthority::transition(
        conn,
        failure_id,
        crate::platform::failure::FailureLifecycleState::Resolved,
        TransitionReason::Resolved,
        actor,
    )
}
