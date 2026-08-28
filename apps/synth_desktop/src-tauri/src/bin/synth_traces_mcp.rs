//! Stdio MCP adapter for Trace V5 inspection.

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

fn connection_file() -> PathBuf {
    instance_paths::ipc_connection_file(
        &["SYNTH_DESKTOP_IPC_FILE", "SYNTH_VISUALS_IPC_FILE"],
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
        {"name":"trace_manage","description":"Inspect sealed Trace V5 archives, run typed read-only queries over the trace index, import a container's sealed trace by identity, and open a trace in the Desktop right panel. Archives are never mutated and no SQL is accepted. Load the use-synth-traces skill.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","enum":["list","get","open","query","snapshot","open_query","import"]},"arguments":{"type":"object","properties":{"trace_id":{"type":"string"},"snapshot_id":{"type":"string"},"container_id":{"type":"string","description":"import only: the registered container that sealed the trace. Workshop resolves its URL itself."},"rollout_id":{"type":"string","description":"import only: the rollout whose sealed trace to import."},"query":{"type":"object","description":"Typed trace query. Fields are allow-listed and compile to a parameterized statement; a hard row cap applies."}},"additionalProperties":false}},"required":["operation"],"additionalProperties":false}}
    ]})
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
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

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-traces-mcp",
            version: env!("CARGO_PKG_VERSION"),
        },
        tools,
        call_tool,
    );
}

