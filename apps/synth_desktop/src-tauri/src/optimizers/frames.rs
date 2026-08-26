//! Durable native-frame lane for optimizer runs.
//!
//! Telemetry events are small, replayable state. PNGs are media. Keeping those
//! two lanes separate prevents the shared event subscription, React state, and
//! every open visual surface from retaining an ever-growing base64 history.

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::storage::ContentStore;

use super::models::OptimizerEventEnvelope;

pub const OPTIMIZER_FRAME_DELTA_SCHEMA_VERSION: &str = "optimizer_frame_delta.v1";
pub const OPTIMIZER_FRAME_REF_SCHEMA_VERSION: &str = "optimizer_frame_ref.v1";
const PNG_DATA_URL_PREFIX: &str = "data:image/png;base64,";
const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
const MAX_RUN_FRAME_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerFrameRef {
    pub schema_version: String,
    pub optimizer_run_id: String,
    #[specta(type = specta_typescript::Number)]
    pub seed: i64,
    #[specta(type = specta_typescript::Number)]
    pub frame_sequence: u64,
    pub event_id: String,
    pub content_digest: String,
    pub content_type: String,
    #[specta(type = specta_typescript::Number)]
    pub size_bytes: u64,
    pub occurred_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerFrameDelta {
    pub schema_version: String,
    pub optimizer_run_id: String,
    #[specta(type = specta_typescript::Number)]
    pub after_frame_sequence: u64,
    #[specta(type = specta_typescript::Number)]
    pub frame_cursor: u64,
    #[specta(type = specta_typescript::Number)]
    pub observed_frames: u64,
    #[specta(type = specta_typescript::Number)]
    pub coalesced_frames: u64,
    pub frames: Vec<OptimizerFrameRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerFrameContent {
    pub frame: OptimizerFrameRef,
    /// Raw base64 without a data-URL prefix. The renderer adds the catalog's
    /// admitted content type and chunks it across the sandbox boundary.
    pub base64: String,
}

fn container_event(event: &OptimizerEventEnvelope, raw: bool) -> Option<&Map<String, Value>> {
    if raw {
        let object = event.raw.as_object()?;
        object
            .get("container_event")
            .or_else(|| object.get("containerEvent"))?
            .as_object()
    } else {
        event
            .delta
            .get("containerEvent")
            .or_else(|| event.delta.get("container_event"))?
            .as_object()
    }
}

fn mutable_container_event(
    event: &mut OptimizerEventEnvelope,
    raw: bool,
) -> Option<&mut Map<String, Value>> {
    if raw {
        let object = event.raw.as_object_mut()?;
        let value = if object.contains_key("container_event") {
            object.get_mut("container_event")
        } else {
            object.get_mut("containerEvent")
        }?;
        value.as_object_mut()
    } else {
        let value = if event.delta.contains_key("containerEvent") {
            event.delta.get_mut("containerEvent")
        } else {
            event.delta.get_mut("container_event")
        }?;
        value.as_object_mut()
    }
}

fn frame_body(event: &OptimizerEventEnvelope) -> Option<(i64, String)> {
    for raw in [false, true] {
        let Some(container) = container_event(event, raw) else {
            continue;
        };
        let Some(seed) = container.get("seed").and_then(Value::as_i64) else {
            continue;
        };
        let Some(frame) = container.get("frame").and_then(Value::as_object) else {
            continue;
        };
        let Some(data_url) = frame
            .get("data_url")
            .or_else(|| frame.get("dataUrl"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if data_url.starts_with(PNG_DATA_URL_PREFIX) {
            return Some((seed, data_url.to_string()));
        }
    }
    None
}

fn rewrite_frame_metadata(event: &mut OptimizerEventEnvelope, metadata: &Value) {
    for raw in [false, true] {
        let Some(container) = mutable_container_event(event, raw) else {
            continue;
        };
        let Some(frame) = container.get_mut("frame").and_then(Value::as_object_mut) else {
            continue;
        };
        frame.remove("data_url");
        frame.remove("dataUrl");
        frame.insert("ref".into(), metadata.clone());
    }
}

fn strip_inline_frame(event: &mut OptimizerEventEnvelope) {
    for raw in [false, true] {
        let Some(container) = mutable_container_event(event, raw) else {
            continue;
        };
        if let Some(frame) = container.get_mut("frame").and_then(Value::as_object_mut) {
            frame.remove("data_url");
            frame.remove("dataUrl");
        }
    }
}

fn record_rejected_frame(conn: &Connection, run_id: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO optimizer_frame_usage(
            optimizer_run_id,retained_frames,retained_bytes,rejected_frames,updated_at
         ) VALUES (?1,0,0,1,?2)
         ON CONFLICT(optimizer_run_id) DO UPDATE SET
            rejected_frames=rejected_frames+1,
            updated_at=excluded.updated_at",
        params![run_id, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

fn decode_png(data_url: &str) -> Result<Vec<u8>> {
    let encoded = data_url
        .strip_prefix(PNG_DATA_URL_PREFIX)
        .ok_or_else(|| anyhow!("frame is not a PNG data URL"))?;
    // Refuse before allocating the decoded body. Base64 expands by 4/3.
    if encoded.len() > (MAX_FRAME_BYTES * 4 / 3) + 8 {
        bail!("frame exceeds the {MAX_FRAME_BYTES}-byte admission ceiling");
    }
    let bytes = STANDARD.decode(encoded).context("decode PNG frame")?;
    if bytes.len() > MAX_FRAME_BYTES {
        bail!("frame exceeds the {MAX_FRAME_BYTES}-byte admission ceiling");
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        bail!("frame data URL does not contain a PNG");
    }
    Ok(bytes)
}

fn frame_ref_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OptimizerFrameRef> {
    Ok(OptimizerFrameRef {
        schema_version: OPTIMIZER_FRAME_REF_SCHEMA_VERSION.into(),
        optimizer_run_id: row.get(0)?,
        seed: row.get(1)?,
        frame_sequence: row.get::<_, i64>(2)?.max(0) as u64,
        event_id: row.get(3)?,
        content_digest: row.get(4)?,
        content_type: row.get(5)?,
        size_bytes: row.get::<_, i64>(6)?.max(0) as u64,
        occurred_at: row.get(7)?,
    })
}

/// Move one admitted inline PNG into CAS and replace both public/raw copies by
/// the same immutable reference. Invalid or over-budget media is stripped and
/// marked on the event, but does not abort unrelated run telemetry.
pub(super) fn persist_event_frame(
    conn: &Connection,
    store: &ContentStore,
    event: &mut OptimizerEventEnvelope,
) -> Result<()> {
    let Some((seed, data_url)) = frame_body(event) else {
        strip_inline_frame(event);
        return Ok(());
    };
    let event_id = event
        .event_id
        .clone()
        .unwrap_or_else(|| format!("{}:{}", event.optimizer_run_id, event.sequence_number));
    let bytes = match decode_png(&data_url) {
        Ok(bytes) => bytes,
        Err(error) => {
            record_rejected_frame(conn, &event.optimizer_run_id)?;
            rewrite_frame_metadata(
                event,
                &json!({
                    "schemaVersion": OPTIMIZER_FRAME_REF_SCHEMA_VERSION,
                    "seed": seed,
                    "frameSequence": event.sequence_number,
                    "admission": "rejected",
                    "reason": error.to_string(),
                }),
            );
            return Ok(());
        }
    };
    let retained: i64 = conn.query_row(
        "SELECT COALESCE((SELECT retained_bytes FROM optimizer_frame_usage WHERE optimizer_run_id=?1), 0)",
        [&event.optimizer_run_id],
        |row| row.get(0),
    )?;
    if (retained.max(0) as u64).saturating_add(bytes.len() as u64) > MAX_RUN_FRAME_BYTES {
        record_rejected_frame(conn, &event.optimizer_run_id)?;
        rewrite_frame_metadata(
            event,
            &json!({
                "schemaVersion": OPTIMIZER_FRAME_REF_SCHEMA_VERSION,
                "seed": seed,
                "frameSequence": event.sequence_number,
                "admission": "rejected",
                "reason": "run frame budget exceeded",
            }),
        );
        return Ok(());
    }
    let digest = store.put_bytes("blobs", &bytes)?;
    let frame = OptimizerFrameRef {
        schema_version: OPTIMIZER_FRAME_REF_SCHEMA_VERSION.into(),
        optimizer_run_id: event.optimizer_run_id.clone(),
        seed,
        frame_sequence: event.sequence_number,
        event_id: event_id.clone(),
        content_digest: digest.clone(),
        content_type: "image/png".into(),
        size_bytes: bytes.len() as u64,
        occurred_at: event.occurred_at.clone(),
    };
    conn.execute(
        "INSERT INTO optimizer_frames(
            optimizer_run_id,seed,frame_sequence,event_id,content_digest,
            content_type,size_bytes,occurred_at,created_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            frame.optimizer_run_id,
            frame.seed,
            frame.frame_sequence as i64,
            frame.event_id,
            frame.content_digest,
            frame.content_type,
            frame.size_bytes as i64,
            frame.occurred_at,
            Utc::now().to_rfc3339(),
        ],
    )
    .with_context(|| format!("catalog optimizer frame {event_id}"))?;
    conn.execute(
        "INSERT INTO optimizer_frame_usage(
            optimizer_run_id,retained_frames,retained_bytes,rejected_frames,updated_at
         ) VALUES (?1,1,?2,0,?3)
         ON CONFLICT(optimizer_run_id) DO UPDATE SET
            retained_frames=retained_frames+1,
            retained_bytes=retained_bytes+excluded.retained_bytes,
            updated_at=excluded.updated_at",
        params![
            event.optimizer_run_id,
            bytes.len() as i64,
            Utc::now().to_rfc3339()
        ],
    )?;
    rewrite_frame_metadata(
        event,
        &json!({
            "schemaVersion": frame.schema_version,
            "seed": frame.seed,
            "frameSequence": frame.frame_sequence,
            "eventId": frame.event_id,
            "contentDigest": digest,
            "contentType": "image/png",
            "sizeBytes": frame.size_bytes,
            "admission": "retained",
        }),
    );
    Ok(())
}

pub(super) fn latest(
    conn: &Connection,
    optimizer_run_id: &str,
    after_frame_sequence: u64,
) -> Result<OptimizerFrameDelta> {
    let (cursor, observed): (i64, i64) = conn.query_row(
        "SELECT COALESCE(MAX(frame_sequence), ?2), COUNT(*)
         FROM optimizer_frames
         WHERE optimizer_run_id=?1 AND frame_sequence>?2",
        params![optimizer_run_id, after_frame_sequence as i64],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let mut statement = conn.prepare(
        "WITH ranked AS (
           SELECT optimizer_run_id,seed,frame_sequence,event_id,content_digest,
                  content_type,size_bytes,occurred_at,
                  ROW_NUMBER() OVER (PARTITION BY seed ORDER BY frame_sequence DESC) AS rank
           FROM optimizer_frames
           WHERE optimizer_run_id=?1 AND frame_sequence>?2
         )
         SELECT optimizer_run_id,seed,frame_sequence,event_id,content_digest,
                content_type,size_bytes,occurred_at
         FROM ranked WHERE rank=1 ORDER BY frame_sequence ASC",
    )?;
    let rows = statement.query_map(
        params![optimizer_run_id, after_frame_sequence as i64],
        frame_ref_from_row,
    )?;
    let frames = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(OptimizerFrameDelta {
        schema_version: OPTIMIZER_FRAME_DELTA_SCHEMA_VERSION.into(),
        optimizer_run_id: optimizer_run_id.into(),
        after_frame_sequence,
        frame_cursor: cursor.max(after_frame_sequence as i64) as u64,
        observed_frames: observed.max(0) as u64,
        coalesced_frames: (observed.max(0) as usize).saturating_sub(frames.len()) as u64,
        frames,
    })
}

pub(super) fn list(
    conn: &Connection,
    optimizer_run_id: &str,
    seed: i64,
    before_frame_sequence: Option<u64>,
    limit: i64,
) -> Result<Vec<OptimizerFrameRef>> {
    let before = before_frame_sequence
        .unwrap_or(i64::MAX as u64)
        .min(i64::MAX as u64) as i64;
    let mut statement = conn.prepare(
        "SELECT optimizer_run_id,seed,frame_sequence,event_id,content_digest,
                content_type,size_bytes,occurred_at
         FROM optimizer_frames
         WHERE optimizer_run_id=?1 AND seed=?2 AND frame_sequence<?3
         ORDER BY frame_sequence DESC LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![optimizer_run_id, seed, before, limit.clamp(1, 500)],
        frame_ref_from_row,
    )?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub(super) fn content(
    conn: &Connection,
    store: &ContentStore,
    optimizer_run_id: &str,
    seed: i64,
    frame_sequence: u64,
) -> Result<OptimizerFrameContent> {
    let frame = conn
        .query_row(
            "SELECT optimizer_run_id,seed,frame_sequence,event_id,content_digest,
                    content_type,size_bytes,occurred_at
             FROM optimizer_frames
             WHERE optimizer_run_id=?1 AND seed=?2 AND frame_sequence=?3",
            params![optimizer_run_id, seed, frame_sequence as i64],
            frame_ref_from_row,
        )
        .optional()?
        .ok_or_else(|| anyhow!("optimizer frame was not found"))?;
    let bytes = store.get_bytes("blobs", &frame.content_digest)?;
    if bytes.len() as u64 != frame.size_bytes {
        bail!("optimizer frame size does not match its catalog entry");
    }
    Ok(OptimizerFrameContent {
        frame,
        base64: STANDARD.encode(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use tempfile::tempdir;

    fn data_url(body: &[u8]) -> String {
        format!("{PNG_DATA_URL_PREFIX}{}", STANDARD.encode(body))
    }

    fn insert_run(conn: &Connection, run_id: &str) -> Result<()> {
        conn.execute(
            "INSERT INTO optimizer_runs(
                id,algorithm_id,algorithm_version,status,source,created_at,
                capabilities_json,bindings_json,input_refs_json,
                output_refs_json,visual_refs_json,summary_json,usage_json,
                payload_json,updated_at
             ) VALUES (?1,'eval','1','running','local','now','{}','[]','[]','[]','[]','{}','{}','{}','now')",
            [run_id],
        )?;
        Ok(())
    }

    #[test]
    fn frame_lane_coalesces_latest_and_reads_history_lazily() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let store = ContentStore::new(storage.content_root());
        storage.database().with_conn(|conn| {
            insert_run(conn, "run-1")?;
            for (sequence, seed, marker) in [(1, 91001, 1u8), (2, 91001, 2), (3, 91002, 3)] {
                let body = [b"\x89PNG\r\n\x1a\n".as_slice(), &[marker]].concat();
                let mut event = OptimizerEventEnvelope {
                    schema_version: "optimizer_event.v1".into(),
                    event_id: Some(format!("event-{sequence}")),
                    event_type: "eval.trial.event".into(),
                    sequence_number: sequence,
                    occurred_at: format!("2026-08-26T00:00:0{sequence}Z"),
                    optimizer_run_id: "run-1".into(),
                    algorithm_id: "eval".into(),
                    level: None,
                    item: None,
                    delta: json!({"containerEvent":{"seed":seed,"frame":{"data_url":data_url(&body)}}}).as_object().unwrap().clone(),
                    snapshot: None,
                    usage_delta: None,
                    artifact_refs: vec![],
                    error: None,
                    raw: json!({"container_event":{"seed":seed,"frame":{"data_url":data_url(&body)}}}),
                };
                persist_event_frame(conn, &store, &mut event)?;
                assert!(event.raw.pointer("/container_event/frame/data_url").is_none());
                assert_eq!(event.raw.pointer("/container_event/frame/ref/admission"), Some(&json!("retained")));
            }
            let delta = latest(conn, "run-1", 0)?;
            assert_eq!(delta.frame_cursor, 3);
            assert_eq!(delta.observed_frames, 3);
            assert_eq!(delta.coalesced_frames, 1);
            assert_eq!(delta.frames.iter().map(|frame| (frame.seed, frame.frame_sequence)).collect::<Vec<_>>(), vec![(91001, 2), (91002, 3)]);
            let history = list(conn, "run-1", 91001, None, 10)?;
            assert_eq!(history.iter().map(|frame| frame.frame_sequence).collect::<Vec<_>>(), vec![2, 1]);
            let loaded = content(conn, &store, "run-1", 91001, 1)?;
            assert!(STANDARD.decode(loaded.base64).unwrap().starts_with(b"\x89PNG"));
            Ok(())
        }).unwrap();
    }

    #[test]
    fn ten_seed_burst_is_coalesced_to_ten_bounded_reads() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let store = ContentStore::new(storage.content_root());
        storage.database().with_conn(|conn| {
            insert_run(conn, "run-burst")?;
            for sequence in 1..=250u64 {
                let seed = 91_001 + ((sequence - 1) % 10) as i64;
                let marker = (sequence % 251) as u8;
                let body = [b"\x89PNG\r\n\x1a\n".as_slice(), &[marker]].concat();
                let mut event = OptimizerEventEnvelope {
                    schema_version: "optimizer_event.v1".into(),
                    event_id: Some(format!("burst-{sequence}")),
                    event_type: "eval.trial.event".into(),
                    sequence_number: sequence,
                    occurred_at: format!("2026-08-26T00:{:02}:{:02}Z", sequence / 60, sequence % 60),
                    optimizer_run_id: "run-burst".into(),
                    algorithm_id: "eval".into(),
                    level: None,
                    item: None,
                    delta: json!({"containerEvent":{"seed":seed,"frame":{"data_url":data_url(&body)}}}).as_object().unwrap().clone(),
                    snapshot: None,
                    usage_delta: None,
                    artifact_refs: vec![],
                    error: None,
                    raw: json!({"container_event":{"seed":seed,"frame":{"data_url":data_url(&body)}}}),
                };
                persist_event_frame(conn, &store, &mut event)?;
            }

            let delta = latest(conn, "run-burst", 0)?;
            assert_eq!(delta.frame_cursor, 250);
            assert_eq!(delta.observed_frames, 250);
            assert_eq!(delta.coalesced_frames, 240);
            assert_eq!(delta.frames.len(), 10);
            assert_eq!(delta.frames.iter().map(|frame| frame.seed).collect::<Vec<_>>(), (91_001..=91_010).collect::<Vec<_>>());
            assert!(delta.frames.iter().all(|frame| frame.frame_sequence > 240));

            let caught_up = latest(conn, "run-burst", delta.frame_cursor)?;
            assert_eq!(caught_up.frame_cursor, 250);
            assert_eq!(caught_up.observed_frames, 0);
            assert!(caught_up.frames.is_empty());
            assert_eq!(list(conn, "run-burst", 91_001, None, 500)?.len(), 25);
            Ok(())
        }).unwrap();
    }
}
