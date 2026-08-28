//! Stdio MCP adapter for session presentation. Forwards through Desktop visuals IPC.
//!
//! Usage (Codex home config):
//!   command = "synth-session-mcp"
//!   env SYNTH_DESKTOP_IPC_FILE / SYNTH_SESSION_ID

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

fn display_err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn request(method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    request_inner(method, path, body).map_err(display_err)
}

fn request_inner(
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Result<Value, synth_desktop_lib::error::AppError> {
    let connection: Connection = serde_json::from_str(
        &fs::read_to_string(connection_file()).map_err(synth_desktop_lib::error::AppError::from)?,
    )
    .map_err(synth_desktop_lib::error::AppError::from)?;
    let payload = body
        .map(|v| serde_json::to_vec(&v).unwrap_or_default())
        .unwrap_or_default();
    let addr = connection
        .url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or_default()
        .parse::<std::net::SocketAddr>()
        .map_err(synth_desktop_lib::error::AppError::from)?;
    let mut stream =
        std::net::TcpStream::connect(addr).map_err(synth_desktop_lib::error::AppError::from)?;
    let wire = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        connection.token,
        payload.len()
    );
    stream
        .write_all(wire.as_bytes())
        .and_then(|_| stream.write_all(&payload))
        .map_err(synth_desktop_lib::error::AppError::from)?;
    let mut response = String::new();
    io::Read::read_to_string(&mut stream, &mut response)
        .map_err(synth_desktop_lib::error::AppError::from)?;
    serde_json::from_str(
        response
            .split("\r\n\r\n")
            .nth(1)
            .ok_or_else(|| synth_desktop_lib::error::AppError::untyped("empty IPC response"))?,
    )
    .map_err(synth_desktop_lib::error::AppError::from)
}

fn tools() -> Value {
    json!({"tools":[
        {"name":"session_present","description":"Set this conversation's title, mascot emotion, and a ≤7-word summary. Load the use-synth-session skill. Title is a manual CoreRuntime rename, not a second identity store. Omit fields you are not changing.","inputSchema":{"type":"object","properties":{"title":{"type":"string","description":"Manual session title. Replaces the current title and blocks later automatic naming."},"emotion":{"type":"string","enum":["idle","thinking","working","success"],"description":"Mascot overlay used when the host is not running a turn."},"summary":{"type":"string","description":"At most seven whitespace-separated words. Rejected if longer; never truncated."}},"additionalProperties":false}}
    ]})
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    if name != "session_present" {
        return Err(format!("unknown tool {name}"));
    }
    let session_id = env::var("SYNTH_SESSION_ID")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "SYNTH_SESSION_ID is required".to_string())?;
    if args.get("title").is_none() && args.get("emotion").is_none() && args.get("summary").is_none()
    {
        return Err("session_present requires title, emotion, or summary".into());
    }
    let mut body = json!({ "sessionId": session_id });
    if let Some(title) = args.get("title") {
        body["title"] = title.clone();
    }
    if let Some(emotion) = args.get("emotion") {
        body["emotion"] = emotion.clone();
    }
    if let Some(summary) = args.get("summary") {
        body["summary"] = summary.clone();
    }
    request("POST", "/v1/sessions/present", Some(body))
}

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-session-mcp",
            version: env!("CARGO_PKG_VERSION"),
        },
        tools,
        call_tool,
    );
}

