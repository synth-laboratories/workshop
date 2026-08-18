//! Shared MCP stdio JSON-RPC line transport for the Desktop MCP bins.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

#[derive(Clone, Debug)]
pub struct McpServerInfo {
    pub name: &'static str,
    pub version: &'static str,
}

const BREAKER_FAILURE_LIMIT: usize = 2;

/// Identity of the thing a call is failing against, not just the tool it went
/// through. A tool like `visual_manage` carries a whole operation family and
/// every visual in the instance: keying the breaker on the tool name alone let
/// two deterministic capture failures on one visual disable `bind`, `show`, and
/// `render` for every other visual, including the recovery paths.
///
/// The key is tool + operation + target id, and the failure signature prefers a
/// structured error `code`, so a different root cause on the same target also
/// gets a fresh recovery budget.
const BREAKER_TARGET_FIELDS: [&str; 7] = [
    "visual_id",
    "visualId",
    "trace_id",
    "traceId",
    "container_id",
    "report_id",
    "id",
];

const BREAKER_SLOT_FIELDS: [&str; 3] = ["slot", "binding_slot", "bindingSlot"];
const BREAKER_CAPABILITY_REVISION_FIELDS: [&str; 4] = [
    "capability_revision",
    "capabilityRevision",
    "capability_digest",
    "capabilityDigest",
];

