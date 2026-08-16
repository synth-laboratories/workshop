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

fn required_authoring_checks(template_id: &str) -> Vec<&'static str> {
    let mut checks = BASE_AUTHORING_CHECKS.to_vec();
    checks.push("screenshotInspected");
    if template_id.starts_with("diagram.") {
        checks.push("noTextCollisions");
        checks.push("focalDensity");
    }
    if template_id == "live.craftax.v1" {
        checks.push("imageReplay");
    }
    checks
}

use anyhow::{Context, Result};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, net::SocketAddr, path::PathBuf, sync::Arc};
use tauri::{AppHandle, LogicalSize, Manager, Size};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualsIpcConnection {
    pub url: String,
    pub token: String,
    pub path: String,
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
            eprintln!("synth-desktop: visuals IPC stopped: {error:#}");
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
            JsonHttpResponse {
                status: StatusCode::CONFLICT,
                body: crate::container_capabilities::preflight_error_body(&error),
                extra_headers: Vec::new(),
            }
        }
        Err(error) if crate::error::error_is::<crate::plugins::PluginNotReady>(&error) => {
            let body = error
                .downcast_ref::<crate::plugins::PluginNotReady>()
                .map(crate::plugins::PluginNotReady::to_json)
                .unwrap_or_else(|| json!({"code":"plugin_not_ready"}));
            JsonHttpResponse {
                status: StatusCode::CONFLICT,
                body,
                extra_headers: Vec::new(),
            }
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
    if auth != Some(token) {
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
    if method == "POST" && path == "/v1/review-window/resize" {
        return resize_review_window(app, &json_body);
    }
    if path.starts_with("/v1/plugins") {
        return dispatch_plugins(method, path, json_body, core, app).await;
    }
    if method == "POST" && path == "/v1/sessions/present" {
        return present_session(app, core, json_body).await;
    }
    if path.starts_with("/v1/optimizers") {
        return dispatch_optimizer(method, path, json_body, core, app).await;
    }
    if path.starts_with("/v1/traces") {
        return dispatch_traces(method, path, json_body, core).await;
    }
    dispatch(method, path, json_body, core).await
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
        "current": {"width": current.width.round() as u64, "height": current.height.round() as u64}
    }))
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

