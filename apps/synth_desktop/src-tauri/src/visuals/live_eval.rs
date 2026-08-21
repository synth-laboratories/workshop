//! W0 live-eval bind contract: slot `stream` only; never guess stream URLs.
//!
//! Workshop does not own task-family live templates or policy pins. A container
//! advertises `visual_template` / `live_eval_template` and optional policy refs;
//! missing fields fail closed. Do not substitute a named family.

use anyhow::{bail, Result};
use serde_json::{json, Value};

pub const LIVE_EVAL_SLOT: &str = "stream";
pub const FORBIDDEN_LIVE_EVAL_SLOTS: &[&str] = &["live", "jobs"];
pub const TEN_LANE_SEEDS: [i64; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
const SECRET_BINDING_KEYS: &[&str] = &["authorization", "api_token", "worker_token", "bearer"];

pub fn is_guessed_stream_url(source: &str) -> bool {
    let path = stream_path(source);
    if path == "/events" {
        return true;
    }
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    matches!(parts.as_slice(), ["rollouts", id, "stream"] if !id.is_empty())
}

/// `/events` was never a create-rollout descriptor. `/rollouts/{id}/stream` may be.
pub fn is_never_declared_stream_url(source: &str) -> bool {
    stream_path(source) == "/events"
}

pub fn assert_live_eval_slot(slot: &str) -> Result<()> {
    if FORBIDDEN_LIVE_EVAL_SLOTS.contains(&slot) {
        bail!("Forbidden live-eval slot \"{slot}\"; bind slot \"{LIVE_EVAL_SLOT}\"");
    }
    Ok(())
}

pub fn assert_declared_stream_source(source: &str) -> Result<()> {
    if is_never_declared_stream_url(source) {
        bail!(
            "Refusing guessed stream URL \"{source}\"; bind the declared stream.id from create-rollout"
        );
    }
    Ok(())
}

/// Classify from an advertised visual template. Do not infer a family by
/// matching container names in runtime ids.
pub fn advertised_live_eval_template(info: &Value) -> Option<&str> {
    info.get("visual_template")
        .or_else(|| info.get("live_eval_template"))
        .or_else(|| info.pointer("/live_eval/template"))
        .or_else(|| info.pointer("/liveEval/templateId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn resolve_live_eval_template(
    requested: Option<&str>,
    advertised: Option<&str>,
) -> Result<String> {
    match (
        requested.map(str::trim).filter(|value| !value.is_empty()),
        advertised.map(str::trim).filter(|value| !value.is_empty()),
    ) {
        (Some(requested), Some(advertised)) if requested == advertised => Ok(requested.to_string()),
        (Some(requested), Some(advertised)) => {
            bail!("open_visual template \"{requested}\" does not match advertised \"{advertised}\"")
        }
        (Some(requested), None) => Ok(requested.to_string()),
        (None, Some(advertised)) => Ok(advertised.to_string()),
        (None, None) => {
            bail!("open_visual requires templateId or a container that advertises a live eval template")
        }
    }
}

fn advertised_live_frames(info: &Value) -> Option<&Value> {
    info.get("live_frames")
        .or_else(|| info.pointer("/live_eval/live_frames"))
        .or_else(|| info.pointer("/liveEval/liveFrames"))
}

fn advertised_policy_refs(info: &Value) -> Option<&Value> {
    info.get("policy_refs")
        .or_else(|| info.get("policyRefs"))
        .or_else(|| info.pointer("/live_eval/policy_refs"))
        .or_else(|| info.pointer("/live_eval/policyRefs"))
        .or_else(|| info.pointer("/liveEval/policyRefs"))
        .or_else(|| info.pointer("/capabilities/policy_refs"))
}

fn advertised_mcp_bind(info: &Value) -> Option<&str> {
    info.get("mcp_bind")
        .or_else(|| info.get("mcpBind"))
        .or_else(|| info.pointer("/live_eval/mcp_bind"))
        .or_else(|| info.pointer("/liveEval/mcpBind"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn advertised_requires_mcp(info: &Value) -> bool {
    info.get("requires_visuals_mcp")
        .or_else(|| info.get("requiresVisualsMcp"))
        .or_else(|| info.pointer("/live_eval/requires_visuals_mcp"))
        .or_else(|| info.pointer("/liveEval/requiresVisualsMcp"))
        .and_then(Value::as_bool)
        == Some(true)
        || advertised_mcp_bind(info).is_some()
}

fn advertised_benchmark_family(info: &Value) -> Option<&str> {
    info.get("benchmark_family")
        .or_else(|| info.get("benchmarkFamily"))
        .or_else(|| info.pointer("/live_eval/benchmark_family"))
        .or_else(|| info.pointer("/liveEval/benchmarkFamily"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn advertised_family(info: &Value) -> Option<&str> {
    info.get("env_family")
        .or_else(|| info.get("task_family"))
        .or_else(|| info.get("runtime_family"))
        .or_else(|| info.pointer("/live_eval/family"))
        .or_else(|| info.pointer("/liveEval/family"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn require_policy_pins(pins: &[Value]) -> Result<()> {
    if pins.is_empty() {
        bail!("live eval policyRefs must be a non-empty array of policy_ref objects");
    }
    for pin in pins {
        let harness = pin.get("harness").and_then(Value::as_str).unwrap_or("");
        let config = pin.get("config");
        if harness.trim().is_empty() {
            bail!("live eval policy_ref requires harness");
        }
        if config.is_none() || config == Some(&Value::Null) {
            bail!("live eval policy_ref requires config");
        }
    }
    Ok(())
}

fn resolve_policy_pins(info: &Value, requested: Option<&Value>) -> Result<Option<Vec<Value>>> {
    let value = match requested {
        Some(value) => Some(value),
        None => advertised_policy_refs(info),
    };
    let Some(value) = value else {
        return Ok(None);
    };
    let pins = value.as_array().cloned().ok_or_else(|| {
        anyhow::anyhow!("live eval policyRefs must be an array of policy_ref objects")
    })?;
    require_policy_pins(&pins)?;
    Ok(Some(pins))
}

/// If the container advertised an MCP bind, the start policy_ref must carry it.
pub fn require_advertised_mcp_bind(live_eval: &Value, policy_ref: &Value) -> Result<()> {
    let required = live_eval.get("requiresVisualsMcp").and_then(Value::as_bool) == Some(true);
    let expected = live_eval
        .get("mcpBind")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !required && expected.is_empty() {
        return Ok(());
    }
    if expected.is_empty() {
        bail!("refusing start: advertised live eval requires an MCP bind, but none was advertised");
    }
    let actual = policy_ref
        .get("mcp_bind")
        .or_else(|| policy_ref.pointer("/config/mcp_bind"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if actual != expected {
        bail!(
            "refusing start: policy_ref.mcp_bind must be `{expected}` as advertised by the container"
        );
    }
    Ok(())
}

/// `/reward` authority from env status. Incomplete stays null, never 0.
pub fn reward_from_env_status(status: &str) -> Option<f64> {
    match status {
        "completed" => Some(1.0),
        "game_over" => Some(0.0),
        _ => None,
    }
}

pub fn assert_no_live_secrets(value: &Value) -> Result<()> {
    walk_for_live_secrets(value, "")
}

fn walk_for_live_secrets(value: &Value, key: &str) -> Result<()> {
    let key_lower = key.to_ascii_lowercase();
    if SECRET_BINDING_KEYS
        .iter()
        .any(|secret| key_lower == *secret || key_lower.ends_with(&format!("_{secret}")))
        && !value.is_null()
        && value.as_str() != Some("")
    {
        bail!("token must never appear in live eval log or bindings ({key})");
    }
    match value {
        Value::String(text) => {
            let lower = text.to_ascii_lowercase();
            if text.contains("API_TOKEN=") || lower.contains("bearer ") {
                bail!("token must never appear in live eval log or bindings");
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                walk_for_live_secrets(item, key)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for (child_key, child) in map {
                walk_for_live_secrets(child, child_key)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Ten live-eval lanes, seeds 0–9. Caller names environment_ref / policy_ref / task_world.
/// This does not invoke a paid policy.
pub fn seed_lane_pins(
    environment_ref: &str,
    policy_ref: &Value,
    task_world: &Value,
) -> Result<Vec<Value>> {
    if environment_ref.trim().is_empty() {
        bail!("10-lane pin requires environment_ref");
    }
    if !policy_ref.is_object() {
        bail!("10-lane pin requires policy_ref object");
    }
    let harness = policy_ref
        .get("harness")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let config = policy_ref
        .get("config")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if harness.is_empty() || config.is_empty() {
        bail!("10-lane pin requires policy_ref.harness and policy_ref.config");
    }
    let world_id = task_world
        .get("world_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if world_id.is_empty() {
        bail!("10-lane pin requires task_world.world_id");
    }
    let revision = task_world.get("revision").cloned().unwrap_or(Value::Null);
    Ok(TEN_LANE_SEEDS
        .iter()
        .map(|seed| {
            json!({
                "environment_ref": environment_ref,
                "policy_ref": policy_ref,
                "task_world": {
                    "world_id": world_id,
                    "revision": revision,
                    "seed": seed
                },
                "task_instance_id": format!("seed:{seed}"),
                "seed": seed,
                "slot": LIVE_EVAL_SLOT
            })
        })
        .collect())
}

pub fn pending_stream_bindings() -> Value {
    json!({
        "schemaVersion": "synth.visual-bindings.v1",
        "slots": [{
            "slot": LIVE_EVAL_SLOT,
            "kind": "inline",
            "schema": "synth.trace-stream-event.v1",
            "data": { "events": [] }
        }]
    })
}

pub fn live_sse_bindings(source: &str) -> Value {
    json!({
        "schemaVersion": "synth.visual-bindings.v1",
        "slots": [{
            "slot": LIVE_EVAL_SLOT,
            "kind": "live_sse",
            "schema": "synth.trace-stream-event.v1",
            "source": source
        }]
    })
}

pub fn live_eval_bind_metadata(info: &Value, policy_refs: Option<&Value>) -> Result<Value> {
    let template_id = advertised_live_eval_template(info).ok_or_else(|| {
        anyhow::anyhow!(
            "live eval bind requires an advertised visual_template / live_eval_template"
        )
    })?;
    let mut bind = serde_json::Map::new();
    bind.insert("templateId".into(), json!(template_id));
    bind.insert("slot".into(), json!(LIVE_EVAL_SLOT));
    if let Some(family) = advertised_family(info) {
        bind.insert("family".into(), json!(family));
    }
    if let Some(frames) = advertised_live_frames(info) {
        bind.insert("liveFrames".into(), frames.clone());
    }
    if let Some(pins) = resolve_policy_pins(info, policy_refs)? {
        bind.insert("policyRefs".into(), json!(pins));
    }
    if let Some(mcp) = advertised_mcp_bind(info) {
        bind.insert("mcpBind".into(), json!(mcp));
    }
    if advertised_requires_mcp(info) {
        bind.insert("requiresVisualsMcp".into(), json!(true));
    }
    if let Some(benchmark) = advertised_benchmark_family(info) {
        bind.insert("benchmarkFamily".into(), json!(benchmark));
    }
    let bind = Value::Object(bind);
    assert_no_live_secrets(&bind)?;
    Ok(bind)
}

fn stream_path(source: &str) -> String {
    let without_query = source.split(['?', '#']).next().unwrap_or(source);
    if let Some(rest) = without_query.split("://").nth(1) {
        let path = rest.find('/').map(|idx| &rest[idx..]).unwrap_or("/");
        return path.trim_end_matches('/').to_string();
    }
    without_query.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbids_live_and_jobs_slots() {
        assert!(assert_live_eval_slot("live").is_err());
        assert!(assert_live_eval_slot("jobs").is_err());
        assert!(assert_live_eval_slot("stream").is_ok());
        assert!(assert_live_eval_slot("acceptance").is_ok());
    }

    #[test]
    fn pending_visual_is_honest_empty_inline_data() {
        let bindings = pending_stream_bindings();
        let slot = &bindings["slots"][0];
        assert_eq!(slot["slot"], LIVE_EVAL_SLOT);
        assert_eq!(slot["kind"], "inline");
        assert_eq!(slot["data"]["events"], json!([]));
        assert!(slot.get("source").is_none());
    }

    #[test]
    fn live_sse_bindings_use_declared_source() {
        let bindings = live_sse_bindings("http://127.0.0.1:8098/rollouts/r1/stream");
        let slot = &bindings["slots"][0];
        assert_eq!(slot["slot"], LIVE_EVAL_SLOT);
        assert_eq!(slot["kind"], "live_sse");
        assert_eq!(slot["source"], "http://127.0.0.1:8098/rollouts/r1/stream");
    }

    #[test]
    fn refuses_guessed_stream_urls() {
        assert!(is_guessed_stream_url("http://127.0.0.1:8098/events"));
        assert!(is_guessed_stream_url(
            "http://127.0.0.1:8098/rollouts/r1/stream"
        ));
        assert!(assert_declared_stream_source("http://127.0.0.1:8098/events").is_err());
        assert!(assert_declared_stream_source("http://127.0.0.1:8098/rollouts/r1/stream").is_ok());
        assert!(!is_guessed_stream_url(
            "http://127.0.0.1:8098/declared/stream_r1"
        ));
    }

    #[test]
    fn template_comes_from_advertisement_not_runtime_tokens() {
        assert_eq!(
            advertised_live_eval_template(&json!({"visual_template": "live.eval.v1"})),
            Some("live.eval.v1")
        );
        assert_eq!(
            advertised_live_eval_template(&json!({"live_eval_template": "live.frames.v1"})),
            Some("live.frames.v1")
        );
        assert_eq!(
            advertised_live_eval_template(&json!({"live_eval": {"template": "live.text.v1"}})),
            Some("live.text.v1")
        );
        assert!(advertised_live_eval_template(&json!({"target_id": "craftax_engine"})).is_none());
        assert!(advertised_live_eval_template(&json!({"runtime_family": "harbor"})).is_none());
        assert!(advertised_live_eval_template(&json!({"target_id": "unknown"})).is_none());
    }

    #[test]
    fn bind_copies_advertised_contract_and_does_not_invent_pins() {
        let bind = live_eval_bind_metadata(
            &json!({
                "visual_template": "live.eval.v1",
                "live_frames": "unsupported",
                "runtime_family": "text-env",
                "policy_refs": [
                    {"harness": "fused", "config": "policy_a"},
                    {"harness": "fused", "config": "policy_b"}
                ]
            }),
            None,
        )
        .unwrap();
        assert_eq!(
            bind,
            json!({
                "family": "text-env",
                "templateId": "live.eval.v1",
                "slot": "stream",
                "liveFrames": "unsupported",
                "policyRefs": [
                    {"harness": "fused", "config": "policy_a"},
                    {"harness": "fused", "config": "policy_b"}
                ]
            })
        );
        let native = live_eval_bind_metadata(
            &json!({"visual_template": "live.eval.v1", "live_frames": "native"}),
            None,
        )
        .unwrap();
        assert_eq!(native["liveFrames"], "native");
        assert!(native.get("policyRefs").is_none());
        assert!(live_eval_bind_metadata(&json!({"runtime_family": "text-env"}), None).is_err());
        assert!(resolve_live_eval_template(None, None).is_err());
        assert_eq!(
            resolve_live_eval_template(None, Some("live.eval.v1")).unwrap(),
            "live.eval.v1"
        );
        assert!(resolve_live_eval_template(Some("live.other.v1"), Some("live.eval.v1")).is_err());
    }

    #[test]
    fn advertised_mcp_bind_must_appear_on_start_policy() {
        let bind = live_eval_bind_metadata(
            &json!({
                "visual_template": "live.eval.v1",
                "live_frames": "unsupported",
                "benchmark_family": "visuals",
                "mcp_bind": "synth_visuals",
                "policy_refs": [{
                    "harness": "fused",
                    "config": "author",
                    "policy": "codex",
                    "mcp_bind": "synth_visuals"
                }]
            }),
            None,
        )
        .unwrap();
        assert_eq!(bind["mcpBind"], "synth_visuals");
        assert_eq!(bind["requiresVisualsMcp"], true);
        assert_eq!(bind["benchmarkFamily"], "visuals");
        assert!(require_advertised_mcp_bind(&bind, &bind["policyRefs"][0]).is_ok());
        assert!(require_advertised_mcp_bind(
            &bind,
            &json!({"harness":"fused", "config":"author", "policy":"codex"})
        )
        .is_err());
    }

    #[test]
    fn advertised_pins_fail_closed_without_harness_or_config() {
        assert!(require_policy_pins(&[]).is_err());
        assert!(require_policy_pins(&[json!({"harness": "fused"})]).is_err());
        assert!(live_eval_bind_metadata(
            &json!({
                "visual_template": "live.eval.v1",
                "policy_refs": [{"harness": "fused"}]
            }),
            None
        )
        .is_err());
        assert_eq!(reward_from_env_status("completed"), Some(1.0));
        assert_eq!(reward_from_env_status("game_over"), Some(0.0));
        assert_eq!(reward_from_env_status("running"), None);
        assert!(assert_no_live_secrets(&json!({
            "observation": "a locked door",
            "Authorization": "Bearer secret-token"
        }))
        .is_err());
        assert!(assert_no_live_secrets(&json!({"text": "DIGBENCH_API_TOKEN=leak"})).is_err());
        assert!(assert_no_live_secrets(&json!({"observation": "inspect"})).is_ok());
    }

    #[test]
    fn seed_lane_pins_seeds_zero_through_nine() {
        let pins = seed_lane_pins(
            "env:example",
            &json!({"harness": "react", "config": "default"}),
            &json!({"world_id": "world_default", "revision": "v1"}),
        )
        .unwrap();
        assert_eq!(pins.len(), 10);
        let seeds: Vec<i64> = pins
            .iter()
            .map(|pin| pin["task_world"]["seed"].as_i64().unwrap())
            .collect();
        assert_eq!(seeds, TEN_LANE_SEEDS.to_vec());
        for pin in &pins {
            assert_eq!(pin["environment_ref"], "env:example");
            assert_eq!(pin["policy_ref"]["harness"], "react");
            assert_eq!(pin["slot"], "stream");
            assert_eq!(pin["task_world"]["world_id"], "world_default");
        }
        assert!(seed_lane_pins(
            "env:example",
            &json!({"harness": "react"}),
            &json!({"world_id": "world_default"})
        )
        .is_err());
    }
}
