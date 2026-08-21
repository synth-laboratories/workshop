//! Stdio MCP adapter for local diagnostics.
//!
//! The agent gets five typed operations and nothing else. There is no LogsQL
//! parameter, no SQL parameter, no path, no URL, and no way to reach the
//! sidecar's own endpoints: every argument is an allow-listed field that the
//! Desktop compiles into a bounded query on the other side of the loopback.

#[path = "../ipc/mcp_stdio.rs"]
mod mcp_stdio;

#[path = "../instance_paths.rs"]
mod instance_paths;

use mcp_stdio::{run_stdio_server, McpServerInfo};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::{self, Write},
    path::PathBuf,
};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    url: String,
    token: String,
}

/// Arguments the adapter will forward. Anything else is refused before a
/// request is made — this list is the reason a hallucinated `logsql` or `path`
/// argument fails loudly instead of reaching the Desktop.
const ALLOWED_ARGUMENTS: &[&str] = &[
    "scope",
    "component",
    "severity",
    "code",
    "event",
    "since",
    "until",
    "limit",
    "cursor",
    "instance_id",
    "session_id",
    "turn_id",
    "tool_call_id",
    "command_id",
    "visual_id",
    "container_id",
    "rollout_id",
    "stream_id",
    "optimizer_run_id",
    "trace_id",
    "sessionRef",
];

fn connection_file() -> PathBuf {
    instance_paths::ipc_connection_file(
        &["SYNTH_DESKTOP_IPC_FILE"],
        "visuals-ipc.json",
    )
}

fn request(method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let connection: Connection = serde_json::from_str(
        &fs::read_to_string(connection_file()).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let mut payload_value = body.unwrap_or_else(|| json!({}));
    if let Some(object) = payload_value.as_object_mut() {
        if !object.contains_key("sessionRef") && !object.contains_key("session_id") {
            if let Ok(session) = env::var("SYNTH_SESSION_ID") {
                if !session.trim().is_empty() {
                    object.insert("sessionRef".into(), json!(session));
                }
            }
        }
    }
    let payload = serde_json::to_vec(&payload_value).unwrap_or_default();
    let addr = connection
        .url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or_default()
        .parse::<std::net::SocketAddr>()
        .map_err(|error| error.to_string())?;
    let mut stream = std::net::TcpStream::connect(addr).map_err(|error| error.to_string())?;
    let wire = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        connection.token,
        payload.len()
    );
    stream
        .write_all(wire.as_bytes())
        .and_then(|_| stream.write_all(&payload))
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    io::Read::read_to_string(&mut stream, &mut response).map_err(|error| error.to_string())?;
    let body = response
        .split("\r\n\r\n")
        .nth(1)
        .ok_or_else(|| "empty IPC response".to_string())?;
    serde_json::from_str(body).map_err(|error| error.to_string())
}

