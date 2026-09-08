//! Stdio MCP adapter for Trace V5 inspection.

use crate::ipc::mcp_stdio;

use crate::instance::paths as instance_paths;

use mcp_stdio::{run_stdio_server, McpServerInfo};
use serde_json::{json, Value};
use std::{
    env,
    path::PathBuf,
};


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
        {"name":"trace_manage","description":"Inspect sealed Trace V5 archives, run typed read-only queries over the trace index, import a container's sealed trace by identity, and open a trace in the Desktop right panel. Archives are never mutated and no SQL is accepted. Load the use-synth-traces skill.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","enum":["list","get","open","query","snapshot","open_query","import"]},"arguments":{"type":"object","properties":{"trace_id":{"type":"string"},"snapshot_id":{"type":"string"},"container_id":{"type":"string","description":"import only: the registered container that sealed the trace. Workshop resolves its URL itself."},"rollout_id":{"type":"string","description":"import only: the rollout whose sealed trace to import."},"query":{"type":"object","description":"Typed trace query. Fields are allow-listed and compile to a parameterized statement; a hard row cap applies."}},"additionalProperties":false}},"required":["operation"],"additionalProperties":false}}
    ]})
}

pub(crate) fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    super::check_agent_call(name, args).map_err(|error| error.to_string())?;
    if name != "trace_manage" {
        return Err(format!("unknown tool {name}"));
    }
    let operation = args
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| "operation required".to_string())?;
    let nested = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
    // Reject anything that is not a trace identity. Paths, URLs, and query
    // strings are how an agent-facing surface turns into arbitrary data access.
    if let Some(object) = nested.as_object() {
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "trace_id"
                    | "snapshot_id"
                    | "query"
                    | "sessionRef"
                    | "session_id"
                    // Import names a container and a rollout, never a path or a
                    // URL: Workshop resolves the container's address from its
                    // own trusted registry.
                    | "container_id"
                    | "rollout_id"
            ) {
                return Err(format!("trace arguments reject `{key}`"));
            }
        }
    }
    match operation {
        "list" => request("GET", "/v1/traces", None),
        "get" => request("POST", "/v1/traces/get", Some(nested)),
        "open" => request("POST", "/v1/traces/open", Some(nested)),
        "query" => request("POST", "/v1/traces/query", Some(nested)),
        "snapshot" => request("POST", "/v1/traces/snapshot", Some(nested)),
        "open_query" => request("POST", "/v1/traces/open_query", Some(nested)),
        "import" => request("POST", "/v1/traces/import", Some(nested)),
        other => Err(format!("unknown trace operation `{other}`")),
    }
}

pub fn run() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-traces-mcp",
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
    fn schema_offers_no_sql_paths_or_urls() {
        let catalog = tools();
        let encoded = catalog["tools"][0]["inputSchema"].to_string();
        assert!(catalog.to_string().contains("trace_manage"));
        assert!(!encoded.contains("\"sql\""));
        assert!(!encoded.contains("\"path\""));
        assert!(!encoded.contains("\"url\""));
        assert!(!encoded.contains("additionalProperties\":true"));
    }

    #[test]
    fn arguments_reject_anything_but_a_trace_identity() {
        let err = call_tool(
            "trace_manage",
            &json!({"operation":"open","arguments":{"trace_id":"t1","path":"/etc/passwd"}}),
        )
        .unwrap_err();
        assert!(err.contains("reject"), "{err}");
    }

    /// Import exists because a container can seal a trace this Workshop has
    /// never seen. It still may not take a path or a URL: identity in, resolved
    /// address on the trusted side.
    #[test]
    fn import_takes_identities_and_still_refuses_paths() {
        let catalog = tools();
        let operations = catalog["tools"][0]["inputSchema"]["properties"]["operation"]["enum"]
            .as_array()
            .unwrap();
        assert!(operations.iter().any(|value| value == "import"));
        let encoded = catalog["tools"][0]["inputSchema"].to_string();
        assert!(!encoded.contains("\"path\""));
        assert!(!encoded.contains("\"url\""));
        let err = call_tool(
            "trace_manage",
            &json!({"operation":"import","arguments":{"container_id":"ctr_1","bundle_path":"/tmp/x"}}),
        )
        .unwrap_err();
        assert!(err.contains("reject"), "{err}");
    }

    #[test]
    fn unknown_operations_are_refused_before_any_request() {
        let err = call_tool("trace_manage", &json!({"operation":"delete"})).unwrap_err();
        assert!(err.contains("unknown trace operation"), "{err}");
    }
}
