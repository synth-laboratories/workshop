//! The pure recording gate.
//!
//! Given an event, the enablement state, and the host envelope, decide
//! whether it records and with which properties. No I/O: the store owns
//! persistence, this module owns the decision, and tests exercise it without
//! a database.

use anyhow::{anyhow, Result};
use serde_json::{Map, Value};

use super::contract::{self, EventSpec, Sensitivity};
use super::privacy;

pub enum Decision {
    Record {
        properties: Value,
    },
    /// Optional analytics are disabled; the event is silently not recorded.
    Drop,
}

pub fn gate(
    spec: &EventSpec,
    optional_enabled: bool,
    envelope: Map<String, Value>,
    properties: Value,
) -> Result<Decision> {
    if spec.sensitivity == Sensitivity::Optional && !optional_enabled {
        return Ok(Decision::Drop);
    }
    if let Some(field) = privacy::forbidden_field(&properties) {
        return Err(anyhow!(
            "telemetry payload refused: forbidden field {field}"
        ));
    }
    let Value::Object(incoming) = properties else {
        return Err(anyhow!("telemetry properties must be an object"));
    };
    let mut out = envelope;
    for (key, value) in incoming {
        if !contract::allowed_property(spec, &key) {
            return Err(anyhow!(
                "telemetry payload refused: property {key} is not allowlisted"
            ));
        }
        if matches!(value, Value::Object(_) | Value::Array(_)) {
            return Err(anyhow!(
                "telemetry payload refused: nested values are not allowed"
            ));
        }
        validate_property(&key, &value)?;
        out.insert(key, value);
    }
    Ok(Decision::Record {
        properties: Value::Object(out),
    })
}

fn validate_property(key: &str, value: &Value) -> Result<()> {
    match key {
        "outcome" => match value.as_str() {
            Some("success" | "failure" | "cancelled") => Ok(()),
            _ => Err(anyhow!(
                "telemetry payload refused: outcome must be success, failure, or cancelled"
            )),
        },
        "duration_ms" if value.as_u64().is_none() => Err(anyhow!(
            "telemetry payload refused: duration_ms must be a non-negative integer"
        )),
        "workflow_family" | "error_class" if value.as_str().is_none() => Err(anyhow!(
            "telemetry payload refused: property {key} must be a string"
        )),
        _ => Ok(()),
    }
}

