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
    json!({"tools":[
        {"name":"plugin_manage","description":"Manage built-in Workshop product plugins. Load the use-synth-plugins skill. Callers supply only plugin_id and optional catalog version — never URLs, paths, commands, env, or tokens.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","enum":["list","status","capabilities","enable","disable","install","start","restart","stop","update","remove"]},"arguments":{"type":"object","properties":{"plugin_id":{"type":"string","enum":["optimizers"]},"version":{"type":"string"}},"required":["plugin_id"],"additionalProperties":false}},"required":["operation","arguments"],"additionalProperties":false}}
    ]})
}

pub(crate) fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    super::check_agent_call(name, args).map_err(|error| error.to_string())?;
    if name != "plugin_manage" {
        return Err(format!("unknown tool {name}"));
    }
    let operation = args
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| "operation required".to_string())?;
    let mut nested = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
    if let Some(object) = nested.as_object() {
        for key in object.keys() {
            if key != "plugin_id" && key != "version" && key != "sessionRef" && key != "session_id"
            {
                return Err(format!("plugin arguments reject `{key}`"));
            }
        }
    }
    if let Some(object) = nested.as_object_mut() {
        object
            .entry("plugin_id")
            .or_insert_with(|| json!("optimizers"));
    }
    request(
        "POST",
        "/v1/plugins/manage",
        Some(json!({
            "operation": operation,
            "arguments": nested
        })),
    )
}

pub fn run() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-plugins-mcp",
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
    fn schema_rejects_urls_paths_and_arbitrary_plugins() {
        let catalog = tools();
        let schema = &catalog["tools"][0]["inputSchema"];
        let encoded = schema.to_string();
        assert!(catalog.to_string().contains("plugin_manage"));
        assert!(encoded.contains("optimizers"));
        assert!(!encoded.contains("additionalProperties\":true"));
        assert!(!encoded.contains("\"url\""));
        assert!(!encoded.contains("\"command\""));
        assert!(!encoded.contains("\"token\""));
        let err = call_tool(
            "plugin_manage",
            &json!({
                "operation": "install",
                "arguments": {"plugin_id":"optimizers","url":"https://evil.example"}
            }),
        )
        .unwrap_err();
        assert!(err.contains("reject"));
    }
}
