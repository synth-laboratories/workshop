//! Stdio MCP adapter for Trace V5 annotation jobs.
//!
//! Every operation proxies, by identity only, to the Desktop IPC surface
//! (`/v1/annotations/*`), which resolves the owning container from the trusted
//! registry and forwards to that container's annotation router. Paid operations
//! carry an opaque `reservation_id` issued by the host's approval broker; the
//! agent never supplies an authorization object, a cap, or a URL.
//!
//! The host side lives in `annotations_ipc.rs`; `OPERATIONS` there mirrors this
//! table and a test keeps them equal.

#![recursion_limit = "256"]

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
    let body = response
        .split("\r\n\r\n")
        .nth(1)
        .ok_or_else(|| "empty IPC response".to_string())?;
    serde_json::from_str(body).map_err(|error| error.to_string())
}

/// Operation catalog. Mirrors `synth_containers.tracing.annotation.operations.OPERATION_DESCRIPTORS`.
/// `paid` operations may start bounded paid compute; everything else is read-only or free.
const OPERATIONS: &[(&str, bool, bool, &str)] = &[
    ("annotation_list_definitions", true, false, "List annotators and rubrics compatible with a trace: immutable definition/program digests, taxonomy, runner class, and whether running one is paid."),
    ("annotation_estimate", true, false, "Idempotency key, cached-or-not, resolved model/effort, limits, and whether a broker reservation is needed. No compute."),
    ("annotation_start", false, true, "Enqueue one annotation job (202; poll annotation_get). Cached results return without compute. Paid annotators need a reservation_id from the host broker, bound to this trace/annotator/model/session."),
    ("annotation_get", true, false, "Job state, typed error, receipts, usage, sealed output ids."),
    ("annotation_events", true, false, "Poll the annotation job event log (sequence cursor). Events cover prepared → running → tool → validating → sealed/abstained/failed/cancelled. Hidden chain-of-thought is never included."),
    ("annotation_cancel", false, false, "Cancel a prepared or running job; sealed results are never removed."),
    ("annotation_list", true, false, "Current annotations on a trace from the sealed evidence head, with filters. Labels are diagnostics, never reward."),
    ("annotation_get_evidence", true, false, "Resolve an annotation's target and evidence selectors to the exact cited text."),
    ("verification_start", false, true, "Enqueue a rubric verification (VerifierResultV2 on completion; poll verification_get). Scores never change environment reward."),
    ("verification_get", true, false, "Verification job state and sealed verifier result id."),
    ("annotation_review", false, false, "Accept, reject, dispute, or flag an annotation: appends a superseding revision, never edits history."),
    ("annotation_consensus", false, false, "Inter-annotator agreement over repeats plus majority consensus records."),
    ("annotation_campaign", false, true, "Fan one plan (annotators x repeats) over a run's sealed traces; returns an estimate first when estimate_only is set."),
    ("annotation_protocol_get", true, false, "Installed live annotation protocol identity on a container (protocol_revision_id, digests, judge model); never source or credentials."),
    ("annotation_protocol_update", false, false, "Install a live annotation protocol revision (code + protocol_id + configuration) on a container; with run_id, advance that run's pin so its next rollouts use it; with rollout_ids, hot-swap rollouts running now (carry_state carries snapshot state). Findings stay provisional and never touch reward."),
    ("annotation_control_send", false, false, "Send one consumer -> annotator control to a running rollout: op message ({type: note|judge_now|set, ...}), protocol.update (protocol_revision_id, carry_state), or stop (reason). The durable acknowledgement lands on the rollout's annotation stream."),
    ("annotation_provisional_list", true, false, "Provisional live findings relayed for an eval run (optionally one rollout_id), with supersede/retract history and post-seal reconciliation (resolved | corroborated | unresolved | unsealed). Never sealed evidence."),
];