fn validated_loopback_rollout_base(base: &str) -> Result<String> {
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
    wait_for_stream_subscribed(client, &poll_url, SUBSCRIBE_READY_TIMEOUT).await?;

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
        .get("slot")
        .or_else(|| body.get("streamSlot"))
        .or_else(|| body.get("stream_slot"))
        .and_then(Value::as_str)
        .unwrap_or(LIVE_EVAL_SLOT);
    assert_live_eval_slot(requested)?;
    if requested != LIVE_EVAL_SLOT {
        anyhow::bail!(
            "visuals IPC scripted rollouts bind slot \"{LIVE_EVAL_SLOT}\", not \"{requested}\""
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
    let family = classified
        .map(|family| family.as_str().to_string())
        .or_else(|| {
            info.as_ref()
                .and_then(|value| value.get("env_family").or_else(|| value.get("task_family")))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| request.task_family.clone());
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
                && !path.ends_with("/probe")
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
            let family = info
                .as_ref()
                .and_then(|v| v.get("env_family"))
                .and_then(Value::as_str)
                .map(str::to_string);
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
            let prepare_body = json!({"rollout_id": rollout_id, "telemetry": telemetry});
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
            let stream = declared_stream_descriptor(&prepared)?
                .context("prepare omitted stream descriptor")?;
            let poll_url = resolve_declared_url(&base, &declared_poll_url(&stream)?)?;
            let sse_url = resolve_declared_url(&base, &declared_sse_url(&stream)?)?;
            crate::visuals::assert_declared_stream_source(&sse_url)?;
            Ok(json!({
                "container_id": id, "rollout_id": rollout_id, "prepared": prepared, "stream": stream,
                "resolved": {"poll_url": poll_url, "sse_url": sse_url},
                "visual_binding": {"slot":"stream","kind":"live_sse","source":sse_url,"poll_url":poll_url,"schema":"synth.trace-stream-event.v1"},
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
            Ok(json!({"container_id": id, "rollout_id": rollout_id, "state": state}))
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
            let visual = registry.get(visual_id.to_string()).await?;
            let quality = visual
                .metadata
                .get("qualityGate")
                .filter(|value| value.get("ready").and_then(Value::as_bool) == Some(true))
                .context("refusing rollout start: visual is not ready")?;
            if quality.get("revision").and_then(Value::as_i64) != Some(visual.current_revision) {
                anyhow::bail!("refusing rollout start: visual readiness receipt is stale");
            }
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
            let subscription =
                wait_for_stream_subscribed(&client, &poll_url, SUBSCRIBE_READY_TIMEOUT).await?;
            let mut start_body = json!({
                "rollout_id": rollout_id, "seed": body.get("seed"), "task_instance_id": task_instance_id,
                "policy_ref": policy_ref, "telemetry": telemetry, "slot": LIVE_EVAL_SLOT
            });
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
            let (state, recovered) = start_rollout_idempotently(
                &client,
                &recovery_client,
                &base,
                rollout_id,
                &start_body,
            )
            .await
            .context("POST /rollouts after stream.subscribed")?;
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
                } = run_one_scripted_rollout(&client, &base, seed, &actions, None).await?;
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
        ("GET", path) if path.starts_with("/v1/visuals/templates/") => {
            let id = path.trim_start_matches("/v1/visuals/templates/");
            Ok(json!({"template": registry.get_template(id)?}))
        }
        ("GET", "/v1/visuals") => {
            let query: VisualQuery = serde_json::from_value(body.clone()).unwrap_or_default();
            Ok(json!({"visuals": registry.list(query).await?}))
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
            let required_checks = required_authoring_checks(&visual.template_id);
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
            {
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
            let required = required_authoring_checks(&current.template_id);
            for review in &current_reviews {
                for check in &required {
                    if review
                        .pointer(&format!("/checks/{check}"))
                        .and_then(Value::as_bool)
                        != Some(true)
                    {
                        anyhow::bail!("visual review is not passing required check {check}");
                    }
                }
            }
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
            let widths: std::collections::BTreeSet<u64> = current_reviews
                .iter()
                .filter_map(|review| review.pointer("/viewport/width").and_then(Value::as_u64))
                .collect();
            if widths.len() < 2 {
                anyhow::bail!("visual readiness requires reviews at two distinct viewport widths");
            }
            metadata.insert("qualityGate".into(), json!({"ready": true, "revision": revision, "reviewCount": current_reviews.len(), "readyAt": chrono::Utc::now().to_rfc3339()}));
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
            let (visual, event) = registry.show(id.to_string(), session_id).await?;
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
            Ok(json!({"visual": visual, "event": event}))
        }
        _ => anyhow::bail!("unsupported visuals IPC route {method} {path}"),
    }
}

async fn dispatch_optimizer(
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
        ("GET", "/v1/optimizers/recipes") => Ok(json!({ "recipes": optimizers.list_recipes() })),
        ("POST", "/v1/optimizers/eval/candidates") => {
            let request: crate::optimizers::EvalStageCandidatesRequest =
                serde_json::from_value(body)?;
            let manifest = optimizers.stage_eval_candidates(request).await?;
            Ok(json!({ "candidateSet": manifest }))
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
                        .unwrap_or("gepa.banking77.smoke.v1");
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
            let (run, event) = optimizers.cancel(id.to_string()).await?;
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
        _ => anyhow::bail!("unsupported optimizer IPC route {method} {path}"),
    }
}

/// Agent-facing Trace V5 access.
///
/// Read-only. `open` goes through the same `PresentationService` the native
/// Data page uses, so the agent cannot reach a different inspector, bind one
/// trace's visual to another's digest, or bypass the inspectability policy.
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
        ("POST", "/v1/traces/open") => {
            let id = trace_id()?;
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
mod tests {
    use super::*;

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
    fn craftax_readiness_requires_real_image_replay() {
        assert!(!required_authoring_checks("live.harbor_eval.v1").contains(&"imageReplay"));
        assert!(required_authoring_checks("live.craftax.v1").contains(&"imageReplay"));
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
        assert!(require_scripted_stream_slot(&json!({"slot": "live"})).is_err());
        assert!(require_scripted_stream_slot(&json!({"slot": "jobs"})).is_err());
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
        let outcome =
            run_one_scripted_rollout(&client, &base, 1, &["do".into()], Some("r1".into()))
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
        let error = run_one_scripted_rollout(&client, &base, 7, &["do".into()], Some("r1".into()))
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
        let err = run_one_scripted_rollout(&client, &base, 1, &["do".into()], Some("r1".into()))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("stream.subscribed"));
        assert!(!started.load(std::sync::atomic::Ordering::SeqCst));
        task.abort();
    }

    #[test]
    fn harbor_and_digbench_register_metadata_is_visual_first() {
        let harbor =
            live_eval_bind_metadata(crate::visuals::LiveEvalFamily::Harbor, &json!({}), None)
                .unwrap();
        assert_eq!(harbor["templateId"], "live.harbor_eval.v1");
        assert_eq!(harbor["slot"], "stream");
        assert_eq!(harbor["liveFrames"], "unsupported");
        assert_eq!(harbor["policyRefs"].as_array().map(Vec::len), Some(2));
        assert!(live_eval_bind_metadata(
            crate::visuals::LiveEvalFamily::Harbor,
            &json!({"live_frames": "native"}),
            None
        )
        .is_err());
        let digbench =
            live_eval_bind_metadata(crate::visuals::LiveEvalFamily::Digbench, &json!({}), None)
                .unwrap();
        assert_eq!(digbench["templateId"], "live.digbench.v1");
        assert_eq!(digbench["policyRefs"][0]["harness"], "react_legal_actions");
        assert_eq!(digbench["policyRefs"][1]["mcp_bind"], "digbench-mcp");
    }
}
