//! Eval loopback IPC — profile-gated programmatic driver for live Workshop.
//!
//! Wire protocol: `synth.eval-driver.v1` (see `EVAL_DRIVER.md` beside this crate).
//!
//! This is product code in the same category as [`crate::visuals_ipc`]: loopback-only
//! bind, bearer token, descriptor JSON in the instance data root. It is compiled into
//! debug/dev builds and only spawned for named development instances. Production /
//! release builds never listen.

use crate::codex::{self, CodexManager, CodexSessionStartRequest, CodexTurnSendRequest};
use crate::container_stream::{
    declared_poll_url, declared_sse_url, declared_stream_descriptor, refuse_auto_transport,
    require_caller_policy_ref, resolve_declared_url, wait_for_stream_subscribed,
    SUBSCRIBE_READY_TIMEOUT,
};
use crate::core_runtime::CoreRuntime;
use crate::ipc::{serve_json_with_limit, JsonHttpRequest, JsonHttpResponse};
use crate::laguna::LagunaManager;
use crate::storage::{EventAppend, EventSource};
use crate::synth_config;
use crate::trace_ingest::TraceBundleIngestRequest;
use crate::visuals::{
    assert_declared_stream_source, assert_live_eval_slot, classify_live_eval_family,
    craftax_ten_lane_pins, live_sse_bindings, pending_stream_bindings, resolve_live_eval_template,
    VisualCreateRequest, VisualUpdateRequest, CRAFTAX_TEN_LANE_SEEDS,
};
use crate::visuals_ipc;
use anyhow::{anyhow, bail, Context, Result};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, sync::Arc, time::Duration};
use tauri::AppHandle;
use tokio::net::TcpListener;
use uuid::Uuid;

pub const PROTOCOL_VERSION: &str = "synth.eval-driver.v1";
const OPENROUTER_CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
#[allow(dead_code)]
const DEFAULT_POLICY_ACTIONS: &[&str] = &["do", "left", "do", "up", "do", "right", "do", "down"];
const LIVE_EVAL_SLOT: &str = "stream";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalDriverConnection {
    pub schema_version: String,
    pub url: String,
    pub token: String,
    pub path: String,
    pub instance_name: Option<String>,
    pub source_revision: String,
}

pub fn connection_path(root: &std::path::Path) -> PathBuf {
    root.join("eval-driver.json")
}

/// Named development instances only. Release/production builds never spawn.
pub fn should_spawn() -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }
    crate::instance::name().is_some() || std::env::var_os("SYNTH_DESKTOP_EVAL_DRIVER").is_some()
}

pub struct EvalDriverDeps {
    pub core: Arc<CoreRuntime>,
    pub codex: Arc<CodexManager>,
    pub laguna: Arc<LagunaManager>,
    pub app: AppHandle,
}

pub async fn spawn(deps: EvalDriverDeps, root: PathBuf) -> Result<EvalDriverConnection> {
    if !should_spawn() {
        bail!("eval driver is disabled outside named development instances");
    }
    let token = format!("synth_eval_{}", Uuid::new_v4());
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("bind eval driver")?;
    let addr = listener.local_addr()?;
    let diagnostics = crate::instance::diagnostics();
    let connection = EvalDriverConnection {
        schema_version: PROTOCOL_VERSION.into(),
        url: format!("http://{addr}"),
        token: token.clone(),
        path: connection_path(&root).display().to_string(),
        instance_name: diagnostics.name.clone(),
        source_revision: diagnostics.source_revision.clone(),
    };
    fs::create_dir_all(&root)?;
    let connection_file = connection_path(&root);
    fs::write(&connection_file, serde_json::to_string_pretty(&connection)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&connection_file, fs::Permissions::from_mode(0o600))?;
    }
    patch_instance_manifest(&connection);

    let serve = Arc::new(deps);
    let serve_token = token.clone();
    tauri::async_runtime::spawn(async move {
        let result = serve_json_with_limit(
            listener,
            crate::limits::EVAL_DRIVER_MAX_BODY_BYTES,
            move |request| {
                let deps = serve.clone();
                let token = serve_token.clone();
                async move { route_request(request, &deps, &token).await }
            },
        )
        .await;
        if let Err(error) = result {
            eprintln!("synth-desktop: eval driver stopped: {error:#}");
        }
    });
    Ok(connection)
}

fn patch_instance_manifest(connection: &EvalDriverConnection) {
    let Some(path) = std::env::var_os(crate::instance::MANIFEST_ENV).map(PathBuf::from) else {
        return;
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut manifest) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    manifest["evalDriver"] = json!({
        "schemaVersion": PROTOCOL_VERSION,
        "url": connection.url,
        "descriptorPath": connection.path,
        "sourceRevision": connection.source_revision,
    });
    if let Ok(body) = serde_json::to_vec_pretty(&manifest) {
        let temporary = path.with_extension("json.evaldriver");
        if fs::write(&temporary, body).is_ok() {
            let _ = fs::rename(temporary, path);
        }
    }
}

async fn route_request(
    request: JsonHttpRequest,
    deps: &EvalDriverDeps,
    token: &str,
) -> JsonHttpResponse {
    let result = dispatch_request(request, deps, token).await;
    let mut response = match result {
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
        Err(error) if crate::error::error_is::<crate::error::ProtocolMismatch>(&error) => {
            JsonHttpResponse::error(StatusCode::UPGRADE_REQUIRED, error.to_string())
        }
        Err(error) => JsonHttpResponse::error(StatusCode::BAD_REQUEST, error.to_string()),
    };
    response
        .extra_headers
        .push(("x-synth-eval-driver", PROTOCOL_VERSION.to_owned()));
    response
}