fn tools() -> Value {
    let operations: Vec<Value> = OPERATIONS
        .iter()
        .map(|(name, _, _, _)| json!(name))
        .collect();
    let descriptions: Vec<String> = OPERATIONS
        .iter()
        .map(|(name, read_only, paid, text)| {
            let flags = match (read_only, paid) {
                (true, _) => "read-only",
                (false, true) => "may start paid compute",
                (false, false) => "writes evidence",
            };
            format!("{name} [{flags}]: {text}")
        })
        .collect();
    json!({"tools":[
        {"name":"annotation_manage",
         "description": format!(
            "Trace V5 annotation and verification over sealed traces. Load the trace-v5-annotate / trace-v5-verify / annotation-review skills. Job, annotation, bundle, and execution-trace ids are immutable; cached results are returned when the idempotency key matches; qualitative scores are not reward; `verified` milestones require engine evidence. Operations: {}",
            descriptions.join(" | ")),
         "inputSchema":{"type":"object","properties":{
            "operation":{"type":"string","enum":operations},
            "arguments":{"type":"object","properties":{
                "trace_id":{"type":"string"},
                "job_id":{"type":"string"},
                "annotation_id":{"type":"string"},
                "annotator_id":{"type":"string"},
                "domain":{"type":"string"},
                "request":{"type":"object","description":"AnnotationJobRequestV1 (digests, model, effort, limits)"},
                "reservation_id":{"type":"string","description":"opaque id issued by the host approval broker; never a spending limit or a credential"},
                "session_id":{"type":"string"},
                "filters":{"type":"object"},
                "decision":{"type":"string","enum":["accepted","rejected","disputed","needs_review"]},
                "reviewer":{"type":"string"},
                "rationale":{"type":"string"},
                "evidence":{"type":"array"},
                "majority_threshold":{"type":"number"},
                "container_id":{"type":"string","description":"immutable id of the registered container that sealed the trace (from container_list); required on every operation"},
                "run_id":{"type":"string","description":"annotation_campaign: the eval/optimizer run whose sealed traces to annotate"},
                "traces":{"type":"array","description":"annotation_campaign: trace refs {kind: trace_v5, id, digest}"},
                "label":{"type":"string"},
                "annotators":{"type":"array"},
                "estimate_only":{"type":"boolean"},
                "after":{"type":"integer","description":"annotation_events: sequence cursor, default 0"},
                "limit":{"type":"integer","description":"annotation_events: page size, default 1000"},
                "rollout_id":{"type":"string","description":"annotation_control_send: the running rollout whose annotator receives the control"},
                "rollout_ids":{"type":"array","items":{"type":"string"},"description":"annotation_protocol_update: rollouts running now to hot-swap onto the new revision"},
                "protocol_id":{"type":"string","description":"annotation_protocol_update: the PROTOCOL_ID the file declares"},
                "protocol_revision_id":{"type":"string","description":"annotation_control_send protocol.update: an installed anprev_ revision"},
                "code":{"type":"string","description":"annotation_protocol_update: stdlib-only protocol source"},
                "configuration":{"type":"object","description":"annotation_protocol_update: protocol configuration; may carry a model block (model, base_url, api_key_env, max_calls); never a key"},
                "source_revision":{"type":"string"},
                "op":{"type":"string","enum":["message","protocol.update","stop"]},
                "message":{"type":"object","description":"annotation_control_send message: {type: note|judge_now|set, ...}"},
                "carry_state":{"type":"boolean"},
                "reason":{"type":"string"}
            },"additionalProperties":false}
         },"required":["operation"],"additionalProperties":false},
         "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}
    ]})
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    if name != "annotation_manage" {
        return Err(format!("unknown tool {name}"));
    }
    let operation = args
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| "operation required".to_string())?;
    let nested = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
    // Identity in, resolved address on the trusted side. Paths, URLs, caps, and
    // authorization objects are exactly what an agent-facing surface must refuse.
    if let Some(object) = nested.as_object() {
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "trace_id"
                    | "job_id"
                    | "annotation_id"
                    | "annotator_id"
                    | "domain"
                    | "request"
                    | "reservation_id"
                    | "session_id"
                    | "sessionRef"
                    | "filters"
                    | "decision"
                    | "reviewer"
                    | "rationale"
                    | "evidence"
                    | "majority_threshold"
                    | "run_id"
                    | "annotators"
                    | "estimate_only"
                    | "container_id"
                    | "traces"
                    | "label"
                    | "repeats"
                    | "after"
                    | "limit"
                    | "rollout_id"
                    | "rollout_ids"
                    | "protocol_id"
                    | "protocol_revision_id"
                    | "code"
                    | "configuration"
                    | "source_revision"
                    | "op"
                    | "message"
                    | "carry_state"
                    | "reason"
                    | "control_id"
            ) {
                return Err(format!("annotation arguments reject `{key}`"));
            }
        }
        if let Some(reservation) = object.get("reservation_id") {
            if !reservation.is_string() {
                return Err("reservation_id must be an opaque string".to_string());
            }
        }
    }
    if !OPERATIONS
        .iter()
        .any(|(known, _, _, _)| *known == operation)
    {
        return Err(format!("unknown annotation operation `{operation}`"));
    }
    request(
        "POST",
        &format!("/v1/annotations/{operation}"),
        Some(nested),
    )
}

fn main() {
    run_stdio_server(
        McpServerInfo {
            name: "synth-annotations-mcp",
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
    fn schema_offers_no_paths_urls_caps_or_authorization_objects() {
        let catalog = tools();
        let encoded = catalog["tools"][0]["inputSchema"].to_string();
        assert!(catalog.to_string().contains("annotation_manage"));
        for forbidden in [
            "\"path\"",
            "\"url\"",
            "max_cost",
            "authorization",
            "cap_usd",
        ] {
            assert!(!encoded.contains(forbidden), "{forbidden}");
        }
        assert!(!encoded.contains("additionalProperties\":true"));
    }

    #[test]
    fn catalog_names_every_operation_the_skills_use() {
        let ops = tools()["tools"][0]["inputSchema"]["properties"]["operation"]["enum"].clone();
        for required in [
            "annotation_list_definitions",
            "annotation_estimate",
            "annotation_start",
            "annotation_get",
            "annotation_cancel",
            "annotation_list",
            "annotation_get_evidence",
            "verification_start",
            "verification_get",
            "annotation_review",
            "annotation_consensus",
        ] {
            assert!(
                ops.as_array()
                    .unwrap()
                    .iter()
                    .any(|value| value == required),
                "{required}"
            );
        }
    }

    #[test]
    fn arguments_reject_authorization_objects_and_paths() {
        let err = call_tool(
            "annotation_manage",
            &json!({"operation":"annotation_start","arguments":{"request":{},"authorization":{"max_cost_usd":99}}}),
        )
        .unwrap_err();
        assert!(err.contains("reject"), "{err}");
        let err = call_tool(
            "annotation_manage",
            &json!({"operation":"annotation_start","arguments":{"request":{},"reservation_id":{"cap":1}}}),
        )
        .unwrap_err();
        assert!(err.contains("opaque string"), "{err}");
        let err = call_tool(
            "annotation_manage",
            &json!({"operation":"annotation_get","arguments":{"job_id":"j","path":"/etc/passwd"}}),
        )
        .unwrap_err();
        assert!(err.contains("reject"), "{err}");
    }

    #[test]
    fn unknown_operations_are_refused_before_any_request() {
        let err = call_tool(
            "annotation_manage",
            &json!({"operation":"delete_everything"}),
        )
        .unwrap_err();
        assert!(err.contains("unknown annotation operation"), "{err}");
    }
}
