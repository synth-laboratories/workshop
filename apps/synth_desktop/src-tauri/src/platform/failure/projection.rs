//! Pure FailureView projection. Renderer and MCP consume only this envelope.

use serde::{Deserialize, Serialize};

use super::definition::{FailureDefinition, FAILURE_SCHEMA_VERSION};
use super::occurrence::OperationalFailure;
use super::redaction::redact_value;
use super::remediation::FailureRemediationView;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FailureContextView {
    pub session_id: Option<String>,
    pub container_id: Option<String>,
    pub evaluation_id: Option<String>,
    pub rollout_id: Option<String>,
    pub visual_id: Option<String>,
    pub operation_id: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub facts: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FailureView {
    pub schema_version: String,
    pub failure_id: String,
    pub code: String,
    pub category: String,
    pub disposition: String,
    pub lifecycle_state: String,
    pub operation: String,
    pub phase: String,
    pub message: String,
    pub remediation: Option<FailureRemediationView>,
    pub safe_context: FailureContextView,
    pub diagnostic_reference: String,
}

impl FailureView {
    pub fn from_occurrence(failure: &OperationalFailure) -> Self {
        let facts = redact_value(failure.kind.safe_facts());
        Self {
            schema_version: FAILURE_SCHEMA_VERSION.into(),
            failure_id: failure.failure_id.0.clone(),
            code: failure.kind.code().into(),
            category: failure.kind.category().as_str().into(),
            disposition: failure.disposition.as_str().into(),
            lifecycle_state: failure.lifecycle_state.as_str().into(),
            operation: failure.operation.as_str().into(),
            phase: failure.phase.as_str().into(),
            message: failure.kind.message(),
            remediation: failure.kind.remediation().map(|r| r.view()),
            safe_context: FailureContextView {
                session_id: failure.context.session_id.clone(),
                container_id: failure.context.container_id.clone(),
                evaluation_id: failure.context.evaluation_id.clone(),
                rollout_id: failure.context.rollout_id.clone(),
                visual_id: failure.context.visual_id.clone(),
                operation_id: failure.context.operation_id.as_ref().map(|id| id.0.clone()),
                facts,
            },
            diagnostic_reference: failure.failure_id.0.clone(),
        }
    }
}

/// Parse a boundary envelope. Anything that is not `synth.failure-view.v1` is
/// `failure_contract_invalid` — never shown as raw transport prose.
pub fn parse_view(value: &serde_json::Value) -> Result<FailureView, String> {
    if value.get("schemaVersion").and_then(|v| v.as_str()) != Some(FAILURE_SCHEMA_VERSION)
        && value.get("schema_version").and_then(|v| v.as_str()) != Some(FAILURE_SCHEMA_VERSION)
    {
        return Err("missing synth.failure-view.v1".into());
    }
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}