async fn dispatch_request(
    request: JsonHttpRequest,
    deps: &EvalDriverDeps,
    token: &str,
) -> Result<Value> {
    let auth = request
        .authorization
        .as_deref()
        .and_then(|value| value.strip_prefix("Bearer ").map(str::trim));
    if auth != Some(token) {
        return Err(anyhow!(crate::error::Unauthorized).context("unauthorized eval driver request"));
    }
    if let Some(version) = request
        .raw_headers
        .get("x-synth-eval-driver")
        .and_then(|value| value.to_str().ok())
    {
        if version != PROTOCOL_VERSION {
            return Err(anyhow!(crate::error::ProtocolMismatch {
                expected: PROTOCOL_VERSION,
                got: version.to_owned(),
            }));
        }
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
    dispatch(request.method.as_str(), path, json_body, deps).await
}

fn query_json(query: &str) -> Value {
    let mut object = Map::new();
    for item in query.split('&').filter(|item| !item.is_empty()) {
        let (key, value) = item.split_once('=').unwrap_or((item, ""));
        object.insert(key.to_string(), Value::String(percent_decode(value)));
    }
    Value::Object(object)
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = &value[i + 1..i + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn dispatch(method: &str, path: &str, body: Value, deps: &EvalDriverDeps) -> Result<Value> {
    if let Some(route) = laguna_eval_route(method, path) {
        return dispatch_laguna(route, &deps.laguna).await;
    }
    let core = &deps.core;
    match (method, path) {
        ("GET", "/health") | ("GET", "/v1/health") => Ok(json!({
            "ok": true,
            "service": "synth-eval-driver",
            "schemaVersion": PROTOCOL_VERSION,
            "instance": crate::instance::diagnostics(),
        })),
        ("POST", "/v1/sessions") | ("POST", "/v1/create_session") => {
            create_session(deps, body).await
        }
        ("POST", path) if path.starts_with("/v1/sessions/") && path.ends_with("/messages") => {
            let session_id = path
                .trim_start_matches("/v1/sessions/")
                .trim_end_matches("/messages")
                .trim_end_matches('/')
                .to_string();
            send_message(deps, &session_id, body).await
        }
        ("POST", "/v1/send_message") => {
            let session_id = body
                .get("sessionId")
                .and_then(|value| value.as_str().map(str::to_string))
                .context("send_message requires sessionId")?;
            send_message(deps, &session_id, body).await
        }
        ("POST", path) if path.starts_with("/v1/sessions/") && path.ends_with("/wait_terminal") => {
            let session_id = path
                .trim_start_matches("/v1/sessions/")
                .trim_end_matches("/wait_terminal")
                .trim_end_matches('/')
                .to_string();
            wait_for_terminal(core, &session_id, body).await
        }
        ("POST", "/v1/wait_for_terminal") => {
            let session_id = body
                .get("sessionId")
                .and_then(|value| value.as_str().map(str::to_string))
                .context("wait_for_terminal requires sessionId")?;
            wait_for_terminal(core, &session_id, body).await
        }
        ("GET", path) if path.starts_with("/v1/sessions/") && path.ends_with("/export") => {
            let session_id = path
                .trim_start_matches("/v1/sessions/")
                .trim_end_matches("/export")
                .trim_end_matches('/')
                .to_string();
            export_session(core, &deps.codex, &session_id).await
        }
        ("POST", "/v1/export_session") => {
            let session_id = body
                .get("sessionId")
                .and_then(|value| value.as_str().map(str::to_string))
                .context("export_session requires sessionId")?;
            export_session(core, &deps.codex, &session_id).await
        }
        ("GET", "/v1/containers") | ("POST", "/v1/containers") => {
            visuals_ipc::dispatch(method, path, body, core).await
        }
        ("GET", path)
            if path.starts_with("/v1/containers/")
                && !path.ends_with("/probe")
                && !path.ends_with("/rollouts")
                && !path.ends_with("/policy_rollouts") =>
        {
            visuals_ipc::dispatch(method, path, body, core).await
        }
        ("POST", path) if path.starts_with("/v1/containers/") && path.ends_with("/probe") => {
            visuals_ipc::dispatch(method, path, body, core).await
        }
        ("POST", path) if path.starts_with("/v1/containers/") && path.ends_with("/rollouts") => {
            // Scripted bounded rollouts (transport gate) — same as visuals IPC.
            visuals_ipc::dispatch(method, path, body, core).await
        }
        ("POST", path)
            if path.starts_with("/v1/containers/") && path.ends_with("/policy_rollouts") =>
        {
            let id = path
                .trim_start_matches("/v1/containers/")
                .trim_end_matches("/policy_rollouts")
                .trim_end_matches('/');
            run_policy_rollout(deps, id, body).await
        }
        ("POST", "/v1/container_register") => {
            visuals_ipc::dispatch("POST", "/v1/containers", body, core).await
        }
        ("POST", "/v1/container_probe") => {
            let id = body
                .get("containerId")
                .and_then(Value::as_str)
                .context("container_probe requires containerId")?;
            visuals_ipc::dispatch(
                "POST",
                &format!("/v1/containers/{id}/probe"),
                json!({}),
                core,
            )
            .await
        }
        ("POST", "/v1/open_visual") => open_visual(core, body).await,
        ("POST", path) if path.starts_with("/v1/visuals/") && path.ends_with("/update") => {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/update")
                .trim_end_matches('/');
            update_visual(core, id, body).await
        }
        ("POST", path) if path.starts_with("/v1/visuals/") && path.ends_with("/show") => {
            visuals_ipc::dispatch(method, path, body, core).await
        }
        ("POST", path)
            if path.starts_with("/v1/visuals/") && path.ends_with("/visualsbench_export") =>
        {
            let id = path
                .trim_start_matches("/v1/visuals/")
                .trim_end_matches("/visualsbench_export")
                .trim_end_matches('/');
            export_visualsbench(core, id, body).await
        }
        ("POST", "/v1/traces/ingest") => ingest_trace_bundle(core, body).await,
        ("POST", "/v1/policy_preflight") => policy_preflight(deps, body).await,
        _ => bail!("unsupported eval driver route {method} {path}"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LagunaEvalRoute {
    Ensure,
    Status,
    Inference,
    Unload,
}

fn laguna_eval_route(method: &str, path: &str) -> Option<LagunaEvalRoute> {
    match (method, path) {
        ("POST", "/v1/laguna/ensure") => Some(LagunaEvalRoute::Ensure),
        ("GET", "/v1/laguna/status") => Some(LagunaEvalRoute::Status),
        ("GET", "/v1/laguna/inference") => Some(LagunaEvalRoute::Inference),
        ("POST", "/v1/laguna/model/unload") => Some(LagunaEvalRoute::Unload),
        _ => None,
    }
}

async fn dispatch_laguna(route: LagunaEvalRoute, laguna: &LagunaManager) -> Result<Value> {
    match route {
        LagunaEvalRoute::Ensure => laguna_ensure(laguna).await,
        LagunaEvalRoute::Status => laguna_status(laguna).await,
        LagunaEvalRoute::Inference => Ok(serde_json::to_value(laguna.inference_snapshot().await?)?),
        LagunaEvalRoute::Unload => Ok(serde_json::to_value(laguna.unload_model().await?)?),
    }
}

/// Start the same managed Laguna runtime used by the composer, retaining the
/// product status as the prerequisite receipt when weights or hardware are not
/// available. Provider credentials and the daemon key never cross this API.
async fn laguna_ensure(laguna: &LagunaManager) -> Result<Value> {
    let root = crate::runtime::workshop_root()?;
    let base_url = match laguna.ensure_for_turn(&root).await {
        Ok(base_url) => base_url,
        Err(error) => {
            laguna.set_error(error.to_string()).await;
            None
        }
    };
    let product_status = laguna.status().await;
    let (outcome, code) = classify_laguna_ensure(&product_status, base_url.is_some());
    let mut status = serde_json::to_value(product_status)?;
    let status_object = status
        .as_object_mut()
        .context("Laguna status must serialize as an object")?;
    status_object.insert("outcome".into(), Value::String(outcome.into()));
    status_object.insert("code".into(), Value::String(code.into()));
    Ok(json!({
        "ok": base_url.is_some(),
        "baseUrl": base_url,
        "status": status,
    }))
}

fn classify_laguna_ensure(
    status: &crate::laguna::LagunaStatus,
    ready: bool,
) -> (&'static str, &'static str) {
    if ready && status.phase == "ready" {
        return ("ready", "ready");
    }
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return ("unmet_prerequisite", "hardware_unsupported");
    }
    if status.phase == "not_installed" {
        return ("unmet_prerequisite", "weights_unavailable");
    }
    ("product_error", "runtime_unavailable")
}

/// Mirror `laguna_get_status`: the first read ensures the managed runtime and
/// subsequent reads refresh `/health`, so evals observe the same status the UI
/// renders rather than a parallel test probe.
async fn laguna_status(laguna: &LagunaManager) -> Result<Value> {
    let status = if laguna.status().await.phase == "unknown" {
        let root = crate::runtime::workshop_root()?;
        if let Err(error) = laguna.ensure(&root).await {
            laguna.set_error(error.to_string()).await;
        }
        laguna.status().await
    } else {
        laguna.refresh().await
    };
    Ok(serde_json::to_value(status)?)
}

async fn export_visualsbench(core: &CoreRuntime, visual_id: &str, body: Value) -> Result<Value> {
    let visual = core.visuals().get(visual_id.to_string()).await?;
    let mut revisions = core.visuals().revisions(visual_id.to_string()).await?;
    revisions.sort_by_key(|row| row.revision);
    let current_revision = revisions
        .iter()
        .find(|row| row.revision == visual.current_revision)
        .context("current visual revision is missing")?;
    let annotations = core.visuals().annotations(visual_id.to_string()).await?;
    let active_annotations = annotations
        .into_iter()
        .filter(|row| !row.tombstoned && row.visual_revision <= visual.current_revision)
        .collect::<Vec<_>>();
    let overlay_digest = core
        .visuals()
        .overlay_digest(visual_id.to_string(), visual.current_revision)
        .await?;

    let mut journal = Vec::new();
    let mut after = 0_i64;
    loop {
        let page = core.journal().events_after(after, 1_000).await?;
        if page.is_empty() {
            break;
        }
        for event in &page {
            after = after.max(event.sequence);
            if event.payload.get("visualId").and_then(Value::as_str) == Some(visual_id) {
                journal.push(json!({
                    "kind": event.kind,
                    "visualId": visual_id,
                    "revision": event.payload.get("revision").or_else(|| event.payload.get("visualRevision")),
                    "sequence": event.sequence,
                }));
            }
        }
        if page.len() < 1_000 || journal.len() > 50_000 {
            break;
        }
    }

    let requested_viewports = body
        .get("viewports")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let reviews = visual
        .metadata
        .get("authoringReviews")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let captures = reviews
        .iter()
        .filter(|review| {
            review.get("revision").and_then(Value::as_i64) == Some(visual.current_revision)
        })
        .filter_map(|review| {
            let viewport = review.get("viewport")?;
            let width = viewport.get("width")?.as_u64()?;
            let height = viewport.get("height")?.as_u64()?;
            let requested = requested_viewports.iter().find(|candidate| {
                candidate.get("width").and_then(Value::as_u64) == Some(width)
                    && candidate.get("height").and_then(Value::as_u64) == Some(height)
            });
            if !requested_viewports.is_empty() && requested.is_none() {
                return None;
            }
            let screenshot_path = review.get("screenshotPath")?.as_str()?;
            let bytes = fs::read(screenshot_path).ok();
            let screenshot_sha256 = bytes
                .as_deref()
                .map(hex_sha256)
                .unwrap_or_default();
            let checks = review.get("checks").cloned().unwrap_or_else(|| json!({}));
            Some(json!({
                "viewport": {
                    "width": width,
                    "height": height,
                    "name": requested.and_then(|value| value.get("name")).and_then(Value::as_str).unwrap_or("recorded"),
                },
                "screenshotPath": screenshot_path,
                "screenshotSha256": screenshot_sha256,
                "findings": {
                    "noTextCollisions": checks.get("noTextCollisions").cloned().unwrap_or(Value::Null),
                    "noHorizontalOverflow": checks.get("noOverflow").cloned().unwrap_or(Value::Null),
                    "falsifiedMissing": checks.get("falsifiedMissing").cloned().unwrap_or(Value::Bool(false)),
                },
                "inspected": bytes.is_some() && checks.get("screenshotInspected").and_then(Value::as_bool) == Some(true),
            }))
        })
        .collect::<Vec<_>>();
    let annotation_ids = active_annotations
        .iter()
        .map(|row| Value::String(row.id.clone()))
        .collect::<Vec<_>>();
    let trace_digest = visual.trace_id.clone();
    Ok(json!({
        "schemaVersion": "synth.visualsbench-export.v1",
        "sourceRevision": crate::instance::diagnostics().source_revision,
        "visual": {
            "id": visual.id,
            "revision": visual.current_revision,
            "templateId": current_revision.template_id,
            "contentDigest": current_revision.content_digest,
            "bindingsDigest": current_revision.bindings_digest,
            "bindings": current_revision.bindings.clone().unwrap_or_else(|| visual.bindings.clone()),
        },
        "revisions": revisions.iter().map(|row| json!({
            "visualId": row.visual_id,
            "revision": row.revision,
            "contentDigest": row.content_digest,
            "bindingsDigest": row.bindings_digest,
        })).collect::<Vec<_>>(),
        "journal": journal,
        "annotations": active_annotations,
        "overlayDigest": overlay_digest,
        "nextTurnContext": {
            "visualId": visual_id,
            "overlayDigest": overlay_digest,
            "annotationIds": annotation_ids,
        },
        "traceDigestBefore": trace_digest,
        "traceDigestAfter": trace_digest,
        "captures": captures,
        "taskGraderRef": visual.metadata.get("taskGraderRef").cloned().unwrap_or(Value::Null),
    }))
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

async fn create_session(deps: &EvalDriverDeps, body: Value) -> Result<Value> {
    let session_id = body
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("eval_{}", Uuid::new_v4()));
    let workspace = body
        .get("workspace")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| std::env::var("SYNTH_DESKTOP_WORKSPACE").ok())
        .unwrap_or_else(|| {
            crate::instance::data_root()
                .join("workspace")
                .display()
                .to_string()
        });
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("openai/gpt-5.6-luna")
        .to_string();
    let provider_name = body
        .get("provider")
        .or_else(|| body.get("providerName"))
        .and_then(Value::as_str)
        .unwrap_or("openrouter")
        .to_string();
    let mut start = CodexSessionStartRequest {
        session_id: session_id.clone(),
        workspace,
        base_url: body
            .get("baseUrl")
            .and_then(Value::as_str)
            .unwrap_or("https://openrouter.ai/api/v1")
            .to_string(),
        api_key: String::new(),
        model,
        provider_name: Some(provider_name),
        provider_title: body
            .get("providerTitle")
            .and_then(Value::as_str)
            .map(str::to_string),
        provider_env_key: None,
        approval_policy: Some(
            body.get("approvalPolicy")
                .and_then(Value::as_str)
                .unwrap_or("never")
                .into(),
        ),
        sandbox: Some(
            body.get("sandbox")
                .and_then(Value::as_str)
                .unwrap_or("workspace-write")
                .into(),
        ),
        service_tier: body
            .get("serviceTier")
            .and_then(Value::as_str)
            .map(str::to_string),
        thread_id: None,
        multi_agent_version: None,
        auto_compact_token_limit: body.get("autoCompactTokenLimit").and_then(Value::as_u64),
        writable_roots: Vec::new(),
        local_model_catalog: None,
        broker_credential: false,
    };
    start = prepare_start(&deps.laguna, start).await?;
    let info = deps
        .codex
        .start(deps.app.clone(), start)
        .await
        .context("codex session start")?;
    Ok(json!({
        "sessionId": session_id,
        "info": info,
    }))
}

/// Mirrors `prepare_codex_provider` in `lib.rs`: one preparation rule per
/// `codex::ProviderClass`, so the eval driver exercises the same credential
/// custody the product uses.
async fn prepare_start(
    laguna: &LagunaManager,
    mut request: CodexSessionStartRequest,
) -> Result<CodexSessionStartRequest> {
    request.local_model_catalog = None;
    match crate::codex::provider_class(request.provider_name.as_deref()) {
        crate::codex::ProviderClass::LocalLaguna => {
            let root = crate::runtime::workshop_root()?;
            let model = laguna.configured_model_id()?;
            crate::codex::apply_local_laguna_provider(&mut request, &model);
            request.base_url = laguna
                .ensure_for_turn(&root)
                .await?
                .ok_or_else(|| anyhow!("Laguna Responses server is unavailable"))?;
            request.api_key = laguna
                .api_key()
                .context("Laguna daemon credential is unavailable after ensure")?;
            let catalog = laguna
                .codex_model_catalog(&request.base_url, &request.api_key)
                .await?;
            crate::codex::apply_local_laguna_catalog_metadata(&mut request, catalog)?;
        }
        crate::codex::ProviderClass::OpenRouter => {
            let key = synth_config::openrouter_api_key()?;
            // Staged for native custody, exactly like the production path;
            // `CodexManager::start` exchanges it for a loopback lease at spawn.
            crate::codex::apply_openrouter_provider(&mut request, key.as_deref())
                .map_err(|message| anyhow!(message))?;
        }
        crate::codex::ProviderClass::SynthCloud => {
            let resolved = synth_config::resolve()?;
            // Same split as the production path in `lib.rs`: only Codex's
            // Responses traffic uses the source-owned profile gateway, never
            // account/billing. An unknown profile fails closed instead of
            // silently reusing the backend URL.
            let gateway_url = synth_config::require_responses_gateway_url(&resolved)
                .map_err(|message| anyhow!(message))?;
            crate::codex::apply_synth_cloud_provider(
                &mut request,
                &gateway_url,
                resolved.api_key.as_deref(),
            )
            .map_err(|message| anyhow!(message))?;
        }
        crate::codex::ProviderClass::OpenaiCodexOauth => {
            bail!("ChatGPT subscription sessions are not available through the eval driver")
        }
        crate::codex::ProviderClass::Direct => {}
    }
    Ok(request)
}

async fn send_message(deps: &EvalDriverDeps, session_id: &str, body: Value) -> Result<Value> {
    let prompt = body
        .get("body")
        .or_else(|| body.get("prompt"))
        .or_else(|| body.get("message"))
        .and_then(Value::as_str)
        .context("send_message requires body")?
        .to_string();
    let effort = body
        .get("effort")
        .or_else(|| body.get("reasoningEffort"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let workspace = body
        .get("workspace")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| std::env::var("SYNTH_DESKTOP_WORKSPACE").ok())
        .unwrap_or_else(|| {
            crate::instance::data_root()
                .join("workspace")
                .display()
                .to_string()
        });
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("openai/gpt-5.6-luna")
        .to_string();
    let provider_name = body
        .get("provider")
        .or_else(|| body.get("providerName"))
        .and_then(Value::as_str)
        .unwrap_or("openrouter")
        .to_string();
    let mut start = CodexSessionStartRequest {
        session_id: session_id.to_string(),
        workspace,
        base_url: body
            .get("baseUrl")
            .and_then(Value::as_str)
            .unwrap_or("https://openrouter.ai/api/v1")
            .to_string(),
        api_key: String::new(),
        model,
        provider_name: Some(provider_name),
        provider_title: None,
        provider_env_key: None,
        approval_policy: Some("never".into()),
        sandbox: Some("workspace-write".into()),
        service_tier: body
            .get("serviceTier")
            .and_then(Value::as_str)
            .map(str::to_string),
        thread_id: None,
        multi_agent_version: None,
        auto_compact_token_limit: body.get("autoCompactTokenLimit").and_then(Value::as_u64),
        writable_roots: Vec::new(),
        local_model_catalog: None,
        broker_credential: false,
    };
    start = prepare_start(&deps.laguna, start).await?;
    let info = deps
        .codex
        .send_turn(
            deps.app.clone(),
            CodexTurnSendRequest {
                start,
                prompt,
                effort,
                compact_before_model_switch: false,
                client_message_id: None,
            },
        )
        .await
        .map_err(|failure| anyhow!("{}: {}", failure.code, failure.message))?;
    Ok(json!({"ok": true, "sessionId": session_id, "info": info}))
}

async fn wait_for_terminal(core: &CoreRuntime, session_id: &str, body: Value) -> Result<Value> {
    let timeout_ms = body
        .get("timeoutMs")
        .and_then(Value::as_u64)
        .unwrap_or(600_000)
        .clamp(1_000, 3_600_000);
    let poll_ms = body
        .get("pollMs")
        .and_then(Value::as_u64)
        .unwrap_or(500)
        .clamp(100, 5_000);
    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    let mut after = 0_i64;
    loop {
        let events = core
            .journal()
            .session_events_after(session_id.to_string(), after, 500)
            .await?;
        for event in &events {
            after = after.max(event.session_sequence.unwrap_or(event.sequence));
            let kind = event.kind.as_str();
            if is_terminal_event(kind, &event.payload) {
                return Ok(json!({
                    "terminal": true,
                    "kind": kind,
                    "event": event,
                    "sessionId": session_id,
                }));
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(json!({
                "terminal": false,
                "timedOut": true,
                "sessionId": session_id,
                "afterSequence": after,
            }));
        }
        tokio::time::sleep(Duration::from_millis(poll_ms)).await;
    }
}

fn is_terminal_event(kind: &str, payload: &Value) -> bool {
    matches!(
        kind,
        "run.completed"
            | "run.failed"
            | "run.cancelled"
            | "session.run.completed"
            | "session.run.failed"
    ) || (kind == "run.status_changed"
        && matches!(
            payload.get("to").and_then(Value::as_str),
            Some("completed" | "failed" | "cancelled" | "interrupted")
        ))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvalProviderBinding {
    provider: String,
    model: String,
    endpoint_class: &'static str,
    credential_binding: &'static str,
    brokered: bool,
    fallback_allowed: bool,
}

fn eval_provider_binding(record: &crate::codex::CodexSessionRecord) -> EvalProviderBinding {
    let (endpoint_class, credential_binding, brokered) =
        match crate::codex::provider_class(Some(&record.provider_name)) {
            crate::codex::ProviderClass::LocalLaguna => {
                ("local-loopback-responses", "local-daemon-bearer", false)
            }
            crate::codex::ProviderClass::OpenRouter => {
                ("openrouter-responses", "native-loopback-lease", true)
            }
            crate::codex::ProviderClass::SynthCloud => (
                "synth-cloud-responses-gateway",
                "native-loopback-lease",
                true,
            ),
            crate::codex::ProviderClass::OpenaiCodexOauth => {
                ("chatgpt-codex", "oauth-session-file", false)
            }
            crate::codex::ProviderClass::Direct => {
                ("custom-responses", "provider-environment", false)
            }
        };
    EvalProviderBinding {
        provider: record.provider_name.clone(),
        model: record.model.clone(),
        endpoint_class,
        credential_binding,
        brokered,
        fallback_allowed: false,
    }
}

async fn export_session(
    core: &CoreRuntime,
    codex: &CodexManager,
    session_id: &str,
) -> Result<Value> {
    let mut events = Vec::new();
    let mut after = 0_i64;
    loop {
        let page = core
            .journal()
            .session_events_after(session_id.to_string(), after, 500)
            .await?;
        if page.is_empty() {
            break;
        }
        for event in &page {
            after = after.max(event.session_sequence.unwrap_or(event.sequence));
        }
        events.extend(page);
        if events.len() > 50_000 {
            break;
        }
    }
    let session = core.sessions().get(session_id.to_string()).await?;
    let provider_binding = codex
        .list()
        .await
        .into_iter()
        .find(|record| record.session_id == session_id)
        .map(|record| eval_provider_binding(&record))
        .with_context(|| format!("Codex provider binding missing for eval session {session_id}"))?;
    let visuals = core
        .visuals()
        .list(crate::visuals::VisualQuery {
            session_id: Some(session_id.to_string()),
            limit: Some(100),
            ..Default::default()
        })
        .await?;
    let mut visual_exports = Vec::with_capacity(visuals.len());
    for visual in visuals {
        let revisions = core.visuals().revisions(visual.id.clone()).await?;
        let renditions = core.visuals().list_renditions(visual.id.clone()).await?;
        let content = core.visuals().visual_source(visual.id.clone()).await.ok();
        visual_exports.push(json!({
            "visual": visual,
            "revisions": revisions,
            "renditions": renditions,
            "content": content,
        }));
    }
    Ok(json!({
        "schemaVersion": "synth.eval-session-export.v1",
        "sessionId": session_id,
        "session": session,
        "providerBinding": provider_binding,
        "events": events,
        "eventCount": events.len(),
        "visuals": visual_exports,
        "sourceRevision": crate::instance::diagnostics().source_revision,
    }))
}

async fn open_visual(core: &CoreRuntime, body: Value) -> Result<Value> {
    let container_id = body
        .get("containerId")
        .or_else(|| body.get("container_id"))
        .and_then(Value::as_str);
    let mut family = body
        .get("family")
        .and_then(Value::as_str)
        .and_then(|value| classify_live_eval_family(&json!({"runtime_family": value}), None));
    if let Some(id) = container_id {
        let container = core.data().get_container(id.to_string()).await?;
        family = family.or_else(|| {
            container
                .metadata
                .get("liveEval")
                .and_then(|value| value.get("family"))
                .and_then(Value::as_str)
                .and_then(|value| {
                    classify_live_eval_family(&json!({"runtime_family": value}), None)
                })
        });
        family = family.or_else(|| {
            classify_live_eval_family(
                container.metadata.get("info").unwrap_or(&json!({})),
                container.task_family.as_deref(),
            )
        });
    }
    let requested = body.get("templateId").and_then(Value::as_str);
    let template_id = resolve_live_eval_template(requested, family)?;
    let session_id = body
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let title = body
        .get("title")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| match family {
            Some(family) => format!("{} live eval", family.as_str()),
            None => "Live container eval".into(),
        });
    let bindings = body
        .get("bindings")
        .cloned()
        .unwrap_or_else(pending_stream_bindings);
    let create = VisualCreateRequest {
        template_id,
        title: Some(title),
        bindings: Some(bindings),
        id: None,
        status: None,
        renderer_kind: None,
        session_id: session_id.clone(),
        message_id: None,
        run_id: None,
        trace_id: None,
        parent_visual_id: None,
        source_agent_id: None,
        source_model: None,
        content: None,
        metadata: body.get("metadata").cloned(),
    };
    let (visual, event) = core.visuals().create(create).await?;
    let (shown, show_event) = core.visuals().show(visual.id.clone(), session_id).await?;
    core.broadcast_committed(Some(serde_json::from_value(event.clone())?));
    core.broadcast_committed(Some(serde_json::from_value(show_event.clone())?));
    Ok(json!({
        "opened": true,
        "visual": shown,
        "createEvent": event,
        "showEvent": show_event,
        "templateId": shown.template_id,
        "family": family.map(|value| value.as_str()),
    }))
}

async fn update_visual(core: &CoreRuntime, visual_id: &str, body: Value) -> Result<Value> {
    let request = VisualUpdateRequest {
        title: body
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string),
        bindings: body.get("bindings").cloned(),
        status: None,
        renderer_kind: None,
        message_id: None,
        run_id: None,
        trace_id: body
            .get("traceId")
            .and_then(Value::as_str)
            .map(str::to_string),
        content: None,
        metadata: body.get("metadata").cloned(),
        bump_revision: Some(true),
    };
    let (visual, event) = core
        .visuals()
        .update(visual_id.to_string(), request)
        .await?;
    core.broadcast_committed(Some(serde_json::from_value(event.clone())?));
    Ok(json!({ "ok": true, "visual": visual, "event": event }))
}

async fn ingest_trace_bundle(core: &CoreRuntime, body: Value) -> Result<Value> {
    let source_path = body
        .get("sourcePath")
        .or_else(|| body.get("source_path"))
        .and_then(Value::as_str)
        .context("traces/ingest requires sourcePath")?
        .to_string();
    let request = TraceBundleIngestRequest {
        source_path,
        source_kind: body
            .get("sourceKind")
            .or_else(|| body.get("source_kind"))
            .and_then(Value::as_str)
            .map(str::to_string),
        title: body
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string),
        source_uri: body
            .get("sourceUri")
            .or_else(|| body.get("source_uri"))
            .and_then(Value::as_str)
            .map(str::to_string),
    };
    let (result, event) = core.data().ingest_trace_bundle(request).await?;
    core.broadcast_committed(event);
    Ok(serde_json::to_value(result)?)
}

async fn policy_preflight(deps: &EvalDriverDeps, body: Value) -> Result<Value> {
    let provider = body
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("openrouter");
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match resolve_policy_target(deps, provider, &model).await {
        Ok(target) => match probe_policy_endpoint(&target).await {
            Ok(()) => Ok(json!({
                "ok": true,
                "provider": target.provider,
                "model": model,
                "chatUrl": target.chat_url,
            })),
            Err(error) => Ok(json!({
                "ok": false,
                "provider": target.provider,
                "model": model,
                "chatUrl": target.chat_url,
                "detail": error.to_string(),
            })),
        },
        Err(error) => Ok(json!({
            "ok": false,
            "provider": provider,
            "model": model,
            "detail": error.to_string(),
        })),
    }
}

async fn probe_policy_endpoint(target: &PolicyTarget) -> Result<()> {
    // Fail closed on unreachable provider endpoints before any paid rollout batch.
    // Deliberately no completion request — connectivity only.
    let url = reqwest::Url::parse(&target.chat_url)
        .with_context(|| format!("invalid provider chat URL: {}", target.chat_url))?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("provider chat URL missing host: {}", target.chat_url))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let addr = format!("{host}:{port}");
    tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    .with_context(|| format!("provider endpoint timed out: {addr}"))?
    .with_context(|| format!("provider endpoint unreachable: {addr}"))?;
    Ok(())
}

#[derive(Clone, Debug)]
struct PolicyTarget {
    provider: String,
    chat_url: String,
    api_key: String,
    /// `chat` = OpenAI chat completions; `responses` = OpenAI Responses API.
    wire: &'static str,
}

fn policy_chat_url(provider: &str, base: &str) -> (String, &'static str) {
    let trimmed = base.trim_end_matches('/');
    match provider {
        "openrouter" => {
            if trimmed.ends_with("/chat/completions") {
                (trimmed.to_string(), "chat")
            } else if trimmed.ends_with("/v1") || trimmed.ends_with("/api/v1") {
                (format!("{trimmed}/chat/completions"), "chat")
            } else {
                (OPENROUTER_CHAT_URL.to_string(), "chat")
            }
        }
        "synth-cloud" => {
            // Synth Cloud Codex path is Responses (`{backend}/api/v1` + /responses).
            if trimmed.ends_with("/responses") {
                (trimmed.to_string(), "responses")
            } else if trimmed.ends_with("/api/v1") {
                (format!("{trimmed}/responses"), "responses")
            } else {
                (format!("{trimmed}/api/v1/responses"), "responses")
            }
        }
        "local-laguna" => {
            if trimmed.ends_with("/chat/completions") {
                (trimmed.to_string(), "chat")
            } else if trimmed.ends_with("/v1") {
                (format!("{trimmed}/chat/completions"), "chat")
            } else {
                (format!("{trimmed}/v1/chat/completions"), "chat")
            }
        }
        _ => (format!("{trimmed}/v1/chat/completions"), "chat"),
    }
}

async fn resolve_policy_target(
    deps: &EvalDriverDeps,
    provider: &str,
    _model: &str,
) -> Result<PolicyTarget> {
    match codex::provider_class(Some(provider)) {
        codex::ProviderClass::OpenRouter => {
            let api_key = synth_config::openrouter_api_key()?.ok_or_else(|| {
                anyhow!("OpenRouter API key is not configured on the Workshop host")
            })?;
            let (chat_url, wire) = policy_chat_url("openrouter", OPENROUTER_CHAT_URL);
            Ok(PolicyTarget {
                provider: "openrouter".into(),
                chat_url,
                api_key,
                wire,
            })
        }
        codex::ProviderClass::SynthCloud => {
            let resolved = synth_config::resolve()?;
            let api_key = resolved.api_key.clone().ok_or_else(|| {
                anyhow!("Synth Cloud API key is not configured on the Workshop host")
            })?;
            // Same split as Codex's own session-start path: only Responses
            // traffic uses the source-owned profile gateway, and an unknown
            // profile fails closed rather than silently reusing
            // the backend URL. `resolved.backend_url` above still backs the
            // API key lookup; nothing here touches account/billing endpoints.
            let gateway_url = synth_config::require_responses_gateway_url(&resolved)
                .map_err(|message| anyhow!(message))?;
            let (chat_url, wire) = policy_chat_url("synth-cloud", &gateway_url);
            Ok(PolicyTarget {
                provider: "synth-cloud".into(),
                chat_url,
                api_key,
                wire,
            })
        }
        codex::ProviderClass::LocalLaguna => {
            let root = crate::runtime::workshop_root()?;
            let base_url = deps
                .laguna
                .ensure_for_turn(&root)
                .await?
                .ok_or_else(|| anyhow!("local Laguna daemon is unavailable"))?;
            let api_key = deps
                .laguna
                .api_key()
                .unwrap_or_else(|| "local-laguna".into());
            let (chat_url, wire) = policy_chat_url("local-laguna", &base_url);
            Ok(PolicyTarget {
                provider: "local-laguna".into(),
                chat_url,
                api_key,
                wire,
            })
        }
        codex::ProviderClass::OpenaiCodexOauth => {
            bail!("policy_rollouts do not accept ChatGPT subscription credentials")
        }
        codex::ProviderClass::Direct => {
            bail!("policy_rollouts require provider=openrouter|synth-cloud|local-laguna")
        }
    }
}

async fn run_policy_rollout(
    deps: &EvalDriverDeps,
    container_id: &str,
    body: Value,
) -> Result<Value> {
    let core = &deps.core;
    let container = core.data().get_container(container_id.to_string()).await?;
    // Same gate as the direct IPC prepare route: reject unhealthy, stale, or
    // capability-incompatible records before any mutating call.
    crate::container_capabilities::preflight_prepare_request(&container, &body)?;
    let base = container
        .base_url
        .as_deref()
        .context("container has no base URL")?;
    let base = validated_loopback_base(base)?;
    let task_instance_id = body
        .get("taskInstanceId")
        .or_else(|| body.get("task_instance_id"))
        .and_then(Value::as_str)
        .context("policy_rollouts require taskInstanceId")?
        .to_string();
    let provider = body
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("openrouter");
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .context("policy_rollouts require model")?
        .to_string();
    let reasoning_effort = body
        .get("reasoningEffort")
        .or_else(|| body.get("reasoning_effort"))
        .and_then(Value::as_str)
        .unwrap_or("medium")
        .to_string();
    let timeout_s = body
        .get("timeoutS")
        .or_else(|| body.get("timeout_per_rollout_s"))
        .and_then(Value::as_u64)
        .unwrap_or(600)
        .clamp(30, 3600);
    let telemetry = body.get("telemetry").cloned().unwrap_or(json!({
        "enabled": true,
        "transport": "sse",
        "detail": "standard",
        "frame": {"enabled": false}
    }));
    refuse_auto_transport(&telemetry)?;
    let slot = require_stream_slot(&body)?;

    let client = crate::http::http_client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(timeout_s))
        .build()?;
    let recovery_client = crate::http::http_client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(crate::limits::VISUALS_IPC_ROLL_TIMEOUT)
        .build()?;

    let seed = seed_from_task_instance(&task_instance_id)?;
    // A1: open the family visual before prepare so the pane exists before any
    // paid call. After prepare, rebind slot `stream` to the declared SSE URL
    // (never guess `/events`) and wait for `stream.subscribed` before start.
    let visual_id = match body
        .get("visualId")
        .or_else(|| body.get("visual_id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| {
            container
                .metadata
                .get("liveVisualId")
                .or_else(|| container.metadata.get("live_visual_id"))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
        }) {
        Some(id) => id,
        _ => {
            let opened = open_visual(
                core,
                json!({
                    "containerId": container_id,
                    "title": format!("Live eval {task_instance_id}"),
                }),
            )
            .await?;
            opened
                .pointer("/visual/id")
                .and_then(Value::as_str)
                .context("open_visual omitted visual.id")?
                .to_string()
        }
    };

    // C1-08 / TS-E01: prepare → bind declared SSE → poll until stream.subscribed → start.
    let rollout_id = body
        .get("rollout_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("roll_{}", Uuid::new_v4().simple()));
    let prepare_body = json!({ "rollout_id": rollout_id, "telemetry": telemetry });
    let mut prepare_response = client
        .post(format!("{base}/rollouts/prepare"))
        .json(&prepare_body)
        .send()
        .await;
    if prepare_response.is_err() {
        prepare_response = client
            .post(format!("{base}/rollouts/prepare"))
            .json(&prepare_body)
            .send()
            .await;
    }
    let prepared = prepare_response
        .context("idempotent POST /rollouts/prepare")?
        .error_for_status()
        .context("POST /rollouts/prepare")?
        .json::<Value>()
        .await?;
    let returned_rollout_id = prepared
        .get("rollout_id")
        .and_then(Value::as_str)
        .context("prepare omitted rollout_id")?
        .to_string();
    if returned_rollout_id != rollout_id {
        bail!("prepare returned a different rollout_id than the caller-stable id");
    }
    let prepared_stream = declared_stream_descriptor(&prepared)?
        .context("prepare omitted stream descriptor; refusing to guess /events")?;
    let poll_url = resolve_declared_url(&base, &declared_poll_url(&prepared_stream)?)?;
    let sse_url = resolve_declared_url(&base, &declared_sse_url(&prepared_stream)?)?;
    assert_declared_stream_source(&sse_url)?;
    update_visual(
        core,
        &visual_id,
        json!({
            "bindings": live_sse_bindings(&sse_url),
            "metadata": {
                "containerId": container_id,
                "rolloutId": rollout_id,
                "streamState": "bound_before_start",
                "streamId": prepared_stream.get("id"),
            }
        }),
    )
    .await?;
    wait_for_stream_subscribed(&client, &poll_url, SUBSCRIBE_READY_TIMEOUT).await?;

    let mut start_body = json!({
        "rollout_id": rollout_id,
        "task_instance_id": task_instance_id,
        "seed": seed,
        "telemetry": telemetry,
        "slot": slot,
        "policy_ref": require_caller_policy_ref(&body)?,
    });
    if let Some(world_ref) = body
        .get("worldRef")
        .or_else(|| body.get("world_ref"))
        .cloned()
    {
        start_body["world_ref"] = world_ref;
    }
    if let Some(environment_ref) = body
        .get("environmentRef")
        .or_else(|| body.get("environment_ref"))
        .cloned()
    {
        start_body["environment_ref"] = environment_ref;
    }
    if let Some(task_world) = body
        .get("taskWorld")
        .or_else(|| body.get("task_world"))
        .cloned()
    {
        start_body["task_world"] = task_world;
    }
    let started = std::time::Instant::now();
    let (state, recovered) = visuals_ipc::start_rollout_idempotently(
        &client,
        &recovery_client,
        &base,
        &rollout_id,
        &start_body,
    )
    .await
    .context("POST /rollouts after stream.subscribed")?;
    let stream = declared_stream_descriptor(&state)?.or(Some(prepared_stream));
    refuse_host_side_policy_loop(&state)?;

    let event_log = client
        .get(&poll_url)
        .query(&[("after", "0")])
        .send()
        .await?
        .error_for_status()
        .context("GET declared transports.poll.url after container-owned policy")?
        .json::<Value>()
        .await?;
    let events = envelopes_from_policy_log(&event_log);
    let actions_taken = harvest_actions(&events);
    let usage_total = harvest_usage(state.get("usage"), &events);
    let calls = harvest_policy_calls(&events, state.get("usage"));
    let spool = crate::storage::persist_live_envelopes(
        core.content(),
        stream
            .as_ref()
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str),
        Some(&rollout_id),
        crate::storage::envelopes_from_event_log(&event_log),
    )?;

    let scored = client
        .post(format!("{base}/reward"))
        .json(&json!({ "rollout_id": rollout_id }))
        .send()
        .await
        .ok()
        .filter(|response| response.status().is_success());
    let reward_body = match scored {
        Some(response) => response.json::<Value>().await.unwrap_or(Value::Null),
        None => Value::Null,
    };
    let reward = match reward_body.get("reward") {
        Some(value) if !value.is_null() => value.clone(),
        _ => match state.get("reward") {
            Some(value) if !value.is_null() => value.clone(),
            _ => Value::Null,
        },
    };
    let achievements = harvest_achievements(&events);

    core.update_container_last_rollout(container_id.to_string(), rollout_id.clone())
        .await?;

    let env_terminated = state
        .get("terminated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let env_truncated = state
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let policy_model = harvest_policy_model(&events).unwrap_or(model.clone());

    let _ = core
        .append_and_broadcast(EventAppend {
            event_id: None,
            session_id: None,
            run_id: None,
            source: EventSource::System,
            kind: "eval.policy_rollout.completed".into(),
            payload: json!({
                "containerId": container_id,
                "rolloutId": rollout_id,
                "taskInstanceId": task_instance_id,
                "provider": provider,
                "model": policy_model,
                "reasoningEffort": reasoning_effort,
                "reward": reward,
                "actionCount": actions_taken.len(),
                "calls": calls,
                "terminated": env_terminated,
                "truncated": env_truncated,
                "visualId": visual_id,
                "policyAuthority": "container",
            }),
            remote_sequence: None,
            command_id: None,
            created_at: None,
        })
        .await;

    Ok(json!({
        "schemaVersion": "synth.eval-policy-rollout.v1",
        "containerId": container_id,
        "baseUrl": base,
        "rolloutId": rollout_id,
        "taskInstanceId": task_instance_id,
        "seed": seed,
        "provider": provider,
        "model": policy_model,
        "reasoningEffort": reasoning_effort,
        "actions": actions_taken,
        "calls": calls,
        "wallS": started.elapsed().as_secs_f64(),
        "score": reward,
        "reward": reward,
        "achievements": achievements,
        "terminated": env_terminated,
        "truncated": env_truncated,
        "state": state,
        "eventLog": event_log,
        "spoolDigest": spool.digest,
        "traceCorrelation": json!({
            "schemaVersion": "synth.trace-correlation.v1",
            "authority": "container_stream",
            "containerId": container_id,
            "rolloutId": rollout_id,
            "taskInstanceId": task_instance_id,
            "seed": seed,
            "boundRolloutId": rollout_id,
            "visualId": visual_id,
            "actionCount": actions_taken.len(),
            "spoolDigest": spool.digest,
        }),
        "stream": stream,
        "visualId": visual_id,
        "policyAuthority": "container",
        "recovered": recovered,
        "usage": {
            "promptTokens": usage_total.prompt,
            "completionTokens": usage_total.completion,
            "totalTokens": usage_total.total,
            "costUsd": usage_total.cost_usd,
            "calls": calls,
        }
    }))
}

fn refuse_host_side_policy_loop(state: &Value) -> Result<()> {
    if container_owned_policy_completed(state) {
        Ok(())
    } else {
        bail!(
            "policy_rollouts refuse a host-side model loop; container did not complete a policy-owned rollout"
        )
    }
}

fn container_owned_policy_completed(state: &Value) -> bool {
    state.get("terminated").and_then(Value::as_bool) == Some(true)
        || matches!(
            state.get("status").and_then(Value::as_str),
            Some("completed" | "scored" | "failed" | "truncated")
        )
}

fn envelopes_from_policy_log(log: &Value) -> Vec<Value> {
    crate::storage::envelopes_from_event_log(log)
}

fn event_kind(event: &Value) -> &str {
    event
        .get("kind")
        .or_else(|| event.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn event_payload(event: &Value) -> &Value {
    event.get("payload").unwrap_or(event)
}

fn harvest_actions(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| {
            if event_kind(event) != "action" {
                return None;
            }
            event_payload(event)
                .get("action")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

fn harvest_usage(start_usage: Option<&Value>, events: &[Value]) -> UsageAcc {
    let mut acc = UsageAcc::default();
    if let Some(usage) = start_usage.filter(|value| value.is_object()) {
        add_usage(&mut acc, usage);
        if acc.total > 0 || acc.prompt > 0 || acc.completion > 0 || acc.cost_usd.is_some() {
            return acc;
        }
    }
    for event in events {
        if event_kind(event) != "span.policy.data" {
            continue;
        }
        let payload = event_payload(event);
        if payload.get("delta").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        if let Some(usage) = payload.get("usage") {
            add_usage(&mut acc, usage);
        }
    }
    acc
}

fn add_usage(acc: &mut UsageAcc, usage: &Value) {
    let read = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| usage.get(*key).and_then(Value::as_u64))
    };
    if let Some(value) = read(&["prompt_tokens", "promptTokens"]) {
        acc.prompt = acc.prompt.saturating_add(value);
    }
    if let Some(value) = read(&["completion_tokens", "completionTokens"]) {
        acc.completion = acc.completion.saturating_add(value);
    }
    if let Some(value) = read(&["total_tokens", "totalTokens"]) {
        acc.total = acc.total.saturating_add(value);
    }
    if let Some(value) = usage
        .get("cost_usd")
        .or_else(|| usage.get("costUsd"))
        .or_else(|| usage.get("cost"))
        .and_then(Value::as_f64)
    {
        acc.cost_usd = Some(acc.cost_usd.unwrap_or(0.0) + value);
    }
}

fn harvest_policy_calls(events: &[Value], start_usage: Option<&Value>) -> u64 {
    if let Some(calls) = start_usage.and_then(|usage| usage.get("calls").and_then(Value::as_u64)) {
        return calls;
    }
    events
        .iter()
        .filter(|event| event_kind(event) == "span.policy.closed")
        .count() as u64
}

fn harvest_policy_model(events: &[Value]) -> Option<String> {
    events.iter().rev().find_map(|event| {
        if event_kind(event) != "span.policy.data" {
            return None;
        }
        let payload = event_payload(event);
        if payload.get("delta").and_then(Value::as_bool) == Some(true) {
            return None;
        }
        payload
            .get("model")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn harvest_achievements(events: &[Value]) -> Value {
    let mut names = Vec::new();
    for event in events {
        if event_kind(event) != "achievement_unlocked" {
            continue;
        }
        let payload = event_payload(event);
        if let Some(name) = payload
            .get("achievement")
            .or_else(|| payload.pointer("/payload/achievement"))
            .and_then(Value::as_str)
        {
            if !name.is_empty() && !names.iter().any(|existing| existing == name) {
                names.push(name.to_string());
            }
        }
    }
    json!(names)
}

#[derive(Default)]
struct UsageAcc {
    prompt: u64,
    completion: u64,
    total: u64,
    cost_usd: Option<f64>,
}

#[allow(dead_code)]
struct PolicyPlan {
    actions: Vec<String>,
    response_id: String,
    response_model: String,
}

#[allow(dead_code, clippy::too_many_arguments)]
async fn build_trace_correlation(
    client: &reqwest::Client,
    base: &str,
    container_id: &str,
    rollout_id: &str,
    task_instance_id: &str,
    seed: i64,
    provider: &str,
    requested_model: &str,
    model: Option<&str>,
    state: &Value,
    action: Option<&str>,
    step: Option<u64>,
    model_response_id: Option<&str>,
    model_event_id: Option<&str>,
) -> Result<Value> {
    let step = step.context("policy rollout took no action to correlate")?;
    let action = action.context("policy rollout omitted the correlated action")?;
    let model_response_id =
        model_response_id.context("policy rollout omitted the correlated model response id")?;
    let model_event_id =
        model_event_id.context("policy rollout omitted the correlated journal event id")?;
    let model = model.context("policy rollout omitted the provider-returned model identity")?;
    let observation_text = state
        .pointer("/readout/observation_text")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let ascii = state
        .pointer("/readout/ascii")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let (observation_source, observation) = observation_text
        .map(|value| ("state.readout.observation_text", value))
        .or_else(|| ascii.map(|value| ("state.readout.ascii", value)))
        .context("policy rollout omitted a non-empty observation")?;
    let excerpt = observation.chars().take(512).collect::<String>();
    let reward = state
        .get("reward")
        .cloned()
        .context("policy rollout omitted reward evidence")?;
    if !reward.is_number() {
        bail!("policy rollout reward evidence is not numeric");
    }

    // The immutable step route snapshots the current frame on demand even when
    // replay persistence is disabled. Hash the exact bytes referenced by the proof.
    let frame_url = format!("{base}/rollouts/{rollout_id}/frames/{step}.png");
    let frame_bytes = client
        .get(&frame_url)
        .send()
        .await?
        .error_for_status()
        .context("fetch correlated Craftax frame")?
        .bytes()
        .await?;
    if frame_bytes.is_empty() {
        bail!("correlated Craftax frame is empty");
    }
    let frame_sha256 = format!("{:x}", Sha256::digest(&frame_bytes));

    trace_correlation_payload(
        container_id,
        rollout_id,
        task_instance_id,
        seed,
        provider,
        requested_model,
        model,
        step,
        action,
        observation_source,
        &excerpt,
        reward,
        model_event_id,
        model_response_id,
        &frame_url,
        &frame_sha256,
    )
}

#[allow(clippy::too_many_arguments)]
fn trace_correlation_payload(
    container_id: &str,
    rollout_id: &str,
    task_instance_id: &str,
    seed: i64,
    provider: &str,
    requested_model: &str,
    model: &str,
    step: u64,
    action: &str,
    observation_source: &str,
    observation_excerpt: &str,
    reward: Value,
    model_event_id: &str,
    model_response_id: &str,
    frame_url: &str,
    frame_sha256: &str,
) -> Result<Value> {
    let mut correlation = json!({
        "schemaVersion": "synth.trace-correlation.v1",
        "containerId": container_id,
        "rolloutId": rollout_id,
        "seed": seed,
        "taskInstanceId": task_instance_id,
        "observation": {
            "step": step,
            "source": observation_source,
            "excerpt": observation_excerpt,
        },
        "action": {"step": step, "name": action},
        "reward": {"step": step, "value": reward},
        "frame": {"step": step, "url": frame_url, "sha256": frame_sha256},
        "modelEvent": {
            "kind": "eval.policy_model.response",
            "id": model_event_id,
            "providerResponseId": model_response_id,
            "provider": provider,
            "requestedModel": requested_model,
            "model": model,
            "boundRolloutId": rollout_id,
        },
    });
    let trace_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&correlation)?)
    );
    correlation["traceDigest"] = json!(trace_digest);
    Ok(correlation)
}

fn policy_request(wire: &str, model: &str, reasoning_effort: &str, prompt: &str) -> Value {
    let mut request = if wire == "responses" {
        // Luna / Responses rejects chat-only fields like temperature.
        json!({
            "model": model,
            "input": [{
                "role": "user",
                "content": [{"type": "input_text", "text": format!(
                    "Return only JSON with an actions array. No prose.\n{prompt}"
                )}]
            }],
        })
    } else {
        json!({
            "model": model,
            "messages": [
                {"role": "system", "content": "Return only JSON with an actions array. No prose."},
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.2,
        })
    };
    if !reasoning_effort.is_empty() {
        request["reasoning"] = json!({ "effort": reasoning_effort });
        if wire != "responses" {
            request["reasoning_effort"] = json!(reasoning_effort);
        }
    }
    request
}

#[allow(dead_code)]
async fn policy_actions_from_model(
    client: &reqwest::Client,
    target: &PolicyTarget,
    model: &str,
    reasoning_effort: &str,
    readout: &Value,
    usage: &mut UsageAcc,
) -> Result<PolicyPlan> {
    let valid = readout
        .get("valid_actions")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| {
            DEFAULT_POLICY_ACTIONS
                .iter()
                .map(|value| (*value).to_string())
                .collect()
        });
    let prompt = format!(
        "You are playing Craftax. Choose 1 to 8 valid actions that maximize unlocked achievements.\n\
         Return ONLY a JSON object {{\"actions\":[...]}} using names from this allow-list: {}.\n\
         Observation:\n{}",
        valid.join(", "),
        serde_json::to_string(readout).unwrap_or_else(|_| "{}".into())
    );
    let request = policy_request(target.wire, model, reasoning_effort, &prompt);
    let mut builder = client
        .post(&target.chat_url)
        .bearer_auth(&target.api_key)
        .header("X-Title", "Synth Workshop Eval Driver");
    if target.provider == "openrouter" {
        builder = builder.header("HTTP-Referer", "https://synth.dev");
    }
    let response = builder
        .json(&request)
        .send()
        .await?
        .error_for_status()
        .with_context(|| {
            format!(
                "policy model call failed for provider={} url={}",
                target.provider, target.chat_url
            )
        })?
        .json::<Value>()
        .await?;
    let response_id = response
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .with_context(|| {
            format!(
                "{} omitted the model response id required for trace correlation",
                target.provider
            )
        })?
        .to_string();
    let response_model = response
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or(Some(model))
        .filter(|value| !value.trim().is_empty())
        .with_context(|| {
            format!(
                "{} omitted the model identity required for trace correlation",
                target.provider
            )
        })?
        .to_string();
    if let Some(u) = response.get("usage") {
        usage.prompt += u
            .get("prompt_tokens")
            .or_else(|| u.get("input_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        usage.completion += u
            .get("completion_tokens")
            .or_else(|| u.get("output_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        usage.total += u.get("total_tokens").and_then(Value::as_u64).unwrap_or(0);
        if let Some(cost) = u.get("cost").and_then(Value::as_f64).or_else(|| {
            u.get("cost_usd")
                .and_then(Value::as_f64)
                .or_else(|| u.get("total_cost").and_then(Value::as_f64))
        }) {
            usage.cost_usd = Some(usage.cost_usd.unwrap_or(0.0) + cost);
        }
    }
    let content = if target.wire == "responses" {
        response
            .get("output_text")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                response
                    .get("output")
                    .and_then(Value::as_array)
                    .and_then(|items| {
                        for item in items {
                            if let Some(parts) = item.get("content").and_then(Value::as_array) {
                                for part in parts {
                                    if let Some(text) = part
                                        .get("text")
                                        .and_then(Value::as_str)
                                        .or_else(|| part.pointer("/text").and_then(Value::as_str))
                                    {
                                        if !text.trim().is_empty() {
                                            return Some(text.to_string());
                                        }
                                    }
                                }
                            }
                        }
                        None
                    })
            })
            .unwrap_or_default()
    } else {
        response
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    Ok(PolicyPlan {
        actions: extract_actions(&content, &valid),
        response_id,
        response_model,
    })
}

fn extract_actions(text: &str, valid: &[String]) -> Vec<String> {
    let valid_set: std::collections::BTreeSet<_> = valid.iter().cloned().collect();
    let aliases = [
        ("move_left", "left"),
        ("move_right", "right"),
        ("move_up", "up"),
        ("move_down", "down"),
        ("interact", "do"),
    ];
    let mut found = Vec::new();
    if let Some(start) = text.find('{') {
        if let Some(end) = text[start..].rfind('}') {
            if let Ok(value) = serde_json::from_str::<Value>(&text[start..start + end + 1]) {
                if let Some(actions) = value.get("actions").and_then(Value::as_array) {
                    for action in actions {
                        let raw = action.as_str().unwrap_or("").to_ascii_lowercase();
                        let mapped = aliases
                            .iter()
                            .find(|(from, _)| *from == raw)
                            .map(|(_, to)| (*to).to_string())
                            .unwrap_or(raw);
                        if valid_set.contains(&mapped) {
                            found.push(mapped);
                        }
                    }
                }
            }
        }
    }
    if found.is_empty() {
        // Fail closed to a single safe no-op-ish interact when parse fails.
        if let Some(fallback) = valid
            .iter()
            .find(|action| action.as_str() == "do")
            .cloned()
            .or_else(|| valid.first().cloned())
        {
            found.push(fallback);
        }
    }
    found.into_iter().take(8).collect()
}

fn seed_from_task_instance(task_instance_id: &str) -> Result<i64> {
    // craftax:test:2001 → 2001
    let seed = task_instance_id
        .rsplit(':')
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .context("taskInstanceId must end with an integer seed")?;
    Ok(seed)
}

fn require_stream_slot(body: &Value) -> Result<&'static str> {
    let requested = body
        .get("slot")
        .or_else(|| body.get("streamSlot"))
        .or_else(|| body.get("stream_slot"))
        .and_then(Value::as_str)
        .unwrap_or(LIVE_EVAL_SLOT);
    assert_live_eval_slot(requested)?;
    if requested != LIVE_EVAL_SLOT {
        bail!("eval driver visual-attached rollouts bind slot \"{LIVE_EVAL_SLOT}\", not \"{requested}\"");
    }
    Ok(LIVE_EVAL_SLOT)
}

/// Pin 10 Craftax lanes (seeds 0–9) for Containers HTTP. Does not call a paid policy.
fn craftax_ten_lane_request(body: &Value) -> Result<Vec<Value>> {
    let environment_ref = body
        .get("environment_ref")
        .or_else(|| body.get("environmentRef"))
        .and_then(Value::as_str)
        .context("10-lane Craftax pin requires environment_ref")?;
    let policy_ref = require_caller_policy_ref(body)?;
    let task_world = body
        .get("task_world")
        .or_else(|| body.get("taskWorld"))
        .cloned()
        .context("10-lane Craftax pin requires task_world")?;
    let pins = craftax_ten_lane_pins(environment_ref, &policy_ref, &task_world)?;
    if pins.len() != CRAFTAX_TEN_LANE_SEEDS.len() {
        bail!("10-lane Craftax pin must emit seeds 0–9");
    }
    Ok(pins)
}

fn wants_craftax_ten_lane(body: &Value) -> bool {
    body.get("count").and_then(Value::as_u64) == Some(10)
        || body
            .get("seeds")
            .and_then(Value::as_array)
            .is_some_and(|seeds| {
                seeds.iter().filter_map(Value::as_i64).collect::<Vec<_>>()
                    == CRAFTAX_TEN_LANE_SEEDS.to_vec()
            })
}

fn validated_loopback_base(base: &str) -> Result<String> {
    let trimmed = base.trim_end_matches('/');
    let parsed = reqwest::Url::parse(trimmed).context("invalid container base URL")?;
    let local_host = matches!(
        parsed.host_str(),
        Some("127.0.0.1") | Some("localhost") | Some("::1") | Some("[::1]")
    );
    if parsed.scheme() != "http" || !local_host {
        bail!("eval driver rollouts are limited to registered loopback HTTP containers");
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container_stream::{
        poll_has_stream_subscribed, require_caller_policy_ref,
        require_stream_subscribed_before_start,
    };
    use serde_json::json;

    #[test]
    fn protocol_version_is_stable() {
        assert_eq!(PROTOCOL_VERSION, "synth.eval-driver.v1");
    }

    #[test]
    fn laguna_eval_routes_are_explicit_and_method_scoped() {
        assert_eq!(
            laguna_eval_route("POST", "/v1/laguna/ensure"),
            Some(LagunaEvalRoute::Ensure)
        );
        assert_eq!(
            laguna_eval_route("GET", "/v1/laguna/status"),
            Some(LagunaEvalRoute::Status)
        );
        assert_eq!(
            laguna_eval_route("GET", "/v1/laguna/inference"),
            Some(LagunaEvalRoute::Inference)
        );
        assert_eq!(
            laguna_eval_route("POST", "/v1/laguna/model/unload"),
            Some(LagunaEvalRoute::Unload)
        );
        assert_eq!(laguna_eval_route("GET", "/v1/laguna/ensure"), None);
        assert_eq!(laguna_eval_route("POST", "/v1/laguna/status"), None);
        assert_eq!(laguna_eval_route("GET", "/v1/laguna/model/unload"), None);
    }

    fn laguna_status_fixture(phase: &str) -> crate::laguna::LagunaStatus {
        crate::laguna::LagunaStatus {
            phase: phase.into(),
            base_url: Some("http://127.0.0.1:17301".into()),
            backend: Some("mlx_lm".into()),
            loaded_model: None,
            detail: None,
            memory_bytes: None,
            idle_seconds: None,
            idle_unload_after_seconds: None,
            last_used_at: None,
            free_at: None,
            updated_at: 1,
        }
    }

    #[test]
    fn laguna_ensure_classifies_only_stable_prerequisites_as_skips() {
        assert_eq!(
            classify_laguna_ensure(&laguna_status_fixture("ready"), true),
            ("ready", "ready")
        );
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(
                classify_laguna_ensure(&laguna_status_fixture("not_installed"), false),
                ("unmet_prerequisite", "weights_unavailable")
            );
            for phase in ["error", "unavailable", "unloaded"] {
                assert_eq!(
                    classify_laguna_ensure(&laguna_status_fixture(phase), false),
                    ("product_error", "runtime_unavailable")
                );
            }
        } else {
            assert_eq!(
                classify_laguna_ensure(&laguna_status_fixture("error"), false),
                ("unmet_prerequisite", "hardware_unsupported")
            );
        }
    }

    #[test]
    fn provider_binding_is_typed_and_contains_no_endpoint_or_credential() {
        let record = crate::codex::CodexSessionRecord {
            session_id: "eval-1".into(),
            thread_id: "thread-1".into(),
            workspace: "/tmp/workspace".into(),
            model: "openai/gpt-5.6-luna".into(),
            provider_name: "openrouter".into(),
            provider_title: "OpenRouter Responses".into(),
            base_url: "http://127.0.0.1:12345/v1".into(),
            status: "ready".into(),
            title: None,
            title_origin: None,
            presentation_emotion: None,
            presentation_summary: None,
            approval_policy: "never".into(),
            sandbox: "workspace-write".into(),
        };
        let binding = eval_provider_binding(&record);
        assert_eq!(binding.provider, "openrouter");
        assert_eq!(binding.model, "openai/gpt-5.6-luna");
        assert_eq!(binding.endpoint_class, "openrouter-responses");
        assert_eq!(binding.credential_binding, "native-loopback-lease");
        assert!(binding.brokered);
        assert!(!binding.fallback_allowed);
        let value = serde_json::to_value(binding).unwrap();
        assert!(value.get("baseUrl").is_none());
        assert!(value.get("apiKey").is_none());
        assert!(!value.to_string().contains("127.0.0.1"));
    }

    #[test]
    fn extracts_seed_from_task_instance() {
        assert_eq!(seed_from_task_instance("craftax:test:2001").unwrap(), 2001);
        assert!(seed_from_task_instance("bad").is_err());
    }

    #[test]
    fn policy_rollout_forwards_declared_stream_and_does_not_invent_urls() {
        let state = json!({
            "rollout_id": "r1",
            "stream": {
                "id": "stream_r1",
                "transports": { "sse": { "url": "http://127.0.0.1:8098/rollouts/r1/stream" } }
            }
        });
        let stream = declared_stream_descriptor(&state).unwrap().unwrap();
        assert_eq!(stream["id"], "stream_r1");
        assert!(declared_stream_descriptor(&json!({"rollout_id":"r1"}))
            .unwrap()
            .is_none());
        assert!(refuse_auto_transport(&json!({"transport":"auto"})).is_err());
        assert!(refuse_auto_transport(&json!({"transport":"sse"})).is_ok());
        assert!(declared_poll_url(&stream).is_err());
        assert!(declared_poll_url(&json!({"id":"stream_r1"})).is_err());
    }

    #[test]
    fn policy_rollout_binds_declared_sse_not_events_guess() {
        let stream = json!({
            "id": "stream:r1",
            "transports": {
                "poll": { "url": "/rollouts/r1/events" },
                "sse": { "url": "/rollouts/r1/stream" }
            }
        });
        assert_eq!(declared_sse_url(&stream).unwrap(), "/rollouts/r1/stream");
        let absolute =
            resolve_declared_url("http://127.0.0.1:8098", &declared_sse_url(&stream).unwrap())
                .unwrap();
        assert_eq!(absolute, "http://127.0.0.1:8098/rollouts/r1/stream");
        let bindings = live_sse_bindings(&absolute);
        assert_eq!(bindings["slots"][0]["kind"], "live_sse");
        assert_eq!(bindings["slots"][0]["slot"], "stream");
        assert_eq!(
            bindings["slots"][0]["source"],
            "http://127.0.0.1:8098/rollouts/r1/stream"
        );
        assert!(declared_sse_url(&json!({
            "transports": { "poll": { "url": "/rollouts/r1/events" } }
        }))
        .is_err());
    }

    #[test]
    fn policy_rollouts_require_caller_policy_ref() {
        assert!(require_caller_policy_ref(&json!({})).is_err());
        assert!(require_caller_policy_ref(&json!({"policy_ref": {"config": "x"}})).is_err());
        assert!(require_caller_policy_ref(&json!({"policy_ref": {"harness": "react"}})).is_err());
        let pin = require_caller_policy_ref(&json!({
            "policy_ref": {"harness": "react", "config": "caller_config"}
        }))
        .unwrap();
        assert_eq!(pin["harness"], "react");
        assert_eq!(pin["config"], "caller_config");
    }

    #[test]
    fn refuse_host_side_policy_when_container_did_not_finish() {
        assert!(refuse_host_side_policy_loop(&json!({"status": "running"})).is_err());
        assert!(refuse_host_side_policy_loop(&json!({"terminated": true})).is_ok());
        assert!(refuse_host_side_policy_loop(&json!({"status": "completed"})).is_ok());
    }

    #[test]
    fn harvests_actions_and_skips_token_delta_usage() {
        let events = vec![
            json!({"kind": "action", "payload": {"action": "do"}}),
            json!({"kind": "span.policy.data", "payload": {"delta": true, "usage": {"total_tokens": 99}}}),
            json!({"kind": "span.policy.data", "payload": {"model": "from-log", "usage": {"total_tokens": 13}}}),
            json!({"kind": "span.policy.closed"}),
            json!({"kind": "achievement_unlocked", "payload": {"achievement": "wood"}}),
        ];
        assert_eq!(harvest_actions(&events), vec!["do".to_string()]);
        let usage = harvest_usage(None, &events);
        assert_eq!(usage.total, 13);
        assert_eq!(harvest_policy_model(&events).as_deref(), Some("from-log"));
        assert_eq!(harvest_achievements(&events), json!(["wood"]));
        assert_eq!(harvest_policy_calls(&events, None), 1);
    }

    #[test]
    fn declared_poll_url_uses_descriptor_and_never_guesses_events() {
        let stream = json!({
            "id": "stream:r1",
            "transports": { "poll": { "url": "/rollouts/r1/events" } }
        });
        assert_eq!(declared_poll_url(&stream).unwrap(), "/rollouts/r1/events");
        let absolute = resolve_declared_url(
            "http://127.0.0.1:8098",
            &declared_poll_url(&stream).unwrap(),
        )
        .unwrap();
        assert_eq!(absolute, "http://127.0.0.1:8098/rollouts/r1/events");
        assert!(declared_poll_url(&json!({"poll_url": "/events"})).is_err());
    }

    #[test]
    fn poll_has_stream_subscribed_detects_ready_ack() {
        let ready = json!({
            "events": [{
                "kind": "stream.subscribed",
                "control": true,
                "payload": {
                    "ready": true,
                    "stream.id": "stream:r1",
                    "rollout_id": "r1",
                    "next_sequence": 1
                }
            }]
        });
        assert!(poll_has_stream_subscribed(&ready));
        assert!(require_stream_subscribed_before_start(&ready).is_ok());

        let top_level_ready = json!({
            "items": [{ "kind": "stream.subscribed", "ready": true }]
        });
        assert!(poll_has_stream_subscribed(&top_level_ready));
    }

    #[test]
    fn require_stream_subscribed_refuses_start_when_ack_is_missing() {
        let missing = json!({ "events": [{ "kind": "heartbeat" }] });
        assert!(!poll_has_stream_subscribed(&missing));
        let err = require_stream_subscribed_before_start(&missing).unwrap_err();
        assert!(err.to_string().contains("stream.subscribed"));

        let not_ready = json!({
            "events": [{
                "kind": "stream.subscribed",
                "payload": { "ready": false }
            }]
        });
        assert!(require_stream_subscribed_before_start(&not_ready).is_err());
        assert!(require_stream_subscribed_before_start(&json!({})).is_err());
    }

    #[test]
    fn visual_attached_rollout_slot_must_be_stream() {
        assert_eq!(require_stream_slot(&json!({})).unwrap(), "stream");
        assert_eq!(
            require_stream_slot(&json!({"slot":"stream"})).unwrap(),
            "stream"
        );
        assert!(require_stream_slot(&json!({"slot":"live"})).is_err());
        assert!(require_stream_slot(&json!({"slot":"jobs"})).is_err());
    }

    #[test]
    fn craftax_ten_lane_request_pins_seeds_zero_through_nine() {
        assert!(wants_craftax_ten_lane(&json!({"count": 10})));
        assert!(wants_craftax_ten_lane(&json!({
            "seeds": [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
        })));
        assert!(!wants_craftax_ten_lane(&json!({"count": 1})));
        let pins = craftax_ten_lane_request(&json!({
            "environment_ref": "env:craftax_gold",
            "policy_ref": {"harness": "react", "config": "luna_med"},
            "task_world": {"world_id": "craftax_default", "revision": "symbolic_survival"}
        }))
        .unwrap();
        assert_eq!(pins.len(), 10);
        assert_eq!(pins[0]["seed"], 0);
        assert_eq!(pins[9]["seed"], 9);
        assert_eq!(pins[3]["task_instance_id"], "seed:3");
        assert_eq!(pins[3]["environment_ref"], "env:craftax_gold");
        assert_eq!(pins[3]["policy_ref"]["config"], "luna_med");
        assert_eq!(pins[3]["task_world"]["seed"], 3);
        assert_eq!(pins[3]["slot"], "stream");
        assert!(craftax_ten_lane_request(&json!({
            "policy_ref": {"harness": "react", "config": "luna_med"},
            "task_world": {"world_id": "craftax_default"}
        }))
        .is_err());
    }

    #[test]
    fn extracts_actions_from_model_json() {
        let valid = vec!["do".into(), "left".into(), "up".into()];
        assert_eq!(
            extract_actions(r#"{"actions":["left","do"]}"#, &valid),
            vec!["left".to_string(), "do".to_string()]
        );
        assert_eq!(extract_actions("not json", &valid), vec!["do".to_string()]);
    }

    #[test]
    fn production_gate_requires_debug_or_named_instance_env() {
        // In unit tests we are always under debug_assertions.
        assert!(cfg!(debug_assertions));
        // Without a named instance or opt-in env, should_spawn is false.
        std::env::remove_var("SYNTH_DESKTOP_INSTANCE");
        std::env::remove_var("SYNTH_DESKTOP_EVAL_DRIVER");
        assert!(!should_spawn());
        std::env::set_var("SYNTH_DESKTOP_EVAL_DRIVER", "1");
        assert!(should_spawn());
        std::env::remove_var("SYNTH_DESKTOP_EVAL_DRIVER");
    }

    #[test]
    fn loopback_validation_rejects_remote() {
        assert!(validated_loopback_base("http://127.0.0.1:8098").is_ok());
        assert!(validated_loopback_base("http://example.com:8098").is_err());
        assert!(validated_loopback_base("https://127.0.0.1:8098").is_err());
    }

    #[test]
    fn wait_terminal_recognizes_codex_run_status_events() {
        for status in ["completed", "failed", "cancelled", "interrupted"] {
            assert!(is_terminal_event(
                "run.status_changed",
                &json!({"from": "running", "to": status})
            ));
        }
        assert!(!is_terminal_event(
            "run.status_changed",
            &json!({"from": "created", "to": "running"})
        ));
        assert!(is_terminal_event("run.completed", &json!({})));
    }

    #[test]
    fn trace_correlation_payload_binds_every_evidence_kind() {
        let value = trace_correlation_payload(
            "container-1",
            "rollout-1",
            "craftax:test:2001",
            2001,
            "openrouter",
            "openai/gpt-5.6-luna",
            "openai/gpt-5.6-luna",
            7,
            "left",
            "state.readout.observation_text",
            "player at 1,2",
            json!(0.25),
            "event-1",
            "generation-1",
            "http://127.0.0.1:8098/rollouts/rollout-1/frames/7.png",
            &"a".repeat(64),
        )
        .unwrap();

        assert_eq!(value["schemaVersion"], "synth.trace-correlation.v1");
        assert_eq!(value["rolloutId"], "rollout-1");
        assert_eq!(value["observation"]["step"], 7);
        assert_eq!(value["action"]["step"], 7);
        assert_eq!(value["reward"]["step"], 7);
        assert_eq!(value["frame"]["step"], 7);
        assert_eq!(value["modelEvent"]["provider"], "openrouter");
        assert_eq!(value["modelEvent"]["boundRolloutId"], "rollout-1");
        assert!(value["traceDigest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
    }

    #[test]
    fn policy_chat_urls_normalize_per_provider() {
        assert_eq!(
            policy_chat_url("openrouter", "https://openrouter.ai/api/v1"),
            (
                "https://openrouter.ai/api/v1/chat/completions".into(),
                "chat"
            )
        );
        assert_eq!(
            policy_chat_url("synth-cloud", "https://api.synth.dev"),
            ("https://api.synth.dev/api/v1/responses".into(), "responses")
        );
        assert_eq!(
            policy_chat_url("local-laguna", "http://127.0.0.1:17301"),
            ("http://127.0.0.1:17301/v1/chat/completions".into(), "chat")
        );
    }

    #[test]
    fn responses_policy_request_excludes_chat_only_fields() {
        let request = policy_request("responses", "openai/gpt-5.6-luna", "low", "observe");
        assert_eq!(request["model"], "openai/gpt-5.6-luna");
        assert_eq!(request["reasoning"]["effort"], "low");
        assert_eq!(request["input"].as_array().map(Vec::len), Some(1));
        assert!(request.get("temperature").is_none());
        assert!(request.get("messages").is_none());
        assert!(request.get("reasoning_effort").is_none());
    }

    #[test]
    fn policy_chat_url_for_synth_cloud_uses_the_responses_gateway() {
        // Mirrors what `resolve_policy_target`'s SynthCloud branch composes:
        // the composer/eval "policy" path must redirect to the profile's
        // source-owned Responses gateway just like Codex's own session-start
        // path does, while the resolved backend URL (what account/billing
        // reads) stays untouched.
        let resolved = synth_config::ResolvedBackend {
            config_path: std::path::PathBuf::from("/tmp/config.toml"),
            env_file: std::path::PathBuf::from("/tmp/.env"),
            backend_url: "https://mcp.usesynth.ai".into(),
            responses_gateway_url: Some(
                "https://synth-responses-gateway-prod-production.up.railway.app".into(),
            ),
            api_key: Some("sk_dev".into()),
            worker_key: None,
        };

        let default_gateway = synth_config::require_responses_gateway_url(&resolved)
            .expect("a profile gateway is configured");
        let (gateway_url, wire) = policy_chat_url("synth-cloud", &default_gateway);
        assert_eq!(
            gateway_url,
            "https://synth-responses-gateway-prod-production.up.railway.app/api/v1/responses"
        );
        assert_eq!(wire, "responses");
        // Gateway routing must never mutate the resolved backend URL that
        // account/billing calls read directly.
        assert_eq!(resolved.backend_url, "https://mcp.usesynth.ai");
    }

    #[test]
    fn resolve_policy_target_gateway_fails_closed_without_a_configured_gateway() {
        // An unknown profile must fail closed rather than have the policy
        // path silently fall back to `resolved.backend_url`.
        let resolved = synth_config::ResolvedBackend {
            config_path: std::path::PathBuf::from("/tmp/config.toml"),
            env_file: std::path::PathBuf::from("/tmp/.env"),
            backend_url: "https://api.usesynth.ai".into(),
            responses_gateway_url: None,
            api_key: Some("sk_dev".into()),
            worker_key: None,
        };
        let error = synth_config::require_responses_gateway_url(&resolved).unwrap_err();
        assert!(error.to_lowercase().contains("gateway"));
    }

    #[test]
    fn policy_provider_class_rejects_direct_providers() {
        assert!(matches!(
            codex::provider_class(Some("openai")),
            codex::ProviderClass::Direct
        ));
        assert!(matches!(
            codex::provider_class(Some("synth-cloud")),
            codex::ProviderClass::SynthCloud
        ));
        assert!(matches!(
            codex::provider_class(Some("local-laguna")),
            codex::ProviderClass::LocalLaguna
        ));
        assert!(matches!(
            codex::provider_class(Some("openrouter")),
            codex::ProviderClass::OpenRouter
        ));
    }
}
