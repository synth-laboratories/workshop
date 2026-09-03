//! Agent-facing control for Workshop's displayed plugin destinations.

#[path = "../instance_paths.rs"]
mod instance_paths;
#[path = "../ipc/mcp_stdio.rs"]
mod mcp_stdio;

use mcp_stdio::{run_stdio_server, McpServerInfo};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    time::Duration,
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

fn request(method: &str, path: &str, body: Value) -> Result<Value, String> {
    let connection: Connection =
        serde_json::from_str(&fs::read_to_string(connection_file()).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let addr = connection
        .url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or_default()
        .parse()
        .map_err(|e: std::net::AddrParseError| e.to_string())?;
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(10)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    let payload = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    let wire = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", connection.token, payload.len());
    stream
        .write_all(wire.as_bytes())
        .and_then(|_| stream.write_all(&payload))
        .map_err(|e| e.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or("malformed display IPC response")?;
    if !headers.lines().next().unwrap_or_default().contains(" 2") {
        return Err(body.trim().to_string());
    }
    serde_json::from_str(body).map_err(|e| e.to_string())
}

fn tools() -> Value {
    json!({"tools":[{"name":"workshop_display","description":"List Workshop plugin destinations or choose which are visible in the user's sidebar. Use only when the user asks to change the Workshop display.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","enum":["list","set_visible"]},"visible_plugin_ids":{"type":"array","items":{"type":"string","enum":["visuals","reports","experiments","optimizers","inventory","inference","computer-use"]}}},"required":["operation"],"additionalProperties":false}}]})
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    if name != "workshop_display" {
        return Err(format!("unknown tool {name}"));
    }
    match args.get("operation").and_then(Value::as_str) {
        Some("list") => request("GET", "/v1/display/plugins", json!({})),
        Some("set_visible") => {
            let ids = args
                .get("visible_plugin_ids")
                .and_then(Value::as_array)
                .ok_or("visible_plugin_ids required")?;
            request(
                "POST",
                "/v1/display/plugins/visibility",
                json!({"visiblePluginIds": ids}),
            )
        }
        _ => Err("unsupported operation".into()),
    }
}

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "workshop-display",
            version: env!("CARGO_PKG_VERSION"),
        },
        tools,
        call_tool,
    );
}
