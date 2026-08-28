//! FailureRuntime: identity, lifecycle, query, redaction, delivery.
//!
//! See `notes/specifications/workshop/failure_runtime.md`.

mod authority;
pub mod definition;
pub mod lifecycle;
pub mod occurrence;
pub mod projection;
pub mod query;
pub mod recovery;
pub mod redaction;
pub mod relationship;
pub mod remediation;
pub mod repository;


use anyhow::Result;
use rusqlite::Connection;
use std::sync::Arc;

use crate::platform::operations::{OperationContext, OperationKind, OperationPhase};
use crate::storage::Database;

pub use authority::{view_of, FailureAuthority};
pub use definition::{
    AdmissionFailure, ApprovalFailure, AuthenticationFailure, ContainerFailure, ContractFailure,
    EvaluationFailure, FailureCategory, FailureDefinition, FailureDisposition, FailureId,
    FailureKind, FailureStateEffect, HealthObservation, HealthSource, HealthStatus,
    PersistenceFailure, ProviderFailure, SessionFailure, TelemetryFailure, VisualFailure,
    CODE_CONTRACT_INVALID, CODE_HISTORICAL_UNCLASSIFIED, FAILURE_SCHEMA_VERSION,
};
pub use lifecycle::{FailureLifecycleState, TransitionReason};
pub use occurrence::{FailureCause, OperationalFailure};
pub use projection::{parse_view, FailureContextView, FailureView};
pub use query::{FailureQuery, FailureQueryResult};
pub use recovery::{RecoveryAction, RecoveryPlan, RecoveryReceipt};
pub use remediation::{FailureRemediation, FailureRemediationView};

#[derive(Clone)]
pub struct FailureRuntime {
    db: Arc<Database>,
}

impl FailureRuntime {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    pub fn raise(
        &self,
        kind: FailureKind,
        context: OperationContext,
        operation: OperationKind,
        phase: OperationPhase,
        cause: Option<FailureCause>,
        actor: &str,
    ) -> Result<OperationalFailure> {
        self.db.transaction(|conn| {
            FailureAuthority::raise(conn, kind, context, operation, phase, cause, None, actor)
        })
    }

    pub fn raise_in_tx(
        conn: &Connection,
        kind: FailureKind,
        context: OperationContext,
        operation: OperationKind,
        phase: OperationPhase,
        cause: Option<FailureCause>,
        actor: &str,
    ) -> Result<OperationalFailure> {
        FailureAuthority::raise(conn, kind, context, operation, phase, cause, None, actor)
    }

    pub fn transition(
        &self,
        failure_id: &str,
        to: FailureLifecycleState,
        reason: TransitionReason,
        actor: &str,
    ) -> Result<OperationalFailure> {
        self.db
            .transaction(|conn| FailureAuthority::transition(conn, failure_id, to, reason, actor))
    }

    pub fn get(&self, failure_id: &str) -> Result<Option<FailureView>> {
        self.db.with_conn(|conn| query::get_view(conn, failure_id))
    }

    pub fn query(&self, query: FailureQuery) -> Result<FailureQueryResult> {
        self.db.with_conn(|conn| query.execute(conn))
    }

    pub fn timeline(&self, failure_id: &str) -> Result<Vec<serde_json::Value>> {
        self.db.with_conn(|conn| query::timeline(conn, failure_id))
    }

    pub fn export_bundle(&self, failure_id: &str) -> Result<serde_json::Value> {
        self.db.with_conn(|conn| {
            let view = query::get_view(conn, failure_id)?
                .ok_or_else(|| anyhow::anyhow!("failure `{failure_id}` not found"))?;
            let timeline = query::timeline(conn, failure_id)?;
            let logs = crate::platform::logging::query::for_failure(conn, failure_id)?;
            Ok(redaction::redact_value(serde_json::json!({
                "schema": "synth.failure-bundle.v1",
                "failure": view,
                "timeline": timeline,
                "logs": logs,
            })))
        })
    }
}
