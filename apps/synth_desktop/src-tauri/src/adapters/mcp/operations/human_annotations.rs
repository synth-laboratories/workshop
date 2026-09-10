//! Agent-facing human review workflow. This surface creates immutable tasks and
//! opens the native Workshop review panel; it never exposes private drafts or
//! microphone bytes to the agent.


use crate::instance::paths as instance_paths;
use crate::ipc::mcp_stdio;

use mcp_stdio::{run_stdio_server, McpServerInfo};
use serde_json::{json, Value};
use std::{
    env,
    path::PathBuf,
    time::{Duration, Instant},
};

fn connection_file() -> PathBuf {
    instance_paths::ipc_connection_file(
        &["SYNTH_DESKTOP_IPC_FILE", "SYNTH_VISUALS_IPC_FILE"],
        "visuals-ipc.json",
    )
}
fn request(path: &str, body: Value) -> Result<Value, String> {
    let mut payload = body;

    if let Some(object) = payload.as_object_mut() {
        if !object.contains_key("sessionRef") && !object.contains_key("session_id") {
            if let Ok(session) = env::var("SYNTH_SESSION_ID") {
                if !session.trim().is_empty() { object.insert("sessionRef".into(), json!(session)); }
            }
        }
    }
    crate::adapters::mcp::local_request(&connection_file(), "POST", path, payload)
}
pub(crate) fn tools() -> Value {
    json!({"tools":[{"name":"human_annotation_manage","description":"Preview and validate before immutable create; then show the native right-panel review, wait/get, and export. human_annotation_get accepts taskId or sessionId for private-safe status, or the copied resultId for the full sealed record and seal. Campaign operations expose agreement, adjudication, close, and append-only result supersession. Human drafts, quiz keys, and microphone bytes remain private until sealed submission.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","enum":["human_annotation_preview","human_annotation_create","human_annotation_show","human_annotation_get","human_annotation_wait","human_annotation_list","human_annotation_export","human_annotation_cancel","human_annotation_supersede","human_annotation_campaign_create","human_annotation_campaign_status","human_annotation_campaign_close","human_annotation_campaign_adjudicate"]},"arguments":{"type":"object"}},"required":["operation"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}]})
}
pub(crate) fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    super::check_agent_call(name, args).map_err(|error| error.to_string())?;
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
pub fn run() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-human-annotations",
            version: env!("CARGO_PKG_VERSION"),
        },
        tools,
        call_tool,
    )
}
