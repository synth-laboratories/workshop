use crate::platform::failure::{FailureView, OperationalFailure};

pub fn tool_error_body(failure: &OperationalFailure) -> serde_json::Value {
    let view = FailureView::from_occurrence(failure);
    serde_json::json!({
        "schemaVersion": view.schema_version,
        "failureId": view.failure_id,
        "code": view.code,
        "category": view.category,
        "disposition": view.disposition,
        "lifecycleState": view.lifecycle_state,
        "operation": view.operation,
        "phase": view.phase,
        "message": view.message,
        "remediation": view.remediation,
        "safeContext": view.safe_context,
        "diagnosticReference": view.diagnostic_reference,
        "retryable": view.disposition == "retryable" || view.disposition == "approval_required",
    })
}
