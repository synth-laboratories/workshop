mod account;
mod account_cloud;
mod cloud;
mod codex;
mod credential_broker;
pub mod contract;
pub mod core_runtime;
mod device_auth;
mod domain;
mod eval_driver;
mod instance;
mod intern_api;
pub mod data;
mod laguna;
mod optimizers;
mod runtime;
mod skills;
pub mod storage;
mod synth_config;
mod tariffs;
mod terminal;
pub mod trace_ingest;
mod update_check;
mod visuals;
mod visuals_ipc;
mod whisper;
mod workspace_scope;

use base64::Engine as _;
use codex::{
    CodexApprovalDecisionRequest, CodexManager, CodexSessionInfo, CodexSessionRecord,
    CodexSessionRequest, CodexSessionStartRequest, CodexSteerRequest, CodexTurnFailure,
    CodexTurnSendRequest, CodexTurnStartRequest,
};
pub use core_runtime::CoreRuntime;
use instance::InstanceDiagnostics;
use intern_api::{
    InternControlResult, InternSendResult, InternSessionControlRequest, InternSessionCreateRequest,
    InternSessionSendRequest, InternSessionWire,
};
use data::{
    ContainerDeployment, ContainerRegisterRequest, DataCounts, ResolvedTraceProjection,
    TraceRecord, UsageEntry,
};
use laguna::{LagunaManager, LagunaModelHit, LagunaStatus};
use optimizers::{
    OptimizerCreateRequest, OptimizerEventEnvelope, OptimizerImportLocalRequest, OptimizerQuery,
    OptimizerRecipeRunRequest, OptimizerReconcileRequest, OptimizerRelationship,
    OptimizerRunRecord, OptimizerStateSlice,
};
use serde_json::Value;
use std::sync::Arc;
use storage::{AppEvent, CoreDiagnostics, ModelPerformanceRepository, ModelPerformanceSummary};
use synth_config::{
    BackendSettings, BackendSettingsUpdate, ModelMultiAgentSetting, ModelMultiAgentUpdate,
    WorkspaceAccessSettings, WorkspaceAccessUpdate,
};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use terminal::{TerminalCreateRequest, TerminalEvent, TerminalInfo, TerminalManager};
use trace_ingest::{TraceBundleIngestRequest, TraceBundleIngestResult};
use visuals::{
    TemplateMeta, VisualCreateRequest, VisualQuery, VisualRecord, VisualRevision,
    VisualUpdateRequest,
};
use workspace_scope::WorkspaceGrantRequest;
use workspace_scope::{ConversationWorkspaceScope, WorkspaceAccessMode};

#[tauri::command]
fn core_diagnostics(state: State<'_, Arc<CoreRuntime>>) -> Result<CoreDiagnostics, String> {
    state.diagnostics().map_err(|error| error.to_string())
}

#[tauri::command]
fn desktop_instance_diagnostics() -> InstanceDiagnostics {
    instance::diagnostics()
}

#[tauri::command]
fn desktop_image_preview(path: String) -> Result<String, String> {
    let path = std::path::Path::new(&path)
        .canonicalize()
        .map_err(|_| "Screenshot is unavailable".to_string())?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| "Screenshot has no supported format".to_string())?;
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return Err("Screenshot format is unsupported".into()),
    };
    let metadata = std::fs::metadata(&path).map_err(|_| "Screenshot is unavailable".to_string())?;
    if !metadata.is_file() || metadata.len() > 20 * 1024 * 1024 {
        return Err("Screenshot must be a file smaller than 20 MB".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "Screenshot could not be read".to_string())?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command]
