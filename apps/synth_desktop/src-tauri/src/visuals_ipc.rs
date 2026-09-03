//! Authenticated loopback IPC so the visual MCP adapter never opens SQLite.

use crate::codex::CodexManager;
use crate::container_stream::{
    authoritative_poll_telemetry, declared_poll_url, declared_sse_url, declared_stream_descriptor,
    normalized_rollout_telemetry, refuse_auto_transport, require_caller_policy_ref,
    require_task_instance, resolve_declared_url, wait_for_stream_subscribed,
    SUBSCRIBE_READY_TIMEOUT,
};
use crate::core_runtime::CoreRuntime;
use crate::data::ContainerRegisterRequest;
use crate::domain::{PresentationField, SessionTitleOrigin};
use crate::ipc::{serve_json, JsonHttpRequest, JsonHttpResponse};
use crate::limits;
use crate::reports::{
    ExperimentRecordUpsert, ReportAttachTrace, ReportCreateRequest, ReportQuery,
    ReportUpdateRequest, ReportVisibilityRequestCreate, ResearchLogAppend,
};
use crate::visuals::{
    assert_live_eval_slot, classify_live_eval_family, live_eval_bind_metadata,
    require_visualsbench_start_policy, VisualAnnotationCreate, VisualCreateRequest, VisualQuery,
    VisualStatus, VisualUpdateRequest, LIVE_EVAL_SLOT,
};
use crate::visuals::{TemplateMeta, TemplateObservationContract};
use base64::Engine;

const MAX_SCRIPTED_ROLLOUTS: u64 = 10;
const BASE_AUTHORING_CHECKS: [&str; 6] = [
    "rendered",
    "noOverflow",
    "primarySurfaceVisible",
    "temporalControls",
    "traceInspector",
    "realEvidence",
];

fn required_authoring_checks(template: &TemplateMeta) -> Vec<&'static str> {
    let mut checks = BASE_AUTHORING_CHECKS.to_vec();
    checks.push("screenshotInspected");
    if template.id.starts_with("diagram.") {
        checks.push("noTextCollisions");
        checks.push("focalDensity");
    }
    if template
        .observation_contract
        .as_ref()
        .is_some_and(|contract| contract.readiness.minimum_rendered_frame_count > 0)
    {
        checks.push("imageReplay");
    }
    checks
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisualCaptureObservationReceipt {
    schema_version: String,
    visual_id: String,
    revision: i64,
    screenshot_path: String,
    capture_time: String,
    observation: Option<RenderedVisualObservation>,
}

fn capture_observation_receipt(screenshot: &str) -> Result<VisualCaptureObservationReceipt> {
    let path = std::path::Path::new(screenshot).with_extension("observations.json");
    let receipt: VisualCaptureObservationReceipt =
        serde_json::from_slice(&fs::read(&path).with_context(|| {
            format!(
                "visual review requires capture observations beside {}",
                path.display()
            )
        })?)
        .context("visual capture observation receipt is invalid")?;
    if receipt.schema_version != "synth.visual-capture-observation.v1"
        || receipt.screenshot_path != screenshot
    {
        anyhow::bail!("visual capture observation receipt does not match screenshot");
    }
    Ok(receipt)
}

/// Rendered transport states that can carry evidence.
///
/// `live` is caught up with at least one stream open; `terminal` is every
/// declared stream closed. Every other state — including `connecting` from the
/// pre-state-machine vocabulary — means the pane is not showing a settled
/// answer, whether or not a template's own contract remembered to list it.
///
/// See: docs/contracts/visual_replay_transport.md.
const READY_TRANSPORT_STATES: &[&str] = &["live", "terminal"];

fn validate_readiness_observation(
    contract: &TemplateObservationContract,
    visual_id: &str,
    revision: i64,
    bindings_digest: &str,
    observation: &RenderedVisualObservation,
) -> Result<()> {
    if observation.visual_id != visual_id || observation.rendered_revision != revision {
        anyhow::bail!(
            "captured rendered revision {} for {}, expected revision {revision} for {visual_id}",
            observation.rendered_revision,
            observation.visual_id
        );
    }
    if observation.bindings_digest != bindings_digest {
        anyhow::bail!("captured bindings do not match the current saved revision");
    }
    let readiness = &contract.readiness;
    // Readiness is decided by an allowlist, not a denylist. A denylist accepts
    // every state nobody thought to list, including a state a future template
    // invents — and "unknown" is exactly the case where a pane is least likely
    // to be showing real evidence.
    if !READY_TRANSPORT_STATES.contains(&observation.transport_state.as_str()) {
        anyhow::bail!(
            "visual readiness rejects rendered transport state {}; \
             a ready visual is one of {}",
            observation.transport_state,
            READY_TRANSPORT_STATES.join(", ")
        );
    }
    if readiness
        .reject_transport_states
        .iter()
        .any(|state| state == &observation.transport_state)
    {
        anyhow::bail!(
            "visual readiness rejects rendered transport state {}",
            observation.transport_state
        );
    }
    if observation
        .error
        .as_deref()
        .is_some_and(|error| !error.is_empty())
    {
        anyhow::bail!("visual readiness rejects a rendered error state");
    }
    if observation.rollout_count < readiness.minimum_rollout_count {
        anyhow::bail!("visual readiness has zero or insufficient rendered rollouts");
    }
    if observation.rendered_frame_count < readiness.minimum_rendered_frame_count {
        anyhow::bail!("visual readiness has zero or insufficient rendered frame evidence");
    }
    if observation.semantic_event_count < readiness.minimum_semantic_event_count {
        anyhow::bail!("visual readiness has zero or insufficient semantic event evidence");
    }
    if readiness.require_terminal && !observation.terminal {
        anyhow::bail!("visual readiness requires rendered terminal evidence");
    }
    Ok(())
}

/// Decide readiness from the *latest* review at each viewport width, not from
/// every review ever recorded against the revision.
///
/// A live visual legitimately fails review before its stream starts and passes
/// once evidence is terminal, and neither transition changes the durable
/// revision. Requiring every historical review to pass therefore turned an
/// honest pre-start failure into a permanent veto, and agents worked around it
/// by bumping the revision for cosmetic reasons. Superseded reviews stay in
/// `authoringReviews` as provenance; they just stop being vetoes.
///
/// Each returned receipt binds the certifying capture to its revision, bindings
/// digest, transport state, evidence counts, and viewport, so a later reader can
/// tell a current pass from a stale one without re-deriving it.
fn certification_receipts(
    visual_id: &str,
    revision: i64,
    current_reviews: &[&Value],
    required_checks: &[&'static str],
    contract: Option<&TemplateObservationContract>,
    bindings_digest: Option<&str>,
) -> Result<Vec<Value>> {
    let mut latest_by_width: BTreeMap<u64, &Value> = BTreeMap::new();
    for review in current_reviews {
        let Some(width) = review.pointer("/viewport/width").and_then(Value::as_u64) else {
            continue;
        };
        latest_by_width.insert(width, review);
    }
    if latest_by_width.len() < 2 {
        anyhow::bail!("visual readiness requires reviews at two distinct viewport widths");
    }
    let mut receipts = Vec::new();
    for (width, review) in &latest_by_width {
        for check in required_checks {
            if review
                .pointer(&format!("/checks/{check}"))
                .and_then(Value::as_bool)
                != Some(true)
            {
                anyhow::bail!(
                    "the latest review at width {width} is not passing required check {check}; \
                     capture and review this width again once it renders correctly"
                );
            }
        }
        let mut receipt = json!({
            "revision": revision,
            "viewportWidth": width,
            "viewportHeight": review.pointer("/viewport/height").cloned().unwrap_or(Value::Null),
            "screenshotPath": review.get("screenshotPath").cloned().unwrap_or(Value::Null),
            "captureTime": review.get("captureTime").cloned().unwrap_or(Value::Null),
            "reviewedAt": review.get("reviewedAt").cloned().unwrap_or(Value::Null),
        });
        if let Some(contract) = contract {
            let digest =
                bindings_digest.context("current visual revision is missing bindings digest")?;
            let observation: RenderedVisualObservation = serde_json::from_value(
                review
                    .get("observations")
                    .cloned()
                    .filter(|value| !value.is_null())
                    .with_context(|| {
                        format!(
                            "the latest review at width {width} carries no rendered observation; \
                             capture this width again with the pane open"
                        )
                    })?,
            )
            .context("visual review rendered observations are invalid")?;
            validate_readiness_observation(contract, visual_id, revision, digest, &observation)?;
            receipt["bindingsDigest"] = json!(digest);
            receipt["transportState"] = json!(observation.transport_state);
            receipt["terminal"] = json!(observation.terminal);
            receipt["renderedFrameCount"] = json!(observation.rendered_frame_count);
            receipt["semanticEventCount"] = json!(observation.semantic_event_count);
            receipt["observedAt"] = json!(observation.observed_at);
        }
        receipts.push(receipt);
    }
    Ok(receipts)
}

use anyhow::{Context, Result};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Digest;
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, Size};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualsIpcConnection {
    pub url: String,
    pub token: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RenderedVisualObservation {
    pub schema_version: String,
    pub visual_id: String,
    #[specta(type = specta_typescript::Number)]
    pub rendered_revision: i64,
    pub bindings_digest: String,
    pub transport_state: String,
    #[specta(type = specta_typescript::Number)]
    pub rollout_count: u64,
    #[specta(type = specta_typescript::Number)]
    pub rendered_frame_count: u64,
    #[specta(type = specta_typescript::Number)]
    pub semantic_event_count: u64,
    pub terminal: bool,
    pub error: Option<String>,
    pub observed_at: String,
}

static RENDERED_OBSERVATIONS: OnceLock<Mutex<BTreeMap<String, RenderedVisualObservation>>> =
    OnceLock::new();

/// The data root this server was spawned with. Review capture writes PNGs to
/// caller-named paths, and this is the boundary those paths must stay inside.
static VISUALS_DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn record_rendered_observation(observation: RenderedVisualObservation) -> Result<()> {
    if observation.schema_version != "synth.rendered-visual-observation.v1" {
        anyhow::bail!("unsupported rendered visual observation schema");
    }
    if observation.visual_id.trim().is_empty() || observation.rendered_revision < 1 {
        anyhow::bail!("rendered visual observation requires visual identity and revision");
    }
    if observation.bindings_digest.trim().is_empty()
        || observation.transport_state.trim().is_empty()
    {
        anyhow::bail!("rendered visual observation requires bindings and transport authority");
    }
    RENDERED_OBSERVATIONS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| anyhow::anyhow!("rendered observation store is unavailable"))?
        .insert(observation.visual_id.clone(), observation);
    Ok(())
}

fn rendered_observation(visual_id: &str) -> Result<RenderedVisualObservation> {
    RENDERED_OBSERVATIONS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| anyhow::anyhow!("rendered observation store is unavailable"))?
        .get(visual_id)
        .cloned()
        .with_context(|| format!("no rendered observation is available for visual {visual_id}"))
}

pub fn connection_path(root: &std::path::Path) -> PathBuf {
    root.join("visuals-ipc.json")
}

pub async fn spawn(
    core: Arc<CoreRuntime>,
    app: AppHandle,
    root: PathBuf,
) -> Result<VisualsIpcConnection> {
    let token = format!("synth_vis_{}", Uuid::new_v4());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("bind visuals IPC")?;
    let addr = listener.local_addr()?;
    let connection = VisualsIpcConnection {
        url: format!("http://{addr}"),
        token: token.clone(),
        path: connection_path(&root).display().to_string(),
    };
    fs::create_dir_all(&root)?;
    let _ = VISUALS_DATA_ROOT.set(root.clone());
    let connection_file = connection_path(&root);
    fs::write(&connection_file, serde_json::to_string_pretty(&connection)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&connection_file, fs::Permissions::from_mode(0o600))?;
    }

    let serve_core = core.clone();
    tauri::async_runtime::spawn(async move {
        let result = serve_json(listener, move |request| {
            let core = serve_core.clone();
            let app = app.clone();
            let token = token.clone();
            async move { route_request(request, &core, &app, &token).await }
        })
        .await;
        if let Err(error) = result {
            crate::platform::logging::report(
                "visuals_ipc",
                "eprintln",
                format!("synth-desktop: visuals IPC stopped: {error:#}"),
            );
        }
    });
    Ok(connection)
}

async fn route_request(
    request: JsonHttpRequest,
    core: &CoreRuntime,
    app: &AppHandle,
    token: &str,
) -> JsonHttpResponse {
    // Every MCP adapter call arrives here, so this is the one place that has to
    // instrument them. Operation name, duration, status, and correlated
    // identities only — never the arguments, which carry policy refs, prompts,
    // and whatever else a caller decided to send.
    let started = std::time::Instant::now();
    let operation = redacted_operation(&request.path);
    let correlation = correlation_from_ipc_body(&request.body);
    // Every agent tool call crosses this boundary, which makes it the one place
    // fault injection can crash Workshop at a realistic point in a turn rather
    // than at an arbitrary instruction.
    crate::recovery::crash_checkpoint(crate::recovery::checkpoints::BEFORE_TOOL_DISPATCH);
    let response = route_request_inner(request, core, app, token).await;
    crate::recovery::crash_checkpoint(crate::recovery::checkpoints::AFTER_TOOL_DISPATCH);
    let elapsed = started.elapsed();
    if !response.status.is_success() {
        record_ipc_failure(core, &operation, &response, correlation, elapsed);
    } else if elapsed >= SLOW_CALL_THRESHOLD {
        // Thresholded, never sampled: a sampled latency record answers no
        // specific question, while "this call took 40 seconds" answers one.
        record_slow_call(core, &operation, correlation, elapsed);
    }
    response
}

/// A successful call slower than this is worth one record.
const SLOW_CALL_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(5);

fn record_slow_call(
    core: &CoreRuntime,
    operation: &str,
    correlation: crate::diagnostics::Correlation,
    elapsed: std::time::Duration,
) {
    let mut input = crate::diagnostics::DiagnosticInput::new(
        crate::diagnostics::Severity::Info,
        "mcp",
        "mcp.request.slow",
        "mcp_request_slow",
        format!("{operation} took {}ms", elapsed.as_millis()),
    );
    input.correlation = correlation;
    input.details.insert("operation".into(), json!(operation));
    input
        .details
        .insert("duration_ms".into(), json!(elapsed.as_millis() as u64));
    core.diagnostics_service().emit(input);
}

