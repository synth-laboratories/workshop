//! Stdio MCP adapter for Workshop product plugins.

use crate::ipc::mcp_stdio;

use crate::instance::paths as instance_paths;

use mcp_stdio::{run_stdio_server, McpServerInfo};
use serde_json::{json, Value};
use std::{
    env,
    path::PathBuf,
    time::Duration,
};

// Starting a cold local sidecar is allowed to take up to the Desktop's
// readiness ceiling. The MCP adapter must still finish with a diagnostic
// instead of waiting indefinitely for a half-closed IPC connection.
const IPC_REQUEST_TIMEOUT: Duration = Duration::from_secs(75);


fn connection_file() -> PathBuf {
    instance_paths::ipc_connection_file(
        &["SYNTH_DESKTOP_IPC_FILE", "SYNTH_VISUALS_IPC_FILE"],
        "visuals-ipc.json",
    )
}

fn request(method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let mut payload = body.unwrap_or_else(|| json!({}));

    if let Some(object) = payload.as_object_mut() {
        if !object.contains_key("sessionRef") && !object.contains_key("session_id") {
            if let Ok(session) = env::var("SYNTH_SESSION_ID") {
                if !session.trim().is_empty() { object.insert("sessionRef".into(), json!(session)); }
            }
        }
    }
    crate::adapters::mcp::local_request(&connection_file(), method, path, payload)
}

pub(crate) fn tools() -> Value {
    json!({"tools":[{"name":"jesterky_prepare","description":"Prepare optional Jesterky analysis targets from immutable trace query result IDs. Free; does not start compute. Independent of annotated eval jobs; defaults to the saved annotation scope. Requires installed Jesterky. Load use-synth-jesterky.","inputSchema":{"type":"object","properties":{"snapshot_id":{"type":"string"},"annotation_scope":{"type":"string","enum":["selected_rollouts","selected_evidence"],"description":"Override saved scope for this preparation only. selected_rollouts analyzes entire selected rollouts; selected_evidence analyzes selected event rows."},"result_ids":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":200}},"required":["snapshot_id","result_ids"],"additionalProperties":false}},{"name":"jesterky_settings","description":"Read the saved Jesterky analysis scope; no compute.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}]})
}
pub(crate) fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    if name == "jesterky_settings" {
        if !args.as_object().is_some_and(|o| o.is_empty()) {
            return Err("settings takes no arguments".into());
        }
        return request("POST", "/v1/jesterky/settings", None);
    }
    if name != "jesterky_prepare" {
        return Err(format!("unknown Jesterky tool {name}"));
    }
    let object = args.as_object().ok_or("arguments must be an object")?;
    if object
        .keys()
        .any(|k| k != "snapshot_id" && k != "result_ids" && k != "annotation_scope")
    {
        return Err("unknown preparation argument".into());
    }
    request("POST", "/v1/jesterky/prepare", Some(args.clone()))
}
pub fn run() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-jesterky-mcp",
            version: env!("CARGO_PKG_VERSION"),
        },
        tools,
        call_tool,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_advertises_independent_scope_and_settings_are_read_only() {
        let catalog = tools();
        assert_eq!(
            catalog["tools"][0]["inputSchema"]["properties"]["annotation_scope"]["enum"],
            json!(["selected_rollouts", "selected_evidence"])
        );
        assert_eq!(catalog["tools"][1]["name"], "jesterky_settings");
        assert!(call_tool(
            "jesterky_settings",
            &json!({"annotation_scope":"selected_rollouts"})
        )
        .unwrap_err()
        .contains("no arguments"));
        assert!(
            call_tool("jesterky_prepare", &json!({"run_id":"active-eval"}))
                .unwrap_err()
                .contains("unknown preparation argument")
        );
    }
}
