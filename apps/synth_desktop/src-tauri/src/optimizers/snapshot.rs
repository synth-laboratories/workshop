use super::{OptimizerEventEnvelope, OptimizerRunRecord};
use crate::storage::{ContentStore, Database};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, sync::Arc};

pub const OPTIMIZER_SNAPSHOT_SCHEMA: &str = "synth.optimizer-run-snapshot.v1";
const MAX_SNAPSHOT_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerRunSnapshot {
    pub schema_version: String,
    pub source_instance_id: String,
    pub source_bundle_id: String,
    pub source_run_id: String,
    pub captured_at: String,
    pub terminal_cursor: u64,
    pub sealed: bool,
    pub run: OptimizerRunRecord,
    pub result: Value,
    pub terminal_manifest: Option<Value>,
    pub events: Vec<OptimizerEventEnvelope>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerSnapshotReceipt {
    pub schema_version: String,
    pub snapshot_id: String,
    pub content_digest: String,
    pub source_instance_id: String,
    pub source_run_id: String,
    pub terminal_cursor: u64,
    pub sealed: bool,
    pub terminal_status: Option<String>,
    pub captured_at: String,
    pub imported_at: String,
    pub artifact_path: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerSnapshotImportRequest {
    pub path: String,
    #[serde(default)]
    pub expected_digest: Option<String>,
}

pub fn canonical_bytes(snapshot: &OptimizerRunSnapshot) -> Result<Vec<u8>> {
    validate(snapshot)?;
    serde_json::to_vec(snapshot).context("serialize optimizer snapshot")
}

pub fn validate(snapshot: &OptimizerRunSnapshot) -> Result<()> {
    if snapshot.schema_version != OPTIMIZER_SNAPSHOT_SCHEMA {
        bail!(
            "unsupported optimizer snapshot schema {}",
            snapshot.schema_version
        );
    }
    if snapshot.source_instance_id.trim().is_empty() || snapshot.source_run_id.trim().is_empty() {
        bail!("optimizer snapshot source identity is required");
    }
    if snapshot.run.id != snapshot.source_run_id {
        bail!("optimizer snapshot run identity does not match sourceRunId");
    }
    if snapshot.run.cursor_seq != snapshot.terminal_cursor {
        bail!("optimizer snapshot cursor does not match run cursor");
    }
    let last = snapshot
        .events
        .last()
        .map(|event| event.sequence_number)
        .unwrap_or(0);
    if last != snapshot.terminal_cursor
        || snapshot.events.iter().enumerate().any(|(i, e)| {
            e.optimizer_run_id != snapshot.source_run_id || e.sequence_number != i as u64 + 1
        })
    {
        bail!("optimizer snapshot event chain is incomplete or non-contiguous");
    }
    if snapshot.sealed != snapshot.terminal_manifest.is_some() {
        bail!("optimizer snapshot sealed state disagrees with terminal manifest");
    }
    Ok(())
}

pub fn persist(
    db: Arc<Database>,
    content: &ContentStore,
    snapshot: &OptimizerRunSnapshot,
) -> Result<OptimizerSnapshotReceipt> {
    let bytes = canonical_bytes(snapshot)?;
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        bail!("optimizer snapshot exceeds 128 MiB limit");
    }
    let digest = content.put_bytes("optimizer_snapshots", &bytes)?;
    let snapshot_id = format!("optsnap_{}", &digest[..24]);
    let imported_at = Utc::now().to_rfc3339();
    let terminal_status = snapshot
        .terminal_manifest
        .as_ref()
        .and_then(|value| {
            value
                .get("terminalStatus")
                .or_else(|| value.get("terminal_status"))
        })
        .and_then(Value::as_str)
        .map(str::to_string);
    let export_dir = content
        .root()
        .parent()
        .unwrap_or(content.root())
        .join("exports")
        .join("optimizer-snapshots");
    fs::create_dir_all(&export_dir)?;
    let artifact = export_dir.join(format!("{snapshot_id}.json"));
    if !artifact.exists() {
        fs::write(&artifact, &bytes)?;
    }
    let metadata =
        json!({"sourceBundleId": snapshot.source_bundle_id, "eventCount": snapshot.events.len()});
    let receipt = OptimizerSnapshotReceipt {
        schema_version: OPTIMIZER_SNAPSHOT_SCHEMA.into(),
        snapshot_id: snapshot_id.clone(),
        content_digest: digest.clone(),
        source_instance_id: snapshot.source_instance_id.clone(),
        source_run_id: snapshot.source_run_id.clone(),
        terminal_cursor: snapshot.terminal_cursor,
        sealed: snapshot.sealed,
        terminal_status: terminal_status.clone(),
        captured_at: snapshot.captured_at.clone(),
        imported_at: imported_at.clone(),
        artifact_path: artifact.display().to_string(),
    };
    db.with_conn(|conn| {
        conn.execute("INSERT INTO optimizer_snapshots(snapshot_id,schema_version,content_digest,source_instance_id,source_run_id,terminal_status,terminal_cursor,sealed,captured_at,imported_at,metadata_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(snapshot_id) DO UPDATE SET imported_at=excluded.imported_at, metadata_json=excluded.metadata_json",
            params![snapshot_id, OPTIMIZER_SNAPSHOT_SCHEMA, digest, snapshot.source_instance_id, snapshot.source_run_id, terminal_status, snapshot.terminal_cursor as i64, snapshot.sealed as i64, snapshot.captured_at, imported_at, serde_json::to_string(&metadata)?])?;
        Ok(())
    })?;
    Ok(receipt)
}

pub fn import_path(
    db: Arc<Database>,
    content: &ContentStore,
    request: OptimizerSnapshotImportRequest,
) -> Result<OptimizerSnapshotReceipt> {
    let path = PathBuf::from(&request.path);
    let bytes =
        fs::read(&path).with_context(|| format!("read optimizer snapshot {}", path.display()))?;
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        bail!("optimizer snapshot exceeds 128 MiB limit");
    }
    let snapshot: OptimizerRunSnapshot =
        serde_json::from_slice(&bytes).context("parse optimizer snapshot")?;
    let canonical = canonical_bytes(&snapshot)?;
    let actual_digest = format!("{:x}", Sha256::digest(&canonical));
    if request
        .expected_digest
        .as_deref()
        .is_some_and(|expected| expected != actual_digest)
    {
        bail!("optimizer snapshot digest did not match expected digest");
    }
    persist(db, content, &snapshot)
}

