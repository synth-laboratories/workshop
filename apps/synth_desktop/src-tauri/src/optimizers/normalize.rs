//! Normalize local OSS (GEPA) and hosted optimizers-beta (GO-EX) payloads
//! into the shared `optimizer_event.v1` envelope.

use super::models::{OptimizerEventEnvelope, OptimizerRunStatus, OPTIMIZER_EVENT_SCHEMA_VERSION};
use serde_json::{json, Map, Value};

pub fn normalize_event(
    raw: &Value,
    default_run_id: &str,
    default_algorithm_id: &str,
) -> Option<OptimizerEventEnvelope> {
    let obj = raw.as_object()?;
    if let Some(payload) = obj.get("payload").and_then(Value::as_object) {
        if payload
            .get("schema_version")
            .and_then(Value::as_str)
            .is_some_and(|value| value.starts_with("optimizer_event"))
        {
            let mut event = normalize_canonical(payload, default_run_id, default_algorithm_id)?;
            if let Some(seq) = as_u64(obj.get("seq").or_else(|| obj.get("_seq"))) {
                if let Some(raw) = event.raw.as_object_mut() {
                    raw.insert("sourceSequenceNumber".into(), json!(seq));
                }
            }
            return Some(event);
        }
    }
    if obj
        .get("schema_version")
        .and_then(Value::as_str)
        .is_some_and(|v| v.starts_with("optimizer_event"))
        || obj.contains_key("optimizer_run_id")
            && (obj.contains_key("sequence_number") || obj.contains_key("sequenceNumber"))
    {
        return normalize_canonical(obj, default_run_id, default_algorithm_id);
    }
    if obj.get("schema_version").and_then(Value::as_str) == Some("event_stream_record.v1")
        || (obj.contains_key("fields")
            && (obj.contains_key("event_type") || obj.contains_key("type")))
    {
        return normalize_gepa_oss(obj, default_run_id, default_algorithm_id);
    }
    if obj.contains_key("_seq") || obj.contains_key("event_type") || obj.contains_key("event_kind")
    {
        return normalize_hosted_or_goex(obj, default_run_id, default_algorithm_id);
    }
    None
}

pub fn normalize_events(
    raw_events: &[Value],
    default_run_id: &str,
    default_algorithm_id: &str,
) -> Vec<OptimizerEventEnvelope> {
    let mut out = Vec::new();
    for raw in raw_events {
        if let Some(event) = normalize_event(raw, default_run_id, default_algorithm_id) {
            out.push(event);
        }
    }
    out.sort_by_key(|event| event.sequence_number);
    out
}

