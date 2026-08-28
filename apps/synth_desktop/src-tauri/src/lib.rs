mod account;
mod account_cloud;
pub mod browser;
pub mod cloud;

/// Narrow public seam for the standalone Intern wire-contract integration
/// test. Re-exporting the protocol types keeps that test on the real crate
/// graph; path-including `cloud/mod.rs` built a second fake crate root and
/// failed as soon as cloud code referenced `crate::storage` or `crate::error`.
#[doc(hidden)]
pub mod intern_protocol_test_support {
    pub use crate::cloud::intern::{
        normalize_event, AsyncCommandRequest, AsyncEnsureRequest, CommandReceipt, InternClient,
        InternEvent, InternRuntime, RuntimeBinding, RuntimeKind, SyncCommandRequest,
        SyncCreateRequest,
    };
}
mod codex;
mod codex_oauth;
mod computer_use;
pub mod container_capabilities;
pub mod container_stream;
mod context;
pub mod contract;
pub mod core_runtime;
mod credential_broker;
pub mod data;
mod device_auth;
pub mod diagnostics;
mod domain;
mod domains;
pub mod error;
#[cfg(feature = "eval-driver")]
mod eval_driver;
pub mod experiments;
mod http;
mod instance;
mod intern_api;
pub mod ipc;
mod laguna;
mod laguna_adapters;
mod limits;
pub mod lineage;
mod model_catalog;
mod optimizers;
mod platform;
mod plugins;
pub mod presentation;
pub mod recovery;
mod reports;
mod runtime;
mod secrets;
mod services;
mod session;
mod skills;
pub mod storage;
mod synth_config;
mod composition;
mod adapters;
mod tariffs;
mod telemetry;
mod terminal;
pub mod trace_ingest;
pub mod trace_query;
pub mod training_artifacts;
pub mod training_models;
mod update_check;
mod visuals;
pub mod visuals_ipc;
mod whisper;
mod workspace_scope;

use base64::Engine as _;
use codex::{
    CodexApprovalDecisionRequest, CodexManager, CodexSessionInfo, CodexSessionRecord,
    CodexSessionRequest, CodexSessionStartRequest, CodexSteerRequest, CodexThreadItemsRequest,
    CodexThreadReadRequest, CodexTurnFailure, CodexTurnSendRequest, CodexTurnStartRequest,
};
pub use core_runtime::CoreRuntime;
use data::{
    ContainerDeployment, ContainerRegisterRequest, DataCounts, ResolvedTraceProjection,
    TraceRecord, UsageEntry,
};
use error::AppError;
use experiments::{
    ExperimentChildCreateRequest, ExperimentCreateRequest, ExperimentEvidenceAttachRequest,
    ExperimentFinalizeRequest, ExperimentGroup, ExperimentRelateRequest,
};
use intern_api::{
    InternControlResult, InternSendResult, InternSessionControlRequest, InternSessionCreateRequest,
    InternSessionSendRequest, InternSessionWire,
};
use laguna::{LagunaManager, LagunaModelHit, LagunaStatus};
use optimizers::{
    kernel::OptimizerRunViewV2, CheckpointInferRequest, HostedTrainingModelCatalog,
    OptimizerCreateRequest, OptimizerEventEnvelope, OptimizerImportLocalRequest, OptimizerQuery,
    OptimizerRecipeRunRequest, OptimizerReconcileRequest, OptimizerRelationship,
    OptimizerRunRecord, OptimizerStateSlice, SavedLoraCheckpoint, SavedLoraCheckpointPage,
    SavedLoraCheckpointQuery, SavedLoraDownload,
    SavedLoraPatchRequest,
};
use plugins::PluginStatus;
use reports::{
    ExperimentRecord, ExperimentRecordUpsert, ReportAudienceRequest, ReportAudienceState,
    ReportComment, ReportCommentCreate, ReportCreateRequest, ReportQuery, ReportRecord,
    ReportRevision, ReportRevisionCompare, ReportSeal, ReportSealBundle, ReportUpdateRequest,
    ReportUpload, ReportVisibilityRequest, ReportVisibilityRequestCreate, ResearchLogAppend,
    ResearchLogEntry,
};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use storage::{
    AppEvent, CoreDiagnostics, ModelPerformanceRepository, ModelPerformanceSummary,
    ModelPerformanceTurnSample,
};
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
    TemplateMeta, VisualAnnotation, VisualAnnotationCreate, VisualAsset, VisualCreateRequest,
    VisualQuery, VisualRecord, VisualRendition, VisualRevision, VisualSeal, VisualSealBundle,
    VisualUpdateRequest, VisualUpload,
};
use workspace_scope::WorkspaceGrantRequest;
use workspace_scope::{ConversationWorkspaceScope, WorkspaceAccessMode};

#[tauri::command]
#[specta::specta]
fn core_diagnostics(state: State<'_, Arc<CoreRuntime>>) -> Result<CoreDiagnostics, AppError> {
    state.diagnostics().map_err(AppError::from)
}

