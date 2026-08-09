use super::models::InternEvent;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Stable Rust-core shape consumed by the journal adapter. Sequence ownership
/// remains with the local journal; `remote_sequence` is the mailbox cursor.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedInternEvent {
    pub event_id: String,
    pub source: String,
    pub kind: String,
    pub payload: Value,
    pub remote_sequence: u64,
    pub command_id: String,
    pub created_at: String,
    pub runtime_id: String,
    pub state_generation: u64,
}

pub fn normalize_event(event: InternEvent) -> NormalizedInternEvent {
    let mut payload = match event.payload {
        Value::Object(map) => map,
        value => {
            let mut map = Map::new();
            map.insert("value".into(), value);
            map
        }
    };
    let intern = payload
        .entry("intern")
        .or_insert_with(|| Value::Object(Map::new()));
    if !intern.is_object() {
        *intern = Value::Object(Map::new());
    }
    if let Some(intern) = intern.as_object_mut() {
        intern.insert("eventId".into(), Value::String(event.event_id.clone()));
        intern.insert(
            "runtimeKind".into(),
            serde_json::to_value(event.runtime_kind).expect("enum serializes"),
        );
        intern.insert("runtimeId".into(), Value::String(event.runtime_id.clone()));
        intern.insert(
            "stateGeneration".into(),
            Value::from(event.state_generation),
        );
    }
    NormalizedInternEvent {
        event_id: event.event_id,
        source: "intern".into(),
        kind: event.event_kind,
        payload: Value::Object(payload),
        remote_sequence: event.sequence,
        command_id: event.command_id,
        created_at: event.created_at,
        runtime_id: event.runtime_id,
        state_generation: event.state_generation,
    }
}