fn normalize_canonical(
    obj: &Map<String, Value>,
    default_run_id: &str,
    default_algorithm_id: &str,
) -> Option<OptimizerEventEnvelope> {
    let sequence_number = as_u64(
        obj.get("sequence_number")
            .or_else(|| obj.get("sequenceNumber"))
            .or_else(|| obj.get("seq"))
            .or_else(|| obj.get("_seq")),
    )?;
    let event_type = obj
        .get("type")
        .or_else(|| obj.get("event_type"))
        .and_then(Value::as_str)
        .unwrap_or("optimizer.event")
        .to_string();
    let run_id = obj
        .get("optimizer_run_id")
        .or_else(|| obj.get("optimizerRunId"))
        .or_else(|| obj.get("run_id"))
        .and_then(Value::as_str)
        .unwrap_or(default_run_id)
        .to_string();
    let algorithm_id = obj
        .get("algorithm_id")
        .or_else(|| obj.get("algorithmId"))
        .or_else(|| obj.get("algorithm"))
        .and_then(Value::as_str)
        .map(normalize_algorithm_id)
        .unwrap_or_else(|| default_algorithm_id.to_string());
    let occurred_at = obj
        .get("occurred_at")
        .or_else(|| obj.get("occurredAt"))
        .or_else(|| obj.get("created_at"))
        .or_else(|| obj.get("createdAt"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut delta = obj
        .get("delta")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    lift_child_resource_ref(&mut delta);
    let usage_delta = obj
        .get("usage_delta")
        .or_else(|| obj.get("usageDelta"))
        .and_then(Value::as_object)
        .cloned()
        .or_else(|| {
            (event_type == "runtime.job.completed")
                .then(|| extract_usage(&Value::Object(delta.clone())))
                .flatten()
        });
    Some(OptimizerEventEnvelope {
        schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
        event_id: obj
            .get("event_id")
            .or_else(|| obj.get("eventId"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| Some(format!("{run_id}:{sequence_number}"))),
        event_type,
        sequence_number,
        occurred_at,
        optimizer_run_id: run_id,
        algorithm_id,
        level: obj.get("level").and_then(Value::as_str).map(str::to_string),
        item: obj.get("item").cloned(),
        delta,
        snapshot: obj.get("snapshot").and_then(Value::as_object).cloned(),
        usage_delta,
        artifact_refs: obj
            .get("artifact_refs")
            .or_else(|| obj.get("artifactRefs"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        error: obj.get("error").cloned(),
        raw: Value::Object(obj.clone()),
    })
}

fn normalize_gepa_oss(
    obj: &Map<String, Value>,
    default_run_id: &str,
    default_algorithm_id: &str,
) -> Option<OptimizerEventEnvelope> {
    let sequence_number = as_u64(
        obj.get("sequence_number")
            .or_else(|| obj.get("sequenceNumber"))
            .or_else(|| obj.get("_seq"))
            .or_else(|| obj.get("seq")),
    )?;
    let event_type = obj
        .get("event_type")
        .or_else(|| obj.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("optimizer.event")
        .to_string();
    let fields = obj.get("fields").cloned().unwrap_or(json!({}));
    let run_id = fields
        .get("run_id")
        .and_then(Value::as_str)
        .unwrap_or(default_run_id)
        .to_string();
    let algorithm_id = normalize_algorithm_id(
        fields
            .get("algorithm_id")
            .or_else(|| fields.get("algorithm"))
            .and_then(Value::as_str)
            .unwrap_or(default_algorithm_id),
    );
    let mut delta = fields.as_object().cloned().unwrap_or_default();
    lift_child_resource_ref(&mut delta);
    if let Some(message) = obj.get("message").and_then(Value::as_str) {
        delta.insert("message".into(), json!(message));
    }
    let item = infer_item_from_gepa(&event_type, &fields);
    let snapshot = gepa_snapshot(&event_type, &fields);
    let artifact_refs = gepa_artifact_refs(&event_type, &fields);
    let usage_delta = if event_type == "runtime.job.completed" {
        extract_usage(&fields)
    } else {
        None
    };
    Some(OptimizerEventEnvelope {
        schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
        event_id: obj
            .get("event_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| Some(format!("{run_id}:{sequence_number}"))),
        event_type,
        sequence_number,
        occurred_at: obj
            .get("timestamp")
            .or_else(|| obj.get("ts"))
            .or_else(|| obj.get("occurred_at"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        optimizer_run_id: run_id,
        algorithm_id,
        level: Some("info".into()),
        item,
        delta,
        snapshot,
        usage_delta,
        artifact_refs,
        error: obj.get("error").cloned(),
        raw: Value::Object(obj.clone()),
    })
}

fn normalize_hosted_or_goex(
    obj: &Map<String, Value>,
    default_run_id: &str,
    default_algorithm_id: &str,
) -> Option<OptimizerEventEnvelope> {
    let sequence_number = as_u64(
        obj.get("_seq")
            .or_else(|| obj.get("seq"))
            .or_else(|| obj.get("sequence_number")),
    )?;
    let event_type = obj
        .get("event_type")
        .or_else(|| obj.get("event_kind"))
        .or_else(|| obj.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("optimizer.event")
        .to_string();
    let payload = obj
        .get("payload")
        .cloned()
        .unwrap_or_else(|| Value::Object(obj.clone()));
    let run_id = obj
        .get("run_id")
        .or_else(|| payload.get("run_id"))
        .and_then(Value::as_str)
        .unwrap_or(default_run_id)
        .to_string();
    let algorithm_hint = obj
        .get("algorithm")
        .or_else(|| obj.get("algorithm_id"))
        .or_else(|| payload.get("algorithm"))
        .and_then(Value::as_str)
        .unwrap_or(default_algorithm_id);
    let algorithm_id = normalize_algorithm_id(algorithm_hint);
    let mut delta = payload.as_object().cloned().unwrap_or_default();
    lift_child_resource_ref(&mut delta);
    if !obj.contains_key("payload") {
        // Native go-ex jsonl: keep useful top-level keys in delta.
        for key in ["theme", "saturation", "phase", "tick", "status", "message"] {
            if let Some(value) = obj.get(key) {
                delta.insert(key.to_string(), value.clone());
            }
        }
    }
    let snapshot = if matches!(
        event_type.as_str(),
        "frontier.updated"
            | "frontier.snapshot"
            | "go-ex.board.updated"
            | "goex.board.updated"
            | "board.updated"
    ) {
        Some(delta.clone())
    } else {
        None
    };
    let mapped_type = map_goex_event_type(&event_type, &algorithm_id);
    Some(OptimizerEventEnvelope {
        schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
        event_id: Some(format!("{run_id}:{sequence_number}")),
        event_type: mapped_type,
        sequence_number,
        occurred_at: obj
            .get("created_at")
            .or_else(|| obj.get("ts"))
            .or_else(|| obj.get("timestamp"))
            .or_else(|| obj.get("occurred_at"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        optimizer_run_id: run_id,
        algorithm_id,
        level: Some("info".into()),
        item: infer_item_from_hosted(&event_type, &payload),
        delta,
        snapshot,
        usage_delta: extract_usage(&payload),
        artifact_refs: vec![],
        error: obj
            .get("error")
            .cloned()
            .or_else(|| payload.get("error").cloned()),
        raw: Value::Object(obj.clone()),
    })
}

pub fn normalize_algorithm_id(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "go-ex" => "go-ex".into(),
        "gepa" => "gepa".into(),
        "eval" => "eval".into(),
        "sft" => "sft".into(),
        "cispo" => "cispo".into(),
        other if !other.is_empty() => other.to_string(),
        _ => "unknown".into(),
    }
}

pub fn cloud_run_to_mirror(
    payload: &Value,
) -> Option<(String, String, String, u64, Option<String>)> {
    let obj = payload.as_object()?;
    let id = obj
        .get("run_id")
        .or_else(|| obj.get("id"))
        .and_then(Value::as_str)?
        .to_string();
    let algorithm_id = normalize_algorithm_id(
        obj.get("algorithm")
            .or_else(|| obj.get("algorithm_id"))
            .and_then(Value::as_str)
            .unwrap_or("gepa"),
    );
    let status = obj
        .get("status")
        .and_then(Value::as_str)
        .and_then(OptimizerRunStatus::parse)?
        .as_str()
        .to_string();
    let cursor_seq = as_u64(obj.get("cursor_seq")).unwrap_or(0);
    let objective = obj
        .get("objective")
        .or_else(|| obj.get("project_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some((id, algorithm_id, status, cursor_seq, objective))
}

fn map_goex_event_type(event_type: &str, algorithm_id: &str) -> String {
    if algorithm_id != "go-ex" {
        return event_type.to_string();
    }
    match event_type {
        "board.updated" | "tick.updated" => "go-ex.board.updated".into(),
        "theme.updated" | "themes.updated" => "go-ex.theme.updated".into(),
        "run.started" | "optimizer.run.started" => "optimizer.run.created".into(),
        "run.completed" | "optimizer.run.completed" => "optimizer.run.completed".into(),
        other => {
            if other.starts_with("go-ex.")
                || other.starts_with("goex.")
                || other.starts_with("optimizer.")
            {
                other.to_string()
            } else {
                format!("go-ex.{other}")
            }
        }
    }
}

fn infer_item_from_gepa(event_type: &str, fields: &Value) -> Option<Value> {
    let candidate_id = fields
        .get("candidate_id")
        .or_else(|| fields.get("changed_candidate_id"))
        .or_else(|| fields.get("best_candidate_id"))
        .and_then(Value::as_str)?;
    let status = if event_type.contains("accepted") {
        "accepted"
    } else if event_type.contains("rejected") {
        "rejected"
    } else {
        "evaluated"
    };
    Some(json!({
        "kind": "candidate",
        "type": "candidate",
        "id": candidate_id,
        "status": status,
        "raw": fields
    }))
}

fn gepa_snapshot(event_type: &str, fields: &Value) -> Option<Map<String, Value>> {
    if event_type == "frontier.updated" {
        let best_id = fields.get("best_candidate_id").and_then(Value::as_str);
        let cells = fields
            .get("frontier")
            .and_then(Value::as_array)
            .map(|frontier| {
                frontier
                    .iter()
                    .filter_map(|entry| {
                        let id = entry.get("candidate_id")?.as_str()?;
                        Some(json!({
                            "candidateId": id,
                            "quality": entry.get("train_reward").cloned().unwrap_or(Value::Null),
                            "heldoutQuality": entry.get("heldout_reward").cloned().unwrap_or(Value::Null),
                            "costUsd": entry.get("cost_usd").cloned().unwrap_or(Value::Null),
                            "accent": best_id == Some(id),
                        }))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut snapshot = Map::new();
        snapshot.insert("cells".into(), Value::Array(cells));
        if let Some(score) = fields.get("best_train_reward") {
            snapshot.insert("bestScore".into(), score.clone());
        }
        return Some(snapshot);
    }
    None
}

fn gepa_artifact_refs(event_type: &str, fields: &Value) -> Vec<Value> {
    let (kind, path_key, title) = match event_type {
        "score_chart.written" => ("chart", "chart_path", "GEPA score chart"),
        "workspace.persisted" => ("workspace", "workspace_db_path", "Optimizer workspace"),
        _ => return vec![],
    };
    fields
        .get(path_key)
        .and_then(Value::as_str)
        .map(|path| vec![json!({ "kind": kind, "id": path, "path": path, "title": title })])
        .unwrap_or_default()
}

fn infer_item_from_hosted(event_type: &str, payload: &Value) -> Option<Value> {
    if let Some(candidate_id) = payload
        .get("candidate_id")
        .or_else(|| payload.get("candidateId"))
        .and_then(Value::as_str)
    {
        return Some(json!({
            "kind": "candidate",
            "type": "candidate",
            "id": candidate_id,
            "status": payload.get("status").cloned().unwrap_or(json!("updated")),
            "raw": payload
        }));
    }
    if event_type.contains("checkpoint") {
        if let Some(id) = payload
            .get("checkpoint_id")
            .or_else(|| payload.get("id"))
            .and_then(Value::as_str)
        {
            return Some(json!({
                "kind": "checkpoint",
                "type": "checkpoint",
                "id": id,
                "status": "created",
                "raw": payload
            }));
        }
    }
    None
}

fn lift_child_resource_ref(delta: &mut Map<String, Value>) {
    if delta
        .get("child_resource_ref")
        .is_some_and(is_container_resource_ref)
    {
        return;
    }
    if let Some(child) = delta.get("child_eval_ref").cloned() {
        if is_container_resource_ref(&child) {
            delta.insert("child_resource_ref".into(), child);
        }
    }
}

fn is_container_resource_ref(value: &Value) -> bool {
    value.get("schema").and_then(Value::as_str) == Some("synth.resource-ref.v1")
        && value.get("kind").and_then(Value::as_str) == Some("container_rollout")
        && value
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty())
        && value
            .pointer("/attributes/stream_id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty())
        && value
            .pointer("/attributes/reward_url")
            .and_then(Value::as_str)
            .is_some_and(|url| !url.is_empty())
}

fn extract_usage(fields: &Value) -> Option<Map<String, Value>> {
    let mut usage = Map::new();
    for (src, dst) in [
        ("cost_usd", "cost_usd"),
        ("prompt_tokens", "prompt_tokens"),
        ("completion_tokens", "completion_tokens"),
        ("rollouts", "rollouts"),
        ("rollout_count", "rollouts"),
        ("wall_time_ms", "wall_time_ms"),
        ("wall_seconds", "wall_time_ms"),
    ] {
        if let Some(value) = fields.get(src) {
            if src == "wall_seconds" {
                if let Some(seconds) = value.as_f64() {
                    usage.insert(dst.into(), json!((seconds * 1000.0) as u64));
                }
            } else {
                usage.insert(dst.into(), value.clone());
            }
        }
    }
    if let Some(nested) = fields.get("usage") {
        for key in ["prompt_tokens", "completion_tokens"] {
            if let Some(value) = nested.get(key) {
                usage.insert(key.into(), value.clone());
            }
        }
    }
    if usage.is_empty() {
        None
    } else {
        Some(usage)
    }
}

fn as_u64(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    if let Some(n) = value.as_u64() {
        return Some(n);
    }
    if let Some(n) = value.as_i64() {
        return Some(n.max(0) as u64);
    }
    if let Some(s) = value.as_str() {
        return s.parse().ok();
    }
    None
}