fn arg_str<'a>(args: &'a Value, field: &str) -> Option<&'a str> {
    args.get(field)
        .and_then(Value::as_str)
        .or_else(|| {
            args.pointer(&format!("/arguments/{field}"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn breaker_key(tool: &str, args: &Value) -> String {
    let mut key = tool.to_string();
    if let Some(operation) = args.get("operation").and_then(Value::as_str) {
        key.push('/');
        key.push_str(operation);
    }
    for field in BREAKER_TARGET_FIELDS {
        if let Some(target) = arg_str(args, field) {
            key.push('#');
            key.push_str(target);
            break;
        }
    }
    if let Some(slot) = BREAKER_SLOT_FIELDS.iter().find_map(|field| arg_str(args, field)) {
        key.push('/');
        key.push_str(slot);
    }
    if let Some(revision) = args
        .get("revision")
        .or_else(|| args.pointer("/arguments/revision"))
        .and_then(Value::as_i64)
    {
        key.push('@');
        key.push_str(&revision.to_string());
    }
    // Capability revision is an observation identity, not a visual revision.
    // Stripping it collapsed probe recovery into the same prepare breaker.
    if let Some(revision) = BREAKER_CAPABILITY_REVISION_FIELDS
        .iter()
        .find_map(|field| arg_str(args, field))
    {
        key.push_str("~cap:");
        key.push_str(revision);
    }
    if let Some(token) = breaker_payload_token(tool, args) {
        key.push_str("~req:");
        key.push_str(&token);
    }
    key
}

/// Bind and prepare keys include the request that failed, so a corrected
/// payload is a new recovery attempt. Identical retries still share a key.
fn breaker_payload_token(tool: &str, args: &Value) -> Option<String> {
    let operation = args.get("operation").and_then(Value::as_str).unwrap_or("");
    let bind_like = tool.contains("bind")
        || matches!(operation, "bind" | "create" | "create_with_bind" | "update");
    let prepare_like = tool.contains("prepare_rollout");
    if !bind_like && !prepare_like {
        return None;
    }
    let mut material = String::new();
    for field in [
        "kind",
        "source",
        "schema",
        "path",
        "template_id",
        "templateId",
        "policy_ref",
        "policyRef",
    ] {
        if let Some(value) = args.get(field).or_else(|| args.pointer(&format!("/arguments/{field}"))) {
            if !value.is_null() {
                material.push_str(field);
                material.push(':');
                material.push_str(&value.to_string());
            }
        }
    }
    let has_data = args.get("data").is_some()
        || args.pointer("/arguments/data").is_some()
        || args
            .pointer("/bindings")
            .and_then(Value::as_object)
            .is_some_and(|object| object.contains_key("data"));
    if has_data {
        material.push_str("data:1");
    }
    if material.is_empty() {
        return None;
    }
    Some(format!("{:x}", simple_token(&material)))
}

fn simple_token(material: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in material.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

fn breaker_target(args: &Value) -> Option<&str> {
    BREAKER_TARGET_FIELDS.iter().find_map(|field| {
        args.get(*field)
            .and_then(Value::as_str)
            .or_else(|| {
                args.pointer(&format!("/arguments/{field}"))
                    .and_then(Value::as_str)
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

fn visual_recovery_operation(tool: &str, args: &Value) -> bool {
    if tool != "visual_manage" {
        return false;
    }
    matches!(
        args.get("operation").and_then(Value::as_str),
        Some("capture_review" | "review" | "update" | "bind" | "create_with_bind")
    )
}

fn container_recovery_operation(tool: &str) -> bool {
    matches!(
        tool,
        "container_probe" | "container_get" | "container_list"
    )
}

/// A structured `{"code": …}` failure is the same failure however its message
/// is worded, and a different code is a different failure however similar the
/// prose. Capability revision/digest stays in the signature so a successful
/// probe that advances the observation is not the same stale failure.
/// Fall back to digit-normalized text only for unstructured errors.
fn failure_signature(error: &str) -> String {
    serde_json::from_str::<Value>(error)
        .ok()
        .and_then(|value| {
            let code = value.get("code").and_then(Value::as_str)?;
            let mut signature = format!("code:{code}");
            if let Some(revision) = value
                .get("capability_revision")
                .or_else(|| value.get("capabilityRevision"))
                .or_else(|| value.get("observed_at"))
                .or_else(|| value.get("observedAt"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                signature.push('@');
                signature.push_str(revision);
            }
            Some(signature)
        })
        .unwrap_or_else(|| normalize_error(error))
}

#[derive(Default)]
struct ToolLoopBreaker {
    failures: HashMap<String, (String, usize)>,
}

impl ToolLoopBreaker {
    fn terminal_error(&self, tool: &str, args: &Value) -> Option<String> {
        let key = breaker_key(tool, args);
        let (error, count) = self.failures.get(&key)?;
        (*count >= BREAKER_FAILURE_LIMIT).then(|| {
            format!(
                "ToolLoopBreaker: {key} stopped before invocation after {count} repeated failures: {error}. Change approach or fix the reported cause before retrying. Other operations and other targets are unaffected."
            )
        })
    }

    fn record_failure(&mut self, tool: &str, args: &Value, error: &str) {
        let key = breaker_key(tool, args);
        let signature = failure_signature(error);
        match self.failures.get_mut(&key) {
            Some((previous, count)) if previous == &signature => *count += 1,
            _ => {
                self.failures.insert(key, (signature, 1));
            }
        }
    }

    fn record_success(&mut self, tool: &str, args: &Value) {
        self.failures.remove(&breaker_key(tool, args));
        // A successful review/capture/update changes the evidence that
        // `mark_ready` evaluates. Keeping an earlier readiness failure armed
        // after that recovery made the breaker replay stale prose before the
        // readiness service could inspect the corrected reviews.
        if visual_recovery_operation(tool, args) {
            if let Some(target) = breaker_target(args) {
                let ready = format!("visual_manage/mark_ready#{target}");
                let bind = format!("visual_manage/bind#{target}");
                self.failures
                    .retain(|key, _| !key.starts_with(&ready) && !key.starts_with(&bind));
            }
        }
        // Probe/get/list refresh the capability observation used by prepare.
        // A successful probe must clear stale prepare breakers for that
        // container without a process restart.
        if container_recovery_operation(tool) {
            if let Some(target) = breaker_target(args) {
                let prepare = format!("container_prepare_rollout#{target}");
                self.failures.retain(|key, (signature, _)| {
                    !(key.starts_with(&prepare)
                        && signature.starts_with("code:container_capabilities_stale"))
                });
            } else if tool == "container_list" {
                self.failures.retain(|key, (signature, _)| {
                    !(key.starts_with("container_prepare_rollout#")
                        && signature.starts_with("code:container_capabilities_stale"))
                });
            }
        }
    }
}

fn normalize_error(error: &str) -> String {
    let mut normalized = String::with_capacity(error.len());
    let mut in_digits = false;
    for character in error.chars() {
        if character.is_ascii_digit() {
            if !in_digits {
                normalized.push('#');
                in_digits = true;
            }
        } else {
            in_digits = false;
            normalized.push(character);
        }
    }
    normalized
}

/// A structured application failure (`{"code": …, "remediation": …}`) keeps its
/// shape under `structuredContent.structuredError` so the transcript can render
/// the code and the remediation instead of a JSON blob, while `error` stays a
/// one-line human summary for clients that only read text.
fn tool_error_result(error: String, terminal: bool) -> Value {
    let structured_error = serde_json::from_str::<Value>(&error)
        .ok()
        .filter(|value| value.get("code").and_then(Value::as_str).is_some());
    let summary = structured_error
        .as_ref()
        .map(structured_error_summary)
        .unwrap_or_else(|| error.clone());
    let mut structured = json!({"error": summary, "terminal": terminal});
    if let Some(detail) = &structured_error {
        structured["structuredError"] = detail.clone();
    }
    let text = structured_error
        .as_ref()
        .and_then(|detail| serde_json::to_string_pretty(detail).ok())
        .unwrap_or(error);
    json!({
        "content": [{"type":"text","text":text}],
        "structuredContent": structured,
        "isError": true
    })
}

fn structured_error_summary(detail: &Value) -> String {
    let mut summary = detail
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("error")
        .to_string();
    if let Some(missing) = detail.get("missing").and_then(Value::as_array) {
        let names: Vec<&str> = missing.iter().filter_map(Value::as_str).collect();
        if !names.is_empty() {
            summary.push_str(&format!(": missing {}", names.join(", ")));
        }
    }
    if let Some(remediation) = detail.get("remediation").and_then(Value::as_str) {
        summary.push_str(&format!(" — {remediation}"));
    }
    summary
}

/// Drive the NDJSON MCP stdio loop. `tools` returns the `tools/list` payload
/// object (must contain a `tools` array). `call_tool` handles `tools/call`.
pub fn run_stdio_server(
    info: McpServerInfo,
    tools: impl Fn() -> Value,
    call_tool: impl Fn(&str, &Value) -> Result<Value, String>,
) {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut breaker = ToolLoopBreaker::default();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { continue };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let response = match method {
            "initialize" => {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": { "tools": {} },
                        "serverInfo": {
                            "name": info.name,
                            "version": info.version,
                        }
                    }
                })
            }
            "notifications/initialized" | "notifications/cancelled" => continue,
            "tools/list" => json!({"jsonrpc":"2.0","id":id,"result":tools()}),
            "tools/call" => {
                let params = req.get("params").cloned().unwrap_or(json!({}));
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                if let Some(error) = breaker.terminal_error(name, &args) {
                    json!({"jsonrpc":"2.0","id":id,"result":tool_error_result(error, true)})
                } else {
                    match call_tool(name, &args) {
                        Ok(mut result) => {
                            breaker.record_success(name, &args);
                            let image = result
                                .as_object_mut()
                                .and_then(|object| object.remove("_mcpImage"));
                            let mut content = vec![json!({
                                "type": "text",
                                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
                            })];
                            if let Some(image) = image {
                                if let (Some(data), Some(mime_type)) = (
                                    image.get("data").and_then(Value::as_str),
                                    image.get("mimeType").and_then(Value::as_str),
                                ) {
                                    content.push(json!({
                                        "type": "image",
                                        "data": data,
                                        "mimeType": mime_type,
                                    }));
                                }
                            }
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": content,
                                    "structuredContent": result
                                }
                            })
                        }
                        Err(error) => {
                            breaker.record_failure(name, &args, &error);
                            json!({"jsonrpc":"2.0","id":id,"result":tool_error_result(error, false)})
                        }
                    }
                }
            }
            _ => {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": format!("unknown method {method}")}
                })
            }
        };
        let _ = writeln!(stdout, "{response}");
        let _ = stdout.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breaker_ignores_changing_numeric_arguments_and_resets_on_success() {
        let args = json!({"visual_id": "vis_1"});
        let mut breaker = ToolLoopBreaker::default();
        breaker.record_failure(
            "visual_capture_review",
            &args,
            "viewport 640 failed: denied",
        );
        assert!(breaker
            .terminal_error("visual_capture_review", &args)
            .is_none());
        breaker.record_failure(
            "visual_capture_review",
            &args,
            "viewport 800 failed: denied",
        );
        let terminal = breaker
            .terminal_error("visual_capture_review", &args)
            .unwrap();
        assert!(terminal.contains("stopped before invocation"));
        breaker.record_success("visual_capture_review", &args);
        assert!(breaker
            .terminal_error("visual_capture_review", &args)
            .is_none());
    }

    /// Seed 205: two deterministic capture failures on one visual disabled
    /// `bind`, `show`, and `render` for every visual, including the recovery
    /// path the agent needed.
    #[test]
    fn breaker_stops_one_operation_and_target_not_a_whole_tool() {
        let capture = json!({"operation": "capture_review", "arguments": {"visual_id": "vis_a"}});
        let mut breaker = ToolLoopBreaker::default();
        breaker.record_failure("visual_manage", &capture, "no rendered observation");
        breaker.record_failure("visual_manage", &capture, "no rendered observation");
        assert!(breaker.terminal_error("visual_manage", &capture).is_some());

        let other_operation = json!({"operation": "show", "arguments": {"visual_id": "vis_a"}});
        let other_visual =
            json!({"operation": "capture_review", "arguments": {"visual_id": "vis_b"}});
        assert!(breaker
            .terminal_error("visual_manage", &other_operation)
            .is_none());
        assert!(breaker
            .terminal_error("visual_manage", &other_visual)
            .is_none());
    }

    #[test]
    fn breaker_gives_a_new_root_cause_a_fresh_budget() {
        let args = json!({"visual_id": "vis_1"});
        let mut breaker = ToolLoopBreaker::default();
        let missing = json!({"code": "visual_observation_unavailable"}).to_string();
        let stale = json!({"code": "visual_revision_stale"}).to_string();
        breaker.record_failure("visual_capture_review", &args, &missing);
        breaker.record_failure("visual_capture_review", &args, &missing);
        assert!(breaker
            .terminal_error("visual_capture_review", &args)
            .is_some());
        // A different code means the agent is no longer repeating itself.
        breaker.record_failure("visual_capture_review", &args, &stale);
        assert!(breaker
            .terminal_error("visual_capture_review", &args)
            .is_none());
    }

    #[test]
    fn corrected_review_reopens_readiness_for_the_same_visual() {
        let ready = json!({
            "operation": "mark_ready",
            "arguments": {"visual_id": "vis_1", "revision": 5}
        });
        let review = json!({
            "operation": "review",
            "arguments": {"visual_id": "vis_1", "revision": 5}
        });
        let mut breaker = ToolLoopBreaker::default();
        breaker.record_failure("visual_manage", &ready, "screenshotInspected is missing");
        breaker.record_failure("visual_manage", &ready, "screenshotInspected is missing");
        assert!(breaker.terminal_error("visual_manage", &ready).is_some());

        breaker.record_success("visual_manage", &review);
        assert!(breaker.terminal_error("visual_manage", &ready).is_none());
    }

    #[test]
    fn readiness_breaker_is_scoped_to_revision() {
        let revision_five = json!({
            "operation": "mark_ready",
            "arguments": {"visual_id": "vis_1", "revision": 5}
        });
        let revision_six = json!({
            "operation": "mark_ready",
            "arguments": {"visual_id": "vis_1", "revision": 6}
        });
        let mut breaker = ToolLoopBreaker::default();
        breaker.record_failure("visual_manage", &revision_five, "review failed");
        breaker.record_failure("visual_manage", &revision_five, "review failed");
        assert!(breaker
            .terminal_error("visual_manage", &revision_five)
            .is_some());
        assert!(breaker
            .terminal_error("visual_manage", &revision_six)
            .is_none());
    }

    #[test]
    fn tool_failures_are_results_not_json_rpc_errors() {
        let result = tool_error_result("denied".into(), true);
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["terminal"], true);
        assert_eq!(result["structuredContent"]["error"], "denied");
        assert!(result["structuredContent"].get("structuredError").is_none());
    }

    #[test]
    fn successful_probe_clears_stale_prepare_breaker_without_restart() {
        let container = json!({"container_id": "ctr_banking77"});
        let mut breaker = ToolLoopBreaker::default();
        let stale = json!({
            "code": "container_capabilities_stale",
            "observed_at": "2026-08-17T12:00:00Z"
        })
        .to_string();
        breaker.record_failure("container_prepare_rollout", &container, &stale);
        breaker.record_failure("container_prepare_rollout", &container, &stale);
        assert!(breaker
            .terminal_error("container_prepare_rollout", &container)
            .is_some());
        assert!(breaker
            .terminal_error("container_probe", &container)
            .is_none());
        assert!(breaker
            .terminal_error("container_get", &container)
            .is_none());

        breaker.record_success("container_probe", &container);
        assert!(
            breaker
                .terminal_error("container_prepare_rollout", &container)
                .is_none(),
            "probe must supersede stale prepare breaker state"
        );
    }

    #[test]
    fn breaker_key_keeps_capability_revision_and_bind_slot() {
        let first = json!({
            "container_id": "ctr_1",
            "capability_revision": "rev-a"
        });
        let second = json!({
            "container_id": "ctr_1",
            "capability_revision": "rev-b"
        });
        assert_ne!(breaker_key("container_prepare_rollout", &first), breaker_key("container_prepare_rollout", &second));

        let bind = json!({
            "operation": "bind",
            "arguments": {"visual_id": "vis_1", "slot": "experiment", "kind": "inline"}
        });
        let show = json!({
            "operation": "show",
            "arguments": {"visual_id": "vis_1"}
        });
        assert!(breaker_key("visual_manage", &bind).contains("/experiment"));
        assert_ne!(
            breaker_key("visual_manage", &bind),
            breaker_key("visual_manage", &show)
        );
    }

    #[test]
    fn corrected_bind_payload_is_a_new_recovery_attempt() {
        let missing = json!({
            "operation": "bind",
            "arguments": {"visual_id": "vis_1", "slot": "experiment", "kind": "inline"}
        });
        let corrected = json!({
            "operation": "bind",
            "arguments": {
                "visual_id": "vis_1",
                "slot": "experiment",
                "kind": "inline",
                "data": {"experimentId": "exp_1"}
            }
        });
        let mut breaker = ToolLoopBreaker::default();
        let error = json!({"code": "visual_binding_invalid"}).to_string();
        breaker.record_failure("visual_manage", &missing, &error);
        breaker.record_failure("visual_manage", &missing, &error);
        assert!(breaker.terminal_error("visual_manage", &missing).is_some());
        assert!(breaker.terminal_error("visual_manage", &corrected).is_none());
        assert!(breaker
            .terminal_error(
                "visual_manage",
                &json!({"operation": "show", "arguments": {"visual_id": "vis_1"}})
            )
            .is_none());
    }

    #[test]
    fn container_capability_failure_crosses_the_mcp_boundary_as_an_error() {
        let detail = json!({
            "code": "container_capability_mismatch",
            "container_id": "ctr_33d6ee47de1e430ab80b1403ba04e555",
            "missing": ["rollouts.prepare", "trace_v5.capture"],
            "retryable": false,
            "remediation": "Select a normalized live-policy pool; this record is a raw environment engine."
        });
        let result = tool_error_result(serde_json::to_string(&detail).unwrap(), false);
        assert_eq!(result["isError"], true);
        assert_eq!(
            result["structuredContent"]["structuredError"]["code"],
            "container_capability_mismatch"
        );
        assert_eq!(
            result["structuredContent"]["structuredError"]["retryable"],
            false
        );
        let summary = result["structuredContent"]["error"].as_str().unwrap();
        assert!(summary.contains("container_capability_mismatch"));
        assert!(summary.contains("rollouts.prepare"));
        assert!(summary.contains("normalized live-policy pool"));
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("remediation"));
    }
}
