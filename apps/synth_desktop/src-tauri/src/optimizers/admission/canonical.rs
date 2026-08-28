//! Canonical JSON and content digests.
//!
//! An approval receipt binds a digest, and execution refuses to run anything
//! whose digest differs. That only works if the same specification always
//! serializes to the same bytes — so the encoding here is defined, not
//! inherited from whatever key order a `serde_json::Map` happened to have.
//!
//! Rules:
//!
//! * Object keys are sorted by their Unicode scalar sequence, recursively.
//! * No insignificant whitespace.
//! * Integers render as integers; floats render in the shortest form that
//!   round-trips, with `-0.0` folded to `0`.
//! * `NaN` and infinities are refused rather than encoded, because they have no
//!   JSON spelling and would otherwise become `null` in silence.
//!
//! Workshop's own specification fields deliberately use integer micros and
//! `NonZero` counts, so floats only ever appear inside an operator-supplied
//! policy configuration. They are still defined here rather than left to
//! chance.

use super::ids::Digest;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};
use std::fmt;

/// JSON that has been canonicalized. The inner `Value` is kept for inspection;
/// the canonical bytes are what any digest is taken over.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CanonicalJson {
    value: Value,
}

impl CanonicalJson {
    /// Canonicalize an arbitrary value. Fails only on values JSON cannot
    /// represent, which must not be silently coerced.
    pub fn new(value: Value) -> Result<Self, CanonicalError> {
        Ok(Self {
            value: canonicalize(value)?,
        })
    }

    /// An empty object. Used where a specification field is structurally
    /// required but the declaration genuinely carries no configuration — which
    /// is different from the field being absent.
    pub fn empty_object() -> Self {
        Self {
            value: Value::Object(Map::new()),
        }
    }

    pub fn as_value(&self) -> &Value {
        &self.value
    }

    pub fn into_value(self) -> Value {
        self.value
    }

    pub fn is_empty_object(&self) -> bool {
        self.value.as_object().is_some_and(Map::is_empty)
    }

    /// The canonical byte encoding. This is the only input a digest is ever
    /// taken over.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        write_canonical(&self.value, &mut buffer);
        buffer
    }

    pub fn to_canonical_string(&self) -> String {
        // `write_canonical` only ever emits UTF-8.
        String::from_utf8(self.to_bytes()).expect("canonical JSON is UTF-8")
    }

    /// Digest over the canonical bytes.
    pub fn digest(&self) -> Digest {
        digest_bytes(&self.to_bytes())
    }
}

impl fmt::Display for CanonicalJson {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_canonical_string())
    }
}

impl<'de> Deserialize<'de> for CanonicalJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Digest arbitrary bytes with the one algorithm this module admits.
pub fn digest_bytes(bytes: &[u8]) -> Digest {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let output = hasher.finalize();
    let mut raw = [0u8; 32];
    raw.copy_from_slice(&output);
    Digest::from_sha256(raw)
}

/// Digest a serializable value by canonicalizing it first. Every digest in the
/// admission pipeline goes through here so that two structurally identical
/// specifications can never disagree because of field order.
pub fn digest_of<T: Serialize>(value: &T) -> Result<Digest, CanonicalError> {
    let raw = serde_json::to_value(value).map_err(CanonicalError::Serialize)?;
    Ok(CanonicalJson::new(raw)?.digest())
}

fn canonicalize(value: Value) -> Result<Value, CanonicalError> {
    Ok(match value {
        Value::Object(map) => {
            // `serde_json::Map` may or may not preserve insertion order
            // depending on features enabled anywhere in the dependency graph.
            // Sorting explicitly means the canonical form does not depend on
            // that.
            let mut sorted: Vec<(String, Value)> = map.into_iter().collect();
            sorted.sort_by(|left, right| left.0.cmp(&right.0));
            let mut rebuilt = Map::new();
            for (key, entry) in sorted {
                rebuilt.insert(key, canonicalize(entry)?);
            }
            Value::Object(rebuilt)
        }
        // Array order is data, never sorted.
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(canonicalize)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Number(number) => {
            if let Some(float) = number.as_f64() {
                if !float.is_finite() {
                    return Err(CanonicalError::NonFiniteNumber);
                }
            }
            Value::Number(number)
        }
        other => other,
    })
}

fn write_canonical(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(number) => out.extend_from_slice(render_number(number).as_bytes()),
        Value::String(text) => write_json_string(text, out),
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        Value::Object(map) => {
            out.push(b'{');
            // `canonicalize` already sorted these; sort again so that a
            // hand-built `CanonicalJson` cannot bypass the ordering rule.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_json_string(key, out);
                out.push(b':');
                write_canonical(&map[key], out);
            }
            out.push(b'}');
        }
    }
}

fn render_number(number: &serde_json::Number) -> String {
    if let Some(value) = number.as_u64() {
        return value.to_string();
    }
    if let Some(value) = number.as_i64() {
        return value.to_string();
    }
    match number.as_f64() {
        // `-0.0` and `0.0` are the same quantity; rendering them differently
        // would make two equal specifications digest differently.
        Some(value) if value == 0.0 => "0".to_string(),
        // Rust's shortest round-tripping float formatting. A float that is
        // exactly integral still renders with its fraction so it cannot be
        // confused with an integer field.
        Some(value) => {
            let rendered = format!("{value}");
            if rendered.contains(['.', 'e', 'E']) {
                rendered
            } else {
                format!("{rendered}.0")
            }
        }
        None => number.to_string(),
    }
}

fn write_json_string(text: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for character in text.chars() {
        match character {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\u{08}' => out.extend_from_slice(b"\\b"),
            '\u{0c}' => out.extend_from_slice(b"\\f"),
            control if (control as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", control as u32).as_bytes());
            }
            other => {
                let mut buffer = [0u8; 4];
                out.extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
            }
        }
    }
    out.push(b'"');
}

#[derive(Debug)]
pub enum CanonicalError {
    NonFiniteNumber,
    Serialize(serde_json::Error),
}

impl fmt::Display for CanonicalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteNumber => formatter.write_str(
                "a NaN or infinite number has no JSON encoding and must not be canonicalized to null",
            ),
            Self::Serialize(error) => write!(formatter, "value is not serializable: {error}"),
        }
    }
}

impl std::error::Error for CanonicalError {}

