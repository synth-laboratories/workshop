//! Stdio MCP adapter for Workshop product plugins.

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
        {"name":"plugin_manage","description":"Manage built-in Workshop product plugins. Load the use-synth-plugins skill. Callers supply only plugin_id and optional catalog version — never URLs, paths, commands, env, or tokens.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","enum":["list","status","capabilities","enable","disable","install","start","stop","update","remove"]},"arguments":{"type":"object","properties":{"plugin_id":{"type":"string","enum":["optimizers"]},"version":{"type":"string"}},"required":["plugin_id"],"additionalProperties":false}},"required":["operation","arguments"],"additionalProperties":false}}
    ]})
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
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

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-plugins-mcp",
            version: "0.3.0",
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
