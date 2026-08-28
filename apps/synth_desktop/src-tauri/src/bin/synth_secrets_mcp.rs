//! Stdio MCP adapter for the local Workshop secrets vault.
//!
//! The agent may list opaque workspace roots, remember/register relative
//! credential locations, and request bounded use. It never receives plaintext,
//! canonical paths, or masked suffixes.

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

const FORBIDDEN_ARGUMENTS: &[&str] = &[
    "value",
    "secret",
    "apiKey",
    "api_key",
    "token",
    "password",
    "credential",
    "get",
    "reveal",
    "export",
    "readValue",
    "read_value",
];

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
    serde_json::from_str(
        response
            .split("\r\n\r\n")
            .nth(1)
            .ok_or_else(|| "empty IPC response".to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn tools() -> Value {
    json!({"tools":[
        {
            "name": "secrets_manage",
            "description": "Workshop credential locator registry. List approved workspace root references, bindings, and remembered locations; request registration or bounded use; and revoke run capabilities without unregistering their reusable source. Never returns plaintext, canonical paths, or masked suffixes. Native approvals block until the operator decides. Load the use-synth-secrets skill. Do not pass values, tokens, API keys, or absolute paths.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "operation": {"type": "string", "enum": ["workspace_roots_list", "bindings_list", "locators_list", "locator_request", "locator_status", "locator_remove", "source_request", "source_status", "source_remove", "request_use", "use_revoke", "list", "request_env_import"]},
                    "provider": {"type": "string", "description": "Provider id such as openai, anthropic, or openrouter."},
                    "scope": {"type": "string"},
                    "workspaceRootRef": {"type": "string", "description": "Opaque reference returned by workspace_roots_list."},
                    "relativePath": {"type": "string", "maxLength": 256, "description": "Path relative to workspaceRootRef, commonly .env. Absolute paths and .. are refused."},
                    "locatorId": {"type": "string"},
                    "sourceId": {"type": "string"},
                    "secretId": {"type": "string"},
                    "variable": {"type": "string", "description": "Exact environment variable name, such as OPENROUTER_API_KEY."},
                    "label": {"type": "string"},
                    "runId": {"type": "string"},
                    "recipeId": {"type": "string"},
                    "capabilityId": {"type": "string"},
                    "workload": {"type": "string", "enum": ["chat_completions", "codex_responses"], "description": "Fixed provider-wire contract. Inline evaluations issue their capability directly from the approved execution envelope; this session-use operation cannot widen it."}
                },
                "required": ["operation"],
                "additionalProperties": false
            }
        }
    ]})
}

fn reject_forbidden(args: &Value) -> Result<(), String> {
    let Some(object) = args.as_object() else {
        return Ok(());
    };
    for key in object.keys() {
        if FORBIDDEN_ARGUMENTS
            .iter()
            .any(|forbidden| forbidden.eq_ignore_ascii_case(key))
        {
            return Err(format!(
                "rejecting `{key}`: secrets MCP never accepts credential values"
            ));
        }
    }
    Ok(())
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    if name != "secrets_manage" {
        return Err(format!("unknown tool {name}"));
    }
    reject_forbidden(args)?;
    let operation = args
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    match operation {
        "workspace_roots_list" => {
            request("POST", "/v1/secrets/workspace_roots", Some(args.clone()))
        }
        "bindings_list" => request("POST", "/v1/secrets/bindings", Some(args.clone())),
        "locators_list" => request("POST", "/v1/secrets/locators", Some(args.clone())),
        "locator_request" => request("POST", "/v1/secrets/locator_request", Some(args.clone())),
        "locator_status" => request("POST", "/v1/secrets/locator_status", Some(args.clone())),
        "locator_remove" => request("POST", "/v1/secrets/locator_remove", Some(args.clone())),
        "source_request" => request("POST", "/v1/secrets/source_request", Some(args.clone())),
        "source_status" => request("POST", "/v1/secrets/source_status", Some(args.clone())),
        "source_remove" => request("POST", "/v1/secrets/source_remove", Some(args.clone())),
        "use_revoke" => {
            let targets = ["capabilityId", "runId"]
                .into_iter()
                .filter(|key| {
                    args.get(*key)
                        .and_then(Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty())
                })
                .count();
            if targets != 1 {
                return Err("use_revoke requires exactly one capabilityId or runId".into());
            }
            request("POST", "/v1/secrets/use_revoke", Some(args.clone()))
        }
        "list" => request("POST", "/v1/secrets/list", Some(args.clone())),
        "request_env_import" => request("POST", "/v1/secrets/import", Some(args.clone())),
        "request_use" => {
            let targets = ["locatorId", "sourceId", "secretId"]
                .into_iter()
                .filter(|key| {
                    args.get(*key)
                        .and_then(Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty())
                })
                .count();
            if targets != 1 {
                return Err(
                    "request_use requires exactly one locatorId, sourceId, or secretId".into(),
                );
            }
            request("POST", "/v1/secrets/use", Some(args.clone()))
        }
        other => Err(format!("unknown secrets operation `{other}`")),
    }
}

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-secrets-mcp",
            version: env!("CARGO_PKG_VERSION"),
        },
        tools,
        call_tool,
    );
}

