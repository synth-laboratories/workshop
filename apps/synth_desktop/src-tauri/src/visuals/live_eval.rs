//! W0 live-eval bind contract: input `stream` only; never guess Craftax/Harbor URLs.

use anyhow::{bail, Result};
use serde_json::{json, Value};

pub const LIVE_EVAL_INPUT: &str = "stream";
pub const LIVE_EVAL_SLOT: &str = LIVE_EVAL_INPUT;
pub const FORBIDDEN_LIVE_EVAL_SLOTS: &[&str] = &["live", "jobs"];
pub const LIVE_CRAFTAX_TEMPLATE: &str = "live.craftax.v1";
pub const LIVE_HARBOR_TEMPLATE: &str = "live.harbor_eval.v1";
pub const CRAFTAX_TEN_LANE_SEEDS: [i64; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
const SECRET_BINDING_KEYS: &[&str] = &[
    "authorization",
    "api_token",
    "worker_token",
    "bearer",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveEvalFamily {
    Craftax,
    Harbor,
}

impl LiveEvalFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Craftax => "craftax",
            Self::Harbor => "harbor",
        }
    }

    pub fn template_id(self) -> &'static str {
        match self {
            Self::Craftax => LIVE_CRAFTAX_TEMPLATE,
            Self::Harbor => LIVE_HARBOR_TEMPLATE,
        }
    }
}

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
        bail!("Forbidden live-eval input \"{slot}\"; bind input \"{LIVE_EVAL_INPUT}\"");
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

/// Classify from `/info` / `/metadata` / register `taskFamily`. No invented families.
pub fn classify_live_eval_family(
    info: &Value,
    task_family: Option<&str>,
) -> Option<LiveEvalFamily> {
    let mut tokens = Vec::new();
    for key in ["runtime_family", "env_family", "task_family", "target_id"] {
        if let Some(value) = info.get(key).and_then(Value::as_str) {
            tokens.push(value.to_ascii_lowercase());
        }
    }
    if let Some(chain) = info.get("adapter_chain").and_then(Value::as_array) {
        for item in chain {
            if let Some(value) = item.as_str() {
                tokens.push(value.to_ascii_lowercase());
            }
        }
    }
    if let Some(family) = task_family {
        tokens.push(family.to_ascii_lowercase());
    }
    for token in &tokens {
        if token.contains("harbor") {
            return Some(LiveEvalFamily::Harbor);
        }
        if token.contains("craftax") {
            return Some(LiveEvalFamily::Craftax);
        }
    }
    None
}

pub fn resolve_live_eval_template(
    requested: Option<&str>,
    family: Option<LiveEvalFamily>,
) -> Result<String> {
    match (requested, family) {
        (Some(template), Some(family)) => {
            assert_template_matches_family(template, family)?;
            Ok(template.to_string())
        }
        (Some(template), None) => Ok(template.to_string()),
        (None, Some(family)) => Ok(family.template_id().to_string()),
        (None, None) => {
            bail!("open_visual requires templateId or a classified container family")
        }
    }
}

pub fn assert_template_matches_family(template_id: &str, family: LiveEvalFamily) -> Result<()> {
    let expected = family.template_id();
    if template_id == expected {
        return Ok(());
    }
    bail!(
        "{} binds template \"{expected}\", not \"{template_id}\"",
        family.as_str()
    )
}

fn advertised_live_frames(info: &Value) -> &str {
    info.get("live_frames")
        .and_then(Value::as_str)
        .unwrap_or("unsupported")
}

/// Harbor must not advertise map frames. Desktop refuses rather than invent a Craftax view.
pub fn assert_harbor_live_frames(info: &Value) -> Result<()> {
    let frames = advertised_live_frames(info);
    if frames.eq_ignore_ascii_case("native") || frames.eq_ignore_ascii_case("true") {
        bail!("Harbor must not advertise live_frames={frames}");
    }
    Ok(())
}

pub fn harbor_policy_pins(requested: Option<&Value>) -> Result<Vec<Value>> {
    let pins = if let Some(value) = requested {
        if let Some(arr) = value.as_array() {
            arr.clone()
        } else {
            bail!("Harbor policyRefs must be an array of policy_ref objects");
        }
    } else {
        vec![
            json!({"harness": "harbor_fused", "config": "luna_med"}),
            json!({"harness": "harbor_fused", "config": "sol_med"}),
        ]
    };
    require_harbor_policy_pins(&pins)?;
    Ok(pins)
}

