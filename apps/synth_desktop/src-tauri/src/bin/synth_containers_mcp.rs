//! Stdio MCP adapter for Synth Desktop's local container registry.

#[path = "../ipc/mcp_stdio.rs"]
mod mcp_stdio;

use mcp_stdio::{run_stdio_server, McpServerInfo};
use serde_json::{json, Value};
use std::{env, fs, io, io::Write, path::PathBuf};

#[derive(serde::Deserialize)]
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
    let wire = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", connection.token, payload.len());
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
            .ok_or_else(|| synth_desktop_lib::error::AppError::message("empty IPC response"))?,
    )
    .map_err(synth_desktop_lib::error::AppError::from)
}

fn tools() -> Value {
    json!({"tools":[
        {"name":"container_list","description":"List registered local containers with cached readiness and task family","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}},
        {"name":"container_register","description":"Register and hydrate one container URL explicitly supplied by the user or workspace. This does not scan or guess ports.","inputSchema":{"type":"object","properties":{"base_url":{"type":"string"},"name":{"type":"string"},"location":{"type":"string","default":"local"},"task_family":{"type":"string"},"metadata":{"type":"object"}},"required":["base_url"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}},
        {"name":"container_get","description":"Get a container including cached health and hydrated /info metadata","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"}},"required":["container_id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}},
        {"name":"container_probe","description":"Probe one registered container and refresh /health and /info; never scans ports","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"}},"required":["container_id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
        ,{"name":"container_prepare_rollout","description":"Idempotently prepare one caller-stable rollout identity and return its declared stream descriptor. Repeating the same rollout_id restores the same preparation; changed transport or retention conflicts.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"rollout_id":{"type":"string"},"task_instance_id":{"type":"string"},"seed":{"type":"integer"},"policy_ref":{"type":"object","properties":{"harness":{"type":"string"},"config":{"type":"string"},"code":{}},"additionalProperties":true},"telemetry":{"type":"object"}},"required":["container_id"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}
        ,{"name":"container_start_prepared_rollout","description":"Idempotently start the exact prepared rollout after stream.subscribed and a current visual.ready receipt. A reconnect replays the same immutable rollout identity; changed task or policy conflicts. The host does not pick luna_med.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"rollout_id":{"type":"string"},"stream":{"type":"object"},"visual_id":{"type":"string"},"seed":{"type":"integer"},"task_instance_id":{"type":"string"},"policy_ref":{"type":"object","properties":{"harness":{"type":"string"},"config":{"type":"string"},"code":{}},"required":["harness"],"additionalProperties":true},"telemetry":{"type":"object"}},"required":["container_id","rollout_id","stream","visual_id","policy_ref"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}
        ,{"name":"container_get_rollout","description":"Restore authoritative rollout lifecycle state after a timeout or reconnect without starting work.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"rollout_id":{"type":"string"}},"required":["container_id","rollout_id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
        ,{"name":"container_poll_rollout","description":"Resume the exact declared poll stream after a sequence cursor. Returns events plus authoritative high_water and closed cursor state; it never re-executes the rollout.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"rollout_id":{"type":"string"},"stream":{"type":"object"},"after":{"type":"integer","minimum":0}},"required":["container_id","rollout_id","stream"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
        ,{"name":"container_run_rollouts","description":"Scripted engine-acceptance only: 1-10 bounded rollouts with an explicit action list. Not a ReAct or model evaluation. Live policy evals use container_prepare_rollout then container_start_prepared_rollout with policy_ref.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"count":{"type":"integer","minimum":1,"maximum":10},"seeds":{"type":"array","items":{"type":"integer"},"maxItems":10},"actions":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":64}},"required":["container_id","count","actions"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":true}}
    ]})
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    let id = || {
        args.get("container_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "container_id required".to_string())
    };
    match name {
        "container_list" => request("GET", "/v1/containers", None),
        "container_register" => {
            let base_url = args
                .get("base_url")
                .and_then(Value::as_str)
                .ok_or_else(|| "base_url required".to_string())?;
            request(
                "POST",
                "/v1/containers",
                Some(json!({
                    "baseUrl": base_url,
                    "name": args.get("name"),
                    "location": args.get("location").cloned().unwrap_or(json!("local")),
                    "taskFamily": args.get("task_family"),
                    "metadata": args.get("metadata").cloned().unwrap_or(json!({}))
                })),
            )
        }
        "container_get" => request("GET", &format!("/v1/containers/{}", id()?), None),
        "container_probe" => request(
            "POST",
            &format!("/v1/containers/{}/probe", id()?),
            Some(json!({})),
        ),
        "container_prepare_rollout" => request(
            "POST",
            &format!("/v1/containers/{}/rollouts/prepare", id()?),
            Some(args.clone()),
        ),
        "container_start_prepared_rollout" => request(
            "POST",
            &format!("/v1/containers/{}/rollouts/start", id()?),
            Some(args.clone()),
        ),
        "container_get_rollout" => {
            let rollout_id = args
                .get("rollout_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "rollout_id required".to_string())?;
            request(
                "GET",
                &format!("/v1/containers/{}/rollouts/{rollout_id}", id()?),
                None,
            )
        }
        "container_poll_rollout" => request(
            "POST",
            &format!("/v1/containers/{}/rollouts/poll", id()?),
            Some(args.clone()),
        ),
        "container_run_rollouts" => request(
            "POST",
            &format!("/v1/containers/{}/rollouts", id()?),
            Some(args.clone()),
        ),
        _ => Err(format!("unknown tool {name}")),
    }
}

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-containers-mcp",
            version: "0.1.0",
        },
        tools,
        |name, args| call_tool(name, args),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_bounded_live_rollout_tool() {
        let catalog = tools();
        let rollout = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "container_run_rollouts")
            .unwrap();
        assert_eq!(rollout["inputSchema"]["properties"]["count"]["maximum"], 10);
        assert_eq!(
            rollout["inputSchema"]["properties"]["seeds"]["maxItems"],
            10
        );
        assert_eq!(
            rollout["inputSchema"]["properties"]["actions"]["maxItems"],
            64
        );
        assert_eq!(rollout["annotations"]["idempotentHint"], false);
        assert!(rollout["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "actions"));
        let start = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "container_start_prepared_rollout")
            .unwrap();
        assert!(start["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "policy_ref"));
        assert!(start["description"]
            .as_str()
            .unwrap()
            .contains("does not pick luna_med"));
        assert_eq!(start["annotations"]["idempotentHint"], true);
        for name in ["container_get_rollout", "container_poll_rollout"] {
            let tool = catalog["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["name"] == name)
                .unwrap();
            assert_eq!(tool["annotations"]["readOnlyHint"], true);
            assert_eq!(tool["annotations"]["idempotentHint"], true);
        }
    }
}