async fn core_events_after(
    state: State<'_, Arc<CoreRuntime>>,
    after_sequence: i64,
    limit: Option<i64>,
) -> Result<Vec<AppEvent>, String> {
    state
        .journal()
        .events_after(after_sequence, limit.unwrap_or(500).clamp(1, 2000))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn core_session_events_after(
    state: State<'_, Arc<CoreRuntime>>,
    session_id: String,
    after_sequence: i64,
    limit: Option<i64>,
) -> Result<Vec<AppEvent>, String> {
    state
        .journal()
        .session_events_after(
            session_id,
            after_sequence,
            limit.unwrap_or(500).clamp(1, 2000),
        )
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn intern_sessions_list(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<InternSessionWire>, String> {
    intern_api::list(&state)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn intern_session_create(
    state: State<'_, Arc<CoreRuntime>>,
    request: InternSessionCreateRequest,
) -> Result<InternSessionWire, String> {
    intern_api::create(&state, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn intern_session_send(
    state: State<'_, Arc<CoreRuntime>>,
    request: InternSessionSendRequest,
) -> Result<InternSendResult, String> {
    intern_api::send(&state, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn intern_session_control(
    state: State<'_, Arc<CoreRuntime>>,
    request: InternSessionControlRequest,
) -> Result<InternControlResult, String> {
    intern_api::control(&state, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn intern_session_events_after(
    state: State<'_, Arc<CoreRuntime>>,
    session_id: String,
    after_sequence: i64,
    limit: Option<i64>,
) -> Result<Vec<AppEvent>, String> {
    state
        .journal()
        .session_events_after(
            session_id,
            after_sequence,
            limit.unwrap_or(500).clamp(1, 2_000),
        )
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn data_containers_list(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<ContainerDeployment>, String> {
    state
        .data()
        .list_containers()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn data_containers_get(
    state: State<'_, Arc<CoreRuntime>>,
    container_id: String,
) -> Result<ContainerDeployment, String> {
    state
        .data()
        .get_container(container_id)
        .await
        .map_err(|error| error.to_string())
}

async fn hydrate_container(
    base_url: &str,
    existing_metadata: serde_json::Value,
) -> (String, serde_json::Value, serde_json::Value, Option<String>) {
    let client = reqwest::Client::new();
    let root = base_url.trim_end_matches('/');
    // Health is intentionally cheap and may run frequently. Contract/catalog
    // hydration is cached because task catalogs can contain thousands of rows.
    let refresh_metadata = existing_metadata
        .get("hydratedAt")
        .and_then(|value| value.as_str())
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|hydrated| {
            chrono::Utc::now()
                .signed_duration_since(hydrated.with_timezone(&chrono::Utc))
                .num_seconds()
                >= 300
        })
        .unwrap_or(true);
    let health_result = client
        .get(format!("{root}/health"))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await;
    let (status, health) = match health_result {
        Ok(response) => {
            let code = response.status();
            let payload = response
                .json::<serde_json::Value>()
                .await
                .unwrap_or_else(|_| serde_json::json!({}));
            let ok =
                code.is_success() && payload.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
            (
                if ok { "ready" } else { "unhealthy" }.to_string(),
                serde_json::json!({"ok": ok, "status": code.as_u16(), "payload": payload}),
            )
        }
        Err(error) => (
            "unhealthy".into(),
            serde_json::json!({"ok": false, "error": error.to_string()}),
        ),
    };
    let mut info = existing_metadata.get("info").cloned();
    if refresh_metadata {
        info = None;
        for route in ["info", "metadata"] {
            if let Ok(response) = client
                .get(format!("{root}/{route}"))
                .timeout(std::time::Duration::from_secs(3))
                .send()
                .await
            {
                if response.status().is_success() {
                    info = response.json::<serde_json::Value>().await.ok();
                    if info.is_some() {
                        break;
                    }
                }
            }
        }
    }
    let task_family = info
        .as_ref()
        .and_then(|value| value.get("env_family").or_else(|| value.get("task_family")))
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| {
            health
                .get("payload")
                .and_then(|value| value.get("env_family"))
                .and_then(|value| value.as_str())
                .map(str::to_string)
        });
    let mut metadata = existing_metadata.as_object().cloned().unwrap_or_default();
    metadata.insert(
        "contractHint".into(),
        serde_json::json!(if info.is_some() {
            "info"
        } else {
            "health-only"
        }),
    );
    metadata.insert(
        "healthCheckedAt".into(),
        serde_json::json!(chrono::Utc::now().to_rfc3339()),
    );
    if refresh_metadata {
        metadata.insert(
            "hydratedAt".into(),
            serde_json::json!(chrono::Utc::now().to_rfc3339()),
        );
    }
    if let Some(info) = info {
        metadata.insert("info".into(), info);
    }
    if refresh_metadata {
        for (route, key) in [
            ("task_catalog", "taskCatalog"),
            ("task_info", "taskInfo"),
            ("program", "program"),
            ("dataset", "dataset"),
        ] {
            if let Ok(response) = client
                .get(format!("{root}/{route}"))
                .timeout(std::time::Duration::from_secs(3))
                .send()
                .await
            {
                if response.status().is_success() {
                    if let Ok(value) = response.json::<serde_json::Value>().await {
                        metadata.insert(key.into(), value);
                    }
                }
            }
        }
    }
    (
        status,
        health,
        serde_json::Value::Object(metadata),
        task_family,
    )
}

#[tauri::command]
async fn data_containers_register(
    state: State<'_, Arc<CoreRuntime>>,
    request: ContainerRegisterRequest,
) -> Result<ContainerDeployment, String> {
    if !(request.base_url.starts_with("http://") || request.base_url.starts_with("https://")) {
        return Err("container baseUrl must start with http:// or https://".into());
    }
    let (status, health, metadata, hydrated_family) = hydrate_container(
        &request.base_url,
        request
            .metadata
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
    )
    .await;
    let task_family = hydrated_family.or_else(|| request.task_family.clone());
    state
        .register_container(request, status, health, metadata, task_family)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn data_containers_probe(
    state: State<'_, Arc<CoreRuntime>>,
    container_id: String,
) -> Result<ContainerDeployment, String> {
    let container = state
        .data()
        .get_container(container_id.clone())
        .await
        .map_err(|error| error.to_string())?;
    let Some(base_url) = container.base_url.as_ref() else {
        return Ok(container);
    };
    let (status, health, metadata, task_family) =
        hydrate_container(base_url, container.metadata).await;
    state
        .update_container_hydration(container_id, status, health, metadata, task_family)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn data_traces_list(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<TraceRecord>, String> {
    state
        .data()
        .list_traces()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn data_traces_get(
    state: State<'_, Arc<CoreRuntime>>,
    trace_id: String,
) -> Result<TraceRecord, String> {
    state
        .data()
        .get_trace(trace_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn data_traces_ingest(
    state: State<'_, Arc<CoreRuntime>>,
    request: TraceBundleIngestRequest,
) -> Result<TraceBundleIngestResult, String> {
    let (result, event) = state
        .data()
        .ingest_trace_bundle(request)
        .await
        .map_err(|error| error.to_string())?;
    state.broadcast_committed(event);
    Ok(result)
}

#[tauri::command]
async fn data_trace_projection_resolve(
    state: State<'_, Arc<CoreRuntime>>,
    trace_digest: String,
    projection_kind: String,
) -> Result<ResolvedTraceProjection, String> {
    state
        .data()
        .resolve_trace_projection(trace_digest, projection_kind)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn data_usage_list(
    state: State<'_, Arc<CoreRuntime>>,
    limit: Option<i64>,
) -> Result<Vec<UsageEntry>, String> {
    state
        .data()
        .list_usage(limit.unwrap_or(100))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn model_performance_summary(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<ModelPerformanceSummary>, String> {
    ModelPerformanceRepository::new(state.storage().database().clone())
        .summaries()
        .await
        .map_err(|error| error.to_string())
}

/// Device-wide usage dashboard for one time window, aggregated in SQLite/Rust
/// over the authoritative per-request `usage_records` ledger — the renderer
/// never reduces raw rows itself.
#[tauri::command]
async fn usage_summary(
    state: State<'_, Arc<CoreRuntime>>,
    window: String,
) -> Result<storage::UsageSummary, String> {
    let now = chrono::Utc::now();
    let offset_seconds = chrono::Local::now().offset().local_minus_utc();
    let since_ms = storage::window_start_ms(&window, now, offset_seconds);
    storage::UsageRecordsRepository::new(state.storage().database().clone())
        .summary(window, since_ms)
        .await
        .map_err(|error| error.to_string())
}

/// The provider price cards currently in force. Settings renders these
/// numbers; the renderer never carries its own copy of a rate.
#[tauri::command]
fn tariff_catalog() -> Vec<tariffs::TariffCard> {
    tariffs::cards_in_force(chrono::Utc::now().timestamp_millis())
}

#[tauri::command]
async fn update_status() -> update_check::UpdateStatus {
    update_check::status().await
}

/// Always the fixed public download page — the manifest never chooses the
/// destination.
#[tauri::command]
async fn update_open_download(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(update_check::DOWNLOAD_PAGE, None::<String>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn data_counts(state: State<'_, Arc<CoreRuntime>>) -> Result<DataCounts, String> {
    state
        .data()
        .counts()
        .await
        .map_err(|error| error.to_string())
}

async fn publish_optimizer_event(
    app: &tauri::AppHandle,
    state: &CoreRuntime,
    event: Option<AppEvent>,
) -> Result<(), String> {
    if let Some(event) = event {
        state
            .publish_event(app, event)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn optimizers_algorithms_list(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<Value>, String> {
    Ok(state.optimizers().list_algorithms())
}

#[tauri::command]
async fn optimizers_recipes_list(state: State<'_, Arc<CoreRuntime>>) -> Result<Vec<Value>, String> {
    Ok(state.optimizers().list_recipes())
}

#[tauri::command]
async fn optimizers_recipe_start(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    request: OptimizerRecipeRunRequest,
) -> Result<OptimizerRunRecord, String> {
    let (run, event) = state
        .optimizers()
        .start_recipe(request)
        .await
        .map_err(|error| error.to_string())?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
async fn optimizers_list(
    state: State<'_, Arc<CoreRuntime>>,
    query: Option<OptimizerQuery>,
) -> Result<Vec<OptimizerRunRecord>, String> {
    state
        .optimizers()
        .list(query.unwrap_or_default())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn optimizers_get(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<OptimizerRunRecord, String> {
    state
        .optimizers()
        .get(optimizer_run_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn optimizers_create(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    request: OptimizerCreateRequest,
) -> Result<OptimizerRunRecord, String> {
    let (run, event) = state
        .optimizers()
        .create(request)
        .await
        .map_err(|error| error.to_string())?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
async fn optimizers_refresh(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<OptimizerRunRecord, String> {
    state
        .optimizers()
        .refresh(optimizer_run_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn optimizers_events_after(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
    after_seq: Option<u64>,
    limit: Option<i64>,
) -> Result<Vec<OptimizerEventEnvelope>, String> {
    state
        .optimizers()
        .events_after(optimizer_run_id, after_seq.unwrap_or(0), limit)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn optimizers_get_state(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
    slice_id: String,
    at_seq: Option<u64>,
) -> Result<OptimizerStateSlice, String> {
    state
        .optimizers()
        .get_state(optimizer_run_id, slice_id, at_seq)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn optimizers_get_state_batch(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
    slices: Option<Vec<String>>,
    at_seq: Option<u64>,
) -> Result<Vec<OptimizerStateSlice>, String> {
    state
        .optimizers()
        .get_state_batch(optimizer_run_id, slices, at_seq)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn optimizers_relationships(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<Vec<OptimizerRelationship>, String> {
    state
        .optimizers()
        .relationships(optimizer_run_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn optimizers_cancel(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<OptimizerRunRecord, String> {
    let (run, event) = state
        .optimizers()
        .cancel(optimizer_run_id)
        .await
        .map_err(|error| error.to_string())?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
async fn optimizers_pause(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<OptimizerRunRecord, String> {
    let (run, event) = state
        .optimizers()
        .pause(optimizer_run_id)
        .await
        .map_err(|error| error.to_string())?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
async fn optimizers_resume(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<OptimizerRunRecord, String> {
    let (run, event) = state
        .optimizers()
        .resume(optimizer_run_id)
        .await
        .map_err(|error| error.to_string())?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
async fn optimizers_open_visual(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<OptimizerRunRecord, String> {
    let (run, event) = state
        .optimizers()
        .open_visual(optimizer_run_id)
        .await
        .map_err(|error| error.to_string())?;
    publish_optimizer_event(&app, &state, event).await?;
    if let Some(visual_id) = run
        .visual_refs
        .iter()
        .find(|r| r.kind == "visual")
        .map(|r| r.id.clone())
    {
        let _ = app.emit(
            crate::core_runtime::VISUAL_SHOW_CHANNEL,
            serde_json::json!({
                "kind": "visual.show",
                "payload": { "visualId": visual_id }
            }),
        );
    }
    Ok(run)
}

#[tauri::command]
async fn optimizers_import_local(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    request: OptimizerImportLocalRequest,
) -> Result<OptimizerRunRecord, String> {
    let (run, event) = state
        .optimizers()
        .import_local(request)
        .await
        .map_err(|error| error.to_string())?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
async fn optimizers_reconcile_cloud(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    request: OptimizerReconcileRequest,
) -> Result<OptimizerRunRecord, String> {
    let (run, event) = state
        .optimizers()
        .reconcile_cloud(request)
        .await
        .map_err(|error| error.to_string())?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
async fn optimizers_list_cloud(
    state: State<'_, Arc<CoreRuntime>>,
    algorithm: Option<String>,
    status: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<Value>, String> {
    state
        .optimizers()
        .list_cloud(algorithm, status, limit)
        .await
        .map_err(|error| error.to_string())
}

async fn publish_visual_event(
    app: &tauri::AppHandle,
    core: &CoreRuntime,
    event: serde_json::Value,
) -> Result<(), String> {
    let parsed: AppEvent = serde_json::from_value(event).map_err(|error| error.to_string())?;
    core.publish_event(app, parsed)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn visuals_templates_list(
    state: State<'_, Arc<CoreRuntime>>,
    genre: Option<String>,
) -> Result<Vec<TemplateMeta>, String> {
    state
        .visuals()
        .list_templates(genre.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn visuals_templates_get(
    state: State<'_, Arc<CoreRuntime>>,
    template_id: String,
) -> Result<TemplateMeta, String> {
    state
        .visuals()
        .get_template(&template_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn visuals_list(
    state: State<'_, Arc<CoreRuntime>>,
    query: Option<VisualQuery>,
) -> Result<Vec<VisualRecord>, String> {
    state
        .visuals()
        .list(query.unwrap_or_default())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn visuals_get(
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
) -> Result<VisualRecord, String> {
    state
        .visuals()
        .get(visual_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn visuals_revisions(
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
) -> Result<Vec<VisualRevision>, String> {
    state
        .visuals()
        .revisions(visual_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn visuals_create(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    request: VisualCreateRequest,
) -> Result<VisualRecord, String> {
    let (visual, event) = state
        .visuals()
        .create(request)
        .await
        .map_err(|error| error.to_string())?;
    publish_visual_event(&app, &state, event).await?;
    Ok(visual)
}

#[tauri::command]
async fn visuals_update(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
    request: VisualUpdateRequest,
) -> Result<VisualRecord, String> {
    let (visual, event) = state
        .visuals()
        .update(visual_id, request)
        .await
        .map_err(|error| error.to_string())?;
    publish_visual_event(&app, &state, event).await?;
    Ok(visual)
}

#[tauri::command]
async fn visuals_save(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
    tsx: Option<String>,
) -> Result<VisualRecord, String> {
    let (visual, event) = state
        .visuals()
        .save(visual_id, tsx)
        .await
        .map_err(|error| error.to_string())?;
    publish_visual_event(&app, &state, event).await?;
    Ok(visual)
}

#[tauri::command]
async fn visuals_fork(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
    title: Option<String>,
    session_id: Option<String>,
) -> Result<VisualRecord, String> {
    let (visual, event) = state
        .visuals()
        .fork(visual_id, title, session_id)
        .await
        .map_err(|error| error.to_string())?;
    publish_visual_event(&app, &state, event).await?;
    Ok(visual)
}

#[tauri::command]
async fn visuals_archive(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
) -> Result<VisualRecord, String> {
    let (visual, event) = state
        .visuals()
        .archive(visual_id)
        .await
        .map_err(|error| error.to_string())?;
    publish_visual_event(&app, &state, event).await?;
    Ok(visual)
}

#[tauri::command]
async fn visuals_show(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
    session_id: Option<String>,
) -> Result<VisualRecord, String> {
    let (visual, event) = state
        .visuals()
        .show(visual_id, session_id)
        .await
        .map_err(|error| error.to_string())?;
    publish_visual_event(&app, &state, event).await?;
    Ok(visual)
}

#[tauri::command]
fn synth_config_get() -> Result<BackendSettings, String> {
    synth_config::get().map_err(|error| error.to_string())
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
struct ModelPerformanceMetric {
    model_id: String,
    provider: String,
    sample_count: u64,
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    output_tps_p50: f64,
    output_tps_p95: f64,
    total_tpm_p50: f64,
    total_tpm_p95: f64,
    latency_ms_p50: f64,
    latency_ms_p95: f64,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
struct ModelPerformanceSnapshot {
    window_minutes: u16,
    generated_at: String,
    models: Vec<ModelPerformanceMetric>,
}

#[tauri::command]
async fn model_performance_get(
    window_minutes: Option<u16>,
) -> Result<ModelPerformanceSnapshot, String> {
    let backend = synth_config::resolve().map_err(|error| error.to_string())?;
    let api_key = backend
        .api_key
        .ok_or_else(|| "Sign in to read Synth Cloud model telemetry".to_string())?;
    let window_minutes = window_minutes.unwrap_or(60).clamp(1, 1_440);
    let url = format!(
        "{}/api/v1/usage/model-performance?window_minutes={window_minutes}",
        backend.backend_url.trim_end_matches('/')
    );
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|error| error.to_string())?
        .get(url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|error| {
            let detail = error.to_string();
            if detail.contains("backend-api")
                || detail.contains("dns")
                || detail.contains("failed to lookup")
                || detail.contains("Connection refused")
                || detail.contains("timed out")
                || detail.contains("error sending request")
            {
                "Synth Cloud telemetry could not be reached. Check Account → Synth backend URL."
                    .to_string()
            } else {
                format!("Synth Cloud telemetry request failed: {error}")
            }
        })?;
    let status = response.status();
    if !status.is_success() {
        let detail = response.text().await.unwrap_or_default();
        return Err(format!(
            "Synth Cloud telemetry returned {status}: {}",
            detail.chars().take(240).collect::<String>()
        ));
    }
    response
        .json::<ModelPerformanceSnapshot>()
        .await
        .map_err(|error| format!("Invalid Synth Cloud telemetry response: {error}"))
}

#[tauri::command]
async fn synth_config_update(
    core: State<'_, Arc<CoreRuntime>>,
    request: BackendSettingsUpdate,
) -> Result<BackendSettings, String> {
    let settings = synth_config::update(request).map_err(|error| error.to_string())?;
    core.reload_intern_config()
        .await
        .map_err(|error| error.to_string())?;
    Ok(settings)
}

/// Begin (or resume) browser sign-in via Workshop device pairing and open the
/// system browser. Returns display-safe state only.
#[tauri::command]
async fn account_begin_sign_in(
    app: tauri::AppHandle,
    manager: State<'_, Arc<device_auth::DeviceAuthManager>>,
) -> Result<device_auth::SignInBegin, String> {
    let origin = device_auth::workshop_origin();
    let begin = manager
        .begin(&origin)
        .await
        .map_err(|error| error.to_string())?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(begin.verification_uri.clone(), None::<String>)
        .map_err(|error| error.to_string())?;
    Ok(begin)
}

/// One poll step of the pending sign-in. On success the key is stored through
/// synth_config and the Intern runtime reloads; the key never reaches the
/// renderer.
#[tauri::command]
async fn account_poll_sign_in(
    core: State<'_, Arc<CoreRuntime>>,
    manager: State<'_, Arc<device_auth::DeviceAuthManager>>,
    cloud: State<'_, Arc<account_cloud::AccountCloudClient>>,
) -> Result<device_auth::SignInPoll, String> {
    let origin = device_auth::workshop_origin();
    let result = manager
        .poll(&origin, |key| synth_config::store_api_key(key))
        .await
        .map_err(|error| error.to_string())?;
    if matches!(result, device_auth::SignInPoll::Active) {
        core.reload_intern_config()
            .await
            .map_err(|error| error.to_string())?;
        // A new key invalidates any cached snapshot, and the device is now
        // known to have paired at least once.
        cloud.clear_cache();
        let _ = account::mark_paired(core.storage(), chrono::Utc::now());
    }
    Ok(result)
}

#[tauri::command]
fn account_cancel_sign_in(
    manager: State<'_, Arc<device_auth::DeviceAuthManager>>,
) -> Result<(), String> {
    manager.cancel();
    Ok(())
}

/// Compose the account summary the shell renders: the Synth Cloud Account
/// Snapshot when the device is paired, the labelled local/dev stand-in when it
/// is not reachable outside prod. The renderer never derives plan or identity.
async fn account_summary_now(
    core: &Arc<CoreRuntime>,
    cloud: &Arc<account_cloud::AccountCloudClient>,
    force: bool,
) -> Result<account::AccountSummary, String> {
    let settings = synth_config::get().map_err(|error| error.to_string())?;
    let resolved = synth_config::resolve().map_err(|error| error.to_string())?;
    let origin = device_auth::workshop_origin();
    let now = chrono::Utc::now();
    let read = cloud
        .read(
            &resolved.backend_url,
            resolved.api_key.as_deref(),
            force,
            now,
        )
        .await;
    account::summary(
        core.storage(),
        &origin,
        &settings.profile,
        settings.api_key_configured,
        now,
        &read,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn account_get_summary(
    core: State<'_, Arc<CoreRuntime>>,
    cloud: State<'_, Arc<account_cloud::AccountCloudClient>>,
) -> Result<account::AccountSummary, String> {
    account_summary_now(&core, &cloud, false).await
}

/// Force a snapshot refetch — used after returning from hosted checkout and by
/// the explicit retry in the account menu.
#[tauri::command]
async fn account_refresh(
    core: State<'_, Arc<CoreRuntime>>,
    cloud: State<'_, Arc<account_cloud::AccountCloudClient>>,
) -> Result<account::AccountSummary, String> {
    account_summary_now(&core, &cloud, true).await
}

/// Open a backend-issued hosted billing URL in the system browser. Desktop
/// never renders a payment form and never receives card data.
#[tauri::command]
async fn account_open_billing(
    app: tauri::AppHandle,
    cloud: State<'_, Arc<account_cloud::AccountCloudClient>>,
    action: account_cloud::BillingAction,
    tier: Option<String>,
) -> Result<String, String> {
    let resolved = synth_config::resolve().map_err(|error| error.to_string())?;
    let url = cloud
        .billing_url(
            &resolved.backend_url,
            resolved.api_key.as_deref(),
            action,
            tier.as_deref(),
        )
        .await
        .map_err(|error| error.to_string())?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url.clone(), None::<String>)
        .map_err(|error| error.to_string())?;
    Ok(url)
}

#[tauri::command]
async fn account_sign_out(
    core: State<'_, Arc<CoreRuntime>>,
    cloud: State<'_, Arc<account_cloud::AccountCloudClient>>,
) -> Result<BackendSettings, String> {
    synth_config::remove_api_key().map_err(|error| error.to_string())?;
    // Cloud facts belong to the signed-out session; local history and the
    // device ledger stay untouched.
    cloud.clear_cache();
    core.reload_intern_config()
        .await
        .map_err(|error| error.to_string())?;
    synth_config::get().map_err(|error| error.to_string())
}

#[tauri::command]
fn model_multi_agent_list() -> Result<Vec<ModelMultiAgentSetting>, String> {
    synth_config::model_multi_agent_settings().map_err(|error| error.to_string())
}

#[tauri::command]
fn model_multi_agent_update(
    request: ModelMultiAgentUpdate,
) -> Result<Vec<ModelMultiAgentSetting>, String> {
    synth_config::update_model_multi_agent(request).map_err(|error| error.to_string())
}

#[tauri::command]
fn workspace_access_get() -> Result<WorkspaceAccessSettings, String> {
    synth_config::workspace_access_settings().map_err(|error| error.to_string())
}

#[tauri::command]
fn workspace_access_update(
    request: WorkspaceAccessUpdate,
) -> Result<WorkspaceAccessSettings, String> {
    synth_config::update_workspace_access(request).map_err(|error| error.to_string())
}

#[tauri::command]
async fn laguna_get_status(state: State<'_, Arc<LagunaManager>>) -> Result<LagunaStatus, String> {
    if state.status().await.phase == "unknown" {
        let root = runtime::workshop_root().map_err(|error| error.to_string())?;
        if let Err(error) = state.ensure(&root).await {
            state.set_error(error.to_string()).await;
        }
        return Ok(state.status().await);
    }
    Ok(state.refresh().await)
}

#[tauri::command]
async fn laguna_reload(state: State<'_, Arc<LagunaManager>>) -> Result<LagunaStatus, String> {
    let root = runtime::workshop_root().map_err(|error| error.to_string())?;
    state.reload(&root).await.map_err(|error| error.to_string())
}

#[tauri::command]
fn laguna_models_list(state: State<'_, Arc<LagunaManager>>) -> Result<Vec<LagunaModelHit>, String> {
    state.discover_models().map_err(|error| error.to_string())
}

#[tauri::command]
fn laguna_models_set_directory(
    state: State<'_, Arc<LagunaManager>>,
    path: String,
) -> Result<LagunaModelHit, String> {
    state
        .select_model(std::path::Path::new(&path))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn laguna_models_clear_directory(state: State<'_, Arc<LagunaManager>>) -> Result<(), String> {
    state
        .clear_selected_model()
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn workspace_choose_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Choose a project folder")
        .pick_folder(move |path| {
            let _ = sender.send(path.map(|value| value.to_string()));
        });
    receiver.await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn workspace_scope_get(
    core: State<'_, Arc<CoreRuntime>>,
    session_id: String,
) -> Result<Option<ConversationWorkspaceScope>, String> {
    workspace_scope::get(core.storage().database(), &session_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn workspace_scope_choose_and_attach(
    app: tauri::AppHandle,
    core: State<'_, Arc<CoreRuntime>>,
    codex: State<'_, Arc<CodexManager>>,
    session_id: String,
    proposed_access: WorkspaceAccessMode,
) -> Result<Option<ConversationWorkspaceScope>, String> {
    if proposed_access == WorkspaceAccessMode::ReadOnly {
        return Err("Read-only attachments are not yet supported by the macOS Codex sandbox; no access was granted".into());
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Choose a folder to attach")
        .pick_folder(move |path| {
            let _ = sender.send(path.map(|value| value.to_string()));
        });
    let Some(path) = receiver.await.map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    let scope = workspace_scope::attach(
        core.storage().database(),
        &session_id,
        &path,
        proposed_access,
        workspace_scope::AttachmentSource::UserPicker,
    )
    .await
    .map_err(|error| error.to_string())?;
    // Scope is durable before the old process is fenced. Closing preserves
    // the thread record; the next send resumes it with the new revision.
    codex
        .close(&session_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(Some(scope))
}

#[tauri::command]
async fn workspace_scope_recent_folders(
    core: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<String>, String> {
    workspace_scope::recent_folders(core.storage().database())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn workspace_scope_attach_recent(
    core: State<'_, Arc<CoreRuntime>>,
    codex: State<'_, Arc<CodexManager>>,
    session_id: String,
    path: String,
) -> Result<ConversationWorkspaceScope, String> {
    let scope = workspace_scope::attach_recent(core.storage().database(), &session_id, &path)
        .await
        .map_err(|error| error.to_string())?;
    codex
        .close(&session_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(scope)
}

#[tauri::command]
async fn workspace_scope_remove_attachment(
    core: State<'_, Arc<CoreRuntime>>,
    codex: State<'_, Arc<CodexManager>>,
    session_id: String,
    path: String,
) -> Result<ConversationWorkspaceScope, String> {
    let scope = workspace_scope::remove_attachment(core.storage().database(), &session_id, &path)
        .await
        .map_err(|error| error.to_string())?;
    codex
        .close(&session_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(scope)
}

#[tauri::command]
async fn workspace_scope_request_agent_grant(
    core: State<'_, Arc<CoreRuntime>>,
    session_id: String,
    path: String,
    access: WorkspaceAccessMode,
    reason: String,
) -> Result<WorkspaceGrantRequest, String> {
    workspace_scope::request_grant(
        core.storage().database(),
        &session_id,
        &path,
        access,
        &reason,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn workspace_scope_grants_list(
    core: State<'_, Arc<CoreRuntime>>,
    session_id: String,
) -> Result<Vec<WorkspaceGrantRequest>, String> {
    workspace_scope::list_grants(core.storage().database(), &session_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn workspace_scope_deny_request(
    core: State<'_, Arc<CoreRuntime>>,
    request_id: String,
) -> Result<WorkspaceGrantRequest, String> {
    workspace_scope::deny_grant(core.storage().database(), &request_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn workspace_scope_approve_request(
    app: tauri::AppHandle,
    core: State<'_, Arc<CoreRuntime>>,
    codex: State<'_, Arc<CodexManager>>,
    request_id: String,
) -> Result<Option<ConversationWorkspaceScope>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Confirm the exact requested folder")
        .pick_folder(move |path| {
            let _ = sender.send(path.map(|v| v.to_string()));
        });
    let Some(path) = receiver.await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let scope = workspace_scope::approve_grant(core.storage().database(), &request_id, &path)
        .await
        .map_err(|e| e.to_string())?;
    codex
        .close(&scope.session_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Some(scope))
}

/// Fills in the provider secrets and Laguna base URL that only the Rust side
/// knows. Shared by the plain attach command and the atomic send command.
async fn prepare_codex_start(
    laguna: &LagunaManager,
    core: &CoreRuntime,
    mut request: CodexSessionStartRequest,
) -> Result<CodexSessionStartRequest, String> {
    // Never trust renderer-supplied roots. Rust persistence is authoritative.
    request.writable_roots.clear();
    let scope = workspace_scope::get(core.storage().database(), &request.session_id)
        .await
        .map_err(|error| error.to_string())?;
    let scope = match scope {
        Some(scope) => {
            let requested = workspace_scope::canonical_directory(&request.workspace)
                .map_err(|error| error.to_string())?;
            if requested.to_string_lossy() != scope.workspace {
                return Err(
                    "requested workspace does not match the conversation's persisted scope".into(),
                );
            }
            scope
        }
        None => {
            request.workspace = workspace_scope::canonical_directory(&request.workspace)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .into_owned();
            return prepare_codex_provider(laguna, request).await;
        }
    };
    request.workspace = scope.workspace.clone();
    request.writable_roots = workspace_scope::writable_roots(&scope);
    prepare_codex_provider(laguna, request).await
}

async fn prepare_codex_provider(
    laguna: &LagunaManager,
    mut request: CodexSessionStartRequest,
) -> Result<CodexSessionStartRequest, String> {
    if request.multi_agent_version.is_none() {
        request.multi_agent_version = Some(
            synth_config::resolve_model_multi_agent(&request.model)
                .map_err(|error| error.to_string())?,
        );
    }
    // Exactly one preparation rule per provider class — see
    // `codex::ProviderClass` for the endpoint/credential/custody table. User
    // credentials are only *staged* here; `CodexManager::start` exchanges them
    // for a loopback lease at spawn time, so preparing a send for a live,
    // reused child never invalidates the token that child is presenting.
    match codex::provider_class(request.provider_name.as_deref()) {
        codex::ProviderClass::LocalLaguna => {
            let root = runtime::workshop_root().map_err(|error| error.to_string())?;
            request.base_url = laguna
                .ensure(&root)
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Laguna Responses server is unavailable".to_string())?;
            // The Laguna key is this process's loopback service token, not a
            // user credential: the child talks to the local daemon directly
            // and no broker lease is involved.
            request.api_key = laguna.api_key().unwrap_or_default();
        }
        codex::ProviderClass::OpenRouter => {
            let key = synth_config::openrouter_api_key()
                .map_err(|error| error.to_string())?
                .ok_or_else(|| {
                    "OpenRouter API key is not configured. Add it in Synth backend settings."
                        .to_string()
                })?;
            // The OpenRouter key is the user's, and leaks into shell snapshots
            // the same way the Synth key did; it goes into native custody too.
            codex::stage_brokered_credential(&mut request, &key)?;
        }
        codex::ProviderClass::SynthCloud => {
            let resolved = synth_config::resolve().map_err(|error| error.to_string())?;
            // Only Codex's Responses traffic uses the dedicated, source-owned
            // gateway for the active profile; account and billing calls elsewhere
            // keep reading `resolved.backend_url` directly. A
            // profile with no configured gateway fails closed here rather
            // than silently reusing the backend URL.
            let gateway_url = synth_config::require_responses_gateway_url(&resolved)?;
            codex::apply_synth_cloud_provider(
                &mut request,
                &gateway_url,
                resolved.api_key.as_deref(),
            )?;
        }
        codex::ProviderClass::Direct => {
            // openai / Azure / custom Responses endpoints: the renderer
            // supplies endpoint and env key, Rust holds no credential for
            // them, and the request passes through untouched. Deliberately no
            // brokering — custody is only for credentials Rust itself
            // resolves, never for renderer-controlled values.
        }
    }
    Ok(request)
}

/// One renderer round trip that ensures the app-server attachment and starts
/// the turn. Splitting these into two commands let the child exit in between,
/// which stranded the UI in `Working` with a live `Stop`.
#[tauri::command]
async fn codex_turn_send(
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    laguna: State<'_, Arc<LagunaManager>>,
    core: State<'_, Arc<CoreRuntime>>,
    mut request: CodexTurnSendRequest,
) -> Result<CodexSessionInfo, CodexTurnFailure> {
    let session_id = request.start.session_id.clone();
    request.start = prepare_codex_start(&laguna, &core, request.start)
        .await
        .map_err(|message| CodexTurnFailure {
            code: "codex_provider_unavailable".into(),
            message: message.clone(),
            session_id,
            detail: message,
        })?;
    state.send_turn(app, request).await
}

#[tauri::command]
async fn codex_session_start(
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    laguna: State<'_, Arc<LagunaManager>>,
    core: State<'_, Arc<CoreRuntime>>,
    request: CodexSessionStartRequest,
) -> Result<CodexSessionInfo, String> {
    let request = prepare_codex_start(&laguna, &core, request).await?;
    state
        .start(app, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn codex_turn_start(
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    request: CodexTurnStartRequest,
) -> Result<CodexSessionInfo, String> {
    state
        .start_turn(app, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn codex_turn_interrupt(
    state: State<'_, Arc<CodexManager>>,
    request: CodexSessionRequest,
) -> Result<(), String> {
    state
        .interrupt(&request.session_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn codex_thread_compact(
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    laguna: State<'_, Arc<LagunaManager>>,
    core: State<'_, Arc<CoreRuntime>>,
    request: CodexSessionStartRequest,
) -> Result<(), String> {
    let request = prepare_codex_start(&laguna, &core, request).await?;
    state
        .compact(app, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn codex_turn_steer(
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    request: CodexSteerRequest,
) -> Result<(), String> {
    state
        .steer_turn(app, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn codex_approval_resolve(
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    request: CodexApprovalDecisionRequest,
) -> Result<(), String> {
    state
        .resolve_approval(app, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn codex_session_close(
    state: State<'_, Arc<CodexManager>>,
    request: CodexSessionRequest,
) -> Result<(), String> {
    state
        .close(&request.session_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn codex_sessions_list(
    state: State<'_, Arc<CodexManager>>,
) -> Result<Vec<CodexSessionRecord>, String> {
    Ok(state.list().await)
}

#[tauri::command]
fn codex_default_workspace() -> Result<String, String> {
    let configured = synth_config::allowed_workspace_roots()
        .map_err(|error| format!("Cannot read workspace access settings: {error}"))?;
    let path = configured
        .first()
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("SYNTH_DESKTOP_WORKSPACE").map(std::path::PathBuf::from))
        .unwrap_or_else(|| crate::instance::state_root().join("workspaces/default"));
    std::fs::create_dir_all(&path)
        .map_err(|error| format!("Cannot create the default workspace: {error}"))?;
    let path = path
        .canonicalize()
        .map_err(|error| format!("Default workspace is unavailable: {error}"))?;
    if !path.is_dir() {
        return Err("Default workspace must be a directory".into());
    }
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn terminal_create(
    app: tauri::AppHandle,
    state: State<'_, Arc<TerminalManager>>,
    request: TerminalCreateRequest,
) -> Result<TerminalInfo, String> {
    state
        .create(app, request)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn terminal_list(
    state: State<'_, Arc<TerminalManager>>,
    workspace_id: Option<String>,
) -> Vec<TerminalInfo> {
    state.list(workspace_id.as_deref())
}

#[tauri::command]
fn terminal_snapshot(
    state: State<'_, Arc<TerminalManager>>,
    terminal_id: String,
    after_sequence: Option<u64>,
) -> Result<Vec<TerminalEvent>, String> {
    state
        .snapshot(&terminal_id, after_sequence.unwrap_or(0))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn terminal_write(
    state: State<'_, Arc<TerminalManager>>,
    terminal_id: String,
    data: String,
) -> Result<(), String> {
    state
        .write(&terminal_id, &data)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn terminal_resize(
    state: State<'_, Arc<TerminalManager>>,
    terminal_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state
        .resize(&terminal_id, cols, rows)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn terminal_close(
    state: State<'_, Arc<TerminalManager>>,
    terminal_id: String,
) -> Result<(), String> {
    state.close(&terminal_id).map_err(|error| error.to_string())
}

pub fn run() {
    tauri::Builder::default()
        // This must be the first plugin registered. All app state, IPC, and
        // SQLite ownership belongs to the original process.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            instance::mark_manifest_running();
            // Builds before the credential broker exported provider keys into
            // Codex, which recorded them in its shell snapshots. Scrub what
            // those builds left behind in Desktop's own Codex homes.
            match credential_broker::redact_managed_shell_snapshots(&codex::codex_root()) {
                Ok(0) => {}
                Ok(count) => {
                    eprintln!("redacted provider secrets from {count} Codex shell snapshot(s)")
                }
                Err(error) => {
                    return Err(std::io::Error::other(format!(
                        "could not scrub provider secrets from Codex shell snapshots, so a \
                         credential may remain in cleartext under {}: {error:#}",
                        codex::codex_root().display()
                    ))
                    .into())
                }
            }
            let core =
                Arc::new(CoreRuntime::open_default().map_err(|error| {
                    std::io::Error::other(format!("open CoreRuntime: {error}"))
                })?);
            let migration = crate::storage::legacy_migration::MigrationService::new(
                core.storage().database().clone(),
                core.storage().content_root().to_path_buf(),
                crate::storage::app_data_root().join("migration-backups"),
            );
            let laguna = Arc::new(LagunaManager::new());
            let codex = Arc::new(CodexManager::new(Some(core.clone())));
            app.manage(core.clone());
            app.manage(migration);
            app.manage(codex.clone());
            app.manage(Arc::new(TerminalManager::new()));
            app.manage(Arc::new(device_auth::DeviceAuthManager::new()));
            app.manage(Arc::new(account_cloud::AccountCloudClient::new()));
            app.manage(laguna.clone());

            // All committed CoreRuntime events reach Tauri through this single
            // forwarder. Producers only journal and broadcast.
            core.spawn_forwarder(app.handle().clone());

            let mut status_updates = laguna.subscribe();
            let status_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(status) = status_updates.recv().await {
                    let _ = status_handle.emit(crate::contract::events::EventChannel::LAGUNA_STATUS, status);
                }
            });

            let bootstrap_handle = app.handle().clone();
            let bootstrap_core = core.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = bootstrap_core.bootstrap(&bootstrap_handle).await {
                    eprintln!("CoreRuntime bootstrap failed: {error}");
                }
                if let Err(error) = bootstrap_core.resume_intern_providers().await {
                    eprintln!("Intern restart reconciliation failed: {error}");
                }
            });

            let ipc_core = core.clone();
            let ipc_root = crate::storage::app_data_root();
            tauri::async_runtime::spawn(async move {
                match visuals_ipc::spawn(ipc_core, ipc_root).await {
                    Ok(connection) => {
                        eprintln!(
                            "Visuals IPC listening at {} (token written to {})",
                            connection.url, connection.path
                        );
                    }
                    Err(error) => eprintln!("Visuals IPC failed to start: {error}"),
                }
            });

            if eval_driver::should_spawn() {
                let eval_core = core.clone();
                let eval_codex = codex.clone();
                let eval_laguna = laguna.clone();
                let eval_app = app.handle().clone();
                let eval_root = crate::storage::app_data_root();
                tauri::async_runtime::spawn(async move {
                    match eval_driver::spawn(
                        eval_driver::EvalDriverDeps {
                            core: eval_core,
                            codex: eval_codex,
                            laguna: eval_laguna,
                            app: eval_app,
                        },
                        eval_root,
                    )
                    .await
                    {
                        Ok(connection) => {
                            eprintln!(
                                "Eval driver ({}) listening at {} (descriptor {})",
                                eval_driver::PROTOCOL_VERSION,
                                connection.url,
                                connection.path
                            );
                        }
                        Err(error) => eprintln!("Eval driver failed to start: {error}"),
                    }
                });
            }

            if let Some(window) = app.get_webview_window("main") {
                window.show()?;
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            desktop_instance_diagnostics,
            desktop_image_preview,
            core_diagnostics,
            core_events_after,
            core_session_events_after,
            intern_sessions_list,
            intern_session_create,
            intern_session_send,
            intern_session_control,
            intern_session_events_after,
            data_containers_list,
            data_containers_get,
            data_containers_register,
            data_containers_probe,
            data_traces_list,
            data_traces_get,
            data_traces_ingest,
            data_trace_projection_resolve,
            data_usage_list,
            model_performance_summary,
            usage_summary,
            tariff_catalog,
            update_status,
            update_open_download,
            data_counts,
            optimizers_algorithms_list,
            optimizers_recipes_list,
            optimizers_recipe_start,
            optimizers_list,
            optimizers_get,
            optimizers_create,
            optimizers_refresh,
            optimizers_events_after,
            optimizers_get_state,
            optimizers_get_state_batch,
            optimizers_relationships,
            optimizers_cancel,
            optimizers_pause,
            optimizers_resume,
            optimizers_open_visual,
            optimizers_import_local,
            optimizers_reconcile_cloud,
            optimizers_list_cloud,
            visuals_templates_list,
            visuals_templates_get,
            visuals_list,
            visuals_get,
            visuals_revisions,
            visuals_create,
            visuals_update,
            visuals_save,
            visuals_fork,
            visuals_archive,
            visuals_show,
            synth_config_get,
            synth_config_update,
            model_performance_get,
            account_begin_sign_in,
            account_get_summary,
            account_refresh,
            account_open_billing,
            account_poll_sign_in,
            account_cancel_sign_in,
            account_sign_out,
            model_multi_agent_list,
            model_multi_agent_update,
            workspace_access_get,
            workspace_access_update,
            workspace_scope_get,
            workspace_scope_choose_and_attach,
            workspace_scope_recent_folders,
            workspace_scope_attach_recent,
            workspace_scope_remove_attachment,
            workspace_scope_request_agent_grant,
            workspace_scope_grants_list,
            workspace_scope_approve_request,
            workspace_scope_deny_request,
            crate::storage::legacy_migration::commands::migration_scan,
            crate::storage::legacy_migration::commands::migration_prepare,
            crate::storage::legacy_migration::commands::migration_apply,
            crate::storage::legacy_migration::commands::migration_cancel,
            laguna_get_status,
            laguna_reload,
            laguna_models_list,
            laguna_models_set_directory,
            laguna_models_clear_directory,
            laguna::laguna_inference_snapshot,
            laguna::laguna_inference_stream_start,
            laguna::laguna_inference_stream_stop,
            laguna::laguna_model_unload,
            laguna::laguna_model_download,
            laguna::laguna_model_delete,
            laguna::laguna_settings_snapshot,
            laguna::laguna_settings_update,
            whisper::whisper_models_list,
            whisper::whisper_model_download,
            whisper::whisper_models_set_selected,
            whisper::whisper_models_clear,
            whisper::whisper_runtime_status,
            whisper::whisper_runtime_warm,
            whisper::whisper_transcribe,
            whisper::whisper_transcribe_base64,
            skills::skills_list,
            workspace_choose_directory,
            codex_session_start,
            codex_turn_start,
            codex_turn_send,
            codex_turn_interrupt,
            codex_thread_compact,
            codex_turn_steer,
            codex_approval_resolve,
            codex_session_close,
            codex_sessions_list,
            codex_default_workspace,
            terminal_create,
            terminal_list,
            terminal_snapshot,
            terminal_write,
            terminal_resize,
            terminal_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running Synth Desktop");
}
