//! Stdio MCP adapter for the local Workshop secrets vault.
//!
//! The agent may list aliases, ask the host to import a `.env`, and request
//! bounded use. It never receives plaintext, and it cannot create, reveal,
//! export, or commit a credential.

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
            "description": "Local Workshop secrets vault. List registered provider aliases, ask the host to import a .env (masked preview only), or request bounded use. Never returns plaintext. The user approves imports and use in Settings → Secrets. Load the use-synth-secrets skill. Do not pass values, tokens, or API keys as arguments.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "operation": {"type": "string", "enum": ["list", "request_env_import", "request_use"]},
                    "provider": {"type": "string", "description": "Optional provider filter for list: openai, anthropic, openrouter."},
                    "scope": {"type": "string"},
                    "sourcePath": {"type": "string", "description": "Absolute path to a .env file. The host reads it; this tool result contains names and masked suffixes only."},
                    "variableNames": {"type": "array", "items": {"type": "string"}},
                    "secretId": {"type": "string"},
                    "runId": {"type": "string"},
                    "recipeId": {"type": "string"},
                    "workload": {"type": "string", "enum": ["chat_completions", "codex_responses"], "description": "Fixed provider-wire contract. The agent cannot set operations, models, cost, or lifetime."}
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
        "list" => request("POST", "/v1/secrets/list", Some(args.clone())),
        "request_env_import" => {
            if args
                .get("sourcePath")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                return Err("request_env_import requires sourcePath".into());
            }
            request("POST", "/v1/secrets/import", Some(args.clone()))
        }
        "request_use" => {
            if args
                .get("secretId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                return Err("request_use requires secretId from list".into());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_only_the_safe_operations() {
        let encoded = tools().to_string();
        assert!(encoded.contains("secrets_manage"));
        assert!(encoded.contains("request_env_import"));
        assert!(encoded.contains("request_use"));
        assert!(encoded.contains("codex_responses"));
        for forbidden in [
            "secrets_create",
            "secrets_get",
            "reveal",
            "export",
            "readValue",
            "commit",
            "\"value\"",
        ] {
            assert!(
                !encoded.contains(forbidden),
                "{forbidden} leaked into schema"
            );
        }
        assert!(encoded.contains("additionalProperties\":false"));
    }

    #[test]
    fn credential_arguments_are_refused_before_any_request() {
        for hostile in ["value", "apiKey", "token", "password", "reveal"] {
            let error = call_tool(
                "secrets_manage",
                &json!({"operation": "list", (hostile): "sk-secret"}),
            )
            .unwrap_err();
            assert!(error.contains("reject"), "{hostile}: {error}");
        }
    }

    #[test]
    fn unknown_operations_are_refused_before_any_request() {
        let error = call_tool("secrets_manage", &json!({"operation": "create"})).unwrap_err();
        assert!(error.contains("unknown secrets operation"), "{error}");
    }
}
