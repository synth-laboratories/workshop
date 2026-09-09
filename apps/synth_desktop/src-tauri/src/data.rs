//! Rust-owned Data domain backed by the CoreRuntime SQLite store.

use crate::storage::{AppEvent, ContentStore, Database, EventAppend, EventSource};
use crate::trace_ingest::{
    inspect_input, project_trace_archive, qualified_sha256, InspectedInput,
    TraceBundleIngestRequest, TraceBundleIngestResult,
};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{io::Read, sync::Arc};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContainerRegisterRequest {
    pub name: Option<String>,
    pub base_url: String,
    pub location: Option<String>,
    pub task_family: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
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
    pub current_failure_id: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub health: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Value,
    pub created_at: String,
    pub updated_at: String,
}

/// Fields bound to the approved workspace declaration, rather than observed
/// from a live container. Catalog registration and probing refresh runtime
/// facts, but must not detach the durable record from the source revision that
/// admission is authorized to read.
const DURABLE_CONTAINER_DECLARATION_KEYS: &[&str] = &[
    "workspaceSpecId",
    "sourcePath",
    "declarationOrigin",
    "launchDeclaration",
    "policySourcePath",
    "gitRevision",
    "manifestHash",
];

fn merge_container_hydration_metadata(previous: Option<&Value>, mut observed: Value) -> Value {
    let Some(previous) = previous.and_then(Value::as_object) else {
        return observed;
    };
    let Some(observed) = observed.as_object_mut() else {
        return Value::Object(previous.clone());
    };
    let has_workspace_declaration = previous
        .get("workspaceSpecId")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
        && (previous.get("declarationOrigin").is_some() || previous.get("sourcePath").is_some());
    if !has_workspace_declaration {
        return Value::Object(observed.clone());
    }
    for key in DURABLE_CONTAINER_DECLARATION_KEYS {
        if let Some(value) = previous.get(*key) {
            observed.insert((*key).to_string(), value.clone());
        }
    }
    Value::Object(observed.clone())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
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
    #[specta(type = specta_typescript::Unknown)]
    pub metrics: Value,
    pub path: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Value,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTraceProjection {
    pub trace_digest: String,
    pub projection_kind: String,
    pub projection_schema: String,
    pub payload_digest: String,
    pub relative_path: String,
    #[specta(type = specta_typescript::Unknown)]
    pub payload: Value,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
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
    #[specta(type = specta_typescript::Unknown)]
    pub validation: Value,
    #[serde(default)]
    pub traces: Vec<InspectedTrace>,
    #[serde(default)]
    pub assets: Vec<InspectedAsset>,
    #[serde(default)]
    pub projections: Vec<InspectedProjection>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
pub struct InspectedTrace {
    #[serde(alias = "id")]
    pub trace_id: String,
    #[serde(alias = "digest", alias = "content_digest")]
    pub trace_digest: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub trial_id: Option<String>,
    #[serde(default)]
    pub episode_id: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
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
    #[specta(type = Option<specta_typescript::Number>)]
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
    #[specta(type = Option<specta_typescript::Number>)]
    pub prompt_tokens: Option<i64>,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub completion_tokens: Option<i64>,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub span_count: Option<i64>,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub event_count: Option<i64>,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub tool_call_count: Option<i64>,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub error_count: Option<i64>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub duration_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
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
    #[specta(type = Option<specta_typescript::Number>)]
    pub byte_size: Option<i64>,
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub verified: bool,
}

/// Media types declared by the sealed traces inside a trusted archive, keyed by
/// the qualified digest of the artifact body.
///
/// Content addressing has no opinion about what a body is, so the bundle
/// manifest types every CAS blob as `application/octet-stream`. Only the sealed
/// Trace V5 document declares an artifact's `media_type` and `role`. Deriving
/// media presence from the blob inventory alone therefore reports "no media"
/// for every real capture whose frames live in the CAS, which silently hides
/// those traces from the media filter and from media-bound visuals.
fn declared_artifact_media(
    archive: Option<&[u8]>,
    assets: &[InspectedAsset],
) -> Result<std::collections::HashMap<String, (String, Option<String>)>> {
    let mut declared = std::collections::HashMap::new();
    let Some(archive) = archive else {
        return Ok(declared);
    };
    let sealed: Vec<String> = assets
        .iter()
        .filter(|asset| asset.available && asset.kind == "trace")
        .map(|asset| asset.relative_path.clone())
        .collect();
    if sealed.is_empty() {
        return Ok(declared);
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))
        .context("open trusted trace archive for artifact declarations")?;
    for path in sealed {
        let Ok(mut entry) = zip.by_name(&path) else {
            continue;
        };
        if entry.size() > MAX_SEALED_TRACE_BYTES {
            bail!("sealed trace {path} exceeds {MAX_SEALED_TRACE_BYTES} bytes");
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        let document: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("decode sealed trace {path}"))?;
        let Some(artifacts) = document.get("artifacts").and_then(Value::as_array) else {
            continue;
        };
        for artifact in artifacts {
            let (Some(digest), Some(media_type)) = (
                artifact.get("digest").and_then(Value::as_str),
                artifact.get("media_type").and_then(Value::as_str),
            ) else {
                continue;
            };
            let role = artifact
                .get("role")
                .and_then(Value::as_str)
                .map(str::to_owned);
            declared.insert(qualified_sha256(digest)?, (media_type.to_owned(), role));
        }
    }
    Ok(declared)
}

/// Publish the sealed trace's own media bodies into Workshop's blob CAS.
///
/// A replayed visual resolves a frame by content digest. While media existed
/// only in the live run relay, a sealed trace could name a frame it could not
/// show, and a consumer had no honest choice but to fall back to the live
/// latest-frame endpoint — which is exactly what replay must never do. The
/// bodies are already verified inside the trusted archive, so republishing them
/// under their own digest adds no new authority: it only makes the sealed
/// bundle self-sufficient for offline replay.
fn publish_declared_media(
    content: &ContentStore,
    archive: Option<&[u8]>,
    assets: &[InspectedAsset],
    declared: &std::collections::HashMap<String, (String, Option<String>)>,
) -> Result<Vec<String>> {
    let mut published = Vec::new();
    let (Some(archive), false) = (archive, declared.is_empty()) else {
        return Ok(published);
    };
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))
        .context("open trusted trace archive for media publication")?;
    for asset in assets {
        if !asset.available || !asset.verified {
            continue;
        }
        let Some(digest) = asset.bytes_digest.as_deref() else {
            continue;
        };
        let qualified = qualified_sha256(digest)?;
        let Some((media_type, _)) = declared.get(&qualified) else {
            continue;
        };
        if !is_media_type(media_type) || content.exists("blobs", &qualified[7..]) {
            continue;
        }
        let mut entry = match zip.by_name(&asset.relative_path) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.size() > MAX_MEDIA_OBJECT_BYTES {
            bail!(
                "sealed media {} exceeds {MAX_MEDIA_OBJECT_BYTES} bytes",
                asset.relative_path
            );
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        let stored = content.put_bytes("blobs", &bytes)?;
        if qualified_sha256(&stored)? != qualified {
            bail!(
                "sealed media {} did not hash to its declared digest",
                asset.relative_path
            );
        }
        published.push(qualified);
    }
    Ok(published)
}

const MAX_MEDIA_OBJECT_BYTES: u64 = 32 * 1024 * 1024;

fn is_media_type(media_type: &str) -> bool {
    media_type.starts_with("image/")
        || media_type.starts_with("audio/")
        || media_type.starts_with("video/")
}

