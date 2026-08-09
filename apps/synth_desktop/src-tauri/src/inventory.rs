//! Rust-owned Inventory domain backed by the CoreRuntime SQLite store.

use crate::storage::{AppEvent, ContentStore, Database, EventAppend, EventSource};
use crate::trace_ingest::{
    inspect_input, project_trace_archive, qualified_sha256, InspectedInput,
    TraceBundleIngestRequest, TraceBundleIngestResult,
};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{io::Read, sync::Arc};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerRegisterRequest {
    pub name: Option<String>,
    pub base_url: String,
    pub location: Option<String>,
    pub task_family: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContainerDeployment {
    pub id: String,
    pub name: String,
    pub location: String,
    pub status: String,
    pub base_url: Option<String>,
    pub pool_id: Option<String>,
    pub task_family: Option<String>,
    pub last_rollout_id: Option<String>,
    pub health: Value,
    pub metadata: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TraceRecord {
    pub id: String,
    pub digest: String,
    pub title: String,
    pub source: String,
    pub container_id: Option<String>,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub reward: Option<f64>,
    pub metrics: Value,
    pub path: Option<String>,
    pub metadata: Value,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTraceProjection {
    pub trace_digest: String,
    pub projection_kind: String,
    pub projection_schema: String,
    pub payload_digest: String,
    pub relative_path: String,
    pub payload: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TraceBundleInspection {
    pub schema_version: String,
    pub input_kind: String,
    #[serde(alias = "compatibility_level")]
    pub compatibility: String,
    pub source_bytes_digest: Option<String>,
    pub bundle_digest: Option<String>,
    #[serde(default)]
    pub archive_digest: Option<String>,
    #[serde(default)]
    pub self_contained: Option<bool>,
    #[serde(default)]
    pub trusted: bool,
    #[serde(default)]
    pub validation: Value,
    #[serde(default)]
    pub traces: Vec<InspectedTrace>,
    #[serde(default)]
    pub assets: Vec<InspectedAsset>,
    #[serde(default)]
    pub projections: Vec<InspectedProjection>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct InspectedTrace {
    #[serde(alias = "id")]
    pub trace_id: String,
    #[serde(alias = "digest", alias = "content_digest")]
    pub trace_digest: String,
    #[serde(default)]
    pub schema_version: Option<String>,
    #[serde(default, alias = "kind")]
    pub trace_kind: Option<String>,
    #[serde(default)]
    pub capture_id: Option<String>,
    #[serde(default)]
    pub binding_digest: Option<String>,
    #[serde(default)]
    pub sealed_path: Option<String>,
    #[serde(default)]
    pub source_format: Option<String>,
    #[serde(default)]
    pub producer: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub harness: Option<String>,
    #[serde(default)]
    pub benchmark: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub seed: Option<i64>,
    #[serde(default)]
    pub terminal_reason: Option<String>,
    #[serde(default)]
    pub lifecycle_status: Option<String>,
    #[serde(default)]
    pub capture_status: Option<String>,
    #[serde(default)]
    pub reward: Option<f64>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub prompt_tokens: Option<i64>,
    #[serde(default)]
    pub completion_tokens: Option<i64>,
    #[serde(default)]
    pub span_count: Option<i64>,
    #[serde(default)]
    pub event_count: Option<i64>,
    #[serde(default)]
    pub tool_call_count: Option<i64>,
    #[serde(default)]
    pub error_count: Option<i64>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct InspectedAsset {
    #[serde(alias = "path")]
    pub relative_path: String,
    pub kind: String,
    #[serde(default)]
    pub role: Option<String>,
    pub bytes_digest: Option<String>,
    #[serde(default)]
    pub semantic_digest: Option<String>,
    pub media_type: String,
    #[serde(alias = "size")]
    pub byte_size: Option<i64>,
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct InspectedProjection {
    pub path: String,
    #[serde(alias = "payload_digest", alias = "projection_digest")]
    pub digest: Option<String>,
    pub format: Option<String>,
    pub source_trace_digest: Option<String>,
    #[serde(default)]
    pub schema_version: Option<String>,
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageEntry {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub cost_usd: Option<f64>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InventoryCounts {
    pub containers: i64,
    pub traces: i64,
    pub usage: i64,
}

#[derive(Clone)]
pub struct InventoryStore {
    db: Arc<Database>,
    content: ContentStore,
}

impl InventoryStore {
    pub fn new(db: Arc<Database>, content: ContentStore) -> Self {
        Self { db, content }
    }

    pub async fn list_containers(&self) -> Result<Vec<ContainerDeployment>> {
        self.db.clone().run(|conn| list_containers(conn)).await
    }

    pub async fn get_container(&self, id: String) -> Result<ContainerDeployment> {
        self.db
            .clone()
            .run(move |conn| load_container(conn, &id))
            .await
    }

    pub async fn upsert_container(
        &self,
        request: ContainerRegisterRequest,
        status: String,
        health: Value,
        metadata: Value,
        task_family: Option<String>,
    ) -> Result<(ContainerDeployment, AppEvent)> {
        self.db.clone().run_transaction(move |conn| {
            let now = Utc::now().to_rfc3339();
            let base_url = request.base_url.trim_end_matches('/').to_string();
            let existing_id: Option<String> = conn.query_row(
                "SELECT id FROM containers WHERE base_url = ?1 LIMIT 1",
                params![&base_url],
                |row| row.get(0),
            ).optional()?;
            let id = existing_id.unwrap_or_else(|| format!("ctr_{}", Uuid::new_v4().simple()));
            let name = request.name.filter(|value| !value.trim().is_empty()).unwrap_or_else(|| "Attached container".into());
            let location = request.location.unwrap_or_else(|| "local".into());
            let health_json = serde_json::to_string(&health)?;
            let metadata_json = serde_json::to_string(&metadata)?;
            conn.execute(
                "INSERT INTO containers(id,name,location,status,base_url,task_family,health_json,metadata_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9) ON CONFLICT(id) DO UPDATE SET name=excluded.name,location=excluded.location,status=excluded.status,base_url=excluded.base_url,task_family=excluded.task_family,health_json=excluded.health_json,metadata_json=excluded.metadata_json,updated_at=excluded.updated_at",
                params![&id, &name, &location, &status, &base_url, &task_family, health_json, metadata_json, &now],
            )?;
            let container = load_container(conn, &id)?;
            let event = crate::storage::append_event(conn, EventAppend {
                event_id: None, session_id: None, run_id: None, source: EventSource::Local,
                kind: "container.registered".into(),
                payload: serde_json::json!({"containerId": id, "baseUrl": base_url, "status": status, "taskFamily": task_family}),
                remote_sequence: None, command_id: None, created_at: Some(now),
            })?;
            Ok((container, event))
        }).await
    }

    pub async fn update_container_hydration(
        &self,
        id: String,
        status: String,
        health: Value,
        metadata: Value,
        task_family: Option<String>,
    ) -> Result<(ContainerDeployment, AppEvent)> {
        self.db.clone().run_transaction(move |conn| {
            let now = Utc::now().to_rfc3339();
            let changed = conn.execute(
                "UPDATE containers SET status=?1,health_json=?2,metadata_json=?3,task_family=COALESCE(?4,task_family),updated_at=?5 WHERE id=?6",
                params![&status, serde_json::to_string(&health)?, serde_json::to_string(&metadata)?, &task_family, &now, &id],
            )?;
            if changed == 0 { return Err(anyhow!("container not found: {id}")); }
            let container = load_container(conn, &id)?;
            let event = crate::storage::append_event(conn, EventAppend {
                event_id: None, session_id: None, run_id: None, source: EventSource::Local,
                kind: "container.probed".into(), payload: serde_json::json!({"containerId": id, "status": status, "hydratedAt": now}),
                remote_sequence: None, command_id: None, created_at: Some(now),
            })?;
            Ok((container, event))
        }).await
    }

    pub async fn update_container_health(
        &self,
        id: String,
        status: String,
        health: Value,
    ) -> Result<(ContainerDeployment, AppEvent)> {
        self.db.clone().run_transaction(move |conn| {
            let now = Utc::now().to_rfc3339();
            let health_json = serde_json::to_string(&health)?;
            let changed = conn.execute(
                "UPDATE containers SET status = ?1, health_json = ?2, updated_at = ?3 WHERE id = ?4",
                params![&status, health_json, &now, &id],
            )?;
            if changed == 0 {
                return Err(anyhow!("container not found: {id}"));
            }
            let container = load_container(conn, &id)?;
            let event = crate::storage::append_event(
                conn,
                EventAppend {
                    event_id: None,
                    session_id: None,
                    run_id: None,
                    source: EventSource::Local,
                    kind: "container.health.updated".into(),
                    payload: serde_json::json!({
                        "containerId": id,
                        "status": status,
                        "health": health,
                        "updatedAt": now,
                    }),
                    remote_sequence: None,
                    command_id: None,
                    created_at: Some(now),
                },
            )?;
            Ok((container, event))
        }).await
    }

    pub async fn update_container_last_rollout(
        &self,
        id: String,
        rollout_id: String,
    ) -> Result<(ContainerDeployment, AppEvent)> {
        self.db
            .clone()
            .run_transaction(move |conn| {
                let now = Utc::now().to_rfc3339();
                let changed = conn.execute(
                    "UPDATE containers SET last_rollout_id=?1,updated_at=?2 WHERE id=?3",
                    params![&rollout_id, &now, &id],
                )?;
                if changed == 0 {
                    return Err(anyhow!("container not found: {id}"));
                }
                let container = load_container(conn, &id)?;
                let event = crate::storage::append_event(
                    conn,
                    EventAppend {
                        event_id: None,
                        session_id: None,
                        run_id: None,
                        source: EventSource::Local,
                        kind: "container.rollout.completed".into(),
                        payload: serde_json::json!({
                            "containerId": id,
                            "rolloutId": rollout_id,
                            "updatedAt": now,
                        }),
                        remote_sequence: None,
                        command_id: None,
                        created_at: Some(now),
                    },
                )?;
                Ok((container, event))
            })
            .await
    }

    pub async fn list_traces(&self) -> Result<Vec<TraceRecord>> {
        self.db.clone().run(|conn| list_traces(conn)).await
    }

    pub async fn get_trace(&self, id: String) -> Result<TraceRecord> {
        self.db
            .clone()
            .run(move |conn| load_trace(conn, &id)?.ok_or_else(|| anyhow!("trace not found: {id}")))
            .await
    }

    pub async fn ingest_trace_bundle(
        &self,
        request: TraceBundleIngestRequest,
    ) -> Result<(TraceBundleIngestResult, Option<AppEvent>)> {
        let staging_root = self.content.root().join(".trace-staging");
        let inspected = inspect_input(&request, &staging_root).await?;
        self.commit_inspected_trace(request, inspected).await
    }

    pub(crate) async fn commit_inspected_trace(
        &self,
        request: TraceBundleIngestRequest,
        inspected: InspectedInput,
    ) -> Result<(TraceBundleIngestResult, Option<AppEvent>)> {
        let input_digest = inspected
            .inspection
            .source_bytes_digest
            .as_deref()
            .map(qualified_sha256)
            .transpose()?
            .or_else(|| inspected.archive_bytes.as_deref().map(sha256_qualified))
            .ok_or_else(|| anyhow!("trace inspection omitted the input digest"))?;
        let quarantine_bytes = inspected
            .raw_file_bytes
            .as_deref()
            .or(inspected.archive_bytes.as_deref());
        let stored_import_path = if let Some(bytes) = quarantine_bytes {
            let stored = self.content.put_bytes("trace_imports", bytes)?;
            if qualified_sha256(&stored)? != input_digest {
                // Directory inputs have no original byte stream. Their safe deterministic
                // snapshot may differ from a separately reported source inventory digest.
                if inspected.raw_file_bytes.is_some() {
                    bail!("synth-containers input digest did not match the supplied file bytes");
                }
            }
            Some(
                self.content
                    .path_for("trace_imports", &stored)
                    .display()
                    .to_string(),
            )
        } else {
            None
        };

        let validation_ok = inspected
            .inspection
            .validation
            .get("valid")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let accepted_compatibility = matches!(
            inspected.inspection.compatibility.as_str(),
            "native" | "legacy_native" | "migrated"
        );
        let trusted = inspected.inspection.trusted
            && inspected.inspection.self_contained == Some(true)
            && validation_ok
            && accepted_compatibility
            && inspected.inspection.bundle_digest.is_some()
            && inspected.inspection.archive_digest.is_some()
            && inspected.archive_bytes.is_some();

        let mut archive_digest = None;
        let mut archive_path = None;
        let bundle_digest = inspected
            .inspection
            .bundle_digest
            .as_deref()
            .map(qualified_sha256)
            .transpose()?;
        if trusted {
            let archive = inspected.archive_bytes.as_deref().expect("checked above");
            let stored = self.content.put_bytes("traces", archive)?;
            let qualified_stored = qualified_sha256(&stored)?;
            let declared = qualified_sha256(
                inspected
                    .inspection
                    .archive_digest
                    .as_deref()
                    .expect("checked above"),
            )?;
            if qualified_stored != declared {
                bail!("verified archive digest did not match synth-containers inspection");
            }
            archive_path = Some(
                self.content
                    .path_for("traces", &stored)
                    .display()
                    .to_string(),
            );
            archive_digest = Some(qualified_stored);
        }

        let now = Utc::now().to_rfc3339();
        let source_uri = request
            .source_uri
            .clone()
            .or_else(|| Some(request.source_path.clone()));
        let source_kind = request
            .source_kind
            .clone()
            .unwrap_or_else(|| inspected.inspection.input_kind.clone());
        let compatibility = inspected.inspection.compatibility.clone();
        let validation_status = if validation_ok { "valid" } else { "invalid" }.to_string();
        let errors = inspected
            .inspection
            .validation
            .get("issues")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let inspection_json = inspected.inspection_json;
        let traces = inspected.inspection.traces;
        let assets = inspected.inspection.assets;
        let projections = inspected.inspection.projections;
        let archive_byte_size = inspected
            .archive_bytes
            .as_ref()
            .map_or(0, |body| body.len() as i64);
        let input_byte_size = inspected
            .raw_file_bytes
            .as_ref()
            .map_or(archive_byte_size, |body| body.len() as i64);
        let return_input_digest = input_digest.clone();
        let return_bundle_digest = bundle_digest.clone();
        let return_archive_digest = archive_digest.clone();
        let return_compatibility = compatibility.clone();
        let return_validation = inspected.inspection.validation.clone();
        let db = self.db.clone();
        let result = db.run_transaction(move |conn| {
            let duplicate: bool = if trusted {
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM trace_bundles WHERE bundle_digest=?1 AND archive_digest=?2)",
                    params![&bundle_digest, &archive_digest],
                    |row| row.get(0),
                )?
            } else {
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM trace_imports WHERE input_digest=?1 AND compatibility_level=?2 AND validation_status=?3)",
                    params![&input_digest, &compatibility, &validation_status],
                    |row| row.get(0),
                )?
            };
            conn.execute(
                "INSERT INTO trace_imports(input_digest,stored_path,source_kind,source_uri,compatibility_level,validation_status,detected_schema,detected_bundle_digest,byte_size,imported_at,error_json,metadata_json)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                 ON CONFLICT(input_digest) DO UPDATE SET stored_path=COALESCE(excluded.stored_path,trace_imports.stored_path),source_uri=COALESCE(excluded.source_uri,trace_imports.source_uri),compatibility_level=excluded.compatibility_level,validation_status=excluded.validation_status,detected_schema=excluded.detected_schema,detected_bundle_digest=excluded.detected_bundle_digest,byte_size=excluded.byte_size,error_json=excluded.error_json,metadata_json=excluded.metadata_json",
                params![
                    &input_digest,
                    &stored_import_path,
                    &source_kind,
                    &source_uri,
                    &compatibility,
                    &validation_status,
                    "synth.trace-inspection.v1",
                    &bundle_digest,
                    input_byte_size,
                    &now,
                    serde_json::to_string(&errors)?,
                    serde_json::to_string(&inspection_json)?,
                ],
            )?;

            let mut records = Vec::new();
            if trusted {
                let bundle_digest = bundle_digest.as_ref().expect("trusted bundle digest");
                let archive_digest = archive_digest.as_ref().expect("trusted archive digest");
                let archive_path = archive_path.as_ref().expect("trusted archive path");
                conn.execute(
                    "INSERT INTO trace_bundles(bundle_digest,archive_digest,archive_path,schema_version,compatibility_level,validation_status,self_contained,source_kind,source_uri,object_count,byte_size,imported_at,metadata_json)
                     VALUES(?1,?2,?3,'synth.trace-bundle.v1',?4,?5,1,?6,?7,?8,?9,?10,?11)
                     ON CONFLICT(bundle_digest) DO UPDATE SET archive_digest=excluded.archive_digest,archive_path=excluded.archive_path,compatibility_level=excluded.compatibility_level,validation_status=excluded.validation_status,self_contained=1,source_uri=COALESCE(excluded.source_uri,trace_bundles.source_uri),object_count=excluded.object_count,byte_size=excluded.byte_size,metadata_json=excluded.metadata_json",
                    params![bundle_digest,archive_digest,archive_path,&compatibility,&validation_status,&source_kind,&source_uri,assets.len() as i64,archive_byte_size,&now,serde_json::to_string(&inspection_json)?],
                )?;

                let has_media = assets.iter().any(|asset| {
                    asset.available
                        && (asset.media_type.starts_with("image/")
                            || asset.media_type.starts_with("audio/")
                            || asset.media_type.starts_with("video/"))
                });
                let has_evidence = assets
                    .iter()
                    .any(|asset| asset.available && asset.kind == "evidence");

                for trace in &traces {
                    let trace_digest = qualified_sha256(&trace.trace_digest)?;
                    let existing_id: Option<String> = conn
                        .query_row(
                            "SELECT id FROM traces WHERE digest=?1",
                            params![&trace_digest],
                            |row| row.get(0),
                        )
                        .optional()?;
                    let row_id = existing_id.unwrap_or_else(|| {
                        format!("tracev5_{}", &trace_digest[7..31])
                    });
                    let title = request.title.clone().unwrap_or_else(|| trace.trace_id.clone());
                    let metadata = serde_json::json!({
                        "schemaVersion": trace.schema_version.as_deref().unwrap_or("synth.trace.v5"),
                        "bundleDigest": bundle_digest,
                        "archiveDigest": archive_digest,
                        "compatibilityLevel": compatibility,
                        "captureId": trace.capture_id,
                        "sourceFormat": trace.source_format,
                        "producer": trace.producer,
                        "model": trace.model,
                        "provider": trace.provider,
                        "harness": trace.harness,
                        "benchmark": trace.benchmark,
                        "taskId": trace.task_id,
                        "seed": trace.seed,
                        "terminalReason": trace.terminal_reason,
                        "lifecycleStatus": trace.lifecycle_status,
                        "captureStatus": trace.capture_status,
                        "costUsd": trace.cost_usd,
                        "promptTokens": trace.prompt_tokens,
                        "completionTokens": trace.completion_tokens,
                        "spanCount": trace.span_count,
                        "eventCount": trace.event_count,
                        "toolCallCount": trace.tool_call_count,
                        "errorCount": trace.error_count,
                        "durationMs": trace.duration_ms,
                        "hasMedia": has_media,
                        "hasEvidence": has_evidence,
                    });
                    conn.execute(
                        "INSERT INTO traces(id,digest,title,source,reward,metrics_json,path,metadata_json,created_at)
                         VALUES(?1,?2,?3,'import',?4,'[]',?5,?6,?7)
                         ON CONFLICT(digest) DO UPDATE SET path=excluded.path,metadata_json=excluded.metadata_json",
                        params![&row_id,&trace_digest,&title,trace.reward,archive_path,serde_json::to_string(&metadata)?,&now],
                    )?;
                    conn.execute(
                        "INSERT INTO trace_bundle_members(bundle_digest,trace_row_id,trace_digest,trace_id,capture_id,binding_digest,sealed_path)
                         VALUES(?1,?2,?3,?4,?5,?6,?7)
                         ON CONFLICT(bundle_digest,trace_digest) DO UPDATE SET trace_row_id=excluded.trace_row_id,trace_id=excluded.trace_id,capture_id=excluded.capture_id,binding_digest=excluded.binding_digest,sealed_path=excluded.sealed_path",
                        params![bundle_digest,&row_id,&trace_digest,&trace.trace_id,&trace.capture_id,&trace.binding_digest,&trace.sealed_path],
                    )?;
                    let search_text = [
                        Some(trace.trace_id.as_str()),
                        trace.model.as_deref(),
                        trace.provider.as_deref(),
                        trace.benchmark.as_deref(),
                        trace.task_id.as_deref(),
                    ].into_iter().flatten().collect::<Vec<_>>().join(" ");
                    conn.execute(
                        "INSERT INTO trace_index(trace_digest,projector_version,trace_kind,producer,model,provider,harness,benchmark,task_id,seed,terminal_reason,lifecycle_status,capture_status,reward,cost_usd,prompt_tokens,completion_tokens,span_count,event_count,tool_call_count,error_count,started_at,ended_at,duration_ms,has_media,has_evidence,search_text)
                         VALUES(?1,'synth.trace-inspection.v1',?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)
                         ON CONFLICT(trace_digest) DO UPDATE SET projector_version=excluded.projector_version,trace_kind=excluded.trace_kind,producer=excluded.producer,model=excluded.model,provider=excluded.provider,harness=excluded.harness,benchmark=excluded.benchmark,task_id=excluded.task_id,seed=excluded.seed,terminal_reason=excluded.terminal_reason,lifecycle_status=excluded.lifecycle_status,capture_status=excluded.capture_status,reward=excluded.reward,cost_usd=excluded.cost_usd,prompt_tokens=excluded.prompt_tokens,completion_tokens=excluded.completion_tokens,span_count=excluded.span_count,event_count=excluded.event_count,tool_call_count=excluded.tool_call_count,error_count=excluded.error_count,started_at=excluded.started_at,ended_at=excluded.ended_at,duration_ms=excluded.duration_ms,search_text=excluded.search_text",
                        params![&trace_digest,&trace.trace_kind,&trace.producer,&trace.model,&trace.provider,&trace.harness,&trace.benchmark,&trace.task_id,trace.seed,&trace.terminal_reason,&trace.lifecycle_status,&trace.capture_status,trace.reward,trace.cost_usd,trace.prompt_tokens,trace.completion_tokens,trace.span_count.unwrap_or(0),trace.event_count.unwrap_or(0),trace.tool_call_count.unwrap_or(0),trace.error_count.unwrap_or(0),&trace.started_at,&trace.ended_at,trace.duration_ms,has_media as i64,has_evidence as i64,&search_text],
                    )?;
                    records.push(load_trace(conn, &row_id)?.context("load imported trace")?);
                }

                for asset in &assets {
                    let Some(bytes_digest) = asset.bytes_digest.as_deref() else { continue; };
                    conn.execute(
                        "INSERT INTO trace_assets(bundle_digest,relative_path,kind,role,bytes_digest,semantic_digest,media_type,byte_size,availability)
                         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
                         ON CONFLICT(bundle_digest,relative_path) DO UPDATE SET kind=excluded.kind,role=excluded.role,bytes_digest=excluded.bytes_digest,semantic_digest=excluded.semantic_digest,media_type=excluded.media_type,byte_size=excluded.byte_size,availability=excluded.availability",
                        params![bundle_digest,&asset.relative_path,&asset.kind,&asset.role,qualified_sha256(bytes_digest)?,asset.semantic_digest.as_deref().map(qualified_sha256).transpose()?,&asset.media_type,asset.byte_size.unwrap_or(0),if asset.available && asset.verified {"verified"} else if asset.available {"available"} else {"missing"}],
                    )?;
                }

                for projection in &projections {
                    if !projection.available || !projection.verified { continue; }
                    let (Some(trace_digest),Some(payload_digest),Some(kind)) = (
                        projection.source_trace_digest.as_deref(),
                        projection.digest.as_deref(),
                        projection.format.as_deref(),
                    ) else { continue; };
                    let consumer_kind = projection_consumer_kind(kind);
                    conn.execute(
                        "UPDATE trace_assets SET role=?1 WHERE bundle_digest=?2 AND relative_path=?3 AND kind='projection'",
                        params![&consumer_kind,bundle_digest,&projection.path],
                    )?;
                    conn.execute(
                        "INSERT INTO trace_projection_cache(trace_digest,projection_kind,projection_schema,projector_version,source_digest,payload_digest,created_at)
                         VALUES(?1,?2,?3,'synth-containers',?1,?4,?5)
                         ON CONFLICT(trace_digest,projection_kind,projector_version) DO UPDATE SET projection_schema=excluded.projection_schema,source_digest=excluded.source_digest,payload_digest=excluded.payload_digest,created_at=excluded.created_at",
                        // `schema_version` describes the projection envelope
                        // (`synth.projection-manifest.v1`). Consumers need the
                        // payload contract recorded in `format`.
                        params![qualified_sha256(trace_digest)?,&consumer_kind,kind,qualified_sha256(payload_digest)?,&now],
                    )?;
                }
            }

            let event = (!duplicate).then(|| crate::storage::append_event(conn, EventAppend {
                event_id: None,
                session_id: None,
                run_id: None,
                source: EventSource::Local,
                kind: if trusted { "trace.bundle.imported" } else { "trace.bundle.quarantined" }.into(),
                payload: serde_json::json!({
                    "inputDigest": input_digest,
                    "bundleDigest": bundle_digest,
                    "archiveDigest": archive_digest,
                    "compatibilityLevel": compatibility,
                    "trusted": trusted,
                    "traceCount": records.len(),
                }),
                remote_sequence: None,
                command_id: None,
                created_at: Some(now.clone()),
            })).transpose()?;
            Ok((records,event,duplicate))
        }).await?;

        Ok((
            TraceBundleIngestResult {
                compatibility_level: return_compatibility,
                trusted,
                duplicate: result.2,
                input_digest: return_input_digest,
                bundle_digest: return_bundle_digest,
                archive_digest: return_archive_digest,
                traces: result.0,
                validation: return_validation,
            },
            result.1,
        ))
    }

    pub async fn resolve_trace_projection(
        &self,
        trace_digest: String,
        projection_kind: String,
    ) -> Result<ResolvedTraceProjection> {
        let trace_digest = qualified_sha256(&trace_digest)?;
        let requested_kind = projection_kind.clone();
        let lookup_digest = trace_digest.clone();
        let resolved = self.db.clone().run(move |conn| {
            conn.query_row(
                "SELECT tpc.projection_schema,tpc.payload_digest,tb.archive_path,ta.relative_path
                 FROM trace_projection_cache tpc
                 JOIN trace_bundle_members tbm ON tbm.trace_digest=tpc.trace_digest
                 JOIN trace_bundles tb ON tb.bundle_digest=tbm.bundle_digest
                 JOIN trace_assets ta ON ta.bundle_digest=tb.bundle_digest AND ta.kind='projection' AND ta.role=tpc.projection_kind
                 WHERE tpc.trace_digest=?1 AND tpc.projection_kind=?2 AND tb.validation_status='valid' AND tb.self_contained=1
                 ORDER BY tb.imported_at DESC LIMIT 1",
                params![&lookup_digest,&requested_kind],
                |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?)),
            ).optional().map_err(Into::into)
        }).await?;
        let Some((projection_schema, payload_digest, archive_path, relative_path)) = resolved
        else {
            let lookup_digest = trace_digest.clone();
            let archive_path = self.db.clone().run(move |conn| {
                conn.query_row(
                    "SELECT tb.archive_path
                     FROM trace_bundle_members tbm
                     JOIN trace_bundles tb ON tb.bundle_digest=tbm.bundle_digest
                     WHERE tbm.trace_digest=?1 AND tb.validation_status='valid' AND tb.self_contained=1
                     ORDER BY tb.imported_at DESC LIMIT 1",
                    params![&lookup_digest],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(Into::into)
            }).await?.ok_or_else(|| anyhow!("trusted Trace V5 archive not found for {trace_digest}"))?;
            let derived = project_trace_archive(
                std::path::Path::new(&archive_path),
                &trace_digest,
                &projection_kind,
                &self.content.root().join(".trace-staging"),
            )
            .await?;
            return Ok(ResolvedTraceProjection {
                trace_digest,
                projection_kind,
                projection_schema: derived.projection_schema,
                payload_digest: derived.payload_digest,
                relative_path: derived.relative_path,
                payload: derived.payload,
            });
        };
        let archive_path = std::path::PathBuf::from(archive_path);
        let entry_path = relative_path.clone();
        let payload = tokio::task::spawn_blocking(move || -> Result<Value> {
            let file = std::fs::File::open(&archive_path).with_context(|| {
                format!("open trusted trace archive {}", archive_path.display())
            })?;
            let mut archive = zip::ZipArchive::new(file).context("open trusted trace ZIP")?;
            let mut entry = archive.by_name(&entry_path).with_context(|| {
                format!("projection asset missing from trusted archive: {entry_path}")
            })?;
            if entry.size() > 64 * 1024 * 1024 {
                bail!("projection payload exceeds 64 MiB");
            }
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes)?;
            let document: Value =
                serde_json::from_slice(&bytes).context("decode projection JSON")?;
            Ok(document.get("payload").cloned().unwrap_or(document))
        })
        .await
        .context("projection resolver worker")??;
        Ok(ResolvedTraceProjection {
            trace_digest,
            projection_kind,
            projection_schema,
            payload_digest,
            relative_path,
            payload,
        })
    }

    pub async fn list_usage(&self, limit: i64) -> Result<Vec<UsageEntry>> {
        self.db
            .clone()
            .run(move |conn| list_usage(conn, limit.clamp(1, 2000)))
            .await
    }

    pub async fn counts(&self) -> Result<InventoryCounts> {
        self.db
            .clone()
            .run(|conn| {
                Ok(InventoryCounts {
                    containers: conn
                        .query_row("SELECT COUNT(*) FROM containers", [], |row| row.get(0))?,
                    traces: conn.query_row("SELECT COUNT(*) FROM traces", [], |row| row.get(0))?,
                    usage: conn
                        .query_row("SELECT COUNT(*) FROM usage_ledger", [], |row| row.get(0))?,
                })
            })
            .await
    }
}

fn sha256_qualified(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn projection_consumer_kind(format: &str) -> String {
    format
        .strip_prefix("synth.trace-projection.")
        .and_then(|value| value.strip_suffix(".v1"))
        .unwrap_or(format)
        .to_string()
}

fn parse_json(raw: String) -> rusqlite::Result<Value> {
    serde_json::from_str(&raw).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            raw.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn container_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContainerDeployment> {
    Ok(ContainerDeployment {
        id: row.get(0)?,
        name: row.get(1)?,
        location: row.get(2)?,
        status: row.get(3)?,
        base_url: row.get(4)?,
        pool_id: row.get(5)?,
        task_family: row.get(6)?,
        last_rollout_id: row.get(7)?,
        health: parse_json(row.get(8)?)?,
        metadata: parse_json(row.get(9)?)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn load_container(conn: &Connection, id: &str) -> Result<ContainerDeployment> {
    conn.query_row(
        "SELECT id,name,location,status,base_url,pool_id,task_family,last_rollout_id,health_json,metadata_json,created_at,updated_at FROM containers WHERE id=?1",
        params![id], container_from_row,
    ).optional()?.ok_or_else(|| anyhow!("container not found: {id}"))
}

fn list_containers(conn: &Connection) -> Result<Vec<ContainerDeployment>> {
    let mut statement = conn.prepare(
        "SELECT id,name,location,status,base_url,pool_id,task_family,last_rollout_id,health_json,metadata_json,created_at,updated_at FROM containers ORDER BY updated_at DESC, id",
    )?;
    let rows = statement
        .query_map([], container_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn trace_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TraceRecord> {
    Ok(TraceRecord {
        id: row.get(0)?,
        digest: row.get(1)?,
        title: row.get(2)?,
        source: row.get(3)?,
        container_id: row.get(4)?,
        session_id: row.get(5)?,
        run_id: row.get(6)?,
        reward: row.get(7)?,
        metrics: parse_json(row.get(8)?)?,
        path: row.get(9)?,
        metadata: parse_json(row.get(10)?)?,
        created_at: row.get(11)?,
    })
}

fn load_trace(conn: &Connection, id: &str) -> Result<Option<TraceRecord>> {
    Ok(conn.query_row(
        "SELECT id,digest,title,source,container_id,session_id,run_id,reward,metrics_json,path,metadata_json,created_at FROM traces WHERE id=?1 OR digest=?1",
        params![id], trace_from_row,
    ).optional()?)
}

fn list_traces(conn: &Connection) -> Result<Vec<TraceRecord>> {
    let mut statement = conn.prepare(
        "SELECT id,digest,title,source,container_id,session_id,run_id,reward,metrics_json,path,metadata_json,created_at FROM traces ORDER BY created_at DESC, id",
    )?;
    let rows = statement
        .query_map([], trace_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn usage_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageEntry> {
    Ok(UsageEntry {
        id: row.get(0)?,
        provider: row.get(1)?,
        model: row.get(2)?,
        session_id: row.get(3)?,
        run_id: row.get(4)?,
        prompt_tokens: row.get(5)?,
        completion_tokens: row.get(6)?,
        total_tokens: row.get(7)?,
        cost_usd: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn list_usage(conn: &Connection, limit: i64) -> Result<Vec<UsageEntry>> {
    let mut statement = conn.prepare(
        "SELECT id,provider,model,session_id,run_id,prompt_tokens,completion_tokens,total_tokens,cost_usd,created_at FROM usage_ledger ORDER BY created_at DESC, id LIMIT ?1",
    )?;
    let rows = statement
        .query_map(params![limit], usage_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use tempfile::tempdir;

    #[tokio::test]
    async fn lists_rust_owned_inventory_tables() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let db = storage.database().clone();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO containers(id,name,location,status,health_json,metadata_json,created_at,updated_at) VALUES('ctr_1','Local','local','ready','{\"ok\":true}','{}','2026-01-01','2026-01-02')", [])?;
            conn.execute("INSERT INTO traces(id,digest,title,source,metrics_json,metadata_json,created_at) VALUES('trace_1','digest_1','Trace','local','[]','{}','2026-01-03')", [])?;
            conn.execute("INSERT INTO usage_ledger(id,provider,model,prompt_tokens,completion_tokens,total_tokens,created_at) VALUES('usage_1','openrouter','luna',2,3,5,'2026-01-04')", [])?;
            Ok(())
        }).unwrap();
        let inventory = InventoryStore::new(db, ContentStore::new(storage.content_root()));
        assert_eq!(inventory.list_containers().await.unwrap()[0].id, "ctr_1");
        assert_eq!(inventory.list_traces().await.unwrap()[0].id, "trace_1");
        assert_eq!(inventory.list_usage(100).await.unwrap()[0].total_tokens, 5);
        assert_eq!(
            inventory.counts().await.unwrap(),
            InventoryCounts {
                containers: 1,
                traces: 1,
                usage: 1
            }
        );
    }

    #[tokio::test]
    async fn health_update_and_journal_event_commit_together() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let db = storage.database().clone();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO containers(id,name,location,status,health_json,metadata_json,created_at,updated_at) VALUES('ctr_1','Local','local','starting','{}','{}','2026-01-01','2026-01-01')", [])?;
            Ok(())
        }).unwrap();
        let inventory = InventoryStore::new(db.clone(), ContentStore::new(storage.content_root()));

        let (container, event) = inventory
            .update_container_health(
                "ctr_1".into(),
                "ready".into(),
                serde_json::json!({"ok": true}),
            )
            .await
            .unwrap();

        assert_eq!(container.status, "ready");
        assert_eq!(event.kind, "container.health.updated");
        assert_eq!(event.payload["containerId"], "ctr_1");
        db.with_conn(|conn| {
            let events: i64 = conn.query_row(
                "SELECT COUNT(*) FROM events WHERE kind = 'container.health.updated'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(events, 1);
            Ok(())
        })
        .unwrap();
    }

    #[tokio::test]
    async fn health_update_rolls_back_when_journal_append_fails() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let db = storage.database().clone();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO containers(id,name,location,status,health_json,metadata_json,created_at,updated_at) VALUES('ctr_1','Local','local','starting','{}','{}','2026-01-01','2026-01-01')", [])?;
            conn.execute_batch(
                "CREATE TRIGGER reject_health_event
                 BEFORE INSERT ON events
                 WHEN NEW.kind = 'container.health.updated'
                 BEGIN SELECT RAISE(ABORT, 'test journal failure'); END;",
            )?;
            Ok(())
        }).unwrap();
        let inventory = InventoryStore::new(db.clone(), ContentStore::new(storage.content_root()));

        assert!(inventory
            .update_container_health(
                "ctr_1".into(),
                "ready".into(),
                serde_json::json!({"ok": true}),
            )
            .await
            .is_err());

        let unchanged = inventory.get_container("ctr_1".into()).await.unwrap();
        assert_eq!(unchanged.status, "starting");
        assert_eq!(unchanged.health, serde_json::json!({}));
    }
}
