//! Stdio MCP adapter for Trace V5 inspection.

#[path = "../ipc/mcp_stdio.rs"]
mod mcp_stdio;

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
    env::var("SYNTH_DESKTOP_IPC_FILE")
        .or_else(|_| env::var("SYNTH_VISUALS_IPC_FILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            env::var_os("SYNTH_DESKTOP_DATA_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    dirs::data_dir()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("Synth Desktop")
                })
                .join("visuals-ipc.json")
        })
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
        {"name":"trace_manage","description":"Inspect sealed Trace V5 archives, run typed read-only queries over the trace index, and open a trace in the Desktop right panel. Archives are never mutated and no SQL is accepted. Load the use-synth-traces skill.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","enum":["list","get","open","query","snapshot"]},"arguments":{"type":"object","properties":{"trace_id":{"type":"string"},"snapshot_id":{"type":"string"},"query":{"type":"object","description":"Typed trace query. Fields are allow-listed and compile to a parameterized statement; a hard row cap applies."}},"additionalProperties":false}},"required":["operation"],"additionalProperties":false}}
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
                "trace_id" | "snapshot_id" | "query" | "sessionRef" | "session_id"
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
        other => Err(format!("unknown trace operation `{other}`")),
    }
}

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-traces-mcp",
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

    #[test]
    fn unknown_operations_are_refused_before_any_request() {
        let err = call_tool("trace_manage", &json!({"operation":"delete"})).unwrap_err();
        assert!(err.contains("unknown trace operation"), "{err}");
    }
}
