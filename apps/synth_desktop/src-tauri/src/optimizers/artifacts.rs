//! Durable optimizer artifact declarations and bounded byte access.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use rusqlite::{params, Connection};
use serde_json::Value;

use super::models::{
    OptimizerArtifactPage, OptimizerArtifactRange, OptimizerEventEnvelope, OptimizerRunArtifact,
    OPTIMIZER_ARTIFACT_SCHEMA_VERSION,
};

const ARTIFACT_PAGE_SCHEMA_VERSION: &str = "optimizer_artifact_page.v1";
const ARTIFACT_RANGE_SCHEMA_VERSION: &str = "optimizer_artifact_range.v1";
const MAX_RANGE_BYTES: u64 = 1024 * 1024;
pub(super) const SAFE_ARTIFACT_MEDIA_TYPES: &[&str] =
    &["application/json", "image/png", "video/mp4"];

fn string_field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| {
        value
            .get(*name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

fn media_type(reference: &Value, locator: &str) -> Option<String> {
    string_field(
        reference,
        &["mediaType", "media_type", "contentType", "content_type"],
    )
    .map(str::to_ascii_lowercase)
    .or_else(|| {
        let path = locator.to_ascii_lowercase();
        if path.ends_with(".mp4") {
            Some("video/mp4".into())
        } else if path.ends_with(".json") || path.ends_with(".jsonl") {
            Some("application/json".into())
        } else if path.ends_with(".png") {
            Some("image/png".into())
        } else {
            None
        }
    })
}

fn event_work_item_id(event: &OptimizerEventEnvelope) -> Option<String> {
    event
        .item
        .as_ref()
        .and_then(|item| {
            string_field(
                item,
                &["id", "workItemId", "work_item_id", "trialId", "trial_id"],
            )
        })
        .or_else(|| {
            ["workItemId", "work_item_id", "trialId", "trial_id"]
                .iter()
                .find_map(|key| event.delta.get(*key).and_then(Value::as_str))
        })
        .map(str::to_string)
}

fn event_rollout_id(event: &OptimizerEventEnvelope, reference: &Value) -> Option<String> {
    string_field(reference, &["rolloutId", "rollout_id"])
        .or_else(|| {
            event
                .item
                .as_ref()
                .and_then(|item| string_field(item, &["rolloutId", "rollout_id"]))
        })
        .or_else(|| {
            ["rolloutId", "rollout_id"]
                .iter()
                .find_map(|key| event.delta.get(*key).and_then(Value::as_str))
        })
        .or_else(|| {
            event
                .delta
                .get("container_event")
                .and_then(|carrier| string_field(carrier, &["rollout_id", "rolloutId"]))
        })
        .or_else(|| {
            event
                .delta
                .get("containerEvent")
                .and_then(|carrier| string_field(carrier, &["rolloutId", "rollout_id"]))
        })
        .map(str::to_string)
}

/// Index complete artifact declarations in the same transaction as the event
/// rows. Existing producers sometimes attach diagnostic scalar refs; those are
/// not byte-addressable artifacts and remain on the event without entering the
/// artifact table.
pub(super) fn persist_event_artifacts(
    conn: &Connection,
    events: &[OptimizerEventEnvelope],
) -> Result<()> {
    for event in events {
        for reference in &event.artifact_refs {
            let Some(object) = reference.as_object() else {
                continue;
            };
            let Some(kind) = string_field(reference, &["kind", "type"]) else {
                continue;
            };
            let Some(locator) = string_field(
                reference,
                &[
                    "id",
                    "refId",
                    "ref_id",
                    "path",
                    "uri",
                    "casDigest",
                    "cas_digest",
                ],
            ) else {
                continue;
            };
            let artifact_id =
                string_field(reference, &["artifactId", "artifact_id"]).unwrap_or(locator);
            let digest = string_field(reference, &["digest", "sha256", "casDigest", "cas_digest"]);
            let media_type = media_type(reference, locator);
            let byte_size = object
                .get("byteSize")
                .or_else(|| object.get("byte_size"))
                .or_else(|| object.get("bytes"))
                .and_then(Value::as_u64)
                .or_else(|| {
                    std::fs::metadata(locator)
                        .ok()
                        .map(|metadata| metadata.len())
                });
            let work_item_id = event_work_item_id(event);
            let rollout_id = event_rollout_id(event, reference);
            let changed = conn.execute(
                "INSERT OR IGNORE INTO optimizer_run_artifacts(
                    optimizer_run_id, artifact_id, sequence, work_item_id, rollout_id,
                    kind, locator, digest, media_type, byte_size, metadata_json, declared_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    event.optimizer_run_id,
                    artifact_id,
                    i64::try_from(event.sequence_number).unwrap_or(i64::MAX),
                    work_item_id,
                    rollout_id,
                    kind,
                    locator,
                    digest,
                    media_type,
                    byte_size.and_then(|value| i64::try_from(value).ok()),
                    serde_json::to_string(reference)?,
                    event.occurred_at,
                ],
            )?;
            if changed == 0 {
                let existing: String = conn.query_row(
                    "SELECT locator FROM optimizer_run_artifacts
                     WHERE optimizer_run_id=?1 AND artifact_id=?2",
                    params![event.optimizer_run_id, artifact_id],
                    |row| row.get(0),
                )?;
                if existing != locator {
                    bail!(
                        "artifact identity {artifact_id:?} for run {} was redeclared from {existing:?} to {locator:?}",
                        event.optimizer_run_id
                    );
                }
            }
        }
    }
    Ok(())
}

fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OptimizerRunArtifact> {
    let sequence: i64 = row.get(2)?;
    let byte_size: Option<i64> = row.get(9)?;
    let metadata_json: String = row.get(10)?;
    Ok(OptimizerRunArtifact {
        schema_version: OPTIMIZER_ARTIFACT_SCHEMA_VERSION.into(),
        optimizer_run_id: row.get(0)?,
        artifact_id: row.get(1)?,
        sequence: sequence.max(0) as u64,
        work_item_id: row.get(3)?,
        rollout_id: row.get(4)?,
        kind: row.get(5)?,
        locator: row.get(6)?,
        digest: row.get(7)?,
        media_type: row.get(8)?,
        byte_size: byte_size.map(|value| value.max(0) as u64),
        metadata: serde_json::from_str(&metadata_json).unwrap_or(Value::Null),
        declared_at: row.get(11)?,
    })
}

pub(super) fn list(
    conn: &Connection,
    optimizer_run_id: &str,
    after_sequence: u64,
    limit: i64,
) -> Result<OptimizerArtifactPage> {
    let mut statement = conn.prepare(
        "SELECT optimizer_run_id, artifact_id, sequence, work_item_id, rollout_id,
                kind, locator, digest, media_type, byte_size, metadata_json, declared_at
         FROM optimizer_run_artifacts
         WHERE optimizer_run_id=?1 AND sequence IN (
             SELECT sequence
             FROM optimizer_run_artifacts
             WHERE optimizer_run_id=?1 AND sequence>?2
             GROUP BY sequence
             ORDER BY sequence
             LIMIT ?3
         )
         ORDER BY sequence, artifact_id",
    )?;
    let artifacts = statement
        .query_map(
            params![
                optimizer_run_id,
                i64::try_from(after_sequence).unwrap_or(i64::MAX),
                limit.clamp(1, 500)
            ],
            row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let next_sequence = artifacts
        .last()
        .map(|artifact| artifact.sequence)
        .unwrap_or(after_sequence);
    Ok(OptimizerArtifactPage {
        schema_version: ARTIFACT_PAGE_SCHEMA_VERSION.into(),
        optimizer_run_id: optimizer_run_id.into(),
        after_sequence,
        artifacts,
        next_sequence,
    })
}

pub(super) fn list_all(
    conn: &Connection,
    optimizer_run_id: &str,
) -> Result<Vec<OptimizerRunArtifact>> {
    Ok(list(conn, optimizer_run_id, 0, 500)?.artifacts)
}

pub(super) fn read_range(
    conn: &Connection,
    optimizer_run_id: &str,
    artifact_id: &str,
    offset: u64,
    length: u64,
) -> Result<OptimizerArtifactRange> {
    if length == 0 || length > MAX_RANGE_BYTES {
        bail!("artifact byte range length must be between 1 and {MAX_RANGE_BYTES}");
    }
    let artifact = conn
        .query_row(
            "SELECT optimizer_run_id, artifact_id, sequence, work_item_id, rollout_id,
                    kind, locator, digest, media_type, byte_size, metadata_json, declared_at
             FROM optimizer_run_artifacts
             WHERE optimizer_run_id=?1 AND artifact_id=?2",
            params![optimizer_run_id, artifact_id],
            row,
        )
        .context("artifact is not declared by this optimizer run")?;
    let media_type = artifact
        .media_type
        .as_deref()
        .context("artifact has no declared or inferable media type")?;
    if !SAFE_ARTIFACT_MEDIA_TYPES.contains(&media_type) {
        bail!("artifact media type {media_type:?} is not byte-streamable");
    }
    let raw_path = artifact
        .locator
        .strip_prefix("file://")
        .unwrap_or(&artifact.locator);
    let path = Path::new(raw_path);
    if !path.is_absolute() {
        bail!("artifact locator is not an absolute local file");
    }
    let link_metadata = std::fs::symlink_metadata(path).context("artifact file is unavailable")?;
    if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
        bail!("artifact locator must be a regular non-symlink file");
    }
    let total_bytes = link_metadata.len();
    if offset > total_bytes {
        bail!("artifact byte range starts beyond end of file");
    }
    let byte_length = length.min(total_bytes.saturating_sub(offset));
    let mut bytes = vec![0u8; byte_length as usize];
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut bytes)?;
    Ok(OptimizerArtifactRange {
        schema_version: ARTIFACT_RANGE_SCHEMA_VERSION.into(),
        optimizer_run_id: optimizer_run_id.into(),
        artifact_id: artifact_id.into(),
        media_type: media_type.into(),
        offset,
        byte_length,
        total_bytes,
        eof: offset.saturating_add(byte_length) >= total_bytes,
        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}
