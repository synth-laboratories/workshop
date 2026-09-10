use super::{OptimizerEventEnvelope, OptimizerRunRecord};
use crate::storage::{ContentStore, Database};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::PathBuf, sync::Arc};

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

/// Deterministic, comparison-oriented projection over the immutable run
/// evidence. This does not rewrite the source run's terminal usage lanes:
/// container-reported per-rollout policy usage is kept separate and labeled
/// with its own completeness signal.
pub fn evidence_summary(snapshot: &OptimizerRunSnapshot) -> Value {
    let records = snapshot
        .run
        .summary
        .get("records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let projected = records
        .iter()
        .map(|record| {
            json!({
                "rolloutId": record.get("rolloutId").cloned().unwrap_or(Value::Null),
                "seed": record.get("seed").cloned().unwrap_or(Value::Null),
                "status": record.get("status").cloned().unwrap_or(Value::Null),
                "reward": record.get("reward").cloned().unwrap_or(Value::Null),
                "costUsd": record.pointer("/usage/cost").cloned().unwrap_or(Value::Null),
                "tokens": record.pointer("/usage/tokens").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    let rewards = projected
        .iter()
        .filter_map(|record| record.get("reward").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    let costs = projected
        .iter()
        .filter_map(|record| record.get("costUsd").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    let tokens = projected
        .iter()
        .filter_map(|record| record.get("tokens").and_then(Value::as_u64))
        .collect::<Vec<_>>();
    let total_reward = rewards.iter().sum::<f64>();
    let total_cost = costs.iter().sum::<f64>();
    let rollout_count = projected.len();
    let total_tokens = tokens.iter().try_fold(0u64, |sum, value| sum.checked_add(*value));
    let reward_complete = rollout_count > 0 && rewards.len() == rollout_count && total_reward.is_finite();
    let cost_complete = rollout_count > 0 && costs.len() == rollout_count && total_cost.is_finite();
    let token_complete = rollout_count > 0 && tokens.len() == rollout_count && total_tokens.is_some();
    let score_per_dollar =
        (reward_complete && cost_complete && total_cost > 0.0).then_some(total_reward / total_cost)
            .filter(|value| value.is_finite());

    json!({
        "schemaVersion": "optimizer_evidence_summary.v1",
        "runId": snapshot.source_run_id,
        "status": snapshot.run.status,
        "objective": snapshot.run.objective,
        "policyRef": snapshot.run.summary.get("policyRef").cloned().unwrap_or(Value::Null),
        "rolloutCount": rollout_count,
        "records": projected,
        "reward": {
            "complete": reward_complete,
            "reportedRollouts": rewards.len(),
            "total": reward_complete.then_some(total_reward),
            "mean": reward_complete.then_some(total_reward / rollout_count as f64),
        },
        "cost": {
            "basis": "container_reported_rollout_policy_usage",
            "complete": cost_complete,
            "reportedRollouts": costs.len(),
            "totalUsd": cost_complete.then_some(total_cost),
        },
        "tokens": {
            "basis": "container_reported_rollout_policy_usage",
            "complete": token_complete,
            "reportedRollouts": tokens.len(),
            "total": if token_complete { total_tokens } else { None },
        },
        "efficiency": {
            "basis": "total_reward_divided_by_container_reported_rollout_policy_cost",
            "scorePerDollar": score_per_dollar,
        },
        "terminalUsage": snapshot.terminal_manifest.as_ref()
            .and_then(|manifest| manifest.get("usage"))
            .cloned()
            .unwrap_or(Value::Null),
    })
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
    if snapshot.terminal_cursor > i64::MAX as u64 {
        bail!("optimizer snapshot cursor exceeds storage range");
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
    if let Some(manifest) = snapshot.terminal_manifest.as_ref() {
        super::terminal::snapshot_status(&snapshot.run, manifest)?;
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
    // An existing CAS path may have been damaged outside the app. Never issue
    // a successful import receipt for bytes the reader would later refuse.
    content.get_bytes_bounded("optimizer_snapshots", &digest, MAX_SNAPSHOT_BYTES)?;
    let snapshot_id = format!("optsnap_{}", &digest[..24]);
    let imported_at = Utc::now().to_rfc3339();
    let terminal_status = snapshot
        .terminal_manifest
        .as_ref()
        .map(|manifest| super::terminal::snapshot_status(&snapshot.run, manifest))
        .transpose()?;
    let export_dir = content
        .root()
        .parent()
        .unwrap_or(content.root())
        .join("exports")
        .join("optimizer-snapshots");
    fs::create_dir_all(&export_dir)?;
    let artifact = export_dir.join(format!("{snapshot_id}.json"));
    if !artifact.exists() {
        let mut file = tempfile::NamedTempFile::new_in(&export_dir)?;
        std::io::Write::write_all(&mut file, &bytes)?;
        file.persist(&artifact)?;
    } else if read_bounded_file(&artifact)? != bytes {
        bail!("optimizer snapshot export artifact failed content verification");
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

fn read_bounded_file(path: &std::path::Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path).with_context(|| format!("read optimizer snapshot {}", path.display()))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() { bail!("optimizer snapshot must be a regular file"); }
    if metadata.len() > MAX_SNAPSHOT_BYTES as u64 { bail!("optimizer snapshot exceeds 128 MiB limit"); }
    let mut bytes = Vec::new();
    file.take(MAX_SNAPSHOT_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SNAPSHOT_BYTES { bail!("optimizer snapshot exceeds 128 MiB limit"); }
    Ok(bytes)
}

pub fn import_path(
    db: Arc<Database>,
    content: &ContentStore,
    request: OptimizerSnapshotImportRequest,
) -> Result<OptimizerSnapshotReceipt> {
    let path = PathBuf::from(&request.path);
    let bytes = read_bounded_file(&path)?;
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
    let bytes = content.get_bytes_bounded("optimizer_snapshots", &digest, MAX_SNAPSHOT_BYTES)?;
    let snapshot: OptimizerRunSnapshot = serde_json::from_slice(&bytes)?;
    validate(&snapshot)?;
    if source_instance_id != snapshot.source_instance_id || source_run_id != snapshot.source_run_id
        || cursor < 0 || cursor as u64 != snapshot.terminal_cursor || (sealed != 0) != snapshot.sealed
        || captured_at != snapshot.captured_at || schema_version != snapshot.schema_version {
        bail!("optimizer snapshot receipt disagrees with immutable content");
    }
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

    fn sample() -> OptimizerRunSnapshot {
        serde_json::from_value(json!({
            "schemaVersion": OPTIMIZER_SNAPSHOT_SCHEMA, "sourceInstanceId": "test-source",
            "sourceBundleId": "test-bundle", "sourceRunId": "run-test", "capturedAt": "2026-09-10T00:00:00Z",
            "terminalCursor": 1, "sealed": true,
            "run": {"schemaVersion": "synth.optimizer-run.v1", "id": "run-test", "algorithmId": "eval",
                "status": "completed", "source": "local", "createdAt": "2026-09-10T00:00:00Z", "cursorSeq": 1,
                "summary": {"records": [{"rolloutId": "rollout-1", "reward": 1, "usage": {"cost": 0.5, "tokens": 10}}]}},
            "result": {"reward": 1},
            "terminalManifest": {"schemaVersion": "optimizer_terminal_manifest.v2", "optimizerRunId": "run-test",
                "algorithmId": "eval", "terminalCursor": 1, "terminal": {"kind": "completed"}},
            "events": [{"schemaVersion": "synth.optimizer-event.v1", "type": "run.completed", "sequenceNumber": 1,
                "occurredAt": "2026-09-10T00:00:00Z", "optimizerRunId": "run-test", "algorithmId": "eval"}]
        })).unwrap()
    }

    #[test]
    fn snapshot_refuses_gaps_identity_and_seal_drift() {
        let original = sample();
        assert!(validate(&original).is_ok());
        let mut invalid = vec![original.clone(); 6];
        invalid[0].events.clear();
        invalid[1].events[0].optimizer_run_id = "other".into();
        invalid[2].events[0].sequence_number = 2;
        invalid[3].sealed = false;
        invalid[4].terminal_manifest.as_mut().unwrap()["optimizerRunId"] = json!("other");
        invalid[5].terminal_manifest.as_mut().unwrap()["terminalCursor"] = json!(2);
        for snapshot in invalid { assert!(validate(&snapshot).is_err()); }
    }

    #[test]
    fn snapshot_roundtrip_is_content_addressed_and_never_creates_a_run() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(dir.path().join("db.sqlite3")).unwrap());
        let content = ContentStore::new(dir.path().join("store"));
        let original = sample();
        let receipt = persist(db.clone(), &content, &original).unwrap();
        assert_eq!(receipt.terminal_status.as_deref(), Some("completed"));
        let imported = import_path(db.clone(), &content, OptimizerSnapshotImportRequest {
            path: receipt.artifact_path.clone(), expected_digest: Some(receipt.content_digest.clone()),
        }).unwrap();
        assert_eq!(imported.snapshot_id, receipt.snapshot_id);
        let (loaded, _) = load(db.clone(), &content, &receipt.snapshot_id).unwrap();
        assert_eq!(loaded, original);
        db.with_conn(|conn| {
            assert_eq!(conn.query_row("SELECT count(*) FROM optimizer_snapshots", [], |r| r.get::<_, i64>(0))?, 1);
            assert_eq!(conn.query_row("SELECT count(*) FROM optimizer_runs", [], |r| r.get::<_, i64>(0))?, 0);
            Ok(())
        }).unwrap();
        assert!(import_path(db.clone(), &content, OptimizerSnapshotImportRequest {
            path: receipt.artifact_path, expected_digest: Some("wrong".into()),
        }).is_err());
        fs::write(content.path_for("optimizer_snapshots", &receipt.content_digest), b"corrupted").unwrap();
        assert!(load(db, &content, &receipt.snapshot_id).is_err());
    }

    #[test]
    fn snapshot_summary_does_not_turn_missing_usage_into_zero() {
        let mut snapshot = sample();
        assert_eq!(evidence_summary(&snapshot)["efficiency"]["scorePerDollar"], 2.0);
        snapshot.run.summary["records"][0].as_object_mut().unwrap().remove("usage");
        let summary = evidence_summary(&snapshot);
        assert_eq!(summary["cost"]["complete"], false);
        assert!(summary["cost"]["totalUsd"].is_null());
        assert!(summary["efficiency"]["scorePerDollar"].is_null());
    }

    #[test]
    fn snapshot_usage_overflow_is_incomplete_not_a_panic_or_infinity() {
        let mut snapshot = sample();
        snapshot.run.summary["records"] = json!([
            {"reward": f64::MAX, "usage": {"cost": f64::MAX, "tokens": u64::MAX}},
            {"reward": f64::MAX, "usage": {"cost": f64::MAX, "tokens": u64::MAX}}
        ]);
        let summary = evidence_summary(&snapshot);
        for field in ["reward", "cost", "tokens"] { assert_eq!(summary[field]["complete"], false); }
        assert!(summary["tokens"]["total"].is_null());
        assert!(summary["efficiency"]["scorePerDollar"].is_null());
    }

    #[test]
    fn snapshot_read_refuses_directories_and_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_bounded_file(dir.path()).is_err());
        let path = dir.path().join("oversized.json");
        fs::File::create(&path).unwrap().set_len(MAX_SNAPSHOT_BYTES as u64 + 1).unwrap();
        assert!(read_bounded_file(&path).is_err());
    }
}
