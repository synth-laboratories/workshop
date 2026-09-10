use crate::storage::Database;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;

fn canonical_digest(value: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(value).context("serialize visual engine state")?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn required_i64(value: &Value, camel: &str, snake: &str) -> Result<i64> {
    value.get(camel).or_else(|| value.get(snake)).and_then(Value::as_i64)
        .with_context(|| format!("{camel} is required"))
}

fn required_str<'a>(value: &'a Value, camel: &str, snake: &str) -> Result<&'a str> {
    value.get(camel).or_else(|| value.get(snake)).and_then(Value::as_str)
        .with_context(|| format!("{camel} is required"))
}

#[derive(Clone)]
pub struct VisualStateStore { db: Arc<Database> }

impl VisualStateStore {
    pub fn new(db: Arc<Database>) -> Self { Self { db } }

    pub async fn presentation(&self, visual_id: String) -> Result<Option<Value>> {
        self.db.run(move |conn| {
            conn.query_row(
                "SELECT value_json FROM visual_presentation_states WHERE visual_id=?1",
                [visual_id], |row| row.get::<_, String>(0),
            ).optional()?.map(|raw| serde_json::from_str(&raw).context("parse presentation state")).transpose()
        }).await
    }

    pub async fn put_presentation(&self, visual_id: String, request: Value) -> Result<Value> {
        let expected = request.get("expectedStateVersion").or_else(|| request.get("expected_state_version"))
            .and_then(Value::as_i64);
        let revision = required_i64(&request, "revision", "revision")?;
        let schema = request.get("schemaVersion").or_else(|| request.get("schema_version"))
            .and_then(Value::as_str).unwrap_or("synth.visual-presentation.v1").to_owned();
        let value = request.get("value").cloned().unwrap_or_else(|| json!({}));
        if !value.is_object() { bail!("presentation value must be an object"); }
        let digest = canonical_digest(&value)?;
        let now = Utc::now().to_rfc3339();
        self.db.run_transaction(move |conn| {
            let current: Option<i64> = conn.query_row(
                "SELECT state_version FROM visual_presentation_states WHERE visual_id=?1",
                [&visual_id], |row| row.get(0),
            ).optional()?;
            if let Some(expected) = expected {
                let actual = current.unwrap_or(0);
                if expected != actual { bail!("stale visual state: expected {expected}, current {actual}"); }
            }
            let state_version = current.map_or(1, |version| version + 1);
            let stored = json!({"schemaVersion":schema,"revision":revision,"stateVersion":state_version,"value":value,"digest":digest,"updatedAt":now});
            conn.execute(
                "INSERT INTO visual_presentation_states(visual_id,visual_revision,state_version,schema_version,value_json,digest,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(visual_id) DO UPDATE SET visual_revision=excluded.visual_revision,state_version=excluded.state_version,schema_version=excluded.schema_version,value_json=excluded.value_json,digest=excluded.digest,updated_at=excluded.updated_at",
                params![visual_id, revision, state_version, schema, stored.to_string(), digest, now],
            )?;
            Ok(stored)
        }).await
    }

    pub async fn put_snapshot(&self, visual_id: String, snapshot: Value) -> Result<Value> {
        let snapshot_id = required_str(&snapshot, "id", "id")?.to_owned();
        let declared_visual = required_str(&snapshot, "visualId", "visual_id")?;
        if declared_visual != visual_id { bail!("snapshot visualId does not match route"); }
        let revision = required_i64(&snapshot, "revision", "revision")?;
        let state_version = required_i64(&snapshot, "stateVersion", "state_version")?;
        let semantic = required_str(&snapshot, "semanticSceneDigest", "semantic_scene_digest")?.to_owned();
        let presentation = required_str(&snapshot, "presentationDigest", "presentation_digest")?.to_owned();
        let captured_at = snapshot.get("capturedAt").or_else(|| snapshot.get("captured_at"))
            .and_then(Value::as_str).unwrap_or_else(|| "").to_owned();
        if captured_at.is_empty() { bail!("capturedAt is required"); }
        let raw = snapshot.to_string();
        self.db.run_transaction(move |conn| {
            conn.execute(
                "INSERT INTO visual_snapshots(snapshot_id,visual_id,visual_revision,state_version,snapshot_json,semantic_scene_digest,presentation_digest,captured_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![snapshot_id, visual_id, revision, state_version, raw, semantic, presentation, captured_at],
            )?;
            Ok(snapshot)
        }).await
    }

    pub async fn snapshots(&self, visual_id: String) -> Result<Vec<Value>> {
        self.db.run(move |conn| {
            let mut statement = conn.prepare("SELECT snapshot_json FROM visual_snapshots WHERE visual_id=?1 ORDER BY captured_at,snapshot_id")?;
            let rows = statement.query_map([visual_id], |row| row.get::<_, String>(0))?;
            rows.map(|row| Ok(serde_json::from_str(&row?).context("parse visual snapshot")?)).collect()
        }).await
    }

