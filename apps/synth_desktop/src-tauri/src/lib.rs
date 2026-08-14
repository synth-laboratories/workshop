mod account;
mod account_cloud;
mod cloud;
mod codex;
mod codex_oauth;
mod container_stream;
pub mod contract;
pub mod core_runtime;
mod credential_broker;
pub mod data;
mod device_auth;
mod domain;
pub mod error;
mod eval_driver;
mod http;
mod instance;
mod intern_api;
pub mod ipc;
mod laguna;
mod limits;
mod optimizers;
mod plugins;
mod runtime;
mod services;
mod session;
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
use data::{
    ContainerDeployment, ContainerRegisterRequest, DataCounts, ResolvedTraceProjection,
    TraceRecord, UsageEntry,
};
use error::AppError;
use plugins::{PluginStatus};
use intern_api::{
    InternControlResult, InternSendResult, InternSessionControlRequest, InternSessionCreateRequest,
    InternSessionSendRequest, InternSessionWire,
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
    BackendSettings, BackendSettingsUpdate, DesktopPermissionSettings, DesktopPermissionUpdate,
    ModelMultiAgentSetting, ModelMultiAgentUpdate, WorkspaceAccessSettings, WorkspaceAccessUpdate,
};
use tauri::{Emitter, Manager, RunEvent, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use terminal::{TerminalCreateRequest, TerminalEvent, TerminalInfo, TerminalManager};
use trace_ingest::{TraceBundleIngestRequest, TraceBundleIngestResult};
use visuals::{
    TemplateMeta, VisualAsset, VisualCreateRequest, VisualQuery, VisualRecord, VisualRendition,
    VisualRevision, VisualUpdateRequest,
};
use workspace_scope::WorkspaceGrantRequest;
use workspace_scope::{ConversationWorkspaceScope, WorkspaceAccessMode};

#[tauri::command]
#[specta::specta]
fn core_diagnostics(state: State<'_, Arc<CoreRuntime>>) -> Result<CoreDiagnostics, AppError> {
    state.diagnostics().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn desktop_image_preview(path: String) -> Result<String, AppError> {
    let path = std::path::Path::new(&path)
        .canonicalize()
        .map_err(|_| AppError::io("Screenshot is unavailable"))?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| AppError::message("Screenshot has no supported format"))?;
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return Err("Screenshot format is unsupported".into()),
    };
    let metadata =
        std::fs::metadata(&path).map_err(|_| AppError::io("Screenshot is unavailable"))?;
    if !metadata.is_file() || metadata.len() > limits::IMAGE_PREVIEW_MAX_BYTES {
        return Err("Screenshot must be a file smaller than 20 MB".into());
    }
    let bytes = std::fs::read(path).map_err(|_| AppError::io("Screenshot could not be read"))?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command]
