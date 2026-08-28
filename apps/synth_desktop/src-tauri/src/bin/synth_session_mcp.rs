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
        {"name":"session_present","description":"Set this conversation's title, mascot emotion, and a ≤7-word summary. Load the use-synth-session skill. Title is a manual CoreRuntime rename, not a second identity store. Omit fields you are not changing.","inputSchema":{"type":"object","properties":{"title":{"type":"string","description":"Manual session title. Replaces the current title and blocks later automatic naming."},"emotion":{"type":"string","enum":["idle","thinking","working","success"],"description":"Mascot overlay used when the host is not running a turn."},"summary":{"type":"string","description":"At most seven whitespace-separated words. Rejected if longer; never truncated."}},"additionalProperties":false}},
        {"name":"approvals_list","description":"List approvals still awaiting a decision. Read-only, and discloses exactly what the modal would. Use it when a mutation appears to hang: a request raised against a conversation nobody has open is otherwise invisible until it times out. `requiresHuman` says whether you may settle it yourself.","inputSchema":{"type":"object","properties":{"allSessions":{"type":"boolean","description":"List every session's pending approvals instead of only this conversation's."}},"additionalProperties":false}},
        {"name":"approval_resolve","description":"Settle one pending approval. Spending and credential consent (`requiresHuman: true`) stay with a person by default and are refused unless the operator launched Workshop with SYNTH_DESKTOP_ALLOW_AGENT_HUMAN_APPROVALS=1; the receipt records that a non-human settled it. Never approve your own spending to get unblocked — report it and let the operator decide.","inputSchema":{"type":"object","required":["approvalId","decision"],"properties":{"approvalId":{"type":"string","description":"Identifier from approvals_list."},"decision":{"type":"string","enum":["once","always","always-this-workspace","reject"],"description":"`once` consents to this request alone. Remembered scopes are refused for approvals that require a person."}},"additionalProperties":false}}
    ]})
}

fn session_id() -> Result<String, String> {
    env::var("SYNTH_SESSION_ID")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "SYNTH_SESSION_ID is required".to_string())
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "session_present" => {}
        "approvals_list" => {
            let mut body = json!({});
            if args.get("allSessions").and_then(Value::as_bool) != Some(true) {
                body["sessionId"] = json!(session_id()?);
            }
            return request("POST", "/v1/sessions/approvals/list", Some(body));
        }
        "approval_resolve" => {
            let approval_id = args
                .get("approvalId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "approvalId is required".to_string())?;
            let decision = args
                .get("decision")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "decision is required".to_string())?;
            return request(
                "POST",
                "/v1/sessions/approvals/resolve",
                Some(json!({
                    "sessionId": session_id()?,
                    "approvalId": approval_id,
                    "decision": decision,
                })),
            );
        }
        _ => return Err(format!("unknown tool {name}")),
    }
    let session_id = session_id()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_presentation_and_approval_tools() {
        let catalog = tools();
        let listed = catalog["tools"].as_array().unwrap();
        let names = listed
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, ["session_present", "approvals_list", "approval_resolve"]);
        let encoded = catalog.to_string();
        assert!(encoded.contains("idle"));
        assert!(encoded.contains("thinking"));
        assert!(encoded.contains("working"));
        assert!(encoded.contains("success"));
        assert!(encoded.contains("seven"));
        assert!(!encoded.contains("session_id"));
    }

    /// The session id is ambient. A tool that accepted one would let an agent
    /// read or settle another conversation's approvals.
    #[test]
    fn approval_tools_never_accept_a_session_id() {
        let catalog = tools();
        for tool in catalog["tools"].as_array().unwrap() {
            let properties = &tool["inputSchema"]["properties"];
            assert!(properties.get("sessionId").is_none(), "{}", tool["name"]);
            assert_eq!(tool["inputSchema"]["additionalProperties"], json!(false));
        }
    }

    /// Spending consent is the one gate that cannot be described as routine.
    /// The description has to say so, because the model reads it before it
    /// reaches for the tool.
    #[test]
    fn resolve_discloses_that_spending_stays_human() {
        let catalog = tools();
        let resolve = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "approval_resolve")
            .unwrap();
        let description = resolve["description"].as_str().unwrap();
        assert!(description.contains("requiresHuman"));
        assert!(description.contains("SYNTH_DESKTOP_ALLOW_AGENT_HUMAN_APPROVALS"));
        assert!(description.contains("Never approve your own spending"));
        let decisions = resolve["inputSchema"]["properties"]["decision"]["enum"]
            .as_array()
            .unwrap();
        assert!(decisions.contains(&json!("reject")));
        assert!(decisions.contains(&json!("once")));
    }
}
