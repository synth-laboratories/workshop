//! Consumer gate for the Optimizers-owned optimizer event wire contract.
//!
//! Workshop's durable model has a camelCase IPC envelope and additional local
//! fields. It must not become a second owner of the optimizer vocabulary. At
//! the append boundary we therefore project imported eval-worker events and
//! relayed eval carriers into Optimizers' snake_case `optimizer_event.v1`,
//! validate that projection against the vendored owner schema, and keep
//! Workshop-local event families on their existing path.

use std::sync::OnceLock;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};

use super::models::OptimizerEventEnvelope;

const OWNER_SCHEMA_JSON: &str =
    include_str!("../../../../../contracts/optimizer-event-v1/schema.json");

/// Removal flag for the one-release read window promised by the owner schema.
/// Producers never write this spelling. Delete the alias reads and this flag
/// after the release following 2026-08, in lockstep with Optimizers.
pub(super) const COMPAT_CONTAINER_EVENT_CAMEL_CASE_THROUGH: &str = "release-after-2026-08";

static OWNER_SCHEMA: OnceLock<std::result::Result<Value, String>> = OnceLock::new();

fn owner_schema() -> Result<&'static Value> {
    OWNER_SCHEMA
        .get_or_init(|| serde_json::from_str(OWNER_SCHEMA_JSON).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| anyhow!("vendored Optimizers event schema is invalid JSON: {error}"))
}

fn is_owner_controlled_eval_event(event: &OptimizerEventEnvelope) -> bool {
    if event.algorithm_id != "eval" {
        return false;
    }
    // Every carrier is cross-repo by definition. The persisted-worker path
    // additionally preserves the producer schema in `raw`, which lets the
    // rest of its eval.* vocabulary be gated without sweeping in Workshop's
    // locally authored eval orchestration.
    event.event_type == "eval.trial.event"
        || (event.event_type.starts_with("eval.")
            && event.raw.get("schema_version").and_then(Value::as_str)
                == Some("eval.worker-event.v1"))
}

/// Normalize and validate the owner-controlled slice immediately before any
/// row is inserted. This mutates only the carrier alias inside `delta`.
pub(super) fn normalize_and_validate_imported_eval_events(
    events: &mut [OptimizerEventEnvelope],
) -> Result<()> {
    for event in events {
        if !is_owner_controlled_eval_event(event) {
            continue;
        }
        if event.event_type == "eval.trial.event" {
            normalize_container_event(event)?;
        }
        let owner_wire = owner_wire_projection(event);
        validate_owner_optimizer_event(&owner_wire).with_context(|| {
            format!(
                "validate imported Optimizers event {} at sequence {}",
                event.event_type, event.sequence_number
            )
        })?;
    }
    Ok(())
}

fn normalize_container_event(event: &mut OptimizerEventEnvelope) -> Result<()> {
    let legacy = event.delta.remove("containerEvent");
    match (event.delta.get("container_event"), legacy) {
        (Some(canonical), Some(legacy)) if canonical != &legacy => {
            bail!("eval.trial.event has conflicting delta.container_event and delta.containerEvent")
        }
        (Some(_), Some(_)) | (Some(_), None) => {}
        (None, Some(legacy)) => {
            event.delta.insert("container_event".into(), legacy);
        }
        (None, None) => bail!("eval.trial.event is missing canonical delta.container_event"),
    }
    Ok(())
}

/// Explicit mapping between Workshop storage/IPC names and the owner wire.
/// Do not serialize `OptimizerEventEnvelope` and rename opportunistically: its
/// camelCase serde shape is an independent renderer contract.
fn owner_wire_projection(event: &OptimizerEventEnvelope) -> Value {
    json!({
        "schema_version": event.schema_version,
        "type": event.event_type,
        "sequence_number": event.sequence_number,
        "run_id": event.optimizer_run_id,
        "algorithm_id": event.algorithm_id,
        "slot": "optimizer_run",
        "created_at": event.occurred_at,
        "item": owner_item_projection(event.item.as_ref()),
        "delta": event.delta,
        "error": event.error,
        "raw": event.raw,
    })
}

fn owner_item_projection(item: Option<&Value>) -> Value {
    let Some(Value::Object(item)) = item else {
        return Value::Null;
    };
    let mut normalized = item.clone();
    if let Some(kind) = item
        .get("type")
        .or_else(|| item.get("kind"))
        .and_then(Value::as_str)
    {
        // Workshop calls an evaluated episode a trial; the owner envelope's
        // equivalent item noun is rollout.
        let owner_kind = match kind {
            "trial" => "rollout",
            "frontierCell" => "frontier_cell",
            value => value,
        };
        normalized.insert("type".into(), json!(owner_kind));
    }
    Value::Object(normalized)
}