/// Operation name for a diagnostic label: the route with its identities
/// stripped, so `/v1/containers/ctr_9/rollouts/start` stays one low-cardinality
/// label instead of one label per container.
fn redacted_operation(path: &str) -> String {
    let path = path.split('?').next().unwrap_or(path);
    path.split('/')
        .map(|segment| {
            if segment.len() > 12 && segment.contains('_') {
                "{id}"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Pull whatever identities the request body already names. An MCP failure
/// that knows its rollout is worth far more than one that only knows its path.
fn correlation_from_ipc_body(body: &Value) -> crate::diagnostics::Correlation {
    let mut correlation = crate::diagnostics::Correlation::default();
    let Some(object) = body.as_object() else {
        return correlation;
    };
    for (camel, snake, field) in [
        ("sessionRef", "session_id", "session_id"),
        ("visualId", "visual_id", "visual_id"),
        ("containerId", "container_id", "container_id"),
        ("rolloutId", "rollout_id", "rollout_id"),
        ("streamId", "stream_id", "stream_id"),
        ("optimizerRunId", "optimizer_run_id", "optimizer_run_id"),
        ("traceId", "trace_id", "trace_id"),
        ("commandId", "command_id", "command_id"),
    ] {
        if let Some(value) = object
            .get(camel)
            .or_else(|| object.get(snake))
            .and_then(Value::as_str)
        {
            correlation.set(field, Some(value.to_owned()));
        }
    }
    correlation
}

fn record_ipc_failure(
    core: &CoreRuntime,
    operation: &str,
    response: &JsonHttpResponse,
    correlation: crate::diagnostics::Correlation,
    elapsed: std::time::Duration,
) {
    let status = response.status.as_u16();
    // A stable code from the responding subsystem beats a generic one; a
    // capability rejection must stay queryable as itself.
    let code = response
        .body
        .get("code")
        .and_then(Value::as_str)
        .filter(|code| {
            code.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
        .unwrap_or(crate::diagnostics::codes::MCP_REQUEST_FAILED)
        .to_owned();
    let message = response
        .body
        .get("message")
        .or_else(|| response.body.get("error"))
        .and_then(Value::as_str)
        .unwrap_or("IPC request failed")
        .to_owned();
    let mut input = crate::diagnostics::DiagnosticInput::new(
        if status >= 500 {
            crate::diagnostics::Severity::Error
        } else {
            crate::diagnostics::Severity::Warn
        },
        "mcp",
        "mcp.request.failed",
        &code,
        message,
    )
    .retryable(status == 429 || status >= 500);
    input.correlation = correlation;
    input.details.insert("operation".into(), json!(operation));
    input.details.insert("status".into(), json!(status));
    input
        .details
        .insert("duration_ms".into(), json!(elapsed.as_millis() as u64));
    if let Some(remediation) = response.body.get("remediation") {
        input
            .details
            .insert("remediation".into(), remediation.clone());
    }
    if let Some(missing) = response.body.get("missingOperations") {
        input
            .details
            .insert("missing_operations".into(), missing.clone());
    }
    core.diagnostics_service().emit(input);
}

async fn route_request_inner(
    request: JsonHttpRequest,
    core: &CoreRuntime,
    app: &AppHandle,
    token: &str,
) -> JsonHttpResponse {
    match dispatch_request(request, core, app, token).await {
        Ok(value) => JsonHttpResponse::ok(value),
        Err(error) if crate::error::error_is::<crate::error::Unauthorized>(&error) => {
            JsonHttpResponse::error(StatusCode::UNAUTHORIZED, error.to_string())
        }
        Err(error)
            if crate::error::error_is::<crate::container_capabilities::ContainerPreflightError>(
                &error,
            ) =>
        {
            let body = error
                .chain()
                .find_map(|cause| {
                    cause.downcast_ref::<crate::container_capabilities::ContainerPreflightError>()
                })
                .and_then(|preflight| {
                    core.storage()
                        .database()
                        .transaction(|conn| {
                            crate::domains::containers::raise_probe_failure(
                                conn,
                                crate::domains::containers::from_preflight(preflight),
                                &preflight.container_id,
                                None,
                            )
                        })
                        .ok()
                        .map(|raised| crate::adapters::mcp::tool_error_body(&raised))
                })
                .unwrap_or_else(|| crate::container_capabilities::preflight_error_body(&error));
            JsonHttpResponse::with_status(StatusCode::CONFLICT, body)
        }
        Err(error) if crate::error::error_is::<crate::error::StructuredFailure>(&error) => {
            let body = error
                .chain()
                .find_map(|cause| cause.downcast_ref::<crate::error::StructuredFailure>())
                .map(crate::error::StructuredFailure::to_json)
                .unwrap_or_else(|| json!({"code": "internal", "error": error.to_string()}));
            JsonHttpResponse::with_status(StatusCode::BAD_REQUEST, body)
        }
        Err(error)
            if error.chain().any(|cause| {
                cause
                    .downcast_ref::<crate::optimizers::admission::AdmissionError>()
                    .is_some()
            }) =>
        {
            let body = error
                .chain()
                .find_map(|cause| {
                    cause.downcast_ref::<crate::optimizers::admission::AdmissionError>()
                })
                .and_then(|admission| {
                    core.storage()
                        .database()
                        .transaction(|conn| {
                            crate::domains::evaluations::raise(conn, admission, None)
                        })
                        .ok()
                        .map(|raised| crate::adapters::mcp::tool_error_body(&raised))
                })
                .unwrap_or_else(|| json!({"code": "admission_failed"}));
            JsonHttpResponse::with_status(StatusCode::BAD_REQUEST, body)
        }
        Err(error) if crate::error::error_is::<crate::plugins::PluginNotReady>(&error) => {
            let body = error
                .downcast_ref::<crate::plugins::PluginNotReady>()
                .map(crate::plugins::PluginNotReady::to_json)
                .unwrap_or_else(|| json!({"code":"plugin_not_ready"}));
            JsonHttpResponse::with_status(StatusCode::CONFLICT, body)
        }
        Err(error) => JsonHttpResponse::error(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn dispatch_request(
    request: JsonHttpRequest,
    core: &CoreRuntime,
    app: &AppHandle,
    token: &str,
) -> Result<Value> {
    let auth = request
        .authorization
        .as_deref()
        .and_then(|value| value.strip_prefix("Bearer ").map(str::trim));
    if !auth.is_some_and(|value| crate::ipc::constant_time_eq(value.as_bytes(), token.as_bytes())) {
        return Err(anyhow::Error::new(crate::error::Unauthorized));
    }
    let (path, query_string) = request
        .path
        .split_once('?')
        .unwrap_or((request.path.as_str(), ""));
    let json_body = if request.body.is_null() {
        query_json(query_string)
    } else {
        request.body
    };
    let method = request.method.as_str();
    if method == "GET" && path.starts_with("/v1/review-observations/") {
        let visual_id = path.trim_start_matches("/v1/review-observations/");
        if visual_id.is_empty() || visual_id.contains('/') {
            anyhow::bail!("invalid review observation visual id");
        }
        // An absent observation is an answer, not a failure. Whether it is a
        // *fatal* answer depends on the template's contract, and only this side
        // knows the template — so report both and let capture decide once.
        let registry = core.visuals();
        let required = match registry.get(visual_id.to_string()).await {
            Ok(visual) => registry
                .get_template(&visual.template_id)
                .map(|template| template.observation_contract.is_some())
                .unwrap_or(false),
            Err(_) => false,
        };
        return Ok(json!({
            "observation": rendered_observation(visual_id).ok(),
            "required": required,
        }));
    }
    if method == "POST" && path == "/v1/review-window/resize" {
        return resize_review_window(app, &json_body);
    }
    if method == "POST" && path == "/v1/review-window/capture" {
        return capture_review_window(app, &json_body).await;
    }
    if method == "POST" && path == "/v1/capture" {
        return capture_surface(app, &json_body).await;
    }
    if path.starts_with("/v1/plugins") {
        return dispatch_plugins(method, path, json_body, core, app).await;
    }
    if path.starts_with("/v1/display/plugins") {
        return dispatch_display_plugins(method, path, json_body, app);
    }
    if path.starts_with("/v1/computer-use") {
        return dispatch_computer_use(method, path, json_body, core, app).await;
    }
    if method == "POST" && path == "/v1/sessions/present" {
        return present_session(app, core, json_body).await;
    }
    if path.starts_with("/v1/optimizers")
        || path.starts_with("/v1/training")
        || path.starts_with("/v1/mlx")
    {
        return dispatch_optimizer(method, path, json_body, core, app).await;
    }
    if path.starts_with("/v1/experiments") {
        return dispatch_experiments(method, path, json_body, core).await;
    }
    if path.starts_with("/v1/research-log") {
        return dispatch_research_log(method, path, json_body, core).await;
    }
    if path.starts_with("/v1/traces") {
        return dispatch_traces(method, path, json_body, core).await;
    }
    if path.starts_with("/v1/analysis") {
        return dispatch_analysis(method, path, json_body, core).await;
    }
    if path.starts_with("/v1/annotations") {
        return crate::annotations_ipc::dispatch_annotations(method, path, json_body, core, app)
            .await;
    }
    if path.starts_with("/v1/human-annotations") {
        return dispatch_human_annotations(method, path, json_body, core, app).await;
    }
    if path.starts_with("/v1/diagnostics") {
        return dispatch_diagnostics(method, path, json_body, core).await;
    }
    if path.starts_with("/v1/secrets") {
        return dispatch_secrets(method, path, json_body, core, app).await;
    }
    if method == "POST" && path.starts_with("/v1/containers/") && path.ends_with("/restart") {
        return dispatch_container_restart(path, json_body, core, app).await;
    }
    dispatch(method, path, json_body, core).await
}

async fn dispatch_human_annotations(
    method: &str,
    path: &str,
    body: Value,
    core: &CoreRuntime,
    app: &AppHandle,
) -> Result<Value> {
    use crate::human_annotations::models::{
        HumanAnnotationCancelRequest, HumanAnnotationCreateRequest, HumanAnnotationExportRequest,
        HumanAnnotationListQuery,
    };
    let service = crate::human_annotations::from_core(core);
    match (method, path) {
        ("POST", "/v1/human-annotations/create") => Ok(serde_json::to_value(
            service
                .create(serde_json::from_value::<HumanAnnotationCreateRequest>(
                    body,
                )?)
                .await?,
        )?),
        ("POST", "/v1/human-annotations/show") => {
            let session_id = body
                .get("sessionId")
                .or_else(|| body.get("session_id"))
                .and_then(Value::as_str)
                .context("sessionId required")?
                .to_owned();
            let view = service.open(session_id.clone()).await?;
            app.emit(
                "human-annotation:show",
                serde_json::json!({"sessionId":session_id}),
            )?;
            Ok(serde_json::to_value(view)?)
        }
        ("POST", "/v1/human-annotations/get") => {
            let id = body
                .get("taskId")
                .or_else(|| body.get("task_id"))
                .or_else(|| body.get("sessionId"))
                .or_else(|| body.get("session_id"))
                .and_then(Value::as_str)
                .context("taskId required")?
                .to_owned();
            Ok(serde_json::to_value(service.status(id).await?)?)
        }
        ("POST", "/v1/human-annotations/list") => Ok(Value::Array(
            service
                .list(serde_json::from_value::<HumanAnnotationListQuery>(body)?)
                .await?,
        )),
        ("POST", "/v1/human-annotations/cancel") => Ok(serde_json::to_value(
            service
                .cancel(serde_json::from_value::<HumanAnnotationCancelRequest>(
                    body,
                )?)
                .await?,
        )?),
        ("POST", "/v1/human-annotations/export") => Ok(serde_json::to_value(
            service
                .export(serde_json::from_value::<HumanAnnotationExportRequest>(
                    body,
                )?)
                .await?,
        )?),
        _ => anyhow::bail!("unsupported human annotation IPC route {method} {path}"),
    }
}

fn dispatch_display_plugins(
    method: &str,
    path: &str,
    body: Value,
    app: &AppHandle,
) -> Result<Value> {
    const ALLOWED: [&str; 7] = [
        "visuals",
        "reports",
        "experiments",
        "optimizers",
        "inventory",
        "inference",
        "computer-use",
    ];
    if method == "GET" && path == "/v1/display/plugins" {
        return Ok(json!({"pluginIds": ALLOWED}));
    }
    if method != "POST" || path != "/v1/display/plugins/visibility" {
        anyhow::bail!("unsupported display IPC route {method} {path}");
    }
    let ids = body
        .get("visiblePluginIds")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("visiblePluginIds array required"))?;
    let mut visible = Vec::new();
    for value in ids {
        let id = value
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("plugin ids must be strings"))?;
        if !ALLOWED.contains(&id) {
            anyhow::bail!("unknown display plugin `{id}`");
        }
        if !visible.iter().any(|current| current == id) {
            visible.push(id.to_string());
        }
    }
    app.emit(
        "workshop-display-plugin-visibility",
        json!({"visiblePluginIds": visible}),
    )?;
    Ok(json!({"visiblePluginIds": visible, "applied": true}))
}

fn resize_review_window(app: &AppHandle, body: &Value) -> Result<Value> {
    let window = app
        .get_webview_window("main")
        .context("review capture requires the main Desktop window")?;
    let width = body
        .get("width")
        .and_then(Value::as_f64)
        .context("review window width is required")?;
    let height = body
        .get("height")
        .and_then(Value::as_f64)
        .context("review window height is required")?;
    if !(320.0..=2400.0).contains(&width) || !(400.0..=1800.0).contains(&height) {
        anyhow::bail!("review window must be within 320x400 and 2400x1800");
    }
    // A review viewport is a CSS viewport, so every size on this endpoint is
    // logical. `inner_size` reports physical pixels: on a 2x display a
    // 1440-wide window reads back as 2880, which the caller then sends here to
    // restore and which this bound rejects — leaving the user's window stuck at
    // the review size. Applying a logical request as physical also halved the
    // captured viewport on those displays, so responsive review ran at the
    // wrong breakpoint.
    let scale = window.scale_factor().context("read display scale factor")?;
    let previous = window
        .inner_size()
        .context("read review window size")?
        .to_logical::<f64>(scale);
    window
        .set_size(Size::Logical(LogicalSize::new(width, height)))
        .context("resize review window")?;
    // Report what the window manager actually gave us; it may clamp.
    let current = window
        .inner_size()
        .context("read resized review window size")?
        .to_logical::<f64>(scale);
    Ok(json!({
        "previous": {"width": previous.width.round() as u64, "height": previous.height.round() as u64},
        "current": {"width": current.width.round() as u64, "height": current.height.round() as u64},
        // Identity of the window this call actually resized. Capture verifies
        // it instead of re-resolving one by name and size, which is how a
        // compact review lost its own window to a minimum-size filter. The
        // process id is exact and free: several named instances of this app run
        // at once and share a bundle prefix, so a name is not an identity.
        "scaleFactor": scale,
        "windowLabel": window.label(),
        "processId": std::process::id(),
    }))
}

/// How long the renderer gets to relayout at the review viewport before the
/// snapshot. Carried over from the previous capture pipeline, where the helper
/// slept between resize and `screencapture` for the same reason.
const REVIEW_CAPTURE_SETTLE: std::time::Duration = std::time::Duration::from_secs(3);

/// The whole snapshot round trip, resize excluded. Bounds the window a wedged
/// WebKit could hold the resized viewport, and the IPC route with it.
/// First snapshot attempt. `setAfterScreenUpdates(true)` waits for the renderer
/// to commit a frame, so this budget is really "how long may a repaint take",
/// not how long the encode takes.
const REVIEW_CAPTURE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Second attempt, after the first has already forced layout and paint.
///
/// A terminal SFT surface -- 2917 events, checkpoint curves, a rollout ledger --
/// missed the 10s budget on its first frame, and the run's most important
/// capture was lost with nothing to retry it. The cheapest fix is to ask twice:
/// the first attempt pays for layout, the second usually returns immediately.
/// A longer single timeout would make every genuinely dead webview wait twice
/// as long before saying so.
const REVIEW_CAPTURE_RETRY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

/// Tell the React shell to make one selected visual the only visible surface
/// while a review image is taken. This is deliberately an ephemeral renderer
/// state: a review must not mutate the user's saved layout, nor should a
/// narrow viewport spend most of its pixels on navigation chrome.
#[cfg(target_os = "macos")]
fn set_review_capture_mode(app: &AppHandle, visual_id: &str, active: bool) -> Result<()> {
    let window = app
        .get_webview_window("main")
        .context("review capture requires the main Desktop window")?;
    let detail = serde_json::to_string(&json!({
        "active": active,
        "visualId": visual_id,
    }))?;
    window
        .eval(format!(
            "window.__synthVisualReviewCapture={detail};document.documentElement.removeAttribute('data-synth-review-capture-ready');document.documentElement.toggleAttribute('data-synth-review-capture',{active});window.dispatchEvent(new CustomEvent('synth:visual-review-capture',{{detail:window.__synthVisualReviewCapture}}));"
        ))
        .context("set review capture renderer mode")
}

/// The fixed layout delay above permits CSS and WebKit to react to a resize.
/// This acknowledgement closes the remaining cold-start race: React may still
/// be mounting the requested visual when that delay expires.
#[cfg(target_os = "macos")]
async fn wait_for_review_capture_surface(app: &AppHandle, visual_id: &str) -> Result<bool> {
    // Three seconds was enough for a warm app and not for a cold one. On a
    // freshly launched instance the first heavy template -- a trace workstation
    // or an optimizer workspace -- had not mounted before the deadline, so the
    // wait gave up and the capture proceeded with the visual still closed.
    //
    // Waiting longer is safe now that a scope mismatch is an error: the loop
    // still returns the instant the renderer acknowledges, so a surface that is
    // already open pays nothing, and a surface that never opens fails loudly at
    // the end rather than yielding a picture of whatever was on screen.
    //
    // Eight seconds, not twenty. The visuals IPC is single-threaded, so this
    // wait holds it: at twenty seconds a run of failing captures made every
    // other call on the socket return EAGAIN, turning one broken capture into
    // an unusable control plane. Eight covers a cold mount with room to spare
    // and bounds what one bad request can cost everything else.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let window = app
            .get_webview_window("main")
            .context("review capture requires the main Desktop window")?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let sender = std::sync::Arc::new(std::sync::Mutex::new(Some(sender)));
        let callback_sender = std::sync::Arc::clone(&sender);
        window
            .eval_with_callback(
                "document.documentElement.dataset.synthReviewCaptureReady || ''",
                move |value| {
                    if let Some(sender) = callback_sender
                        .lock()
                        .ok()
                        .and_then(|mut sender| sender.take())
                    {
                        let _ = sender.send(value);
                    }
                },
            )
            .context("query review capture renderer readiness")?;
        if let Ok(Ok(value)) =
            tokio::time::timeout(std::time::Duration::from_millis(250), receiver).await
        {
            if value.contains(visual_id) {
                return Ok(true);
            }
        }
        if tokio::time::Instant::now() >= deadline {
            // Some already-focused routes do not rerun their React effect when
            // the capture request repeats. The conservative settle window has
            // elapsed; capture instead of rejecting a valid visual solely for
            // lack of a duplicate acknowledgement -- but report that nothing
            // acknowledged, so the caller can check the surface another way.
            return Ok(false);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[cfg(target_os = "macos")]
fn reset_review_capture_scroll(app: &AppHandle) -> Result<()> {
    let window = app
        .get_webview_window("main")
        .context("review capture requires the main Desktop window")?;
    window
        .eval(
            "window.scrollTo(0,0);document.scrollingElement?.scrollTo(0,0);document.querySelectorAll('*').forEach((element)=>{element.scrollTop=0;element.scrollLeft=0;});",
        )
        .context("reset review capture scroll position")
}

/// The certified review capture: `capture_surface` with the visual scope and a
/// required viewport. Keeping it a caller rather than a second implementation
/// is what stops the review path and the agent-facing one from drifting.
#[cfg(target_os = "macos")]
async fn capture_review_window(app: &AppHandle, body: &Value) -> Result<Value> {
    let visual_id = body
        .get("visualId")
        .or_else(|| body.get("visual_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("review capture requires visualId")?
        .to_string();
    // A review is always taken at an explicit breakpoint, so unlike a general
    // capture it refuses to fall back to the window's current size.
    body.get("width")
        .and_then(Value::as_f64)
        .context("review window width is required")?;
    body.get("height")
        .and_then(Value::as_f64)
        .context("review window height is required")?;
    let mut request = body.clone();
    request["scope"] = json!("visual");
    request["target"] = json!(visual_id);
    capture_surface(app, &request).await
}

#[cfg(not(target_os = "macos"))]
async fn capture_review_window(_app: &AppHandle, _body: &Value) -> Result<Value> {
    anyhow::bail!("UnsupportedCapturePlatform: host webview snapshot requires macOS")
}

/* ── Surface capture ─────────────────────────────────────────────────────────
 *
 * One pipeline behind every screenshot the host takes of itself.
 *
 * `capture_review_window` grew for a single job: photograph one visual, in
 * isolation, at a review viewport. Everything it learned on the way — resize
 * and restore on this side of the IPC so a dying helper cannot strand the
 * user's window, wait for a renderer acknowledgement rather than a fixed
 * sleep, snapshot the app's own WKWebView so no Screen Recording grant and no
 * window visibility are required — applies just as well to photographing a
 * plugin page or the whole app. It was only the *subject* that was hard-wired.
 *
 * So the subject became a parameter. `CaptureScope` says what to photograph;
 * the orchestration below is shared, and `capture_review_window` is now a thin
 * caller of it so the certified review path and the new agent-facing one
 * cannot drift.
 */

/// What a capture is a picture of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CaptureScope {
    /// The app exactly as it stands: real chrome, current route, current
    /// scroll. The default subject for "what does Workshop look like now".
    App,
    /// One plugin destination, with the app's chrome intact around it.
    Plugin(String),
    /// One visual, isolated from the surrounding chrome. The review subject.
    Visual(String),
    /// One element, addressed by `data-testid`, cropped out of an app capture.
    Element(String),
}

/// Plugin destinations the renderer can be routed to. Mirrors the `ALLOWED`
/// list in `dispatch_display_plugins`; a capture must not be able to name a
/// destination the display contract does not admit.
const CAPTURE_PLUGINS: [&str; 7] = [
    "visuals",
    "reports",
    "experiments",
    "optimizers",
    "inventory",
    "inference",
    "computer-use",
];

impl CaptureScope {
    fn parse(scope: &str, target: Option<&str>) -> Result<Self> {
        let target = target.map(str::trim).filter(|value| !value.is_empty());
        match scope {
            "app" => Ok(Self::App),
            "plugin" => {
                let id = target.context("capture scope `plugin` requires a target plugin id")?;
                if !CAPTURE_PLUGINS.contains(&id) {
                    anyhow::bail!(
                        "unknown capture plugin `{id}`; expected one of {}",
                        CAPTURE_PLUGINS.join(", ")
                    );
                }
                Ok(Self::Plugin(id.to_string()))
            }
            "visual" => Ok(Self::Visual(
                target
                    .context("capture scope `visual` requires a target visual id")?
                    .to_string(),
            )),
            "element" => Ok(Self::Element(
                target
                    .context("capture scope `element` requires a target data-testid")?
                    .to_string(),
            )),
            other => anyhow::bail!(
                "unknown capture scope `{other}`; expected app, plugin, visual, or element"
            ),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Plugin(_) => "plugin",
            Self::Visual(_) => "visual",
            Self::Element(_) => "element",
        }
    }

    fn target(&self) -> Option<&str> {
        match self {
            Self::App => None,
            Self::Plugin(id) | Self::Visual(id) | Self::Element(id) => Some(id),
        }
    }

    /// Scroll is app state everywhere except a review, where the subject is the
    /// visual's own top-of-surface and a carried-over scroll offset is noise.
    fn resets_scroll(&self) -> bool {
        matches!(self, Self::Visual(_))
    }

    /// The renderer only has to route somewhere for a scope that names a
    /// destination. `element` crops whatever is already on screen.
    fn routes(&self) -> bool {
        matches!(self, Self::Plugin(_) | Self::Visual(_))
    }
}

/// A CSS-pixel rectangle read from the renderer, before scaling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CaptureRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Crop a captured PNG to one element's box.
///
/// The snapshot is in device pixels and the rectangle arrives in CSS pixels, so
/// the scale factor has to be applied here — a 2x display otherwise crops the
/// top-left quadrant of the intended region and calls it the element. Kept pure
/// and separate from the window plumbing so the arithmetic is testable without
/// a display.
#[cfg(target_os = "macos")]
pub(crate) fn crop_png(bytes: &[u8], rect: CaptureRect, scale: f64) -> Result<Vec<u8>> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().context("decode capture for crop")?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .context("read capture pixels")?;
    if info.bit_depth != png::BitDepth::Eight {
        anyhow::bail!("capture crop supports 8-bit images only");
    }
    let channels = info.color_type.samples();
    let (image_width, image_height) = (info.width as i64, info.height as i64);

    // Round outward: a half-pixel box that rounds inward clips the element's
    // own border, which is exactly the evidence a layout review is looking at.
    let left = ((rect.x * scale).floor() as i64).clamp(0, image_width);
    let top = ((rect.y * scale).floor() as i64).clamp(0, image_height);
    let right = (((rect.x + rect.width) * scale).ceil() as i64).clamp(left, image_width);
    let bottom = (((rect.y + rect.height) * scale).ceil() as i64).clamp(top, image_height);
    let (width, height) = ((right - left) as usize, (bottom - top) as usize);
    if width == 0 || height == 0 {
        anyhow::bail!(
            "element is not visible in the capture: its box resolved to {width}x{height} pixels"
        );
    }

    let stride = info.width as usize * channels;
    let mut cropped = Vec::with_capacity(width * height * channels);
    for row in 0..height {
        let start = (top as usize + row) * stride + left as usize * channels;
        cropped.extend_from_slice(&buffer[start..start + width * channels]);
    }

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width as u32, height as u32);
        encoder.set_color(info.color_type);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().context("write cropped PNG header")?;
        writer
            .write_image_data(&cropped)
            .context("write cropped PNG pixels")?;
    }
    Ok(out)
}

/// Announce a non-review capture to the renderer.
///
/// Deliberately a second, parallel protocol rather than a rename of the review
/// one: `visual` scope keeps emitting exactly what it emitted before, so the
/// certified capture → review → mark_ready chain and the CSS that isolates a
/// reviewed visual are untouched by this addition.
#[cfg(target_os = "macos")]
fn set_capture_mode(app: &AppHandle, scope: &CaptureScope, active: bool) -> Result<()> {
    let window = app
        .get_webview_window("main")
        .context("capture requires the main Desktop window")?;
    let detail = serde_json::to_string(&json!({
        "active": active,
        "scope": scope.name(),
        "target": scope.target(),
        "route": scope.routes(),
    }))?;
    window
        .eval(format!(
            "window.__synthCapture={detail};document.documentElement.removeAttribute('data-synth-capture-ready');document.documentElement.toggleAttribute('data-synth-capture',{active});window.dispatchEvent(new CustomEvent('synth:capture',{{detail:window.__synthCapture}}));"
        ))
        .context("set renderer capture mode")
}

/// Read one JS expression out of the renderer as a string.
#[cfg(target_os = "macos")]
async fn eval_string(
    app: &AppHandle,
    script: &str,
    timeout: std::time::Duration,
) -> Result<String> {
    let window = app
        .get_webview_window("main")
        .context("capture requires the main Desktop window")?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = std::sync::Arc::new(std::sync::Mutex::new(Some(sender)));
    let callback_sender = std::sync::Arc::clone(&sender);
    window
        .eval_with_callback(script, move |value| {
            if let Some(sender) = callback_sender
                .lock()
                .ok()
                .and_then(|mut sender| sender.take())
            {
                let _ = sender.send(value);
            }
        })
        .context("evaluate renderer expression")?;
    tokio::time::timeout(timeout, receiver)
        .await
        .context("renderer did not answer in time")?
        .context("renderer answer channel closed")
}

/// Wait for the renderer to acknowledge that it is showing the requested
/// surface. Same contract as the review path: a bounded wait, then capture
/// anyway, because an already-focused route may not rerun its effect and a
/// valid surface must not be refused for want of a duplicate acknowledgement.
#[cfg(target_os = "macos")]
async fn wait_for_capture_surface(app: &AppHandle, scope: &CaptureScope) -> Result<()> {
    let Some(target) = scope.target() else {
        return Ok(());
    };
    let expected = format!("{}:{target}", scope.name());
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if let Ok(value) = eval_string(
            app,
            "document.documentElement.dataset.synthCaptureReady || ''",
            std::time::Duration::from_millis(250),
        )
        .await
        {
            if value.contains(&expected) {
                return Ok(());
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// The on-screen box of one `data-testid`, in CSS pixels.
#[cfg(target_os = "macos")]
async fn capture_element_rect(app: &AppHandle, testid: &str) -> Result<CaptureRect> {
    // The id travels as a JSON string literal so a quote in it cannot end the
    // selector and run as script.
    let literal = serde_json::to_string(testid)?;
    let script = format!(
        "(()=>{{const el=document.querySelector(`[data-testid=${{CSS.escape?CSS.escape({literal}):{literal}}}]`)||document.querySelector('[data-testid='+JSON.stringify({literal})+']');if(!el)return '';const r=el.getBoundingClientRect();return JSON.stringify({{x:r.x,y:r.y,width:r.width,height:r.height}});}})()"
    );
    let raw = eval_string(app, &script, std::time::Duration::from_secs(2)).await?;
    let trimmed = raw.trim().trim_matches('"').replace("\\\"", "\"");
    if trimmed.is_empty() {
        anyhow::bail!("no element with data-testid `{testid}` is on screen");
    }
    let parsed: Value = serde_json::from_str(&trimmed)
        .with_context(|| format!("element rect for `{testid}` was not JSON: {raw}"))?;
    let number = |key: &str| -> Result<f64> {
        parsed
            .get(key)
            .and_then(Value::as_f64)
            .with_context(|| format!("element rect for `{testid}` is missing {key}"))
    };
    let rect = CaptureRect {
        x: number("x")?,
        y: number("y")?,
        width: number("width")?,
        height: number("height")?,
    };
    if rect.width <= 0.0 || rect.height <= 0.0 {
        anyhow::bail!("element `{testid}` has no on-screen box to capture");
    }
    Ok(rect)
}

/// What the app was showing, and what its own layout audit found, read at the
/// moment of the capture.
///
/// A screenshot proves a state rendered; it does not say the state is
/// defensible. Harvesting both here — inside the same held viewport, before the
/// window is restored — is what makes a capture reviewable evidence rather than
/// an image somebody has to squint at later.
#[cfg(target_os = "macos")]
async fn harvest_capture_evidence(app: &AppHandle) -> (Value, Value) {
    let state = eval_string(
        app,
        r#"JSON.stringify({
            ...JSON.parse(document.documentElement.dataset.synthAppState || '{}'),
            visualRenderError: document.querySelector('[data-testid="visual-invalid"]')?.textContent?.trim() || undefined
        })"#,
        std::time::Duration::from_millis(500),
    )
    .await
    .ok()
    .and_then(|raw| parse_renderer_json(&raw))
    .unwrap_or(Value::Null);
    let audit = eval_string(
        app,
        "(window.__synthCaptureAudit && window.__synthCaptureAudit()) || ''",
        std::time::Duration::from_secs(3),
    )
    .await
    .ok()
    .and_then(|raw| parse_renderer_json(&raw))
    .unwrap_or(Value::Null);
    (state, audit)
}

/// `eval_with_callback` hands back the JS value already serialized, so a string
/// result arrives quoted and escaped. Unwrap one layer before parsing, and fall
/// back to the raw text so a protocol change degrades to "unavailable" rather
/// than to a wrong record.
#[cfg(target_os = "macos")]
fn parse_renderer_json(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "\"\"" {
        return None;
    }
    if let Ok(Value::String(inner)) = serde_json::from_str::<Value>(trimmed) {
        if let Ok(parsed) = serde_json::from_str::<Value>(&inner) {
            return Some(parsed);
        }
    }
    serde_json::from_str::<Value>(trimmed).ok()
}

/// The caller names the file; this side refuses to write outside its own data
/// root. Both ends are canonicalized so `..` segments cannot slip past the
/// prefix check.
fn resolve_capture_output(body: &Value) -> Result<PathBuf> {
    let output = body
        .get("outputPath")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("capture requires outputPath")?;
    let output = PathBuf::from(output);
    if !output.is_absolute() {
        anyhow::bail!("capture outputPath must be absolute");
    }
    let root = VISUALS_DATA_ROOT
        .get()
        .context("visuals IPC data root is not initialized")?;
    let parent = output
        .parent()
        .context("capture outputPath requires a parent directory")?;
    fs::create_dir_all(parent)?;
    let canonical_root = fs::canonicalize(root).context("resolve visuals data root")?;
    let canonical_parent = fs::canonicalize(parent).context("resolve capture output directory")?;
    if !canonical_parent.starts_with(&canonical_root) {
        anyhow::bail!(
            "capture outputPath must stay under the visuals data root {}",
            canonical_root.display()
        );
    }
    Ok(output)
}

/// Resize if asked, snapshot the app's own webview, restore — one call.
///
/// The snapshot needs no Screen Recording TCC grant, no window-identity
/// resolution, and no visibility: it captures correctly while the app is
/// occluded or backgrounded. Holding resize and restore on this side means a
/// helper that dies mid-capture cannot strand the user's window at the capture
/// size. A capture with no requested viewport never resizes at all, because
/// "what does the app look like right now" must not begin by changing it.
#[cfg(target_os = "macos")]
pub(crate) async fn capture_surface(app: &AppHandle, body: &Value) -> Result<Value> {
    let scope = CaptureScope::parse(
        body.get("scope").and_then(Value::as_str).unwrap_or("app"),
        body.get("target")
            .or_else(|| body.get("visualId"))
            .or_else(|| body.get("visual_id"))
            .and_then(Value::as_str),
    )?;
    let output = resolve_capture_output(body)?;
    let viewport = match (
        body.get("width").and_then(Value::as_f64),
        body.get("height").and_then(Value::as_f64),
    ) {
        (Some(width), Some(height)) => Some((width, height)),
        (None, None) => None,
        _ => anyhow::bail!("a capture viewport needs both width and height"),
    };

    // Enter capture mode before resizing so React has the entire settle window
    // to route to the requested surface and lay it out.
    match &scope {
        CaptureScope::Visual(id) => set_review_capture_mode(app, id, true)?,
        other => set_capture_mode(app, other, true)?,
    }
    let leave_capture_mode = |app: &AppHandle| match &scope {
        CaptureScope::Visual(id) => set_review_capture_mode(app, id, false),
        other => set_capture_mode(app, other, false),
    };

    let geometry = match capture_window_geometry(app, viewport) {
        Ok(geometry) => geometry,
        Err(error) => {
            let _ = leave_capture_mode(app);
            return Err(error);
        }
    };
    // Do not `?` between here and the restore: the window may be resized, and
    // any early return from this span leaves it that way.
    tokio::time::sleep(REVIEW_CAPTURE_SETTLE).await;

    // Whether the renderer positively acknowledged the requested visual. The
    // scope check below needs this because the two ways a visual can be "open"
    // are different states: `openArtifactId` is the artifact pane, set by
    // toggleArtifact, while a review capture drives the visuals page and
    // publishes `synthReviewCaptureReady`. Comparing only against the pane
    // rejected correct captures of a visual reviewed through the page.
    let mut review_acknowledged = false;
    let snapshot = match &scope {
        CaptureScope::Visual(id) => wait_for_review_capture_surface(app, id)
            .await
            .map(|acknowledged| {
                review_acknowledged = acknowledged;
            }),
        other => wait_for_capture_surface(app, other).await,
    }
    .and_then(|()| {
        if scope.resets_scroll() {
            reset_review_capture_scroll(app)
        } else {
            Ok(())
        }
    });
    let mut cropped_to = None;
    let snapshot = match snapshot {
        Ok(()) => match app.get_webview_window("main") {
            Some(window) => {
                let first =
                    crate::visuals::snapshot::capture_webview_png(&window, REVIEW_CAPTURE_TIMEOUT)
                        .await;
                let attempt = match first {
                    Err(_) => {
                        crate::visuals::snapshot::capture_webview_png(
                            &window,
                            REVIEW_CAPTURE_RETRY_TIMEOUT,
                        )
                        .await
                    }
                    ok => ok,
                };
                match attempt {
                    Ok(bytes) => match &scope {
                        CaptureScope::Element(testid) => {
                            match capture_element_rect(app, testid).await {
                                Ok(rect) => {
                                    cropped_to = Some(rect);
                                    crop_png(&bytes, rect, geometry.scale)
                                }
                                Err(error) => Err(error),
                            }
                        }
                        _ => Ok(bytes),
                    },
                    Err(error) => Err(error),
                }
            }
            None => Err(anyhow::anyhow!("capture requires the main Desktop window")),
        },
        Err(error) => Err(error),
    };

    // Before the restore: the audit must measure the viewport that was
    // photographed, not the one the window goes back to.
    let (app_state, audit) = match &snapshot {
        Ok(_) => harvest_capture_evidence(app).await,
        Err(_) => (Value::Null, Value::Null),
    };

    let restore = match viewport {
        Some(_) => resize_review_window(
            app,
            &json!({"width": geometry.previous.0, "height": geometry.previous.1}),
        )
        .map(|_| ()),
        // Nothing was resized, so there is nothing to put back.
        None => Ok(()),
    };
    let capture_mode_restore = leave_capture_mode(app);
    let written = match &snapshot {
        Ok(bytes) => fs::write(&output, bytes).context("write capture PNG"),
        Err(_) => Ok(()),
    };

    // A failed restore leaves the user's window at the capture viewport; it has
    // to reach the caller even when the capture itself failed.
    match (snapshot, restore, capture_mode_restore, written) {
        (Err(capture), Err(restore), _, _) => Err(anyhow::anyhow!(
            "{capture:#}; additionally the Desktop window was not restored: {restore:#}"
        )),
        (Err(capture), Ok(_), _, _) => Err(capture),
        (Ok(_), Err(restore), _, _) => Err(anyhow::anyhow!(
            "captured but failed to restore Desktop window: {restore:#}"
        )),
        (Ok(_), Ok(_), Err(restore), _) => Err(anyhow::anyhow!(
            "captured but failed to restore the renderer layout: {restore:#}"
        )),
        (Ok(_), Ok(_), Ok(()), Err(write)) => Err(write),
        (Ok(bytes), Ok(_), Ok(()), Ok(())) => {
            // A scoped capture must photograph the surface it was asked for.
            //
            // `scope: visual` asks the renderer to open a visual and then
            // photographs the window. When that open silently does not happen
            // -- a full page reload had just reset the route to chat -- the
            // capture returned a PNG of the chat view, `ok`, with the requested
            // id in `target` and `openVisualId: null` two lines below it. Every
            // claim built on these captures assumes the picture is of what its
            // name says, so a mismatch has to be an error rather than something
            // a reader might notice in the metadata.
            if let CaptureScope::Visual(target) = &scope {
                let opened = app_state
                    .get("openVisualId")
                    .and_then(Value::as_str)
                    .unwrap_or("none");
                if !review_acknowledged && opened != target.as_str() {
                    return Err(anyhow::anyhow!(
                        "capture_scope_mismatch: asked for visual `{target}` but the renderer \
                         had `{opened}` open; the image would not be of the requested surface"
                    ));
                }
            }
            let (width, height) = png_dimensions(&bytes).unwrap_or((0, 0));
            let diagnostics = crate::instance::diagnostics();
            Ok(json!({
                "schemaVersion": "synth.surface-capture.v1",
                "path": output.to_string_lossy(),
                "digest": format!("sha256:{:x}", sha2::Sha256::digest(&bytes)),
                "scope": scope.name(),
                "target": scope.target(),
                "capturedAt": chrono::Utc::now().to_rfc3339(),
                "instance": {
                    "name": diagnostics.name,
                    "mode": diagnostics.mode,
                    "appVersion": diagnostics.app_version,
                    "sourceRevision": diagnostics.source_revision,
                    "buildRevision": diagnostics.build_revision,
                },
                "appState": app_state,
                "audit": audit,
                "width": width,
                "height": height,
                "previous": {"width": geometry.previous.0, "height": geometry.previous.1},
                "current": {"width": geometry.current.0, "height": geometry.current.1},
                "resized": viewport.is_some(),
                "croppedTo": cropped_to.map(|rect| json!({
                    "x": rect.x, "y": rect.y, "width": rect.width, "height": rect.height
                })),
                "scaleFactor": geometry.scale,
                "windowLabel": geometry.label,
                "processId": std::process::id(),
                "captureMode": "host-webview-snapshot",
                "restored": true,
            }))
        }
    }
}

#[cfg(target_os = "macos")]
struct CaptureGeometry {
    previous: (u64, u64),
    current: (u64, u64),
    scale: f64,
    label: String,
}

/// Resize to the requested viewport, or just report the window as it stands.
#[cfg(target_os = "macos")]
fn capture_window_geometry(
    app: &AppHandle,
    viewport: Option<(f64, f64)>,
) -> Result<CaptureGeometry> {
    let window = app
        .get_webview_window("main")
        .context("capture requires the main Desktop window")?;
    let scale = window.scale_factor().context("read display scale factor")?;
    let label = window.label().to_string();
    let Some((width, height)) = viewport else {
        let size = window
            .inner_size()
            .context("read capture window size")?
            .to_logical::<f64>(scale);
        let logical = (size.width.round() as u64, size.height.round() as u64);
        return Ok(CaptureGeometry {
            previous: logical,
            current: logical,
            scale,
            label,
        });
    };
    let resize = resize_review_window(app, &json!({"width": width, "height": height}))?;
    let read = |key: &str, field: &str| -> u64 {
        resize
            .get(key)
            .and_then(|value| value.get(field))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    Ok(CaptureGeometry {
        previous: (read("previous", "width"), read("previous", "height")),
        current: (read("current", "width"), read("current", "height")),
        scale,
        label,
    })
}

#[cfg(not(target_os = "macos"))]
pub(crate) async fn capture_surface(_app: &AppHandle, _body: &Value) -> Result<Value> {
    anyhow::bail!("UnsupportedCapturePlatform: host webview snapshot requires macOS")
}

#[cfg(target_os = "macos")]
fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let reader = decoder.read_info().ok()?;
    let info = reader.info();
    Some((info.width, info.height))
}

fn json_field<'a>(body: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    body.get(camel).or_else(|| body.get(snake))
}

async fn present_session(app: &AppHandle, core: &CoreRuntime, body: Value) -> Result<Value> {
    let session_id = json_field(&body, "sessionId", "session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("sessionId is required")?
        .to_owned();
    let title = json_field(&body, "title", "title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let emotion = PresentationField::from_json(json_field(&body, "emotion", "emotion"))?;
    let summary = PresentationField::from_json(json_field(&body, "summary", "summary"))?;
    if title.is_none()
        && matches!(emotion, PresentationField::Unchanged)
        && matches!(summary, PresentationField::Unchanged)
    {
        anyhow::bail!("session_present requires title, emotion, or summary");
    }
    if let Some(title) = title {
        if let Some(manager) = app.try_state::<Arc<CodexManager>>() {
            manager.set_thread_name(app, &session_id, title).await?;
        } else {
            let mutation = core
                .sessions()
                .set_title(session_id.clone(), title, SessionTitleOrigin::Manual)
                .await?;
            core.broadcast_committed(mutation.event);
        }
    }
    if !matches!(emotion, PresentationField::Unchanged)
        || !matches!(summary, PresentationField::Unchanged)
    {
        if let Some(manager) = app.try_state::<Arc<CodexManager>>() {
            return manager
                .set_presentation(app, &session_id, emotion, summary)
                .await;
        }
        let mutation = core
            .sessions()
            .set_presentation(session_id.clone(), emotion, summary)
            .await?;
        core.broadcast_committed(mutation.event.clone());
        return Ok(json!({
            "sessionId": session_id,
            "title": mutation.value.title,
            "emotion": mutation.value.metadata.get("presentationEmotion"),
            "summary": mutation.value.metadata.get("presentationSummary"),
        }));
    }
    let session = core
        .sessions()
        .get(session_id.clone())
        .await?
        .context("session not found")?;
    Ok(json!({
        "sessionId": session.id,
        "title": session.title,
        "emotion": session.metadata.get("presentationEmotion"),
        "summary": session.metadata.get("presentationSummary"),
    }))
}

fn query_json(query: &str) -> Value {
    let mut object = serde_json::Map::new();
    for item in query.split('&').filter(|item| !item.is_empty()) {
        let (key, value) = item.split_once('=').unwrap_or((item, ""));
        let value = percent_decode(value);
        let parsed = if matches!(key, "limit" | "offset") {
            value
                .parse::<i64>()
                .map(Value::from)
                .unwrap_or(Value::String(value))
        } else {
            Value::String(value)
        };
        object.insert(key.to_string(), parsed);
    }
    Value::Object(object)
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(hex);
                index += 3;
                continue;
            }
        }
        out.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The registered container's own origin, proved local before anything is
/// fetched from it. Frame media resolves against this and nothing else.
pub(crate) fn validated_loopback_rollout_base(base: &str) -> Result<String> {
    let trimmed = base.trim_end_matches('/');
    let parsed = reqwest::Url::parse(trimmed).context("invalid container base URL")?;
    let local_host = matches!(
        parsed.host_str(),
        Some("127.0.0.1") | Some("localhost") | Some("::1") | Some("[::1]")
    );
    if parsed.scheme() != "http" || !local_host {
        anyhow::bail!("live rollout execution is limited to registered loopback HTTP containers");
    }
    Ok(trimmed.to_string())
}

pub(crate) async fn get_rollout_status(
    client: &reqwest::Client,
    base: &str,
    rollout_id: &str,
) -> Result<Option<Value>> {
    let response = client
        .get(format!("{base}/rollouts/{rollout_id}"))
        .send()
        .await
        .context("GET authoritative rollout status")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    Ok(Some(response.error_for_status()?.json::<Value>().await?))
}

/// Open a durable receipt for a rollout launch, returning its id.
///
/// Best-effort by design: a receipt is a recovery aid, and failing the user's
/// eval because bookkeeping could not be written would trade a rare wrong
/// restart for a certain lost run. A missing receipt degrades recovery to
/// "restartable", which is the pre-existing behaviour.
async fn begin_rollout_receipt(
    core: &CoreRuntime,
    session_id: Option<&str>,
    rollout_id: &str,
    request: &Value,
) -> Option<String> {
    // Without a session there is nothing to attribute the action to, and
    // recovery scopes receipts by run. Skip rather than write an orphan row.
    let session_id = session_id?.to_owned();
    let rollout_id = rollout_id.to_owned();
    let request = request.clone();
    core.storage()
        .database()
        .run_transaction(move |conn| {
            let run_id: Option<String> = conn
                .query_row(
                    "SELECT active_run_id FROM sessions WHERE id = ?1",
                    rusqlite::params![session_id],
                    |row| row.get(0),
                )
                .ok()
                .flatten();
            let receipt = crate::recovery::receipts::begin(
                conn,
                &session_id,
                run_id.as_deref(),
                // The container owns rollout identity, so the caller-stable
                // rollout id is already the idempotency key for this action.
                &format!("rollout:{rollout_id}"),
                "container.rollout.start",
                &request,
            )?;
            Ok(receipt.tool_call_id)
        })
        .await
        .ok()
}

async fn settle_rollout_receipt(
    core: &CoreRuntime,
    tool_call_id: Option<&str>,
    external_object_id: Option<&str>,
) {
    let Some(tool_call_id) = tool_call_id.map(str::to_owned) else {
        return;
    };
    let external_object_id = external_object_id.map(str::to_owned);
    let _ = core
        .storage()
        .database()
        .run(move |conn| {
            crate::recovery::receipts::settle(conn, &tool_call_id, external_object_id.as_deref())
        })
        .await;
}

fn rollout_started(state: &Value) -> bool {
    state.get("started").and_then(Value::as_bool) == Some(true)
        || state.get("terminated").and_then(Value::as_bool) == Some(true)
        || matches!(
            state.get("status").and_then(Value::as_str),
            Some("running" | "completed" | "failed" | "cancelled" | "truncated")
        )
}

pub(crate) async fn start_rollout_idempotently(
    execution_client: &reqwest::Client,
    recovery_client: &reqwest::Client,
    base: &str,
    rollout_id: &str,
    start_body: &Value,
) -> Result<(Value, bool)> {
    let send = || {
        execution_client
            .post(format!("{base}/rollouts"))
            .json(start_body)
    };
    match send().send().await {
        Ok(response) => return Ok((response.error_for_status()?.json::<Value>().await?, false)),
        Err(first_error) => {
            if let Some(state) = get_rollout_status(recovery_client, base, rollout_id).await? {
                if rollout_started(&state) {
                    return Ok((state, true));
                }
            }

            // The request either never reached the façade or left an outcome gap. Replaying the
            // exact immutable identity is safe: Containers owns the idempotency boundary.
            match send().send().await {
                Ok(response) => {
                    return Ok((response.error_for_status()?.json::<Value>().await?, true));
                }
                Err(retry_error) => {
                    if let Some(state) =
                        get_rollout_status(recovery_client, base, rollout_id).await?
                    {
                        if rollout_started(&state) {
                            return Ok((state, true));
                        }
                    }
                    return Err(retry_error).with_context(|| {
                        format!(
                            "idempotent rollout start failed after reconnect; first error: {first_error}"
                        )
                    });
                }
            }
        }
    }
}

#[derive(Debug)]
struct ScriptedRollout {
    rollout_id: String,
    state: Value,
    events: Value,
    stream_id: Option<String>,
}

/// C1-08: normalized Containers prepare → declared poll until `stream.subscribed` → start.
async fn run_one_scripted_rollout(
    client: &reqwest::Client,
    base: &str,
    seed: i64,
    actions: &[String],
    requested_rollout_id: Option<String>,
    diagnostics: &crate::container_stream::StreamDiagnostics,
) -> Result<ScriptedRollout> {
    let telemetry = authoritative_poll_telemetry();
    refuse_auto_transport(&telemetry)?;
    let requested_rollout_id =
        requested_rollout_id.unwrap_or_else(|| format!("roll_{}", Uuid::new_v4().simple()));
    let prepare = client
        .post(format!("{base}/rollouts/prepare"))
        .json(&json!({ "rollout_id": requested_rollout_id, "telemetry": telemetry }))
        .send()
        .await
        .context("POST /rollouts/prepare")?;
    let prepare_status = prepare.status();
    if !prepare_status.is_success() {
        anyhow::bail!(
            "normalized Containers POST /rollouts/prepare failed with {prepare_status}; native benchmark routes must be folded inside Containers"
        );
    }
    let prepared = prepare.json::<Value>().await?;
    let rollout_id = prepared
        .get("rollout_id")
        .and_then(Value::as_str)
        .context("prepare omitted rollout_id")?
        .to_string();
    if rollout_id != requested_rollout_id {
        anyhow::bail!("prepare returned a different rollout_id than the caller-stable id");
    }
    let prepared_stream = declared_stream_descriptor(&prepared)?
        .context("prepare omitted stream descriptor; refusing to guess /events")?;
    let poll_url = resolve_declared_url(base, &declared_poll_url(&prepared_stream)?)?;
    let rollout_diagnostics = diagnostics.clone().with_rollout(&rollout_id);
    wait_for_stream_subscribed(
        client,
        &poll_url,
        SUBSCRIBE_READY_TIMEOUT,
        &rollout_diagnostics,
    )
    .await?;

    let mut state = client
        .post(format!("{base}/rollouts"))
        .json(&json!({
            "rollout_id": rollout_id,
            "seed": seed,
            "telemetry": telemetry,
            "slot": LIVE_EVAL_SLOT,
        }))
        .send()
        .await?
        .error_for_status()
        .context("POST /rollouts after stream.subscribed")?
        .json::<Value>()
        .await?;
    step_until_done(client, base, &rollout_id, &mut state, actions).await?;
    let events = client
        .get(&poll_url)
        .query(&[("after", "0")])
        .send()
        .await?
        .error_for_status()
        .context("GET declared transports.poll.url")?
        .json::<Value>()
        .await?;
    let stream_id = prepared_stream
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            state
                .get("stream")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    Ok(ScriptedRollout {
        rollout_id,
        state,
        events,
        stream_id,
    })
}

async fn step_until_done(
    client: &reqwest::Client,
    base: &str,
    rollout_id: &str,
    state: &mut Value,
    actions: &[String],
) -> Result<()> {
    for action in actions {
        if state
            .get("terminated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || state
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            break;
        }
        *state = client
            .post(format!("{base}/rollouts/{rollout_id}/step"))
            .json(&json!({"action": action}))
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
    }
    Ok(())
}

fn require_scripted_stream_slot(body: &Value) -> Result<()> {
    let requested = body
        .get("input")
        .or_else(|| body.get("slot"))
        .or_else(|| body.get("streamSlot"))
        .or_else(|| body.get("stream_slot"))
        .and_then(Value::as_str)
        .unwrap_or(LIVE_EVAL_SLOT);
    assert_live_eval_slot(requested)?;
    if requested != LIVE_EVAL_SLOT {
        anyhow::bail!(
            "visuals IPC scripted rollouts bind input \"{LIVE_EVAL_SLOT}\", not \"{requested}\""
        );
    }
    Ok(())
}

/// One health interpretation for every registry write. HTTP success alone is
/// not readiness: a service answering `200 {"ok": false}` must be recorded as
/// unhealthy, or it passes the health half of the prepare preflight.
async fn probe_health(client: &reqwest::Client, base: &str) -> (&'static str, Value) {
    use crate::container_capabilities::{observed_status, READY_STATUS, UNHEALTHY_STATUS};
    match client.get(format!("{base}/health")).send().await {
        Ok(response) => {
            let code = response.status();
            let payload = response.json::<Value>().await.unwrap_or(json!({}));
            let status = observed_status(code.as_u16(), &payload);
            (
                status,
                json!({"ok": status == READY_STATUS, "status": code.as_u16(), "payload": payload}),
            )
        }
        Err(error) => (
            UNHEALTHY_STATUS,
            json!({"ok": false, "error": error.to_string()}),
        ),
    }
}

async fn register_hydrated_container(
    core: &CoreRuntime,
    request: ContainerRegisterRequest,
) -> Result<crate::data::ContainerDeployment> {
    if !(request.base_url.starts_with("http://") || request.base_url.starts_with("https://")) {
        anyhow::bail!("container baseUrl must start with http:// or https://");
    }
    let base = request.base_url.trim_end_matches('/');
    let client = crate::http::http_client_with_timeout(limits::CONTAINER_PROBE_TIMEOUT);
    let (status, health) = probe_health(&client, base).await;
    let mut info = None;
    for route in ["info", "metadata"] {
        if let Ok(response) = client.get(format!("{base}/{route}")).send().await {
            if response.status().is_success() {
                info = response.json::<Value>().await.ok();
                if info.is_some() {
                    break;
                }
            }
        }
    }
    let classified = info
        .as_ref()
        .and_then(|value| classify_live_eval_family(value, request.task_family.as_deref()))
        .or_else(|| classify_live_eval_family(&json!({}), request.task_family.as_deref()));
    let family = observed_task_family(info.as_ref(), classified, request.task_family.as_deref());
    let mut metadata = request
        .metadata
        .clone()
        .unwrap_or_else(|| json!({}))
        .as_object()
        .cloned()
        .unwrap_or_default();
    metadata.insert("hydratedAt".into(), json!(chrono::Utc::now().to_rfc3339()));
    metadata.insert(
        "contractHint".into(),
        json!(if info.is_some() {
            "info"
        } else {
            "health-only"
        }),
    );
    if let Some(family) = classified {
        let policy_refs = metadata.get("policyRefs").cloned();
        metadata.insert(
            "liveEval".into(),
            live_eval_bind_metadata(
                family,
                info.as_ref().unwrap_or(&json!({})),
                policy_refs.as_ref(),
            )?,
        );
    }
    let declared = crate::synth_config::container_capability_declaration(base).unwrap_or_default();
    crate::container_capabilities::write_capability_metadata(
        &mut metadata,
        info.as_ref(),
        declared.as_ref(),
        true,
        chrono::Utc::now(),
    );
    if let Some(value) = info {
        metadata.insert("info".into(), value);
    }
    for (route, key) in [
        ("task_catalog", "taskCatalog"),
        ("task_info", "taskInfo"),
        ("program", "program"),
        ("dataset", "dataset"),
    ] {
        if let Ok(response) = client.get(format!("{base}/{route}")).send().await {
            if response.status().is_success() {
                if let Ok(value) = response.json::<Value>().await {
                    metadata.insert(key.into(), value);
                }
            }
        }
    }
    core.register_container(
        request,
        status.into(),
        health,
        Value::Object(metadata),
        family,
    )
    .await
}

/// Preserve an explicitly advertised service family when it is not one of the
/// visual-template families.  This is an observed capability, never an
/// inference from a name, port, or URL.
fn observed_task_family(
    info: Option<&Value>,
    classified: Option<crate::visuals::LiveEvalFamily>,
    requested: Option<&str>,
) -> Option<String> {
    classified
        .map(|family| family.as_str().to_string())
        .or_else(|| {
            info.and_then(|value| {
                value
                    .get("env_family")
                    .or_else(|| value.get("task_family"))
                    .or_else(|| value.get("runtime_family"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
        .or_else(|| requested.map(str::to_string))
}

pub async fn dispatch(method: &str, path: &str, body: Value, core: &CoreRuntime) -> Result<Value> {
    let registry = core.visuals();
    let reports = core.reports();
    match (method, path) {
        ("GET", "/health") => Ok(json!({"ok": true, "service": "synth-visuals-ipc"})),
        ("GET", "/v1/reports") => {
            let query: ReportQuery = serde_json::from_value(body.clone()).unwrap_or_default();
            Ok(json!({"reports": reports.list(query).await?}))
        }
        ("POST", "/v1/reports") => {
            let request: ReportCreateRequest = serde_json::from_value(body)?;
            let (report, event) = reports.create(request).await?;
            Ok(json!({"report": report, "event": event}))
        }
        ("GET", path) if path.starts_with("/v1/report-seals/") => {
            let digest = path.trim_start_matches("/v1/report-seals/");
            Ok(json!({"bundle": reports.get_seal(digest.to_string()).await?}))
        }
        ("GET", "/v1/report-seals") => {
            let report_id = body
                .get("report_id")
                .or_else(|| body.get("reportId"))
                .and_then(Value::as_str)
                .map(str::to_string);
            Ok(json!({"seals": reports.list_seals(report_id).await?}))
        }
        ("GET", "/v1/report-visibility-requests") => {
            let report_id = body
                .get("report_id")
                .or_else(|| body.get("reportId"))
                .and_then(Value::as_str)
                .map(str::to_string);
            Ok(json!({"requests": reports.list_visibility_requests(report_id).await?}))
        }
        ("POST", path)
            if path.starts_with("/v1/reports/") && path.ends_with("/visibility-requests") =>
        {
            let id = path
                .trim_start_matches("/v1/reports/")
                .trim_end_matches("/visibility-requests");
            let request: ReportVisibilityRequestCreate = serde_json::from_value(body)?;
            Ok(json!({"request": reports.request_visibility(id.to_string(), request).await?}))
        }
        ("POST", path) if path.starts_with("/v1/reports/") && path.ends_with("/archive") => {
            let id = path
                .trim_start_matches("/v1/reports/")
                .trim_end_matches("/archive");
            Ok(json!({"report": reports.set_archived(id.to_string(), true).await?}))
        }
        ("POST", path) if path.starts_with("/v1/reports/") && path.ends_with("/restore") => {
            let id = path
                .trim_start_matches("/v1/reports/")
                .trim_end_matches("/restore");
            Ok(json!({"report": reports.set_archived(id.to_string(), false).await?}))
        }
        ("GET", path) if path.starts_with("/v1/reports/") && path.ends_with("/experiments") => {
            let id = path
                .trim_start_matches("/v1/reports/")
                .trim_end_matches("/experiments");
            Ok(json!({"experiments": reports.list_experiments(id.to_string()).await?}))
        }
        ("POST", path) if path.starts_with("/v1/reports/") && path.ends_with("/experiments") => {
            let id = path
                .trim_start_matches("/v1/reports/")
                .trim_end_matches("/experiments");
            let request: ExperimentRecordUpsert = serde_json::from_value(body)?;
            Ok(json!({"experiment": reports.upsert_experiment(id.to_string(), request).await?}))
        }
        ("GET", path) if path.starts_with("/v1/reports/") && path.ends_with("/log") => {
            let id = path
                .trim_start_matches("/v1/reports/")
                .trim_end_matches("/log");
            Ok(json!({"entries": reports.list_research_log(id.to_string()).await?}))
        }
        ("POST", path) if path.starts_with("/v1/reports/") && path.ends_with("/log") => {
            let id = path
                .trim_start_matches("/v1/reports/")
                .trim_end_matches("/log");
            let request: ResearchLogAppend = serde_json::from_value(body)?;
            Ok(json!({"entry": reports.append_research_log(id.to_string(), request).await?}))
        }
        ("GET", path) if path.starts_with("/v1/reports/") && path.ends_with("/revision") => {
            let id = path
                .trim_start_matches("/v1/reports/")
                .trim_end_matches("/revision");
            let revision = body.get("revision").and_then(Value::as_i64);
            Ok(json!({"revision": reports.get_revision(id.to_string(), revision).await?}))
        }
        ("POST", path) if path.starts_with("/v1/reports/") && path.ends_with("/traces") => {
            let id = path
                .trim_start_matches("/v1/reports/")
                .trim_end_matches("/traces");
            let mut request: ReportAttachTrace = serde_json::from_value(body)?;
            // Never trust an MCP caller's projection bytes. Resolve the
            // projection from a validated, self-contained Trace V5 bundle or
            // keep the evidence explicitly unverified when no trusted bundle
            // is available.
            request.projection = None;
            let projection_verified = if let Ok(resolved) = core
                .data()
                .resolve_trace_projection(request.trace_digest.clone(), "rollout-inspector".into())
                .await
            {
                request.projection = Some(resolved.payload);
                if request.trace_id.is_none() {
                    request.trace_id = Some(resolved.trace_digest);
                }
                true
            } else {
                false
            };
            let (report, event) = reports
                .attach_trace(id.to_string(), request, projection_verified)
                .await?;
            Ok(json!({"report": report, "event": event}))
        }
        ("POST", path) if path.starts_with("/v1/reports/") && path.ends_with("/seal") => {
            let id = path
                .trim_start_matches("/v1/reports/")
                .trim_end_matches("/seal");
            let revision = body
                .get("revision")
                .and_then(Value::as_i64)
                .context("seal requires exact revision")?;
            let (seal, event) = reports.seal(id.to_string(), revision).await?;
            Ok(json!({"seal": seal, "event": event}))
        }
        ("GET", path) if path.starts_with("/v1/reports/") => {
            let id = path.trim_start_matches("/v1/reports/");
            Ok(json!({"report": reports.get(id.to_string()).await?}))
        }
        ("POST", path) if path.starts_with("/v1/reports/") => {
            let id = path.trim_start_matches("/v1/reports/");
            let request: ReportUpdateRequest = serde_json::from_value(body)?;
            let (report, event) = reports.update(id.to_string(), request).await?;
            Ok(json!({"report": report, "event": event}))
        }
        ("GET", "/v1/containers") => {
            Ok(json!({"containers": core.data().list_containers().await?}))
        }
        ("POST", "/v1/containers/ensure") => {
            let manifest_path = body
                .get("manifestPath")
                .or_else(|| body.get("manifest_path"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!(
                    "manifestPath required; container resolution is not coupled to the active workspace"
                ))?;
            let manifest_path = std::path::PathBuf::from(manifest_path);
            if !manifest_path.is_absolute() {
                anyhow::bail!("manifestPath must be absolute");
            }
            if manifest_path.file_name().and_then(|value| value.to_str())
                != Some("workshop.containers.toml")
            {
                anyhow::bail!("manifestPath must name workshop.containers.toml");
            }
            let manifest_path = manifest_path.canonicalize().with_context(|| {
                format!(
                    "canonicalize container manifest {}",
                    manifest_path.display()
                )
            })?;
            let spec_id = body
                .get("specId")
                .or_else(|| body.get("spec_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("specId required"))?;
            let spec = crate::optimizers::workspace_recipe::load_container_specs_from_manifest(
                &manifest_path,
            )?
            .into_iter()
            .find(|candidate| candidate.id == spec_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "container spec `{spec_id}` is not declared in {}",
                    manifest_path.display()
                )
            })?;
            let origin = spec.origin.to_json();
            let ensured = crate::optimizers::container_lifecycle::ensure_spec(
                core.storage().database(),
                &spec,
            )
            .await?;
            Ok(json!({
                "containerId": ensured.container_id,
                "baseUrl": ensured.base_url,
                "specId": ensured.spec_id,
                "locality": ensured.locality.as_str(),
                "declarationOrigin": origin,
            }))
        }
        ("POST", "/v1/containers") => {
            let request: ContainerRegisterRequest = serde_json::from_value(body)?;
            let container = register_hydrated_container(core, request).await?;
            Ok(json!({
                "container": container,
                "liveEval": container.metadata.get("liveEval").cloned().unwrap_or(Value::Null),
            }))
        }
        ("GET", path)
            if path.starts_with("/v1/containers/")
                && path != "/v1/containers/ensure"
                && path != "/v1/containers/resolve_declaration"
                && !path.ends_with("/probe")
                && !path.ends_with("/reconcile")
                && !path.ends_with("/restart")
                && !path.ends_with("/stop")
                && !path.contains("/rollouts/") =>
        {
            let id = path.trim_start_matches("/v1/containers/");
            Ok(json!({"container": core.data().get_container(id.to_string()).await?}))
        }
        ("POST", path) if path.starts_with("/v1/containers/") && path.ends_with("/probe") => {
            let id = path
                .trim_start_matches("/v1/containers/")
                .trim_end_matches("/probe")
                .trim_end_matches('/');
            let container = core.data().get_container(id.to_string()).await?;
            let base = container
                .base_url
                .as_deref()
                .context("container has no base URL")?
                .trim_end_matches('/');
            let client = crate::http::http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(limits::CONTAINER_PROBE_TIMEOUT)
                .build()?;
            let (status, health) = probe_health(&client, base).await;
            // Same discovery order as register/hydrate: `/info` first, then the
            // `/metadata` fallback, so a probe cannot see fewer capabilities
            // than registration did.
            let mut info = None;
            for route in ["info", "metadata"] {
                if let Ok(response) = client.get(format!("{base}/{route}")).send().await {
                    if response.status().is_success() {
                        info = response.json::<Value>().await.ok();
                        if info.is_some() {
                            break;
                        }
                    }
                }
            }
            let mut metadata = container.metadata.as_object().cloned().unwrap_or_default();
            metadata.insert("hydratedAt".into(), json!(chrono::Utc::now().to_rfc3339()));
            // A probe refreshes the typed capability projection without
            // mutating the remote workload: `/health` and `/info` only.
            let declared =
                crate::synth_config::container_capability_declaration(base).unwrap_or_default();
            crate::container_capabilities::write_capability_metadata(
                &mut metadata,
                info.as_ref(),
                declared.as_ref(),
                true,
                chrono::Utc::now(),
            );
            if let Some(value) = info.clone() {
                metadata.insert("info".into(), value);
            }
            let classified = info
                .as_ref()
                .and_then(|value| {
                    classify_live_eval_family(value, container.task_family.as_deref())
                })
                .or_else(|| {
                    classify_live_eval_family(&json!({}), container.task_family.as_deref())
                });
            if let Some(family) = classified {
                let policy_refs = metadata
                    .get("liveEval")
                    .and_then(|value| value.get("policyRefs"))
                    .cloned();
                metadata.insert(
                    "liveEval".into(),
                    live_eval_bind_metadata(
                        family,
                        info.as_ref().unwrap_or(&json!({})),
                        policy_refs.as_ref(),
                    )?,
                );
            }
            for (route, key) in [
                ("task_catalog", "taskCatalog"),
                ("task_info", "taskInfo"),
                ("program", "program"),
                ("dataset", "dataset"),
            ] {
                if let Ok(response) = client.get(format!("{base}/{route}")).send().await {
                    if response.status().is_success() {
                        if let Ok(value) = response.json::<Value>().await {
                            metadata.insert(key.into(), value);
                        }
                    }
                }
            }
            let family =
                observed_task_family(info.as_ref(), classified, container.task_family.as_deref());
            let live = status == crate::container_capabilities::READY_STATUS
                && health.get("ok").and_then(Value::as_bool) != Some(false);
            crate::optimizers::container_lifecycle::stamp_metadata_freshness(
                &mut metadata,
                live,
                &chrono::Utc::now().to_rfc3339(),
            );
            let updated = core
                .update_container_hydration(
                    id.to_string(),
                    status.into(),
                    health,
                    Value::Object(metadata),
                    family,
                )
                .await?;
            Ok(json!({"container":updated}))
        }
        ("POST", path) if path.starts_with("/v1/containers/") && path.ends_with("/reconcile") => {
            let id = path
                .trim_start_matches("/v1/containers/")
                .trim_end_matches("/reconcile")
                .trim_end_matches('/');
            let session = body
                .get("sessionRef")
                .or_else(|| body.get("session_ref"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("sessionRef required"))?;
            let spec = crate::optimizers::container_lifecycle::reconcile_declaration(
                core.storage().database(),
                session,
                id,
            )?;
            let container = core.data().get_container(id.to_string()).await?;
            Ok(json!({
                "container": container,
                "declarationOrigin": spec.origin.to_json(),
                "launchDeclaration": {
                    "valid": true,
                    "command": spec.command,
                    "workingDirectory": spec.cwd.display().to_string(),
                    "sourceRoot": spec.origin.source_root.display().to_string(),
                    "manifestPath": spec.origin.manifest_path.display().to_string(),
                    "sourceRevision": spec.origin.source_revision,
                    "sourceDigest": spec.origin.source_digest,
                },
            }))
        }
        ("POST", "/v1/containers/resolve_declaration") => {
            let session = body
                .get("sessionRef")
                .or_else(|| body.get("session_ref"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("sessionRef required"))?;
            let spec = if let Some(container_id) = body
                .get("containerId")
                .or_else(|| body.get("container_id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                crate::optimizers::container_lifecycle::resolve_declared_spec(
                    core.storage().database(),
                    session,
                    container_id,
                )?
            } else {
                let spec_id = body
                    .get("specId")
                    .or_else(|| body.get("spec_id"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("specId or containerId required"))?;
                crate::optimizers::container_lifecycle::resolve_spec_for_session(
                    core.storage().database(),
                    session,
                    spec_id,
                )?
            };
            Ok(json!({
                "specId": spec.id,
                "sourceRoot": spec.origin.source_root.display().to_string(),
                "manifestPath": spec.origin.manifest_path.display().to_string(),
                "declarationDigest": spec.origin.source_digest,
                "sourceRevision": spec.origin.source_revision,
                "declarationOrigin": spec.origin.to_json(),
            }))
        }
        ("POST", path) if path.starts_with("/v1/containers/") && path.ends_with("/stop") => {
            let id = path
                .trim_start_matches("/v1/containers/")
                .trim_end_matches("/stop")
                .trim_end_matches('/');
            let stopped =
                crate::optimizers::container_lifecycle::stop(core.storage().database(), id).await?;
            Ok(json!({
                "containerId": stopped.container_id,
                "specId": stopped.spec_id,
                "pid": stopped.pid,
                "status": "stopped",
            }))
        }
        ("POST", path) if path.starts_with("/v1/containers/") && path.ends_with("/restart") => {
            anyhow::bail!("container restart requires the app-bound approval route")
        }
        ("POST", path)
            if path.starts_with("/v1/containers/") && path.ends_with("/rollouts/prepare") =>
        {
            let id = path
                .trim_start_matches("/v1/containers/")
                .trim_end_matches("/rollouts/prepare")
                .trim_end_matches('/');
            let container = core.data().get_container(id.to_string()).await?;
            // Preflight before the request is even constructed: an unhealthy,
            // stale, or capability-incompatible record must fail here, not by
            // discovering a 405 after a mutating call.
            crate::container_capabilities::preflight_prepare_request(&container, &body)?;
            let base = validated_loopback_rollout_base(
                container
                    .base_url
                    .as_deref()
                    .context("container has no base URL")?,
            )?;
            let telemetry = normalized_rollout_telemetry(body.get("telemetry"))?;
            let rollout_id = body
                .get("rollout_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("roll_{}", Uuid::new_v4().simple()));
            let client = crate::http::http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(limits::VISUALS_IPC_ROLL_TIMEOUT)
                .build()?;
            let mut prepare_body = json!({"rollout_id": rollout_id, "telemetry": telemetry});
            // A live annotation protocol pin is part of rollout identity; the
            // caller names an installed anprev_ revision and the container
            // declares the sibling channel in the descriptor.
            if let Some(revision) = body
                .get("annotation_protocol_revision_id")
                .and_then(Value::as_str)
            {
                prepare_body["annotation_protocol_revision_id"] = json!(revision);
            }
            let mut response = client
                .post(format!("{base}/rollouts/prepare"))
                .json(&prepare_body)
                .send()
                .await;
            if response.is_err() {
                response = client
                    .post(format!("{base}/rollouts/prepare"))
                    .json(&prepare_body)
                    .send()
                    .await;
            }
            let response = response.context("idempotent POST /rollouts/prepare")?;
            if matches!(
                response.status(),
                reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED
            ) {
                anyhow::bail!("container has no normalized prepare endpoint; native benchmark routes must be folded inside Containers");
            }
            let status = response.status();
            let response_body = response.text().await?;
            if !status.is_success() {
                anyhow::bail!("container prepare failed ({status}): {response_body}");
            }
            let prepared = serde_json::from_str::<Value>(&response_body)
                .context("decode container prepare response")?;
            let returned_rollout_id = prepared
                .get("rollout_id")
                .and_then(Value::as_str)
                .context("prepare omitted rollout_id")?;
            if returned_rollout_id != rollout_id {
                anyhow::bail!("prepare returned a different rollout_id than the caller-stable id");
            }
            let mut stream = declared_stream_descriptor(&prepared)?
                .context("prepare omitted stream descriptor")?;
            if let Some(raw_max_steps) = body.get("max_steps") {
                let max_steps = raw_max_steps
                    .as_u64()
                    .context("max_steps must be a positive integer")?;
                if max_steps == 0 {
                    anyhow::bail!("max_steps must be a positive integer");
                }
                stream
                    .as_object_mut()
                    .context("prepare stream descriptor must be an object")?
                    .insert("max_steps".into(), json!(max_steps));
            }
            let poll_url = resolve_declared_url(&base, &declared_poll_url(&stream)?)?;
            let sse_url = resolve_declared_url(&base, &declared_sse_url(&stream)?)?;
            crate::visuals::assert_declared_stream_source(&sse_url)?;
            // A pinned live annotation protocol declares a sibling channel;
            // bind it beside the rollout stream on live.annotated_rollouts.v1.
            let annotation_visual_binding = match (
                crate::container_stream::declared_annotation_sse_url(&stream),
                crate::container_stream::declared_annotation_poll_url(&stream),
            ) {
                (Some(annotation_sse), Some(annotation_poll)) => {
                    let annotation_sse = resolve_declared_url(&base, &annotation_sse)?;
                    let annotation_poll = resolve_declared_url(&base, &annotation_poll)?;
                    crate::visuals::assert_declared_stream_source(&annotation_sse)?;
                    json!({"input":"stream","kind":"live_sse","source":annotation_sse,"poll_url":annotation_poll,"schema":"synth.trace-stream-event.v1"})
                }
                _ => Value::Null,
            };
            Ok(json!({
                "container_id": id, "rollout_id": rollout_id, "prepared": prepared, "stream": stream,
                "resolved": {"poll_url": poll_url, "sse_url": sse_url},
                "visual_binding": {"input":"stream","kind":"live_sse","source":sse_url,"poll_url":poll_url,"schema":"synth.trace-stream-event.v1"},
                "annotation_visual_binding": annotation_visual_binding,
                "start_blocked_until": "stream.subscribed"
            }))
        }
        ("GET", path) if path.starts_with("/v1/containers/") && path.contains("/rollouts/") => {
            let remainder = path.trim_start_matches("/v1/containers/");
            let (id, rollout_id) = remainder
                .split_once("/rollouts/")
                .context("invalid rollout status path")?;
            if id.is_empty() || rollout_id.is_empty() || rollout_id.contains('/') {
                anyhow::bail!("invalid rollout status path");
            }
            let container = core.data().get_container(id.to_string()).await?;
            let base = validated_loopback_rollout_base(
                container
                    .base_url
                    .as_deref()
                    .context("container has no base URL")?,
            )?;
            let client = crate::http::http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(limits::VISUALS_IPC_ROLL_TIMEOUT)
                .build()?;
            let state = get_rollout_status(&client, &base, rollout_id)
                .await?
                .context("unknown rollout")?;
            // A terminal rollout that names its seal gets imported here, on the
            // authoritative reconciliation an agent already performs. Sealing
            // and discoverability were two separate authorities with no edge
            // between them: Containers held a complete sealed trace and
            // Workshop's index stayed empty, so the inspector could not resolve
            // a trace that demonstrably existed.
            let import = if state.get("terminated").and_then(Value::as_bool) == Some(true) {
                match import_terminal_trace(core, id, rollout_id, &state).await {
                    Ok(value) => value,
                    // Import is reconciliation, not the answer to this call.
                    // A failure here must not hide the rollout state the caller
                    // asked for; it is reported beside it.
                    Err(error) => json!({"indexed": false, "error": error.to_string()}),
                }
            } else {
                Value::Null
            };
            Ok(json!({
                "container_id": id,
                "rollout_id": rollout_id,
                "state": state,
                "trace_import": import,
            }))
        }
        ("POST", path)
            if path.starts_with("/v1/containers/") && path.ends_with("/rollouts/poll") =>
        {
            let id = path
                .trim_start_matches("/v1/containers/")
                .trim_end_matches("/rollouts/poll")
                .trim_end_matches('/');
            let rollout_id = body
                .get("rollout_id")
                .and_then(Value::as_str)
                .context("poll requires rollout_id")?;
            let stream = body
                .get("stream")
                .filter(|value| value.is_object())
                .context("poll requires the exact prepared stream descriptor")?;
            let after = body.get("after").and_then(Value::as_u64).unwrap_or(0);
            let container = core.data().get_container(id.to_string()).await?;
            let base = validated_loopback_rollout_base(
                container
                    .base_url
                    .as_deref()
                    .context("container has no base URL")?,
            )?;
            let mut poll_url =
                reqwest::Url::parse(&resolve_declared_url(&base, &declared_poll_url(stream)?)?)?;
            poll_url
                .query_pairs_mut()
                .clear()
                .append_pair("after", &after.to_string());
            let client = crate::http::http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(limits::VISUALS_IPC_ROLL_TIMEOUT)
                .build()?;
            let page = client
                .get(poll_url)
                .send()
                .await
                .context("resume declared rollout poll")?
                .error_for_status()?
                .json::<Value>()
                .await?;
            if page.get("rollout_id").and_then(Value::as_str) != Some(rollout_id) {
                anyhow::bail!("poll response rollout_id does not match requested rollout");
            }
            let next_cursor = page
                .pointer("/cursor/high_water")
                .cloned()
                .unwrap_or(Value::Null);
            if page.pointer("/cursor/closed").and_then(Value::as_bool) == Some(true) {
                crate::recovery::crash_checkpoint(
                    crate::recovery::checkpoints::AFTER_ROLLOUT_TERMINAL,
                );
            }
            Ok(
                json!({"container_id": id, "rollout_id": rollout_id, "page": page, "next_cursor": next_cursor}),
            )
        }
        ("POST", path)
            if path.starts_with("/v1/containers/") && path.ends_with("/rollouts/start") =>
        {
            let id = path
                .trim_start_matches("/v1/containers/")
                .trim_end_matches("/rollouts/start")
                .trim_end_matches('/');
            let container = core.data().get_container(id.to_string()).await?;
            let base = validated_loopback_rollout_base(
                container
                    .base_url
                    .as_deref()
                    .context("container has no base URL")?,
            )?;
            let rollout_id = body
                .get("rollout_id")
                .and_then(Value::as_str)
                .context("start requires rollout_id")?;
            let visual_id = body
                .get("visual_id")
                .and_then(Value::as_str)
                .context("start requires visual_id")?;
            // The pre-start gate proves that a real visual exists and that its
            // declared stream is subscribed below. Full screenshot-backed
            // readiness is deliberately post-data: Craftax's imageReplay check
            // cannot truthfully pass until the first rollout emits frames.
            let visual = registry.get(visual_id.to_string()).await?;
            let stream = body
                .get("stream")
                .filter(|value| value.is_object())
                .context("start requires the exact prepared stream descriptor")?;
            let policy_ref = require_caller_policy_ref(&body)?;
            if container
                .metadata
                .pointer("/liveEval/requiresVisualsMcp")
                .and_then(Value::as_bool)
                == Some(true)
            {
                require_visualsbench_start_policy(&policy_ref).context(
                    "refusing VisualsBench start: synth_visuals is not bound to the Codex policy",
                )?;
            }
            let task_instance_id = require_task_instance(&body)?;
            let poll_url = resolve_declared_url(&base, &declared_poll_url(stream)?)?;
            let telemetry = normalized_rollout_telemetry(body.get("telemetry"))?;
            let client = crate::http::http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(limits::CONTAINER_POLICY_ROLLOUT_TIMEOUT)
                .build()?;
            let recovery_client = crate::http::http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(limits::VISUALS_IPC_ROLL_TIMEOUT)
                .build()?;
            let stream_diagnostics = crate::container_stream::StreamDiagnostics::new(
                Some(core.diagnostics_service().clone()),
                crate::diagnostics::Correlation {
                    container_id: Some(id.to_string()),
                    rollout_id: Some(rollout_id.to_string()),
                    visual_id: Some(visual_id.to_string()),
                    visual_revision: Some(visual.current_revision),
                    stream_id: stream
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| Some(rollout_id.to_string())),
                    session_id: json_field(&body, "sessionRef", "session_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    ..Default::default()
                },
            );
            let subscription = wait_for_stream_subscribed(
                &client,
                &poll_url,
                SUBSCRIBE_READY_TIMEOUT,
                &stream_diagnostics,
            )
            .await?;
            let mut start_body = json!({
                "rollout_id": rollout_id, "seed": body.get("seed"), "task_instance_id": task_instance_id,
                "policy_ref": policy_ref, "telemetry": telemetry, "slot": LIVE_EVAL_SLOT
            });
            // Prepared NanoHorizon rollouts are pinned to an immutable policy
            // revision. Preserve that pin across the host bridge instead of
            // silently degrading the request to a mutable policy reference.
            if let Some(policy_revision_id) = body
                .get("policy_revision_id")
                .or_else(|| body.get("policyRevisionId"))
                .cloned()
            {
                start_body["policy_revision_id"] = policy_revision_id;
            }
            if let Some(max_steps) = stream.get("max_steps").and_then(Value::as_u64) {
                start_body["max_steps"] = json!(max_steps);
            }
            if let Some(revision) = body
                .get("annotation_protocol_revision_id")
                .and_then(Value::as_str)
            {
                // Same pin as prepare, or the container answers 409
                // rollout_identity_conflict: the observer is part of identity.
                start_body["annotation_protocol_revision_id"] = json!(revision);
            }
            if let Some(environment_ref) = body
                .get("environment_ref")
                .or_else(|| body.get("environmentRef"))
                .cloned()
            {
                start_body["environment_ref"] = environment_ref;
            }
            if let Some(task_world) = body
                .get("task_world")
                .or_else(|| body.get("taskWorld"))
                .cloned()
            {
                start_body["task_world"] = task_world;
            }
            if let Some(world_ref) = body
                .get("world_ref")
                .or_else(|| body.get("worldRef"))
                .cloned()
            {
                start_body["world_ref"] = world_ref;
            }
            // Launching a rollout spends real compute. Record that it is about
            // to leave the process *before* it does, so a crash in the gap is
            // recoverable as "outcome unknown" rather than replayed blind into a
            // second paid run. See `recovery::receipts`.
            let receipt = begin_rollout_receipt(
                &core,
                body.get("sessionRef")
                    .or_else(|| body.get("session_id"))
                    .and_then(Value::as_str),
                rollout_id,
                &start_body,
            )
            .await;
            let started = start_rollout_idempotently(
                &client,
                &recovery_client,
                &base,
                rollout_id,
                &start_body,
            )
            .await;
            crate::recovery::crash_checkpoint(crate::recovery::checkpoints::AFTER_ROLLOUT_LAUNCH);
            // A transport error is deliberately *not* recorded as failed: it
            // does not prove the façade never accepted the rollout, and
            // claiming it did is exactly how a duplicate gets launched later.
            let (state, recovered) = started.context("POST /rollouts after stream.subscribed")?;
            settle_rollout_receipt(&core, receipt.as_deref(), Some(rollout_id)).await;
            crate::recovery::crash_checkpoint(crate::recovery::checkpoints::AFTER_TOOL_RECEIPT);
            core.update_container_last_rollout(id.to_string(), rollout_id.to_string())
                .await?;
            Ok(
                json!({"container_id":id,"rollout_id":rollout_id,"visual_id":visual_id,"visual_revision":visual.current_revision,"state":state,"subscription":subscription,"started":true,"recovered":recovered}),
            )
        }
        ("POST", path) if path.starts_with("/v1/containers/") && path.ends_with("/rollouts") => {
            let id = path
                .trim_start_matches("/v1/containers/")
                .trim_end_matches("/rollouts")
                .trim_end_matches('/');
            let container = core.data().get_container(id.to_string()).await?;
            let base = container
                .base_url
                .as_deref()
                .context("container has no base URL")?;
            let base = validated_loopback_rollout_base(base)?;
            let count = body.get("count").and_then(Value::as_u64).unwrap_or(1);
            if !(1..=MAX_SCRIPTED_ROLLOUTS).contains(&count) {
                anyhow::bail!("count must be between 1 and {MAX_SCRIPTED_ROLLOUTS}");
            }
            let actions = body
                .get("actions")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .map(|value| {
                            value
                                .as_str()
                                .map(str::to_string)
                                .context("actions must contain only strings")
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .context("container_run_rollouts requires an explicit actions list; this is scripted engine acceptance, not a policy eval")?;
            if actions.is_empty() || actions.len() > 64 {
                anyhow::bail!("actions must contain between 1 and 64 bounded steps");
            }
            require_scripted_stream_slot(&body)?;
            if let Some(telemetry) = body.get("telemetry") {
                refuse_auto_transport(telemetry)?;
            }
            let seeds = body.get("seeds").and_then(Value::as_array);
            if let Some(values) = seeds {
                if values.len() != count as usize
                    || values.iter().any(|value| value.as_i64().is_none())
                {
                    anyhow::bail!("seeds must contain exactly count integer values");
                }
            }
            let client = crate::http::http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(limits::VISUALS_IPC_ROLL_TIMEOUT)
                .build()?;
            let mut rollouts = Vec::with_capacity(count as usize);
            for index in 0..count {
                let seed = seeds
                    .and_then(|values| values.get(index as usize))
                    .and_then(Value::as_i64)
                    .unwrap_or(index as i64 + 1);
                let ScriptedRollout {
                    rollout_id,
                    state,
                    events,
                    stream_id,
                } = run_one_scripted_rollout(
                    &client,
                    &base,
                    seed,
                    &actions,
                    None,
                    &crate::container_stream::StreamDiagnostics::new(
                        Some(core.diagnostics_service().clone()),
                        crate::diagnostics::Correlation {
                            container_id: Some(id.to_string()),
                            ..Default::default()
                        },
                    ),
                )
                .await?;
                let spool = crate::storage::persist_live_envelopes(
                    core.content(),
                    stream_id.as_deref(),
                    Some(&rollout_id),
                    crate::storage::envelopes_from_event_log(&events),
                )?;
                core.update_container_last_rollout(id.to_string(), rollout_id.clone())
                    .await?;
                rollouts.push(json!({
                    "rollout_id": rollout_id,
                    "seed": seed,
                    "actions": actions.clone(),
                    "state": state,
                    "event_log": events,
                    "spool_digest": spool.digest,
                }));
            }
            Ok(json!({
                "container_id": id,
                "base_url": base,
                "rollout_count": rollouts.len(),
                "rollouts": rollouts,
            }))
        }
        ("GET", "/v1/visuals/templates") => {
            let genre = body.get("genre").and_then(Value::as_str);
            Ok(json!({"templates": registry.list_templates(genre)?}))
        }
        ("POST", "/v1/visuals/templates/import") => {
            let source_path = body
                .get("sourcePath")
                .or_else(|| body.get("source_path"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("source_path is required"))?;
            Ok(json!({"template": registry.import_template(source_path)?}))
        }
        ("GET", path) if path.starts_with("/v1/visuals/templates/") => {
            let id = path.trim_start_matches("/v1/visuals/templates/");
            Ok(json!({"template": registry.get_template(id)?}))
        }
        ("GET", "/v1/visuals") => {
            let query: VisualQuery = serde_json::from_value(body.clone()).unwrap_or_default();
            Ok(json!({"visuals": registry.list(query).await?}))
        }
        ("GET", path) if path.starts_with("/v1/cas/") => {
            let digest = path
                .trim_start_matches("/v1/cas/")
                .trim_start_matches("sha256:");
            if digest.is_empty() || digest.contains('/') {
                anyhow::bail!("invalid content digest");
            }
            let bytes = registry.content().get_bytes("blobs", digest)?;
            match serde_json::from_slice::<Value>(&bytes) {
                Ok(value) => Ok(value),
                Err(_) => Ok(json!({
                    "digest": digest,
                    "mediaType": "application/octet-stream",
                    "base64": base64::engine::general_purpose::STANDARD.encode(bytes),
                })),
            }
        }
        ("GET", "/v1/seals") => {
            let visual_id = body
                .get("visual_id")
                .or_else(|| body.get("visualId"))
                .and_then(Value::as_str)
                .map(str::to_string);
            Ok(json!({"seals": registry.list_seals(visual_id).await?}))
        }
        ("GET", path) if path.starts_with("/v1/seals/") => {
            let digest = path.trim_start_matches("/v1/seals/");
            if digest.is_empty() || digest.contains('/') {
                anyhow::bail!("invalid seal receipt digest");
            }
            Ok(json!({"bundle": registry.get_seal(digest.to_string()).await?}))
        }
        ("GET", path) if path.starts_with("/v1/visuals/") && path.ends_with("/annotations") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/annotations")
                .trim_end_matches('/');
            let annotations = registry.annotations(id.to_string()).await?;
            let revision = registry.get(id.to_string()).await?.current_revision;
            let overlay_digest = registry.overlay_digest(id.to_string(), revision).await?;
            Ok(
                json!({"annotations": annotations, "overlayDigest": overlay_digest, "revision": revision}),
            )
        }
        ("GET", path) if path.starts_with("/v1/visuals/") && path.ends_with("/authoring") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/authoring")
                .trim_end_matches('/');
            let visual = registry.get(id.to_string()).await?;
            let template = registry.get_template(&visual.template_id)?;
            let required_checks = required_authoring_checks(&template);
            let reviews = visual
                .metadata
                .get("authoringReviews")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let automated_findings =
                if let Some(kind) = crate::visuals::systems::template_kind(&visual.template_id) {
                    let asset = registry.visual_source(id.to_string()).await?;
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(asset.base64)
                        .context("systems authoring source must be base64")?;
                    let source = String::from_utf8(bytes)
                        .context("systems authoring source must be UTF-8")?;
                    crate::visuals::systems::authoring_findings(&source, kind)?
                } else if crate::visuals::charts::is_chart_template(&visual.template_id) {
                    // Recorded by the render that produced the image. A chart's
                    // real width is only known after its bindings resolve, so
                    // re-deriving from the spec alone would under-report.
                    visual
                        .metadata
                        .get("authoringFindings")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
            let annotations = registry.annotations(id.to_string()).await?;
            let overlay_digest = registry
                .overlay_digest(id.to_string(), visual.current_revision)
                .await?;
            Ok(json!({
                "visual": visual,
                "template": template,
                "annotations": annotations,
                "overlayDigest": overlay_digest,
                "authoring": {
                    "rendererContract": "trusted_template_configuration",
                    "arbitraryTsxExecuted": false,
                    "requiredIterations": 2,
                    "reviewCount": reviews.len(),
                    "requiredChecks": required_checks,
                    "automatedFindings": automated_findings,
                    "instruction": "Render and show in Desktop, capture and inspect screenshots at wide and compact viewports, revise until automated findings and visible collisions are resolved, then record two screenshot-backed passing reviews before mark_ready."
                }
            }))
        }
        ("POST", path) if path.starts_with("/v1/visuals/") && path.ends_with("/annotations") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/annotations")
                .trim_end_matches('/');
            let request: VisualAnnotationCreate = serde_json::from_value(body)?;
            let (annotation, event) = registry.create_annotation(id.to_string(), request).await?;
            Ok(json!({"annotation": annotation, "event": event}))
        }
        ("POST", path) if path.starts_with("/v1/visuals/") && path.ends_with("/seal") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/seal")
                .trim_end_matches('/');
            let revision = body
                .get("revision")
                .and_then(Value::as_i64)
                .context("seal requires exact revision")?;
            let (seal, event) = registry.seal(id.to_string(), revision).await?;
            Ok(json!({"seal": seal, "event": event}))
        }
        ("GET", path) if path.starts_with("/v1/visuals/") && path.ends_with("/content") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/content")
                .trim_end_matches('/');
            Ok(json!({"content": registry.visual_source(id.to_string()).await?}))
        }
        ("GET", path) if path.starts_with("/v1/visuals/") && path.ends_with("/renditions") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/renditions")
                .trim_end_matches('/');
            Ok(json!({"renditions": registry.list_renditions(id.to_string()).await?}))
        }
        ("GET", path) if path.starts_with("/v1/visuals/") && path.contains("/renditions/") => {
            let rest = path.trim_start_matches("/v1/visuals/");
            let (id, format) = rest
                .split_once("/renditions/")
                .ok_or_else(|| anyhow::anyhow!("invalid rendition path"))?;
            let theme = body
                .get("theme")
                .and_then(Value::as_str)
                .map(str::to_string);
            let size = body
                .get("size")
                .or_else(|| body.get("size_class"))
                .and_then(Value::as_str)
                .map(str::to_string);
            Ok(json!({
                "rendition": registry
                    .visual_rendition(id.to_string(), Some(format.to_string()), theme, size)
                    .await?
            }))
        }
        ("GET", path) if path.starts_with("/v1/visuals/") && !path.ends_with("/show") => {
            let id = path.trim_start_matches("/v1/visuals/");
            if id.contains('/') {
                anyhow::bail!("unsupported visuals path");
            }
            Ok(json!({"visual": registry.get(id.to_string()).await?}))
        }
        ("POST", path) if path.starts_with("/v1/visuals/") && path.ends_with("/reviews") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/reviews")
                .trim_end_matches('/');
            let current = registry.get(id.to_string()).await?;
            let revision = body
                .get("revision")
                .and_then(Value::as_i64)
                .context("review requires revision")?;
            if revision != current.current_revision {
                anyhow::bail!(
                    "review revision {revision} is stale; current revision is {}",
                    current.current_revision
                );
            }
            let viewport = body
                .get("viewport")
                .filter(|value| value.is_object())
                .context("review requires viewport")?;
            let width = viewport
                .get("width")
                .and_then(Value::as_u64)
                .context("review viewport requires width")?;
            let height = viewport
                .get("height")
                .and_then(Value::as_u64)
                .context("review viewport requires height")?;
            if width < 320 || height < 400 {
                anyhow::bail!("review viewport is below the supported 320x400 floor");
            }
            let checks = body
                .get("checks")
                .filter(|value| value.is_object())
                .context("review requires checks")?;
            let findings = body
                .get("findings")
                .and_then(Value::as_array)
                .context("review requires findings")?;
            if findings.iter().any(|value| !value.is_string()) {
                anyhow::bail!("review findings must be strings");
            }
            let capture_receipt = {
                let screenshot = body
                    .get("screenshot_path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .context("visual review requires screenshot_path from capture_review")?;
                if !(screenshot.ends_with(".png")
                    || screenshot.ends_with(".jpg")
                    || screenshot.ends_with(".jpeg"))
                {
                    anyhow::bail!(
                        "visual review screenshot_path must reference a PNG or JPEG capture"
                    );
                }
                let screenshot_path = std::path::Path::new(screenshot);
                if !screenshot_path.is_absolute() || !screenshot_path.is_file() {
                    anyhow::bail!(
                        "visual review screenshot_path must be an existing absolute file"
                    );
                }
                let receipt = capture_observation_receipt(screenshot)?;
                if receipt.visual_id != id || receipt.revision != revision {
                    anyhow::bail!(
                        "visual capture observations do not match the reviewed visual revision"
                    );
                }
                receipt
            };
            let template = registry.get_template(&current.template_id)?;
            if template.observation_contract.is_some() && capture_receipt.observation.is_none() {
                anyhow::bail!(
                    "this template requires mechanically harvested rendered observations"
                );
            }
            let mut metadata = current.metadata.as_object().cloned().unwrap_or_default();
            let mut reviews = metadata
                .get("authoringReviews")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            reviews.push(json!({
                "revision": revision,
                "viewport": viewport,
                "checks": checks,
                "findings": findings,
                "screenshotPath": body.get("screenshot_path").cloned().unwrap_or(Value::Null),
                "captureTime": capture_receipt.capture_time,
                "observations": capture_receipt.observation,
                "reviewedAt": chrono::Utc::now().to_rfc3339(),
            }));
            metadata.insert("authoringReviews".into(), Value::Array(reviews.clone()));
            metadata.remove("qualityGate");
            let (visual, event) = registry
                .update(
                    id.to_string(),
                    VisualUpdateRequest {
                        title: None,
                        bindings: None,
                        status: Some(VisualStatus::Draft),
                        renderer_kind: None,
                        message_id: None,
                        run_id: None,
                        trace_id: None,
                        content: None,
                        metadata: Some(Value::Object(metadata)),
                        bump_revision: Some(false),
                    },
                )
                .await?;
            Ok(json!({"visual": visual, "event": event, "reviewCount": reviews.len()}))
        }
        ("POST", path) if path.starts_with("/v1/visuals/") && path.ends_with("/ready") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/ready")
                .trim_end_matches('/');
            let current = registry.get(id.to_string()).await?;
            let revision = body
                .get("revision")
                .and_then(Value::as_i64)
                .context("mark_ready requires revision")?;
            if revision != current.current_revision {
                anyhow::bail!(
                    "ready revision {revision} is stale; current revision is {}",
                    current.current_revision
                );
            }
            let mut metadata = current.metadata.as_object().cloned().unwrap_or_default();
            let reviews = metadata
                .get("authoringReviews")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let current_reviews: Vec<&Value> = reviews
                .iter()
                .filter(|review| review.get("revision").and_then(Value::as_i64) == Some(revision))
                .collect();
            if current_reviews.len() < 2 {
                anyhow::bail!(
                    "visual readiness requires at least two reviews of revision {revision}"
                );
            }
            let template = registry.get_template(&current.template_id)?;
            let required = required_authoring_checks(&template);
            let bindings_digest = if template.observation_contract.is_some() {
                let durable = registry
                    .revisions(id.to_string())
                    .await?
                    .into_iter()
                    .find(|candidate| candidate.revision == revision)
                    .context("current visual revision record is missing")?;
                Some(
                    durable
                        .bindings_digest
                        .context("current visual revision is missing bindings digest")?,
                )
            } else {
                None
            };
            let receipts = certification_receipts(
                id,
                revision,
                &current_reviews,
                &required,
                template.observation_contract.as_ref(),
                bindings_digest.as_deref(),
            )?;
            if let Some(kind) = crate::visuals::systems::template_kind(&current.template_id) {
                let asset = registry.visual_source(id.to_string()).await?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(asset.base64)
                    .context("systems readiness source must be base64")?;
                let source =
                    String::from_utf8(bytes).context("systems readiness source must be UTF-8")?;
                let findings = crate::visuals::systems::authoring_findings(&source, kind)?;
                if !findings.is_empty() {
                    anyhow::bail!(
                        "systems visual has unresolved automated findings: {}",
                        findings.join("; ")
                    );
                }
            }
            if crate::visuals::charts::is_chart_template(&current.template_id) {
                let findings: Vec<String> = current
                    .metadata
                    .get("authoringFindings")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                if !findings.is_empty() {
                    anyhow::bail!(
                        "chart visual has unresolved automated findings: {}",
                        findings.join("; ")
                    );
                }
            }
            metadata.insert(
                "qualityGate".into(),
                json!({
                    "ready": true,
                    "revision": revision,
                    "reviewCount": current_reviews.len(),
                    "certifiedBy": receipts,
                    "supersededReviewCount": current_reviews.len() - receipts.len(),
                    "readyAt": chrono::Utc::now().to_rfc3339(),
                }),
            );
            let (visual, event) = registry
                .update(
                    id.to_string(),
                    VisualUpdateRequest {
                        title: None,
                        bindings: None,
                        status: Some(VisualStatus::Saved),
                        renderer_kind: None,
                        message_id: None,
                        run_id: None,
                        trace_id: None,
                        content: None,
                        metadata: Some(Value::Object(metadata)),
                        bump_revision: Some(false),
                    },
                )
                .await?;
            Ok(json!({"visual": visual, "event": event, "ready": true, "revision": revision}))
        }
        ("POST", "/v1/visuals") => {
            let request: VisualCreateRequest = serde_json::from_value(body)?;
            let (visual, event) = registry.create(request).await?;
            Ok(json!({"visual": visual, "event": event}))
        }
        ("POST", path) if path.starts_with("/v1/visuals/") && path.ends_with("/save") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/save")
                .trim_end_matches('/');
            let tsx = body.get("tsx").and_then(Value::as_str).map(str::to_string);
            let (visual, event) = registry.save(id.to_string(), tsx).await?;
            Ok(json!({"visual": visual, "event": event}))
        }
        ("POST", path) if path.starts_with("/v1/visuals/") && path.ends_with("/fork") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/fork")
                .trim_end_matches('/');
            let title = body
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string);
            let session_id = body
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string);
            let (visual, event) = registry.fork(id.to_string(), title, session_id).await?;
            Ok(json!({"visual": visual, "event": event}))
        }
        ("POST", path) if path.starts_with("/v1/visuals/") && path.ends_with("/archive") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/archive")
                .trim_end_matches('/');
            let (visual, event) = registry.archive(id.to_string()).await?;
            Ok(json!({"visual": visual, "event": event}))
        }
        ("POST", path) if path.starts_with("/v1/visuals/") && path.ends_with("/show") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/show")
                .trim_end_matches('/');
            let session_id = body
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| std::env::var("SYNTH_SESSION_ID").ok());
            let foreground_owner = body
                .get("foregroundOwner")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let (visual, mut event) = registry.show(id.to_string(), session_id).await?;
            if foreground_owner {
                if let Some(payload) = event.get_mut("payload").and_then(Value::as_object_mut) {
                    // Presentation intent is broadcast-only. The durable
                    // visual.show record remains an ownership fact; reopening
                    // Workshop must not replay an old QA navigation request.
                    payload.insert("foregroundOwner".into(), Value::Bool(true));
                }
            }
            core.broadcast_committed(Some(serde_json::from_value(event.clone())?));
            Ok(json!({"opened": true, "visual": visual, "event": event}))
        }
        ("POST", path) if path.starts_with("/v1/visuals/") && path.ends_with("/render") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/render")
                .trim_end_matches('/');
            let visual = registry.render_visual(id).await?;
            Ok(json!({"visual": visual}))
        }
        ("POST", path) if path.starts_with("/v1/visuals/") => {
            let id = path.trim_start_matches("/v1/visuals/");
            let request: VisualUpdateRequest = serde_json::from_value(body)?;
            let (visual, event) = registry.update(id.to_string(), request).await?;
            // MCP bind/update writes a durable visual.updated event. Publish the
            // same committed event to the renderer so an already-open pane
            // reconciles its binding instead of remaining frozen until reload.
            core.broadcast_committed(Some(serde_json::from_value(event.clone())?));
            Ok(json!({"visual": visual, "event": event}))
        }
        _ => anyhow::bail!("unsupported visuals IPC route {method} {path}"),
    }
}

pub(crate) async fn dispatch_optimizer(
    method: &str,
    path: &str,
    body: Value,
    core: &CoreRuntime,
    app: &AppHandle,
) -> Result<Value> {
    let optimizers = core.optimizers();
    match (method, path) {
        ("GET", "/v1/optimizers/algorithms") => {
            Ok(json!({ "algorithms": optimizers.list_algorithms() }))
        }
        ("GET", "/v1/optimizers/recipes") => {
            let session = body
                .get("sessionRef")
                .or_else(|| body.get("session_ref"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| std::env::var("SYNTH_SESSION_ID").ok());
            Ok(json!({
                "recipes": optimizers.list_recipes_for_session(session.as_deref())
            }))
        }
        ("POST", "/v1/optimizers/evaluations/spec/draft")
        | ("POST", "/v1/optimizers/evaluations/spec/validate")
        | ("POST", "/v1/optimizers/evaluations/spec/admit") => {
            let request_value = body.get("request").cloned().unwrap_or(body);
            let request: crate::optimizers::admission::InlineRequest =
                crate::optimizers::admission::InlineRequest::from_tool_arguments(request_value)?;
            let admissible =
                crate::optimizers::inline_eval::admit_inline(optimizers, request).await?;
            Ok(json!({
                "sourceKind": "inline",
                "executionSpecDigest": admissible.digest().as_str(),
                "executionSpec": admissible.canonical().as_value(),
                "approvalDisclosure": admissible.approval_disclosure(),
                "status": "ready_for_approval"
            }))
        }
        ("POST", "/v1/optimizers/evaluations/start") => {
            let idempotency_key = body
                .get("idempotencyKey")
                .or_else(|| body.get("idempotency_key"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("idempotencyKey is required for evaluation_start"))?
                .to_owned();
            anyhow::ensure!(
                idempotency_key.len() <= 128,
                "idempotencyKey must be at most 128 characters"
            );
            let request: crate::optimizers::admission::InlineRequest =
                crate::optimizers::admission::InlineRequest::from_tool_arguments(
                    body.get("request").cloned().unwrap_or_else(|| body.clone()),
                )?;
            let session_ref = body
                .get("sessionRef")
                .or_else(|| body.get("session_ref"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| std::env::var("SYNTH_SESSION_ID").ok());
            let open_visual = body
                .get("openVisual")
                .or_else(|| body.get("open_visual"))
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let codex = app.state::<Arc<crate::codex::CodexManager>>();
            let run = crate::authorize_inline_evaluation_start(
                app,
                core,
                &codex,
                request,
                session_ref,
                open_visual,
                idempotency_key,
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            Ok(json!({
                "run": run,
                "sourceKind": "inline",
                "status": run.status,
                "optimizerRunId": run.id,
                "visualRefs": run.visual_refs,
                "eventCursor": run.cursor_seq
            }))
        }
        ("POST", "/v1/optimizers/eval/candidates") => {
            let request: crate::optimizers::EvalStageCandidatesRequest =
                serde_json::from_value(body)?;
            let manifest = optimizers.stage_eval_candidates(request).await?;
            Ok(json!({ "candidateSet": manifest }))
        }
        ("POST", path)
            if path.starts_with("/v1/training/artifacts/") && path.ends_with("/chat") =>
        {
            let id = path
                .trim_start_matches("/v1/training/artifacts/")
                .trim_end_matches("/chat")
                .trim_end_matches('/');
            let confirm = body
                .get("confirm")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !confirm {
                anyhow::bail!("launch_artifact_inference requires confirm=true");
            }
            let message = body
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Reply with one short sentence confirming which adapter you are.");
            let inference = crate::optimizers::launch_artifact_inference(id, message).await?;
            Ok(json!({ "inference": inference }))
        }
        ("GET", "/v1/mlx/inspect") => {
            Ok(crate::optimizers::typed_capabilities::inspect_local_mlx())
        }
        ("GET", "/v1/training/mlx-runtime") => Ok(serde_json::to_value(
            crate::optimizers::mlx_runtime::runtime_status(),
        )?),
        ("POST", "/v1/training/mlx-runtime/install") => {
            let confirm = body
                .get("confirm")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let status =
                crate::optimizers::mlx_runtime::training_mlx_runtime_install(app.clone(), confirm)
                    .await
                    .map_err(anyhow::Error::msg)?;
            Ok(serde_json::to_value(status)?)
        }
        ("GET", "/v1/mlx/install-plan") => {
            let model_id = body.get("model_id").and_then(Value::as_str);
            crate::optimizers::typed_capabilities::plan_model_install(model_id)
        }
        ("POST", "/v1/mlx/install") => {
            let confirm = body
                .get("confirm")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let model_id = body.get("model_id").and_then(Value::as_str);
            crate::optimizers::typed_capabilities::install_model_or_runtime(model_id, confirm)
        }
        ("POST", "/v1/training/plans") => {
            let recipe_id = body
                .get("recipe_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("recipe_id required"))?;
            crate::optimizers::typed_capabilities::create_training_plan(recipe_id)
        }
        ("GET", "/v1/training/artifacts") => {
            crate::optimizers::typed_capabilities::list_training_artifacts()
        }
        ("GET", path) if path.starts_with("/v1/training/artifacts/") => {
            let id = path.trim_start_matches("/v1/training/artifacts/");
            crate::optimizers::typed_capabilities::inspect_training_artifact(id)
        }
        ("POST", path)
            if path.starts_with("/v1/training/artifacts/") && path.ends_with("/eval") =>
        {
            let id = path
                .trim_start_matches("/v1/training/artifacts/")
                .trim_end_matches("/eval")
                .trim_end_matches('/');
            let confirm = body
                .get("confirm")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let recipe_id = body.get("recipe_id").and_then(Value::as_str);
            let request = crate::optimizers::typed_capabilities::launch_artifact_eval_request(
                id, recipe_id, confirm,
            )?;
            let admitted: crate::optimizers::OptimizerRecipeRunRequest = serde_json::from_value(
                json!({
                    "recipeId": request["recipeId"],
                    "trainingArtifactId": request["trainingArtifactId"],
                    "sessionRef": body.get("sessionRef").cloned().or_else(|| body.get("session_ref").cloned()),
                    "openVisual": body.get("openVisual").cloned().or_else(|| body.get("open_visual").cloned()).unwrap_or(json!(true))
                }),
            )?;
            let codex = app.state::<Arc<crate::codex::CodexManager>>();
            let run = crate::authorize_optimizer_recipe_start(app, core, &codex, admitted)
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            Ok(json!({ "run": run }))
        }
        ("POST", path)
            if path.starts_with("/v1/training/artifacts/")
                && (path.ends_with("/export") || path.ends_with("/delete")) =>
        {
            let export = path.ends_with("/export");
            let id = path
                .trim_start_matches("/v1/training/artifacts/")
                .trim_end_matches(if export { "/export" } else { "/delete" })
                .trim_end_matches('/');
            let confirm = body
                .get("confirm")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let destination = body
                .get("destination")
                .and_then(Value::as_str)
                .or_else(|| body.get("path").and_then(Value::as_str));
            let expected_digest = body
                .get("digest")
                .and_then(Value::as_str)
                .or_else(|| body.get("expectedDigest").and_then(Value::as_str));
            crate::optimizers::typed_capabilities::export_or_delete_artifact(
                id,
                if export { "export" } else { "delete" },
                confirm,
                destination,
                expected_digest,
            )
        }
        ("POST", "/v1/optimizers/recipes/prepare") => {
            let request: crate::optimizers::OptimizerRecipeRunRequest =
                serde_json::from_value(body)?;
            let (run, event) = optimizers.prepare_recipe(request).await?;
            Ok(
                json!({ "run": run, "event": event, "preparationDigest": run.summary.get("preparationDigest") }),
            )
        }
        ("POST", "/v1/optimizers/recipes/run") => {
            let request: crate::optimizers::OptimizerRecipeRunRequest =
                serde_json::from_value(body)?;
            let codex = app.state::<Arc<crate::codex::CodexManager>>();
            let run = crate::authorize_optimizer_recipe_start(app, core, &codex, request)
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            Ok(json!({ "run": run }))
        }
        ("POST", "/v1/optimizers/workflows/start") => {
            let request: crate::optimizers::OptimizerRecipeRunRequest =
                serde_json::from_value(body)?;
            let requested_recipe_id = request.recipe_id.clone();
            crate::refresh_optimizer_workflow_containers(core, &request.recipe_id).await?;
            let codex = app.state::<Arc<crate::codex::CodexManager>>();
            let run = crate::authorize_optimizer_recipe_start(app, core, &codex, request)
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            let actual_recipe_id = run
                .summary
                .get("recipeId")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("admitted workflow did not record its recipe id"))?;
            if actual_recipe_id != requested_recipe_id {
                anyhow::bail!(
                    "workflow recipe identity mismatch: requested {requested_recipe_id}, admitted {actual_recipe_id}"
                );
            }
            Ok(json!({
                "run": run,
                "workflow": {
                    "status": run.status,
                    "requestedRecipeId": requested_recipe_id,
                    "recipeId": actual_recipe_id,
                    "exactRecipe": true,
                    "optimizerRunId": run.id,
                    "visualRefs": run.visual_refs,
                    "eventCursor": run.cursor_seq
                }
            }))
        }
        ("POST", "/v1/optimizers/runs/start") => {
            let id = body
                .get("optimizerRunId")
                .or_else(|| body.get("optimizer_run_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("optimizer_run_id required"))?;
            let digest = body
                .get("preparationDigest")
                .or_else(|| body.get("preparation_digest"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let approval = body
                .get("approvalReceiptId")
                .or_else(|| body.get("approval_receipt_id"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let session_ref = body
                .get("sessionRef")
                .or_else(|| body.get("session_ref"))
                .and_then(Value::as_str);
            let mut approval = approval;
            if approval.is_none() {
                if let Some(broker) =
                    app.try_state::<Arc<crate::session::approval::ApprovalBroker>>()
                {
                    let run = optimizers.get(id.to_string()).await?;
                    let recipe_id = run
                        .summary
                        .get("recipeId")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            anyhow::anyhow!("prepared optimizer run omitted recipeId")
                        })?;
                    let max_cost_usd = run
                        .summary
                        .pointer("/limits/maxCostUsd")
                        .and_then(Value::as_f64)
                        .ok_or_else(|| {
                            anyhow::anyhow!("prepared optimizer run omitted maxCostUsd")
                        })?;
                    let max_rollouts = run
                        .summary
                        .pointer("/limits/maxTotalRollouts")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| {
                            anyhow::anyhow!("prepared optimizer run omitted maxTotalRollouts")
                        })?;
                    let proposer_model = run
                        .summary
                        .get("proposerModel")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                        anyhow::anyhow!("prepared optimizer run omitted proposerModel")
                    })?;
                    let provider = run
                        .summary
                        .pointer("/credentialChain/provider")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            anyhow::anyhow!("prepared optimizer run omitted credential provider")
                        })?;
                    let auth = core
                        .plugins()
                        .authorize_compute(
                            broker.inner(),
                            app,
                            session_ref,
                            recipe_id,
                            digest.as_deref().unwrap_or(""),
                            max_cost_usd,
                            max_rollouts,
                            provider,
                            proposer_model,
                            300,
                        )
                        .await?;
                    if auth.rejected {
                        return Ok(json!({
                            "result": "approval_rejected",
                            "approvalReceiptId": auth.approval_id
                        }));
                    }
                    approval = Some(auth.approval_id);
                }
            }
            let (run, event) = optimizers
                .start_prepared(id.to_string(), digest, approval)
                .await?;
            Ok(json!({ "run": run, "event": event }))
        }
        ("GET", "/v1/optimizers/runs") => {
            let query: crate::optimizers::OptimizerQuery =
                serde_json::from_value(body).unwrap_or_default();
            let runs = optimizers.list(query).await?;
            Ok(json!({ "runs": runs }))
        }
        ("POST", "/v1/optimizers/runs") => {
            let request: crate::optimizers::OptimizerCreateRequest = serde_json::from_value(body)?;
            let (run, event) = optimizers.create(request).await?;
            Ok(json!({ "run": run, "event": event }))
        }
        ("GET", path) if path.starts_with("/v1/optimizers/runs/") && path.ends_with("/events") => {
            let id = path
                .trim_start_matches("/v1/optimizers/runs/")
                .trim_end_matches("/events");
            let after_seq = body.get("after_seq").and_then(Value::as_u64).unwrap_or(0);
            let limit = body.get("limit").and_then(Value::as_i64);
            let events = optimizers
                .events_after(id.to_string(), after_seq, limit)
                .await?;
            Ok(json!({ "events": events }))
        }
        ("GET", path)
            if path.starts_with("/v1/optimizers/runs/") && path.ends_with("/milestone") =>
        {
            let id = path
                .trim_start_matches("/v1/optimizers/runs/")
                .trim_end_matches("/milestone");
            let after_seq = body
                .get("after_seq")
                .or_else(|| body.get("afterSeq"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let timeout_ms = body
                .get("timeout_ms")
                .or_else(|| body.get("timeoutMs"))
                .and_then(Value::as_u64)
                .unwrap_or(30_000);
            let mut kinds = Vec::new();
            if let Some(kind) = body.get("kind").and_then(Value::as_str) {
                kinds.push(kind.to_string());
            }
            if let Some(array) = body.get("kinds").and_then(Value::as_array) {
                kinds.extend(array.iter().filter_map(Value::as_str).map(str::to_string));
            }
            let page = optimizers
                .wait_milestone(id.to_string(), after_seq, kinds, timeout_ms)
                .await?;
            Ok(page)
        }
        ("GET", path)
            if path.starts_with("/v1/optimizers/runs/") && path.ends_with("/state/batch") =>
        {
            let id = path
                .trim_start_matches("/v1/optimizers/runs/")
                .trim_end_matches("/state/batch");
            let slices = body.get("slices").and_then(Value::as_array).map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            });
            let at_seq = body.get("at_seq").and_then(Value::as_u64);
            let batch = optimizers
                .get_state_batch(id.to_string(), slices, at_seq)
                .await?;
            Ok(json!({ "slices": batch }))
        }
        ("GET", path) if path.starts_with("/v1/optimizers/runs/") && path.contains("/state/") => {
            let rest = path.trim_start_matches("/v1/optimizers/runs/");
            let (id, slice) = rest
                .split_once("/state/")
                .ok_or_else(|| anyhow::anyhow!("invalid optimizer state path"))?;
            let at_seq = body.get("at_seq").and_then(Value::as_u64);
            let slice = optimizers
                .get_state(id.to_string(), slice.to_string(), at_seq)
                .await?;
            Ok(json!({ "slice": slice }))
        }
        ("POST", path)
            if path.starts_with("/v1/optimizers/runs/") && path.ends_with("/open_visual") =>
        {
            let id = path
                .trim_start_matches("/v1/optimizers/runs/")
                .trim_end_matches("/open_visual");
            let session_ref = body
                .get("sessionRef")
                .and_then(Value::as_str)
                .map(str::to_string);
            let (run, event) = optimizers
                .open_visual_in_session(id.to_string(), session_ref)
                .await?;
            Ok(json!({ "run": run, "event": event }))
        }
        ("POST", path)
            if path.starts_with("/v1/optimizers/runs/") && path.ends_with("/refresh") =>
        {
            let id = path
                .trim_start_matches("/v1/optimizers/runs/")
                .trim_end_matches("/refresh");
            let run = optimizers.refresh(id.to_string()).await?;
            Ok(json!({ "run": run }))
        }
        ("POST", path)
            if path.starts_with("/v1/optimizers/runs/")
                && path.ends_with("/reconcile_evidence") =>
        {
            let id = path
                .trim_start_matches("/v1/optimizers/runs/")
                .trim_end_matches("/reconcile_evidence");
            let run = optimizers
                .reconcile_evaluation_evidence(id.to_string())
                .await?;
            Ok(json!({ "run": run }))
        }
        ("POST", path) if path.starts_with("/v1/optimizers/runs/") && path.ends_with("/pause") => {
            let id = path
                .trim_start_matches("/v1/optimizers/runs/")
                .trim_end_matches("/pause");
            let (run, event) = optimizers.pause(id.to_string()).await?;
            Ok(json!({ "run": run, "event": event }))
        }
        ("POST", path) if path.starts_with("/v1/optimizers/runs/") && path.ends_with("/resume") => {
            let id = path
                .trim_start_matches("/v1/optimizers/runs/")
                .trim_end_matches("/resume");
            let (run, event) = optimizers.resume(id.to_string()).await?;
            Ok(json!({ "run": run, "event": event }))
        }
        ("POST", path) if path.starts_with("/v1/optimizers/runs/") && path.ends_with("/cancel") => {
            let id = path
                .trim_start_matches("/v1/optimizers/runs/")
                .trim_end_matches("/cancel");
            // This route is the agent surface (MCP relays through it). The
            // caller may name itself in the body; without that, the route
            // itself is the most specific identity available.
            let requested_by = body
                .get("requestedBy")
                .or_else(|| body.get("requested_by"))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("agent:optimizers_ipc");
            let request = crate::optimizers::kernel::CancellationRequest::new(
                crate::optimizers::kernel::CancellationCause::AgentRequested,
                requested_by,
                format!("run:{id}"),
            );
            let (run, event) = optimizers.cancel(id.to_string(), request).await?;
            Ok(json!({ "run": run, "event": event }))
        }
        ("GET", path) if path.starts_with("/v1/optimizers/runs/") && path.ends_with("/result") => {
            let id = path
                .trim_start_matches("/v1/optimizers/runs/")
                .trim_end_matches("/result");
            let result = optimizers.get_result(id.to_string()).await?;
            Ok(json!({ "result": result }))
        }
        ("GET", path) if path.starts_with("/v1/optimizers/runs/") && path.ends_with("/ready") => {
            let id = path
                .trim_start_matches("/v1/optimizers/runs/")
                .trim_end_matches("/ready");
            let timeout_ms = body
                .get("timeout_ms")
                .or_else(|| body.get("timeoutMs"))
                .and_then(Value::as_u64)
                .unwrap_or(15_000);
            let receipt = optimizers
                .await_visual_ready(id.to_string(), timeout_ms)
                .await?;
            Ok(json!({ "receipt": receipt }))
        }
        ("POST", path)
            if path.starts_with("/v1/optimizers/runs/") && path.ends_with("/finalize") =>
        {
            let id = path
                .trim_start_matches("/v1/optimizers/runs/")
                .trim_end_matches("/finalize");
            let result = optimizers.get_result(id.to_string()).await?;
            Ok(json!({ "result": result }))
        }
        ("GET", path) if path.starts_with("/v1/optimizers/runs/") => {
            let id = path.trim_start_matches("/v1/optimizers/runs/");
            let run = optimizers.get(id.to_string()).await?;
            Ok(json!({ "run": run }))
        }
        ("POST", "/v1/optimizers/import_local") => {
            let request: crate::optimizers::OptimizerImportLocalRequest =
                serde_json::from_value(body)?;
            let (run, event) = optimizers.import_local(request).await?;
            Ok(json!({ "run": run, "event": event }))
        }
        ("POST", "/v1/optimizers/reconcile_cloud") => {
            let request: crate::optimizers::OptimizerReconcileRequest =
                serde_json::from_value(body)?;
            let (run, event) = optimizers.reconcile_cloud(request).await?;
            Ok(json!({ "run": run, "event": event }))
        }
        ("GET", "/v1/optimizers/cloud/runs") => {
            let algorithm = body
                .get("algorithm")
                .and_then(Value::as_str)
                .map(str::to_string);
            let status = body
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_string);
            let limit = body.get("limit").and_then(Value::as_i64);
            let runs = optimizers.list_cloud(algorithm, status, limit).await?;
            Ok(json!({ "runs": runs }))
        }
        ("GET", "/v1/optimizers/checkpoints") => {
            let query: crate::optimizers::SavedLoraCheckpointQuery =
                serde_json::from_value(body).unwrap_or_default();
            let page = optimizers.search_saved_lora_checkpoints(query).await?;
            Ok(json!({ "page": page }))
        }
        ("POST", "/v1/optimizers/checkpoints/archive") => {
            let checkpoint_id = body
                .get("checkpointId")
                .or_else(|| body.get("checkpoint_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("checkpoint_id required"))?;
            let checkpoint = optimizers
                .archive_saved_lora_checkpoint(checkpoint_id.to_string())
                .await?;
            Ok(json!({ "checkpoint": checkpoint }))
        }
        ("POST", "/v1/optimizers/checkpoints/import") => {
            let path = body
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("path required"))?;
            let checkpoint = optimizers.import_saved_lora_dir(path.to_string()).await?;
            Ok(json!({ "checkpoint": checkpoint }))
        }
        ("POST", "/v1/optimizers/checkpoints/infer") => {
            let request: crate::optimizers::CheckpointInferRequest = serde_json::from_value(body)?;
            Ok(optimizers.infer_saved_lora(request).await?)
        }
        ("POST", "/v1/optimizers/checkpoints/update") => {
            let checkpoint_id = body
                .get("checkpointId")
                .or_else(|| body.get("checkpoint_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("checkpoint_id required"))?;
            let patch = crate::optimizers::SavedLoraPatchRequest {
                name: body.get("name").and_then(Value::as_str).map(str::to_string),
                description: body
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                tags: body.get("tags").and_then(Value::as_array).map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                }),
            };
            let checkpoint = optimizers
                .patch_saved_lora(checkpoint_id.to_string(), patch)
                .await?;
            Ok(json!({ "checkpoint": checkpoint }))
        }
        ("POST", "/v1/optimizers/checkpoints/publish") => {
            let checkpoint_id = body
                .get("checkpointId")
                .or_else(|| body.get("checkpoint_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("checkpoint_id required"))?;
            let checkpoint = optimizers
                .publish_saved_lora(checkpoint_id.to_string())
                .await?;
            Ok(json!({ "checkpoint": checkpoint }))
        }
        _ => anyhow::bail!("unsupported optimizer IPC route {method} {path}"),
    }
}

/// Agent-facing Trace V5 access.
///
/// Read-only. `open` goes through the same `PresentationService` the native
/// Data page uses, so the agent cannot reach a different inspector, bind one
/// trace's visual to another's digest, or bypass the inspectability policy.
/// Diagnostics IPC. Every route takes the same typed query object and refuses
/// anything it does not recognize, so the adapter's allow-list and this
/// dispatcher fail closed independently rather than trusting each other.
async fn dispatch_diagnostics(
    method: &str,
    path: &str,
    body: Value,
    core: &CoreRuntime,
) -> Result<Value> {
    let service = core.diagnostics_service();
    // The adapter forwards `sessionRef` on every call for provenance; it is not
    // a query field, and a caller that meant to filter by session says so.
    let mut request = body.clone();
    if let Some(object) = request.as_object_mut() {
        object.remove("sessionRef");
    }
    match (method, path) {
        ("POST", "/v1/diagnostics/status") | ("GET", "/v1/diagnostics/status") => {
            Ok(service.status().await)
        }
        ("POST", "/v1/diagnostics/query") => {
            service
                .query(crate::diagnostics::query::parse(&request)?)
                .await
        }
        ("POST", "/v1/diagnostics/tail") => {
            service
                .tail(crate::diagnostics::query::parse(&request)?)
                .await
        }
        ("POST", "/v1/diagnostics/explain") => {
            service
                .explain(crate::diagnostics::query::parse(&request)?)
                .await
        }
        ("POST", "/v1/diagnostics/bundle") => {
            service
                .bundle(crate::diagnostics::query::parse(&request)?)
                .await
        }
        ("POST", "/v1/diagnostics/clear-index") => service.clear_index().await,
        (method, path) => anyhow::bail!("unknown diagnostics route {method} {path}"),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SecretsEmptyRequest {}

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SecretsListRequest {
    provider: Option<String>,
    scope: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SecretsLocatorRequest {
    workspace_root_ref: String,
    relative_path: String,
    provider: String,
    variable: String,
    #[serde(default)]
    label: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SecretsSourceRequest {
    #[serde(default)]
    locator_id: Option<String>,
    #[serde(default)]
    workspace_root_ref: Option<String>,
    #[serde(default)]
    relative_path: Option<String>,
    provider: String,
    variable: String,
    #[serde(default)]
    label: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SecretsIdRequest {
    locator_id: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SecretsRevokeUseRequest {
    #[serde(default)]
    capability_id: Option<String>,
    #[serde(default)]
    run_id: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SecretsUseRequest {
    #[serde(default)]
    locator_id: Option<String>,
    #[serde(default)]
    source_id: Option<String>,
    #[serde(default)]
    secret_id: Option<String>,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    recipe_id: Option<String>,
    #[serde(default)]
    workload: Option<String>,
}

fn parse_secrets_request<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    if body.as_object().is_some_and(|object| {
        object.keys().any(|key| {
            matches!(
                key.to_ascii_lowercase().as_str(),
                "value" | "secret" | "apikey" | "api_key" | "token" | "password" | "credential"
            )
        })
    }) {
        return Err(anyhow::Error::new(crate::error::StructuredFailure::new(
            crate::secrets::lease::CREDENTIAL_LOCATOR_VALUE_SUPPLIED,
            "credential values are not accepted by the locator registry",
            "Pass an opaque workspaceRootRef, a relative path, and the environment variable name only.",
        )));
    }
    serde_json::from_value(body).map_err(|error| {
        anyhow::Error::new(crate::error::StructuredFailure::new(
            "credential_locator_invalid_request",
            format!("invalid secrets request: {error}"),
            "Use only the documented operation fields; absolute paths and credential values are never accepted.",
        ))
    })
}

fn structured_credential_error(error: anyhow::Error) -> anyhow::Error {
    let Some(failure) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<crate::secrets::lease::CredentialError>())
    else {
        return error;
    };
    let code = match failure.code.as_str() {
        crate::secrets::lease::CREDENTIAL_SOURCE_UNCONFIGURED => {
            crate::secrets::lease::CREDENTIAL_SOURCE_UNCONFIGURED
        }
        crate::secrets::lease::CREDENTIAL_VALUE_MISSING => {
            crate::secrets::lease::CREDENTIAL_VALUE_MISSING
        }
        crate::secrets::lease::CREDENTIAL_VALUE_UNLOADED => {
            crate::secrets::lease::CREDENTIAL_VALUE_UNLOADED
        }
        crate::secrets::lease::CREDENTIAL_LOCATOR_UNAPPROVED_WORKSPACE => {
            crate::secrets::lease::CREDENTIAL_LOCATOR_UNAPPROVED_WORKSPACE
        }
        crate::secrets::lease::CREDENTIAL_PATH_ESCAPE => {
            crate::secrets::lease::CREDENTIAL_PATH_ESCAPE
        }
        crate::secrets::lease::CREDENTIAL_LOCATOR_NOT_REGULAR_FILE => {
            crate::secrets::lease::CREDENTIAL_LOCATOR_NOT_REGULAR_FILE
        }
        crate::secrets::lease::CREDENTIAL_LOCATOR_VALUE_SUPPLIED => {
            crate::secrets::lease::CREDENTIAL_LOCATOR_VALUE_SUPPLIED
        }
        crate::secrets::lease::CREDENTIAL_LOCATOR_PICKER_MISMATCH => {
            crate::secrets::lease::CREDENTIAL_LOCATOR_PICKER_MISMATCH
        }
        crate::secrets::lease::CREDENTIAL_LOCATOR_BROAD_DISCOVERY => {
            crate::secrets::lease::CREDENTIAL_LOCATOR_BROAD_DISCOVERY
        }
        crate::secrets::lease::CREDENTIAL_LOCATOR_COMPAT_IMPORT => {
            crate::secrets::lease::CREDENTIAL_LOCATOR_COMPAT_IMPORT
        }
        crate::secrets::lease::CREDENTIAL_LOCATOR_LIMIT => {
            crate::secrets::lease::CREDENTIAL_LOCATOR_LIMIT
        }
        crate::secrets::lease::CREDENTIAL_DECISION_EXCEEDS_REQUEST => {
            crate::secrets::lease::CREDENTIAL_DECISION_EXCEEDS_REQUEST
        }
        crate::secrets::lease::CREDENTIAL_SOURCE_CONSENT_PENDING => {
            crate::secrets::lease::CREDENTIAL_SOURCE_CONSENT_PENDING
        }
        _ => return error,
    };
    anyhow::Error::new(
        crate::error::StructuredFailure::new(
            code,
            failure.message.clone(),
            "Inspect workspace_roots, bindings, locators, and source status before retrying.",
        )
        .retryable(failure.retryable),
    )
}

async fn dispatch_secrets(
    method: &str,
    path: &str,
    body: Value,
    core: &CoreRuntime,
    app: &AppHandle,
) -> Result<Value> {
    let lower = path.to_ascii_lowercase();
    if lower.contains("create")
        || lower.contains("replace")
        || lower.contains("delete")
        || lower.contains("reveal")
        || lower.contains("export")
        || lower.contains("commit")
        || lower.contains("grant")
        || lower.contains("get")
        || lower.contains("test")
        || lower.contains("value")
    {
        anyhow::bail!(
            "secrets MCP cannot create, reveal, export, commit, or test credentials; \
             list credential locations, request registration, or request bounded use. \
             Native approval cards settle agent requests."
        );
    }
    let secrets = core.secrets();
    let session_id = body
        .get("sessionRef")
        .or_else(|| body.get("session_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let mut request = body;
    if let Some(object) = request.as_object_mut() {
        object.remove("sessionRef");
        object.remove("session_id");
        object.remove("operation");
    }
    match (method, path) {
        ("POST", "/v1/secrets/workspace_roots") => {
            let _: SecretsEmptyRequest = parse_secrets_request(request)?;
            Ok(json!({
                "workspaceRoots": secrets.workspace_roots(),
            }))
        }
        ("POST", "/v1/secrets/bindings")
        | ("GET", "/v1/secrets")
        | ("POST", "/v1/secrets")
        | ("POST", "/v1/secrets/list") => {
            let filters: SecretsListRequest = parse_secrets_request(request)?;
            let mut bindings = secrets.bindings().map_err(structured_credential_error)?;
            if let Some(provider) = filters.provider.as_deref() {
                bindings.retain(|binding| binding.provider == provider);
            }
            let _ = filters.scope;
            Ok(json!({
                "bindings": bindings,
                "guidance": "Bindings contain source licenses and load state only. They never contain values or masked suffixes."
            }))
        }
        ("POST", "/v1/secrets/locators") => {
            let filters: SecretsListRequest = parse_secrets_request(request)?;
            let mut locators = secrets.locators(false).map_err(structured_credential_error)?;
            if let Some(provider) = filters.provider.as_deref() {
                locators.retain(|locator| locator.provider == provider);
            }
            let _ = filters.scope;
            Ok(json!({ "locators": locators }))
        }
        ("POST", "/v1/secrets/locator_request") => {
            let session_id = session_id.ok_or_else(|| anyhow::Error::new(
                crate::error::StructuredFailure::new(
                    "credential_access_requires_session",
                    "remembering a credential location requires an owning session",
                    "Retry from the active agent session.",
                )
            ))?;
            let input: SecretsLocatorRequest = parse_secrets_request(request)?;
            let label = input.label.unwrap_or_else(|| input.provider.clone());
            let locator = secrets
                .remember_workspace_locator_pending(
                    &input.workspace_root_ref,
                    &input.relative_path,
                    &input.provider,
                    &input.variable,
                    &label,
                )
                .map_err(structured_credential_error)?;
            match locator.state {
                crate::secrets::CredentialLocatorState::Observed => {
                    return Ok(json!({ "status": "remembered", "locator": locator }));
                }
                crate::secrets::CredentialLocatorState::WorkspaceAuthorityRevoked => {
                    return Err(structured_credential_error(
                        crate::secrets::lease::CredentialError::new(
                            crate::secrets::lease::CREDENTIAL_LOCATOR_UNAPPROVED_WORKSPACE,
                            "locator",
                            false,
                            "This folder is allowed again. Forget and remember to restore.",
                        )
                        .anyhow(),
                    ));
                }
                crate::secrets::CredentialLocatorState::ApprovalPending => {}
                _ => {
                    return Err(anyhow::anyhow!(
                        "credential locator is not available to remember"
                    ));
                }
            }
            let codex = app.state::<Arc<crate::codex::CodexManager>>();
            let outcome = codex
                .approvals
                .authorize_host_outcome(
                    app,
                    Some(&session_id),
                    crate::session::approval::ApprovalKind::CredentialAccess {
                        consent: crate::session::approval::CredentialConsent::RememberLocator,
                        provider: input.provider,
                        purpose: "Remember this credential location without loading its value".into(),
                        locator_id: Some(locator.id.clone()),
                        display_path: Some(locator.display_path.clone()),
                        variable: Some(input.variable),
                        switch_from_display: None,
                    },
                )
                .await;
            match outcome {
                Ok((_, crate::session::approval::ApprovalDecision::Credential {
                    outcome: crate::session::approval::CredentialDecision::RememberLocator,
                })) => secrets
                    .settle_remembered_locator(&locator.id)
                    .map_err(structured_credential_error)?,
                Ok(_) => unreachable!("credential decision validation refused this outcome"),
                Err(error) => {
                    let _ = secrets.deny_pending_locator(&locator.id);
                    return Err(structured_credential_error(error));
                }
            }
            let remembered = secrets
                .locators(false)
                .map_err(structured_credential_error)?
                .into_iter()
                .find(|row| row.id == locator.id);
            Ok(json!({ "status": "remembered", "locator": remembered }))
        }
        ("POST", "/v1/secrets/source_request") => {
            let session_id = session_id.ok_or_else(|| anyhow::Error::new(
                crate::error::StructuredFailure::new(
                    "credential_access_requires_session",
                    "registering a credential source requires an owning session",
                    "Retry from the active agent session.",
                )
            ))?;
            let input: SecretsSourceRequest = parse_secrets_request(request)?;
            if input.locator_id.is_some()
                && (input.workspace_root_ref.is_some() || input.relative_path.is_some())
            {
                anyhow::bail!(
                    "source_request accepts locatorId or workspaceRootRef plus relativePath, not both"
                );
            }
            let label = input.label.clone().unwrap_or_else(|| input.provider.clone());
            let locator = if let Some(locator_id) = input.locator_id.as_deref() {
                secrets
                    .locators(false)
                    .map_err(structured_credential_error)?
                    .into_iter()
                    .find(|row| row.id == locator_id)
                    .ok_or_else(|| anyhow::anyhow!("credential locator {locator_id} was not found"))?
            } else {
                let workspace_root_ref = input.workspace_root_ref.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("workspaceRootRef is required when locatorId is omitted")
                })?;
                let relative_path = input.relative_path.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("relativePath is required when locatorId is omitted")
                })?;
                secrets
                    .remember_workspace_locator_pending(
                        workspace_root_ref,
                        relative_path,
                        &input.provider,
                        &input.variable,
                        &label,
                    )
                    .map_err(structured_credential_error)?
            };
            if locator.provider != input.provider.trim().to_ascii_lowercase()
                || locator.variable != input.variable.trim()
            {
                anyhow::bail!(
                    "source_request provider and variable must match the selected locator"
                );
            }
            if locator.loaded {
                return Ok(json!({ "status": "registered", "locator": locator }));
            }
            secrets
                .begin_source_consent(&input.provider, &input.variable)
                .map_err(structured_credential_error)?;
            let was_existing = match secrets.prepare_register_approval(&locator.id) {
                Ok(value) => value,
                Err(error) => {
                    secrets.end_source_consent(&input.provider, &input.variable);
                    return Err(structured_credential_error(error));
                }
            };
            let switch_from_display = match secrets.locators(false) {
                Ok(rows) => rows
                    .into_iter()
                    .find(|row| {
                        row.preferred
                            && row.id != locator.id
                            && row.provider == locator.provider
                            && row.variable == locator.variable
                    })
                    .map(|row| row.display_path),
                Err(error) => {
                    if was_existing {
                        let _ = secrets.settle_remembered_locator(&locator.id);
                    } else {
                        let _ = secrets.deny_pending_locator(&locator.id);
                    }
                    secrets.end_source_consent(&input.provider, &input.variable);
                    return Err(structured_credential_error(error));
                }
            };
            let codex = app.state::<Arc<crate::codex::CodexManager>>();
            let outcome = codex
                .approvals
                .authorize_host_outcome(
                    app,
                    Some(&session_id),
                    crate::session::approval::ApprovalKind::CredentialAccess {
                        consent: crate::session::approval::CredentialConsent::RegisterSource,
                        provider: input.provider.clone(),
                        purpose: "Register this location as the provider source".into(),
                        locator_id: Some(locator.id.clone()),
                        display_path: Some(locator.display_path.clone()),
                        variable: Some(input.variable.clone()),
                        switch_from_display,
                    },
                )
                .await;
            let result: Result<Value> = match outcome {
                Ok((_, crate::session::approval::ApprovalDecision::Credential {
                    outcome: crate::session::approval::CredentialDecision::RememberLocator,
                })) => secrets
                    .settle_remembered_locator(&locator.id)
                    .map(|_| json!({ "status": "remembered" }))
                    .map_err(structured_credential_error),
                Ok((_, crate::session::approval::ApprovalDecision::Credential {
                    outcome: crate::session::approval::CredentialDecision::RegisterSource,
                })) => secrets
                    .register_locator(&locator.id)
                    .map(|registered| {
                        json!({ "status": "registered", "locator": registered })
                    })
                    .map_err(structured_credential_error),
                Ok(_) => unreachable!("credential decision validation refused this outcome"),
                Err(error) => Err(structured_credential_error(error)),
            };
            if result.is_err() {
                if was_existing {
                    let _ = secrets.settle_remembered_locator(&locator.id);
                } else {
                    let _ = secrets.deny_pending_locator(&locator.id);
                }
            }
            secrets.end_source_consent(&input.provider, &input.variable);
            result
        }
        ("POST", "/v1/secrets/locator_status")
        | ("POST", "/v1/secrets/source_status") => {
            let input: SecretsIdRequest = parse_secrets_request(request)?;
            let locator = secrets
                .locators(false)
                .map_err(structured_credential_error)?
                .into_iter()
                .find(|row| row.id == input.locator_id)
                .ok_or_else(|| anyhow::anyhow!("credential locator was not found"))?;
            Ok(json!({ "locator": locator }))
        }
        ("POST", "/v1/secrets/locator_remove") => {
            let input: SecretsIdRequest = parse_secrets_request(request)?;
            let codex = app.state::<Arc<crate::codex::CodexManager>>();
            let _ = codex
                .approvals
                .expire_credential_locator(app, &input.locator_id, "credential_locator_forgotten")
                .await;
            secrets
                .forget_locator(&input.locator_id)
                .map_err(structured_credential_error)?;
            Ok(json!({ "status": "forgotten" }))
        }
        ("POST", "/v1/secrets/source_remove") => {
            let input: SecretsIdRequest = parse_secrets_request(request)?;
            let codex = app.state::<Arc<crate::codex::CodexManager>>();
            let _ = codex
                .approvals
                .expire_credential_locator(app, &input.locator_id, "credential_source_removed")
                .await;
            secrets
                .remove_locator_source(&input.locator_id)
                .map_err(structured_credential_error)?;
            Ok(json!({
                "status": "unregistered",
                "sourceRegistered": false,
                "guidance": "This removes the reusable source registration. Use use_revoke to revoke only a run capability."
            }))
        }
        ("POST", "/v1/secrets/use_revoke") => {
            let input: SecretsRevokeUseRequest = parse_secrets_request(request)?;
            match (input.capability_id.as_deref(), input.run_id.as_deref()) {
                (Some(capability_id), None) => {
                    secrets
                        .revoke_capability(capability_id, "agent")
                        .map_err(structured_credential_error)?;
                    Ok(json!({
                        "status": "revoked",
                        "capabilityId": capability_id,
                        "sourceRegistered": true,
                    }))
                }
                (None, Some(run_id)) => {
                    let revoked = secrets
                        .revoke_run(run_id)
                        .map_err(structured_credential_error)?;
                    Ok(json!({
                        "status": "revoked",
                        "runId": run_id,
                        "capabilityIds": revoked,
                        "sourceRegistered": true,
                    }))
                }
                _ => anyhow::bail!(
                    "use_revoke requires exactly one of capabilityId or runId"
                ),
            }
        }
        ("POST", "/v1/secrets/import") => {
            Err(anyhow::Error::new(crate::error::StructuredFailure::new(
                crate::secrets::lease::CREDENTIAL_LOCATOR_COMPAT_IMPORT,
                "request_env_import has been replaced by the credential locator registry",
                "Call workspace_roots_list, locators_list, source_request, then request_use. Absolute source paths are not accepted.",
            )))
        }
        ("POST", "/v1/secrets/use") => {
            let input: SecretsUseRequest = parse_secrets_request(request)?;
            let target_count = [
                input.locator_id.is_some(),
                input.source_id.is_some(),
                input.secret_id.is_some(),
            ]
            .into_iter()
            .filter(|present| *present)
            .count();
            if target_count != 1 {
                anyhow::bail!("request_use requires exactly one of locatorId, sourceId, or secretId");
            }
            let secret_id = if let Some(locator_id) = input.locator_id.as_deref() {
                secrets
                    .source_for_locator(locator_id)
                    .map_err(structured_credential_error)?
            } else {
                input.source_id.or(input.secret_id).expect("one target was checked")
            };
            let run_id = input.run_id.unwrap_or_else(|| "session".into());
            let recipe_id = input.recipe_id.unwrap_or_else(|| "session".into());
            // An MCP client may select a known workload contract, never arbitrary
            // operations or spend limits.  Codex is a Responses client, whereas
            // the generic assistant path remains Chat Completions only.
            let policy = agent_use_policy(input.workload.as_deref())?;
            let mut result =
                secrets.request_use(&secret_id, &run_id, &recipe_id, policy.clone(), "agent")?;
            if result.status == "approval_required" {
                let provider_display = secrets
                    .list(None, None)?
                    .into_iter()
                    .find(|entry| entry.id == secret_id)
                    .map(|entry| entry.provider)
                    .unwrap_or_else(|| "Unknown provider".into());
                let session_id = session_id.as_deref()
                    .ok_or_else(|| anyhow::anyhow!(
                        "credential_access_requires_session: request_use must name its owning session"
                    ))?;
                let request_id = result.request_id.clone().ok_or_else(|| {
                    anyhow::anyhow!(
                        "credential_access_request_invalid: pending use has no request id"
                    )
                })?;
                let purpose = format!(
                    "Issue a run-scoped Workshop proxy capability for recipe {recipe_id}, run {run_id}; operations={}; maxCalls={}; maxCostUsd={}",
                    policy.operations.join(","),
                    policy.max_calls,
                    policy.max_cost_usd,
                );
                let codex = app.state::<Arc<crate::codex::CodexManager>>();
                let approval = codex
                    .approvals
                    .authorize_host(
                        app,
                        Some(&session_id),
                        crate::session::approval::ApprovalKind::CredentialAccess {
                            consent: crate::session::approval::CredentialConsent::IssueLease,
                            provider: provider_display,
                            purpose,
                            locator_id: input.locator_id.clone(),
                            display_path: None,
                            variable: None,
                            switch_from_display: None,
                        },
                    )
                    .await;
                match approval {
                    Ok(_) => {
                        result = secrets.grant_pending(&request_id, "agent", false)?;
                    }
                    Err(error) => {
                        let _ = secrets.deny_pending(&request_id, "agent");
                        return Err(structured_credential_error(
                            error.context("credential access was not approved"),
                        ));
                    }
                }
            }
            Ok(json!({
                "status": result.status,
                "requestId": result.request_id,
                "capabilityId": result.capability_id,
                "proxyOrigin": result.proxy_origin,
                "handle": result.handle,
                "provider_routes": result.provider_routes,
                "requestedPolicy": {
                    "operations": policy.operations,
                    "maxCalls": policy.max_calls,
                    "maxCostUsd": policy.max_cost_usd,
                },
                "guidance": "The native approval has settled. Use provider_routes.openai_base unchanged with OPENAI_API_KEY=workshop-proxy. Do not construct a route from proxyOrigin or handle."
            }))
        }
        (method, path) => anyhow::bail!("unknown secrets route {method} {path}"),
    }
}

/// Product-owned, least-privilege policies available to the agent-facing
/// secrets adapter.  The agent can name a workload shape, but it cannot widen
/// its operation set, model allowance, budget, or lifetime.
fn agent_use_policy(workload: Option<&str>) -> Result<crate::secrets::SecretsUsePolicy> {
    let mut policy = crate::secrets::SecretsUsePolicy::default();
    match workload.unwrap_or("chat_completions") {
        "chat_completions" => Ok(policy),
        "codex_responses" => {
            policy.operations = vec!["responses.create".into()];
            Ok(policy)
        }
        other => anyhow::bail!(
            "unsupported secrets workload `{other}`; use chat_completions or codex_responses"
        ),
    }
}

/// Fail an inspector request with a code that names the recovery, not with
/// "trace not found".
///
/// Containers sealed a trace and Workshop's index did not have it, because
/// nothing carries a sealed trace across that boundary on its own. The agent
/// could only guess at binding shapes and paths. Say which side is missing it,
/// and which call fixes that.
async fn require_indexed_trace(core: &CoreRuntime, trace_id: &str) -> Result<()> {
    if core.data().get_trace(trace_id.to_string()).await.is_ok() {
        return Ok(());
    }
    Err(anyhow::Error::new(
        crate::error::StructuredFailure::new(
            "trace_not_indexed",
            format!("{trace_id} is not in this Workshop's trace index"),
            "A container can hold a sealed trace this Workshop has never imported. Import it by identity with trace_manage import {container_id, rollout_id}, then open it.",
        )
        .retryable(false)
        .with_details(json!({"traceId": trace_id})),
    ))
}

/// Import the seal a terminal rollout record names, unless it is already
/// indexed. Silent when the container announces no trace: a rollout that sealed
/// nothing is not a failure.
async fn import_terminal_trace(
    core: &CoreRuntime,
    container_id: &str,
    rollout_id: &str,
    state: &Value,
) -> Result<Value> {
    let Some(reference) = state.get("trace").filter(|value| value.is_object()) else {
        return Ok(Value::Null);
    };
    if let Some(trace_id) = reference.get("trace_id").and_then(Value::as_str) {
        if core.data().get_trace(trace_id.to_string()).await.is_ok() {
            return Ok(json!({"indexed": true, "duplicate": true, "traceId": trace_id}));
        }
    }
    import_container_trace(core, container_id, rollout_id).await
}

/// Import a container's sealed trace by identity.
///
/// The container registry is the trusted side of this call: the agent names a
/// container and a rollout, and Workshop resolves the URL itself. No path or
/// URL crosses the agent boundary, which is the reason the trace tools reject
/// those arguments in the first place.
///
/// Preference order is deliberate. A full Trace V5 bundle archive is the only
/// input that can be *trusted*, indexed, and projected for the inspector; a
/// lite seal document is recorded as a provenance-bearing import but cannot be
/// projected, and this says so rather than reporting a success the inspector
/// will contradict.
pub(crate) async fn import_container_trace(
    core: &CoreRuntime,
    container_id: &str,
    rollout_id: &str,
) -> Result<Value> {
    let (result, event, _) =
        import_container_trace_into(core.data(), container_id, rollout_id).await?;
    core.broadcast_committed(event);
    Ok(result)
}

/// The import itself, over a `DataStore` rather than the whole runtime.
///
/// The eval worker needs exactly this and holds no `CoreRuntime`; splitting it
/// out is what lets a rollout seal its replay at the moment it finishes instead
/// of waiting for an agent to ask for it later. The committed event is returned
/// rather than broadcast so each caller places it on its own bus.
pub(crate) async fn import_container_trace_into(
    data: &crate::data::DataStore,
    container_id: &str,
    rollout_id: &str,
) -> Result<(
    Value,
    Option<crate::storage::AppEvent>,
    Vec<ImportedTraceFrame>,
)> {
    let container = data.get_container(container_id.to_string()).await?;
    let base = validated_loopback_rollout_base(
        container
            .base_url
            .as_deref()
            .context("container has no base URL")?,
    )?;
    let client = crate::http::http_client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(limits::VISUALS_IPC_ROLL_TIMEOUT)
        .build()?;
    let state = get_rollout_status(&client, &base, rollout_id)
        .await?
        .context("unknown rollout")?;
    let reference = state.get("trace").filter(|value| value.is_object());
    let staging = data.staging_root().join("container-seals");
    fs::create_dir_all(&staging)?;

    // A bundle archive first: only a self-contained Trace V5 archive can be
    // trusted, indexed, and projected into the inspector. Lane E serves the
    // capture-supervisor zip at `/rollouts/{id}/trace/bundle` (and may also
    // announce `bundle_url` on the terminal record).
    let mut bundle = None;
    for route in sealed_trace_bundle_routes(rollout_id, reference) {
        if let Some(bytes) = fetch_trace_artifact(&client, &base, &route).await? {
            bundle = Some(bytes);
            break;
        }
    }
    let (source_path, source_kind) = if let Some(bytes) = bundle {
        let path = staging.join(format!("{rollout_id}.trace-bundle.zip"));
        fs::write(&path, bytes)?;
        (path, "container_bundle")
    } else {
        let mut seal = None;
        for route in sealed_trace_lite_routes(rollout_id, reference) {
            if let Some(bytes) = fetch_trace_artifact(&client, &base, &route).await? {
                seal = Some(bytes);
                break;
            }
        }
        let Some(bytes) = seal else {
            return Err(anyhow::Error::new(
                crate::error::StructuredFailure::new(
                    "trace_not_sealed_by_container",
                    format!("{container_id} has no sealed trace for rollout {rollout_id}"),
                    "Wait for the rollout to reach a terminal state, then import again. A running rollout has sealed nothing yet.",
                )
                .retryable(true)
                .with_details(json!({"containerId": container_id, "rolloutId": rollout_id})),
            ));
        };
        let path = staging.join(format!("{rollout_id}.trace-v5.json"));
        fs::write(&path, bytes)?;
        (path, "container_seal")
    };

    let result = data
        .ingest_trace_bundle(crate::trace_ingest::TraceBundleIngestRequest {
            source_path: source_path.display().to_string(),
            source_kind: Some(source_kind.to_owned()),
            title: Some(format!("{rollout_id} · {container_id}")),
            source_uri: Some(format!("{base}/rollouts/{rollout_id}")),
            container_id: Some(container.id.clone()),
        })
        .await;
    let (result, event) = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = fs::remove_file(&source_path);
            return Err(error);
        }
    };
    let (frames, max_step, trace_provenance) =
        (if source_kind == "container_bundle" && result.trusted {
            extract_imported_trace_frames(&source_path, rollout_id)
        } else {
            Ok((Vec::new(), None, None))
        })?;
    let _ = fs::remove_file(&source_path);

    let indexed: Vec<Value> = result
        .traces
        .iter()
        .map(|trace| {
            json!({
                // `traceId` is Workshop's stable local index identity.
                "traceId": trace.id,
                // `producerTraceId` is the identity sealed inside Trace V5.
                // They are not interchangeable and usually differ.
                "producerTraceId": trace.metadata.get("producerTraceId"),
                "digest": trace.digest,
            })
        })
        .collect();
    Ok((
        json!({
            "containerId": container_id,
            "rolloutId": rollout_id,
            "sourceKind": source_kind,
            "compatibilityLevel": result.compatibility_level,
            "trusted": result.trusted,
            "duplicate": result.duplicate,
            "inputDigest": result.input_digest,
            "bundleDigest": result.bundle_digest,
            "archiveDigest": result.archive_digest,
            "traceProvenance": trace_provenance,
            // Inspectable only when a capture-supervisor Trace V5 bundle indexed
            // real traces. A lite seal is retained with its provenance but cannot
            // be projected; saying otherwise is how an agent retries an inspector
            // that can never render.
            "inspectable": source_kind == "container_bundle" && !indexed.is_empty(),
            "traces": indexed,
            "note": if indexed.is_empty() {
                "Imported as a provenance record only: this container returned a lite seal, not a self-contained Trace V5 bundle, so it cannot be projected into the inspector."
            } else {
                "Sealed Trace V5 is now indexed in Workshop."
            },
            "embeddedFrameCount": frames.len(),
            "maxStep": max_step,
        }),
        event,
        frames,
    ))
}

#[derive(Clone, Debug)]
pub(crate) struct ImportedTraceFrame {
    pub bytes: Vec<u8>,
    pub digest: String,
    pub width: u32,
    pub height: u32,
    pub step: i64,
    pub producer_digest: Option<String>,
}

fn extract_imported_trace_frames(
    archive_path: &std::path::Path,
    rollout_id: &str,
) -> Result<(Vec<ImportedTraceFrame>, Option<i64>, Option<Value>)> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("open imported trace archive {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("open imported trace ZIP")?;
    let sealed_names = (0..archive.len())
        .filter_map(|index| {
            let entry = archive.by_index(index).ok()?;
            let name = entry.name().to_string();
            (name.contains("/sealed/") && name.ends_with(".json")).then_some(name)
        })
        .collect::<Vec<_>>();
    let mut frames = Vec::new();
    let mut max_step = None::<i64>;
    let mut trace_provenance = None::<Value>;
    for name in sealed_names {
        let document = {
            let mut entry = archive.by_name(&name)?;
            if entry.size() > limits::MAX_IMPORTED_TRACE_BYTES {
                anyhow::bail!("sealed trace document exceeded import limit");
            }
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes)?;
            serde_json::from_slice::<Value>(&bytes).context("decode sealed trace document")?
        };
        if document
            .pointer("/identity/rollout_id")
            .and_then(Value::as_str)
            != Some(rollout_id)
        {
            continue;
        }
        if let Some(provenance) = document.get("provenance").filter(|value| value.is_object()) {
            if let Some(previous) = &trace_provenance {
                anyhow::ensure!(
                    previous == provenance,
                    "sealed traces for rollout `{rollout_id}` disagree on provenance"
                );
            } else {
                trace_provenance = Some(provenance.clone());
            }
        }
        let artifacts = document
            .get("artifacts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|artifact| {
                let id = artifact.get("artifact_id")?.as_str()?.to_string();
                (artifact.get("media_type").and_then(Value::as_str) == Some("image/png"))
                    .then_some((id, artifact.clone()))
            })
            .collect::<std::collections::HashMap<_, _>>();
        for event in document
            .get("events")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for pointer in ["/payload/env_steps", "/payload/step", "/payload/step_index"] {
                if let Some(step) = event.pointer(pointer).and_then(Value::as_i64) {
                    max_step = Some(max_step.map_or(step, |current| current.max(step)));
                }
            }
        }
        let mut imported_artifacts = std::collections::HashSet::new();
        for event in document
            .get("events")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|event| event.get("event_type").and_then(Value::as_str) == Some("frame"))
        {
            let artifact_id = event
                .get("artifact_ids")
                .and_then(Value::as_array)
                .and_then(|ids| ids.first())
                .and_then(Value::as_str)
                .context("sealed frame event omitted its PNG artifact")?;
            let step = match event.pointer("/payload/step").and_then(Value::as_i64) {
                Some(step) => step,
                None if imported_artifacts.contains(artifact_id) => continue,
                None => anyhow::bail!(
                    "sealed frame event omitted step for unique artifact `{artifact_id}`"
                ),
            };
            let artifact = artifacts
                .get(artifact_id)
                .context("sealed frame event references an unknown PNG artifact")?;
            let uri = artifact
                .get("uri")
                .and_then(Value::as_str)
                .context("sealed frame artifact omitted bundle URI")?;
            let bytes = {
                let mut blob = archive
                    .by_name(uri)
                    .context("sealed frame blob missing from bundle")?;
                if blob.size() > 16 * 1024 * 1024 {
                    anyhow::bail!("sealed frame blob exceeded 16 MiB");
                }
                let mut bytes = Vec::with_capacity(blob.size() as usize);
                blob.read_to_end(&mut bytes)?;
                bytes
            };
            let actual = format!("sha256:{:x}", sha2::Sha256::digest(&bytes));
            let digest = artifact
                .get("digest")
                .and_then(Value::as_str)
                .context("sealed frame artifact omitted digest")?;
            if actual != digest {
                anyhow::bail!("sealed frame artifact digest mismatch");
            }
            if artifact.get("size_bytes").and_then(Value::as_u64) != Some(bytes.len() as u64) {
                anyhow::bail!("sealed frame artifact size mismatch");
            }
            let decoder = png::Decoder::new(bytes.as_slice());
            let mut reader = decoder.read_info().context("decode sealed frame PNG")?;
            let (width, height) = (reader.info().width, reader.info().height);
            let mut decoded = vec![0; reader.output_buffer_size()];
            reader
                .next_frame(&mut decoded)
                .context("fully decode sealed frame PNG")?;
            frames.push(ImportedTraceFrame {
                bytes,
                digest: digest.to_string(),
                width,
                height,
                step,
                producer_digest: event
                    .pointer("/payload/source_event_digest")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
            imported_artifacts.insert(artifact_id.to_string());
        }
    }
    Ok((frames, max_step, trace_provenance))
}

/// Capture-supervisor bundle first (Lane E `/rollouts/{id}/trace/bundle`),
/// then any announced `bundle_url`. Duplicate URLs are collapsed.
pub(crate) fn sealed_trace_bundle_routes(
    rollout_id: &str,
    reference: Option<&Value>,
) -> Vec<String> {
    let mut routes = Vec::new();
    let mut push = |route: String| {
        if !route.is_empty() && !routes.iter().any(|existing| existing == &route) {
            routes.push(route);
        }
    };
    if let Some(url) = reference
        .and_then(|value| value.get("bundle_url"))
        .and_then(Value::as_str)
    {
        push(url.to_owned());
    }
    push(format!("/rollouts/{rollout_id}/trace/bundle"));
    routes
}

fn sealed_trace_lite_routes(rollout_id: &str, reference: Option<&Value>) -> Vec<String> {
    let mut routes = Vec::new();
    let mut push = |route: String| {
        if !route.is_empty() && !routes.iter().any(|existing| existing == &route) {
            routes.push(route);
        }
    };
    if let Some(url) = reference
        .and_then(|value| value.get("url"))
        .and_then(Value::as_str)
    {
        push(url.to_owned());
    }
    push(format!("/rollouts/{rollout_id}/trace"));
    routes
}

/// Fetch one trace artifact, treating 404 as "this container does not offer it"
/// rather than as a failure.
async fn fetch_trace_artifact(
    client: &reqwest::Client,
    base: &str,
    route: &str,
) -> Result<Option<Vec<u8>>> {
    let url = resolve_declared_url(base, route)?;
    let response = client.get(url).send().await;
    let response = match response {
        Ok(response) => response,
        // A container that never implemented the route is a fallback, not an
        // error worth failing the whole import over.
        Err(_) => return Ok(None),
    };
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let bytes = response.error_for_status()?.bytes().await?;
    if bytes.len() as u64 > limits::MAX_IMPORTED_TRACE_BYTES {
        anyhow::bail!(
            "container trace artifact exceeded {} bytes",
            limits::MAX_IMPORTED_TRACE_BYTES
        );
    }
    Ok(Some(bytes.to_vec()))
}

async fn dispatch_experiments(
    method: &str,
    path: &str,
    body: Value,
    core: &CoreRuntime,
) -> Result<Value> {
    match (method, path) {
        ("POST", "/v1/experiments") | ("POST", "/v1/experiments/") => {
            let mut payload = body;
            if payload.get("createdAt").is_none() {
                payload["createdAt"] = json!(chrono::Utc::now().to_rfc3339());
            }
            let request: crate::experiments::ExperimentCreateRequest =
                serde_json::from_value(payload)?;
            Ok(json!({"experiment": core.data().experiment_create(request).await?}))
        }
        ("POST", path) if path.starts_with("/v1/experiments/") && path.ends_with("/children") => {
            let parent_id = path
                .trim_start_matches("/v1/experiments/")
                .trim_end_matches("/children")
                .trim_end_matches('/');
            anyhow::ensure!(
                !parent_id.is_empty() && !parent_id.contains('/'),
                "invalid experiment children path"
            );
            let mut payload = body;
            payload["parentExperimentId"] = json!(parent_id);
            if payload.get("createdAt").is_none() && payload.get("created_at").is_none() {
                payload["createdAt"] = json!(chrono::Utc::now().to_rfc3339());
            }
            let request: crate::experiments::ExperimentChildCreateRequest =
                serde_json::from_value(payload)?;
            Ok(json!({
                "experiment": core.data().experiment_create_child(request).await?
            }))
        }
        ("POST", path) if path.starts_with("/v1/experiments/") && path.ends_with("/relate") => {
            let experiment_id = path
                .trim_start_matches("/v1/experiments/")
                .trim_end_matches("/relate")
                .trim_end_matches('/');
            anyhow::ensure!(
                !experiment_id.is_empty() && !experiment_id.contains('/'),
                "invalid experiment relate path"
            );
            let mut payload = body;
            payload["experimentId"] = json!(experiment_id);
            if payload.get("createdAt").is_none() && payload.get("created_at").is_none() {
                payload["createdAt"] = json!(chrono::Utc::now().to_rfc3339());
            }
            let request: crate::experiments::ExperimentRelateRequest =
                serde_json::from_value(payload)?;
            Ok(json!({
                "experiment": core.data().experiment_relate(request).await?
            }))
        }
        ("POST", path) if path.starts_with("/v1/experiments/") && path.ends_with("/activate") => {
            let experiment_id = path
                .trim_start_matches("/v1/experiments/")
                .trim_end_matches("/activate")
                .trim_end_matches('/');
            anyhow::ensure!(
                !experiment_id.is_empty() && !experiment_id.contains('/'),
                "invalid experiment activate path"
            );
            let session_id = json_field(&body, "sessionId", "session_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .context("experiments activate requires sessionId")?;
            Ok(json!({
                "experiment": core.data().experiment_activate(session_id.to_owned(), experiment_id.to_owned()).await?
            }))
        }
        ("GET", "/v1/experiments") | ("GET", "/v1/experiments/") => {
            let session_id = json_field(&body, "sessionId", "session_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if let Some(session_id) = session_id {
                let group = core
                    .data()
                    .experiment_for_session(session_id.to_owned())
                    .await?;
                Ok(json!({"sessionId": session_id, "experiment": group}))
            } else {
                let query = body.get("query").and_then(Value::as_str).map(str::to_owned);
                Ok(json!({"experiments": core.data().experiments_list(query).await?}))
            }
        }
        ("PATCH", path) if path.starts_with("/v1/experiments/") => {
            let experiment_id = path
                .trim_start_matches("/v1/experiments/")
                .trim_end_matches('/');
            anyhow::ensure!(
                !experiment_id.is_empty() && !experiment_id.contains('/'),
                "invalid experiment path"
            );
            let mut payload = body;
            payload["experimentId"] = json!(experiment_id);
            if payload.get("updatedAt").is_none() && payload.get("updated_at").is_none() {
                payload["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
            }
            let request: crate::experiments::ExperimentUpdateRequest =
                serde_json::from_value(payload)?;
            Ok(json!({"experiment": core.data().experiment_update(request).await?}))
        }
        ("GET", path) if path.starts_with("/v1/experiments/") => {
            let experiment_id = path
                .trim_start_matches("/v1/experiments/")
                .trim_end_matches('/');
            anyhow::ensure!(
                !experiment_id.is_empty() && !experiment_id.contains('/'),
                "invalid experiment path"
            );
            Ok(json!({
                "experiment": core.data().experiment_get(experiment_id.to_owned()).await?
            }))
        }
        ("POST", path) if path.starts_with("/v1/experiments/") && path.ends_with("/evidence") => {
            let experiment_id = path
                .trim_start_matches("/v1/experiments/")
                .trim_end_matches("/evidence")
                .trim_end_matches('/');
            anyhow::ensure!(
                !experiment_id.is_empty() && !experiment_id.contains('/'),
                "invalid experiment evidence path"
            );
            let mut payload = body;
            payload["experimentId"] = json!(experiment_id);
            if payload.get("attachedAt").is_none() && payload.get("attached_at").is_none() {
                payload["attachedAt"] = json!(chrono::Utc::now().to_rfc3339());
            }
            let request: crate::experiments::ExperimentEvidenceAttachRequest =
                serde_json::from_value(payload)?;
            Ok(json!({"experiment": core.data().experiment_attach_evidence(request).await?}))
        }
        ("POST", path) if path.starts_with("/v1/experiments/") && path.ends_with("/finalize") => {
            let experiment_id = path
                .trim_start_matches("/v1/experiments/")
                .trim_end_matches("/finalize")
                .trim_end_matches('/');
            let mut payload = body;
            payload["experimentId"] = json!(experiment_id);
            if payload.get("finalizedAt").is_none() {
                payload["finalizedAt"] = json!(chrono::Utc::now().to_rfc3339());
            }
            let request: crate::experiments::ExperimentFinalizeRequest =
                serde_json::from_value(payload)?;
            Ok(json!({"experiment": core.data().experiment_finalize(request).await?}))
        }
        _ => anyhow::bail!("unknown experiments route {method} {path}"),
    }
}

async fn dispatch_research_log(
    method: &str,
    path: &str,
    body: Value,
    core: &CoreRuntime,
) -> Result<Value> {
    match (method, path) {
        ("GET", "/v1/research-log") | ("GET", "/v1/research-log/") => {
            let query = body.get("query").and_then(Value::as_str).map(str::to_owned);
            let experiment_id = body
                .get("experimentId")
                .or_else(|| body.get("experiment_id"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            Ok(json!({"entries": core.data().research_log_list(query, experiment_id).await?}))
        }
        ("POST", "/v1/research-log") | ("POST", "/v1/research-log/") => {
            let request: crate::experiments::ResearchJournalAppendRequest =
                serde_json::from_value(body)?;
            Ok(json!({"entry": core.data().research_log_append(request).await?}))
        }
        _ => anyhow::bail!("unknown research log route {method} {path}"),
    }
}

async fn dispatch_analysis(
    method: &str,
    path: &str,
    body: Value,
    core: &CoreRuntime,
) -> Result<Value> {
    match (method, path) {
        ("POST", "/v1/analysis/projection") => {
            let kind = body
                .get("kind")
                .and_then(Value::as_str)
                .context("kind required")?;
            let digest = body
                .get("digest")
                .or_else(|| body.get("source"))
                .and_then(Value::as_str)
                .context("digest required")?;
            let kind = kind.to_string();
            let digest = digest.to_string();
            core.storage()
                .database()
                .run_read(move |conn| {
                    crate::session::annotation_projection::projection_payload(conn, &kind, &digest)
                })
                .await
        }
        ("POST", "/v1/analysis/open") => {
            let trace_id = body
                .get("trace_id")
                .or_else(|| body.get("traceId"))
                .and_then(Value::as_str)
                .context("trace_id required")?;
            let evidence_digest = body
                .get("evidence_digest")
                .or_else(|| body.get("evidenceDigest"))
                .and_then(Value::as_str)
                .context("evidence_digest required")?;
            let rubric_digest = body
                .get("rubric_digest")
                .or_else(|| body.get("rubricDigest"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let campaign_id = body
                .get("campaign_id")
                .or_else(|| body.get("campaignId"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let title = body.get("title").and_then(Value::as_str).map(str::to_owned);
            let session_id = body
                .get("sessionRef")
                .or_else(|| body.get("session_id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| std::env::var("SYNTH_SESSION_ID").ok());
            let trace = core.data().get_trace(trace_id.to_string()).await?;
            let visual = crate::presentation::ensure_annotation_workbench(
                core,
                crate::presentation::AnnotationWorkbenchRequest {
                    trace,
                    evidence_digest: evidence_digest.to_string(),
                    rubric_digest,
                    campaign_id,
                    title,
                    session_id: session_id.clone(),
                },
            )
            .await?;
            let (shown, event) = core.visuals().show(visual.id.clone(), session_id).await?;
            core.broadcast_committed(Some(serde_json::from_value(event.clone())?));
            Ok(json!({
                "opened": true,
                "visualId": shown.id,
                "templateId": shown.template_id,
                "revision": shown.current_revision,
                "visual": shown,
            }))
        }
        ("POST", "/v1/analysis/campaigns") => {
            let eval_run_id = body
                .get("eval_run_id")
                .or_else(|| body.get("evalRunId"))
                .or_else(|| body.get("runId"))
                .and_then(Value::as_str)
                .context("evalRunId required")?;
            let eval_run_id = eval_run_id.to_string();
            core.storage()
                .database()
                .run_read(move |conn| {
                    let campaigns = crate::session::annotation_projection::list_campaigns_for_eval(
                        conn,
                        &eval_run_id,
                    )?;
                    Ok(json!({ "campaigns": campaigns }))
                })
                .await
        }
        ("POST", "/v1/analysis/findings") => {
            let digest = body
                .get("trace_digest")
                .or_else(|| body.get("traceDigest"))
                .and_then(Value::as_str)
                .context("traceDigest required")?;
            let digest = digest.to_string();
            core.storage()
                .database()
                .run_read(move |conn| {
                    let findings = crate::session::annotation_projection::list_findings_for_trace(
                        conn, &digest,
                    )?;
                    Ok(json!({ "findings": findings }))
                })
                .await
        }
        ("POST", "/v1/analysis/review") => {
            let finding_id = body
                .get("finding_id")
                .or_else(|| body.get("findingId"))
                .and_then(Value::as_str)
                .context("findingId required")?
                .to_string();
            let decision = body
                .get("decision")
                .and_then(Value::as_str)
                .unwrap_or("flag")
                .to_string();
            let reviewer = body
                .get("reviewer")
                .and_then(Value::as_str)
                .unwrap_or("workshop")
                .to_string();
            let rationale = body
                .get("rationale")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let evidence_head = body
                .get("evidence_head_digest")
                .or_else(|| body.get("evidenceHeadDigest"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let review_id = core
                .storage()
                .database()
                .run_transaction({
                    let finding_id = finding_id.clone();
                    let evidence_head = evidence_head.clone();
                    let decision = decision.clone();
                    let reviewer = reviewer.clone();
                    let rationale = rationale.clone();
                    move |conn| {
                        crate::session::annotation_projection::record_local_review(
                            conn,
                            &finding_id,
                            &evidence_head,
                            &decision,
                            &reviewer,
                            &rationale,
                        )
                    }
                })
                .await?;
            Ok(json!({ "reviewId": review_id, "findingId": finding_id, "decision": decision }))
        }
        _ => anyhow::bail!("unsupported analysis IPC route {method} {path}"),
    }
}

async fn dispatch_traces(
    method: &str,
    path: &str,
    body: Value,
    core: &CoreRuntime,
) -> Result<Value> {
    let trace_id = || -> Result<String> {
        body.get("trace_id")
            .or_else(|| body.get("traceId"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .context("trace_id required")
    };
    match (method, path) {
        ("GET", "/v1/traces") | ("GET", "/v1/traces/") => {
            let traces = core.data().list_traces().await?;
            // Report why an unavailable trace is unavailable rather than
            // omitting it, so the agent can say so instead of guessing.
            let rows: Vec<Value> = traces
                .iter()
                .map(|trace| {
                    let inspectability = crate::presentation::trace_inspectability(trace);
                    json!({
                        "traceId": trace.id,
                        "digest": trace.digest,
                        "title": trace.title,
                        "source": trace.source,
                        "reward": trace.reward,
                        "createdAt": trace.created_at,
                        "inspectable": inspectability.eligible(),
                        "inspectability": inspectability.label(),
                    })
                })
                .collect();
            Ok(json!({ "traces": rows, "count": rows.len() }))
        }
        ("POST", "/v1/traces/get") => {
            let trace = core.data().get_trace(trace_id()?).await?;
            let inspectability = crate::presentation::trace_inspectability(&trace);
            Ok(json!({
                "trace": trace,
                "inspectable": inspectability.eligible(),
                "inspectability": inspectability.label(),
            }))
        }
        ("POST", "/v1/traces/query") => {
            let query = crate::trace_query::parse_query(body.get("query").unwrap_or(&Value::Null))?;
            let snapshot = core
                .data()
                .query_traces(query, chrono::Utc::now().to_rfc3339())
                .await?;
            Ok(serde_json::to_value(snapshot)?)
        }
        ("POST", "/v1/traces/snapshot") => {
            let snapshot_id = body
                .get("snapshot_id")
                .or_else(|| body.get("snapshotId"))
                .and_then(Value::as_str)
                .context("snapshot_id required")?;
            let snapshot = core.data().query_snapshot(snapshot_id.to_string()).await?;
            Ok(serde_json::to_value(snapshot)?)
        }
        ("POST", "/v1/traces/open_query") => {
            let snapshot_id = body
                .get("snapshot_id")
                .or_else(|| body.get("snapshotId"))
                .and_then(Value::as_str)
                .context("snapshot_id required")?;
            let visual = crate::presentation::ensure_query_catalog(core, snapshot_id).await?;
            let session_id = body
                .get("sessionRef")
                .or_else(|| body.get("session_id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| std::env::var("SYNTH_SESSION_ID").ok());
            let (shown, event) = core.visuals().show(visual.id.clone(), session_id).await?;
            core.broadcast_committed(Some(serde_json::from_value(event.clone())?));
            Ok(json!({
                "opened": true,
                "snapshotId": snapshot_id,
                "visualId": shown.id,
                "templateId": shown.template_id,
                "visual": shown,
            }))
        }
        ("POST", "/v1/traces/import") => {
            let container_id = body
                .get("container_id")
                .or_else(|| body.get("containerId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .context("trace import requires container_id")?;
            let rollout_id = body
                .get("rollout_id")
                .or_else(|| body.get("rolloutId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .context("trace import requires rollout_id")?;
            import_container_trace(core, container_id, rollout_id).await
        }
        ("POST", "/v1/traces/open") => {
            let id = trace_id()?;
            require_indexed_trace(core, &id).await?;
            let visual = crate::presentation::ensure_trace_inspector(core, &id).await?;
            let session_id = body
                .get("sessionRef")
                .or_else(|| body.get("session_id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| std::env::var("SYNTH_SESSION_ID").ok());
            let (shown, event) = core.visuals().show(visual.id.clone(), session_id).await?;
            core.broadcast_committed(Some(serde_json::from_value(event.clone())?));
            Ok(json!({
                "opened": true,
                "traceId": id,
                "digest": crate::presentation::trace_digest_binding(&shown),
                "visualId": shown.id,
                "templateId": shown.template_id,
                "visual": shown,
            }))
        }
        _ => anyhow::bail!("unsupported trace IPC route {method} {path}"),
    }
}

/// Computer Use over loopback IPC.
///
/// Three routes only. There is deliberately no install, enable, or remove here:
/// the lifecycle of the plugin that hands an agent control of the machine is
/// human-only, and the agent-facing adapter cannot reach what does not exist.
/// See `docs/COMPUTER_USE.md` §4.
async fn dispatch_computer_use(
    method: &str,
    path: &str,
    body: Value,
    core: &CoreRuntime,
    app: &AppHandle,
) -> Result<Value> {
    let session_id = body
        .get("sessionRef")
        .or_else(|| body.get("session_id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| std::env::var("SYNTH_SESSION_ID").ok());

    match (method, path) {
        ("GET", "/v1/computer-use/status") => {
            // Refreshing grants is best effort: a helper that is not installed
            // yet must still produce a status that says so, rather than an
            // error the agent cannot interpret.
            let _ = core.computer_use().refresh_grants().await;
            let status = core.computer_use().status().await;
            let apps = match session_id.as_deref() {
                Some(session) => core.computer_use().allowlisted_apps(session).await,
                None => Vec::new(),
            };
            Ok(json!({ "status": status, "allowedApps": apps }))
        }
        ("POST", "/v1/computer-use/perform") => {
            let session_id = session_id.ok_or_else(|| {
                anyhow::anyhow!("computer use requires an agent session for approval")
            })?;
            let action: crate::computer_use::vocabulary::Action =
                serde_json::from_value(body.clone())
                    .map_err(|error| anyhow::anyhow!("unusable action: {error}"))?;
            let broker = app
                .try_state::<Arc<crate::session::approval::ApprovalBroker>>()
                .ok_or_else(|| {
                    anyhow::anyhow!("the approval broker is unavailable; refusing to act")
                })?;
            // Opening the run lazily keeps the agent from having to call a
            // separate `begin`, which is a step it would forget and a state it
            // would then have to recover from.
            let store =
                crate::storage::content_store::ContentStore::new(core.storage().content_root());
            let _ = core.computer_use().begin(&session_id, store).await;
            core.computer_use()
                .perform(app, broker.inner(), &session_id, action)
                .await
        }
        ("POST", "/v1/computer-use/end") => {
            if let Some(session) = session_id.as_deref() {
                core.computer_use().end(session).await?;
            }
            Ok(json!({ "ended": true }))
        }
        _ => anyhow::bail!("unsupported computer-use IPC route {method} {path}"),
    }
}

pub(crate) async fn dispatch_container_restart(
    path: &str,
    body: Value,
    core: &CoreRuntime,
    app: &AppHandle,
) -> Result<Value> {
    let id = path
        .trim_start_matches("/v1/containers/")
        .trim_end_matches("/restart")
        .trim_end_matches('/');
    let session = body
        .get("sessionRef")
        .or_else(|| body.get("session_ref"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("sessionRef required"))?;
    // A restart is a maintenance operation, not an implicit cancellation.
    // Refuse before approval when any non-terminal optimizer run is bound to
    // this evaluator so the operator can stop or cancel that work explicitly.
    let active = core
        .optimizers()
        .list(crate::optimizers::models::OptimizerQuery {
            limit: Some(1_000),
            ..Default::default()
        })
        .await?
        .into_iter()
        .filter(|run| {
            !crate::optimizers::models::OptimizerRunStatus::str_is_terminal(&run.status)
                && (run
                    .execution_bindings
                    .iter()
                    .any(|binding| binding.kind == "container_http" && binding.id == id)
                    || run
                        .summary
                        .get("containerId")
                        .and_then(Value::as_str)
                        .is_some_and(|container_id| container_id == id))
        })
        .map(|run| run.id)
        .collect::<Vec<_>>();
    if !active.is_empty() {
        anyhow::bail!(
            "container_restart_blocked_active_optimizer_runs: `{id}` is used by {}; cancel or finish those runs before restarting",
            active.join(", ")
        );
    }
    // Validate and reconcile before the destructive approval. An invalid
    // declaration must not consume a click.
    let spec = crate::optimizers::container_lifecycle::reconcile_declaration(
        core.storage().database(),
        session,
        id,
    )?;
    let launcher = spec.command.join(" ");
    let declaration_digest =
        crate::optimizers::container_lifecycle::approval_declaration_digest(&spec)?;
    let effect = format!(
        "Stop the currently registered workload when Workshop owns it, then run `{launcher}` from {} (revision {}, digest {}).",
        spec.origin.source_root.display(),
        spec.origin.source_revision.as_deref().unwrap_or("unknown"),
        spec.origin.source_digest.as_deref().unwrap_or("none")
    );
    let broker = app
        .try_state::<Arc<crate::session::approval::ApprovalBroker>>()
        .ok_or_else(|| anyhow::anyhow!("approval broker unavailable"))?;
    let authorization = broker
        .authorize_host(
            app,
            Some(session),
            crate::session::approval::ApprovalKind::ContainerLifecycle {
                container_id: id.to_owned(),
                declaration_id: spec.id.clone(),
                declaration_digest: declaration_digest.clone(),
                manifest_path: spec.origin.manifest_path.display().to_string(),
                source_root: spec.origin.source_root.display().to_string(),
                source_revision: spec.origin.source_revision.clone(),
                source_digest: spec.origin.source_digest.clone(),
                action: "force_replace".into(),
                effect,
            },
        )
        .await
        .map_err(|error| {
            let _ = core.storage().database().transaction(|conn| {
                if let Some(open) =
                    crate::platform::failure::repository::FailureRepository::open_for_container(
                        conn, id,
                    )?
                {
                    crate::platform::failure::FailureAuthority::transition(
                        conn,
                        open.failure_id.as_str(),
                        crate::platform::failure::FailureLifecycleState::Terminalized,
                        crate::platform::failure::TransitionReason::ApprovalDenied,
                        "operator",
                    )?;
                }
                Ok(())
            });
            error
        });
    let mut continuation =
        crate::optimizers::container_lifecycle::ContainerReplacementContinuation::new(
            authorization,
            declaration_digest,
        );
    let outcome = continuation
        .consume(core.storage().database(), session, id, &spec)
        .await?;
    let approval_id = outcome.approval_id;
    let approved = outcome.declaration;
    let stopped = outcome.stopped;
    let ensured = outcome.ensured;
    let _ = core.storage().database().transaction(|conn| {
        if let Some(open) =
            crate::platform::failure::repository::FailureRepository::open_for_container(conn, id)?
        {
            let plan = crate::platform::failure::RecoveryPlan::restart_container(
                open.failure_id.clone(),
                id.to_owned(),
                spec.id.clone(),
            );
            crate::platform::failure::recovery::insert_plan(conn, &plan)?;
            crate::platform::failure::recovery::insert_receipt(
                conn,
                &crate::platform::failure::RecoveryReceipt {
                    recovery_id: plan.recovery_id.clone(),
                    failure_id: open.failure_id.clone(),
                    status: "completed".into(),
                    approval_id: Some(approval_id.clone()),
                    completed_at: chrono::Utc::now(),
                    detail: serde_json::json!({"containerId": ensured.container_id}),
                },
            )?;
            crate::platform::failure::FailureAuthority::transition(
                conn,
                open.failure_id.as_str(),
                crate::platform::failure::FailureLifecycleState::Resolved,
                crate::platform::failure::TransitionReason::Resolved,
                "container_restart",
            )?;
            crate::domains::containers::clear_current(conn, id)?;
        }
        Ok(())
    });
    Ok(json!({
        "containerId": ensured.container_id,
        "baseUrl": ensured.base_url,
        "specId": ensured.spec_id,
        "locality": ensured.locality.as_str(),
        "replacedPid": stopped.as_ref().map(|value| value.pid),
        "replacementMode": "declared-command-force",
        "approvalId": approval_id,
        "declarationOrigin": approved.origin.to_json(),
        "status": "ready",
    }))
}

async fn dispatch_plugins(
    method: &str,
    path: &str,
    body: Value,
    core: &CoreRuntime,
    app: &AppHandle,
) -> Result<Value> {
    let session_owned = body
        .get("sessionRef")
        .or_else(|| body.get("session_id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| std::env::var("SYNTH_SESSION_ID").ok());
    let session_id = session_owned.as_deref();
    let plugin_id = body
        .get("plugin_id")
        .or_else(|| body.get("pluginId"))
        .cloned()
        .unwrap_or_else(|| json!("optimizers"));
    let version = body.get("version").cloned();
    let arguments = json!({
        "plugin_id": plugin_id,
        "version": version,
    });
    let mapped = match (method, path) {
        ("GET", "/v1/plugins") | ("GET", "/v1/plugins/") => "list",
        ("GET", "/v1/plugins/optimizers") => "status",
        ("GET", "/v1/plugins/optimizers/capabilities") => "capabilities",
        ("POST", "/v1/plugins/optimizers/enable") => "enable",
        ("POST", "/v1/plugins/optimizers/disable") => "disable",
        ("POST", "/v1/plugins/optimizers/install") => "install",
        ("POST", "/v1/plugins/optimizers/start") => "start",
        ("POST", "/v1/plugins/optimizers/stop") => "stop",
        ("POST", "/v1/plugins/optimizers/update") => "update",
        ("POST", "/v1/plugins/optimizers/remove") => "remove",
        ("POST", "/v1/plugins/manage") => body
            .get("operation")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("operation required"))?,
        _ => anyhow::bail!("unsupported plugin IPC route {method} {path}"),
    };
    let arguments = if path == "/v1/plugins/manage" {
        body.get("arguments").cloned().unwrap_or(arguments)
    } else {
        arguments
    };
    let broker = app.try_state::<Arc<crate::session::approval::ApprovalBroker>>();
    match broker {
        Some(broker) => {
            core.plugins()
                .manage(core, broker.inner(), app, session_id, mapped, &arguments)
                .await
        }
        None => {
            if matches!(mapped, "list" | "status" | "capabilities") {
                core.plugins()
                    .manage(
                        core,
                        &crate::session::approval::ApprovalBroker::new(
                            crate::session::SessionPersistence::Null,
                        ),
                        app,
                        session_id,
                        mapped,
                        &arguments,
                    )
                    .await
            } else {
                anyhow::bail!("plugin approval broker is unavailable")
            }
        }
    }
}

pub fn local_addr(url: &str) -> Result<SocketAddr> {
    let trimmed = url.trim_start_matches("http://");
    trimmed.parse().context("parse visuals IPC addr")
}

#[cfg(test)]
mod diagnostics_tests {
    use super::*;
    use crate::diagnostics::DiagnosticQuery;
    use tempfile::tempdir;

    fn capability_rejection() -> JsonHttpResponse {
        JsonHttpResponse::with_status(
            StatusCode::CONFLICT,
            json!({
                "code": "capability_mismatch",
                "container_id": "ctr_9",
                "missingOperations": ["rollouts/start"],
                "remediation": "Re-probe the container, then start only against a declared capability set.",
                "retryable": false
            }),
        )
    }

    #[tokio::test]
    async fn a_capability_rejection_is_queryable_by_its_own_stable_code() {
        let dir = tempdir().unwrap();
        let core = CoreRuntime::open(dir.path()).unwrap();
        let body = json!({
            "containerId": "ctr_9",
            "rolloutId": "roll_3",
            "visualId": "vis_9"
        });

        record_ipc_failure(
            &core,
            &redacted_operation("/v1/containers/ctr_deadbeefcafe/rollouts/start"),
            &capability_rejection(),
            correlation_from_ipc_body(&body),
            std::time::Duration::from_millis(42),
        );

        let result = core
            .diagnostics_service()
            .query(DiagnosticQuery {
                codes: vec!["capability_mismatch".into()],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(result["count"], json!(1));
        let event = &result["events"][0];
        assert_eq!(event["component"], json!("mcp"));
        assert_eq!(event["container_id"], json!("ctr_9"));
        assert_eq!(event["rollout_id"], json!("roll_3"));
        assert_eq!(event["visual_id"], json!("vis_9"));
        assert_eq!(
            event["details"]["missing_operations"],
            json!(["rollouts/start"])
        );
        assert!(event["details"]["remediation"]
            .as_str()
            .unwrap()
            .contains("Re-probe"));
        assert_eq!(event["details"]["status"], json!(409));
        assert_eq!(event["details"]["duration_ms"], json!(42));
        // The identity is in the body, not the label: one operation label per
        // route, never one per container.
        assert_eq!(
            event["details"]["operation"],
            json!("/v1/containers/{id}/rollouts/start")
        );
    }

    #[tokio::test]
    async fn a_server_failure_is_an_error_and_a_refusal_is_a_warning() {
        let dir = tempdir().unwrap();
        let core = CoreRuntime::open(dir.path()).unwrap();
        record_ipc_failure(
            &core,
            "/v1/visuals/{id}/render",
            &JsonHttpResponse::error(StatusCode::INTERNAL_SERVER_ERROR, "renderer exploded"),
            crate::diagnostics::Correlation::default(),
            std::time::Duration::from_millis(1),
        );
        record_ipc_failure(
            &core,
            "/v1/visuals",
            &JsonHttpResponse::error(StatusCode::BAD_REQUEST, "templateId is required"),
            crate::diagnostics::Correlation::default(),
            std::time::Duration::from_millis(1),
        );

        let result = core
            .diagnostics_service()
            .query(DiagnosticQuery::default())
            .await
            .unwrap();
        assert_eq!(result["count"], json!(2));
        let by_code: std::collections::HashMap<&str, &serde_json::Value> = result["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| (event["message"].as_str().unwrap(), event))
            .collect();
        assert_eq!(by_code["renderer exploded"]["severity"], json!("error"));
        assert_eq!(by_code["renderer exploded"]["retryable"], json!(true));
        assert_eq!(by_code["templateId is required"]["severity"], json!("warn"));
        assert_eq!(by_code["templateId is required"]["retryable"], json!(false));
    }

    #[tokio::test]
    async fn a_successful_call_records_nothing() {
        let dir = tempdir().unwrap();
        let core = CoreRuntime::open(dir.path()).unwrap();
        let response = JsonHttpResponse::ok(json!({"ok": true}));
        assert!(response.status.is_success());
        let result = core
            .diagnostics_service()
            .query(DiagnosticQuery::default())
            .await
            .unwrap();
        assert_eq!(result["count"], json!(0));
    }

    #[test]
    fn operation_labels_never_carry_an_identity() {
        assert_eq!(
            redacted_operation("/v1/containers/ctr_0f9b2c4d8e/rollouts/poll?x=1"),
            "/v1/containers/{id}/rollouts/poll"
        );
        assert_eq!(
            redacted_operation("/v1/optimizers/opt_run_98f3aa12bc/events"),
            "/v1/optimizers/{id}/events"
        );
        // Short, stable segments stay legible.
        assert_eq!(redacted_operation("/v1/visuals"), "/v1/visuals");
        assert_eq!(
            redacted_operation("/v1/diagnostics/query"),
            "/v1/diagnostics/query"
        );
    }

    #[test]
    fn correlation_is_read_in_both_casings() {
        let camel = correlation_from_ipc_body(&json!({"rolloutId": "roll_1", "visualId": "vis_1"}));
        let snake =
            correlation_from_ipc_body(&json!({"rollout_id": "roll_1", "visual_id": "vis_1"}));
        assert_eq!(camel, snake);
        assert_eq!(camel.rollout_id.as_deref(), Some("roll_1"));
        assert!(correlation_from_ipc_body(&json!("not an object")).is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_scope_names_its_subject_or_refuses() {
        assert_eq!(CaptureScope::parse("app", None).unwrap(), CaptureScope::App);
        // A target on an app capture is meaningless, not fatal: there is
        // nothing to point it at, and refusing would only make the tool fussy.
        assert_eq!(
            CaptureScope::parse("app", Some("visuals")).unwrap(),
            CaptureScope::App
        );
        assert_eq!(
            CaptureScope::parse("plugin", Some("visuals")).unwrap(),
            CaptureScope::Plugin("visuals".into())
        );
        assert_eq!(
            CaptureScope::parse("visual", Some("vis_1")).unwrap(),
            CaptureScope::Visual("vis_1".into())
        );
        assert_eq!(
            CaptureScope::parse("element", Some("visuals-preview")).unwrap(),
            CaptureScope::Element("visuals-preview".into())
        );

        // A scope that names a destination must actually name one. Whitespace
        // is not a target: it would route nowhere and photograph whatever the
        // window happened to be showing.
        for (scope, target) in [
            ("plugin", None),
            ("plugin", Some("   ")),
            ("visual", None),
            ("element", None),
        ] {
            assert!(
                CaptureScope::parse(scope, target).is_err(),
                "{scope} {target:?}"
            );
        }
        // Only destinations the display contract admits.
        assert!(CaptureScope::parse("plugin", Some("settings")).is_err());
        assert!(CaptureScope::parse("window", None).is_err());
    }

    #[test]
    fn only_a_visual_capture_routes_away_and_resets_scroll() {
        // Scroll position is app state; a capture that silently scrolled the
        // user's window to the top would be photographing a different app than
        // the one it was asked about. Only a review, whose subject is the
        // visual's own surface, resets it.
        assert!(CaptureScope::Visual("vis_1".into()).resets_scroll());
        assert!(!CaptureScope::App.resets_scroll());
        assert!(!CaptureScope::Plugin("visuals".into()).resets_scroll());
        assert!(!CaptureScope::Element("x".into()).resets_scroll());

        // `element` crops what is already on screen, so it must not navigate.
        assert!(CaptureScope::Plugin("visuals".into()).routes());
        assert!(CaptureScope::Visual("vis_1".into()).routes());
        assert!(!CaptureScope::App.routes());
        assert!(!CaptureScope::Element("x".into()).routes());
    }

    #[cfg(target_os = "macos")]
    fn solid_png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            // Encode the x coordinate into red and y into green so a crop can
            // be checked for position, not just for size.
            let mut pixels = Vec::with_capacity((width * height * 4) as usize);
            for y in 0..height {
                for x in 0..width {
                    pixels.extend_from_slice(&[x as u8, y as u8, 0, 255]);
                }
            }
            writer.write_image_data(&pixels).unwrap();
        }
        out
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn crop_applies_the_display_scale_factor() {
        // The rect arrives in CSS pixels and the snapshot is in device pixels.
        // Without the scale factor a 2x display crops the top-left quadrant of
        // the intended region and calls it the element.
        let image = solid_png(200, 200);
        let rect = CaptureRect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };

        let at_1x = crop_png(&image, rect, 1.0).unwrap();
        assert_eq!(png_dimensions(&at_1x), Some((30, 40)));

        let at_2x = crop_png(&image, rect, 2.0).unwrap();
        assert_eq!(png_dimensions(&at_2x), Some((60, 80)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn crop_rounds_outward_so_a_border_is_not_clipped() {
        // A half-pixel box that rounded inward would shave the element's own
        // border — exactly the evidence a layout review is looking at.
        let image = solid_png(100, 100);
        let rect = CaptureRect {
            x: 10.5,
            y: 10.5,
            width: 20.2,
            height: 20.2,
        };
        let cropped = crop_png(&image, rect, 1.0).unwrap();
        assert_eq!(png_dimensions(&cropped), Some((21, 21)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn crop_clamps_to_the_image_and_refuses_an_offscreen_element() {
        let image = solid_png(100, 100);

        // Partly offscreen: keep the visible part rather than failing.
        let overhang = CaptureRect {
            x: 80.0,
            y: 80.0,
            width: 40.0,
            height: 40.0,
        };
        assert_eq!(
            png_dimensions(&crop_png(&image, overhang, 1.0).unwrap()),
            Some((20, 20))
        );

        // Entirely offscreen has nothing to show, and a zero-pixel PNG would be
        // a worse answer than an error that says the element is not visible.
        let gone = CaptureRect {
            x: 400.0,
            y: 400.0,
            width: 10.0,
            height: 10.0,
        };
        let error = crop_png(&image, gone, 1.0).unwrap_err().to_string();
        assert!(error.contains("not visible"), "{error}");
    }

    #[test]
    fn agent_secret_workload_selects_only_fixed_wire_operations() {
        let chat = agent_use_policy(None).unwrap();
        assert_eq!(chat.operations, vec!["chat.completions.create"]);

        let codex = agent_use_policy(Some("codex_responses")).unwrap();
        assert_eq!(codex.operations, vec!["responses.create"]);
        assert_eq!(codex.max_calls, chat.max_calls);
        assert_eq!(codex.max_cost_usd, chat.max_cost_usd);

        assert!(agent_use_policy(Some("arbitrary.operations")).is_err());
    }

    #[test]
    fn preserves_explicit_runtime_family_for_gepa_v2_selection() {
        let info = json!({
            "runtime_family": "healthbench",
            "metadata": {"optimizer": {"contract": "synth_optimizers.gepa.v2"}},
            "capabilities": {"operations": {"prepare": true, "start": true}}
        });
        assert_eq!(
            observed_task_family(Some(&info), None, None).as_deref(),
            Some("healthbench")
        );
    }

    #[test]
    fn parses_query_values() {
        assert_eq!(
            query_json("search=reward+chart&limit=5&offset=2"),
            json!({
                "search": "reward chart", "limit": 5, "offset": 2
            })
        );
    }

    #[test]
    fn capture_supervisor_bundle_route_is_tried_before_lite_seal() {
        let announced = json!({
            "bundle_url": "/rollouts/roll_e_bundle/trace/bundle",
            "url": "/rollouts/roll_e_bundle/trace",
            "kind": "trace_v5_bundle",
            "inspectable": true
        });
        assert_eq!(
            sealed_trace_bundle_routes("roll_e_bundle", Some(&announced)),
            vec!["/rollouts/roll_e_bundle/trace/bundle".to_string()]
        );
        assert_eq!(
            sealed_trace_bundle_routes("roll_missing", None),
            vec!["/rollouts/roll_missing/trace/bundle".to_string()]
        );
        assert_eq!(
            sealed_trace_lite_routes("roll_e_bundle", Some(&announced)),
            vec!["/rollouts/roll_e_bundle/trace".to_string()]
        );
    }

    #[test]
    fn lite_seal_import_receipt_is_not_inspectable() {
        let receipt = json!({
            "sourceKind": "container_seal",
            "inspectable": false,
            "traces": [],
            "note": "Imported as a provenance record only: this container returned a lite seal, not a self-contained Trace V5 bundle, so it cannot be projected into the inspector."
        });
        assert_eq!(receipt["inspectable"], false);
        assert!(receipt["traces"].as_array().unwrap().is_empty());
    }

    fn live_contract() -> TemplateObservationContract {
        TemplateObservationContract {
            schema_version: "synth.visual-observation-contract.v1".into(),
            readiness: crate::visuals::TemplateReadinessContract {
                reject_transport_states: vec![
                    "connecting".into(),
                    "reconnecting".into(),
                    "error".into(),
                ],
                minimum_rollout_count: 1,
                minimum_rendered_frame_count: 1,
                minimum_semantic_event_count: 1,
                require_terminal: true,
            },
        }
    }

    fn rendered_observation() -> RenderedVisualObservation {
        RenderedVisualObservation {
            schema_version: "synth.rendered-visual-observation.v1".into(),
            visual_id: "vis_1".into(),
            rendered_revision: 14,
            bindings_digest: "bindings-14".into(),
            transport_state: "terminal".into(),
            rollout_count: 10,
            rendered_frame_count: 21,
            semantic_event_count: 26,
            terminal: true,
            error: None,
            observed_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn review(width: u64, passing: bool, observation: Option<&RenderedVisualObservation>) -> Value {
        let checks: serde_json::Map<String, Value> = BASE_AUTHORING_CHECKS
            .iter()
            .chain(["screenshotInspected", "imageReplay"].iter())
            .map(|check| ((*check).to_string(), json!(passing)))
            .collect();
        json!({
            "revision": 14,
            "viewport": {"width": width, "height": 900},
            "checks": Value::Object(checks),
            "findings": [],
            "screenshotPath": format!("/tmp/vis_1-r14-{width}x900.png"),
            "captureTime": "2026-08-17T01:00:00Z",
            "observations": observation.map(|value| serde_json::to_value(value).unwrap()),
            "reviewedAt": "2026-08-17T01:00:01Z",
        })
    }

    /// Seeds 202/204: honest pre-start reviews (no frames yet) kept vetoing
    /// readiness after terminal evidence arrived on the same revision, and the
    /// only workaround was a cosmetic revision bump.
    #[test]
    fn certification_uses_the_latest_review_at_each_width_not_all_history() {
        let contract = live_contract();
        let mut pre_start = rendered_observation();
        pre_start.transport_state = "connecting".into();
        pre_start.rendered_frame_count = 0;
        pre_start.terminal = false;
        let terminal = rendered_observation();
        let required = required_authoring_checks(
            &crate::visuals::resolve_template("live.craftax.v1").unwrap(),
        );

        let failed_wide = review(1280, false, Some(&pre_start));
        let failed_compact = review(640, false, Some(&pre_start));
        let passed_wide = review(1280, true, Some(&terminal));
        let passed_compact = review(640, true, Some(&terminal));
        let history = vec![&failed_wide, &failed_compact, &passed_wide, &passed_compact];

        let receipts = certification_receipts(
            "vis_1",
            14,
            &history,
            &required,
            Some(&contract),
            Some("bindings-14"),
        )
        .expect("terminal evidence must certify over a superseded pre-start failure");
        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[0]["viewportWidth"], 640);
        assert_eq!(receipts[1]["viewportWidth"], 1280);
        // The receipt binds the pass to the evidence it was taken from.
        assert_eq!(receipts[0]["bindingsDigest"], "bindings-14");
        assert_eq!(receipts[0]["transportState"], "terminal");
        assert_eq!(receipts[0]["renderedFrameCount"], 21);
    }

    /// The inverse: a later failure at one width is not certified around by an
    /// earlier pass at that same width.
    #[test]
    fn certification_rejects_a_width_whose_latest_review_regressed() {
        let contract = live_contract();
        let terminal = rendered_observation();
        let required = required_authoring_checks(
            &crate::visuals::resolve_template("live.craftax.v1").unwrap(),
        );
        let passed_wide = review(1280, true, Some(&terminal));
        let passed_compact = review(640, true, Some(&terminal));
        let failed_compact = review(640, false, Some(&terminal));
        let history = vec![&passed_wide, &passed_compact, &failed_compact];
        let error = certification_receipts(
            "vis_1",
            14,
            &history,
            &required,
            Some(&contract),
            Some("bindings-14"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("width 640"), "{error}");
    }

    /// The Trace inspector renders no image frames. Its contract must still be
    /// certifiable from a terminal projection, which is what makes the template
    /// reviewable at all.
    #[test]
    fn certification_admits_a_frameless_terminal_projection() {
        let template = crate::visuals::resolve_template("trace.rollout_inspector.v1").unwrap();
        let contract = template
            .observation_contract
            .clone()
            .expect("the trace inspector must declare an observation contract");
        assert!(!required_authoring_checks(&template).contains(&"imageReplay"));
        let mut observation = rendered_observation();
        observation.rendered_frame_count = 0;
        observation.rollout_count = 1;
        let required = required_authoring_checks(&template);
        let wide = review(1280, true, Some(&observation));
        let compact = review(640, true, Some(&observation));
        let receipts = certification_receipts(
            "vis_1",
            14,
            &[&wide, &compact],
            &required,
            Some(&contract),
            Some("bindings-14"),
        )
        .expect("a sealed trace projection is terminal evidence");
        assert_eq!(receipts.len(), 2);
    }

    #[test]
    fn certification_requires_two_distinct_widths() {
        let required = required_authoring_checks(
            &crate::visuals::resolve_template("live.craftax.v1").unwrap(),
        );
        let first = review(1280, true, None);
        let second = review(1280, true, None);
        assert!(
            certification_receipts("vis_1", 14, &[&first, &second], &required, None, None)
                .unwrap_err()
                .to_string()
                .contains("two distinct viewport widths")
        );
    }

    #[test]
    fn observation_contract_drives_image_replay_without_template_id_logic() {
        let mut template = crate::visuals::resolve_template("live.craftax.v1").unwrap();
        assert!(required_authoring_checks(&template).contains(&"imageReplay"));
        template.id = "live.future_eval.v1".into();
        assert!(required_authoring_checks(&template).contains(&"imageReplay"));
        template.observation_contract = None;
        assert!(!required_authoring_checks(&template).contains(&"imageReplay"));
    }

    #[test]
    fn readiness_accepts_matching_mechanically_harvested_evidence() {
        validate_readiness_observation(
            &live_contract(),
            "vis_1",
            14,
            "bindings-14",
            &rendered_observation(),
        )
        .unwrap();
    }

    #[test]
    fn readiness_rejects_connecting_and_zero_evidence() {
        let mut observation = rendered_observation();
        observation.transport_state = "connecting".into();
        assert!(validate_readiness_observation(
            &live_contract(),
            "vis_1",
            14,
            "bindings-14",
            &observation
        )
        .unwrap_err()
        .to_string()
        .contains("transport state"));
        observation.transport_state = "terminal".into();
        observation.rendered_frame_count = 0;
        assert!(validate_readiness_observation(
            &live_contract(),
            "vis_1",
            14,
            "bindings-14",
            &observation
        )
        .unwrap_err()
        .to_string()
        .contains("frame evidence"));
    }

    /// A state machine gains states. Readiness must not silently accept one
    /// because no template contract listed it — that is how a pane that is not
    /// showing evidence gets marked ready.
    #[test]
    fn readiness_rejects_every_transport_state_that_is_not_settled_evidence() {
        for state in [
            "idle",
            "declared",
            "replaying",
            "connecting",
            "reconnecting",
            "error",
            "surprise",
        ] {
            let mut observation = rendered_observation();
            observation.transport_state = state.into();
            let error = validate_readiness_observation(
                &live_contract(),
                "vis_1",
                14,
                "bindings-14",
                &observation,
            )
            .unwrap_err()
            .to_string();
            assert!(
                error.contains("transport state"),
                "{state} must be rejected: {error}"
            );
        }
        for state in ["live", "terminal"] {
            let mut observation = rendered_observation();
            observation.transport_state = state.into();
            validate_readiness_observation(
                &live_contract(),
                "vis_1",
                14,
                "bindings-14",
                &observation,
            )
            .unwrap_or_else(|error| panic!("{state} must be able to carry evidence: {error}"));
        }
    }

    #[test]
    fn readiness_rejects_stale_revision_and_mismatched_bindings() {
        let observation = rendered_observation();
        assert!(validate_readiness_observation(
            &live_contract(),
            "vis_1",
            15,
            "bindings-14",
            &observation
        )
        .is_err());
        assert!(validate_readiness_observation(
            &live_contract(),
            "vis_1",
            14,
            "bindings-15",
            &observation
        )
        .unwrap_err()
        .to_string()
        .contains("bindings"));
    }

    #[test]
    fn rollout_base_is_strictly_loopback_http() {
        assert_eq!(
            validated_loopback_rollout_base("http://127.0.0.1:8098/").unwrap(),
            "http://127.0.0.1:8098"
        );
        assert!(validated_loopback_rollout_base("https://127.0.0.1:8098").is_err());
        assert!(validated_loopback_rollout_base("http://example.com:8098").is_err());
        assert!(validated_loopback_rollout_base("file:///tmp/craftax").is_err());
    }

    #[test]
    fn reconnect_classifies_only_authoritative_started_states() {
        assert!(!rollout_started(&json!({
            "status": "prepared", "started": false, "terminated": false
        })));
        assert!(rollout_started(&json!({
            "status": "running", "started": true, "terminated": false
        })));
        assert!(rollout_started(&json!({
            "status": "failed", "started": true, "terminated": true
        })));
    }

    #[tokio::test]
    async fn ambiguous_start_disconnect_recovers_authoritative_state_without_replay() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 4096];
            let read = first.read(&mut request).await.unwrap();
            assert!(String::from_utf8_lossy(&request[..read]).starts_with("POST /rollouts "));
            drop(first); // The mutation landed, but the response transport was lost.

            let (mut status, _) = listener.accept().await.unwrap();
            let read = status.read(&mut request).await.unwrap();
            assert!(String::from_utf8_lossy(&request[..read]).starts_with("GET /rollouts/r1 "));
            let body =
                r#"{"rollout_id":"r1","status":"running","started":true,"terminated":false}"#;
            status
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let client = test_client();
        let (state, recovered) = start_rollout_idempotently(
            &client,
            &client,
            &format!("http://{addr}"),
            "r1",
            &json!({"rollout_id":"r1"}),
        )
        .await
        .unwrap();
        assert!(recovered);
        assert_eq!(state["status"], "running");
        task.await.unwrap();
    }

    #[test]
    fn scripted_rollouts_bind_slot_stream() {
        assert!(require_scripted_stream_slot(&json!({})).is_ok());
        assert!(require_scripted_stream_slot(&json!({"slot": "stream"})).is_ok());
        assert!(require_scripted_stream_slot(&json!({"input": "stream"})).is_ok());
        assert!(require_scripted_stream_slot(&json!({"slot": "live"})).is_err());
        assert!(require_scripted_stream_slot(&json!({"input": "jobs"})).is_err());
    }

    #[test]
    fn public_scripted_rollout_batch_supports_exactly_ten() {
        assert_eq!(MAX_SCRIPTED_ROLLOUTS, 10);
        assert!((1..=MAX_SCRIPTED_ROLLOUTS).contains(&10));
        assert!(!(1..=MAX_SCRIPTED_ROLLOUTS).contains(&11));
    }

    fn route_path(path: &str) -> &str {
        path.split('?').next().unwrap_or(path)
    }

    async fn spawn_mock(
        handler: impl Fn(JsonHttpRequest) -> JsonHttpResponse + Clone + Send + Sync + 'static,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _ = serve_json(listener, move |request| {
                let handler = handler.clone();
                async move { handler(request) }
            })
            .await;
        });
        (format!("http://{addr}"), task)
    }

    fn test_client() -> reqwest::Client {
        crate::http::http_client_builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn containers_facade_waits_for_subscribed_then_polls_declared_url() {
        let hits = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let subscribed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hits_h = hits.clone();
        let started_h = started.clone();
        let subscribed_h = subscribed.clone();
        let (base, task) = spawn_mock(move |request| {
            let method = request.method.as_str().to_string();
            let path = route_path(&request.path).to_string();
            hits_h.lock().unwrap().push(format!("{method} {path}"));
            match (method.as_str(), path.as_str()) {
                ("POST", "/rollouts/prepare") => JsonHttpResponse::ok(json!({
                    "rollout_id": "r1",
                    "stream": {
                        "id": "stream:r1",
                        "transports": { "poll": { "url": "/rollouts/r1/events" } }
                    }
                })),
                ("GET", "/rollouts/r1/events") => {
                    subscribed_h.store(true, std::sync::atomic::Ordering::SeqCst);
                    JsonHttpResponse::ok(json!({
                        "events": [{
                            "kind": "stream.subscribed",
                            "ready": true,
                            "payload": { "ready": true, "rollout_id": "r1" }
                        }]
                    }))
                }
                ("POST", "/rollouts") => {
                    assert!(
                        subscribed_h.load(std::sync::atomic::Ordering::SeqCst),
                        "C1-08: POST /rollouts before stream.subscribed"
                    );
                    started_h.store(true, std::sync::atomic::Ordering::SeqCst);
                    assert_eq!(
                        request.body.get("rollout_id").and_then(Value::as_str),
                        Some("r1")
                    );
                    assert_eq!(
                        request.body.get("slot").and_then(Value::as_str),
                        Some("stream")
                    );
                    JsonHttpResponse::ok(json!({
                        "rollout_id": "r1",
                        "terminated": false,
                        "truncated": false
                    }))
                }
                ("POST", "/rollouts/r1/step") => JsonHttpResponse::ok(json!({
                    "rollout_id": "r1",
                    "terminated": true,
                    "truncated": false
                })),
                ("GET", "/rollouts/r1/event_log") => JsonHttpResponse::error(
                    StatusCode::NOT_FOUND,
                    "gold event_log must not be guessed",
                ),
                _ => JsonHttpResponse::error(
                    StatusCode::NOT_FOUND,
                    format!("unexpected {method} {path}"),
                ),
            }
        })
        .await;
        let client = test_client();
        let outcome = run_one_scripted_rollout(
            &client,
            &base,
            1,
            &["do".into()],
            Some("r1".into()),
            &crate::container_stream::StreamDiagnostics::none(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.rollout_id, "r1");
        assert_eq!(outcome.stream_id.as_deref(), Some("stream:r1"));
        assert!(started.load(std::sync::atomic::Ordering::SeqCst));
        let recorded = hits.lock().unwrap().clone();
        assert!(recorded.iter().any(|hit| hit == "POST /rollouts/prepare"));
        let prepare_at = recorded
            .iter()
            .position(|hit| hit == "POST /rollouts/prepare")
            .unwrap();
        let start_at = recorded
            .iter()
            .position(|hit| hit == "POST /rollouts")
            .unwrap();
        let poll_at = recorded
            .iter()
            .position(|hit| hit == "GET /rollouts/r1/events")
            .unwrap();
        assert!(prepare_at < poll_at);
        assert!(poll_at < start_at);
        assert!(!recorded.iter().any(|hit| hit.contains("/event_log")));
        task.abort();
    }

    #[tokio::test]
    async fn native_benchmark_routes_are_refused_outside_containers_fold() {
        let hits = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let hits_h = hits.clone();
        let (base, task) = spawn_mock(move |request| {
            let method = request.method.as_str().to_string();
            let path = route_path(&request.path).to_string();
            hits_h.lock().unwrap().push(format!("{method} {path}"));
            match (method.as_str(), path.as_str()) {
                ("POST", "/rollouts/prepare") => {
                    JsonHttpResponse::error(StatusCode::NOT_FOUND, "no prepare")
                }
                _ => JsonHttpResponse::error(
                    StatusCode::NOT_FOUND,
                    format!("unexpected {method} {path}"),
                ),
            }
        })
        .await;
        let client = test_client();
        let error = run_one_scripted_rollout(
            &client,
            &base,
            7,
            &["do".into()],
            Some("r1".into()),
            &crate::container_stream::StreamDiagnostics::none(),
        )
        .await
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("must be folded inside Containers"));
        let recorded = hits.lock().unwrap().clone();
        assert!(recorded.iter().any(|hit| hit == "POST /rollouts/prepare"));
        assert_eq!(recorded.len(), 1);
        task.abort();
    }

    #[tokio::test]
    async fn containers_facade_refuses_start_without_subscribed() {
        let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started_h = started.clone();
        let (base, task) = spawn_mock(move |request| {
            let method = request.method.as_str();
            let path = route_path(&request.path);
            match (method, path) {
                ("POST", "/rollouts/prepare") => JsonHttpResponse::ok(json!({
                    "rollout_id": "r1",
                    "stream": {
                        "id": "stream:r1",
                        "transports": { "poll": { "url": "/rollouts/r1/events" } }
                    }
                })),
                ("GET", "/rollouts/r1/events") => JsonHttpResponse::ok(json!({
                    "events": [{ "kind": "heartbeat" }]
                })),
                ("POST", "/rollouts") => {
                    started_h.store(true, std::sync::atomic::Ordering::SeqCst);
                    JsonHttpResponse::ok(json!({"rollout_id": "r1"}))
                }
                _ => JsonHttpResponse::error(StatusCode::NOT_FOUND, "no"),
            }
        })
        .await;
        let client = test_client();
        let err = run_one_scripted_rollout(
            &client,
            &base,
            1,
            &["do".into()],
            Some("r1".into()),
            &crate::container_stream::StreamDiagnostics::none(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("stream.subscribed"));
        assert!(!started.load(std::sync::atomic::Ordering::SeqCst));
        task.abort();
    }

    #[test]
    fn harbor_register_metadata_is_visual_first() {
        let harbor =
            live_eval_bind_metadata(crate::visuals::LiveEvalFamily::Harbor, &json!({}), None)
                .unwrap();
        assert_eq!(harbor["templateId"], "live.harbor_eval.v1");
        assert_eq!(harbor["input"], "stream");
        assert_eq!(harbor["slot"], "stream");
        assert_eq!(harbor["liveFrames"], "unsupported");
        assert_eq!(harbor["policyRefs"].as_array().map(Vec::len), Some(2));
        assert!(live_eval_bind_metadata(
            crate::visuals::LiveEvalFamily::Harbor,
            &json!({"live_frames": "native"}),
            None
        )
        .is_err());
    }

    #[test]
    fn portable_trace_png_is_extracted_and_verified_for_eval_cas() {
        use std::io::Write;
        let png = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
            .unwrap();
        let digest = format!("sha256:{:x}", sha2::Sha256::digest(&png));
        let uri = format!("blobs/sha256/{}/{}", &digest[7..9], &digest[7..]);
        let document = json!({
            "identity": {"rollout_id": "roll_portable"},
            "provenance": {
                "producer_commit": "containers@abc123",
                "container_image_digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            },
            "artifacts": [{
                "artifact_id": "frame_0",
                "digest": digest,
                "media_type": "image/png",
                "size_bytes": png.len(),
                "uri": uri,
            }],
            "events": [{
                "event_type": "frame",
                "artifact_ids": ["frame_0"],
                "payload": {"step": 0, "source_event_digest": "producer16"},
            }],
        });
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("portable.zip");
        let file = fs::File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        archive
            .start_file("traces/roll_portable/sealed/trace.json", options)
            .unwrap();
        archive
            .write_all(&serde_json::to_vec(&document).unwrap())
            .unwrap();
        archive.start_file(&uri, options).unwrap();
        archive.write_all(&png).unwrap();
        archive.finish().unwrap();

        let (frames, max_step, provenance) =
            extract_imported_trace_frames(&archive_path, "roll_portable").unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(max_step, Some(0));
        assert_eq!(frames[0].bytes, png);
        assert_eq!(frames[0].step, 0);
        assert_eq!(frames[0].width, 1);
        assert_eq!(frames[0].height, 1);
        assert_eq!(frames[0].producer_digest.as_deref(), Some("producer16"));
        assert_eq!(provenance.unwrap()["producer_commit"], "containers@abc123");
    }
}