fn is_visualsbench(info: &Value) -> bool {
    ["benchmark_family", "task_family", "target_id", "dataset"]
        .iter()
        .filter_map(|key| info.get(*key).and_then(Value::as_str))
        .any(|value| value.to_ascii_lowercase().contains("visualsbench"))
}

pub fn visualsbench_policy_pins(requested: Option<&Value>) -> Result<Vec<Value>> {
    let pins = if let Some(value) = requested {
        value
            .as_array()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("VisualsBench policyRefs must be an array"))?
    } else {
        vec![json!({
            "harness": "harbor_fused",
            "config": "luna_med",
            "policy": "codex",
            "mcp_bind": "synth_visuals"
        })]
    };
    if pins.is_empty() {
        bail!("VisualsBench requires an explicitly pinned Codex policy_ref");
    }
    for pin in &pins {
        if pin.get("harness").and_then(Value::as_str) != Some("harbor_fused")
            || pin
                .get("config")
                .and_then(Value::as_str)
                .unwrap_or("")
                .is_empty()
            || pin.get("policy").and_then(Value::as_str) != Some("codex")
            || pin.get("mcp_bind").and_then(Value::as_str) != Some("synth_visuals")
        {
            bail!("VisualsBench policy_ref requires harbor_fused + Codex + mcp_bind=synth_visuals");
        }
    }
    Ok(pins)
}

pub fn require_visualsbench_start_policy(policy_ref: &Value) -> Result<()> {
    visualsbench_policy_pins(Some(&json!([policy_ref])))?;
    Ok(())
}