#[specta::specta]
async fn core_events_after(
    state: State<'_, Arc<CoreRuntime>>,
    after_sequence: contract::specta::OpaqueInteger<i64>,
    limit: Option<contract::specta::OpaqueInteger<i64>>,
) -> Result<Vec<AppEvent>, AppError> {
    state
        .journal()
        .events_after(
            after_sequence.0,
            limit.map(|value| value.0).unwrap_or(500).clamp(1, 2000),
        )
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn core_session_events_after(
    state: State<'_, Arc<CoreRuntime>>,
    session_id: String,
    after_sequence: contract::specta::OpaqueInteger<i64>,
    limit: Option<contract::specta::OpaqueInteger<i64>>,
) -> Result<Vec<AppEvent>, AppError> {
    state
        .journal()
        .session_events_after(
            session_id,
            after_sequence.0,
            limit.map(|value| value.0).unwrap_or(500).clamp(1, 2000),
        )
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn intern_sessions_list(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<InternSessionWire>, AppError> {
    intern_api::list(&state).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn intern_session_create(
    state: State<'_, Arc<CoreRuntime>>,
    request: InternSessionCreateRequest,
) -> Result<InternSessionWire, AppError> {
    intern_api::create(&state, request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn intern_session_send(
    state: State<'_, Arc<CoreRuntime>>,
    request: InternSessionSendRequest,
) -> Result<InternSendResult, AppError> {
    intern_api::send(&state, request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn intern_session_control(
    state: State<'_, Arc<CoreRuntime>>,
    request: InternSessionControlRequest,
) -> Result<InternControlResult, AppError> {
    intern_api::control(&state, request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn intern_session_events_after(
    state: State<'_, Arc<CoreRuntime>>,
    session_id: String,
    after_sequence: contract::specta::OpaqueInteger<i64>,
    limit: Option<contract::specta::OpaqueInteger<i64>>,
) -> Result<Vec<AppEvent>, AppError> {
    state
        .journal()
        .session_events_after(
            session_id,
            after_sequence.0,
            limit.map(|value| value.0).unwrap_or(500).clamp(1, 2_000),
        )
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn data_containers_list(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<ContainerDeployment>, AppError> {
    state.data().list_containers().await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn data_containers_get(
    state: State<'_, Arc<CoreRuntime>>,
    container_id: String,
) -> Result<ContainerDeployment, AppError> {
    state
        .data()
        .get_container(container_id)
        .await
        .map_err(AppError::from)
}

async fn hydrate_container(
    base_url: &str,
    existing_metadata: serde_json::Value,
) -> (String, serde_json::Value, serde_json::Value, Option<String>) {
    let client = http::http_client_with_timeout(limits::CONTAINER_PROBE_TIMEOUT);
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
                >= limits::CONTAINER_METADATA_REFRESH.as_secs() as i64
        })
        .unwrap_or(true);
    let health_result = client.get(format!("{root}/health")).send().await;
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
            if let Ok(response) = client.get(format!("{root}/{route}")).send().await {
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
        .and_then(|value| {
            crate::visuals::classify_live_eval_family(value, None)
                .map(|family| family.as_str().to_string())
        })
        .or_else(|| {
            info.as_ref()
                .and_then(|value| value.get("env_family").or_else(|| value.get("task_family")))
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
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
    if let Some(info_value) = &info {
        if let Some(family) =
            crate::visuals::classify_live_eval_family(info_value, task_family.as_deref())
        {
            let existing_refs = metadata
                .get("liveEval")
                .and_then(|value| value.get("policyRefs"))
                .cloned();
            match crate::visuals::live_eval_bind_metadata(
                family,
                info_value,
                existing_refs.as_ref(),
            ) {
                Ok(bind) => {
                    metadata.insert("liveEval".into(), bind);
                }
                Err(error) => {
                    metadata.insert("liveEvalError".into(), serde_json::json!(error.to_string()));
                }
            }
        }
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
            if let Ok(response) = client.get(format!("{root}/{route}")).send().await {
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
#[specta::specta]
async fn data_containers_register(
    state: State<'_, Arc<CoreRuntime>>,
    request: ContainerRegisterRequest,
) -> Result<ContainerDeployment, AppError> {
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
    if let Some(error) = metadata
        .get("liveEvalError")
        .and_then(|value| value.as_str())
        .filter(|error| error.contains("live_frames"))
    {
        return Err(error.into());
    }
    state
        .register_container(request, status, health, metadata, task_family)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn data_containers_probe(
    state: State<'_, Arc<CoreRuntime>>,
    container_id: String,
) -> Result<ContainerDeployment, AppError> {
    let container = state
        .data()
        .get_container(container_id.clone())
        .await
        .map_err(AppError::from)?;
    let Some(base_url) = container.base_url.as_ref() else {
        return Ok(container);
    };
    let (status, health, metadata, task_family) =
        hydrate_container(base_url, container.metadata).await;
    state
        .update_container_hydration(container_id, status, health, metadata, task_family)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn data_traces_list(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<TraceRecord>, AppError> {
    state.data().list_traces().await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn data_traces_get(
    state: State<'_, Arc<CoreRuntime>>,
    trace_id: String,
) -> Result<TraceRecord, AppError> {
    state
        .data()
        .get_trace(trace_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn data_traces_ingest(
    state: State<'_, Arc<CoreRuntime>>,
    request: TraceBundleIngestRequest,
) -> Result<TraceBundleIngestResult, AppError> {
    let (result, event) = state
        .data()
        .ingest_trace_bundle(request)
        .await
        .map_err(AppError::from)?;
    state.broadcast_committed(event);
    Ok(result)
}

#[tauri::command]
#[specta::specta]
async fn data_trace_projection_resolve(
    state: State<'_, Arc<CoreRuntime>>,
    trace_digest: String,
    projection_kind: String,
) -> Result<ResolvedTraceProjection, AppError> {
    state
        .data()
        .resolve_trace_projection(trace_digest, projection_kind)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn data_usage_list(
    state: State<'_, Arc<CoreRuntime>>,
    limit: Option<contract::specta::OpaqueInteger<i64>>,
) -> Result<Vec<UsageEntry>, AppError> {
    state
        .data()
        .list_usage(limit.map(|value| value.0).unwrap_or(100))
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn model_performance_summary(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<ModelPerformanceSummary>, AppError> {
    ModelPerformanceRepository::new(state.storage().database().clone())
        .summaries()
        .await
        .map_err(AppError::from)
}

/// Device-wide usage dashboard for one time window, aggregated in SQLite/Rust
/// over the authoritative per-request `usage_records` ledger — the renderer
/// never reduces raw rows itself.
#[tauri::command]
#[specta::specta]
async fn usage_summary(
    state: State<'_, Arc<CoreRuntime>>,
    window: String,
) -> Result<storage::UsageSummary, AppError> {
    let now = chrono::Utc::now();
    let offset_seconds = chrono::Local::now().offset().local_minus_utc();
    let since_ms = storage::window_start_ms(&window, now, offset_seconds);
    storage::UsageRecordsRepository::new(state.storage().database().clone())
        .summary(window, since_ms)
        .await
        .map_err(AppError::from)
}

/// The provider price cards currently in force. Settings renders these
/// numbers; the renderer never carries its own copy of a rate.
#[tauri::command]
#[specta::specta]
fn tariff_catalog() -> Vec<tariffs::TariffCard> {
    tariffs::cards_in_force(chrono::Utc::now().timestamp_millis())
}

#[tauri::command]
#[specta::specta]
async fn update_status() -> update_check::UpdateStatus {
    update_check::status().await
}

/// Always the fixed public download page — the manifest never chooses the
/// destination.
#[tauri::command]
#[specta::specta]
async fn update_open_download(app: tauri::AppHandle) -> Result<(), AppError> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(update_check::DOWNLOAD_PAGE, None::<String>)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn data_counts(state: State<'_, Arc<CoreRuntime>>) -> Result<DataCounts, AppError> {
    state.data().counts().await.map_err(AppError::from)
}

async fn publish_optimizer_event(
    app: &tauri::AppHandle,
    state: &CoreRuntime,
    event: Option<AppEvent>,
) -> Result<(), AppError> {
    if let Some(event) = event {
        state
            .publish_event(app, event)
            .await
            .map_err(AppError::from)?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn optimizers_algorithms_list(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<contract::specta::OpaqueJson>, AppError> {
    Ok(state
        .optimizers()
        .list_algorithms()
        .into_iter()
        .map(contract::specta::OpaqueJson)
        .collect())
}

#[tauri::command]
#[specta::specta]
async fn optimizers_recipes_list(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<contract::specta::OpaqueJson>, AppError> {
    Ok(state
        .optimizers()
        .list_recipes()
        .into_iter()
        .map(contract::specta::OpaqueJson)
        .collect())
}

#[tauri::command]
#[specta::specta]
async fn optimizers_recipe_start(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    request: OptimizerRecipeRunRequest,
) -> Result<OptimizerRunRecord, AppError> {
    let (run, event) = state
        .optimizers()
        .start_recipe(request)
        .await
        .map_err(AppError::from)?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_list(
    state: State<'_, Arc<CoreRuntime>>,
    query: Option<OptimizerQuery>,
) -> Result<Vec<OptimizerRunRecord>, AppError> {
    state
        .optimizers()
        .list(query.unwrap_or_default())
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_get(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<OptimizerRunRecord, AppError> {
    state
        .optimizers()
        .get(optimizer_run_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_create(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    request: OptimizerCreateRequest,
) -> Result<OptimizerRunRecord, AppError> {
    let (run, event) = state
        .optimizers()
        .create(request)
        .await
        .map_err(AppError::from)?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_refresh(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<OptimizerRunRecord, AppError> {
    state
        .optimizers()
        .refresh(optimizer_run_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_events_after(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
    after_seq: Option<contract::specta::OpaqueInteger<u64>>,
    limit: Option<contract::specta::OpaqueInteger<i64>>,
) -> Result<Vec<OptimizerEventEnvelope>, AppError> {
    state
        .optimizers()
        .events_after(
            optimizer_run_id,
            after_seq.map(|value| value.0).unwrap_or(0),
            limit.map(|value| value.0),
        )
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_get_state(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
    slice_id: String,
    at_seq: Option<contract::specta::OpaqueInteger<u64>>,
) -> Result<OptimizerStateSlice, AppError> {
    state
        .optimizers()
        .get_state(optimizer_run_id, slice_id, at_seq.map(|value| value.0))
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_get_state_batch(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
    slices: Option<Vec<String>>,
    at_seq: Option<contract::specta::OpaqueInteger<u64>>,
) -> Result<Vec<OptimizerStateSlice>, AppError> {
    state
        .optimizers()
        .get_state_batch(optimizer_run_id, slices, at_seq.map(|value| value.0))
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_relationships(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<Vec<OptimizerRelationship>, AppError> {
    state
        .optimizers()
        .relationships(optimizer_run_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_cancel(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<OptimizerRunRecord, AppError> {
    let (run, event) = state
        .optimizers()
        .cancel(optimizer_run_id)
        .await
        .map_err(AppError::from)?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_pause(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<OptimizerRunRecord, AppError> {
    let (run, event) = state
        .optimizers()
        .pause(optimizer_run_id)
        .await
        .map_err(AppError::from)?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_resume(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<OptimizerRunRecord, AppError> {
    let (run, event) = state
        .optimizers()
        .resume(optimizer_run_id)
        .await
        .map_err(AppError::from)?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_open_visual(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<OptimizerRunRecord, AppError> {
    let (run, event) = state
        .optimizers()
        .open_visual(optimizer_run_id)
        .await
        .map_err(AppError::from)?;
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
#[specta::specta]
async fn optimizers_import_local(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    request: OptimizerImportLocalRequest,
) -> Result<OptimizerRunRecord, AppError> {
    let (run, event) = state
        .optimizers()
        .import_local(request)
        .await
        .map_err(AppError::from)?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_reconcile_cloud(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    request: OptimizerReconcileRequest,
) -> Result<OptimizerRunRecord, AppError> {
    let (run, event) = state
        .optimizers()
        .reconcile_cloud(request)
        .await
        .map_err(AppError::from)?;
    publish_optimizer_event(&app, &state, event).await?;
    Ok(run)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_list_cloud(
    state: State<'_, Arc<CoreRuntime>>,
    algorithm: Option<String>,
    status: Option<String>,
    limit: Option<contract::specta::OpaqueInteger<i64>>,
) -> Result<Vec<contract::specta::OpaqueJson>, AppError> {
    state
        .optimizers()
        .list_cloud(algorithm, status, limit.map(|value| value.0))
        .await
        .map(|values| {
            values
                .into_iter()
                .map(contract::specta::OpaqueJson)
                .collect()
        })
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn plugins_status(
    state: State<'_, Arc<CoreRuntime>>,
    plugin_id: Option<String>,
) -> Result<PluginStatus, AppError> {
    let _ = plugin_id;
    Ok(state.plugins().status(&state).await)
}

#[tauri::command]
#[specta::specta]
async fn plugins_list(state: State<'_, Arc<CoreRuntime>>) -> Result<Vec<PluginStatus>, AppError> {
    Ok(vec![state.plugins().status(&state).await])
}

#[derive(Clone, Debug, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct VisualReadyRequest {
    visual_id: String,
    optimizer_run_id: String,
    template_id: String,
    replayed_through: u32,
    subscribed_from: u32,
    template_digest: Option<String>,
}

#[tauri::command]
#[specta::specta]
async fn visual_subscription_ready(
    state: State<'_, Arc<CoreRuntime>>,
    request: VisualReadyRequest,
) -> Result<contract::specta::OpaqueJson, AppError> {
    let receipt = serde_json::json!({
        "schemaVersion": "synth.visual-subscription-receipt.v1",
        "visualId": request.visual_id,
        "optimizerRunId": request.optimizer_run_id,
        "templateId": request.template_id,
        "replayedThrough": request.replayed_through,
        "subscribedFrom": request.subscribed_from,
        "templateDigest": request.template_digest,
    });
    let stored = state
        .optimizers()
        .record_visual_ready(request.optimizer_run_id, receipt)
        .await
        .map_err(AppError::from)?;
    Ok(contract::specta::OpaqueJson(stored))
}

async fn publish_visual_event(
    app: &tauri::AppHandle,
    core: &CoreRuntime,
    event: serde_json::Value,
) -> Result<(), AppError> {
    let parsed: AppEvent = serde_json::from_value(event).map_err(AppError::from)?;
    core.publish_event(app, parsed)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn visuals_templates_list(
    state: State<'_, Arc<CoreRuntime>>,
    genre: Option<String>,
) -> Result<Vec<TemplateMeta>, AppError> {
    state
        .visuals()
        .list_templates(genre.as_deref())
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn visuals_templates_get(
    state: State<'_, Arc<CoreRuntime>>,
    template_id: String,
) -> Result<TemplateMeta, AppError> {
    state
        .visuals()
        .get_template(&template_id)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn visuals_list(
    state: State<'_, Arc<CoreRuntime>>,
    query: Option<VisualQuery>,
) -> Result<Vec<VisualRecord>, AppError> {
    state
        .visuals()
        .list(query.unwrap_or_default())
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn visuals_get(
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
) -> Result<VisualRecord, AppError> {
    state.visuals().get(visual_id).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn visuals_revisions(
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
) -> Result<Vec<VisualRevision>, AppError> {
    state
        .visuals()
        .revisions(visual_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn visuals_create(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    request: VisualCreateRequest,
) -> Result<VisualRecord, AppError> {
    let (visual, event) = state
        .visuals()
        .create(request)
        .await
        .map_err(AppError::from)?;
    publish_visual_event(&app, &state, event).await?;
    Ok(visual)
}

#[tauri::command]
#[specta::specta]
async fn visuals_update(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
    request: VisualUpdateRequest,
) -> Result<VisualRecord, AppError> {
    let (visual, event) = state
        .visuals()
        .update(visual_id, request)
        .await
        .map_err(AppError::from)?;
    publish_visual_event(&app, &state, event).await?;
    Ok(visual)
}

#[tauri::command]
#[specta::specta]
async fn visuals_save(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
    tsx: Option<String>,
) -> Result<VisualRecord, AppError> {
    let (visual, event) = state
        .visuals()
        .save(visual_id, tsx)
        .await
        .map_err(AppError::from)?;
    publish_visual_event(&app, &state, event).await?;
    Ok(visual)
}

#[tauri::command]
#[specta::specta]
async fn visuals_fork(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
    title: Option<String>,
    session_id: Option<String>,
) -> Result<VisualRecord, AppError> {
    let (visual, event) = state
        .visuals()
        .fork(visual_id, title, session_id)
        .await
        .map_err(AppError::from)?;
    publish_visual_event(&app, &state, event).await?;
    Ok(visual)
}

#[tauri::command]
#[specta::specta]
async fn visuals_archive(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
) -> Result<VisualRecord, AppError> {
    let (visual, event) = state
        .visuals()
        .archive(visual_id)
        .await
        .map_err(AppError::from)?;
    publish_visual_event(&app, &state, event).await?;
    Ok(visual)
}

#[tauri::command]
#[specta::specta]
async fn visuals_show(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
    session_id: Option<String>,
) -> Result<VisualRecord, AppError> {
    let (visual, event) = state
        .visuals()
        .show(visual_id, session_id)
        .await
        .map_err(AppError::from)?;
    publish_visual_event(&app, &state, event).await?;
    Ok(visual)
}

#[tauri::command]
#[specta::specta]
async fn visuals_content(
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
) -> Result<VisualAsset, AppError> {
    state
        .visuals()
        .visual_source(visual_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn visuals_renditions(
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
) -> Result<Vec<VisualRendition>, AppError> {
    state
        .visuals()
        .list_renditions(visual_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn visuals_rendition(
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
    format: Option<String>,
    theme: Option<String>,
    size_class: Option<String>,
) -> Result<VisualAsset, AppError> {
    state
        .visuals()
        .visual_rendition(visual_id, format, theme, size_class)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn visuals_render(
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
) -> Result<VisualRecord, AppError> {
    state
        .visuals()
        .render_visual(&visual_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn synth_config_get() -> Result<BackendSettings, AppError> {
    synth_config::get().map_err(AppError::from)
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, specta::Type)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
struct ModelPerformanceMetric {
    model_id: String,
    provider: String,
    #[specta(type = specta_typescript::Unknown)]
    sample_count: u64,
    #[specta(type = specta_typescript::Unknown)]
    input_tokens: u64,
    #[specta(type = specta_typescript::Unknown)]
    cached_input_tokens: u64,
    #[specta(type = specta_typescript::Unknown)]
    output_tokens: u64,
    #[specta(type = specta_typescript::Unknown)]
    total_tokens: u64,
    output_tps_p50: f64,
    output_tps_p95: f64,
    total_tpm_p50: f64,
    total_tpm_p95: f64,
    latency_ms_p50: f64,
    latency_ms_p95: f64,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, specta::Type)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
struct ModelPerformanceSnapshot {
    window_minutes: u16,
    generated_at: String,
    models: Vec<ModelPerformanceMetric>,
}

#[tauri::command]
#[specta::specta]
async fn model_performance_get(
    window_minutes: Option<u16>,
) -> Result<ModelPerformanceSnapshot, AppError> {
    let backend = synth_config::resolve().map_err(AppError::from)?;
    let api_key = backend
        .api_key
        .ok_or_else(|| AppError::message("Sign in to read Synth Cloud model telemetry"))?;
    let window_minutes = window_minutes.unwrap_or(60).clamp(1, 1_440);
    let url = format!(
        "{}/api/v1/usage/model-performance?window_minutes={window_minutes}",
        backend.backend_url.trim_end_matches('/')
    );
    let response = http::http_client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(limits::MODEL_PERFORMANCE_TIMEOUT)
        .build()
        .map_err(AppError::from)?
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
                AppError::message(
                    "Synth Cloud telemetry could not be reached. Check Account → Synth backend URL.",
                )
            } else {
                AppError::message(format!("Synth Cloud telemetry request failed: {error}"))
            }
        })?;
    let status = response.status();
    if !status.is_success() {
        let detail = response.text().await.unwrap_or_default();
        return Err(AppError::message(format!(
            "Synth Cloud telemetry returned {status}: {}",
            detail.chars().take(240).collect::<String>()
        )));
    }
    response
        .json::<ModelPerformanceSnapshot>()
        .await
        .map_err(|error| {
            AppError::message(format!("Invalid Synth Cloud telemetry response: {error}"))
        })
}

#[tauri::command]
#[specta::specta]
async fn synth_config_update(
    core: State<'_, Arc<CoreRuntime>>,
    request: BackendSettingsUpdate,
) -> Result<BackendSettings, AppError> {
    let settings = synth_config::update(request).map_err(AppError::from)?;
    core.reload_intern_config().await.map_err(AppError::from)?;
    Ok(settings)
}

/// Begin (or resume) browser sign-in via Workshop device pairing and open the
/// system browser. Returns display-safe state only.
#[tauri::command]
#[specta::specta]
async fn account_begin_sign_in(
    app: tauri::AppHandle,
    manager: State<'_, Arc<device_auth::DeviceAuthManager>>,
) -> Result<device_auth::SignInBegin, AppError> {
    let origin = device_auth::workshop_origin();
    let begin = manager.begin(&origin).await.map_err(AppError::from)?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(begin.verification_uri.clone(), None::<String>)
        .map_err(AppError::from)?;
    Ok(begin)
}

#[tauri::command]
#[specta::specta]
async fn codex_oauth_begin(
    app: tauri::AppHandle,
    manager: State<'_, Arc<codex_oauth::Manager>>,
) -> Result<codex_oauth::BeginResult, AppError> {
    let result = manager
        .inner()
        .clone()
        .begin()
        .await
        .map_err(AppError::from)?;
    if let Err(error) = app
        .opener()
        .open_url(result.authorize_url.clone(), None::<String>)
    {
        let _ = manager.cancel().await;
        return Err(AppError::from(error));
    }
    Ok(result)
}

#[tauri::command]
#[specta::specta]
async fn codex_oauth_complete_manual(
    manager: State<'_, Arc<codex_oauth::Manager>>,
    redirect_url: String,
) -> Result<codex_oauth::Status, AppError> {
    manager
        .complete_manual(&redirect_url)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn codex_oauth_status(
    manager: State<'_, Arc<codex_oauth::Manager>>,
) -> Result<codex_oauth::Status, AppError> {
    manager.status().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn codex_oauth_ensure_ready(
    manager: State<'_, Arc<codex_oauth::Manager>>,
) -> Result<codex_oauth::Status, AppError> {
    manager.ensure_ready().await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn codex_oauth_disconnect(
    manager: State<'_, Arc<codex_oauth::Manager>>,
    codex: State<'_, Arc<CodexManager>>,
) -> Result<codex_oauth::Status, AppError> {
    for session in codex.list().await {
        if session.provider_name == codex_oauth::PROVIDER_ID {
            codex
                .close(&session.session_id)
                .await
                .map_err(AppError::from)?;
        }
    }
    manager.disconnect().await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn codex_oauth_cancel(manager: State<'_, Arc<codex_oauth::Manager>>) -> Result<(), AppError> {
    manager.cancel().await.map_err(AppError::from)
}

/// One poll step of the pending sign-in. On success the key is stored through
/// synth_config and the Intern runtime reloads; the key never reaches the
/// renderer.
#[tauri::command]
#[specta::specta]
async fn account_poll_sign_in(
    core: State<'_, Arc<CoreRuntime>>,
    manager: State<'_, Arc<device_auth::DeviceAuthManager>>,
    cloud: State<'_, Arc<account_cloud::AccountCloudClient>>,
) -> Result<device_auth::SignInPoll, AppError> {
    let origin = device_auth::workshop_origin();
    let result = manager
        .poll(&origin, |key| synth_config::store_api_key(key))
        .await
        .map_err(AppError::from)?;
    if matches!(result, device_auth::SignInPoll::Active) {
        core.reload_intern_config().await.map_err(AppError::from)?;
        // A new key invalidates any cached snapshot, and the device is now
        // known to have paired at least once.
        cloud.clear_cache();
        let _ = account::mark_paired(core.storage(), chrono::Utc::now());
    }
    Ok(result)
}

#[tauri::command]
#[specta::specta]
fn account_cancel_sign_in(
    manager: State<'_, Arc<device_auth::DeviceAuthManager>>,
) -> Result<(), AppError> {
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
) -> Result<account::AccountSummary, AppError> {
    let settings = synth_config::get().map_err(AppError::from)?;
    let resolved = synth_config::resolve().map_err(AppError::from)?;
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
    .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn account_get_summary(
    core: State<'_, Arc<CoreRuntime>>,
    cloud: State<'_, Arc<account_cloud::AccountCloudClient>>,
) -> Result<account::AccountSummary, AppError> {
    account_summary_now(&core, &cloud, false).await
}

/// Force a snapshot refetch — used after returning from hosted checkout and by
/// the explicit retry in the account menu.
#[tauri::command]
#[specta::specta]
async fn account_refresh(
    core: State<'_, Arc<CoreRuntime>>,
    cloud: State<'_, Arc<account_cloud::AccountCloudClient>>,
) -> Result<account::AccountSummary, AppError> {
    account_summary_now(&core, &cloud, true).await
}

/// Open a backend-issued hosted billing URL in the system browser. Desktop
/// never renders a payment form and never receives card data.
#[tauri::command]
#[specta::specta]
async fn account_open_billing(
    app: tauri::AppHandle,
    cloud: State<'_, Arc<account_cloud::AccountCloudClient>>,
    action: account_cloud::BillingAction,
    tier: Option<String>,
) -> Result<String, AppError> {
    let resolved = synth_config::resolve().map_err(AppError::from)?;
    let url = cloud
        .billing_url(
            &resolved.backend_url,
            resolved.api_key.as_deref(),
            action,
            tier.as_deref(),
        )
        .await
        .map_err(AppError::from)?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url.clone(), None::<String>)
        .map_err(AppError::from)?;
    Ok(url)
}

#[tauri::command]
#[specta::specta]
async fn account_sign_out(
    core: State<'_, Arc<CoreRuntime>>,
    cloud: State<'_, Arc<account_cloud::AccountCloudClient>>,
) -> Result<BackendSettings, AppError> {
    synth_config::remove_api_key().map_err(AppError::from)?;
    // Cloud facts belong to the signed-out session; local history and the
    // device ledger stay untouched.
    cloud.clear_cache();
    core.reload_intern_config().await.map_err(AppError::from)?;
    synth_config::get().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn model_multi_agent_list() -> Result<Vec<ModelMultiAgentSetting>, AppError> {
    synth_config::model_multi_agent_settings().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn model_multi_agent_update(
    request: ModelMultiAgentUpdate,
) -> Result<Vec<ModelMultiAgentSetting>, AppError> {
    synth_config::update_model_multi_agent(request).map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn workspace_access_get() -> Result<WorkspaceAccessSettings, AppError> {
    synth_config::workspace_access_settings().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn workspace_access_update(
    request: WorkspaceAccessUpdate,
) -> Result<WorkspaceAccessSettings, AppError> {
    synth_config::update_workspace_access(request).map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn desktop_permissions_get() -> Result<DesktopPermissionSettings, AppError> {
    synth_config::desktop_permission_settings().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn desktop_permissions_update(
    request: DesktopPermissionUpdate,
) -> Result<DesktopPermissionSettings, AppError> {
    synth_config::update_desktop_permissions(request).map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn laguna_get_status(state: State<'_, Arc<LagunaManager>>) -> Result<LagunaStatus, AppError> {
    if state.status().await.phase == "unknown" {
        let root = runtime::workshop_root().map_err(AppError::from)?;
        if let Err(error) = state.ensure(&root).await {
            state.set_error(error.to_string()).await;
        }
        return Ok(state.status().await);
    }
    Ok(state.refresh().await)
}

#[tauri::command]
#[specta::specta]
async fn laguna_reload(state: State<'_, Arc<LagunaManager>>) -> Result<LagunaStatus, AppError> {
    let root = runtime::workshop_root().map_err(AppError::from)?;
    state.reload(&root).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn laguna_models_list(
    state: State<'_, Arc<LagunaManager>>,
) -> Result<Vec<LagunaModelHit>, AppError> {
    state.discover_models().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn laguna_models_set_directory(
    state: State<'_, Arc<LagunaManager>>,
    path: String,
) -> Result<LagunaModelHit, AppError> {
    state
        .select_model(std::path::Path::new(&path))
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn laguna_models_clear_directory(state: State<'_, Arc<LagunaManager>>) -> Result<(), AppError> {
    state.clear_selected_model().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn workspace_choose_directory(app: tauri::AppHandle) -> Result<Option<String>, AppError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Choose a project folder")
        .pick_folder(move |path| {
            let _ = sender.send(path.map(|value| value.to_string()));
        });
    receiver.await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn workspace_scope_get(
    core: State<'_, Arc<CoreRuntime>>,
    session_id: String,
) -> Result<Option<ConversationWorkspaceScope>, AppError> {
    workspace_scope::get(core.storage().database(), &session_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn workspace_scope_choose_and_attach(
    app: tauri::AppHandle,
    core: State<'_, Arc<CoreRuntime>>,
    codex: State<'_, Arc<CodexManager>>,
    session_id: String,
    proposed_access: WorkspaceAccessMode,
) -> Result<Option<ConversationWorkspaceScope>, AppError> {
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
    let Some(path) = receiver.await.map_err(AppError::from)? else {
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
    .map_err(AppError::from)?;
    // Scope is durable before the old process is fenced. Closing preserves
    // the thread record; the next send resumes it with the new revision.
    codex.close(&session_id).await.map_err(AppError::from)?;
    Ok(Some(scope))
}

#[tauri::command]
#[specta::specta]
async fn workspace_scope_recent_folders(
    core: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<String>, AppError> {
    workspace_scope::recent_folders(core.storage().database())
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn workspace_scope_attach_recent(
    core: State<'_, Arc<CoreRuntime>>,
    codex: State<'_, Arc<CodexManager>>,
    session_id: String,
    path: String,
) -> Result<ConversationWorkspaceScope, AppError> {
    let scope = workspace_scope::attach_recent(core.storage().database(), &session_id, &path)
        .await
        .map_err(AppError::from)?;
    codex.close(&session_id).await.map_err(AppError::from)?;
    Ok(scope)
}

#[tauri::command]
#[specta::specta]
async fn workspace_scope_remove_attachment(
    core: State<'_, Arc<CoreRuntime>>,
    codex: State<'_, Arc<CodexManager>>,
    session_id: String,
    path: String,
) -> Result<ConversationWorkspaceScope, AppError> {
    let scope = workspace_scope::remove_attachment(core.storage().database(), &session_id, &path)
        .await
        .map_err(AppError::from)?;
    codex.close(&session_id).await.map_err(AppError::from)?;
    Ok(scope)
}

#[tauri::command]
#[specta::specta]
async fn workspace_scope_request_agent_grant(
    core: State<'_, Arc<CoreRuntime>>,
    session_id: String,
    path: String,
    access: WorkspaceAccessMode,
    reason: String,
) -> Result<WorkspaceGrantRequest, AppError> {
    workspace_scope::request_grant(
        core.storage().database(),
        &session_id,
        &path,
        access,
        &reason,
    )
    .await
    .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn workspace_scope_grants_list(
    core: State<'_, Arc<CoreRuntime>>,
    session_id: String,
) -> Result<Vec<WorkspaceGrantRequest>, AppError> {
    workspace_scope::list_grants(core.storage().database(), &session_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn workspace_scope_deny_request(
    core: State<'_, Arc<CoreRuntime>>,
    request_id: String,
) -> Result<WorkspaceGrantRequest, AppError> {
    workspace_scope::deny_grant(core.storage().database(), &request_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn workspace_scope_approve_request(
    app: tauri::AppHandle,
    core: State<'_, Arc<CoreRuntime>>,
    codex: State<'_, Arc<CodexManager>>,
    request_id: String,
) -> Result<Option<ConversationWorkspaceScope>, AppError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Confirm the exact requested folder")
        .pick_folder(move |path| {
            let _ = sender.send(path.map(|v| v.to_string()));
        });
    let Some(path) = receiver.await.map_err(AppError::from)? else {
        return Ok(None);
    };
    let scope = workspace_scope::approve_grant(core.storage().database(), &request_id, &path)
        .await
        .map_err(AppError::from)?;
    codex
        .close(&scope.session_id)
        .await
        .map_err(AppError::from)?;
    Ok(Some(scope))
}

/// Fills in the provider secrets and Laguna base URL that only the Rust side
/// knows. Shared by the plain attach command and the atomic send command.
async fn prepare_codex_start(
    laguna: &LagunaManager,
    oauth: &codex_oauth::Manager,
    core: &CoreRuntime,
    mut request: CodexSessionStartRequest,
) -> Result<CodexSessionStartRequest, AppError> {
    // Never trust renderer-supplied roots. Rust persistence is authoritative.
    request.writable_roots.clear();
    let scope = workspace_scope::get(core.storage().database(), &request.session_id)
        .await
        .map_err(AppError::from)?;
    let scope = match scope {
        Some(scope) => {
            let requested =
                workspace_scope::canonical_directory(&request.workspace).map_err(AppError::from)?;
            if requested.to_string_lossy() != scope.workspace {
                return Err(
                    "requested workspace does not match the conversation's persisted scope".into(),
                );
            }
            scope
        }
        None => {
            request.workspace = workspace_scope::canonical_directory(&request.workspace)
                .map_err(AppError::from)?
                .to_string_lossy()
                .into_owned();
            return prepare_codex_provider(laguna, oauth, request).await;
        }
    };
    request.workspace = scope.workspace.clone();
    request.writable_roots = workspace_scope::writable_roots(&scope);
    prepare_codex_provider(laguna, oauth, request).await
}

async fn prepare_codex_provider(
    laguna: &LagunaManager,
    oauth: &codex_oauth::Manager,
    mut request: CodexSessionStartRequest,
) -> Result<CodexSessionStartRequest, AppError> {
    if request.multi_agent_version.is_none() {
        request.multi_agent_version =
            Some(synth_config::resolve_model_multi_agent(&request.model).map_err(AppError::from)?);
    }
    // Exactly one preparation rule per provider class — see
    // `codex::ProviderClass` for the endpoint/credential/custody table. User
    // credentials are only *staged* here; `CodexManager::start` exchanges them
    // for a loopback lease at spawn time, so preparing a send for a live,
    // reused child never invalidates the token that child is presenting.
    match codex::provider_class(request.provider_name.as_deref()) {
        codex::ProviderClass::LocalLaguna => {
            let root = runtime::workshop_root().map_err(AppError::from)?;
            request.base_url = laguna
                .ensure(&root)
                .await
                .map_err(AppError::from)?
                .ok_or_else(|| AppError::message("Laguna Responses server is unavailable"))?;
            // The Laguna key is this process's loopback service token, not a
            // user credential: the child talks to the local daemon directly
            // and no broker lease is involved.
            request.api_key = laguna.api_key().unwrap_or_default();
        }
        codex::ProviderClass::OpenRouter => {
            let key = synth_config::openrouter_api_key()
                .map_err(AppError::from)?
                .ok_or_else(|| {
                    AppError::message(
                        "OpenRouter API key is not configured. Add it in Synth backend settings.",
                    )
                })?;
            // The OpenRouter key is the user's, and leaks into shell snapshots
            // the same way the Synth key did; it goes into native custody too.
            codex::stage_brokered_credential(&mut request, &key)?;
        }
        codex::ProviderClass::SynthCloud => {
            let resolved = synth_config::resolve().map_err(AppError::from)?;
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
        codex::ProviderClass::OpenaiCodexOauth => {
            let credential = oauth
                .fresh_credential()
                .await
                .map_err(AppError::from)?
                .ok_or_else(|| {
                    AppError::message("Reconnect ChatGPT subscription in Settings → Models")
                })?;
            const ALLOWED: &[&str] = &["gpt-5.6-luna", "gpt-5.6-sol", "gpt-5.6-terra"];
            if !ALLOWED
                .iter()
                .any(|model| request.model.eq_ignore_ascii_case(model))
            {
                return Err(AppError::message(
                    "This model is not available through the ChatGPT subscription target",
                ));
            }
            request.base_url = "https://chatgpt.com/backend-api/codex".into();
            request.api_key = serde_json::to_string(&credential).map_err(AppError::from)?;
            request.provider_name = Some(codex_oauth::PROVIDER_ID.into());
            request.provider_title = Some("ChatGPT subscription (Codex OAuth)".into());
            request.provider_env_key = None;
            request.broker_credential = false;
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
#[specta::specta]
async fn codex_turn_send(
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    laguna: State<'_, Arc<LagunaManager>>,
    oauth: State<'_, Arc<codex_oauth::Manager>>,
    core: State<'_, Arc<CoreRuntime>>,
    mut request: CodexTurnSendRequest,
) -> Result<CodexSessionInfo, CodexTurnFailure> {
    let session_id = request.start.session_id.clone();
    request.start = prepare_codex_start(&laguna, &oauth, &core, request.start)
        .await
        .map_err(|error| CodexTurnFailure {
            code: "codex_provider_unavailable".into(),
            message: error.message.clone(),
            session_id,
            detail: error.detail,
        })?;
    state.send_turn(app, request).await
}

#[tauri::command]
#[specta::specta]
async fn codex_session_start(
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    laguna: State<'_, Arc<LagunaManager>>,
    oauth: State<'_, Arc<codex_oauth::Manager>>,
    core: State<'_, Arc<CoreRuntime>>,
    request: CodexSessionStartRequest,
) -> Result<CodexSessionInfo, AppError> {
    let request = prepare_codex_start(&laguna, &oauth, &core, request).await?;
    state.start(app, request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn codex_turn_start(
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    request: CodexTurnStartRequest,
) -> Result<CodexSessionInfo, AppError> {
    state.start_turn(app, request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn codex_turn_interrupt(
    state: State<'_, Arc<CodexManager>>,
    request: CodexSessionRequest,
) -> Result<(), AppError> {
    state
        .interrupt(&request.session_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn codex_thread_compact(
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    laguna: State<'_, Arc<LagunaManager>>,
    oauth: State<'_, Arc<codex_oauth::Manager>>,
    core: State<'_, Arc<CoreRuntime>>,
    request: CodexSessionStartRequest,
) -> Result<(), AppError> {
    let request = prepare_codex_start(&laguna, &oauth, &core, request).await?;
    state.compact(app, request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn codex_turn_steer(
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    request: CodexSteerRequest,
) -> Result<(), AppError> {
    state.steer_turn(app, request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn codex_approval_resolve(
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    approvals: State<'_, Arc<crate::session::approval::ApprovalBroker>>,
    request: CodexApprovalDecisionRequest,
) -> Result<(), AppError> {
    if approvals.is_pending(&request.approval_id).await {
        let decision = approvals
            .decision_from_shell(&request.approval_id, &request.decision)
            .await
            .map_err(AppError::from)?;
        approvals
            .resolve(&app, &request.session_id, &request.approval_id, decision)
            .await
            .map_err(AppError::from)?;
        return Ok(());
    }
    state
        .resolve_approval(app, request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn codex_session_close(
    state: State<'_, Arc<CodexManager>>,
    oauth: State<'_, Arc<codex_oauth::Manager>>,
    request: CodexSessionRequest,
) -> Result<(), AppError> {
    state
        .close(&request.session_id)
        .await
        .map_err(AppError::from)?;
    oauth
        .sync_from_session(&request.session_id)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn codex_sessions_list(
    state: State<'_, Arc<CodexManager>>,
) -> Result<Vec<CodexSessionRecord>, AppError> {
    Ok(state.list().await)
}

#[tauri::command]
#[specta::specta]
fn codex_default_workspace() -> Result<String, AppError> {
    let configured = synth_config::allowed_workspace_roots().map_err(|error| {
        AppError::message(format!("Cannot read workspace access settings: {error}"))
    })?;
    let permissions = synth_config::desktop_permission_settings().map_err(|error| {
        AppError::message(format!("Cannot read desktop permission settings: {error}"))
    })?;
    let path = synth_config::select_default_workspace_path(
        &configured,
        &permissions.sandbox_mode,
        std::env::var_os("SYNTH_DESKTOP_WORKSPACE").map(std::path::PathBuf::from),
        dirs::home_dir(),
        crate::instance::state_root().join("workspaces/default"),
    );
    std::fs::create_dir_all(&path)
        .map_err(|error| AppError::io(format!("Cannot create the default workspace: {error}")))?;
    let path = path
        .canonicalize()
        .map_err(|error| AppError::io(format!("Default workspace is unavailable: {error}")))?;
    if !path.is_dir() {
        return Err("Default workspace must be a directory".into());
    }
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
#[specta::specta]
fn terminal_create(
    app: tauri::AppHandle,
    state: State<'_, Arc<TerminalManager>>,
    request: TerminalCreateRequest,
) -> Result<TerminalInfo, AppError> {
    state.create(app, request).map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn terminal_list(
    state: State<'_, Arc<TerminalManager>>,
    workspace_id: Option<String>,
) -> Vec<TerminalInfo> {
    state.list(workspace_id.as_deref())
}

#[tauri::command]
#[specta::specta]
fn terminal_snapshot(
    state: State<'_, Arc<TerminalManager>>,
    terminal_id: String,
    after_sequence: Option<contract::specta::OpaqueInteger<u64>>,
) -> Result<Vec<TerminalEvent>, AppError> {
    state
        .snapshot(
            &terminal_id,
            after_sequence.map(|value| value.0).unwrap_or(0),
        )
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn terminal_write(
    state: State<'_, Arc<TerminalManager>>,
    terminal_id: String,
    data: String,
) -> Result<(), AppError> {
    state.write(&terminal_id, &data).map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn terminal_resize(
    state: State<'_, Arc<TerminalManager>>,
    terminal_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), AppError> {
    state
        .resize(&terminal_id, cols, rows)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn terminal_close(
    state: State<'_, Arc<TerminalManager>>,
    terminal_id: String,
) -> Result<(), AppError> {
    state.close(&terminal_id).map_err(AppError::from)
}

pub fn run() {
    if crate::visuals::mermaid::hidden_mode_requested() {
        std::process::exit(crate::visuals::mermaid::run_hidden_mode());
    }
    let specta = contract::specta::builder();

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
            let optimizer_manager = core.optimizers().manager().clone();
            let receipts = Arc::new(credential_broker::ReceiptStore::new());
            let broker = Arc::new(
                credential_broker::CredentialBroker::start(receipts.clone()).map_err(|error| {
                    std::io::Error::other(format!("start credential broker: {error}"))
                })?,
            );
            let approvals = Arc::new(crate::session::approval::ApprovalBroker::new(
                crate::session::SessionPersistence::from_core(Some(core.clone())),
            ));
            let whisper = Arc::new(whisper::WhisperManager::new());
            let codex = Arc::new(CodexManager::new(Some(core.clone()), broker.clone()));
            let supervisor = Arc::new(services::ServiceSupervisor::new());
            supervisor.register(laguna.clone());
            supervisor.register(optimizer_manager.clone());
            supervisor.register(whisper.clone());
            app.manage(core.clone());
            app.manage(migration);
            app.manage(codex.clone());
            app.manage(broker);
            app.manage(approvals.clone());
            app.manage(receipts);
            app.manage(whisper);
            app.manage(Arc::new(TerminalManager::new()));
            app.manage(Arc::new(device_auth::DeviceAuthManager::new()));
            app.manage(Arc::new(codex_oauth::Manager::production()));
            app.manage(Arc::new(account_cloud::AccountCloudClient::open()));
            app.manage(laguna.clone());
            app.manage(optimizer_manager.clone());
            app.manage(supervisor);

            // All committed CoreRuntime events reach Tauri through this single
            // forwarder. Producers only journal and broadcast.
            core.spawn_forwarder(app.handle().clone());

            let mut status_updates = laguna.subscribe();
            let status_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(status) = status_updates.recv().await {
                    let _ = status_handle
                        .emit(crate::contract::events::EventChannel::LAGUNA_STATUS, status);
                }
            });

            let mut optimizer_status = optimizer_manager.subscribe();
            let optimizer_status_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(status) = optimizer_status.recv().await {
                    let _ = optimizer_status_handle.emit(
                        crate::contract::events::EventChannel::OPTIMIZER_STATUS,
                        status,
                    );
                }
            });

            let bootstrap_handle = app.handle().clone();
            let bootstrap_core = core.clone();
            let bootstrap_approvals = approvals.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = bootstrap_core.bootstrap(&bootstrap_handle).await {
                    eprintln!("CoreRuntime bootstrap failed: {error}");
                }
                if let Err(error) = bootstrap_approvals.expire_restored(&bootstrap_handle).await {
                    eprintln!("approval restore failed: {error}");
                }
                if let Err(error) = bootstrap_core.resume_intern_providers().await {
                    eprintln!("Intern restart reconciliation failed: {error}");
                }
            });

            let ipc_core = core.clone();
            let ipc_app = app.handle().clone();
            let ipc_root = crate::storage::app_data_root();
            tauri::async_runtime::spawn(async move {
                match visuals_ipc::spawn(ipc_core, ipc_app, ipc_root).await {
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
        .invoke_handler(specta.invoke_handler())
        .build(tauri::generate_context!())
        .expect("error while building Synth Desktop")
        .run(|app, event| {
            if let RunEvent::ExitRequested { .. } = event {
                if let Some(supervisor) = app.try_state::<Arc<services::ServiceSupervisor>>() {
                    let supervisor = (*supervisor).clone();
                    tauri::async_runtime::block_on(supervisor.drain_all());
                }
            }
        });
}