fn tools() -> Value {
    json!({"tools":[
        {
            "name": "diagnostics_manage",
            "description": "Query the local diagnostic record of this Workshop instance. Correlates renderer, Tauri, MCP, container, stream, visual, optimizer, and provider failures by shared identity. Operations: status, query, tail, explain, bundle. `explain` takes the identities you already hold (visual_id, session_id, rollout_id, stream_id, trace_id, …) and returns the upstream cause, its downstream symptoms, the evidence, and a remediation. Everything is local; no raw query language, SQL, or filesystem access is accepted. Load the use-synth-diagnostics skill.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "operation": {"type": "string", "enum": ["status", "query", "tail", "explain", "bundle"]},
                    "arguments": {
                        "type": "object",
                        "properties": {
                            "scope": {"type": "array", "items": {"type": "string", "enum": ["renderer", "visuals", "containers", "streams", "mcp", "optimizers", "providers", "session", "storage", "diagnostics"]}},
                            "component": {"type": "array", "items": {"type": "string"}},
                            "severity": {"type": "array", "items": {"type": "string", "enum": ["debug", "info", "warn", "error"]}},
                            "code": {"type": "array", "items": {"type": "string"}, "description": "Stable diagnostic codes, e.g. unsupported_trace_projection_schema."},
                            "event": {"type": "array", "items": {"type": "string"}},
                            "since": {"type": "string", "description": "Window such as 20m, 2h, or 7d. 7d is the maximum."},
                            "until": {"type": "string"},
                            "limit": {"type": "integer", "minimum": 1, "maximum": 500},
                            "cursor": {"type": "integer", "description": "Journal sequence from a previous response."},
                            "instance_id": {"type": "string"},
                            "session_id": {"type": "string"},
                            "turn_id": {"type": "string"},
                            "tool_call_id": {"type": "string"},
                            "command_id": {"type": "string"},
                            "visual_id": {"type": "string"},
                            "container_id": {"type": "string"},
                            "rollout_id": {"type": "string"},
                            "stream_id": {"type": "string"},
                            "optimizer_run_id": {"type": "string"},
                            "trace_id": {"type": "string"}
                        },
                        "additionalProperties": false
                    }
                },
                "required": ["operation"],
                "additionalProperties": false
            }
        }
    ]})
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    if name != "diagnostics_manage" {
        return Err(format!("unknown tool {name}"));
    }
    let operation = args
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| "operation required".to_string())?;
    let nested = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
    if let Some(object) = nested.as_object() {
        for key in object.keys() {
            if !ALLOWED_ARGUMENTS.contains(&key.as_str()) {
                return Err(format!("diagnostics arguments reject `{key}`"));
            }
        }
    } else if !nested.is_null() {
        return Err("diagnostics arguments must be an object".into());
    }
    match operation {
        "status" | "diagnostics_status" => request("POST", "/v1/diagnostics/status", None),
        "query" | "diagnostics_query" => request("POST", "/v1/diagnostics/query", Some(nested)),
        "tail" | "diagnostics_tail" => request("POST", "/v1/diagnostics/tail", Some(nested)),
        "explain" | "diagnostics_explain" => {
            request("POST", "/v1/diagnostics/explain", Some(nested))
        }
        "bundle" | "diagnostics_bundle" => request("POST", "/v1/diagnostics/bundle", Some(nested)),
        other => Err(format!("unknown diagnostics operation `{other}`")),
    }
}

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-diagnostics-mcp",
            version: "0.4.0",
        },
        tools,
        call_tool,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schema_offers_no_raw_query_language_or_filesystem_reach() {
        let encoded = tools()["tools"][0]["inputSchema"].to_string();
        for forbidden in ["\"logsql\"", "\"sql\"", "\"path\"", "\"url\"", "\"file\""] {
            assert!(!encoded.contains(forbidden), "{forbidden} is reachable");
        }
        assert!(!encoded.contains("additionalProperties\":true"));
    }

    #[test]
    fn arguments_outside_the_allow_list_are_refused_before_any_request() {
        for hostile in ["logsql", "sql", "path", "url", "storageDataPath"] {
            let error = call_tool(
                "diagnostics_manage",
                &json!({"operation": "query", "arguments": {hostile: "anything"}}),
            )
            .unwrap_err();
            assert!(error.contains("reject"), "{hostile}: {error}");
        }
    }

    #[test]
    fn unknown_operations_are_refused_before_any_request() {
        let error = call_tool("diagnostics_manage", &json!({"operation": "delete"})).unwrap_err();
        assert!(error.contains("unknown diagnostics operation"), "{error}");
    }

    #[test]
    fn every_allowed_argument_appears_in_the_published_schema() {
        let schema = tools()["tools"][0]["inputSchema"]["properties"]["arguments"]["properties"]
            .as_object()
            .expect("argument schema")
            .clone();
        for argument in ALLOWED_ARGUMENTS {
            if *argument == "sessionRef" {
                continue;
            }
            assert!(schema.contains_key(*argument), "{argument} is undocumented");
        }
    }

    #[test]
    fn the_five_documented_operations_are_all_offered() {
        let operations = tools()["tools"][0]["inputSchema"]["properties"]["operation"]["enum"]
            .as_array()
            .expect("operations")
            .clone();
        for expected in ["status", "query", "tail", "explain", "bundle"] {
            assert!(operations.contains(&json!(expected)), "{expected} missing");
        }
    }
}