pub fn require_harbor_policy_pins(pins: &[Value]) -> Result<()> {
    if pins.len() < 2 {
        bail!("C5-02: Harbor requires two policy_refs registered before start");
    }
    for pin in pins {
        let harness = pin.get("harness").and_then(Value::as_str).unwrap_or("");
        let config = pin.get("config");
        if harness.is_empty() {
            bail!("C5-02: Harbor policy_ref requires harness");
        }
        if config.is_none() || config == Some(&Value::Null) {
            bail!("C5-02: Harbor policy_ref requires config");
        }
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
            if text.contains("sk_env_") || text.to_ascii_lowercase().contains("bearer ") {
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

/// Ten Craftax lanes, seeds 0–9. Caller names environment_ref / policy_ref / task_world.
/// This does not invoke a paid policy.
pub fn craftax_ten_lane_pins(
    environment_ref: &str,
    policy_ref: &Value,
    task_world: &Value,
) -> Result<Vec<Value>> {
    if environment_ref.trim().is_empty() {
        bail!("craftax 10-lane pin requires environment_ref");
    }
    if !policy_ref.is_object() {
        bail!("craftax 10-lane pin requires policy_ref object");
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
        bail!("craftax 10-lane pin requires policy_ref.harness and policy_ref.config");
    }
    let world_id = task_world
        .get("world_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if world_id.is_empty() {
        bail!("craftax 10-lane pin requires task_world.world_id");
    }
    let revision = task_world.get("revision").cloned().unwrap_or(Value::Null);
    Ok(CRAFTAX_TEN_LANE_SEEDS
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
        "inputs": [{
            "input": LIVE_EVAL_INPUT,
            "kind": "inline",
            "schema": "synth.trace-stream-event.v1",
            "data": { "events": [] }
        }]
    })
}

pub fn live_sse_bindings(source: &str) -> Value {
    json!({
        "schemaVersion": "synth.visual-bindings.v1",
        "inputs": [{
            "input": LIVE_EVAL_INPUT,
            "kind": "live_sse",
            "schema": "synth.trace-stream-event.v1",
            "source": source
        }]
    })
}

/// Container registration metadata for a live-eval visual — not the visual
/// binding envelope (`{ schemaVersion, inputs }`). `input` is canonical;
/// `slot` is the same value for old `liveEval.slot` readers.
pub fn live_eval_bind_metadata(
    family: LiveEvalFamily,
    info: &Value,
    policy_refs: Option<&Value>,
) -> Result<Value> {
    match family {
        LiveEvalFamily::Harbor => assert_harbor_live_frames(info)?,
        LiveEvalFamily::Craftax => {}
    }
    let mut bind = serde_json::Map::new();
    bind.insert("family".into(), json!(family.as_str()));
    bind.insert("templateId".into(), json!(family.template_id()));
    bind.insert("input".into(), json!(LIVE_EVAL_INPUT));
    bind.insert("slot".into(), json!(LIVE_EVAL_SLOT));
    match family {
        LiveEvalFamily::Harbor => {
            bind.insert("liveFrames".into(), json!("unsupported"));
        }
        LiveEvalFamily::Craftax => {
            if let Some(frames) = info.get("live_frames") {
                bind.insert("liveFrames".into(), frames.clone());
            }
        }
    }
    match family {
        LiveEvalFamily::Harbor => {
            if is_visualsbench(info) {
                bind.insert("benchmarkFamily".into(), json!("visualsbench"));
                bind.insert("mcpBind".into(), json!("synth_visuals"));
                bind.insert("requiresVisualsMcp".into(), json!(true));
                bind.insert(
                    "policyRefs".into(),
                    json!(visualsbench_policy_pins(policy_refs)?),
                );
            } else {
                bind.insert("policyRefs".into(), json!(harbor_policy_pins(policy_refs)?));
            }
        }
        LiveEvalFamily::Craftax => {
            if let Some(refs) = policy_refs {
                bind.insert("policyRefs".into(), refs.clone());
            }
        }
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
        assert!(bindings.get("slots").is_none());
        let slot = &bindings["inputs"][0];
        assert_eq!(slot["input"], LIVE_EVAL_INPUT);
        assert!(slot.get("slot").is_none());
        assert_eq!(slot["kind"], "inline");
        assert_eq!(slot["data"]["events"], json!([]));
        assert!(slot.get("source").is_none());
    }

    #[test]
    fn live_sse_bindings_use_declared_source() {
        let bindings = live_sse_bindings("http://127.0.0.1:8098/rollouts/r1/stream");
        assert!(bindings.get("slots").is_none());
        let slot = &bindings["inputs"][0];
        assert_eq!(slot["input"], LIVE_EVAL_INPUT);
        assert!(slot.get("slot").is_none());
        assert_eq!(slot["kind"], "live_sse");
        assert_eq!(slot["source"], "http://127.0.0.1:8098/rollouts/r1/stream");
    }

    #[test]
    fn refuses_guessed_craftax_and_harbor_urls() {
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
    fn classifies_family_from_runtime_and_adapter_chain() {
        assert_eq!(
            classify_live_eval_family(
                &json!({"runtime_family": "harbor", "target_id": "harbor_public"}),
                None
            ),
            Some(LiveEvalFamily::Harbor)
        );
        assert_eq!(
            classify_live_eval_family(&json!({"adapter_chain": ["harbor"]}), None),
            Some(LiveEvalFamily::Harbor)
        );
        assert_eq!(
            classify_live_eval_family(&json!({"target_id": "craftax_engine"}), None),
            Some(LiveEvalFamily::Craftax)
        );
        assert!(classify_live_eval_family(&json!({}), Some("digbench_mock")).is_none());
        assert!(classify_live_eval_family(&json!({"target_id": "unknown"}), None).is_none());
    }

    #[test]
    fn harbor_template_and_two_policies_before_start() {
        assert_eq!(LiveEvalFamily::Harbor.template_id(), LIVE_HARBOR_TEMPLATE);
        assert!(assert_template_matches_family(
            "live.container_rollouts.v1",
            LiveEvalFamily::Harbor
        )
        .is_err());
        assert!(
            assert_template_matches_family(LIVE_HARBOR_TEMPLATE, LiveEvalFamily::Harbor).is_ok()
        );
        assert!(assert_harbor_live_frames(&json!({"live_frames": "native"})).is_err());
        assert!(assert_harbor_live_frames(&json!({"live_frames": "unsupported"})).is_ok());
        let pins = harbor_policy_pins(None).unwrap();
        assert_eq!(pins.len(), 2);
        assert!(require_harbor_policy_pins(&[]).is_err());
        assert!(require_harbor_policy_pins(&[json!({"harness": "harbor_fused"})]).is_err());
        let bind = live_eval_bind_metadata(
            LiveEvalFamily::Harbor,
            &json!({"live_frames": "unsupported"}),
            None,
        )
        .unwrap();
        assert_eq!(bind["templateId"], LIVE_HARBOR_TEMPLATE);
        assert_eq!(bind["input"], "stream");
        assert_eq!(bind["slot"], "stream");
        assert_eq!(bind["policyRefs"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            resolve_live_eval_template(None, Some(LiveEvalFamily::Harbor)).unwrap(),
            LIVE_HARBOR_TEMPLATE
        );
        assert!(resolve_live_eval_template(
            Some("live.container_rollouts.v1"),
            Some(LiveEvalFamily::Harbor)
        )
        .is_err());
        assert!(resolve_live_eval_template(None, None).is_err());
    }

    #[test]
    fn harbor_register_writes_exact_live_eval_metadata() {
        let bind = live_eval_bind_metadata(LiveEvalFamily::Harbor, &json!({}), None).unwrap();
        assert_eq!(
            bind,
            json!({
                "family": "harbor",
                "templateId": "live.harbor_eval.v1",
                "input": "stream",
                "slot": "stream",
                "liveFrames": "unsupported",
                "policyRefs": [
                    {"harness": "harbor_fused", "config": "luna_med"},
                    {"harness": "harbor_fused", "config": "sol_med"}
                ]
            })
        );
        assert!(live_eval_bind_metadata(
            LiveEvalFamily::Harbor,
            &json!({"live_frames": "native"}),
            None
        )
        .is_err());
        assert!(assert_live_eval_slot(bind["slot"].as_str().unwrap()).is_ok());
    }

    #[test]
    fn visualsbench_harbor_pins_codex_and_visuals_mcp() {
        let bind = live_eval_bind_metadata(
            LiveEvalFamily::Harbor,
            &json!({
                "runtime_family":"harbor",
                "benchmark_family":"visualsbench",
                "live_frames":"unsupported"
            }),
            None,
        )
        .unwrap();
        assert_eq!(bind["templateId"], "live.harbor_eval.v1");
        assert_eq!(bind["input"], "stream");
        assert_eq!(bind["slot"], "stream");
        assert_eq!(bind["benchmarkFamily"], "visualsbench");
        assert_eq!(bind["requiresVisualsMcp"], true);
        assert_eq!(bind["policyRefs"][0]["policy"], "codex");
        assert_eq!(bind["policyRefs"][0]["mcp_bind"], "synth_visuals");
        assert!(require_visualsbench_start_policy(&bind["policyRefs"][0]).is_ok());
        assert!(require_visualsbench_start_policy(&json!({
            "harness":"harbor_fused", "config":"luna_med", "policy":"codex"
        }))
        .is_err());
    }

    #[test]
    fn live_eval_bindings_refuse_secrets() {
        assert_eq!(reward_from_env_status("completed"), Some(1.0));
        assert_eq!(reward_from_env_status("game_over"), Some(0.0));
        assert_eq!(reward_from_env_status("running"), None);
        assert!(assert_no_live_secrets(&json!({
            "observation": "a locked door",
            "Authorization": "Bearer secret-token"
        }))
        .is_err());
        assert!(assert_no_live_secrets(&json!({"text": "sk_env_leak"})).is_err());
        assert!(assert_no_live_secrets(&json!({"observation": "inspect"})).is_ok());
    }

    #[test]
    fn craftax_ten_lane_pins_seeds_zero_through_nine() {
        let pins = craftax_ten_lane_pins(
            "env:craftax_gold",
            &json!({"harness": "react", "config": "luna_med"}),
            &json!({"world_id": "craftax_default", "revision": "symbolic_survival"}),
        )
        .unwrap();
        assert_eq!(pins.len(), 10);
        let seeds: Vec<i64> = pins
            .iter()
            .map(|pin| pin["task_world"]["seed"].as_i64().unwrap())
            .collect();
        assert_eq!(seeds, CRAFTAX_TEN_LANE_SEEDS.to_vec());
        for pin in &pins {
            assert_eq!(pin["environment_ref"], "env:craftax_gold");
            assert_eq!(pin["policy_ref"]["harness"], "react");
            assert_eq!(pin["slot"], "stream");
            assert_eq!(pin["task_world"]["world_id"], "craftax_default");
        }
        assert!(craftax_ten_lane_pins(
            "env:craftax_gold",
            &json!({"harness": "react"}),
            &json!({"world_id": "craftax_default"})
        )
        .is_err());
    }
}
