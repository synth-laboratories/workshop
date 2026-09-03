//! Agent-facing human review workflow. This surface creates immutable tasks and
//! opens the native Workshop review panel; it never exposes private drafts or
//! microphone bytes to the agent.
#![recursion_limit = "256"]

#[path = "../instance_paths.rs"]
mod instance_paths;
#[path = "../ipc/mcp_stdio.rs"]
mod mcp_stdio;

use mcp_stdio::{run_stdio_server, McpServerInfo};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::{self, Write},
    path::PathBuf,
    time::{Duration, Instant},
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
fn request(path: &str, body: Value) -> Result<Value, String> {
    let connection: Connection =
        serde_json::from_str(&fs::read_to_string(connection_file()).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut body = body;
    if let Some(o) = body.as_object_mut() {
        if !o.contains_key("sessionRef") {
            if let Ok(s) = env::var("SYNTH_SESSION_ID") {
                o.insert("sessionRef".into(), json!(s));
            }
        }
    }
    let payload = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    let addr = connection
        .url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or_default()
        .parse::<std::net::SocketAddr>()
        .map_err(|e| e.to_string())?;
    let mut stream = std::net::TcpStream::connect(addr).map_err(|e| e.to_string())?;
    let wire=format!("POST {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",connection.token,payload.len());
    stream
        .write_all(wire.as_bytes())
        .and_then(|_| stream.write_all(&payload))
        .map_err(|e| e.to_string())?;
    let mut response = String::new();
    io::Read::read_to_string(&mut stream, &mut response).map_err(|e| e.to_string())?;
    let raw = response
        .split("\r\n\r\n")
        .nth(1)
        .ok_or("empty IPC response")?;
    serde_json::from_str(raw).map_err(|e| e.to_string())
}
fn tools() -> Value {
    json!({"tools":[{"name":"human_annotation_manage","description":"Preview and validate before immutable create; then show the native right-panel review, wait/get, and export. human_annotation_get accepts taskId or sessionId for private-safe status, or the copied resultId for the full sealed record and seal. Campaign operations expose agreement, adjudication, close, and append-only result supersession. Human drafts, quiz keys, and microphone bytes remain private until sealed submission.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","enum":["human_annotation_preview","human_annotation_create","human_annotation_show","human_annotation_get","human_annotation_wait","human_annotation_list","human_annotation_export","human_annotation_cancel","human_annotation_supersede","human_annotation_campaign_create","human_annotation_campaign_status","human_annotation_campaign_close","human_annotation_campaign_adjudicate"]},"arguments":{"type":"object"}},"required":["operation"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}]})
}
fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    if name != "human_annotation_manage" {
        return Err(format!("unknown tool {name}"));
    }
    let op = args
        .get("operation")
        .and_then(Value::as_str)
        .ok_or("operation required")?;
    let nested = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
    match op {
        "human_annotation_preview" => request("/v1/human-annotations/preview", nested),
        "human_annotation_create" => request("/v1/human-annotations/create", nested),
        "human_annotation_show" => request("/v1/human-annotations/show", nested),
        "human_annotation_get" => request("/v1/human-annotations/get", nested),
        "human_annotation_list" => request("/v1/human-annotations/list", nested),
        "human_annotation_export" => request("/v1/human-annotations/export", nested),
        "human_annotation_cancel" => request("/v1/human-annotations/cancel", nested),
        "human_annotation_supersede" => request("/v1/human-annotations/supersede", nested),
        "human_annotation_campaign_create" => {
            request("/v1/human-annotations/campaign/create", nested)
        }
        "human_annotation_campaign_status" => {
            request("/v1/human-annotations/campaign/status", nested)
        }
        "human_annotation_campaign_close" => {
            request("/v1/human-annotations/campaign/close", nested)
        }
        "human_annotation_campaign_adjudicate" => {
            request("/v1/human-annotations/campaign/adjudicate", nested)
        }
        "human_annotation_wait" => {
            let timeout = nested
                .get("timeoutMs")
                .and_then(Value::as_u64)
                .unwrap_or(120_000)
                .min(300_000);
            let start = Instant::now();
            loop {
                let value = request("/v1/human-annotations/get", nested.clone())?;
                if matches!(
                    value.get("state").and_then(Value::as_str),
                    Some("submitted" | "superseded" | "cancelled" | "invalidated")
                ) {
                    return Ok(value);
                }
                if start.elapsed() >= Duration::from_millis(timeout) {
                    return Ok(json!({"state":"waiting","timeout":true,"last":value}));
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        _ => Err(format!("unknown human annotation operation `{op}`")),
    }
}
fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-human-annotations",
            version: env!("CARGO_PKG_VERSION"),
        },
        tools,
        call_tool,
    )
}
