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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn persist_dedupes_and_reopens_the_same_sequences() {
        let dir = tempdir().unwrap();
        let store = ContentStore::new(dir.path());
        let log = json!({
            "events": [
                {"kind": "stream.subscribed", "event_id": "sub", "payload": {"ready": true}},
                {"kind": "snapshot", "event_id": "e1", "sequence": 1, "payload": {"reward": 1.5}},
                {"kind": "snapshot", "event_id": "e1", "sequence": 1, "payload": {"reward": 1.5}},
                {"kind": "snapshot", "event_id": "e2", "sequence": 2, "payload": {}}
            ]
        });
        let first = persist_live_envelopes(
            &store,
            Some("stream_r1"),
            Some("r1"),
            envelopes_from_event_log(&log),
        )
        .unwrap();
        assert_eq!(first.envelopes.len(), 3);
        let again = persist_live_envelopes(
            &store,
            Some("stream_r1"),
            Some("r1"),
            envelopes_from_event_log(&log),
        )
        .unwrap();
        assert_eq!(first.digest, again.digest);
        let loaded = load_live_spool(&store, &first.digest).unwrap();
        assert_eq!(loaded.stream_id.as_deref(), Some("stream_r1"));
        assert_eq!(
            loaded
                .envelopes
                .iter()
                .filter_map(|e| e.get("event_id").and_then(Value::as_str))
                .collect::<Vec<_>>(),
            vec!["sub", "e1", "e2"]
        );
    }

    #[test]
    fn a_multiplexed_run_spools_every_lane() {
        // The defect this file used to have. Ten rollouts each restart at
        // sequence 1 and each carry `event_id: "1"`; a bare `event_id`
        // identity persists one of them and leaves the aggregate lane count
        // looking valid, so nothing downstream can tell the evidence is gone.
        let dir = tempdir().unwrap();
        let store = ContentStore::new(dir.path());
        let events: Vec<Value> = (0..10)
            .map(|seed| {
                json!({
                    "kind": "snapshot",
                    "event_id": "1",
                    "sequence": 1,
                    "rollout_id": format!("seed-{seed}"),
                    "lane": format!("seed-{seed}"),
                })
            })
            .collect();
        let spool = persist_live_envelopes(&store, Some("stream_r1"), None, events).unwrap();
        assert_eq!(
            spool.envelopes.len(),
            10,
            "one lane per seed, none collapsed"
        );

        // A genuine reconnect replay of one lane still collapses exactly once.
        let mut replayed = spool.envelopes.clone();
        replayed.push(spool.envelopes[0].clone());
        let again = persist_live_envelopes(&store, Some("stream_r1"), None, replayed).unwrap();
        assert_eq!(again.envelopes.len(), 10);
        assert_eq!(again.digest, spool.digest);
    }

    #[test]
    fn a_legacy_spool_still_loads_under_its_own_schema() {
        // A `…v1` spool was deduplicated under the old identity rule. Refusing
        // to read evidence already captured helps nobody; claiming it is the
        // same artifact a `…v2` spool would be is what the version prevents.
        let dir = tempdir().unwrap();
        let store = ContentStore::new(dir.path());
        let body = json!({
            "schema": LIVE_SPOOL_SCHEMA_V1,
            "stream_id": "stream_r1",
            "rollout_id": null,
            "envelopes": [{"kind": "snapshot", "event_id": "1", "sequence": 1}],
        });
        let digest = store
            .put_bytes("traces", &serde_json::to_vec(&body).unwrap())
            .unwrap();
        let loaded = load_live_spool(&store, &digest).unwrap();
        assert_eq!(loaded.schema, LIVE_SPOOL_SCHEMA_V1);
        assert_eq!(loaded.envelopes.len(), 1);
    }

    #[test]
    fn spool_digest_replays_after_engine_gone_without_ascii_placeholder() {
        let dir = tempdir().unwrap();
        let store = ContentStore::new(dir.path());
        let log = json!({
            "events": [
                {"kind": "stream.subscribed", "event_id": "sub", "payload": {"ready": true}},
                {"kind": "frame", "event_id": "png", "sequence": 1, "payload": {"format": "png", "url": "/rollouts/r1/frames/1.png"}},
                {"kind": "frame", "event_id": "missing", "sequence": 2, "payload": {"format": "png"}}
            ]
        });
        let spool = persist_live_envelopes(
            &store,
            Some("stream_r1"),
            Some("r1"),
            envelopes_from_event_log(&log),
        )
        .unwrap();
        let loaded = load_live_spool(&store, &spool.digest).unwrap();
        assert_eq!(loaded.digest, spool.digest);
        assert_eq!(loaded.envelopes.len(), 3);
        let missing = replay_frame_from_envelope(&loaded.envelopes[2]).unwrap();
        assert_eq!(missing["kind"], "unavailable_image");
        assert!(missing["ascii"].is_null());
        let present = replay_frame_from_envelope(&loaded.envelopes[1]).unwrap();
        assert_eq!(present["kind"], "png");
        assert!(present["ascii"].is_null());
    }

    #[test]
    fn persist_refuses_bearer_token_in_envelopes() {
        let dir = tempdir().unwrap();
        let store = ContentStore::new(dir.path());
        let err = persist_live_envelopes(
            &store,
            Some("stream_r1"),
            Some("r1"),
            vec![
                json!({"kind": "observation", "payload": {"text": "Authorization: Bearer secret"}}),
            ],
        )
        .unwrap_err();
        assert!(err.to_string().contains("token"));
    }
}
