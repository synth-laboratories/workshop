//! Stdio MCP adapter for session presentation. Forwards through Desktop visuals IPC.
//!
//! Usage (Codex home config):
//!   command = "synth-session-mcp"
//!   env SYNTH_DESKTOP_IPC_FILE / SYNTH_SESSION_ID

use crate::ipc::mcp_stdio;

use crate::instance::paths as instance_paths;

use mcp_stdio::{run_stdio_server, McpServerInfo};
use serde_json::{json, Value};
use std::{env, path::PathBuf};


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
    let payload = body.unwrap_or_else(|| json!({}));
    crate::adapters::mcp::local_request(&connection_file(), method, path, payload)
}



pub(crate) fn tools() -> Value {
    json!({"tools":[
        {"name":"session_present","description":"Set this conversation's title, mascot emotion, and a ≤7-word summary. Load the use-synth-session skill. Title is a manual CoreRuntime rename, not a second identity store. Omit fields you are not changing.","inputSchema":{"type":"object","properties":{"title":{"type":"string","description":"Manual session title. Replaces the current title and blocks later automatic naming."},"emotion":{"type":"string","enum":["idle","thinking","working","success"],"description":"Mascot overlay used when the host is not running a turn."},"summary":{"type":"string","description":"At most seven whitespace-separated words. Rejected if longer; never truncated."}},"additionalProperties":false}}
    ]})
}

pub(crate) fn public_tools() -> Value {
    let mut catalogue = tools();
    let tool = &mut catalogue["tools"][0];
    tool["inputSchema"]["properties"]["session_id"] = json!({"type":"string","description":"Existing Workshop task to present."});
    tool["inputSchema"]["required"] = json!(["session_id"]);
    catalogue
}
pub(crate) fn call_public(name: &str, args: &Value) -> Result<Value, String> {
    call_scoped(name, args, args.get("session_id").and_then(Value::as_str).map(str::to_owned))
}
pub(crate) fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    super::check_agent_call(name, args).map_err(|error| error.to_string())?;
    call_scoped(name, args, env::var("SYNTH_SESSION_ID").ok())
}
fn call_scoped(name: &str, args: &Value, session: Option<String>) -> Result<Value, String> {
    if name != "session_present" {
        return Err(format!("unknown tool {name}"));
    }
    let session_id = session
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "an existing Workshop session_id is required".to_string())?;
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

pub fn run() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-session-mcp",
            version: env!("CARGO_PKG_VERSION"),
        },
        tools,
        call_tool,
    );
}