/// Every runtime version Desktop pins, resolved from the one contract table.
///
/// Settings → About renders exactly this struct. Answering "which runtime is
/// installed, and is it new enough?" used to require reading the source.
#[tauri::command]
#[specta::specta]
fn runtime_contracts(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<Vec<contract::runtimes::RuntimeContractView>, AppError> {
    use contract::runtimes::{ReleaseChannel, ALL};
    let channel = match crate::plugins::PluginRegistry::open_default()
        .release_channel()
        .as_str()
    {
        "dev" => ReleaseChannel::Dev,
        _ => ReleaseChannel::Official,
    };
    let sidecar = state
        .optimizers()
        .manager()
        .version()
        .ok()
        .flatten()
        .map(|hit| hit.version);
    let eval = crate::optimizers::eval_runtime::installed_version().or_else(|| {
        crate::optimizers::eval_runtime::provision_from_disk()
            .ok()
            .map(|manifest| manifest.version)
    });
    Ok(ALL
        .iter()
        .map(|entry| {
            let found = match entry.runtime_id {
                "eval" => eval.clone(),
                _ => entry
                    .provisioned_by_desktop
                    .then(|| sidecar.clone())
                    .flatten(),
            };
            entry.view(channel, found)
        })
        .collect())
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
        .ok_or_else(|| AppError::untyped("Screenshot has no supported format"))?;
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return Err(AppError::invalid_argument("Screenshot format is unsupported")),
    };
    let metadata =
        std::fs::metadata(&path).map_err(|_| AppError::io("Screenshot is unavailable"))?;
    if !metadata.is_file() || metadata.len() > limits::IMAGE_PREVIEW_MAX_BYTES {
        return Err(AppError::invalid_argument("Screenshot must be a file smaller than 20 MB"));
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
async fn core_session_events_tail(
    state: State<'_, Arc<CoreRuntime>>,
    session_id: String,
    limit: Option<contract::specta::OpaqueInteger<i64>>,
) -> Result<Vec<AppEvent>, AppError> {
    state
        .journal()
        .session_events_tail(
            session_id,
            limit.map(|value| value.0).unwrap_or(250).clamp(1, 2000),
        )
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn core_session_events_before(
    state: State<'_, Arc<CoreRuntime>>,
    session_id: String,
    before_sequence: contract::specta::OpaqueInteger<i64>,
    limit: Option<contract::specta::OpaqueInteger<i64>>,
) -> Result<Vec<AppEvent>, AppError> {
    state
        .journal()
        .session_events_before(
            session_id,
            before_sequence.0,
            limit.map(|value| value.0).unwrap_or(1000).clamp(1, 2000),
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
        .map(|session| {
            crate::telemetry::mark_once(
                "first_workspace_opened",
                serde_json::json!({"workflow_family": "intern"}),
            );
            session
        })
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
        .map(|result| {
            crate::telemetry::mark_once(
                "first_run_succeeded",
                serde_json::json!({"workflow_family": "intern", "outcome": "success"}),
            );
            crate::telemetry::emit(
                "hosted_job_started",
                serde_json::json!({"workflow_family": "intern"}),
            );
            crate::telemetry::emit(
                "workflow_started",
                serde_json::json!({"workflow_family": "intern"}),
            );
            result
        })
        .map_err(|error| {
            crate::telemetry::emit(
                "hosted_job_failed",
                serde_json::json!({
                    "workflow_family": "intern",
                    "outcome": "failure",
                    "error_class": "request_failed"
                }),
            );
            crate::telemetry::emit(
                "recovery_attempted",
                serde_json::json!({"error_class": "request_failed", "outcome": "failure"}),
            );
            AppError::from(error)
        })
}

#[tauri::command]
#[specta::specta]
async fn intern_session_control(
    state: State<'_, Arc<CoreRuntime>>,
    request: InternSessionControlRequest,
) -> Result<InternControlResult, AppError> {
    let kind = request.kind.clone();
    intern_api::control(&state, request)
        .await
        .map(|result| {
            match kind.as_str() {
                "close" => {
                    crate::telemetry::emit(
                        "hosted_job_completed",
                        serde_json::json!({"workflow_family": "intern", "outcome": "success"}),
                    );
                    crate::telemetry::emit(
                        "workflow_terminal",
                        serde_json::json!({"workflow_family": "intern", "outcome": "success"}),
                    );
                }
                "cancel" => {
                    crate::telemetry::emit(
                        "hosted_job_failed",
                        serde_json::json!({
                            "workflow_family": "intern",
                            "outcome": "cancelled",
                            "error_class": "cancelled"
                        }),
                    );
                    crate::telemetry::emit(
                        "workflow_terminal",
                        serde_json::json!({
                            "workflow_family": "intern",
                            "outcome": "cancelled",
                            "error_class": "cancelled"
                        }),
                    );
                }
                _ => {}
            }
            result
        })
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
async fn training_artifacts_launch_inference(
    id: String,
    message: Option<String>,
    confirm: bool,
) -> Result<contract::specta::OpaqueJson, AppError> {
    if !confirm {
        return Err(AppError::from(anyhow::anyhow!(
            "launch_artifact_inference requires confirm=true"
        )));
    }
    let prompt = message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Reply with one short sentence confirming which adapter you are.");
    optimizers::launch_artifact_inference(&id, prompt)
        .await
        .map(contract::specta::OpaqueJson)
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
    force_info_refresh: bool,
) -> (String, serde_json::Value, serde_json::Value, Option<String>) {
    let client = http::http_client_with_timeout(limits::CONTAINER_PROBE_TIMEOUT);
    let root = base_url.trim_end_matches('/');
    // Health is intentionally cheap and may run frequently. Contract/catalog
    // hydration is cached because task catalogs can contain thousands of rows.
    // `container_probe` always refreshes `/info` so prepare sees revision N+1.
    let refresh_metadata = force_info_refresh
        || existing_metadata
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
            let status = crate::container_capabilities::observed_status(code.as_u16(), &payload);
            (
                status.to_string(),
                serde_json::json!({
                    "ok": status == crate::container_capabilities::READY_STATUS,
                    "status": code.as_u16(),
                    "payload": payload
                }),
            )
        }
        Err(error) => (
            crate::container_capabilities::UNHEALTHY_STATUS.into(),
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
                .and_then(|value| {
                    value
                        .get("env_family")
                        .or_else(|| value.get("task_family"))
                        // HealthBench publishes its explicit service family as
                        // `runtime_family`; preserve that observed contract so
                        // the selector can find the registered GEPA-v2 pool.
                        // Do not infer from a caller name, port, or URL.
                        .or_else(|| value.get("runtime_family"))
                })
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .or_else(|| {
            // Packaged GEPA services identify their task through the immutable
            // runtime id rather than an optional top-level `task_family`.
            // Use that advertised value only; never infer the target from the
            // user supplied name, URL, or port.
            info.as_ref()
                .and_then(|value| value.pointer("/runtime/runtime_id"))
                .and_then(|value| value.as_str())
                .and_then(|runtime_id| {
                    let normalized = runtime_id.to_ascii_lowercase();
                    ["banking77", "healthbench", "craftax", "alfworld"]
                        .into_iter()
                        .find(|family| normalized.contains(family))
                        .map(str::to_string)
                })
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
    // One writer for the typed capability surface: `container_list`,
    // `container_get`, and `container_probe` must never disagree about what a
    // record can do. Reusing a cached `/info` body keeps the earlier
    // observation time so a health-only refresh cannot launder a stale one.
    let declared = synth_config::container_capability_declaration(base_url).unwrap_or_default();
    crate::container_capabilities::write_capability_metadata(
        &mut metadata,
        info.as_ref(),
        declared.as_ref(),
        refresh_metadata,
        chrono::Utc::now(),
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
    let live = status == crate::container_capabilities::READY_STATUS
        && health.get("ok").and_then(|value| value.as_bool()) != Some(false);
    crate::optimizers::container_lifecycle::stamp_metadata_freshness(
        &mut metadata,
        live,
        &chrono::Utc::now().to_rfc3339(),
    );
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
        return Err(AppError::invalid_argument(
            "container baseUrl must start with http:// or https://",
        ));
    }
    let (status, health, metadata, hydrated_family) = hydrate_container(
        &request.base_url,
        request
            .metadata
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
        true,
    )
    .await;
    let task_family = hydrated_family.or_else(|| request.task_family.clone());
    if let Some(error) = metadata
        .get("liveEvalError")
        .and_then(|value| value.as_str())
        .filter(|error| error.contains("live_frames"))
    {
        return Err(AppError::invalid_argument(error));
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
        hydrate_container(base_url, container.metadata, true).await;
    state
        .update_container_hydration(container_id, status, health, metadata, task_family)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn data_containers_reconcile(
    state: State<'_, Arc<CoreRuntime>>,
    container_id: String,
    session_id: String,
) -> Result<ContainerDeployment, AppError> {
    crate::optimizers::container_lifecycle::reconcile_declaration(
        state.storage().database(),
        &session_id,
        &container_id,
    )
    .map_err(AppError::from)?;
    state
        .data()
        .get_container(container_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn data_containers_restart(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    container_id: String,
    session_id: String,
) -> Result<ContainerDeployment, AppError> {
    crate::visuals_ipc::dispatch_container_restart(
        &format!("/v1/containers/{container_id}/restart"),
        serde_json::json!({ "sessionRef": session_id }),
        state.inner(),
        &app,
    )
    .await
    .map_err(AppError::from)?;
    state
        .data()
        .get_container(container_id)
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
async fn data_trace_materialize(
    state: State<'_, Arc<CoreRuntime>>,
    container_id: String,
    rollout_id: String,
) -> Result<contract::specta::OpaqueJson, AppError> {
    crate::visuals_ipc::import_container_trace(&state, &container_id, &rollout_id)
        .await
        .map(contract::specta::OpaqueJson)
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

#[tauri::command]
#[specta::specta]
async fn model_performance_turn_samples(
    state: State<'_, Arc<CoreRuntime>>,
    session_id: String,
) -> Result<Vec<ModelPerformanceTurnSample>, AppError> {
    ModelPerformanceRepository::new(state.storage().database().clone())
        .turn_samples(session_id)
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
        .summary(window, since_ms, offset_seconds)
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

/// Freeze policy files from the session's workspace into one immutable
/// candidate set. Its id is the only policy input `optimizers_recipe_start`
/// accepts for an `eval.*` recipe.
#[tauri::command]
#[specta::specta]
async fn optimizers_stage_eval_candidates(
    state: State<'_, Arc<CoreRuntime>>,
    request: optimizers::EvalStageCandidatesRequest,
) -> Result<contract::specta::OpaqueJson, AppError> {
    state
        .optimizers()
        .stage_eval_candidates(request)
        .await
        .map(contract::specta::OpaqueJson)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_recipe_start(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    codex: State<'_, Arc<CodexManager>>,
    request: OptimizerRecipeRunRequest,
) -> Result<OptimizerRunRecord, AppError> {
    authorize_optimizer_recipe_start(&app, &state, &codex, request).await
}

pub(crate) async fn authorize_optimizer_recipe_start(
    app: &tauri::AppHandle,
    state: &CoreRuntime,
    codex: &CodexManager,
    request: OptimizerRecipeRunRequest,
) -> Result<OptimizerRunRecord, AppError> {
    let should_open_visual = request.open_visual.unwrap_or(false);
    let visual_session_ref = request.session_ref.clone();
    let recipe = state
        .optimizers()
        .list_recipes_for_session(request.session_ref.as_deref())
        .into_iter()
        .find(|recipe| recipe.get("id").and_then(Value::as_str) == Some(request.recipe_id.as_str()))
        .ok_or_else(|| {
            AppError::from(anyhow::anyhow!(
                "unknown optimizer recipe: {}",
                request.recipe_id
            ))
        })?;
    // Local MLX recipes and the pinned local eval smoke do not incur provider
    // charges. The click itself is the operator's explicit instruction.
    if matches!(
        request.recipe_id.as_str(),
        "sft.qwen35-2b.mlx.v1" | "cispo.mlx.v1" | "eval.fixture.policy-smoke.v1"
    ) {
        let (run, event) = state
            .optimizers()
            .start_recipe(request)
            .await
            .map_err(AppError::from)?;
        publish_optimizer_event(app, state, event).await?;
        return Ok(run);
    }
    let session_id = request
        .session_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let requesting_agent = session_id
        .map(|value| format!("Agent session {value}"))
        .unwrap_or_else(|| "Workshop operator".into());
    let algorithm_id = recipe.get("algorithmId").and_then(Value::as_str);
    let is_local_eval = algorithm_id == Some("eval");
    let is_container_baseline_eval = is_local_eval
        && recipe.get("source").and_then(Value::as_str) == Some("workspace")
        && recipe.get("semantics").and_then(Value::as_str) == Some("baseline_eval");
    // Hosted SFT is owned by the public synth-optimizers control plane and
    // does not use the optional local Optimizers sidecar. Requiring that
    // sidecar made an otherwise configured public SFT recipe unreachable.
    let is_hosted_sft =
        algorithm_id == Some("sft") && request.recipe_id != "sft.craftax.gpt-oss.smoke.v1";
    let limits = recipe
        .get("limits")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let (max_cost_usd, max_rollouts) = if is_container_baseline_eval {
        // These recipes evaluate the policy already pinned by a registered
        // container. They have no candidate set: requiring one here prevents
        // the public MCP route from ever reaching `container_eval::start`.
        (
            limits
                .get("maxCostUsd")
                .or_else(|| limits.get("costCeilingUsd"))
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value > 0.0),
            limits.get("maxTotalRollouts").and_then(Value::as_u64),
        )
    } else if is_local_eval {
        let (cost, trials) = {
            let candidate_set_id =
                optimizers::resolve_eval_candidate_set(&request).map_err(AppError::from)?;
            optimizers::paid_compute_bounds(&recipe, Some(candidate_set_id.as_str()))
                .map_err(AppError::from)?
        };
        (Some(cost), Some(trials))
    } else {
        (
            limits
                .get("maxCostUsd")
                .or_else(|| limits.get("costCeilingUsd"))
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value > 0.0),
            [
                "maxTotalRollouts",
                "maxTotalEnvironmentRollouts",
                "maxSearchRollouts",
            ]
            .into_iter()
            .find_map(|key| limits.get(key).and_then(Value::as_u64)),
        )
    };
    let paid_cap = session::approval::PaidComputeCap {
        max_cost_usd_micros: max_cost_usd.map(|value| (value * 1_000_000.0).round() as u64),
        max_rollouts,
    };
    if !paid_cap.is_bounded() {
        return Err(AppError::from(anyhow::anyhow!(
            "optimizer recipe `{}` does not declare an enforceable paid-compute cap",
            request.recipe_id
        )));
    }
    let credential_names = optimizer_recipe_credentials_from_catalog(&recipe, &request.recipe_id);
    if credential_names.iter().any(|name| name == "OPENAI_API_KEY") {
        if let Some(secrets) = crate::secrets::live() {
            let models = recipe
                .get("models")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|model| {
                    model
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| model.as_str())
                        .map(str::to_string)
                })
                .collect::<Vec<_>>();
            let calls_per_rollout = limits
                .get("maximumModelCallsPerRollout")
                .or_else(|| limits.get("maxCallsPerRollout"))
                .and_then(serde_json::Value::as_u64)
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    AppError::from(anyhow::anyhow!(
                        "optimizer recipe `{}` has a provider credential but no admitted \
                         maximumModelCallsPerRollout",
                        request.recipe_id
                    ))
                })?;
            let admitted_rollouts = max_rollouts.filter(|value| *value > 0).ok_or_else(|| {
                AppError::from(anyhow::anyhow!(
                    "optimizer recipe `{}` has a provider credential but no admitted rollout cap",
                    request.recipe_id
                ))
            })?;
            let admitted_cost_micros = paid_cap
                .max_cost_usd_micros
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    AppError::from(anyhow::anyhow!(
                        "optimizer recipe `{}` has a provider credential but no admitted cost cap",
                        request.recipe_id
                    ))
                })?;
            let total_calls = admitted_rollouts
                .saturating_mul(calls_per_rollout)
                .min(u64::from(u32::MAX)) as u32;
            let policy = optimizers::admission::provider_use_policy_from_bounds(
                vec!["chat.completions.create".into()],
                models,
                Vec::new(),
                total_calls,
                admitted_cost_micros,
                crate::limits::SECRETS_CAPABILITY_TTL.as_secs(),
                None,
                None,
            );
            secrets
                .preflight_openai_route(&request.recipe_id, policy)
                .map_err(AppError::from)?;
        } else {
            return Err(AppError::from(
                crate::secrets::lease::CredentialError::new(
                    crate::secrets::lease::PROXY_NOT_RUNNING,
                    "proxy",
                    true,
                    "Workshop secrets proxy is not running",
                )
                .anyhow(),
            ));
        }
    }
    let paid = session::approval::ApprovalKind::PaidCompute {
        operation: "optimizer.recipe.start".into(),
        parameters: serde_json::json!({
            "recipeId": request.recipe_id,
            "algorithmId": recipe.get("algorithmId"),
            "task": recipe.get("task"),
            "limits": limits,
        }),
        estimated_cost_usd_micros: paid_cap.max_cost_usd_micros,
        requested_cap: paid_cap.clone(),
        requesting_agent,
        recipe_id: Some(request.recipe_id.clone()),
        dataset: None,
        proposer_model: None,
        evaluator_model: None,
        timeout_seconds: None,
        credential_names: credential_names.clone(),
        preparation_digest: None,
    };
    let paid_approval_id = codex
        .approvals
        .authorize_host(app, session_id, paid)
        .await
        .map_err(AppError::from)?;

    for provider in &credential_names {
        codex
            .approvals
            .authorize_host(
                app,
                session_id,
                session::approval::ApprovalKind::CredentialAccess {
                    consent: session::approval::CredentialConsent::IssueLease,
                    provider: provider.clone(),
                    purpose: format!("run bounded optimizer recipe {}", request.recipe_id),
                    locator_id: None,
                    display_path: None,
                    variable: None,
                    switch_from_display: None,
                },
            )
            .await
            .map_err(AppError::from)?;
    }

    let sidecar_status = state.optimizers().manager().refresh().await;
    if !is_local_eval && !is_hosted_sft && sidecar_status.phase != "ready" {
        let action = if sidecar_status.phase == "not_installed" {
            "install_and_start"
        } else {
            "start"
        };
        codex
            .approvals
            .authorize_host(
                app,
                session_id,
                session::approval::ApprovalKind::SidecarLifecycle {
                    sidecar: "optimizers".into(),
                    action: action.into(),
                },
            )
            .await
            .map_err(AppError::from)?;
    }

    let (run, event) = state
        .optimizers()
        .start_recipe(request)
        .await
        .map_err(AppError::from)?;
    publish_optimizer_event(app, state, event).await?;
    let run = state
        .optimizers()
        .attach_paid_compute_approval(
            run.id,
            &paid_approval_id,
            paid_cap.max_cost_usd_micros,
            paid_cap.max_rollouts,
        )
        .await
        .map_err(AppError::from)?;
    if should_open_visual {
        let published = run
            .visual_refs
            .iter()
            .find(|reference| reference.kind == "visual")
            .map(|reference| reference.id.clone());
        let visual_id = match published {
            Some(visual_id) => Some(visual_id),
            None => {
                // Admission promised to open an output. A recipe whose service
                // minted no visual must still get one bound to this run and this
                // chat, or the run finishes with nothing in Outputs and nothing
                // saying why.
                let (opened, event) = state
                    .optimizers()
                    .open_visual_in_session(run.id.clone(), visual_session_ref.clone())
                    .await
                    .map_err(AppError::from)?;
                publish_optimizer_event(app, state, event).await?;
                opened
                    .visual_refs
                    .iter()
                    .find(|reference| reference.kind == "visual")
                    .map(|reference| reference.id.clone())
            }
        };
        if let Some(visual_id) = visual_id {
            // Optimizer services create and show visuals internally, but their
            // returned event slot carries the optimizer lifecycle event. Emit
            // a fresh durable visual.show so the renderer receives ownership,
            // adds the visual to this chat's Outputs shelf, and opens the pane
            // without requiring a second agent tool call.
            //
            // The session here is the run's own conversation, never whichever
            // chat happens to be focused: showing is publication into an owner,
            // not a global pane change.
            let (_, event) = state
                .visuals()
                .show(visual_id.clone(), visual_session_ref)
                .await
                .map_err(AppError::from)?;
            publish_visual_event(app, state, event).await?;
            let _ = app.emit(
                crate::core_runtime::VISUAL_SHOW_CHANNEL,
                serde_json::json!({
                    "kind": "visual.show",
                    "payload": { "visualId": visual_id }
                }),
            );
        }
    }
    Ok(run)
}

/// Inline-first evaluation admission. The catalog is intentionally absent:
/// conversational evaluations arrive as typed constraints, are materialized
/// from current container authority, and execute only after their immutable
/// digest is approved.
pub(crate) async fn authorize_inline_evaluation_start(
    app: &tauri::AppHandle,
    state: &CoreRuntime,
    codex: &CodexManager,
    request: optimizers::admission::InlineRequest,
    session_ref: Option<String>,
    open_visual: bool,
) -> Result<OptimizerRunRecord, AppError> {
    let session_id = session_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::invalid_argument(
                "this mutation requires an agent session for approval",
            )
        })?;
    let admissible = optimizers::inline_eval::admit_inline(state.optimizers(), request)
        .await
        .map_err(AppError::from)?;
    let disclosure = admissible.approval_disclosure();
    let recipe = &admissible.spec().recipe;
    let max_cost_usd_micros = recipe.resource_limits.hard_total_cost_micros.as_micros();
    let max_rollouts = recipe.rollout_plan.maximum_rollouts.0.get() as u64;
    let paid_cap = session::approval::PaidComputeCap {
        max_cost_usd_micros: Some(max_cost_usd_micros),
        max_rollouts: Some(max_rollouts),
    };
    let requesting_agent = format!("Agent session {session_id}");
    let provider = recipe.model.provider.as_str().to_string();
    let model = recipe.model.model_id.as_str().to_string();
    let paid_approval_id = codex
        .approvals
        .authorize_host(
            app,
            Some(session_id),
            session::approval::ApprovalKind::PaidCompute {
                operation: "optimizer.evaluation.inline.start".into(),
                parameters: disclosure,
                estimated_cost_usd_micros: None,
                requested_cap: paid_cap.clone(),
                requesting_agent,
                recipe_id: None,
                dataset: None,
                proposer_model: Some(model),
                evaluator_model: None,
                timeout_seconds: None,
                credential_names: vec![format!("{provider}:workshop_secrets_proxy")],
                preparation_digest: Some(admissible.digest().as_str().to_string()),
            },
        )
        .await
        .map_err(AppError::from)?;

    let approved = optimizers::inline_eval::bind_approval(admissible, &paid_approval_id)
        .map_err(AppError::from)?;
    optimizers::inline_eval::reverify(state.optimizers(), &approved)
        .await
        .map_err(AppError::from)?;
    let (run, event) =
        optimizers::inline_eval::execute(state.optimizers(), approved, session_ref.clone())
            .await
            .map_err(AppError::from)?;
    publish_optimizer_event(app, state, event).await?;
    let run = state
        .optimizers()
        .attach_paid_compute_approval(
            run.id,
            &paid_approval_id,
            paid_cap.max_cost_usd_micros,
            paid_cap.max_rollouts,
        )
        .await
        .map_err(AppError::from)?;
    if open_visual {
        if let Some(visual_id) = run
            .visual_refs
            .iter()
            .find(|reference| reference.kind == "visual")
            .map(|reference| reference.id.clone())
        {
            let (_, event) = state
                .visuals()
                .show(visual_id.clone(), session_ref)
                .await
                .map_err(AppError::from)?;
            publish_visual_event(app, state, event).await?;
            let _ = app.emit(
                crate::core_runtime::VISUAL_SHOW_CHANNEL,
                serde_json::json!({
                    "kind": "visual.show",
                    "payload": { "visualId": visual_id }
                }),
            );
        }
    }
    Ok(run)
}

/// Re-observe the target contract at workflow admission. A cached healthy bit
/// is liveness evidence, not permission to reuse an older capability revision.
/// Only container-backed product recipes need this lane; optimizer campaigns
/// keep their own service/cookbook admission in Optimizers.
pub(crate) async fn refresh_optimizer_workflow_containers(
    state: &CoreRuntime,
    _recipe_id: &str,
) -> Result<(), AppError> {
    let rows = state
        .data()
        .list_containers()
        .await
        .map_err(AppError::from)?;
    for row in rows {
        let Some(base_url) = row
            .base_url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        else {
            continue;
        };
        let (status, health, metadata, hydrated_family) =
            hydrate_container(base_url, row.metadata.clone(), true).await;
        state
            .data()
            .update_container_hydration(
                row.id,
                status,
                health,
                metadata,
                hydrated_family.or(row.task_family),
            )
            .await
            .map_err(AppError::from)?;
    }
    Ok(())
}

fn optimizer_recipe_credentials(recipe_id: &str) -> &'static [&'static str] {
    if recipe_id == "sft.craftax.gpt-oss.smoke.v1" {
        &["GROQ_API_KEY", "TINKER_API_KEY"]
    } else if recipe_id == "gelo.craftax.hosted.v1" {
        &["OPTIMIZERS_BETA_SERVICE_TOKEN"]
    } else if matches!(
        recipe_id,
        "sft.craftax.nemotron-nano.tinker.v1" | "sft.banking77.nemotron-lightning.tinker.v1"
    ) {
        &["SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN"]
    } else if recipe_id.starts_with("gepa.") || recipe_id.starts_with("eval.") {
        &["OPENAI_API_KEY"]
    } else {
        &[]
    }
}

fn optimizer_recipe_credentials_from_catalog(recipe: &Value, recipe_id: &str) -> Vec<String> {
    let declared = recipe
        .get("credentialInputs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        });
    declared.unwrap_or_else(|| {
        optimizer_recipe_credentials(recipe_id)
            .iter()
            .map(|name| (*name).to_owned())
            .collect()
    })
}

#[cfg(test)]
mod optimizer_recipe_credential_tests {
    use super::*;

    #[test]
    fn workspace_recipe_catalog_overrides_legacy_prefix_default() {
        let recipe = serde_json::json!({
            "id": "gepa.openrouter.smoke.v1",
            "credentialInputs": ["OPENROUTER_API_KEY"]
        });
        assert_eq!(
            optimizer_recipe_credentials_from_catalog(&recipe, "gepa.openrouter.smoke.v1"),
            vec!["OPENROUTER_API_KEY"]
        );
    }
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
async fn optimizers_run_view_v2(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<OptimizerRunViewV2, AppError> {
    state
        .optimizers()
        .run_view_v2(optimizer_run_id)
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
async fn optimizers_frames_latest(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
    after_frame_sequence: Option<contract::specta::OpaqueInteger<u64>>,
) -> Result<crate::optimizers::OptimizerFrameDelta, AppError> {
    state
        .optimizers()
        .frames_latest(
            optimizer_run_id,
            after_frame_sequence.map(|value| value.0).unwrap_or(0),
        )
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_frames_list(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
    seed: contract::specta::OpaqueInteger<i64>,
    before_frame_sequence: Option<contract::specta::OpaqueInteger<u64>>,
    limit: Option<contract::specta::OpaqueInteger<i64>>,
) -> Result<Vec<crate::optimizers::OptimizerFrameRef>, AppError> {
    state
        .optimizers()
        .frames_list(
            optimizer_run_id,
            seed.0,
            before_frame_sequence.map(|value| value.0),
            limit.map(|value| value.0),
        )
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_frame_content(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
    seed: contract::specta::OpaqueInteger<i64>,
    frame_sequence: contract::specta::OpaqueInteger<u64>,
) -> Result<crate::optimizers::OptimizerFrameContent, AppError> {
    state
        .optimizers()
        .frame_content(optimizer_run_id, seed.0, frame_sequence.0)
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
    // Provenance is attached at the boundary that knows it: this command is
    // the user's own UI gesture.
    let request = optimizers::kernel::CancellationRequest::new(
        optimizers::kernel::CancellationCause::UserRequested,
        "user:ui",
        format!("run:{optimizer_run_id}"),
    );
    let (run, event) = state
        .optimizers()
        .cancel(optimizer_run_id, request)
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
async fn optimizers_saved_loras_search(
    state: State<'_, Arc<CoreRuntime>>,
    query: Option<SavedLoraCheckpointQuery>,
) -> Result<SavedLoraCheckpointPage, AppError> {
    state
        .optimizers()
        .search_saved_lora_checkpoints(query.unwrap_or_default())
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_run_checkpoints_list(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<optimizers::SavedLoraRunPage, AppError> {
    state
        .optimizers()
        .list_saved_lora_checkpoints_for_run(optimizer_run_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_run_outputs(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<optimizers::OptimizerRunOutputs, AppError> {
    state
        .optimizers()
        .run_outputs(optimizer_run_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_training_models(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<HostedTrainingModelCatalog, AppError> {
    state
        .optimizers()
        .hosted_training_models()
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_saved_lora_archive(
    state: State<'_, Arc<CoreRuntime>>,
    checkpoint_id: String,
) -> Result<SavedLoraCheckpoint, AppError> {
    state
        .optimizers()
        .archive_saved_lora_checkpoint(checkpoint_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_saved_lora_download(
    state: State<'_, Arc<CoreRuntime>>,
    checkpoint_id: String,
) -> Result<SavedLoraDownload, AppError> {
    state
        .optimizers()
        .saved_lora_download(checkpoint_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_saved_lora_import(
    state: State<'_, Arc<CoreRuntime>>,
    path: String,
) -> Result<SavedLoraCheckpoint, AppError> {
    state
        .optimizers()
        .import_saved_lora_dir(path)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_checkpoint_infer(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    request: CheckpointInferRequest,
) -> Result<contract::specta::OpaqueJson, AppError> {
    let checkpoint_id = request.checkpoint_id.clone();
    let family = request.family.clone();
    state
        .optimizers()
        .infer_saved_lora_with_delta(request, move |delta| {
            let _ = app.emit(
                crate::contract::events::EventChannel::OPTIMIZER_INFER,
                serde_json::json!({
                    "checkpointId": checkpoint_id,
                    "family": family,
                    "delta": delta,
                    "done": false
                }),
            );
        })
        .await
        .map(contract::specta::OpaqueJson)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_saved_lora_patch(
    state: State<'_, Arc<CoreRuntime>>,
    checkpoint_id: String,
    patch: SavedLoraPatchRequest,
) -> Result<SavedLoraCheckpoint, AppError> {
    state
        .optimizers()
        .patch_saved_lora(checkpoint_id, patch)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_saved_lora_publish(
    state: State<'_, Arc<CoreRuntime>>,
    checkpoint_id: String,
) -> Result<SavedLoraCheckpoint, AppError> {
    state
        .optimizers()
        .publish_saved_lora(checkpoint_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn optimizers_training_reconcile(
    state: State<'_, Arc<CoreRuntime>>,
    optimizer_run_id: String,
) -> Result<contract::specta::OpaqueJson, AppError> {
    state
        .optimizers()
        .reconcile_training(optimizer_run_id)
        .await
        .map(contract::specta::OpaqueJson)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn plugins_status(
    state: State<'_, Arc<CoreRuntime>>,
    plugin_id: Option<String>,
) -> Result<PluginStatus, AppError> {
    // Validate rather than discard: returning the optimizers status for any id
    // asked about let the caller believe a plugin existed that does not.
    if let Some(plugin_id) = plugin_id.as_deref() {
        if plugin_id == plugins::types::COMPUTER_USE_PLUGIN_ID {
            let _ = state.computer_use().refresh_grants().await;
            return Ok(state.computer_use().status().await);
        }
        if plugin_id != plugins::OPTIMIZERS_PLUGIN_ID {
            return Err(AppError::from(anyhow::anyhow!(
                "unknown plugin_id `{plugin_id}`"
            )));
        }
    }
    Ok(state.plugins().status(&state).await)
}

/// Human-triggered plugin lifecycle.
///
/// Delegates to the same `PluginService::manage` the agent-facing
/// `plugin_manage` MCP tool reaches over loopback IPC, so approval policy,
/// active-run guards, retention classes, and receipts are enforced once. Until
/// this existed an agent could install, update, disable, and remove the
/// Optimizers plugin while the UI had no way to do any of it.
#[tauri::command]
#[specta::specta]
async fn plugins_manage(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    approvals: State<'_, Arc<crate::session::approval::ApprovalBroker>>,
    operation: String,
    plugin_id: String,
    version: Option<String>,
    session_id: Option<String>,
) -> Result<contract::specta::OpaqueJson, AppError> {
    let arguments = serde_json::json!({
        "plugin_id": plugin_id,
        "version": version,
    });
    state
        .plugins()
        .manage(
            &state,
            approvals.inner(),
            &app,
            session_id.as_deref(),
            &operation,
            &arguments,
        )
        .await
        .map(contract::specta::OpaqueJson)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn plugins_list(state: State<'_, Arc<CoreRuntime>>) -> Result<Vec<PluginStatus>, AppError> {
    // Both managed plugins. The sidebar renders one row per entry in
    // PLUGIN_NAV and looks its status up by id, so a plugin missing here shows
    // no phase at all rather than showing the wrong one.
    let _ = state.computer_use().refresh_grants().await;
    Ok(vec![
        state.plugins().status(&state).await,
        state.computer_use().status().await,
    ])
}

/// What the Computer Use page renders.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseSnapshot {
    pub status: PluginStatus,
    /// Bundle identifiers this session may drive without a fresh card.
    pub allowed_apps: Vec<String>,
}

#[tauri::command]
#[specta::specta]
async fn computer_use_status(
    state: State<'_, Arc<CoreRuntime>>,
    session_id: Option<String>,
) -> Result<ComputerUseSnapshot, AppError> {
    let _ = state.computer_use().refresh_grants().await;
    let allowed_apps = match session_id.as_deref() {
        Some(session) => state.computer_use().allowlisted_apps(session).await,
        None => Vec::new(),
    };
    Ok(ComputerUseSnapshot {
        status: state.computer_use().status().await,
        allowed_apps,
    })
}

/// Install the helper that ships inside this app bundle.
///
/// The source is our own Resources directory rather than a download: the helper
/// is signed with the same identity as Workshop and shipping it separately
/// would mean a second thing to notarize, host, and keep in version step.
#[tauri::command]
#[specta::specta]
async fn computer_use_install(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<PluginStatus, AppError> {
    let source = bundled_helper_path().ok_or_else(|| {
        AppError::from(anyhow::anyhow!(
            "this build does not ship a Computer Use helper"
        ))
    })?;
    let team = crate::computer_use::helper::expected_team_id();
    let require_notarized = team.is_some();
    crate::computer_use::helper::install(
        &crate::computer_use::helper::SystemCommands,
        &source,
        &crate::computer_use::helper::helper_bundle_path(),
        team.as_deref(),
        require_notarized,
    )
    .map_err(AppError::from)?;
    let _ = state.computer_use().refresh_grants().await;
    Ok(state.computer_use().status().await)
}

#[tauri::command]
#[specta::specta]
async fn computer_use_remove(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<crate::computer_use::helper::RemovalReport, AppError> {
    state.computer_use().remove().await.map_err(AppError::from)
}

/// Revoke one app's standing permission to be driven.
#[tauri::command]
#[specta::specta]
async fn computer_use_revoke_app(
    state: State<'_, Arc<CoreRuntime>>,
    bundle_id: String,
) -> Result<u32, AppError> {
    state
        .computer_use()
        .allowlist()
        .revoke(&bundle_id)
        .map(|count| count as u32)
        .map_err(AppError::from)
}

/// Open the exact Privacy & Security pane for one grant.
#[tauri::command]
#[specta::specta]
async fn computer_use_open_settings(
    _state: State<'_, Arc<CoreRuntime>>,
    permission_id: String,
) -> Result<(), AppError> {
    let url = crate::computer_use::permissions::settings_url(&permission_id)
        .ok_or_else(|| AppError::from(anyhow::anyhow!("unknown permission `{permission_id}`")))?;
    // A probe can report a missing grant, but macOS does not add the helper to
    // Privacy & Security until the helper app itself requests access through a
    // LaunchServices identity. The long-lived MCP child is spawned as a raw
    // executable for stdio and is not sufficient for TCC registration.
    //
    // This route is human-only. Launch an app instance in `request` mode, then
    // open the exact pane where its newly registered row appears.
    let helper = crate::computer_use::helper::helper_bundle_path();
    let requested = std::process::Command::new("/usr/bin/open")
        .arg("-n")
        .arg(&helper)
        .args(["--args", "request"])
        .status()
        .map_err(|error| {
            AppError::from(anyhow::anyhow!(
                "could not request Computer Use permissions: {error}"
            ))
        })?;
    if !requested.success() {
        return Err(AppError::from(anyhow::anyhow!(
            "Computer Use permission request exited with {requested}"
        )));
    }
    std::process::Command::new("/usr/bin/open")
        .arg(url)
        .status()
        .map_err(|error| {
            AppError::from(anyhow::anyhow!("could not open System Settings: {error}"))
        })?;
    Ok(())
}

/// Read-only managed-browser preflight plus the human-owned origin policy.
#[tauri::command]
#[specta::specta]
async fn browser_runtime_status() -> Result<browser::BrowserRuntimeStatus, AppError> {
    Ok(browser::runtime_status())
}

/// Human-only origin approval. Browser MCP deliberately has no equivalent tool.
#[tauri::command]
#[specta::specta]
async fn browser_policy_allow_origin(
    origin: String,
) -> Result<browser::BrowserRuntimeStatus, AppError> {
    browser::allow_origin(&origin).map_err(AppError::from)?;
    Ok(browser::runtime_status())
}

/// Revoke a persistent origin approval for future navigations.
#[tauri::command]
#[specta::specta]
async fn browser_policy_revoke_origin(
    origin: String,
) -> Result<browser::BrowserRuntimeStatus, AppError> {
    browser::revoke_origin(&origin).map_err(AppError::from)?;
    Ok(browser::runtime_status())
}

/// The helper bundle shipped inside this application.
fn bundled_helper_path() -> Option<std::path::PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let contents = executable.parent()?.parent()?;
    let candidate = contents
        .join("Resources")
        .join(crate::computer_use::helper::HELPER_BUNDLE_NAME);
    if candidate.exists() {
        return Some(candidate);
    }
    // Development layout: the helper is built into its own target directory and
    // has not been staged into an app bundle yet.
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../helpers/synth-computer-use/target/bundle")
        .join(crate::computer_use::helper::HELPER_BUNDLE_NAME);
    repo.exists().then_some(repo)
}

#[tauri::command]
#[specta::specta]
async fn plugins_set_release_channel(
    state: State<'_, Arc<CoreRuntime>>,
    plugin_id: String,
    channel: String,
) -> Result<PluginStatus, AppError> {
    if plugin_id != plugins::OPTIMIZERS_PLUGIN_ID {
        return Err(AppError::from(anyhow::anyhow!(
            "unknown plugin_id `{plugin_id}`"
        )));
    }
    state
        .plugins()
        .registry()
        .set_release_channel(&channel)
        .map_err(AppError::from)?;
    Ok(state.plugins().status(&state).await)
}

/// One structured diagnostic from the renderer.
///
/// The renderer is the only surface whose failures were previously invisible
/// to everything else — a `console.error` in a webview reaches no journal, no
/// index, and no agent. This is the narrow command that ends that: it carries
/// the same envelope every other emitter uses, and the backend validates,
/// redacts, and correlates it exactly as if it had originated in Rust.
#[derive(Clone, Debug, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct DiagnosticReportRequest {
    severity: String,
    component: String,
    event: String,
    code: String,
    message: String,
    #[serde(default)]
    retryable: bool,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    turn_id: Option<String>,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    command_id: Option<String>,
    #[serde(default)]
    visual_id: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    visual_revision: Option<i64>,
    #[serde(default)]
    container_id: Option<String>,
    #[serde(default)]
    rollout_id: Option<String>,
    #[serde(default)]
    stream_id: Option<String>,
    #[serde(default)]
    optimizer_run_id: Option<String>,
    #[serde(default)]
    trace_id: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    details: Option<serde_json::Value>,
}

impl From<DiagnosticReportRequest> for diagnostics::DiagnosticInput {
    fn from(request: DiagnosticReportRequest) -> Self {
        let mut input = diagnostics::DiagnosticInput {
            severity: request.severity,
            component: request.component,
            event: request.event,
            code: request.code,
            message: request.message,
            retryable: request.retryable,
            ..Default::default()
        };
        input.correlation.session_id = request.session_id;
        input.correlation.turn_id = request.turn_id;
        input.correlation.tool_call_id = request.tool_call_id;
        input.correlation.command_id = request.command_id;
        input.correlation.visual_id = request.visual_id;
        input.correlation.visual_revision = request.visual_revision;
        input.correlation.container_id = request.container_id;
        input.correlation.rollout_id = request.rollout_id;
        input.correlation.stream_id = request.stream_id;
        input.correlation.optimizer_run_id = request.optimizer_run_id;
        input.correlation.trace_id = request.trace_id;
        if let Some(serde_json::Value::Object(details)) = request.details {
            input.details = details;
        }
        input
    }
}

/// Record a renderer diagnostic. Returns as soon as it is queued.
#[tauri::command]
#[specta::specta]
async fn diagnostics_report(
    state: State<'_, Arc<CoreRuntime>>,
    request: DiagnosticReportRequest,
) -> Result<(), AppError> {
    state
        .diagnostics_service()
        .emit(diagnostics::DiagnosticInput::from(request));
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn diagnostics_status(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<contract::specta::OpaqueJson, AppError> {
    Ok(contract::specta::OpaqueJson(
        state.diagnostics_service().status().await,
    ))
}

/// Typed diagnostic query for the Diagnostics pane. The renderer is bounded by
/// the same contract as the agent — there is only one query path.
#[tauri::command]
#[specta::specta]
async fn diagnostics_query(
    state: State<'_, Arc<CoreRuntime>>,
    request: contract::specta::OpaqueJson,
) -> Result<contract::specta::OpaqueJson, AppError> {
    let query = diagnostics::query::parse(&request.0).map_err(AppError::from)?;
    state
        .diagnostics_service()
        .query(query)
        .await
        .map(contract::specta::OpaqueJson)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn diagnostics_explain(
    state: State<'_, Arc<CoreRuntime>>,
    request: contract::specta::OpaqueJson,
) -> Result<contract::specta::OpaqueJson, AppError> {
    let query = diagnostics::query::parse(&request.0).map_err(AppError::from)?;
    state
        .diagnostics_service()
        .explain(query)
        .await
        .map(contract::specta::OpaqueJson)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn diagnostics_bundle(
    state: State<'_, Arc<CoreRuntime>>,
    request: contract::specta::OpaqueJson,
) -> Result<contract::specta::OpaqueJson, AppError> {
    let query = diagnostics::query::parse(&request.0).map_err(AppError::from)?;
    state
        .diagnostics_service()
        .bundle(query)
        .await
        .map(contract::specta::OpaqueJson)
        .map_err(AppError::from)
}

/// Drop the disposable index. Traces, run evidence, and the authoritative
/// journal are untouched.
#[tauri::command]
#[specta::specta]
async fn diagnostics_clear_index(
    state: State<'_, Arc<CoreRuntime>>,
) -> Result<contract::specta::OpaqueJson, AppError> {
    state
        .diagnostics_service()
        .clear_index()
        .await
        .map(contract::specta::OpaqueJson)
        .map_err(AppError::from)
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

#[derive(Clone, Debug, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct VisualStreamPollRequest {
    visual_id: String,
    poll_url: String,
    after: u32,
    limit: u16,
}

/// Fetch a visual's persisted, declaration-validated poll authority through
/// the native process. WKWebView cannot reliably read loopback HTTP because
/// its CORS/CSP boundary differs from the backend's; this command is narrowly
/// scoped to exact URLs already stored on the named visual.
#[tauri::command]
#[specta::specta]
async fn visual_stream_poll(
    state: State<'_, Arc<CoreRuntime>>,
    request: VisualStreamPollRequest,
) -> Result<contract::specta::OpaqueJson, AppError> {
    let visual = state
        .visuals()
        .get(request.visual_id)
        .await
        .map_err(AppError::from)?;
    // One authority decides what this visual declared. Reading the raw
    // `slots` key here is what let a visual hold ten poll URLs the poll
    // command would have rejected.
    let declared_urls = visuals::declared_poll_urls(&visual.bindings);
    let declared = declared_urls
        .iter()
        .any(|url| url == request.poll_url.as_str());
    // Every renderer poll of a live stream lands here, so this is where a live
    // stream going quiet becomes a record rather than an empty pane.
    let diagnose_at = |severity: diagnostics::Severity,
                       event: &str,
                       code: &str,
                       message: String,
                       retryable: bool,
                       details: serde_json::Value| {
        let mut input =
            diagnostics::DiagnosticInput::new(severity, "container-stream", event, code, message)
                .retryable(retryable);
        input.correlation.visual_id = Some(visual.id.clone());
        input.correlation.visual_revision = Some(visual.current_revision);
        input.correlation.session_id = visual.session_id.clone();
        input.correlation.rollout_id = visual.run_id.clone();
        input.correlation.trace_id = visual.trace_id.clone();
        if let Some(object) = details.as_object() {
            input.details = object.clone();
        }
        state.diagnostics_service().emit(input);
    };
    let fail = |code: &str, message: String, retryable: bool, details: serde_json::Value| {
        diagnose_at(
            diagnostics::Severity::Error,
            "stream.poll.failed",
            code,
            message,
            retryable,
            details,
        );
    };
    if !declared {
        // An undeclared URL is a binding defect, not a transport failure: the
        // visual is asking for a stream it never declared.
        fail(
            diagnostics::codes::VISUAL_BINDING_UNRESOLVED,
            "visual stream poll URL is not declared on this visual".into(),
            false,
            serde_json::json!({"declared_stream_count": declared_urls.len()}),
        );
        return Err(AppError::from(anyhow::anyhow!(
            "visual stream poll URL is not declared on this visual; \
             the visual declares {} live stream(s)",
            declared_urls.len()
        )));
    }
    let limit = request.limit.clamp(1, 500);
    let started = std::time::Instant::now();
    let response = async {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?
            .get(&request.poll_url)
            .query(&[
                ("after", request.after.to_string()),
                ("limit", limit.to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await
    }
    .await;
    match response {
        Ok(page) => {
            // A successful poll has to be as visible as a failing one. Without
            // this, "the renderer never asked" and "the stream returned
            // nothing" are the same empty pane and the same empty query.
            let cursor = page.get("cursor");
            let rows = page
                .get("events")
                .or_else(|| page.pointer("/page/events"))
                .and_then(serde_json::Value::as_array)
                .map(Vec::len);
            let closed = cursor
                .and_then(|cursor| cursor.get("closed"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            diagnose_at(
                diagnostics::Severity::Debug,
                if closed {
                    "stream.poll.closed"
                } else {
                    "stream.poll.page"
                },
                diagnostics::codes::STREAM_POLL_OBSERVED,
                format!(
                    "polled {} row(s) after sequence {}",
                    rows.unwrap_or(0),
                    request.after
                ),
                false,
                // Identifiers and counts only. Envelope bodies never enter a
                // diagnostic: they carry model output and rollout payloads.
                serde_json::json!({
                    "after": request.after,
                    "limit": limit,
                    "row_count": rows,
                    "next": cursor.and_then(|cursor| cursor.get("next")),
                    "high_water": cursor.and_then(|cursor| cursor.get("high_water")),
                    "closed": closed,
                    "duration_ms": started.elapsed().as_millis() as u64,
                }),
            );
            Ok(contract::specta::OpaqueJson(page))
        }
        Err(error) => {
            let status = error.status().map(|status| status.as_u16());
            fail(
                diagnostics::codes::STREAM_INTERRUPTED,
                error.to_string(),
                // A refused or 5xx poll may recover; a 4xx says the stream is
                // gone and retrying only repeats the question.
                error.is_timeout() || error.is_connect() || status.is_none_or(|code| code >= 500),
                serde_json::json!({
                    "status": status,
                    "after": request.after,
                    "duration_ms": started.elapsed().as_millis() as u64,
                }),
            );
            Err(AppError::from(error))
        }
    }
}

/// `synth.visual.media.v1` — the host-mediated binary bridge.
///
/// The `local_cas` binding decodes a CAS object as JSON, which is the right
/// thing for a chart spec and useless for a PNG. Rather than teach that binding
/// about binary — and rather than send an entire timeline of base64 frames into
/// a pane on every update — a visual asks for one digest at a time and the host
/// answers only for media the bound run actually produced.
pub const VISUAL_MEDIA_PROTOCOL: &str = "synth.visual.media.v1";

/// Ceiling on one bridged media response.
///
/// A frame is a screen-sized image. Anything larger is a producer defect or a
/// mis-typed digest, and returning it would put tens of megabytes of base64
/// through the IPC boundary to render one tile.
const VISUAL_MEDIA_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Media types a pane is allowed to be handed. An allowlist, not a denylist:
/// the store also holds trace archives and accessibility trees, and none of
/// them should ever reach a renderer through an image request.
const VISUAL_MEDIA_ALLOWED_TYPES: &[&str] = &["image/png"];

#[derive(Clone, Debug, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct VisualMediaReadRequest {
    visual_id: String,
    /// Workshop's own SHA-256, as it appears in `containerEvent.payload.media`.
    cas_digest: String,
}

#[tauri::command]
#[specta::specta]
async fn visual_media_read(
    state: State<'_, Arc<CoreRuntime>>,
    request: VisualMediaReadRequest,
) -> Result<contract::specta::OpaqueJson, AppError> {
    let visual = state
        .visuals()
        .get(request.visual_id.clone())
        .await
        .map_err(AppError::from)?;
    // The runs this visual *declares*, plus the run it was minted for. A pane
    // does not get to name a run: it gets the ones its bindings already say it
    // is showing.
    let mut candidates = visuals::declared_optimizer_run_ids(&visual.bindings);
    if let Some(run_id) = visual.run_id.clone() {
        if !candidates.contains(&run_id) {
            candidates.push(run_id);
        }
    }
    if candidates.is_empty() {
        return Err(AppError::from(anyhow::anyhow!(
            "visual {} is bound to no optimizer run, so it can be granted no run media",
            visual.id
        )));
    }
    let optimizers = state.optimizers();
    let mut granted = None;
    for run_id in &candidates {
        match optimizers
            .granted_run_media(run_id, &request.cas_digest)
            .await
        {
            Ok(Some(found)) => {
                granted = Some(found);
                break;
            }
            Ok(None) => {}
            // A malformed digest is the same answer for every candidate run;
            // reporting it once is clearer than repeating it per run.
            Err(error) => return Err(AppError::from(error)),
        }
    }
    let Some(granted) = granted else {
        return Err(AppError::from(anyhow::anyhow!(
            "media {} was not produced by any run this visual is bound to",
            request.cas_digest
        )));
    };
    if !VISUAL_MEDIA_ALLOWED_TYPES.contains(&granted.media_type.as_str()) {
        return Err(AppError::from(anyhow::anyhow!(
            "media type {} is not servable to a visual",
            granted.media_type
        )));
    }
    if granted.byte_size > VISUAL_MEDIA_MAX_BYTES {
        return Err(AppError::from(anyhow::anyhow!(
            "media {} is {} bytes, over the {VISUAL_MEDIA_MAX_BYTES}-byte bridge ceiling",
            granted.cas_digest,
            granted.byte_size
        )));
    }
    // `get_bytes` re-verifies the stored bytes against the digest, so a
    // corrupted object fails here rather than rendering as a broken tile.
    let bytes = optimizers
        .read_media_bytes(&granted)
        .map_err(AppError::from)?;
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(contract::specta::OpaqueJson(serde_json::json!({
        "protocol": VISUAL_MEDIA_PROTOCOL,
        "casDigest": granted.cas_digest,
        "mediaType": granted.media_type,
        "byteSize": granted.byte_size,
        "width": granted.width,
        "height": granted.height,
        "rolloutId": granted.rollout_id,
        "step": granted.step,
        "optimizerRunId": granted.optimizer_run_id,
        "dataUrl": format!("data:{};base64,{encoded}", granted.media_type),
    })))
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
fn visuals_observation_report(
    observation: visuals_ipc::RenderedVisualObservation,
) -> Result<(), AppError> {
    visuals_ipc::record_rendered_observation(observation).map_err(AppError::from)
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
async fn visuals_annotations_list(
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
) -> Result<Vec<VisualAnnotation>, AppError> {
    state
        .visuals()
        .annotations(visual_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn visuals_annotation_create(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
    request: VisualAnnotationCreate,
) -> Result<VisualAnnotation, AppError> {
    let (annotation, event) = state
        .visuals()
        .create_annotation(visual_id, request)
        .await
        .map_err(AppError::from)?;
    publish_visual_event(&app, &state, event).await?;
    Ok(annotation)
}

#[tauri::command]
#[specta::specta]
async fn visuals_seals_list(
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: Option<String>,
) -> Result<Vec<VisualSeal>, AppError> {
    state
        .visuals()
        .list_seals(visual_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn visuals_seal(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    visual_id: String,
    revision: contract::specta::OpaqueInteger<i64>,
) -> Result<VisualSeal, AppError> {
    let (seal, event) = state
        .visuals()
        .seal(visual_id, revision.0)
        .await
        .map_err(AppError::from)?;
    publish_visual_event(&app, &state, event).await?;
    Ok(seal)
}

#[tauri::command]
#[specta::specta]
async fn visuals_seal_get(
    state: State<'_, Arc<CoreRuntime>>,
    receipt_digest: String,
) -> Result<VisualSealBundle, AppError> {
    state
        .visuals()
        .get_seal(receipt_digest)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn visuals_upload_status(
    state: State<'_, Arc<CoreRuntime>>,
    receipt_digest: String,
) -> Result<Option<VisualUpload>, AppError> {
    state
        .visuals()
        .upload_status(receipt_digest)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn visuals_share_seal(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    receipt_digest: String,
) -> Result<VisualUpload, AppError> {
    let backend = synth_config::resolve().map_err(AppError::from)?;
    let api_key = backend.api_key.ok_or_else(|| {
        AppError::from(anyhow::anyhow!("Share requires a signed-in Synth account"))
    })?;
    let (upload, event) = state
        .visuals()
        .share_seal(receipt_digest, backend.backend_url, api_key)
        .await
        .map_err(AppError::from)?;
    if event.get("schemaVersion").is_some() {
        publish_visual_event(&app, &state, event).await?;
    }
    Ok(upload)
}

#[tauri::command]
#[specta::specta]
async fn visuals_open_shared(
    state: State<'_, Arc<CoreRuntime>>,
    committed_url: String,
) -> Result<VisualSealBundle, AppError> {
    let backend = synth_config::resolve().map_err(AppError::from)?;
    let api_key = backend.api_key.ok_or_else(|| {
        AppError::from(anyhow::anyhow!(
            "Opening a private shared visual requires a signed-in Synth account"
        ))
    })?;
    state
        .visuals()
        .open_shared_url(committed_url, backend.backend_url, api_key)
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
    crate::telemetry::mark_once(
        "first_experiment_visual",
        serde_json::json!({"workflow_family": "visual"}),
    );
    crate::telemetry::emit(
        "artifact_created",
        serde_json::json!({"workflow_family": "visual"}),
    );
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
async fn reports_list(
    state: State<'_, Arc<CoreRuntime>>,
    query: Option<ReportQuery>,
) -> Result<Vec<ReportRecord>, AppError> {
    state
        .reports()
        .list(query.unwrap_or_default())
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_get(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
) -> Result<ReportRecord, AppError> {
    state.reports().get(report_id).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_revision_get(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
    revision: Option<contract::specta::OpaqueInteger<i64>>,
) -> Result<ReportRevision, AppError> {
    state
        .reports()
        .get_revision(report_id, revision.map(|value| value.0))
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_validate(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
    revision: Option<contract::specta::OpaqueInteger<i64>>,
) -> Result<reports::ReportValidationResult, AppError> {
    state
        .reports()
        .validate(report_id, revision.map(|value| value.0))
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_pin_all(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
) -> Result<ReportRecord, AppError> {
    let (report, event) = state
        .reports()
        .pin_all(report_id)
        .await
        .map_err(AppError::from)?;
    publish_visual_event(&app, &state, event).await?;
    Ok(report)
}

#[tauri::command]
#[specta::specta]
async fn reports_create(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    request: ReportCreateRequest,
) -> Result<ReportRecord, AppError> {
    let (report, event) = state
        .reports()
        .create(request)
        .await
        .map_err(AppError::from)?;
    publish_visual_event(&app, &state, event).await?;
    crate::telemetry::emit("report_created", serde_json::json!({"outcome": "success"}));
    crate::telemetry::emit(
        "artifact_created",
        serde_json::json!({"workflow_family": "report"}),
    );
    Ok(report)
}

#[tauri::command]
#[specta::specta]
async fn reports_update(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
    request: ReportUpdateRequest,
) -> Result<ReportRecord, AppError> {
    let (report, event) = state
        .reports()
        .update(report_id, request)
        .await
        .map_err(AppError::from)?;
    publish_visual_event(&app, &state, event).await?;
    Ok(report)
}

#[tauri::command]
#[specta::specta]
async fn reports_archive(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
) -> Result<ReportRecord, AppError> {
    state
        .reports()
        .set_archived(report_id, true)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_restore(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
) -> Result<ReportRecord, AppError> {
    state
        .reports()
        .set_archived(report_id, false)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_visibility_requests(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: Option<String>,
) -> Result<Vec<ReportVisibilityRequest>, AppError> {
    state
        .reports()
        .list_visibility_requests(report_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_visibility_request(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
    request: ReportVisibilityRequestCreate,
) -> Result<ReportVisibilityRequest, AppError> {
    state
        .reports()
        .request_visibility(report_id, request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_visibility_decide(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    request_id: String,
    approved: bool,
) -> Result<ReportVisibilityRequest, AppError> {
    let decision = state
        .reports()
        .decide_visibility(request_id.clone(), approved, "human".into())
        .await
        .map_err(AppError::from)?;
    if !approved {
        return Ok(decision);
    }
    crate::telemetry::mark_once(
        "first_report_shared",
        serde_json::json!({"outcome": "success"}),
    );
    crate::telemetry::emit(
        "report_published",
        serde_json::json!({"outcome": "success"}),
    );
    let execution: anyhow::Result<()> = async {
        let backend = synth_config::resolve()?;
        let api_key = backend.api_key.ok_or_else(|| {
            anyhow::anyhow!("Approving Report visibility requires a signed-in Synth account")
        })?;
        match decision.target.as_str() {
            "private" => {
                let (_, event) = state
                    .reports()
                    .share_seal(
                        decision.receipt_digest.clone(),
                        backend.backend_url.clone(),
                        api_key.clone(),
                    )
                    .await?;
                if event.get("schemaVersion").is_some() {
                    publish_visual_event(&app, &state, event).await?;
                }
            }
            "public" => {
                let (upload, event) = state
                    .reports()
                    .share_seal(
                        decision.receipt_digest.clone(),
                        backend.backend_url.clone(),
                        api_key.clone(),
                    )
                    .await?;
                if event.get("schemaVersion").is_some() {
                    publish_visual_event(&app, &state, event).await?;
                }
                state
                    .reports()
                    .promote_publication(
                        upload.publication_id.ok_or_else(|| {
                            anyhow::anyhow!("committed upload has no publication")
                        })?,
                        decision
                            .slug
                            .clone()
                            .ok_or_else(|| anyhow::anyhow!("public request has no slug"))?,
                        backend.backend_url.clone(),
                        api_key.clone(),
                        Some(decision.request_id.clone()),
                        decision.reason.clone(),
                    )
                    .await?;
            }
            "unpublished" => {
                let upload = state
                    .reports()
                    .upload_status(decision.receipt_digest.clone())
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("sealed Report has not been shared"))?;
                state
                    .reports()
                    .unpublish_publication(
                        upload
                            .publication_id
                            .ok_or_else(|| anyhow::anyhow!("shared Report has no publication"))?,
                        backend.backend_url.clone(),
                        api_key.clone(),
                        decision.reason.clone(),
                    )
                    .await?;
            }
            _ => anyhow::bail!("unsupported visibility target"),
        }
        Ok(())
    }
    .await;
    let error = execution.as_ref().err().map(ToString::to_string);
    let finished = state
        .reports()
        .finish_visibility(request_id, error)
        .await
        .map_err(AppError::from)?;
    execution.map_err(AppError::from)?;
    Ok(finished)
}

#[tauri::command]
#[specta::specta]
async fn reports_seal(
    app: tauri::AppHandle,
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
    revision: contract::specta::OpaqueInteger<i64>,
) -> Result<ReportSeal, AppError> {
    let (seal, event) = state
        .reports()
        .seal(report_id, revision.0)
        .await
        .map_err(AppError::from)?;
    if event.get("schemaVersion").is_some() {
        publish_visual_event(&app, &state, event).await?;
    }
    Ok(seal)
}

#[tauri::command]
#[specta::specta]
async fn reports_seals_list(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: Option<String>,
) -> Result<Vec<ReportSeal>, AppError> {
    state
        .reports()
        .list_seals(report_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_seal_get(
    state: State<'_, Arc<CoreRuntime>>,
    receipt_digest: String,
) -> Result<ReportSealBundle, AppError> {
    state
        .reports()
        .get_seal(receipt_digest)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_seals_compare(
    state: State<'_, Arc<CoreRuntime>>,
    left_digest: String,
    right_digest: String,
) -> Result<ReportRevisionCompare, AppError> {
    state
        .reports()
        .compare_seals(left_digest, right_digest)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_experiments_list(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
) -> Result<Vec<ExperimentRecord>, AppError> {
    state
        .reports()
        .list_experiments(report_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn experiments_list(
    state: State<'_, Arc<CoreRuntime>>,
    query: Option<String>,
) -> Result<Vec<ExperimentGroup>, AppError> {
    state
        .data()
        .experiments_list(query)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn experiments_get(
    state: State<'_, Arc<CoreRuntime>>,
    experiment_id: String,
) -> Result<Option<ExperimentGroup>, AppError> {
    state
        .data()
        .experiment_get(experiment_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn experiments_attach_evidence(
    state: State<'_, Arc<CoreRuntime>>,
    request: ExperimentEvidenceAttachRequest,
) -> Result<ExperimentGroup, AppError> {
    state
        .data()
        .experiment_attach_evidence(request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn experiments_create(
    state: State<'_, Arc<CoreRuntime>>,
    request: ExperimentCreateRequest,
) -> Result<ExperimentGroup, AppError> {
    state
        .data()
        .experiment_create(request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn experiments_create_child(
    state: State<'_, Arc<CoreRuntime>>,
    request: ExperimentChildCreateRequest,
) -> Result<ExperimentGroup, AppError> {
    state
        .data()
        .experiment_create_child(request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn experiments_relate(
    state: State<'_, Arc<CoreRuntime>>,
    request: ExperimentRelateRequest,
) -> Result<ExperimentGroup, AppError> {
    state
        .data()
        .experiment_relate(request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn experiments_activate(
    state: State<'_, Arc<CoreRuntime>>,
    session_id: String,
    experiment_id: String,
) -> Result<ExperimentGroup, AppError> {
    state
        .data()
        .experiment_activate(session_id, experiment_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn experiments_finalize(
    state: State<'_, Arc<CoreRuntime>>,
    request: ExperimentFinalizeRequest,
) -> Result<ExperimentGroup, AppError> {
    state
        .data()
        .experiment_finalize(request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_experiment_upsert(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
    request: ExperimentRecordUpsert,
) -> Result<ExperimentRecord, AppError> {
    state
        .reports()
        .upsert_experiment(report_id, request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_log_list(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
) -> Result<Vec<ResearchLogEntry>, AppError> {
    state
        .reports()
        .list_research_log(report_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_log_append(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
    request: ResearchLogAppend,
) -> Result<ResearchLogEntry, AppError> {
    state
        .reports()
        .append_research_log(report_id, request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_upload_status(
    state: State<'_, Arc<CoreRuntime>>,
    receipt_digest: String,
) -> Result<Option<ReportUpload>, AppError> {
    state
        .reports()
        .upload_status(receipt_digest)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_share(
    _app: tauri::AppHandle,
    _state: State<'_, Arc<CoreRuntime>>,
    _receipt_digest: String,
) -> Result<ReportUpload, AppError> {
    Err(AppError::untyped(
        "Direct Report sharing is disabled; create and approve a revision-bound visibility request",
    ))
}

#[tauri::command]
#[specta::specta]
async fn reports_audience_set(
    state: State<'_, Arc<CoreRuntime>>,
    publication_id: String,
    request: ReportAudienceRequest,
) -> Result<ReportAudienceState, AppError> {
    let backend = synth_config::resolve().map_err(AppError::from)?;
    let api_key = backend
        .api_key
        .ok_or_else(|| AppError::untyped("sharing a Report requires a signed-in Synth account"))?;
    state
        .reports()
        .set_audience(publication_id, request, backend.backend_url, api_key)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_audience_revoke(
    state: State<'_, Arc<CoreRuntime>>,
    publication_id: String,
    receipt_digest: String,
) -> Result<ReportAudienceState, AppError> {
    let backend = synth_config::resolve().map_err(AppError::from)?;
    let api_key = backend.api_key.ok_or_else(|| {
        AppError::untyped("revoking Report access requires a signed-in Synth account")
    })?;
    state
        .reports()
        .revoke_audience(publication_id, receipt_digest, backend.backend_url, api_key)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_promote(
    _state: State<'_, Arc<CoreRuntime>>,
    _publication_id: String,
    _slug: String,
) -> Result<reports::ReportPromotion, AppError> {
    Err(AppError::untyped(
        "Direct Report promotion is disabled; create and approve a revision-bound public visibility request",
    ))
}

#[tauri::command]
#[specta::specta]
async fn reports_open_shared(
    state: State<'_, Arc<CoreRuntime>>,
    committed_url: String,
) -> Result<ReportSealBundle, AppError> {
    let backend = synth_config::resolve().map_err(AppError::from)?;
    let api_key = backend.api_key.ok_or_else(|| {
        AppError::from(anyhow::anyhow!(
            "opening a private shared Report requires a signed-in Synth account"
        ))
    })?;
    state
        .reports()
        .open_shared_url(committed_url, backend.backend_url, api_key)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_comments_list(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
    revision: Option<contract::specta::OpaqueInteger<i64>>,
) -> Result<Vec<ReportComment>, AppError> {
    state
        .reports()
        .list_comments(report_id, revision.map(|value| value.0))
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn reports_comment_create(
    state: State<'_, Arc<CoreRuntime>>,
    report_id: String,
    revision: contract::specta::OpaqueInteger<i64>,
    request: ReportCommentCreate,
) -> Result<ReportComment, AppError> {
    state
        .reports()
        .create_comment(report_id, revision.0, request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
fn synth_config_get() -> Result<BackendSettings, AppError> {
    synth_config::get().map_err(AppError::from)
}

/// The startup-safe catalog path: configuration plus a persisted public
/// OpenRouter metadata snapshot only. It deliberately never waits for network.
#[tauri::command]
#[specta::specta]
fn model_catalog_get() -> Result<model_catalog::ModelCatalog, AppError> {
    model_catalog::catalog().map_err(AppError::from)
}

/// Explicit background follow-up used after the picker has rendered. OpenRouter
/// metadata is public; no credential is sent to or exposed by this command.
#[tauri::command]
#[specta::specta]
async fn model_catalog_refresh() -> Result<model_catalog::ModelCatalog, AppError> {
    model_catalog::refresh().await.map_err(AppError::from)
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ModelPerformanceMetric {
    model_id: String,
    provider: String,
    #[specta(type = specta_typescript::Number)]
    sample_count: u64,
    #[specta(type = specta_typescript::Number)]
    input_tokens: u64,
    #[specta(type = specta_typescript::Number)]
    cached_input_tokens: u64,
    #[specta(type = specta_typescript::Number)]
    output_tokens: u64,
    #[specta(type = specta_typescript::Number)]
    total_tokens: u64,
    output_tps_p50: f64,
    output_tps_p95: f64,
    total_tpm_p50: f64,
    total_tpm_p95: f64,
    latency_ms_p50: f64,
    latency_ms_p95: f64,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
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
        .ok_or_else(|| AppError::untyped("Sign in to read Synth Cloud model telemetry"))?;
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
                AppError::untyped(
                    "Synth Cloud telemetry could not be reached. Check Account → Synth backend URL.",
                )
            } else {
                AppError::untyped(format!("Synth Cloud telemetry request failed: {error}"))
            }
        })?;
    let status = response.status();
    if !status.is_success() {
        let detail = response.text().await.unwrap_or_default();
        return Err(AppError::untyped(format!(
            "Synth Cloud telemetry returned {status}: {}",
            detail.chars().take(240).collect::<String>()
        )));
    }
    response
        .json::<ModelPerformanceSnapshot>()
        .await
        .map_err(|error| {
            AppError::untyped(format!("Invalid Synth Cloud telemetry response: {error}"))
        })
}

#[tauri::command]
#[specta::specta]
async fn synth_config_update(
    core: State<'_, Arc<CoreRuntime>>,
    cloud: State<'_, Arc<account_cloud::AccountCloudClient>>,
    request: BackendSettingsUpdate,
) -> Result<BackendSettings, AppError> {
    let api_key_updated = request.api_key.is_some();
    let settings = synth_config::update(request).map_err(AppError::from)?;
    core.reload_intern_config().await.map_err(AppError::from)?;
    if api_key_updated {
        cloud.clear_cache();
        let _ = account::mark_paired(core.storage(), chrono::Utc::now());
    }
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
async fn codex_oauth_status(
    manager: State<'_, Arc<codex_oauth::Manager>>,
) -> Result<codex_oauth::Status, AppError> {
    let manager = manager.inner().clone();
    tokio::task::spawn_blocking(move || manager.status())
        .await
        .map_err(|error| {
            AppError::from(anyhow::anyhow!("ChatGPT credential check failed: {error}"))
        })?
        .map_err(AppError::from)
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
    // Credential deletion and child fencing share one lifecycle boundary with
    // attachment starts. Deleting first prevents a new OAuth start, while the
    // write guard prevents an already-authorized start from appearing after
    // the fence snapshot. Conversations remain durable and can be rebound.
    codex
        .fence_provider_attachments_after(codex_oauth::PROVIDER_ID, manager.disconnect())
        .await
        .map_err(AppError::from)
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
        crate::telemetry::mark_once(
            "signup_completed",
            serde_json::json!({"outcome": "success"}),
        );
        crate::telemetry::emit(
            "signin_completed",
            serde_json::json!({"outcome": "success"}),
        );
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
        settings.api_key_configured && !read.unauthenticated,
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
    let url = account_cloud::validate_billing_url(
        &url,
        &resolved.backend_url,
        synth_config::development_routing_enabled(),
    )
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
    // device ledger stay untouched. Optional analytics drop; the install id
    // and essential recovery events remain until retention expires.
    cloud.clear_cache();
    if let Some(telemetry) = crate::telemetry::live() {
        if let Err(error) = telemetry.on_sign_out() {
            crate::platform::logging::report("lib", "eprintln", format!("synth-desktop: sign-out telemetry wipe failed: {error}"));
        }
    }
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
async fn laguna_register_policy(
    core: State<'_, Arc<CoreRuntime>>,
    state: State<'_, Arc<LagunaManager>>,
    checkpoint_id: String,
    model_id: String,
) -> Result<laguna::LagunaPolicy, AppError> {
    let checkpoint = core
        .optimizers()
        .get_local_lora(checkpoint_id.clone())
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::from(anyhow::anyhow!("adapter is not in the catalog")))?;
    if !crate::optimizers::local_lora_is_laguna_compatible(&checkpoint) {
        return Err(AppError::from(anyhow::anyhow!(
            "this adapter is not Laguna-compatible; Qwen Optimizers LoRAs stay on the catalog Chat Completions / Responses buttons"
        )));
    }
    let path = std::path::PathBuf::from(&checkpoint.storage.key);
    if !path.is_dir() {
        return Err(AppError::from(anyhow::anyhow!(
            "this adapter's bytes are missing at {}",
            path.display()
        )));
    }
    state
        .register_policy(&model_id, &path, checkpoint.storage.sha256.as_deref())
        .await
        .map_err(AppError::from)
}

/// What the Settings surface renders for the published finetune.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LagunaAdapterStatus {
    pub model_id: String,
    pub title: String,
    pub digest: String,
    pub installed: bool,
    #[specta(type = specta_typescript::Number)]
    pub download_bytes: u64,
    pub base_revision: String,
    /// False when the installed weights are a different revision. The adapter
    /// is shown either way; only the action is refused.
    pub base_matches: bool,
}

#[tauri::command]
#[specta::specta]
async fn laguna_adapter_status() -> Result<Vec<LagunaAdapterStatus>, AppError> {
    Ok(laguna_adapters::ADAPTER_CATALOG
        .iter()
        .map(|spec| LagunaAdapterStatus {
            model_id: spec.model_id.into(),
            title: spec.title.into(),
            digest: spec.digest.into(),
            installed: laguna_adapters::is_installed(spec),
            download_bytes: spec.download_bytes,
            base_revision: spec.base_revision.into(),
            base_matches: spec.base_revision == laguna::installed_base_revision(),
        })
        .collect())
}

#[tauri::command]
#[specta::specta]
async fn laguna_adapter_download(
    app: tauri::AppHandle,
    core: State<'_, Arc<CoreRuntime>>,
    state: State<'_, Arc<LagunaManager>>,
    model_id: String,
) -> Result<LagunaAdapterStatus, AppError> {
    let spec = laguna_adapters::adapter_spec(&model_id).map_err(AppError::from)?;
    // Through the backend, with the account's own credential. No adapter
    // object is public and Workshop never reaches object storage directly.
    let client =
        crate::optimizers::cloud::CloudOptimizerClient::from_config().map_err(AppError::from)?;
    let emit = |phase: &str, detail: &str, done: u64, total: u64| {
        let _ = app.emit(
            crate::contract::events::EventChannel::LAGUNA_DOWNLOAD,
            serde_json::json!({
                "phase": phase,
                "detail": detail,
                "modelId": spec.model_id,
                "downloadedBytes": done,
                "totalBytes": total,
            }),
        );
    };

    emit(
        "preparing",
        "Reading the adapter manifest…",
        0,
        spec.download_bytes,
    );
    let manifest_json = client
        .adapter_manifest(spec.digest)
        .await
        .map_err(AppError::from)?;
    let manifest =
        laguna_adapters::parse_manifest(&manifest_json.to_string()).map_err(AppError::from)?;
    laguna_adapters::check_pinned(&spec, &manifest).map_err(AppError::from)?;
    laguna_adapters::check_base_revision(&manifest, laguna::installed_base_revision())
        .map_err(AppError::from)?;

    let total: u64 = manifest.files.iter().map(|file| file.bytes).sum();
    let mut fetched: Vec<(String, Vec<u8>)> = Vec::new();
    let mut done = 0u64;
    for file in &manifest.files {
        emit(
            "downloading",
            &format!("Downloading {}…", file.path),
            done,
            total,
        );
        let bytes = client
            .adapter_file(spec.digest, &file.path)
            .await
            .map_err(AppError::from)?;
        done += bytes.len() as u64;
        fetched.push((file.path.clone(), bytes));
    }
    emit("verifying", "Verifying the adapter…", done, total);
    let staged =
        laguna_adapters::stage_verified(&spec, &manifest, &fetched).map_err(AppError::from)?;

    // Install through the catalog's own import so a downloaded adapter and a
    // hand-imported one are the same row, digested by the same code.
    let imported = core
        .optimizers()
        .import_saved_lora_dir(staged.display().to_string())
        .await
        .map_err(AppError::from)?;
    let _ = std::fs::remove_dir_all(&staged);
    let install = laguna_adapters::install_dir(&manifest.digest);
    laguna_adapters::write_manifest_beside(&install, &manifest).map_err(AppError::from)?;
    if imported.checkpoint_id != manifest.digest {
        return Err(AppError::from(anyhow::anyhow!(
            "installed adapter is {} but the manifest published {}",
            imported.checkpoint_id,
            manifest.digest
        )));
    }
    state
        .register_policy(spec.model_id, &install, Some(manifest.digest.as_str()))
        .await
        .map_err(AppError::from)?;
    Ok(LagunaAdapterStatus {
        model_id: spec.model_id.into(),
        title: spec.title.into(),
        digest: spec.digest.into(),
        installed: true,
        download_bytes: spec.download_bytes,
        base_revision: spec.base_revision.into(),
        base_matches: true,
    })
}

#[tauri::command]
#[specta::specta]
async fn laguna_policies(
    state: State<'_, Arc<LagunaManager>>,
) -> Result<Vec<laguna::LagunaPolicy>, AppError> {
    state.policies().await.map_err(AppError::from)
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
        return Err(AppError::untyped("Read-only attachments are not yet supported by the macOS Codex sandbox; no access was granted"));
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
    // Scope is durable before the old process is fenced. Fencing preserves the
    // conversation; the next send re-attaches at the new revision. Closing it
    // durably would make the session terminal and refuse that next run.
    codex
        .fence_attachment(&session_id)
        .await
        .map_err(AppError::from)?;
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
    codex
        .fence_attachment(&session_id)
        .await
        .map_err(AppError::from)?;
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
    codex
        .fence_attachment(&session_id)
        .await
        .map_err(AppError::from)?;
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
        .fence_attachment(&scope.session_id)
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
                return Err(AppError::untyped(
                    "requested workspace does not match the conversation's persisted scope",
                ));
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
    request.local_model_catalog = None;
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
            // The daemon routes policies by the Responses `model` field. The
            // renderer carries the selected catalog id in `adapter` so it can
            // keep the base serving identity separate from policy metadata;
            // promote that id here instead of silently forcing every turn
            // back onto the configured base model. The catalog lookup below
            // remains the authority and rejects unknown policy ids.
            let model = request
                .adapter
                .clone()
                .unwrap_or(laguna.configured_model_id().map_err(AppError::from)?);
            codex::apply_local_laguna_provider(&mut request, &model);
            request.base_url = laguna
                .ensure_for_turn(&root)
                .await
                .map_err(AppError::from)?
                .ok_or_else(|| AppError::untyped("Laguna Responses server is unavailable"))?;
            // The Laguna key is this process's loopback service token, not a
            // user credential: the child talks to the local daemon directly
            // and no broker lease is involved.
            request.api_key = laguna.api_key().ok_or_else(|| {
                AppError::untyped("Laguna daemon credential is unavailable after ensure")
            })?;
            let catalog = laguna
                .codex_model_catalog(&request.base_url, &request.api_key)
                .await
                .map_err(AppError::from)?;
            codex::apply_local_laguna_catalog_metadata(&mut request, catalog)
                .map_err(AppError::from)?;
        }
        codex::ProviderClass::OpenRouter => {
            let key = synth_config::openrouter_api_key().map_err(AppError::from)?;
            // The OpenRouter key is the user's, and leaks into shell snapshots
            // the same way the Synth key did; it goes into native custody too.
            // Its origin is also native-owned: renderer input must never decide
            // where that credential is forwarded.
            codex::apply_openrouter_provider(&mut request, key.as_deref())
                .map_err(AppError::untyped)?;
        }
        codex::ProviderClass::SynthCloud => {
            let resolved = synth_config::resolve().map_err(AppError::from)?;
            // Only Codex's Responses traffic uses the dedicated, source-owned
            // gateway for the active profile; account and billing calls elsewhere
            // keep reading `resolved.backend_url` directly. A
            // profile with no configured gateway fails closed here rather
            // than silently reusing the backend URL.
            let gateway_url = synth_config::require_responses_gateway_url(&resolved)
                .map_err(AppError::untyped)?;
            codex::apply_synth_cloud_provider(
                &mut request,
                &gateway_url,
                resolved.api_key.as_deref(),
            )
            .map_err(AppError::untyped)?;
        }
        codex::ProviderClass::OpenaiCodexOauth => {
            let credential = oauth
                .fresh_credential()
                .await
                .map_err(AppError::from)?
                .ok_or_else(|| {
                    AppError::untyped("Reconnect ChatGPT subscription in Settings → Models")
                })?;
            const ALLOWED: &[&str] = &["gpt-5.6-luna", "gpt-5.6-sol", "gpt-5.6-terra"];
            if !ALLOWED
                .iter()
                .any(|model| request.model.eq_ignore_ascii_case(model))
            {
                return Err(AppError::untyped(
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
            session_id: session_id.clone(),
            detail: error.detail,
        })?;
    if codex::provider_class(request.start.provider_name.as_deref())
        == codex::ProviderClass::SynthCloud
    {
        let resolved = synth_config::resolve().map_err(|error| CodexTurnFailure {
            code: "synth_cloud_admission_failed".into(),
            message: error.to_string(),
            session_id: session_id.clone(),
            detail: String::new(),
        })?;
        // Admission is intentionally isolated from the display cache. A
        // network failure or revoked key must fail closed rather than reuse a
        // previously healthy account snapshot for a paid turn.
        let admission = account_cloud::AccountCloudClient::open();
        let read = admission
            .read(
                &resolved.backend_url,
                resolved.api_key.as_deref(),
                true,
                chrono::Utc::now(),
            )
            .await;
        account_cloud::validate_turn_admission(&read).map_err(|message| {
            let error_class = if read.unauthenticated {
                "auth"
            } else if read.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .allowance
                    .remaining_cents
                    .is_some_and(|cents| cents <= 0)
            }) {
                "quota"
            } else {
                "outage"
            };
            crate::telemetry::emit(
                "recovery_attempted",
                serde_json::json!({"error_class": error_class, "outcome": "failure"}),
            );
            CodexTurnFailure {
                code: "synth_cloud_admission_failed".into(),
                message,
                session_id: session_id.clone(),
                detail: String::new(),
            }
        })?;
    }
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
    let started = state.start(app, request).await.map_err(AppError::from)?;
    crate::telemetry::mark_once(
        "first_workspace_opened",
        serde_json::json!({"workflow_family": "codex"}),
    );
    crate::telemetry::emit(
        "workflow_started",
        serde_json::json!({"workflow_family": "codex"}),
    );
    Ok(started)
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
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    request: CodexSessionRequest,
) -> Result<(), AppError> {
    state
        .interrupt(app, &request.session_id)
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
async fn codex_thread_read(
    state: State<'_, Arc<CodexManager>>,
    request: CodexThreadReadRequest,
) -> Result<contract::specta::OpaqueJson, AppError> {
    state
        .read_thread(request)
        .await
        .map(contract::specta::OpaqueJson)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
async fn codex_thread_items_list(
    state: State<'_, Arc<CodexManager>>,
    request: CodexThreadItemsRequest,
) -> Result<contract::specta::OpaqueJson, AppError> {
    state
        .list_thread_items(request)
        .await
        .map(contract::specta::OpaqueJson)
        .map_err(AppError::from)
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
    match state.resolve_approval(app, request).await {
        Ok(()) => Ok(()),
        Err(error) if crate::session::codex::is_detached_failure(&error) => Err(AppError {
            code: crate::session::codex::CODEX_SESSION_UNHEALTHY.into(),
            message: "This task's local agent is no longer running. Start a new turn to reconnect."
                .into(),
            detail: format!("{error:?}"),
            failure: None,
        }),
        Err(error) => Err(AppError::from(error)),
    }
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
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
) -> Result<Vec<CodexSessionRecord>, AppError> {
    state
        .expire_restored_approvals(&app)
        .await
        .map_err(AppError::from)?;
    Ok(state.list().await)
}

#[tauri::command]
#[specta::specta]
fn codex_default_workspace() -> Result<String, AppError> {
    let configured = synth_config::allowed_workspace_roots().map_err(|error| {
        AppError::untyped(format!("Cannot read workspace access settings: {error}"))
    })?;
    let permissions = synth_config::desktop_permission_settings().map_err(|error| {
        AppError::untyped(format!("Cannot read desktop permission settings: {error}"))
    })?;
    // Finder and LaunchServices do not reliably preserve launcher environment.
    // A named bundle's descriptor is the durable authority for its isolated
    // instance root, so recover the staged workspace from it when the explicit
    // launch variable is absent.
    let launcher_workspace = std::env::var_os("SYNTH_DESKTOP_WORKSPACE")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            crate::instance::identity()
                .ok()
                .and_then(|identity| identity.descriptor)
                .and_then(|descriptor| descriptor.instance_root)
                .map(|root| root.join("workspace"))
                .filter(|path| path.is_dir())
        });
    let path = synth_config::select_default_workspace_path(
        &configured,
        &permissions.sandbox_mode,
        launcher_workspace,
        dirs::home_dir(),
        crate::instance::state_root().join("workspaces/default"),
    );
    std::fs::create_dir_all(&path)
        .map_err(|error| AppError::io(format!("Cannot create the default workspace: {error}")))?;
    let path = path
        .canonicalize()
        .map_err(|error| AppError::io(format!("Default workspace is unavailable: {error}")))?;
    if !path.is_dir() {
        return Err(AppError::invalid_argument("Default workspace must be a directory"));
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
    crate::instance::install_boot_identity_and_lock();
    let specta = contract::specta::builder();

    tauri::Builder::default()
        // This must be the first plugin registered. All app state, IPC, and
        // SQLite ownership belongs to the original process.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            for raw in args.iter().filter(|arg| arg.starts_with("synth-workshop://")) {
                match crate::instance::parse_workshop_deep_link(raw) {
                    Ok(route) => {
                        if let Err(error) = app.emit("desktop:deep-link", route) {
                            crate::platform::logging::report(
                                "lib",
                                "deep_link",
                                format!("could not dispatch Workshop deep link: {error}"),
                            );
                        }
                    }
                    Err(error) => crate::platform::logging::report(
                        "lib",
                        "deep_link",
                        format!("refused Workshop deep link: {error}"),
                    ),
                }
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // The main window starts hidden. Showing it from setup races the overlay
        // webview's first paint on macOS and exposes a dark strip from the window
        // behind through the transparent native titlebar. Wait until CSS and the
        // document have loaded so the custom titlebar is present on first reveal.
        .on_page_load(|webview, payload| {
            if webview.label() == "main"
                && matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
            {
                let window = webview.window();
                let _ = window.maximize();
                let _ = window.show();
                // Diagnostics start here, not in setup: the index is a
                // background convenience and must never sit in front of the
                // first paint. `start` is idempotent, and the bootstrap task
                // arms the same call in case this page never loads — the one
                // failure diagnostics most needs to be running for.
                if let Some(core) = window.app_handle().try_state::<Arc<CoreRuntime>>() {
                    core.diagnostics_service().start();
                }
            }
        })
        .setup(|app| {
            instance::mark_manifest_running();
            // A renderer/bootstrap failure must never leave a running process
            // as an invisible window. Page-load still owns the normal reveal.
            let watchdog_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(15)).await;
                if let Some(window) = watchdog_app.get_webview_window("main") {
                    if !window.is_visible().unwrap_or(false) {
                        if let Err(error) = window.eval(
                            "document.body.innerHTML='<main style=\"font:15px system-ui;padding:32px;color:#1f2937\"><h1>Workshop could not finish loading</h1><p>The renderer did not become ready within 15 seconds. Restart this instance; your runs and artifacts remain durable.</p></main>'"
                        ) {
                            crate::platform::logging::report(
                                "lib",
                                "page_load_watchdog",
                                format!("could not install page-load failure view: {error}"),
                            );
                        }
                        if let Err(error) = window.show() {
                            crate::platform::logging::report(
                                "lib",
                                "page_load_watchdog",
                                format!("could not show main window after page-load timeout: {error}"),
                            );
                        }
                        if let Err(error) = watchdog_app.emit(
                            "desktop:page-load-timeout",
                            serde_json::json!({
                                "code": "renderer_page_load_timeout",
                                "message": "The renderer did not finish loading within 15 seconds."
                            }),
                        ) {
                            crate::platform::logging::report(
                                "lib",
                                "page_load_watchdog",
                                format!("could not emit page-load timeout: {error}"),
                            );
                        }
                    }
                }
            });
            // Builds before the credential broker exported provider keys into
            // Codex, which recorded them in its shell snapshots. Scrub what
            // those builds left behind in Desktop's own Codex homes.
            match credential_broker::redact_managed_shell_snapshots(&codex::codex_root()) {
                Ok(0) => {}
                Ok(count) => {
                    crate::platform::logging::report("lib", "eprintln", format!("redacted provider secrets from {count} Codex shell snapshot(s)"))
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
            if let Err(error) = core.secrets().start_proxy() {
                crate::platform::logging::report("lib", "eprintln", format!("synth-desktop: provider proxy failed to start: {error:#}"));
            }
            crate::secrets::install_live(core.secrets().clone());
            let telemetry = Arc::new(crate::telemetry::ProductTelemetry::new(
                core.storage().database().clone(),
            ));
            crate::telemetry::install_live(telemetry.clone());
            crate::telemetry::mark_once("app_first_launch", serde_json::json!({}));
            let approvals = Arc::new(crate::session::approval::ApprovalBroker::new(
                crate::session::SessionPersistence::from_core(Some(core.clone())),
            ));
            let whisper = Arc::new(whisper::WhisperManager::new());
            let codex = Arc::new(CodexManager::new(
                Some(core.clone()),
                broker.clone(),
                approvals.clone(),
            ));
            let supervisor = Arc::new(services::ServiceSupervisor::new());
            supervisor.register(laguna.clone());
            supervisor.register(optimizer_manager.clone());
            supervisor.register(Arc::new(optimizers::mlx_runtime::MlxRuntimeService::new()));
            supervisor.register(whisper.clone());
            supervisor.register(core.diagnostics_service().sidecar().clone());
            app.manage(core.clone());
            app.manage(core.secrets().clone());
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
            app.manage(telemetry);
            app.manage(laguna.clone());
            app.manage(optimizer_manager.clone());
            app.manage(supervisor);

            // All committed CoreRuntime events reach Tauri through this single
            // forwarder. Producers only journal and broadcast.
            core.spawn_forwarder(app.handle().clone());

            // Backend-owned liveness. The renderer's turn watchdogs are cleared
            // when its window unloads, so they cannot fence a turn whose owner
            // died — this sweep can, with or without a window open.
            core.spawn_lease_watchdog();

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
                    crate::platform::logging::report("lib", "eprintln", format!("CoreRuntime bootstrap failed: {error}"));
                }
                if let Err(error) = bootstrap_approvals.expire_restored(&bootstrap_handle).await {
                    crate::platform::logging::report("lib", "eprintln", format!("approval restore failed: {error}"));
                }
                if let Err(error) = bootstrap_core.resume_intern_providers().await {
                    crate::platform::logging::report("lib", "eprintln", format!("Intern restart reconciliation failed: {error}"));
                }
                // Fallback arm: if the main window never finished loading, the
                // renderer's own failure still has somewhere to be recorded.
                bootstrap_core.diagnostics_service().start();
            });

            let ipc_core = core.clone();
            let ipc_app = app.handle().clone();
            let ipc_root = crate::storage::app_data_root();
            tauri::async_runtime::spawn(async move {
                match visuals_ipc::spawn(ipc_core, ipc_app, ipc_root).await {
                    Ok(connection) => {
                        crate::platform::logging::report("lib", "eprintln", format!(
                            "Visuals IPC listening at {} (token written to {})",
                            connection.url, connection.path
                        ));
                    }
                    Err(error) => crate::platform::logging::report("lib", "eprintln", format!("Visuals IPC failed to start: {error}")),
                }
            });

            #[cfg(feature = "eval-driver")]
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
                            crate::platform::logging::report("lib", "eprintln", format!(
                                "Eval driver ({}) listening at {} (descriptor {})",
                                eval_driver::PROTOCOL_VERSION,
                                connection.url,
                                connection.path
                            ));
                        }
                        Err(error) => crate::platform::logging::report("lib", "eprintln", format!("Eval driver failed to start: {error}")),
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(specta.invoke_handler())
        .build(tauri::generate_context!())
        .expect("error while building Synth Desktop")
        .run(|app, event| {
            // macOS may advance from Command-Q to the terminal `Exit` event
            // without giving every plugin observer an `ExitRequested` callback.
            // Draining is idempotent, so cover both phases: a clean request
            // stops services early, and `Exit` is the final ownership fence.
            if matches!(event, RunEvent::ExitRequested { .. } | RunEvent::Exit) {
                if let Some(supervisor) = app.try_state::<Arc<services::ServiceSupervisor>>() {
                    let supervisor = (*supervisor).clone();
                    tauri::async_runtime::block_on(supervisor.drain_all());
                }
            }
        });
}
