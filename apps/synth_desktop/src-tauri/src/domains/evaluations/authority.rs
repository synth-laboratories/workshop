use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};

use crate::optimizers::admission::error::AdmissionError;
use crate::platform::failure::{
    AdmissionFailure, FailureKind, OperationalFailure,
};
use crate::platform::operations::{OperationContext, OperationKind, OperationPhase};

pub fn from_admission(error: &AdmissionError) -> AdmissionFailure {
    use crate::optimizers::admission::error::AdmissionErrorCode::*;
    match error.code {
        CatalogRecipeNotFound => AdmissionFailure::CatalogRecipeNotFound {
            recipe_id: error.context["recipeId"].as_str().unwrap_or("unknown").into(),
            searched: error.context["recipesSearched"].as_u64().unwrap_or(0) as usize,
        },
        EvaluatorNotDeclared => AdmissionFailure::EvaluatorNotDeclared {
            container_id: error.context["containerId"].as_str().unwrap_or("unknown").into(),
        },
        ScoringContractInvalid => AdmissionFailure::ScoringContractInvalid {
            container_id: error.context["containerId"].as_str().unwrap_or("unknown").into(),
            detail: error.context["detail"].as_str().unwrap_or(&error.message).into(),
        },
        PolicyNotFound => AdmissionFailure::PolicyNotFound {
            namespace: error.context["namespace"].as_str().unwrap_or("").into(),
            name: error.context["name"].as_str().unwrap_or("").into(),
        },
        PolicyRevisionUnresolved => AdmissionFailure::PolicyRevisionUnresolved {
            namespace: error.context["namespace"].as_str().unwrap_or("").into(),
            name: error.context["name"].as_str().unwrap_or("").into(),
        },
        PolicyConfigurationInvalid => AdmissionFailure::PolicyConfigurationInvalid {
            namespace: error.context["namespace"].as_str().unwrap_or("").into(),
            name: error.context["name"].as_str().unwrap_or("").into(),
            detail: error.context["detail"].as_str().unwrap_or(&error.message).into(),
        },
        ModelUnsupported => AdmissionFailure::ModelUnsupported {
            provider: error.context["providerId"].as_str().unwrap_or("").into(),
            model: error.context["modelId"].as_str().unwrap_or("").into(),
            container_id: error.context["containerId"].as_str().unwrap_or("").into(),
        },
        SeedControlUnsupported => AdmissionFailure::SeedControlUnsupported {
            container_id: error.context["containerId"].as_str().unwrap_or("").into(),
        },
        RequestedLimitUnsupported => AdmissionFailure::RequestedLimitUnsupported {
            limit: error.context["limit"].as_str().unwrap_or("limit").into(),
            requested: error.context["requested"].as_u64().unwrap_or(0),
            maximum_supported: error.context["maximumSupported"].as_u64(),
        },
        CostCeilingRequired => AdmissionFailure::CostCeilingRequired,
        CredentialRouteUnavailable => AdmissionFailure::CredentialRouteUnavailable {
            provider: error.context["providerId"].as_str().unwrap_or("").into(),
            detail: error.context["detail"].as_str().unwrap_or(&error.message).into(),
        },
        OutputContractUnsupported => AdmissionFailure::OutputContractUnsupported {
            container_id: error.context["containerId"].as_str().unwrap_or("").into(),
            missing: error.context["missingOperations"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        },
        ExecutionSpecInvalid => AdmissionFailure::ExecutionSpecInvalid {
            detail: error.context["detail"].as_str().unwrap_or(&error.message).into(),
        },
        ExecutionSpecDigestMismatch => AdmissionFailure::ExecutionSpecDigestMismatch {
            expected: error.context["expectedDigest"].as_str().unwrap_or("").into(),
            actual: error.context["actualDigest"].as_str().unwrap_or("").into(),
        },
        ContainerNotFound => {
            // Admission still names these codes; container authority owns the
            // container variants. Map to admission-adjacent facts that keep the
            // original public code via a dedicated admission wrapper only when
            // raised during admission — the container enum owns probe-time
            // not-found. Here we keep the admission subject by using
            // ExecutionSpecInvalid with the original code preserved in detail
            // for non-container stages, and Catalog-style facts for lookup.
            AdmissionFailure::ExecutionSpecInvalid {
                detail: error.message.clone(),
            }
        }
        ContainerSelectionAmbiguous | ContainerUnhealthy | ContainerProtocolUnsupported => {
            AdmissionFailure::ExecutionSpecInvalid {
                detail: error.message.clone(),
            }
        }
    }
}

/// Container-coded admission refusals stay container failures so the Errors
/// pane and restart remediation share one identity with probe-time health.
pub fn kind_from_admission(error: &AdmissionError) -> FailureKind {
    use crate::optimizers::admission::error::AdmissionErrorCode::*;
    match error.code {
        ContainerNotFound => FailureKind::Container(
            crate::platform::failure::ContainerFailure::NotFound {
                requested: error.context["requested"]
                    .as_str()
                    .unwrap_or("unknown")
                    .into(),
            },
        ),
        ContainerSelectionAmbiguous => FailureKind::Container(
            crate::platform::failure::ContainerFailure::SelectionAmbiguous {
                candidates: error.context["candidates"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
            },
        ),
        ContainerUnhealthy => FailureKind::Container(
            crate::platform::failure::ContainerFailure::Unhealthy {
                container_id: error.context["containerId"]
                    .as_str()
                    .unwrap_or("unknown")
                    .into(),
                observation: crate::platform::failure::HealthObservation {
                    source: crate::platform::failure::HealthSource::Registry,
                    status: crate::platform::failure::HealthStatus::Unhealthy,
                    observed_at: Utc::now(),
                    http_status: None,
                    summary: error.context["observedHealth"]
                        .as_str()
                        .map(str::to_owned),
                },
            },
        ),
        ContainerProtocolUnsupported => FailureKind::Container(
            crate::platform::failure::ContainerFailure::ProtocolUnsupported {
                container_id: error.context["containerId"]
                    .as_str()
                    .unwrap_or("unknown")
                    .into(),
                required: error.context["requiredProtocol"]
                    .as_str()
                    .unwrap_or("")
                    .into(),
                observed: error.context["advertisedProtocol"]
                    .as_str()
                    .map(str::to_owned),
            },
        ),
        _ => FailureKind::Admission(from_admission(error)),
    }
}

pub fn raise(
    conn: &Connection,
    error: &AdmissionError,
    evaluation_id: Option<&str>,
) -> Result<OperationalFailure> {
    let mut context = OperationContext::bootstrap(crate::instance::boot_epoch());
    context.evaluation_id = evaluation_id.map(str::to_owned);
    if let Some(container_id) = error.context.get("containerId").and_then(|v| v.as_str()) {
        context.container_id = Some(container_id.to_owned());
    }
    let raised = crate::platform::failure::FailureRuntime::raise_in_tx(
        conn,
        kind_from_admission(error),
        context,
        OperationKind::EvaluationAdmit,
        OperationPhase::Admit,
        None,
        "evaluation_authority",
    )?;
    if let Some(run_id) = evaluation_id {
        conn.execute(
            "UPDATE optimizer_runs SET terminal_failure_id = ?1 WHERE id = ?2",
            params![raised.failure_id.as_str(), run_id],
        )?;
    }
    Ok(raised)
}