const MAX_SEALED_TRACE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, specta::Type)]
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UsageEntry {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub prompt_tokens: i64,
    #[specta(type = specta_typescript::Number)]
    pub completion_tokens: i64,
    #[specta(type = specta_typescript::Number)]
    pub total_tokens: i64,
    pub cost_usd: Option<f64>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DataCounts {
    #[specta(type = specta_typescript::Number)]
    pub containers: i64,
    #[specta(type = specta_typescript::Number)]
    pub traces: i64,
    #[specta(type = specta_typescript::Number)]
    pub usage: i64,
}

#[derive(Clone)]
pub struct DataStore {
    db: Arc<Database>,
    content: ContentStore,
}

impl DataStore {
    pub fn new(db: Arc<Database>, content: ContentStore) -> Self {
        Self { db, content }
    }

    /// Where an import may stage bytes before the format authority inspects
    /// them. Callers use this instead of inventing a temp directory, so staged
    /// trace bytes live under the same instance root as what they become.
    pub fn staging_root(&self) -> std::path::PathBuf {
        self.content.root().join(".trace-staging")
    }

    pub async fn experiment_for_session(
        &self,
        session_id: String,
    ) -> Result<Option<crate::experiments::ExperimentGroup>> {
        self.db
            .clone()
            .run(move |conn| crate::experiments::load_for_session(conn, &session_id))
            .await
    }

    pub async fn experiment_create(
        &self,
        request: crate::experiments::ExperimentCreateRequest,
    ) -> Result<crate::experiments::ExperimentGroup> {
        self.db
            .clone()
            .run_transaction(move |conn| crate::experiments::create(conn, request))
            .await
    }

    pub async fn experiment_create_child(
        &self,
        request: crate::experiments::ExperimentChildCreateRequest,
    ) -> Result<crate::experiments::ExperimentGroup> {
        self.db
            .clone()
            .run_transaction(move |conn| crate::experiments::create_child(conn, request))
            .await
    }

    pub async fn experiment_relate(
        &self,
        request: crate::experiments::ExperimentRelateRequest,
    ) -> Result<crate::experiments::ExperimentGroup> {
        self.db
            .clone()
            .run_transaction(move |conn| crate::experiments::relate(conn, request))
            .await
    }

    pub async fn experiment_activate(
        &self,
        session_id: String,
        experiment_id: String,
    ) -> Result<crate::experiments::ExperimentGroup> {
        self.db
            .clone()
            .run_transaction(move |conn| {
                crate::experiments::activate(conn, &session_id, &experiment_id)
            })
            .await
    }

    pub async fn experiment_update(
        &self,
        request: crate::experiments::ExperimentUpdateRequest,
    ) -> Result<crate::experiments::ExperimentGroup> {
        self.db
            .clone()
            .run_transaction(move |conn| crate::experiments::update(conn, request))
            .await
    }

    pub async fn experiment_finalize(
        &self,
        request: crate::experiments::ExperimentFinalizeRequest,
    ) -> Result<crate::experiments::ExperimentGroup> {
        self.db
            .clone()
            .run_transaction(move |conn| crate::experiments::finalize(conn, request))
            .await
    }

    pub async fn experiments_list(
        &self,
        query: Option<String>,
    ) -> Result<Vec<crate::experiments::ExperimentGroup>> {
        self.db
            .clone()
            .run(move |conn| crate::experiments::list(conn, query.as_deref()))
            .await
    }

    pub async fn experiment_get(
        &self,
        id: String,
    ) -> Result<Option<crate::experiments::ExperimentGroup>> {
        self.db
            .clone()
            .run(move |conn| crate::experiments::get(conn, &id))
            .await
    }

    pub async fn research_log_list(
        &self,
        query: Option<String>,
        experiment_id: Option<String>,
    ) -> Result<Vec<crate::experiments::ResearchJournalEntry>> {
        self.db
            .clone()
            .run(move |conn| {
                crate::experiments::research_log_list(
                    conn,
                    query.as_deref(),
                    experiment_id.as_deref(),
                )
            })
            .await
    }

    pub async fn research_log_append(
        &self,
        request: crate::experiments::ResearchJournalAppendRequest,
    ) -> Result<crate::experiments::ResearchJournalEntry> {
        self.db
            .clone()
            .run_transaction(move |conn| crate::experiments::research_log_append(conn, request))
            .await
    }

    pub async fn experiment_attach_evidence(
        &self,
        request: crate::experiments::ExperimentEvidenceAttachRequest,
    ) -> Result<crate::experiments::ExperimentGroup> {
        self.db
            .clone()
            .run_transaction(move |conn| crate::experiments::attach_evidence(conn, request))
            .await
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
            let existing: Option<(String, String)> = conn.query_row(
                "SELECT id, metadata_json FROM containers WHERE base_url = ?1 LIMIT 1",
                params![&base_url],
                |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional()?;
            let previous_metadata = existing
                .as_ref()
                .and_then(|(_, raw)| serde_json::from_str::<Value>(raw).ok());
            let id = existing
                .map(|(id, _)| id)
                .unwrap_or_else(|| format!("ctr_{}", Uuid::new_v4().simple()));
            let name = request.name.filter(|value| !value.trim().is_empty()).unwrap_or_else(|| "Attached container".into());
            let location = request.location.unwrap_or_else(|| "local".into());
            let health_json = serde_json::to_string(&health)?;
            let metadata = merge_container_hydration_metadata(previous_metadata.as_ref(), metadata);
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
            let previous = load_container(conn, &id).ok();
            let now = Utc::now().to_rfc3339();
            let metadata = merge_container_hydration_metadata(
                previous.as_ref().map(|container| &container.metadata),
                metadata,
            );
            let changed = conn.execute(
                "UPDATE containers SET status=?1,health_json=?2,metadata_json=?3,task_family=COALESCE(?4,task_family),updated_at=?5 WHERE id=?6",
                params![&status, serde_json::to_string(&health)?, serde_json::to_string(&metadata)?, &task_family, &now, &id],
            )?;
            if changed == 0 { return Err(anyhow!("container not found: {id}")); }
            if let Some(previous) = previous {
                let registry = crate::domains::containers::registry_observation(&previous.status, &previous.health);
                let live = crate::domains::containers::live_observation(&status, &health);
                let stopped = previous.status == "stopped" || status == "stopped";
                if let Some(kind) = crate::domains::containers::classify_probe(&id, registry, live, stopped) {
                    if crate::platform::failure::repository::FailureRepository::open_for_container(conn, &id)?.is_none() {
                        crate::domains::containers::raise_probe_failure(conn, kind, &id, None)?;
                    }
                } else if status == crate::container_capabilities::READY_STATUS {
                    if let Some(open) = crate::platform::failure::repository::FailureRepository::open_for_container(conn, &id)? {
                        let _ = crate::platform::failure::FailureAuthority::transition(
                            conn,
                            open.failure_id.as_str(),
                            crate::platform::failure::FailureLifecycleState::Resolved,
                            crate::platform::failure::TransitionReason::Resolved,
                            "container_probe",
                        );
                        crate::domains::containers::clear_current(conn, &id)?;
                    }
                }
            }
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

    /// Run a typed trace query and freeze the result as an immutable snapshot.
    ///
    /// Reads the projection index rather than the sealed archives: a filtered
    /// list must never cost a re-parse of every V5 bundle. Re-running mints a
    /// new snapshot; an existing one is never rewritten, so a visual bound to
    /// a snapshot id shows the same rows forever.
    pub async fn query_traces(
        &self,
        query: crate::trace_query::TraceQuery,
        queried_at: String,
    ) -> Result<crate::trace_query::QuerySnapshot> {
        use crate::trace_query::{
            result_digest, snapshot_id, QuerySnapshot, TRACE_QUERY_RESULT_SCHEMA,
            TRACE_QUERY_SCHEMA,
        };

        let compiled = query.compile()?;
        let query_ast = serde_json::to_value(&query)?;
        let (rows, digests) = self
            .db
            .clone()
            .run(move |conn| run_trace_query(conn, &compiled))
            .await?;

        let truncated = digests.len() as i64 >= compiled_limit(&query);
        let digest = result_digest(&query_ast, &digests);
        let snapshot = QuerySnapshot {
            schema_version: TRACE_QUERY_RESULT_SCHEMA.into(),
            snapshot_id: snapshot_id(&digest),
            domain: "traces".into(),
            query_schema_version: TRACE_QUERY_SCHEMA.into(),
            query_ast,
            result_count: digests.len(),
            result_ids: digests,
            facets: json!({ "rows": rows }),
            result_digest: digest,
            queried_at,
            truncated,
        };
        let stored = snapshot.clone();
        self.db
            .clone()
            .run(move |conn| insert_query_snapshot(conn, &stored))
            .await?;
        Ok(snapshot)
    }

    pub async fn research_query(&self, query: Value) -> Result<crate::trace_query::QuerySnapshot> {
        let input = self.db.clone().run_transaction(move |conn| crate::trace_research::resolve_inputs(conn, &query)).await?;
        let snapshot = crate::trace_research::execute(input, &self.content.root().join("trace-research")).await?;
        let stored = snapshot.clone();
        self.db.clone().run(move |conn| insert_query_snapshot(conn, &stored)).await?;
        Ok(snapshot)
    }

    pub async fn research_source(&self, snapshot_id:String, result_id:String, selector:Option<Value>, offset:usize, limit:usize)->Result<Value>{
        let snapshot=self.query_snapshot(snapshot_id).await?;
        let i=snapshot.result_ids.iter().position(|id|id==&result_id).context("result ID is not in snapshot")?;
        let row=snapshot.facets["rows"].get(i).context("snapshot row missing")?;
        let selected=selector.unwrap_or_else(||row["selector"].clone());
        let allowed=selected==row["selector"]||selected==row["relatedSelector"]||row["evidence"].as_array().is_some_and(|items|items.contains(&selected));
        anyhow::ensure!(!selected.is_null()&&allowed,"selector must be one of this result's citations");
        let td=row["traceDigest"].as_str().context("select a trace result, not an aggregate")?.to_string();
        let path=self.db.clone().run(move|conn|Ok(conn.query_row("SELECT path FROM traces WHERE digest=?1",[td],|r|r.get::<_,String>(0))?)).await?;
        crate::trace_research::execute_value(json!({"operation":"source","archivePath":path,"selector":selected,"offset":offset,"limit":limit}),&self.content.root().join("trace-research")).await
    }

    pub async fn query_snapshot(
        &self,
        snapshot_id: String,
    ) -> Result<crate::trace_query::QuerySnapshot> {
        self.db
            .clone()
            .run(move |conn| load_query_snapshot(conn, &snapshot_id))
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

        // A validated standalone V5 is already a sealed authority. Retain its
        // bytes directly; do not mint a replacement capture just to obtain a ZIP.
        if inspected.inspection.input_kind == "standalone_trace"
            && inspected.inspection.trusted && validation_ok && accepted_compatibility
        {
            let trace = inspected.inspection.traces.first().context("standalone trace missing")?.clone();
            let path = stored_import_path.clone().context("standalone bytes missing")?;
            let digest = qualified_sha256(&trace.trace_digest)?;
            let row_id = format!("tracev5_{}", &digest[7..31]);
            let title = request.title.clone().unwrap_or_else(|| trace.trace_id.clone());
            let metadata = json!({"schemaVersion":"synth.trace.v5","producerTraceId":trace.trace_id,
                "storageKind":"standalone_trace","runId":trace.run_id,"trialId":trace.trial_id,
                "episodeId":trace.episode_id,"effort":trace.effort,"model":trace.model,
                "benchmark":trace.benchmark,"taskId":trace.task_id,"seed":trace.seed,
                "captureStatus":trace.capture_status,"lifecycleStatus":trace.lifecycle_status});
            let input = input_digest.clone();
            let validation = inspected.inspection.validation.clone();
            let compatibility = inspected.inspection.compatibility.clone();
            let container = request.container_id.clone();
            return self.db.clone().run_transaction(move |conn| {
                let duplicate: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM traces WHERE digest=?1)", [&digest], |r| r.get(0))?;
                conn.execute("INSERT INTO traces(id,digest,title,source,container_id,reward,metrics_json,path,metadata_json,created_at) VALUES(?1,?2,?3,'import',?4,?5,'[]',?6,?7,?8) ON CONFLICT(digest) DO UPDATE SET path=excluded.path,metadata_json=excluded.metadata_json,container_id=COALESCE(excluded.container_id,traces.container_id)", params![row_id,digest,title,container,trace.reward,path,metadata.to_string(),Utc::now().to_rfc3339()])?;
                let record = load_trace(conn, &digest)?.context("imported standalone trace missing")?;
                Ok((TraceBundleIngestResult { compatibility_level: compatibility, trusted:true, duplicate, input_digest:input, bundle_digest:None, archive_digest:None, traces:vec![record], validation }, None))
            }).await;
        }

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

        // Type the CAS blobs from the sealed traces, and publish their media
        // bodies, before anything is written: the blob inventory alone cannot
        // tell an observation frame from a log, and a replayed visual must be
        // able to resolve that frame without the live run relay.
        let declared_media = if trusted {
            declared_artifact_media(inspected.archive_bytes.as_deref(), &inspected.inspection.assets)?
        } else {
            std::collections::HashMap::new()
        };
        let published_media = publish_declared_media(
            &self.content,
            inspected.archive_bytes.as_deref(),
            &inspected.inspection.assets,
            &declared_media,
        )?;
        debug_assert!(published_media.len() <= declared_media.len());

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
        // The owning container is the immutable registry id, never a URL or a
        // display name: `traces.container_id` references `containers(id)` and is
        // what paid annotation names on its approval card.
        let owning_container = request
            .container_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        if let Some(id) = &owning_container {
            if id.contains("://") || id.contains('/') {
                bail!("owning container `{id}` must be the immutable container id from the registry, not a URL");
            }
        }
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
            if let Some(container_id) = &owning_container {
                let registered: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM containers WHERE id=?1)",
                    params![container_id],
                    |row| row.get(0),
                )?;
                if !registered {
                    bail!("owning container `{container_id}` is not registered; import with the immutable container id from container_list");
                }
            }
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

                let declared_for = |asset: &InspectedAsset| {
                    asset
                        .bytes_digest
                        .as_deref()
                        .and_then(|digest| qualified_sha256(digest).ok())
                        .and_then(|digest| declared_media.get(&digest))
                };
                let has_media = assets.iter().any(|asset| {
                    asset.available
                        && (is_media_type(&asset.media_type)
                            || declared_for(asset)
                                .is_some_and(|(media_type, _)| is_media_type(media_type)))
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
                        // The producer identity and Workshop's local trace row id
                        // intentionally occupy different namespaces. Keep both so
                        // reconciliation can validate the sealed bundle without
                        // comparing a rollout-owned id to `tracev5_...`.
                        "producerTraceId": trace.trace_id,
                        "runId": trace.run_id,
                        "trialId": trace.trial_id,
                        "episodeId": trace.episode_id,
                        "effort": trace.effort,
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
                        "INSERT INTO traces(id,digest,title,source,container_id,reward,metrics_json,path,metadata_json,created_at)
                         VALUES(?1,?2,?3,'import',?4,?5,'[]',?6,?7,?8)
                         ON CONFLICT(digest) DO UPDATE SET container_id=COALESCE(excluded.container_id,traces.container_id),path=excluded.path,metadata_json=excluded.metadata_json",
                        params![&row_id,&trace_digest,&title,&owning_container,trace.reward,archive_path,serde_json::to_string(&metadata)?,&now],
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
                         ON CONFLICT(trace_digest) DO UPDATE SET projector_version=excluded.projector_version,trace_kind=excluded.trace_kind,producer=excluded.producer,model=excluded.model,provider=excluded.provider,harness=excluded.harness,benchmark=excluded.benchmark,task_id=excluded.task_id,seed=excluded.seed,terminal_reason=excluded.terminal_reason,lifecycle_status=excluded.lifecycle_status,capture_status=excluded.capture_status,reward=excluded.reward,cost_usd=excluded.cost_usd,prompt_tokens=excluded.prompt_tokens,completion_tokens=excluded.completion_tokens,span_count=excluded.span_count,event_count=excluded.event_count,tool_call_count=excluded.tool_call_count,error_count=excluded.error_count,started_at=excluded.started_at,ended_at=excluded.ended_at,duration_ms=excluded.duration_ms,has_media=excluded.has_media,has_evidence=excluded.has_evidence,search_text=excluded.search_text",
                        params![&trace_digest,&trace.trace_kind,&trace.producer,&trace.model,&trace.provider,&trace.harness,&trace.benchmark,&trace.task_id,trace.seed,&trace.terminal_reason,&trace.lifecycle_status,&trace.capture_status,trace.reward,trace.cost_usd,trace.prompt_tokens,trace.completion_tokens,trace.span_count.unwrap_or(0),trace.event_count.unwrap_or(0),trace.tool_call_count.unwrap_or(0),trace.error_count.unwrap_or(0),&trace.started_at,&trace.ended_at,trace.duration_ms,has_media as i64,has_evidence as i64,&search_text],
                    )?;
                    records.push(load_trace(conn, &row_id)?.context("load imported trace")?);
                }

                for asset in &assets {
                    let Some(bytes_digest) = asset.bytes_digest.as_deref() else { continue; };
                    // A blob the sealed trace declares keeps that declaration, so a
                    // consumer can find an observation frame without reopening the
                    // archive. Anything the manifest already typed is left alone.
                    let (media_type, role) = match declared_for(asset) {
                        Some((declared, declared_role)) if !is_media_type(&asset.media_type) => (
                            declared.as_str(),
                            declared_role.as_deref().or(asset.role.as_deref()),
                        ),
                        _ => (asset.media_type.as_str(), asset.role.as_deref()),
                    };
                    conn.execute(
                        "INSERT INTO trace_assets(bundle_digest,relative_path,kind,role,bytes_digest,semantic_digest,media_type,byte_size,availability)
                         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
                         ON CONFLICT(bundle_digest,relative_path) DO UPDATE SET kind=excluded.kind,role=excluded.role,bytes_digest=excluded.bytes_digest,semantic_digest=excluded.semantic_digest,media_type=excluded.media_type,byte_size=excluded.byte_size,availability=excluded.availability",
                        params![bundle_digest,&asset.relative_path,&asset.kind,role,qualified_sha256(bytes_digest)?,asset.semantic_digest.as_deref().map(qualified_sha256).transpose()?,media_type,asset.byte_size.unwrap_or(0),if asset.available && asset.verified {"verified"} else if asset.available {"available"} else {"missing"}],
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

        if trusted {
            for trace in &result.0 {
                let digest=trace.digest.clone();
                let count=self.db.clone().run(move |c| Ok(c.query_row("SELECT event_count FROM trace_index WHERE trace_digest=?1",params![digest],|r|r.get::<_,i64>(0)).optional()?.unwrap_or(0))).await?;
                if count >= 1000 {
                    if let Err(error)=self.prepare_trace_windows(trace.digest.clone()).await {
                        crate::platform::logging::report("trace", "replay_index", format!("Replay index unavailable for {}: {error}",trace.digest));
                    }
                }
            }
        }

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

    /// Build a rebuildable replay index once; only bounded pages reach consumers.
    pub async fn prepare_trace_windows(&self, trace_digest: String) -> Result<String> {
        let trace_digest = qualified_sha256(&trace_digest)?;
        let lookup = trace_digest.clone();
        let cached = self.db.clone().run(move |c| {
            c.query_row("SELECT payload_digest FROM trace_projection_cache WHERE trace_digest=?1 AND projection_kind='rollout-inspector-windows' AND projector_version='workshop.windows.v2'",params![lookup],|r|r.get::<_,String>(0)).optional().map_err(Into::into)
        }).await?;
        if let Some(digest) = cached { return Ok(digest); }
        let projection = self.resolve_trace_projection(trace_digest.clone(), "rollout-inspector".into()).await?;
        let content = self.content.clone();
        let digest = tokio::task::spawn_blocking(move || build_trace_windows(&content, projection)).await??;
        let stored = digest.clone();
        self.db.clone().run(move |c| {
            c.execute("INSERT INTO trace_projection_cache(trace_digest,projection_kind,projection_schema,projector_version,source_digest,payload_digest,created_at) VALUES(?1,'rollout-inspector-windows','synth.trace-window-index.v2','workshop.windows.v2',?1,?2,?3) ON CONFLICT(trace_digest,projection_kind,projector_version) DO UPDATE SET payload_digest=excluded.payload_digest",params![trace_digest,stored,Utc::now().to_rfc3339()])?;Ok(())
        }).await?;
        Ok(digest)
    }

    pub async fn trace_view_window(&self, trace_digest: String, snapshot_digest: Option<String>, offset: usize, limit: usize) -> Result<Value> {
        let trace_digest=qualified_sha256(&trace_digest)?;
        if !(1..=200).contains(&limit) { bail!("trace window limit must be 1..200"); }
        let refresh_annotations = snapshot_digest.is_none();
        let mut snapshot=match snapshot_digest {Some(value)=>qualified_sha256(&value)?,None=>self.prepare_trace_windows(trace_digest.clone()).await?};
        let content=self.content.clone();
        let mut parsed:Value=serde_json::from_slice(&content.get_bytes_bounded("trace_views",snapshot.trim_start_matches("sha256:"),64*1024*1024)?)?;
        if parsed["schemaVersion"] != "synth.trace-window-index.v2" {
            return self.legacy_trace_view_window(trace_digest,Some(snapshot),offset,limit).await;
        }
        if refresh_annotations {
            let overlay = self.db.with_conn(|c| trace_annotation_overlay(c, &trace_digest))?;
            parsed["header"]["annotation_view"] = overlay;
            snapshot = format!("sha256:{}", content.put_bytes("trace_views", &serde_json::to_vec(&parsed)?)?);
        }
        tokio::task::spawn_blocking(move || read_trace_window(&content,parsed,&trace_digest,&snapshot,offset,limit)).await?
    }

    /// A bounded consumer view pinned to the verified full projection in CAS.
    /// The window is explicitly a view, never a newly sealed trace/projection.
    async fn legacy_trace_view_window(&self, trace_digest: String, snapshot_digest: Option<String>, offset: usize, limit: usize) -> Result<Value> {
        let trace_digest = qualified_sha256(&trace_digest)?;
        if !(1..=200).contains(&limit) { bail!("trace window limit must be 1..200"); }
        let (projection, snapshot) = if let Some(snapshot) = snapshot_digest {
            let snapshot = qualified_sha256(&snapshot)?;
            let bytes = self.content.get_bytes_bounded("trace_views", snapshot.trim_start_matches("sha256:"),64*1024*1024)?;
            (serde_json::from_slice::<ResolvedTraceProjection>(&bytes)?, snapshot)
        } else {
            let projection = self.resolve_trace_projection(trace_digest.clone(), "rollout-inspector".into()).await?;
            static CACHE_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let snapshot = {
                let _guard = CACHE_WRITE.lock().map_err(|_| anyhow!("trace view cache lock unavailable"))?;
                self.content.put_bytes("trace_views", &serde_json::to_vec(&projection)?)?
            };
            (projection, format!("sha256:{snapshot}"))
        };
        if projection.trace_digest != trace_digest || projection.projection_kind != "rollout-inspector" {
            bail!("trace window snapshot belongs to a different trace or projection");
        }
        let mut payload = projection.payload;
        let items = payload.pointer_mut("/visual/items").context("projection has no visual items")?;
        let all = items.as_array_mut().context("projection items must be an array")?;
        let total = all.len();
        if offset > total { bail!("trace window offset exceeds retained items"); }
        let end = offset.saturating_add(limit).min(total);
        let mut window = all[offset..end].to_vec();
        for item in &mut window {
            if let Some(detail) = item.get_mut("detail") {
                let text = serde_json::to_string(detail)?;
                if text.len() > 16_000 {
                    *detail = json!({"preview":text.chars().take(4000).collect::<String>(),"payloadTruncated":true,"sourceSelectorRetained":true});
                }
            }
        }
        *items = Value::Array(window);
        payload["schema_version"] = json!("synth.trace-projection.rollout-inspector-window.v1");
        if let Some(object) = payload.as_object_mut() { object.remove("content_digest"); }
        if let Some(object) = payload.get_mut("visual").and_then(Value::as_object_mut) { object.remove("content_digest"); }
        payload["view_window"] = json!({"schemaVersion":"synth.trace-view-window.v1","snapshotDigest":snapshot,"sourceProjectionDigest":projection.payload_digest,"offset":offset,"limit":limit,"total":total,"nextOffset":if end<total {Some(end)} else {None}});
        if serde_json::to_vec(&payload)?.len() > 4*1024*1024 { bail!("trace window exceeds 4 MiB; reduce its limit"); }
        Ok(payload)
    }

    pub async fn list_usage(&self, limit: i64) -> Result<Vec<UsageEntry>> {
        self.db
            .clone()
            .run(move |conn| list_usage(conn, limit.clamp(1, 2000)))
            .await
    }

    pub async fn counts(&self) -> Result<DataCounts> {
        self.db
            .clone()
            .run(|conn| {
                Ok(DataCounts {
                    containers: conn
                        .query_row("SELECT COUNT(*) FROM containers", [], |row| row.get(0))?,
                    traces: conn.query_row("SELECT COUNT(*) FROM traces", [], |row| row.get(0))?,
                    usage: conn
                        .query_row("SELECT COUNT(*) FROM usage_records", [], |row| row.get(0))?,
                })
            })
            .await
    }
}

/// Independent findings are a view overlay, never mutations of sealed trace bytes.
fn trace_annotation_overlay(c: &Connection, trace: &str) -> Result<Value> {
    let mut stmt=c.prepare("SELECT f.finding_id,f.annotator_id,f.taxonomy_label,f.target_selector_json,f.evidence_selectors_json,f.payload_json,f.status,(SELECT decision FROM annotation_reviews r WHERE r.finding_id=f.finding_id AND r.evidence_head_digest=f.evidence_head_digest ORDER BY r.created_at DESC,r.review_id DESC LIMIT 1) FROM annotation_findings f JOIN annotation_evidence_heads h ON h.digest=f.evidence_head_digest WHERE h.trace_digest=?1 ORDER BY f.finding_id LIMIT 201")?;
    let rows=stmt.query_map([trace],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,Option<String>>(7)?)))?;
    let mut records=vec![];let mut bytes=0;let mut truncated=false;
    for row in rows {
        let (id,author,label,target,evidence,payload,status,review)=row?;
        let payload:Value=serde_json::from_str(&payload)?;
        let canonical=payload.get("sourceAnnotation").unwrap_or(&payload);
        let body=canonical.get("rationale").or_else(||canonical.get("summary")).and_then(Value::as_str).unwrap_or("");
        let record=json!({"id":id,"target":serde_json::from_str::<Value>(&target)?,"evidence":serde_json::from_str::<Value>(&evidence)?,"body":body,"labels":canonical.get("labels").cloned().unwrap_or(json!(label.into_iter().collect::<Vec<_>>())) ,"author":author,"reviewState":review.map(Value::String).unwrap_or_else(||canonical.get("review_state").cloned().unwrap_or(json!(status))),"supersedesId":canonical.get("supersedes_id"),"grounding":canonical.get("grounding")});
        let size=serde_json::to_vec(&record)?.len();
        if records.len()>=200 || bytes+size>256*1024 {truncated=true;break;}
        bytes+=size;records.push(record);
    }
    Ok(json!({"schemaVersion":"synth.trace-annotation-view.v1","records":records,"truncated":truncated,"scope":"independent annotations pinned when this view opened"}))
}

fn build_trace_windows(content: &ContentStore, projection: ResolvedTraceProjection) -> Result<String> {
    static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard=WRITE.lock().map_err(|_|anyhow!("trace page cache lock unavailable"))?;
    let mut header=projection.payload;
    let items=header.pointer_mut("/visual/items").context("projection has no visual items")?;
    let Value::Array(mut all)=std::mem::take(items) else {bail!("projection items must be an array");};
    let total=all.len();
    for item in &mut all {
        if let Some(detail)=item.get_mut("detail") {
            let raw=serde_json::to_string(detail)?;
            if raw.len()>16_000 {*detail=json!({"preview":raw.chars().take(4000).collect::<String>(),"payloadTruncated":true,"sourceSelectorRetained":true});}
        }
    }
    let mut chunks=Vec::new();
    for page in all.chunks(200) {
        let bytes=serde_json::to_vec(page)?;
        if bytes.len()>4*1024*1024 {bail!("trace page exceeds 4 MiB");}
        chunks.push(content.put_bytes("trace_views",&bytes)?);
    }
    if let Some(object)=header.as_object_mut() {object.remove("content_digest");}
    if let Some(object)=header.get_mut("visual").and_then(Value::as_object_mut) {object.remove("content_digest");}
    header["schema_version"]=json!("synth.trace-projection.rollout-inspector-window.v1");
    let index=json!({"schemaVersion":"synth.trace-window-index.v2","traceDigest":projection.trace_digest,"sourceProjectionDigest":projection.payload_digest,"header":header,"total":total,"chunks":chunks});
    let bytes=serde_json::to_vec(&index)?;
    if bytes.len()>4*1024*1024 {bail!("trace window index exceeds 4 MiB");}
    Ok(format!("sha256:{}",content.put_bytes("trace_views",&bytes)?))
}

fn read_trace_window(content: &ContentStore,index: Value,trace: &str,snapshot: &str,offset: usize,limit: usize) -> Result<Value> {
    if index["traceDigest"] != trace {bail!("trace window snapshot belongs to a different trace");}
    let total=index["total"].as_u64().context("invalid trace window total")? as usize;
    if offset>total {bail!("trace window offset exceeds retained items");}
    let end=offset.saturating_add(limit).min(total);
    let mut items=Vec::new();
    if end>offset {
        for page in offset/200..=(end-1)/200 {
            let digest=index["chunks"][page].as_str().context("trace page missing")?;
            let rows:Vec<Value>=serde_json::from_slice(&content.get_bytes_bounded("trace_views",digest,4*1024*1024)?)?;
            let start=offset.saturating_sub(page*200);
            let finish=(end-page*200).min(rows.len());
            if start>finish || rows.len()>200 {bail!("invalid trace page range");}
            items.extend_from_slice(&rows[start..finish]);
        }
    }
    if items.len()!=end-offset {bail!("trace window is incomplete");}
    let mut payload=index["header"].clone();
    payload["visual"]["items"]=json!(items);
    payload["view_window"]=json!({"schemaVersion":"synth.trace-view-window.v1","snapshotDigest":snapshot,"sourceProjectionDigest":index["sourceProjectionDigest"],"offset":offset,"limit":limit,"total":total,"nextOffset":if end<total {Some(end)} else {None}});
    if serde_json::to_vec(&payload)?.len()>4*1024*1024 {bail!("trace window exceeds 4 MiB");}
    Ok(payload)
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
        current_failure_id: row.get(12).ok(),
        health: parse_json(row.get(8)?)?,
        metadata: parse_json(row.get(9)?)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn load_container(conn: &Connection, id: &str) -> Result<ContainerDeployment> {
    conn.query_row(
        "SELECT id,name,location,status,base_url,pool_id,task_family,last_rollout_id,health_json,metadata_json,created_at,updated_at,current_failure_id FROM containers WHERE id=?1",
        params![id], container_from_row,
    ).optional()?.ok_or_else(|| anyhow!("container not found: {id}"))
}

fn list_containers(conn: &Connection) -> Result<Vec<ContainerDeployment>> {
    let mut statement = conn.prepare(
        "SELECT id,name,location,status,base_url,pool_id,task_family,last_rollout_id,health_json,metadata_json,created_at,updated_at,current_failure_id FROM containers ORDER BY updated_at DESC, id",
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

/// Bind every compiled parameter positionally; nothing is formatted into SQL.
fn run_trace_query(
    conn: &Connection,
    compiled: &crate::trace_query::CompiledQuery,
) -> Result<(Vec<Value>, Vec<String>)> {
    let mut statement = conn.prepare(&compiled.sql)?;
    let bound: Vec<Box<dyn rusqlite::ToSql>> = compiled
        .params
        .iter()
        .map(|value| -> Box<dyn rusqlite::ToSql> {
            match value {
                Value::String(text) => Box::new(text.clone()),
                Value::Number(number) if number.is_i64() => Box::new(number.as_i64().unwrap()),
                Value::Number(number) => Box::new(number.as_f64().unwrap_or_default()),
                Value::Bool(flag) => Box::new(i64::from(*flag)),
                other => Box::new(other.to_string()),
            }
        })
        .collect();
    let rows = statement
        .query_map(
            rusqlite::params_from_iter(bound.iter().map(|value| value.as_ref())),
            |row| {
                Ok(json!({
                    "traceDigest": row.get::<_, String>(0)?,
                    "model": row.get::<_, Option<String>>(1)?,
                    "provider": row.get::<_, Option<String>>(2)?,
                    "benchmark": row.get::<_, Option<String>>(3)?,
                    "taskId": row.get::<_, Option<String>>(4)?,
                    "lifecycleStatus": row.get::<_, Option<String>>(5)?,
                    "captureStatus": row.get::<_, Option<String>>(6)?,
                    "reward": row.get::<_, Option<f64>>(7)?,
                    "costUsd": row.get::<_, Option<f64>>(8)?,
                    "eventCount": row.get::<_, i64>(9)?,
                    "toolCallCount": row.get::<_, i64>(10)?,
                    "errorCount": row.get::<_, i64>(11)?,
                    "durationMs": row.get::<_, Option<i64>>(12)?,
                    "startedAt": row.get::<_, Option<String>>(13)?,
                    "hasMedia": row.get::<_, i64>(14)? != 0,
                    "hasEvidence": row.get::<_, i64>(15)? != 0,
                }))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let digests = rows
        .iter()
        .filter_map(|row| row.get("traceDigest").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    Ok((rows, digests))
}

fn compiled_limit(query: &crate::trace_query::TraceQuery) -> i64 {
    query
        .limit
        .unwrap_or(crate::trace_query::MAX_LIMIT)
        .clamp(1, crate::trace_query::MAX_LIMIT)
}

/// Snapshots are append-only. `INSERT OR IGNORE` makes re-taking an identical
/// query idempotent rather than rewriting history under an existing id.
fn insert_query_snapshot(
    conn: &Connection,
    snapshot: &crate::trace_query::QuerySnapshot,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO query_snapshots(
            snapshot_id, domain, query_schema_version, query_ast, result_ids,
            result_count, facets, result_digest, queried_at, truncated
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            snapshot.snapshot_id,
            snapshot.domain,
            snapshot.query_schema_version,
            serde_json::to_string(&snapshot.query_ast)?,
            serde_json::to_string(&snapshot.result_ids)?,
            snapshot.result_count as i64,
            serde_json::to_string(&snapshot.facets)?,
            snapshot.result_digest,
            snapshot.queried_at,
            i64::from(snapshot.truncated),
        ],
    )?;
    Ok(())
}

fn load_query_snapshot(
    conn: &Connection,
    snapshot_id: &str,
) -> Result<crate::trace_query::QuerySnapshot> {
    conn.query_row(
        "SELECT snapshot_id, domain, query_schema_version, query_ast, result_ids,
                result_count, facets, result_digest, queried_at, truncated
         FROM query_snapshots WHERE snapshot_id = ?1",
        [snapshot_id],
        |row| {
            Ok(crate::trace_query::QuerySnapshot {
                schema_version: if row.get::<_, String>(2)? == crate::trace_research::SCHEMA { "synth.trace-query-result.v2".into() } else { crate::trace_query::TRACE_QUERY_RESULT_SCHEMA.into() },
                snapshot_id: row.get(0)?,
                domain: row.get(1)?,
                query_schema_version: row.get(2)?,
                query_ast: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
                result_ids: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
                result_count: row.get::<_, i64>(5)? as usize,
                facets: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                result_digest: row.get(7)?,
                queried_at: row.get(8)?,
                truncated: row.get::<_, i64>(9)? != 0,
            })
        },
    )
    .optional()?
    .ok_or_else(|| anyhow!("query snapshot not found: {snapshot_id}"))
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

/// Raw request-level inspection feed over the one authoritative
/// `usage_records` ledger (legacy `usage_ledger` rows were folded in by
/// migration 11). The exposed cost is the settled charge when one exists,
/// otherwise a Backend-owned Synth Cloud estimate. Legacy local tariff
/// estimates are never exposed as money.
fn list_usage(conn: &Connection, limit: i64) -> Result<Vec<UsageEntry>> {
    let mut statement = conn.prepare(
        "SELECT id, provider, model_id AS model, session_id, run_id,
                COALESCE(input_tokens, 0) AS prompt_tokens,
                COALESCE(output_tokens, 0) AS completion_tokens,
                COALESCE(total_tokens, 0) AS total_tokens,
                CASE
                    WHEN cost_source IN ('provider_reported', 'synth_cloud')
                         AND billed_cost_usd IS NOT NULL THEN billed_cost_usd
                    WHEN cost_source = 'synth_cloud' THEN estimated_cost_usd
                    ELSE NULL
                END AS cost_usd,
                created_at
         FROM usage_records
         ORDER BY created_at DESC, id LIMIT ?1",
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
    async fn trace_window_annotation_overlay_pins_reviews_across_pages() {
        let dir=tempdir().unwrap();let storage=Storage::open(dir.path()).unwrap();
        let data=DataStore::new(storage.database().clone(),ContentStore::new(storage.content_root()));
        let trace=format!("sha256:{}","c".repeat(64));
        let base=build_trace_windows(&data.content,ResolvedTraceProjection{trace_digest:trace.clone(),projection_kind:"rollout-inspector".into(),projection_schema:"synth.trace-projection.rollout-inspector.v1".into(),payload_digest:format!("sha256:{}","d".repeat(64)),relative_path:String::new(),payload:json!({"trace_id":"t","trace_digest":trace,"visual":{"items":[{"item_id":"event"},{"item_id":"next"}]}})}).unwrap();
        storage.database().with_conn(|c|{
            c.execute("INSERT INTO trace_projection_cache(trace_digest,projection_kind,projection_schema,projector_version,source_digest,payload_digest,created_at) VALUES(?1,'rollout-inspector-windows','synth.trace-window-index.v2','workshop.windows.v2',?1,?2,'now')",params![trace,base])?;
            c.execute("INSERT INTO annotation_evidence_heads(digest,trace_digest,summary_json,created_at,updated_at) VALUES('head',?1,'{}','now','now')",[&trace])?;
            c.execute(r#"INSERT INTO annotation_findings(finding_id,evidence_head_digest,annotator_id,status,target_selector_json,payload_json,created_at) VALUES('finding','head','ordinary','applied',?1,'{"sourceAnnotation":{"rationale":"Independent finding"}}','now')"#,[json!({"trace_id":"t","trace_digest":trace,"kind":"event","entity_id":"event"}).to_string()])?;Ok(())
        }).unwrap();
        let first=data.trace_view_window(trace.clone(),None,0,1).await.unwrap();
        assert_eq!(first["annotation_view"]["records"][0]["body"],"Independent finding");
        let pinned=first["view_window"]["snapshotDigest"].as_str().unwrap().to_string();
        storage.database().with_conn(|c|{c.execute("INSERT INTO annotation_reviews(review_id,finding_id,evidence_head_digest,decision,created_at) VALUES('review','finding','head','accepted','later')",[])?;Ok(())}).unwrap();
        let next=data.trace_view_window(trace.clone(),Some(pinned),1,1).await.unwrap();
        assert_eq!(next["annotation_view"],first["annotation_view"]);
        let refreshed=data.trace_view_window(trace,None,0,1).await.unwrap();
        assert_eq!(refreshed["annotation_view"]["records"][0]["reviewState"],"accepted");
        assert_ne!(refreshed["view_window"]["snapshotDigest"],first["view_window"]["snapshotDigest"]);
    }

    #[test]
    fn trace_window_chunks_pin_identity_and_reject_damaged_pages() {
        let dir = tempdir().unwrap();
        let content = ContentStore::new(dir.path());
        let trace = format!("sha256:{}", "a".repeat(64));
        let snapshot = build_trace_windows(&content, ResolvedTraceProjection {
            trace_digest: trace.clone(), projection_kind: "rollout-inspector".into(),
            projection_schema: "synth.trace-projection.rollout-inspector.v1".into(),
            payload_digest: format!("sha256:{}", "b".repeat(64)), relative_path: String::new(),
            payload: json!({"trace_digest":trace,"content_digest":"sealed", "visual":{"items":(0..401).map(|i| json!({"id":i,"selector":{"eventId":i},"detail":"x".repeat(17000)})).collect::<Vec<_>>()}}),
        }).unwrap();
        let index: Value = serde_json::from_slice(&content.get_bytes("trace_views", snapshot.trim_start_matches("sha256:")).unwrap()).unwrap();
        let across = read_trace_window(&content, index.clone(), &trace, &snapshot, 199, 2).unwrap();
        assert_eq!(across["visual"]["items"][0]["id"], 199);
        assert_eq!(across["visual"]["items"][1]["selector"]["eventId"], 200);
        assert_eq!(across["visual"]["items"][1]["detail"]["payloadTruncated"], true);
        assert!(across.get("content_digest").is_none());
        assert!(read_trace_window(&content, index.clone(), "another-trace", &snapshot, 0, 1).is_err());
        assert!(read_trace_window(&content, index.clone(), &trace, &snapshot, 402, 1).is_err());
        let chunk = index["chunks"][1].as_str().unwrap();
        std::fs::write(content.path_for("trace_views", chunk), b"[]").unwrap();
        assert!(read_trace_window(&content, index.clone(), &trace, &snapshot, 199, 2).is_err());
        // Damage to an unrelated page cannot force full-trace reads or hide a valid page.
        assert_eq!(read_trace_window(&content, index.clone(), &trace, &snapshot, 400, 1).unwrap()["visual"]["items"][0]["id"], 400);
        std::fs::remove_file(content.path_for("trace_views", index["chunks"][2].as_str().unwrap())).unwrap();
        assert!(read_trace_window(&content, index, &trace, &snapshot, 400, 1).is_err());
        let digest = content.put_bytes("trace_views", b"12345").unwrap();
        assert!(content.get_bytes_bounded("trace_views", &digest, 4).is_err());
    }

    async fn seeded_index_store() -> (tempfile::TempDir, DataStore) {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let db = storage.database().clone();
        db.with_conn(|conn| {
            for (digest, benchmark, status, reward, started) in [
                ("sha256:a1", "craftax", "failed", 0.10, "2026-08-14T10:00:00Z"),
                ("sha256:b2", "craftax", "completed", 0.90, "2026-08-14T11:00:00Z"),
                ("sha256:c3", "banking77", "failed", 0.30, "2026-08-14T12:00:00Z"),
            ] {
                conn.execute(
                    "INSERT INTO trace_index(trace_digest,projector_version,benchmark,lifecycle_status,reward,started_at,search_text)
                     VALUES(?1,'v1',?2,?3,?4,?5,?6)",
                    params![digest, benchmark, status, reward, started, format!("{benchmark} {status}")],
                )?;
            }
            Ok(())
        })
        .unwrap();
        let data = DataStore::new(db, ContentStore::new(storage.content_root()));
        (dir, data)
    }

    #[tokio::test]
    async fn a_typed_query_reads_the_index_and_freezes_its_result() {
        let (_dir, data) = seeded_index_store().await;
        let query = crate::trace_query::TraceQuery {
            r#where: Some(crate::trace_query::TraceWhere {
                benchmark: vec!["craftax".into()],
                lifecycle_status: vec!["failed".into()],
                ..Default::default()
            }),
            ..Default::default()
        };
        let snapshot = data
            .query_traces(query.clone(), "2026-08-15T10:22:00Z".into())
            .await
            .unwrap();

        assert_eq!(snapshot.result_ids, vec!["sha256:a1".to_string()]);
        assert_eq!(snapshot.result_count, 1);
        assert_eq!(snapshot.domain, "traces");
        assert_eq!(snapshot.queried_at, "2026-08-15T10:22:00Z");
        assert!(!snapshot.truncated);
        // The snapshot carries the question as well as the answer, so the page
        // can state what the reader is looking at.
        assert_eq!(snapshot.query_ast["where"]["benchmark"][0], "craftax");

        let reloaded = data
            .query_snapshot(snapshot.snapshot_id.clone())
            .await
            .unwrap();
        assert_eq!(reloaded, snapshot);
    }

    #[tokio::test]
    async fn re_running_a_query_never_rewrites_an_existing_snapshot() {
        let (_dir, data) = seeded_index_store().await;
        let query = crate::trace_query::TraceQuery::default();
        let first = data
            .query_traces(query.clone(), "2026-08-15T10:00:00Z".into())
            .await
            .unwrap();

        // Same question, same rows, later clock: the stored snapshot keeps its
        // original timestamp rather than being updated in place.
        let second = data
            .query_traces(query, "2026-08-15T23:59:00Z".into())
            .await
            .unwrap();
        assert_eq!(first.snapshot_id, second.snapshot_id);
        let stored = data
            .query_snapshot(first.snapshot_id.clone())
            .await
            .unwrap();
        assert_eq!(stored.queried_at, "2026-08-15T10:00:00Z");
    }

    #[tokio::test]
    async fn a_different_result_set_is_a_different_snapshot() {
        let (_dir, data) = seeded_index_store().await;
        let all = data
            .query_traces(crate::trace_query::TraceQuery::default(), "t".into())
            .await
            .unwrap();
        let failed = data
            .query_traces(
                crate::trace_query::TraceQuery {
                    r#where: Some(crate::trace_query::TraceWhere {
                        lifecycle_status: vec!["failed".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                "t".into(),
            )
            .await
            .unwrap();
        assert_eq!(all.result_count, 3);
        assert_eq!(failed.result_count, 2);
        assert_ne!(all.snapshot_id, failed.snapshot_id);
    }

    #[tokio::test]
    async fn a_capped_result_reports_that_it_was_cut() {
        let (_dir, data) = seeded_index_store().await;
        let snapshot = data
            .query_traces(
                crate::trace_query::TraceQuery {
                    limit: Some(2),
                    ..Default::default()
                },
                "t".into(),
            )
            .await
            .unwrap();
        assert_eq!(snapshot.result_count, 2);
        assert!(
            snapshot.truncated,
            "a truncated result must not read as complete"
        );
    }

    /// A trusted, self-contained Trace V5 inspection over an arbitrary archive
    /// body, shaped the way `synth-trace inspect-input` reports it.
    fn trusted_inspection(archive: &[u8], trace_hex: &str) -> crate::trace_ingest::InspectedInput {
        let archive_digest = format!("sha256:{:x}", Sha256::digest(archive));
        let inspection_json = serde_json::json!({
            "schema_version": "synth.trace-inspection.v1",
            "input_kind": "bundle_archive",
            "compatibility": "native",
            "source_bytes_digest": archive_digest,
            "bundle_digest": format!("sha256:{}", "b".repeat(64)),
            "archive_digest": archive_digest,
            "self_contained": true,
            "trusted": true,
            "validation": {"valid": true, "issues": []},
            "traces": [{
                "trace_id": "rollout-1",
                "trace_digest": format!("sha256:{trace_hex}"),
                "schema_version": "synth.trace.v5",
                "reward": 0.5
            }],
            "assets": [],
            "projections": []
        });
        crate::trace_ingest::InspectedInput {
            inspection: serde_json::from_value(inspection_json.clone()).unwrap(),
            inspection_json,
            archive_bytes: Some(archive.to_vec()),
            raw_file_bytes: None,
        }
    }

    fn ingest_request(container_id: Option<&str>) -> crate::trace_ingest::TraceBundleIngestRequest {
        crate::trace_ingest::TraceBundleIngestRequest {
            source_path: "/nonexistent/bundle.zip".into(),
            source_kind: Some("test".into()),
            title: None,
            source_uri: None,
            container_id: container_id.map(str::to_owned),
        }
    }

    async fn stored_owner(db: &crate::storage::Database, digest: &str) -> Option<String> {
        let digest = digest.to_string();
        db.clone()
            .run(move |conn| {
                Ok(conn.query_row(
                    "SELECT container_id FROM traces WHERE digest=?1",
                    params![digest],
                    |row| row.get::<_, Option<String>>(0),
                )?)
            })
            .await
            .unwrap()
    }

    /// A real capture stores its frames in the content-addressed blob store,
    /// whose manifest types every body as `application/octet-stream`. Only the
    /// sealed trace declares that a blob is a PNG observation, so ingest must
    /// read the declaration; otherwise every populated capture reports
    /// `hasMedia: false` and disappears from the media filter.
    fn inspection_with_a_sealed_png_artifact(
        trace_hex: &str,
    ) -> crate::trace_ingest::InspectedInput {
        let frame = b"\x89PNG\r\n\x1a\n deterministic test frame";
        let frame_digest = format!("sha256:{:x}", Sha256::digest(frame));
        let sealed = serde_json::json!({
            "schema_version": "synth.trace.v5",
            "trace_id": "rollout-1",
            "artifacts": [{
                "artifact_id": "art_frame",
                "digest": frame_digest,
                "logical_name": "frame-000.png",
                "media_type": "image/png",
                "role": "observation",
                "size_bytes": frame.len(),
                "uri": "blobs/sha256/aa/frame"
            }]
        });
        let mut archive = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut archive));
            let options = zip::write::SimpleFileOptions::default();
            writer
                .start_file("traces/rollout-1/sealed/trace.json", options)
                .unwrap();
            std::io::Write::write_all(&mut writer, sealed.to_string().as_bytes()).unwrap();
            writer.start_file("blobs/sha256/aa/frame", options).unwrap();
            std::io::Write::write_all(&mut writer, frame).unwrap();
            writer.finish().unwrap();
        }
        let archive_digest = format!("sha256:{:x}", Sha256::digest(&archive));
        let inspection_json = serde_json::json!({
            "schema_version": "synth.trace-inspection.v1",
            "input_kind": "bundle_archive",
            "compatibility": "native",
            "source_bytes_digest": archive_digest,
            "bundle_digest": format!("sha256:{}", "c".repeat(64)),
            "archive_digest": archive_digest,
            "self_contained": true,
            "trusted": true,
            "validation": {"valid": true, "issues": []},
            "traces": [{
                "trace_id": "rollout-1",
                "trace_digest": format!("sha256:{trace_hex}"),
                "schema_version": "synth.trace.v5"
            }],
            "assets": [
                {
                    "path": "traces/rollout-1/sealed/trace.json",
                    "kind": "trace",
                    "role": "sealed_trace",
                    "bytes_digest": format!("sha256:{:x}", Sha256::digest(sealed.to_string().as_bytes())),
                    "media_type": "application/json",
                    "byte_size": sealed.to_string().len(),
                    "available": true,
                    "verified": true
                },
                {
                    "path": "blobs/sha256/aa/frame",
                    "kind": "blob",
                    "role": "blob",
                    "bytes_digest": frame_digest,
                    "media_type": "application/octet-stream",
                    "byte_size": frame.len(),
                    "available": true,
                    "verified": true
                }
            ],
            "projections": []
        });
        crate::trace_ingest::InspectedInput {
            inspection: serde_json::from_value(inspection_json.clone()).unwrap(),
            inspection_json,
            archive_bytes: Some(archive),
            raw_file_bytes: None,
        }
    }

    #[tokio::test]
    async fn sealed_artifact_declarations_type_the_content_addressed_blobs() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let db = storage.database().clone();
        let content = ContentStore::new(storage.content_root());
        let data = DataStore::new(db.clone(), content.clone());
        let trace_hex = "d".repeat(64);
        let frame = b"\x89PNG\r\n\x1a\n deterministic test frame";
        let frame_digest = format!("{:x}", Sha256::digest(frame));
        assert!(
            !content.exists("blobs", &frame_digest),
            "the frame must not already be published"
        );
        let (result, _) = data
            .commit_inspected_trace(
                ingest_request(None),
                inspection_with_a_sealed_png_artifact(&trace_hex),
            )
            .await
            .unwrap();
        assert!(result.trusted);

        let (has_media, blob_media, blob_role, sealed_media): (i64, String, Option<String>, String) =
            db.clone()
                .run(move |conn| {
                    let has_media = conn.query_row(
                        "SELECT has_media FROM trace_index WHERE trace_digest=?1",
                        params![format!("sha256:{}", "d".repeat(64))],
                        |row| row.get(0),
                    )?;
                    let (blob_media, blob_role) = conn.query_row(
                        "SELECT media_type, role FROM trace_assets WHERE relative_path='blobs/sha256/aa/frame'",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )?;
                    let sealed_media = conn.query_row(
                        "SELECT media_type FROM trace_assets WHERE kind='trace'",
                        [],
                        |row| row.get(0),
                    )?;
                    Ok((has_media, blob_media, blob_role, sealed_media))
                })
                .await
                .unwrap();
        assert_eq!(has_media, 1, "a sealed PNG artifact is media");
        assert_eq!(blob_media, "image/png", "the blob keeps its declared type");
        assert_eq!(blob_role.as_deref(), Some("observation"));
        assert_eq!(
            sealed_media, "application/json",
            "an asset the manifest already typed is left alone"
        );

        // A trace first indexed without media (an older ingest, or a bundle
        // whose media arrived later) must not keep the stale flag: the index is
        // what the media filter reads, so a silent disagreement with the trace
        // record hides a populated capture.
        db.clone()
            .run(|conn| {
                conn.execute("UPDATE trace_index SET has_media=0, has_evidence=0", [])?;
                Ok(())
            })
            .await
            .unwrap();
        data.commit_inspected_trace(
            ingest_request(None),
            inspection_with_a_sealed_png_artifact(&"d".repeat(64)),
        )
        .await
        .unwrap();
        let reindexed: i64 = db
            .clone()
            .run(|conn| {
                Ok(conn.query_row("SELECT has_media FROM trace_index", [], |row| row.get(0))?)
            })
            .await
            .unwrap();
        assert_eq!(reindexed, 1, "re-import must correct a stale media flag");

        // Offline replay resolves a frame by digest from Workshop's blob CAS.
        // Without this the sealed trace names a frame it cannot show, and the
        // only way to render one is the live endpoint replay must never poll.
        assert_eq!(
            content.get_bytes("blobs", &frame_digest).unwrap(),
            frame,
            "the sealed media body must be resolvable by its own digest"
        );
    }

    #[tokio::test]
    async fn container_driven_import_records_the_owning_registry_id() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let db = storage.database().clone();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO containers(id,name,location,status,health_json,metadata_json,created_at,updated_at) VALUES('ctr_owner','Local','local','ready','{}','{}','2026-01-01','2026-01-01')", [])?;
            Ok(())
        })
        .unwrap();
        let data = DataStore::new(db.clone(), ContentStore::new(storage.content_root()));
        let trace_hex = "a".repeat(64);
        let digest = format!("sha256:{trace_hex}");

        // A bare file import knows no owner: the column stays NULL rather than
        // guessing from a title or a URL.
        let (result, _) = data
            .commit_inspected_trace(
                ingest_request(None),
                trusted_inspection(b"archive-one", &trace_hex),
            )
            .await
            .unwrap();
        assert!(result.trusted);
        assert_eq!(result.traces[0].container_id, None);
        assert_eq!(stored_owner(&db, &digest).await, None);

        // The same trace imported from its container fills the owner in.
        let (result, _) = data
            .commit_inspected_trace(
                ingest_request(Some("ctr_owner")),
                trusted_inspection(b"archive-one", &trace_hex),
            )
            .await
            .unwrap();
        assert_eq!(result.traces[0].container_id.as_deref(), Some("ctr_owner"));
        assert_eq!(
            stored_owner(&db, &digest).await.as_deref(),
            Some("ctr_owner")
        );

        // A later ownerless re-import never erases a recorded owner.
        data.commit_inspected_trace(
            ingest_request(None),
            trusted_inspection(b"archive-one", &trace_hex),
        )
        .await
        .unwrap();
        assert_eq!(
            stored_owner(&db, &digest).await.as_deref(),
            Some("ctr_owner")
        );
    }

    #[tokio::test]
    async fn import_refuses_an_owner_that_is_not_a_registry_id() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let data = DataStore::new(
            storage.database().clone(),
            ContentStore::new(storage.content_root()),
        );
        let trace_hex = "c".repeat(64);
        let url = data
            .commit_inspected_trace(
                ingest_request(Some("http://127.0.0.1:8123")),
                trusted_inspection(b"archive-two", &trace_hex),
            )
            .await
            .unwrap_err();
        assert!(url.to_string().contains("not a URL"), "{url}");
        let unknown = data
            .commit_inspected_trace(
                ingest_request(Some("ctr_never_registered")),
                trusted_inspection(b"archive-two", &trace_hex),
            )
            .await
            .unwrap_err();
        assert!(unknown.to_string().contains("not registered"), "{unknown}");
        assert_eq!(data.list_traces().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn lists_rust_owned_inventory_tables() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let db = storage.database().clone();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO containers(id,name,location,status,health_json,metadata_json,created_at,updated_at) VALUES('ctr_1','Local','local','ready','{\"ok\":true}','{}','2026-01-01','2026-01-02')", [])?;
            conn.execute("INSERT INTO traces(id,digest,title,source,metrics_json,metadata_json,created_at) VALUES('trace_1','digest_1','Trace','local','[]','{}','2026-01-03')", [])?;
            conn.execute(
                "INSERT INTO usage_records(
                    id,provider,model_id,request_id,measurement_kind,status,
                    started_at_ms,completed_at_ms,input_tokens,output_tokens,
                    billed_cost_usd,estimated_cost_usd,cost_source,source,created_at
                 ) VALUES(
                    'usage_1','openrouter','luna','req-usage-1','provider_reported','completed',
                    0,0,2,3,NULL,NULL,'none','test','2026-01-04'
                 )",
                [],
            )?;
            Ok(())
        }).unwrap();
        let data = DataStore::new(db, ContentStore::new(storage.content_root()));
        assert_eq!(data.list_containers().await.unwrap()[0].id, "ctr_1");
        assert_eq!(data.list_traces().await.unwrap()[0].id, "trace_1");
        assert_eq!(data.list_usage(100).await.unwrap()[0].total_tokens, 5);
        assert_eq!(
            data.counts().await.unwrap(),
            DataCounts {
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
        let data = DataStore::new(db.clone(), ContentStore::new(storage.content_root()));

        let (container, event) = data
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
        let data = DataStore::new(db.clone(), ContentStore::new(storage.content_root()));

        assert!(data
            .update_container_health(
                "ctr_1".into(),
                "ready".into(),
                serde_json::json!({"ok": true}),
            )
            .await
            .is_err());

        let unchanged = data.get_container("ctr_1".into()).await.unwrap();
        assert_eq!(unchanged.status, "starting");
        assert_eq!(unchanged.health, serde_json::json!({}));
    }
}
