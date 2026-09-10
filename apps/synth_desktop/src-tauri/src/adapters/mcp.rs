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

pub mod operations;

#[path = "../agent_integration/transport.rs"]
mod runtime_client;

/// Compatibility transports select from the runtime catalogue; they never own
/// a second browser or storage graph.
pub fn run_browser_stdio() -> anyhow::Result<()> {
    let client = runtime_client::RuntimeClient::new(crate::storage::app_data_root())?;
    client.describe()?;
    crate::ipc::mcp_stdio::run_stdio_server(
        crate::ipc::mcp_stdio::McpServerInfo { name: "synth-browser-mcp", version: env!("CARGO_PKG_VERSION") },
        crate::browser::operations::tools,
        |name, args| client.call(name, args).map_err(|error| error.to_string()),
    );
    Ok(())
}

pub(crate) fn local_request(descriptor: &std::path::Path, method: &str, route: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
    if descriptor.file_name().and_then(|name| name.to_str()) != Some("visuals-ipc.json") {
        return Err("select the instance's visuals-ipc.json descriptor".into());
    }
    let root = descriptor.parent().ok_or("IPC descriptor has no instance root")?;
    runtime_client::RuntimeClient::new(root.to_path_buf()).and_then(|client| client.request_route(method, route, body)).map_err(|error| error.to_string())
}
