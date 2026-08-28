//! Stdio MCP adapter for Synth Desktop's local container registry.

#[path = "../ipc/mcp_stdio.rs"]
mod mcp_stdio;

#[path = "../instance_paths.rs"]
mod instance_paths;

use mcp_stdio::{run_stdio_server_enriched, McpServerInfo};
use serde_json::{json, Value};
use std::{env, fs, io, io::Write, path::PathBuf};

#[derive(serde::Deserialize)]
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

/// A non-2xx IPC response is a **tool failure**, never a successful result that
/// happens to carry an `error` field: the shared stdio layer turns `Err` into
/// `isError: true` so the transcript renders the call as failed. Structured
/// application failures (`{"code": …}`) are passed through verbatim so the
/// agent receives the code, the missing capabilities, and the remediation.
fn request(method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    match request_inner(method, path, body) {
        Ok((status, value)) if (200..300).contains(&status) => Ok(value),
        Ok((status, value)) => Err(application_failure(status, &value)),
        Err(error) => Err(display_err(error)),
    }
}

fn application_failure(status: u16, body: &Value) -> String {
    if body.get("code").and_then(Value::as_str).is_some() {
        return serde_json::to_string(body).unwrap_or_else(|_| format!("IPC status {status}"));
    }
    body.get("error")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("IPC status {status}: {body}"))
}

fn request_inner(
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Result<(u16, Value), synth_desktop_lib::error::AppError> {
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
    let (head, payload) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| synth_desktop_lib::error::AppError::untyped("empty IPC response"))?;
    let status = parse_status_code(head)
        .ok_or_else(|| synth_desktop_lib::error::AppError::untyped("malformed IPC status line"))?;
    match serde_json::from_str::<Value>(payload) {
        Ok(value) => Ok((status, value)),
        // A non-2xx response with an unparseable body is still a failure; do
        // not let a decode error hide the status the host actually returned.
        Err(error) if (200..300).contains(&status) => {
            Err(synth_desktop_lib::error::AppError::from(error))
        }
        Err(_) => Ok((status, json!({"error": payload.trim()}))),
    }
}

fn parse_status_code(head: &str) -> Option<u16> {
    head.lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse::<u16>()
        .ok()
}

const CONTAINER_OPERATIONS: &[(&str, &str)] = &[
    ("list", "container_list"),
    ("discover", "container_discover"),
    ("request_project_source", "project_source_request"),
    ("ensure", "container_ensure"),
    ("get", "container_get"),
    ("probe", "container_probe"),
    ("prepare_rollout", "container_prepare_rollout"),
    ("start_prepared_rollout", "container_start_prepared_rollout"),
    ("get_rollout", "container_get_rollout"),
    ("poll_rollout", "container_poll_rollout"),
    ("create_campaign", "campaign_create"),
    ("campaign_status", "campaign_status"),
    ("campaign_result", "campaign_result"),
    ("run_rollouts", "container_run_rollouts"),
];

fn managed_tool_name(operation: &str) -> Result<&'static str, String> {
    CONTAINER_OPERATIONS
        .iter()
        .find_map(|(candidate, tool)| (*candidate == operation).then_some(*tool))
        .ok_or_else(|| {
            let supported = CONTAINER_OPERATIONS
                .iter()
                .map(|(candidate, _)| *candidate)
                .collect::<Vec<_>>()
                .join(", ");
            format!("unknown container operation {operation}; supported operations: {supported}")
        })
}

