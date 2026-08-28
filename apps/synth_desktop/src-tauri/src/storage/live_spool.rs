//! Persist-raw live-eval envelopes to Desktop CAS before renderer publish.
//!
//! Replay from the digest after the engine is gone (C7-W02 / W0). Duplicate
//! identities are dropped. Control records are stored; visuals decide ready.
//!
//! # What is stored, and what decides "duplicate"
//!
//! Identity is [`crate::stream_fold::envelope_identity`] — the same rule the
//! renderer's fold and the host's receipt use, and the reason this file no
//! longer has one of its own. The rule it used to carry treated a bare
//! `event_id` as globally unique, which is wrong for every multiplexed run: a
//! ten-lane eval legitimately carries ten `event_id: "1"` records, so the
//! spool persisted one lane and the aggregate count still looked right. The
//! renderer's ingest had a comment warning about exactly this bug while the
//! spool committed it.
//!
//! That is why the schema is versioned rather than reinterpreted. A `…v1`
//! spool was deduplicated under the old rule and a multiplexed one may be
//! lane-collapsed; it still loads, because refusing to read evidence already
//! captured helps nobody, but it is not the same artifact a `…v2` spool of the
//! same stream would be.

use super::ContentStore;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

/// Schema written by [`persist_live_envelopes`].
pub const LIVE_SPOOL_SCHEMA: &str = "synth.live-eval-spool.v2";

/// Spools written before identity had one home. Readable, and lane-collapsed
/// for any multiplexed run: see the module header.
pub const LIVE_SPOOL_SCHEMA_V1: &str = "synth.live-eval-spool.v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LiveSpool {
    pub schema: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollout_id: Option<String>,
    pub envelopes: Vec<Value>,
    #[serde(skip)]
    pub digest: String,
}

pub fn envelopes_from_event_log(log: &Value) -> Vec<Value> {
    if let Some(arr) = log.as_array() {
        return arr.clone();
    }
    if let Some(arr) = log.get("events").and_then(Value::as_array) {
        return arr.clone();
    }
    if log.is_object() && log.as_object().is_some_and(|obj| !obj.is_empty()) {
        return vec![log.clone()];
    }
    Vec::new()
}

pub fn persist_live_envelopes(
    store: &ContentStore,
    stream_id: Option<&str>,
    rollout_id: Option<&str>,
    envelopes: impl IntoIterator<Item = Value>,
) -> Result<LiveSpool> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for (index, envelope) in envelopes.into_iter().enumerate() {
        // One identity rule, in one place. The ordinal is the delivery
        // position and is consulted only for an envelope that carries no
        // identity of its own at all.
        let scope = crate::stream_fold::envelope_scope(&envelope);
        let id = crate::stream_fold::envelope_identity(&envelope, &scope, index as u64 + 1);
        if !seen.insert(id) {
            continue;
        }
        unique.push(envelope);
    }
    crate::visuals::assert_no_live_secrets(&json!({ "envelopes": unique }))?;
    let body = json!({
        "schema": LIVE_SPOOL_SCHEMA,
        "stream_id": stream_id,
        "rollout_id": rollout_id,
        "envelopes": unique,
    });
    let bytes = serde_json::to_vec(&body).context("serialize live eval spool")?;
    let digest = store.put_bytes("traces", &bytes)?;
    Ok(LiveSpool {
        schema: LIVE_SPOOL_SCHEMA.into(),
        stream_id: stream_id.map(str::to_string),
        rollout_id: rollout_id.map(str::to_string),
        envelopes: unique,
        digest,
    })
}

pub fn load_live_spool(store: &ContentStore, digest: &str) -> Result<LiveSpool> {
    let bytes = store.get_bytes("traces", digest)?;
    let mut spool: LiveSpool = serde_json::from_slice(&bytes).context("parse live eval spool")?;
    if spool.schema != LIVE_SPOOL_SCHEMA && spool.schema != LIVE_SPOOL_SCHEMA_V1 {
        bail!("unsupported live eval spool schema: {}", spool.schema);
    }
    spool.digest = digest.to_string();
    crate::visuals::assert_no_live_secrets(&json!({ "envelopes": spool.envelopes }))?;
    Ok(spool)
}

/// Replay a frame envelope after the engine is gone. A missing PNG stays
/// unavailable — never an ASCII placeholder.
pub fn replay_frame_from_envelope(envelope: &Value) -> Result<Value> {
    let kind = envelope
        .get("kind")
        .or_else(|| envelope.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if kind != "frame" {
        bail!("replay_frame_from_envelope requires a frame envelope");
    }
    let payload = envelope.get("payload").unwrap_or(envelope);
    let url = payload
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty());
    let format = payload
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let png_advertised = format == "png"
        || url.is_some_and(|value| value.contains(".png") || value.starts_with("data:image/png"));
    if png_advertised && url.is_none() {
        return Ok(json!({
            "kind": "unavailable_image",
            "ascii": Value::Null,
            "url": Value::Null,
            "reason": "missing_png"
        }));
    }
    if png_advertised {
        return Ok(json!({
            "kind": "png",
            "url": url,
            "ascii": Value::Null
        }));
    }
    let ascii = payload
        .get("text")
        .or_else(|| payload.get("ascii"))
        .or_else(|| payload.get("grid"))
        .and_then(Value::as_str);
    Ok(json!({
        "kind": if ascii.is_some() { "ascii" } else { "not_emitted" },
        "url": Value::Null,
        "ascii": ascii
    }))
}

