//! Container settlement: current_failure_id and typed health observation.
//!
//! The generic failure runtime never writes container SQL.

use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection, Transaction};

use crate::platform::failure::{
    definition::FailureDefinition, ContainerFailure, FailureKind, FailureStateEffect,
    HealthObservation, HealthSource, HealthStatus, OperationalFailure,
};
use crate::platform::operations::{OperationContext, OperationKind, OperationPhase};
use crate::platform::persistence::DomainSettlement;

pub struct ContainerSettlement;

impl DomainSettlement for ContainerSettlement {
    fn apply(&self, tx: &Transaction<'_>, failure: &OperationalFailure) -> Result<()> {
        apply_conn(tx, failure)
    }
}

pub fn apply_conn(conn: &Connection, failure: &OperationalFailure) -> Result<()> {
    let FailureKind::Container(kind) = &failure.kind else {
        return Ok(());
    };
    let container_id = match kind {
        ContainerFailure::Unhealthy { container_id, .. }
        | ContainerFailure::HealthAuthorityConflict { container_id, .. }
        | ContainerFailure::LaunchFailed { container_id, .. }
        | ContainerFailure::LaunchDeclarationMissing { container_id, .. }
        | ContainerFailure::CapabilitiesStale { container_id, .. }
        | ContainerFailure::CapabilityMismatch { container_id, .. }
        | ContainerFailure::ProtocolUnsupported { container_id, .. }
        | ContainerFailure::SourceRevisionMismatch { container_id, .. } => container_id.as_str(),
        ContainerFailure::NotFound { .. } | ContainerFailure::SelectionAmbiguous { .. } => {
            return Ok(())
        }
    };
    if matches!(
        failure.kind.state_effect(),
        FailureStateEffect::MarkContainerUnhealthy | FailureStateEffect::BlockContainerMutation
    ) {
        conn.execute(
            "UPDATE containers
             SET current_failure_id = ?1,
                 status = CASE WHEN status = 'stopped' THEN status ELSE 'unhealthy' END,
                 updated_at = ?2
             WHERE id = ?3",
            params![
                failure.failure_id.as_str(),
                Utc::now().to_rfc3339(),
                container_id
            ],
        )?;
    } else {
        conn.execute(
            "UPDATE containers SET current_failure_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![
                failure.failure_id.as_str(),
                Utc::now().to_rfc3339(),
                container_id
            ],
        )?;
    }
    Ok(())
}

pub fn clear_current(conn: &Connection, container_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE containers SET current_failure_id = NULL, updated_at = ?1 WHERE id = ?2",
        params![Utc::now().to_rfc3339(), container_id],
    )?;
    Ok(())
}

pub fn registry_observation(status: &str, health: &serde_json::Value) -> HealthObservation {
    let http_status = health
        .get("status")
        .and_then(|v| v.as_u64())
        .map(|v| v as u16);
    let summary = health
        .get("error")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    HealthObservation {
        source: HealthSource::Registry,
        status: HealthStatus::parse(status),
        observed_at: Utc::now(),
        http_status,
        summary,
    }
}

pub fn live_observation(status: &str, health: &serde_json::Value) -> HealthObservation {
    let mut observation = registry_observation(status, health);
    observation.source = HealthSource::LiveProbe;
    observation
}

pub fn classify_probe(
    container_id: &str,
    registry: HealthObservation,
    live: HealthObservation,
    deliberately_stopped: bool,
) -> Option<ContainerFailure> {
    if deliberately_stopped {
        return None;
    }
    if registry.status != live.status
        && registry.status != HealthStatus::Unknown
        && live.status != HealthStatus::Unknown
    {
        return Some(ContainerFailure::HealthAuthorityConflict {
            container_id: container_id.to_owned(),
            registry,
            live_probe: live,
        });
    }
    if live.status == HealthStatus::Unhealthy {
        return Some(ContainerFailure::Unhealthy {
            container_id: container_id.to_owned(),
            observation: live,
        });
    }
    None
}

pub fn raise_probe_failure(
    conn: &Connection,
    failure: ContainerFailure,
    container_id: &str,
    session_id: Option<String>,
) -> Result<OperationalFailure> {
    let context = OperationContext::bootstrap(crate::instance::boot_epoch())
        .for_container(container_id.to_owned());
    let mut context = context;
    context.session_id = session_id;
    let raised = crate::platform::failure::FailureRuntime::raise_in_tx(
        conn,
        FailureKind::Container(failure),
        context,
        OperationKind::ContainerProbe,
        OperationPhase::Probe,
        None,
        "container_authority",
    )?;
    apply_conn(conn, &raised)?;
    Ok(raised)
}

pub fn from_preflight(
    error: &crate::container_capabilities::ContainerPreflightError,
) -> ContainerFailure {
    match error.code.as_str() {
        crate::container_capabilities::CODE_UNHEALTHY => ContainerFailure::Unhealthy {
            container_id: error.container_id.clone(),
            observation: HealthObservation {
                source: HealthSource::Registry,
                status: HealthStatus::Unhealthy,
                observed_at: Utc::now(),
                http_status: None,
                summary: error.last_probe_error.clone(),
            },
        },
        crate::container_capabilities::CODE_CAPABILITIES_STALE => {
            ContainerFailure::CapabilitiesStale {
                container_id: error.container_id.clone(),
                observed_at: error.observed_at.clone(),
            }
        }
        crate::container_capabilities::CODE_CAPABILITY_MISMATCH => {
            ContainerFailure::CapabilityMismatch {
                container_id: error.container_id.clone(),
                missing: error.missing.clone(),
            }
        }
        _ => ContainerFailure::CapabilityMismatch {
            container_id: error.container_id.clone(),
            missing: error.missing.clone(),
        },
    }
}