fn tools() -> Value {
    let mut catalog = json!({"tools":[
        {"name":"container_manage","description":"Synth container registry and rollout workflows. Use operation discover to find desktop-catalogued sources independent of chat workspaces; then ensure, probe, and get a live service. Do not scan ports, use a shell, or invent a source. Use create_campaign and the prepared-rollout operations only for declared live evaluation workflows.","inputSchema":{"type":"object","properties":{"operation":{"type":"string","description":"Container registry or rollout operation."},"arguments":{"type":"object","description":"Operation arguments.","additionalProperties":true}},"required":["operation","arguments"],"additionalProperties":false}},
        {"name":"container_list","description":"List registered local containers with cached readiness, task family, and the typed live-eval capability projection (operations, advertised policy_refs, capability source, observation time)","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}},
        {"name":"container_ensure","description":"Start or attach the container spec declared in workshop.containers.toml. Relative launch paths resolve against that declaring repository, not the chat or instance workspace. Waits until /health succeeds and returns the registered handle. Does not scan ports. v1 is a supervised child process, not Docker.","inputSchema":{"type":"object","properties":{"spec_id":{"type":"string","description":"id from workshop.containers.toml"},"session_ref":{"type":"string","description":"Optional. Defaults to the calling session."}},"required":["spec_id"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}},
        {"name":"container_get","description":"Get a container including cached health, hydrated /info metadata, and metadata.capabilities: the typed live-eval capability state. Health proves liveness only; read capabilities.operations before planning a prepared-rollout workflow.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"}},"required":["container_id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}},
        {"name":"container_probe","description":"Probe one registered container and refresh /health, /info, and the typed capability projection. Read-only against the container; never scans ports and never issues a rollout to discover support.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"}},"required":["container_id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
        ,{"name":"container_stop","description":"Stop one Workshop-owned supervised container by its registered identity. Verifies the durable PID start identity before signaling and refuses external or stale processes.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"}},"required":["container_id"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":true,"openWorldHint":false}}
        ,{"name":"container_restart","description":"Request Workshop's native clickable approval modal, wait for the operator, then force replacement by re-running the exact versioned launch declaration. Call this tool when replacement is needed; never describe the approval gate in prose or ask the user to type approval. If Workshop has a valid supervised-process receipt it stops that process first; otherwise the declared command is responsible for replacing its named workload. Never discovers or kills a process from a port.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"session_ref":{"type":"string","description":"Optional. Defaults to the calling session."}},"required":["container_id"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":true}}
        ,{"name":"container_reconcile","description":"Re-read the declaring workshop.containers.toml relative to that repository, validate launch paths, and refresh the registry declaration without stopping or starting the workload. Use this to repair stale metadata before requesting replacement.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"session_ref":{"type":"string","description":"Optional. Defaults to the calling session."}},"required":["container_id"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
        ,{"name":"container_prepare_rollout","description":"Idempotently prepare one caller-stable rollout identity and return its declared stream descriptor. Fails locally before any request when the record is unhealthy (container_unhealthy), its capability observation is stale (container_capabilities_stale), or it does not advertise the prepared-rollout workflow or the requested policy_ref (container_capability_mismatch). Repeating the same rollout_id restores the same preparation; changed transport, retention, or max_steps conflicts.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"rollout_id":{"type":"string"},"task_instance_id":{"type":"string"},"seed":{"type":"integer"},"max_steps":{"type":"integer","minimum":1,"description":"Immutable environment-step cap enforced by the container runtime."},"policy_ref":{"type":"object","properties":{"harness":{"type":"string"},"config":{"type":"string"},"code":{}},"additionalProperties":true},"require_trace_v5":{"type":"boolean","default":false,"description":"Set true when this workflow promises sealed Trace V5 evidence; preflight then also requires an explicitly advertised trace_v5.capture."},"telemetry":{"type":"object"}},"required":["container_id"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}
        ,{"name":"container_start_prepared_rollout","description":"Idempotently start the exact prepared rollout after stream.subscribed and a current draft visual subscription. A reconnect replays the same immutable rollout identity; changed task, policy, or the max_steps pin embedded by prepare conflicts. The host does not pick a policy.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"rollout_id":{"type":"string"},"stream":{"type":"object","description":"Pass the exact descriptor returned by prepare; it carries immutable execution pins including max_steps."},"visual_id":{"type":"string"},"seed":{"type":"integer"},"task_instance_id":{"type":"string"},"policy_ref":{"type":"object","properties":{"harness":{"type":"string"},"config":{"type":"string"},"code":{}},"required":["harness"],"additionalProperties":true},"telemetry":{"type":"object"}},"required":["container_id","rollout_id","stream","visual_id","policy_ref"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}
        ,{"name":"container_get_rollout","description":"Restore authoritative rollout lifecycle state after a timeout or reconnect without starting work.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"rollout_id":{"type":"string"}},"required":["container_id","rollout_id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
        ,{"name":"container_poll_rollout","description":"Resume the exact declared poll stream after a sequence cursor. Returns events plus authoritative high_water and closed cursor state; it never re-executes the rollout.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"rollout_id":{"type":"string"},"stream":{"type":"object"},"after":{"type":"integer","minimum":0}},"required":["container_id","rollout_id","stream"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
        ,{"name":"container_run_rollouts","description":"Scripted engine-acceptance only: 1-10 bounded rollouts with an explicit action list. Not a ReAct or model evaluation. Live policy evals use container_prepare_rollout then container_start_prepared_rollout with policy_ref.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"count":{"type":"integer","minimum":1,"maximum":10},"seeds":{"type":"array","items":{"type":"integer"},"maxItems":10},"actions":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":64}},"required":["container_id","count","actions"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":true}}
    ]});
    if let Some(facade) = catalog["tools"].as_array_mut().and_then(|items| {
        items
            .iter_mut()
            .find(|tool| tool["name"] == "container_manage")
    }) {
        facade["inputSchema"]["properties"]["operation"]["enum"] = Value::Array(
            CONTAINER_OPERATIONS
                .iter()
                .map(|(operation, _)| Value::String((*operation).into()))
                .collect(),
        );
    }
    catalog
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    let id = || {
        args.get("container_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "container_id required".to_string())
    };
    match name {
        "container_manage" => {
            let operation = args
                .get("operation")
                .and_then(Value::as_str)
                .ok_or("operation required")?;
            let arguments = args
                .get("arguments")
                .filter(|value| value.is_object())
                .ok_or("arguments must be an object")?;
            call_tool(managed_tool_name(operation)?, arguments)
        }
        "container_list" => request("GET", "/v1/containers", None),
        "container_discover" => request(
            "POST",
            "/v1/container-sources/discover",
            Some(json!({"query": args.get("query")})),
        ),
        "project_source_request" => {
            let path = args
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| "path required".to_string())?;
            let reason = args
                .get("reason")
                .and_then(Value::as_str)
                .ok_or_else(|| "reason required".to_string())?;
            // Only this process knows which chat is asking, and the approval
            // card names the conversation. An agent cannot nominate another.
            let session_id = env::var("SYNTH_SESSION_ID")
                .ok()
                .filter(|value| !value.trim().is_empty());
            request(
                "POST",
                "/v1/project-sources/request",
                Some(json!({
                    "path": path,
                    "reason": reason,
                    "capabilities": args.get("capabilities"),
                    "attachToConversation": args.get("attachToConversation"),
                    "sessionId": session_id,
                })),
            )
        }
        "container_ensure" => {
            let source_id = args
                .get("source_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "source_id required; call container_discover first".to_string())?;
            let spec_id = args
                .get("spec_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "spec_id required".to_string())?;
            request(
                "POST",
                "/v1/containers/ensure",
                Some(json!({ "sourceId": source_id, "specId": spec_id })),
            )
        }
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
        "container_stop" => request(
            "POST",
            &format!("/v1/containers/{}/stop", id()?),
            Some(json!({})),
        ),
        "container_restart" => {
            let mut payload = json!({});
            if let Some(session) = args
                .get("session_ref")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    std::env::var("SYNTH_SESSION_ID")
                        .ok()
                        .filter(|value| !value.trim().is_empty())
                })
            {
                payload["sessionRef"] = json!(session);
            }
            request(
                "POST",
                &format!("/v1/containers/{}/restart", id()?),
                Some(payload),
            )
        }
        "container_reconcile" => {
            let mut payload = json!({});
            if let Some(session) = args
                .get("session_ref")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    std::env::var("SYNTH_SESSION_ID")
                        .ok()
                        .filter(|value| !value.trim().is_empty())
                })
            {
                payload["sessionRef"] = json!(session);
            }
            request(
                "POST",
                &format!("/v1/containers/{}/reconcile", id()?),
                Some(payload),
            )
        }
        "container_prepare_rollout" => request(
            "POST",
            &format!("/v1/containers/{}/rollouts/prepare", id()?),
            Some(args.clone()),
        ),
        "container_start_prepared_rollout" => {
            // The host opens a durable receipt for this launch so a crash
            // cannot lead to a second paid rollout. That receipt has to be
            // attributable, and only this process knows which chat it serves.
            let mut args = args.clone();
            if let (Some(object), Ok(session_id)) =
                (args.as_object_mut(), env::var("SYNTH_SESSION_ID"))
            {
                if !session_id.trim().is_empty() {
                    object.insert("sessionRef".into(), json!(session_id));
                }
            }
            request(
                "POST",
                &format!("/v1/containers/{}/rollouts/start", id()?),
                Some(args),
            )
        }
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

fn enrich_container_breaker_args(name: &str, args: &Value) -> Value {
    if !matches!(
        name,
        "container_ensure" | "container_restart" | "container_reconcile"
    ) {
        return args.clone();
    }
    let mut body = json!({});
    if let Some(session) = args
        .get("session_ref")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            env::var("SYNTH_SESSION_ID")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
    {
        body["sessionRef"] = json!(session);
    }
    if let Some(container_id) = args.get("container_id").and_then(Value::as_str) {
        body["containerId"] = json!(container_id);
    }
    if let Some(spec_id) = args.get("spec_id").and_then(Value::as_str) {
        body["specId"] = json!(spec_id);
    }
    let resolved = match request("POST", "/v1/containers/resolve_declaration", Some(body)) {
        Ok(value) => value,
        Err(error) => serde_json::from_str(&error).unwrap_or_else(|_| json!({})),
    };
    let mut enriched = args.clone();
    let Some(object) = enriched.as_object_mut() else {
        return args.clone();
    };
    for (from, to) in [
        ("sourceRoot", "source_root"),
        ("manifestPath", "manifest_path"),
        ("declarationDigest", "declaration_digest"),
        ("source_root", "source_root"),
        ("manifest", "manifest_path"),
        ("source_digest", "declaration_digest"),
        ("specId", "spec_id"),
    ] {
        if let Some(value) = resolved
            .get(from)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            object.entry(to.to_string()).or_insert_with(|| json!(value));
        }
    }
    enriched
}

fn main() {
    run_stdio_server_enriched(
        McpServerInfo {
            name: "synth-containers-mcp",
            version: env!("CARGO_PKG_VERSION"),
        },
        tools,
        |name, args| call_tool(name, args),
        enrich_container_breaker_args,
    );
}