/// Focused draft-2020-12 evaluator for the owner schema branch Workshop can
/// append. Values (required fields, consts, enums, minima, and carrier rules)
/// are read from the vendored schema, so owner changes fail this consumer gate
/// instead of silently drifting. Workshop deliberately does not implement a
/// general JSON Schema engine here.
fn validate_owner_optimizer_event(event: &Value) -> Result<()> {
    let schema = owner_schema()?;
    let contract = schema
        .pointer("/$defs/optimizer_event")
        .context("owner schema has no $defs.optimizer_event")?;
    let object = event.as_object().context("owner event is not an object")?;

    for name in string_array(contract.get("required"), "optimizer_event.required")? {
        if !object.contains_key(name) {
            bail!("owner optimizer_event requires {name}");
        }
    }

    let properties = contract
        .get("properties")
        .and_then(Value::as_object)
        .context("owner optimizer_event has no properties")?;
    require_const(object, properties, "schema_version")?;
    require_non_empty_string(object, "run_id")?;
    require_non_empty_string(object, "created_at")?;

    let event_type = require_non_empty_string(object, "type")?;
    if !valid_event_type(event_type) {
        bail!("owner event type {event_type:?} violates $defs.event_type.pattern");
    }
    let declared = schema
        .pointer("/$defs/declared_event_types/enum")
        .and_then(Value::as_array)
        .context("owner schema has no declared event vocabulary")?;
    if !declared
        .iter()
        .any(|value| value.as_str() == Some(event_type))
    {
        bail!("owner event type {event_type:?} is not declared by Optimizers");
    }

    let algorithm = require_non_empty_string(object, "algorithm_id")?;
    let algorithms = properties
        .get("algorithm_id")
        .and_then(|value| value.get("enum"))
        .and_then(Value::as_array)
        .context("owner schema has no algorithm_id enum")?;
    if !algorithms
        .iter()
        .any(|value| value.as_str() == Some(algorithm))
    {
        bail!("owner schema does not admit algorithm_id {algorithm:?}");
    }

    let sequence = object
        .get("sequence_number")
        .and_then(Value::as_u64)
        .context("owner sequence_number is not a non-negative integer")?;
    let minimum = properties
        .get("sequence_number")
        .and_then(|value| value.get("minimum"))
        .and_then(Value::as_u64)
        .context("owner schema has no sequence_number minimum")?;
    if sequence < minimum {
        bail!("owner sequence_number {sequence} is below {minimum}");
    }

    if object.get("slot").and_then(Value::as_str).is_none() {
        bail!("owner slot is not a string");
    }
    match object.get("item") {
        Some(Value::Null) | None => {}
        Some(Value::Object(item)) => validate_owner_item(schema, item)?,
        Some(_) => bail!("owner item is neither null nor an object"),
    }

    let delta = object
        .get("delta")
        .and_then(Value::as_object)
        .context("owner delta is not an object")?;
    if event_type == "eval.trial.event" {
        let container = delta
            .get("container_event")
            .and_then(Value::as_object)
            .context("owner eval.trial.event requires delta.container_event object")?;
        if container.is_empty() {
            // The owner schema permits an empty object, but rejecting it here
            // prevents Workshop from claiming it relayed an event it dropped.
            bail!("owner eval.trial.event delta.container_event is empty");
        }
        if delta.contains_key("containerEvent") {
            bail!("producer emitted deprecated delta.containerEvent alias");
        }
    }
    Ok(())
}

fn validate_owner_item(schema: &Value, item: &Map<String, Value>) -> Result<()> {
    let contract = schema
        .pointer("/$defs/optimizer_item")
        .context("owner schema has no $defs.optimizer_item")?;
    for name in string_array(contract.get("required"), "optimizer_item.required")? {
        if !item.contains_key(name) {
            bail!("owner optimizer item requires {name}");
        }
    }
    let kind = require_non_empty_string(item, "type")?;
    let admitted = contract
        .pointer("/properties/type/enum")
        .and_then(Value::as_array)
        .context("owner optimizer_item has no type enum")?;
    if !admitted.iter().any(|value| value.as_str() == Some(kind)) {
        bail!("owner optimizer item type {kind:?} is not admitted");
    }
    Ok(())
}

fn require_const(
    object: &Map<String, Value>,
    properties: &Map<String, Value>,
    name: &str,
) -> Result<()> {
    let expected = properties
        .get(name)
        .and_then(|value| value.get("const"))
        .with_context(|| format!("owner schema has no {name} const"))?;
    let actual = object
        .get(name)
        .with_context(|| format!("owner event has no {name}"))?;
    if actual != expected {
        bail!("owner {name} must be {expected}, got {actual}");
    }
    Ok(())
}

fn require_non_empty_string<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a str> {
    object
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .with_context(|| format!("owner {name} is not a non-empty string"))
}

fn string_array<'a>(value: Option<&'a Value>, label: &str) -> Result<Vec<&'a str>> {
    value
        .and_then(Value::as_array)
        .with_context(|| format!("owner schema has no {label}"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .with_context(|| format!("owner schema {label} contains a non-string"))
        })
        .collect()
}

fn valid_event_type(value: &str) -> bool {
    value.split('.').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    })
}