pub fn load(
    db: Arc<Database>,
    content: &ContentStore,
    snapshot_id: &str,
) -> Result<(OptimizerRunSnapshot, OptimizerSnapshotReceipt)> {
    let id = snapshot_id.to_string();
    let row: Option<(String,String,String,String,i64,i64,Option<String>,String,String)> = db.with_conn(|conn| conn.query_row(
        "SELECT content_digest,source_instance_id,source_run_id,captured_at,terminal_cursor,sealed,terminal_status,imported_at,schema_version FROM optimizer_snapshots WHERE snapshot_id=?1",
        [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).optional().map_err(Into::into))?;
    let (
        digest,
        source_instance_id,
        source_run_id,
        captured_at,
        cursor,
        sealed,
        terminal_status,
        imported_at,
        schema_version,
    ) = row.ok_or_else(|| anyhow::anyhow!("optimizer snapshot not found"))?;
    let bytes = content.get_bytes("optimizer_snapshots", &digest)?;
    let snapshot: OptimizerRunSnapshot = serde_json::from_slice(&bytes)?;
    validate(&snapshot)?;
    let artifact = content
        .root()
        .parent()
        .unwrap_or(content.root())
        .join("exports")
        .join("optimizer-snapshots")
        .join(format!("{snapshot_id}.json"));
    Ok((
        snapshot,
        OptimizerSnapshotReceipt {
            schema_version,
            snapshot_id: snapshot_id.into(),
            content_digest: digest,
            source_instance_id,
            source_run_id,
            terminal_cursor: cursor as u64,
            sealed: sealed != 0,
            terminal_status,
            captured_at,
            imported_at,
            artifact_path: artifact.display().to_string(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    fn example() -> OptimizerRunSnapshot {
        let run: OptimizerRunRecord = serde_json::from_value(json!({
            "schemaVersion":"optimizer_run.v1","id":"opt_1","algorithmId":"eval",
            "status":"completed","source":"local","createdAt":"2026-08-28T00:00:00Z",
            "finishedAt":"2026-08-28T00:01:00Z","cursorSeq":1,"capabilities":{},
            "executionBindings":[],"inputRefs":[],"outputRefs":[],"visualRefs":[],
            "summary":{"meanReward":3.0},"usage":{}
        }))
        .unwrap();
        let event: OptimizerEventEnvelope = serde_json::from_value(json!({
            "schemaVersion":"optimizer_event.v1","eventId":"evt_1","type":"run.completed",
            "sequenceNumber":1,"occurredAt":"2026-08-28T00:01:00Z",
            "optimizerRunId":"opt_1","algorithmId":"eval","delta":{},"artifactRefs":[],"raw":{}
        }))
        .unwrap();
        OptimizerRunSnapshot {
            schema_version: OPTIMIZER_SNAPSHOT_SCHEMA.into(),
            source_instance_id: "r".into(),
            source_bundle_id: "com.synth.r".into(),
            source_run_id: "opt_1".into(),
            captured_at: "2026-08-28T00:01:01Z".into(),
            terminal_cursor: 1,
            sealed: true,
            run,
            result: json!({"terminalManifest":{"terminalStatus":"completed"},"finalCursor":1}),
            terminal_manifest: Some(json!({"terminalStatus":"completed","terminalCursor":1})),
            events: vec![event],
        }
    }

    #[test]
    fn snapshot_round_trip_is_digest_addressed_and_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path().join("core")).unwrap();
        let content = ContentStore::new(storage.content_root());
        let first = persist(storage.database().clone(), &content, &example()).unwrap();
        let second = persist(storage.database().clone(), &content, &example()).unwrap();
        assert_eq!(first.snapshot_id, second.snapshot_id);
        assert_eq!(first.content_digest, second.content_digest);
        let (loaded, receipt) =
            load(storage.database().clone(), &content, &first.snapshot_id).unwrap();
        assert_eq!(loaded.source_run_id, "opt_1");
        assert_eq!(receipt.source_instance_id, "r");
        assert!(receipt.sealed);
    }

    #[test]
    fn snapshot_rejects_a_broken_event_chain() {
        let mut snapshot = example();
        snapshot.events[0].sequence_number = 2;
        assert!(validate(&snapshot)
            .unwrap_err()
            .to_string()
            .contains("event chain"));
    }
}
