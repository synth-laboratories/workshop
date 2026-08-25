//! Stdio MCP adapter for Synth Desktop's local container registry.

#[path = "../ipc/mcp_stdio.rs"]
mod mcp_stdio;

#[path = "../instance_paths.rs"]
mod instance_paths;

use mcp_stdio::{run_stdio_server, McpServerInfo};
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
        .ok_or_else(|| synth_desktop_lib::error::AppError::message("empty IPC response"))?;
    let status = parse_status_code(head)
        .ok_or_else(|| synth_desktop_lib::error::AppError::message("malformed IPC status line"))?;
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
        {"name":"container_discover","description":"Discover container sources from the desktop-level catalog. Sources are independent of chat workspaces. Select one returned source_id before starting a container; discovery never starts a process.","inputSchema":{"type":"object","properties":{"query":{"type":"string","description":"Optional id, family, or source hint."}},"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}},
        {"name":"container_ensure","description":"Start or attach one catalogued container spec, wait until /health succeeds, and return the registered handle. Call container_discover first and pass its source_id; no chat workspace is consulted. Does not scan ports. cwd must stay inside the catalogued source. v1 is a supervised child process, not Docker.","inputSchema":{"type":"object","properties":{"source_id":{"type":"string","description":"source id returned by container_discover"},"spec_id":{"type":"string","description":"id from the source's workshop.containers.toml"}},"required":["source_id","spec_id"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}},
        {"name":"container_get","description":"Get a container including cached health, hydrated /info metadata, and metadata.capabilities: the typed live-eval capability state. Health proves liveness only; read capabilities.operations before planning a prepared-rollout workflow.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"}},"required":["container_id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}},
        {"name":"container_probe","description":"Probe one registered container and refresh /health, /info, and the typed capability projection. Read-only against the container; never scans ports and never issues a rollout to discover support.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"}},"required":["container_id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
        ,{"name":"container_prepare_rollout","description":"Idempotently prepare one caller-stable rollout identity and return its declared stream descriptor. Fails locally before any request when the record is unhealthy (container_unhealthy), its capability observation is stale (container_capabilities_stale), or it does not advertise the prepared-rollout workflow or the requested policy_ref (container_capability_mismatch). Repeating the same rollout_id restores the same preparation; changed transport or retention conflicts.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"rollout_id":{"type":"string"},"task_instance_id":{"type":"string"},"seed":{"type":"integer"},"policy_ref":{"type":"object","properties":{"harness":{"type":"string"},"config":{"type":"string"},"code":{}},"additionalProperties":true},"require_trace_v5":{"type":"boolean","default":false,"description":"Set true when this workflow promises sealed Trace V5 evidence; preflight then also requires an explicitly advertised trace_v5.capture."},"telemetry":{"type":"object"}},"required":["container_id"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}
        ,{"name":"container_start_prepared_rollout","description":"Idempotently start the exact prepared rollout after stream.subscribed and a current visual.ready receipt. A reconnect replays the same immutable rollout identity; changed task or policy conflicts. The host does not pick luna_med.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"rollout_id":{"type":"string"},"stream":{"type":"object"},"visual_id":{"type":"string"},"seed":{"type":"integer"},"task_instance_id":{"type":"string"},"policy_ref":{"type":"object","properties":{"harness":{"type":"string"},"config":{"type":"string"},"code":{}},"required":["harness"],"additionalProperties":true},"telemetry":{"type":"object"}},"required":["container_id","rollout_id","stream","visual_id","policy_ref"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}
        ,{"name":"container_get_rollout","description":"Restore authoritative rollout lifecycle state after a timeout or reconnect without starting work.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"rollout_id":{"type":"string"}},"required":["container_id","rollout_id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
        ,{"name":"container_poll_rollout","description":"Resume the exact declared poll stream after a sequence cursor. Returns events plus authoritative high_water and closed cursor state; it never re-executes the rollout.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"rollout_id":{"type":"string"},"stream":{"type":"object"},"after":{"type":"integer","minimum":0}},"required":["container_id","rollout_id","stream"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
        ,{"name":"campaign_create","description":"Plan one evaluation campaign: a fixed number of rollouts with stable ids, non-overlapping seeds, and one policy. An evaluation is this, not a rollout. The plan is fixed before anything runs; every returned rollout must be started with its own rollout_id, seed, and task_instance_id.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"title":{"type":"string"},"expected_rollouts":{"type":"integer","minimum":1,"maximum":100,"description":"How many terminal rollouts this campaign owes. It cannot settle complete with fewer."},"seeds":{"type":"array","items":{"type":"integer"},"description":"Explicit seeds, one per rollout, all distinct. Omit to allocate a contiguous block from seed_start."},"seed_start":{"type":"integer","description":"First seed of a contiguous block. Seeds may not overlap another open campaign."},"task_instance_template":{"type":"string","description":"Task instance id per rollout; {seed} is substituted. Defaults to seed:{seed}."},"max_concurrency":{"type":"integer","minimum":1},"policy_ref":{"type":"object","properties":{"harness":{"type":"string"},"config":{"type":"string"},"code":{}},"required":["harness"],"additionalProperties":true}},"required":["container_id","expected_rollouts","policy_ref"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}}
        ,{"name":"campaign_status","description":"The campaign plan with each rollout's current state, reconciled against the container's authoritative records rather than any report of them.","inputSchema":{"type":"object","properties":{"campaign_id":{"type":"string"}},"required":["campaign_id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
        ,{"name":"campaign_result","description":"Reconcile, then settle: the campaign's own aggregate over its terminal rollouts — reward distribution, achievement rates, termination reasons, latency, calls, and usage coverage. Returns status complete only when every planned rollout has a terminal record; otherwise partial, naming the missing ones. Do not recompute this yourself.","inputSchema":{"type":"object","properties":{"campaign_id":{"type":"string"}},"required":["campaign_id"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
        ,{"name":"container_run_rollouts","description":"Scripted engine-acceptance only: 1-10 bounded rollouts with an explicit action list. Not a ReAct or model evaluation. Live policy evals use container_prepare_rollout then container_start_prepared_rollout with policy_ref.","inputSchema":{"type":"object","properties":{"container_id":{"type":"string"},"count":{"type":"integer","minimum":1,"maximum":10},"seeds":{"type":"array","items":{"type":"integer"},"maxItems":10},"actions":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":64}},"required":["container_id","count","actions"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":true}}
    ]});
    if let Some(facade) = catalog["tools"]
        .as_array_mut()
        .and_then(|items| items.iter_mut().find(|tool| tool["name"] == "container_manage"))
    {
        facade["inputSchema"]["properties"]["operation"]["enum"] = Value::Array(
            CONTAINER_OPERATIONS
                .iter()
                .map(|(operation, _)| Value::String((*operation).into()))
                .collect(),
        );
    }
    catalog
}

/// A campaign id is a path segment, so it may not smuggle one.
fn campaign_id(args: &Value) -> Result<String, String> {
    let id = args
        .get("campaign_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "campaign_id required".to_string())?;
    if !id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
    {
        return Err("campaign_id must be an identifier".to_string());
    }
    Ok(id.to_owned())
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
        "campaign_create" => request("POST", "/v1/campaigns", Some(args.clone())),
        "campaign_status" => request(
            "POST",
            &format!("/v1/campaigns/{}/reconcile", campaign_id(args)?),
            Some(json!({})),
        ),
        "campaign_result" => request(
            "POST",
            &format!("/v1/campaigns/{}/result", campaign_id(args)?),
            Some(json!({})),
        ),
        _ => Err(format!("unknown tool {name}")),
    }
}

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-containers-mcp",
            version: env!("CARGO_PKG_VERSION"),
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
        let prepare = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "container_prepare_rollout")
            .unwrap();
        let description = prepare["description"].as_str().unwrap();
        for code in [
            "container_unhealthy",
            "container_capabilities_stale",
            "container_capability_mismatch",
        ] {
            assert!(description.contains(code), "prepare must announce {code}");
        }
        assert!(description.contains("Fails locally before any request"));
        assert_eq!(
            prepare["inputSchema"]["properties"]["require_trace_v5"]["type"],
            "boolean"
        );
        assert!(catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "container_ensure"));
        let discover = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "container_discover")
            .expect("container source discovery must be agent-accessible");
        assert_eq!(discover["annotations"]["readOnlyHint"], true);
        let ensure = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "container_ensure")
            .unwrap();
        assert!(ensure["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "source_id"));
        assert!(ensure["inputSchema"]["properties"]
            .get("session_ref")
            .is_none());
        for name in ["container_list", "container_get", "container_probe"] {
            let tool = catalog["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["name"] == name)
                .unwrap();
            assert!(
                tool["description"].as_str().unwrap().contains("capabilit"),
                "{name} must surface the typed capability projection"
            );
        }
        // A campaign is the noun that makes "an evaluation" a count. Five chats
        // each read "one evaluation" as one rollout, and none of them was wrong.
        let campaign = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "campaign_create")
            .expect("the eval surface must offer a campaign primitive");
        assert_eq!(
            campaign["inputSchema"]["properties"]["expected_rollouts"]["type"],
            "integer"
        );
        assert!(campaign["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "expected_rollouts"));
        let result = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "campaign_result")
            .unwrap();
        let description = result["description"].as_str().unwrap();
        assert!(description.contains("partial"), "{description}");
        assert!(description.contains("Do not recompute"), "{description}");

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

    #[test]
    fn compact_facade_routes_catalogued_container_operations() {
        assert_eq!(managed_tool_name("discover").unwrap(), "container_discover");
        assert_eq!(managed_tool_name("ensure").unwrap(), "container_ensure");
        assert_eq!(managed_tool_name("probe").unwrap(), "container_probe");
        assert!(managed_tool_name("scan_ports").is_err());

        let catalog = tools();
        let facade = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "container_manage")
            .expect("Codex must receive the compact container facade");
        let operations = facade["inputSchema"]["properties"]["operation"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            operations,
            CONTAINER_OPERATIONS
                .iter()
                .map(|(operation, _)| *operation)
                .collect::<Vec<_>>()
        );
    }
}