    pub async fn put_recording(&self, visual_id: String, recording: Value) -> Result<Value> {
        let recording_id = required_str(&recording, "id", "id")?.to_owned();
        let declared_visual = required_str(&recording, "visualId", "visual_id")?;
        if declared_visual != visual_id { bail!("recording visualId does not match route"); }
        let started_at = required_str(&recording, "startedAt", "started_at")?.to_owned();
        let ended_at = recording.get("endedAt").or_else(|| recording.get("ended_at")).and_then(Value::as_str).map(str::to_owned);
        let events = recording.get("events").and_then(Value::as_array).cloned().unwrap_or_default();
        let raw = recording.to_string();
        self.db.run_transaction(move |conn| {
            let previous: Option<(String, String)> = conn.query_row(
                "SELECT visual_id,recording_json FROM visual_recordings WHERE recording_id=?1",
                [&recording_id], |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional()?;
            if let Some((owner, previous)) = previous {
                if owner != visual_id { bail!("recording belongs to a different visual"); }
                let previous: Value = serde_json::from_str(&previous)?;
                if previous.get("startedAt").or_else(|| previous.get("started_at")).and_then(Value::as_str) != Some(started_at.as_str()) {
                    bail!("recording startedAt cannot change");
                }
                if let Some(history) = previous.get("events").and_then(Value::as_array) {
                    if !events.starts_with(history) { bail!("recording update must preserve durable event history"); }
                }
            }
            conn.execute(
                "INSERT INTO visual_recordings(recording_id,visual_id,recording_json,started_at,ended_at) VALUES(?1,?2,?3,?4,?5)
                 ON CONFLICT(recording_id) DO UPDATE SET recording_json=excluded.recording_json,ended_at=excluded.ended_at",
                params![recording_id, visual_id, raw, started_at, ended_at],
            )?;
            for event in events {
                let sequence = required_i64(&event, "sequence", "sequence")?;
                let state_version = required_i64(&event, "stateVersion", "state_version")?;
                let occurred_at = required_str(&event, "occurredAt", "occurred_at")?;
                let event_json = event.to_string();
                let existing: Option<String> = conn.query_row(
                    "SELECT event_json FROM visual_recording_events WHERE recording_id=?1 AND sequence=?2",
                    params![recording_id, sequence], |row| row.get(0),
                ).optional()?;
                if let Some(existing) = existing {
                    if existing != event_json { bail!("recording event sequence {sequence} conflicts with durable history"); }
                } else {
                    conn.execute(
                        "INSERT INTO visual_recording_events(recording_id,sequence,state_version,event_json,occurred_at) VALUES(?1,?2,?3,?4,?5)",
                        params![recording_id, sequence, state_version, event_json, occurred_at],
                    )?;
                }
            }
            Ok(recording)
        }).await
    }

    pub async fn recordings(&self, visual_id: String) -> Result<Vec<Value>> {
        self.db.run(move |conn| {
            let mut statement = conn.prepare("SELECT recording_json FROM visual_recordings WHERE visual_id=?1 ORDER BY started_at,recording_id")?;
            let rows = statement.query_map([visual_id], |row| row.get::<_, String>(0))?;
            rows.map(|row| Ok(serde_json::from_str(&row?).context("parse visual recording")?)).collect()
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn store() -> VisualStateStore {
        let root = tempdir().unwrap().keep();
        let db = Arc::new(Database::open(root.join("visual-state.sqlite")).unwrap());
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO visuals(id,current_revision,title,template_id,status,renderer_kind,bindings_json,metadata_json,created_at,updated_at)
                 VALUES('vis_test',1,'Test','analysis.visual.v1','draft','template','{}','{}','2026-09-08T00:00:00Z','2026-09-08T00:00:00Z')", [],
            )?;
            conn.execute(
                "INSERT INTO visual_revisions(visual_id,revision,template_id,renderer_kind,bindings_json,created_at)
                 VALUES('vis_test',1,'analysis.visual.v1','template','{}','2026-09-08T00:00:00Z')", [],
            )?;
            Ok(())
        }).unwrap();
        VisualStateStore::new(db)
    }

    #[tokio::test]
    async fn presentation_updates_are_optimistic_and_durable() {
        let store = store();
        let first = store.put_presentation("vis_test".into(), json!({
            "revision":1,"expectedStateVersion":0,"schemaVersion":"test.presentation.v1","value":{"tab":"failures"}
        })).await.unwrap();
        assert_eq!(first["stateVersion"], 1);
        assert_eq!(store.presentation("vis_test".into()).await.unwrap().unwrap()["value"]["tab"], "failures");
        let error = store.put_presentation("vis_test".into(), json!({
            "revision":1,"expectedStateVersion":0,"value":{"tab":"all"}
        })).await.unwrap_err();
        assert!(error.to_string().contains("stale visual state"));
    }

    #[tokio::test]
    async fn snapshots_and_recordings_round_trip() {
        let store = store();
        let snapshot = json!({
            "id":"snap_1","visualId":"vis_test","revision":1,"stateVersion":1,
            "capturedAt":"2026-09-08T00:00:01Z","semanticSceneDigest":"sha256:scene",
            "presentationDigest":"sha256:presentation","corpus":{},"cohort":{},"exploration":{},"selection":{"members":[]},"renditionRefs":[]
        });
        store.put_snapshot("vis_test".into(), snapshot.clone()).await.unwrap();
        assert_eq!(store.snapshots("vis_test".into()).await.unwrap(), vec![snapshot]);
        let recording = json!({
            "id":"rec_1","visualId":"vis_test","startedAt":"2026-09-08T00:00:00Z","events":[
                {"sequence":1,"stateVersion":1,"occurredAt":"2026-09-08T00:00:01Z","kind":"snapshot_captured"}
            ],"checkpoints":[]
        });
        store.put_recording("vis_test".into(), recording.clone()).await.unwrap();
        assert_eq!(store.recordings("vis_test".into()).await.unwrap(), vec![recording.clone()]);
        let mut truncated = recording.clone();
        truncated["events"] = json!([]);
        assert!(store.put_recording("vis_test".into(), truncated).await.unwrap_err().to_string().contains("preserve durable event history"));
        let mut reassigned = recording.clone();
        reassigned["visualId"] = json!("vis_other");
        assert!(store.put_recording("vis_other".into(), reassigned).await.unwrap_err().to_string().contains("different visual"));
        assert_eq!(store.recordings("vis_test".into()).await.unwrap(), vec![recording]);
    }
}
