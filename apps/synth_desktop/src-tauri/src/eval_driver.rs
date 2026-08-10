//! Eval loopback IPC — profile-gated programmatic driver for live Workshop.
//!
//! Wire protocol: `synth.eval-driver.v1` (see `EVAL_DRIVER.md` beside this crate).
//!
//! This is product code in the same category as [`crate::visuals_ipc`]: loopback-only
//! bind, bearer token, descriptor JSON in the instance data root. It is compiled into
//! debug/dev builds and only spawned for named development instances. Production /
//! release builds never listen.

use crate::codex::{CodexManager, CodexSessionStartRequest, CodexTurnSendRequest};
use crate::core_runtime::CoreRuntime;
use crate::laguna::LagunaManager;
use crate::storage::{EventAppend, EventSource};
use crate::synth_config;
use crate::visuals::VisualCreateRequest;
use crate::visuals_ipc;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, sync::Arc, time::Duration};
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

pub const PROTOCOL_VERSION: &str = "synth.eval-driver.v1";
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const OPENROUTER_CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const DEFAULT_POLICY_ACTIONS: &[&str] = &["do", "left", "do", "up", "do", "right", "do", "down"];

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
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                continue;
            };
            if !peer.ip().is_loopback() {
                continue;
            }
            let deps = serve.clone();
            let token = serve_token.clone();
            tauri::async_runtime::spawn(async move {
                let _ = handle_connection(stream, deps, token).await;
            });
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

async fn handle_connection(
    mut stream: TcpStream,
    deps: Arc<EvalDriverDeps>,
    token: String,
) -> Result<()> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut header_end = None;
    let mut expected_len = None;
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
        if header_end.is_none() {
            header_end = buffer
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|i| i + 4);
            if header_end.is_none() && buffer.len() > MAX_HEADER_BYTES {
                bail!("eval driver headers exceed limit");
            }
            if let Some(end) = header_end {
                let headers = std::str::from_utf8(&buffer[..end])
                    .context("eval driver headers are not UTF-8")?;
                expected_len = Some(parse_content_length(headers)?);
                if expected_len.unwrap_or(0) > MAX_BODY_BYTES {
                    bail!("eval driver body exceeds limit");
                }
            }
        }
        if let (Some(end), Some(length)) = (header_end, expected_len) {
            if buffer.len() >= end + length {
                buffer.truncate(end + length);
                break;
            }
        }
        if buffer.len() > MAX_HEADER_BYTES + MAX_BODY_BYTES {
            break;
        }
    }
    let (status, reason, body) = match dispatch_http(&buffer, &deps, &token).await {
        Ok(value) => (200, "OK", value),
        Err(error) if error.to_string().contains("unauthorized") => {
            (401, "Unauthorized", json!({"error": error.to_string()}))
        }
        Err(error) if error.to_string().contains("protocol mismatch") => {
            (426, "Upgrade Required", json!({"error": error.to_string()}))
        }
        Err(error) => (400, "Bad Request", json!({"error": error.to_string()})),
    };
    let payload = serde_json::to_vec(&body)?;
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Synth-Eval-Driver: {PROTOCOL_VERSION}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(&payload).await?;
    Ok(())
}

fn parse_content_length(headers: &str) -> Result<usize> {
    for line in headers.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().context("invalid Content-Length");
        }
    }
    Ok(0)
}

async fn dispatch_http(raw: &[u8], deps: &EvalDriverDeps, token: &str) -> Result<Value> {
    let header_end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .context("incomplete eval driver headers")?;
    let headers =
        std::str::from_utf8(&raw[..header_end]).context("eval driver headers are not UTF-8")?;
    let content_length = parse_content_length(headers)?;
    if raw.len() < header_end + content_length {
        bail!("incomplete eval driver body");
    }
    let mut lines = headers.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let target = parts.next().unwrap_or("/");
    let (path, query_string) = target.split_once('?').unwrap_or((target, ""));
    let mut auth = None;
    let mut client_protocol = None;
    for line in lines.by_ref() {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("authorization") {
            auth = value.trim().strip_prefix("Bearer ").map(str::to_string);
        }
        if name.eq_ignore_ascii_case("x-synth-eval-driver") {
            client_protocol = Some(value.trim().to_string());
        }
    }
    if auth.as_deref() != Some(token) {
        bail!("unauthorized eval driver request");
    }
    if let Some(version) = client_protocol {
        if version != PROTOCOL_VERSION {
            bail!("protocol mismatch: expected {PROTOCOL_VERSION}, got {version}");
        }
    }
    let body = &raw[header_end..header_end + content_length];
    let json_body: Value = if body.is_empty() {
        query_json(query_string)
    } else {
        serde_json::from_slice(body).context("invalid eval driver JSON body")?
    };
    dispatch(method, path, json_body, deps).await
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
            export_session(core, &session_id).await
        }
        ("POST", "/v1/export_session") => {
            let session_id = body
                .get("sessionId")
                .and_then(|value| value.as_str().map(str::to_string))
                .context("export_session requires sessionId")?;
            export_session(core, &session_id).await
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
            run_policy_rollout(core, id, body).await
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
        ("POST", path) if path.starts_with("/v1/visuals/") && path.ends_with("/show") => {
            visuals_ipc::dispatch(method, path, body, core).await
        }
        _ => bail!("unsupported eval driver route {method} {path}"),
    }
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
        thread_id: None,
        multi_agent_version: None,
        auto_compact_token_limit: body.get("autoCompactTokenLimit").and_then(Value::as_u64),
        writable_roots: Vec::new(),
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

async fn prepare_start(
    laguna: &LagunaManager,
    mut request: CodexSessionStartRequest,
) -> Result<CodexSessionStartRequest> {
    if request.provider_name.as_deref() == Some("local-laguna") {
        let root = crate::runtime::workshop_root()?;
        request.base_url = laguna
            .ensure(&root)
            .await?
            .ok_or_else(|| anyhow!("Laguna Responses server is unavailable"))?;
        request.api_key = laguna.api_key().unwrap_or_default();
    } else if request
        .provider_name
        .as_deref()
        .is_some_and(|provider| provider.eq_ignore_ascii_case("openrouter"))
    {
        request.api_key = synth_config::openrouter_api_key()?
            .ok_or_else(|| anyhow!("OpenRouter API key is not configured"))?;
        request.provider_env_key = Some("OPENROUTER_API_KEY".into());
    } else if request
        .provider_name
        .as_deref()
        .is_some_and(|provider| provider.eq_ignore_ascii_case("synth-cloud"))
    {
        let resolved = synth_config::resolve()?;
        crate::codex::apply_synth_cloud_provider(
            &mut request,
            &resolved.backend_url,
            resolved.api_key.as_deref(),
        )
        .map_err(|message| anyhow!(message))?;
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
        thread_id: None,
        multi_agent_version: None,
        auto_compact_token_limit: body.get("autoCompactTokenLimit").and_then(Value::as_u64),
        writable_roots: Vec::new(),
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
            if matches!(
                kind,
                "run.completed"
                    | "run.failed"
                    | "run.cancelled"
                    | "session.run.completed"
                    | "session.run.failed"
            ) {
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

async fn export_session(core: &CoreRuntime, session_id: &str) -> Result<Value> {
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
    Ok(json!({
        "schemaVersion": "synth.eval-session-export.v1",
        "sessionId": session_id,
        "session": session,
        "events": events,
        "eventCount": events.len(),
        "sourceRevision": crate::instance::diagnostics().source_revision,
    }))
}

async fn open_visual(core: &CoreRuntime, body: Value) -> Result<Value> {
    let template_id = body
        .get("templateId")
        .and_then(Value::as_str)
        .unwrap_or("live.container_rollouts.v1");
    let session_id = body
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let title = body
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Live container eval")
        .to_string();
    let bindings = body.get("bindings").cloned().unwrap_or(json!({}));
    let create = VisualCreateRequest {
        template_id: template_id.into(),
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
    core.broadcast_committed(Some(serde_json::from_value(show_event.clone())?));
    Ok(json!({
        "opened": true,
        "visual": shown,
        "createEvent": event,
        "showEvent": show_event,
    }))
}

async fn run_policy_rollout(core: &CoreRuntime, container_id: &str, body: Value) -> Result<Value> {
    let container = core
        .inventory()
        .get_container(container_id.to_string())
        .await?;
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
    if provider != "openrouter" {
        bail!("policy_rollouts currently support provider=openrouter only");
    }
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .context("policy_rollouts require model")?
        .to_string();
    let reasoning_effort = body
        .get("reasoningEffort")
        .or_else(|| body.get("reasoning_effort"))
        .and_then(Value::as_str)
        .unwrap_or("low")
        .to_string();
    let max_steps = body
        .get("maxSteps")
        .or_else(|| body.get("max_steps"))
        .and_then(Value::as_u64)
        .unwrap_or(64)
        .clamp(1, 256) as usize;
    let max_calls = body
        .get("maxCalls")
        .or_else(|| body.get("max_calls"))
        .and_then(Value::as_u64)
        .unwrap_or(16)
        .clamp(1, 64) as usize;
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

    let api_key = synth_config::openrouter_api_key()?
        .ok_or_else(|| anyhow!("OpenRouter API key is not configured on the Workshop host"))?;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(timeout_s))
        .build()?;

    let seed = seed_from_task_instance(&task_instance_id)?;
    let create_body = json!({
        "seed": seed,
        "telemetry": telemetry,
    });
    let mut state = client
        .post(format!("{base}/rollouts"))
        .json(&create_body)
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    let rollout_id = state
        .get("rollout_id")
        .and_then(Value::as_str)
        .context("container omitted rollout_id")?
        .to_string();
    let stream = state.get("stream").cloned();

    let mut usage_total = UsageAcc::default();
    let mut calls = 0usize;
    let started = std::time::Instant::now();
    let mut actions_taken = Vec::new();
    let mut correlated_model_response_id = None;
    let mut correlated_model_event_id = None;
    let mut correlated_model_name = None;
    let mut correlated_action = None;
    let mut correlated_step = None;

    for _ in 0..max_calls {
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
        if actions_taken.len() >= max_steps {
            break;
        }
        let readout = state.get("readout").cloned().unwrap_or_else(|| json!({}));
        let planned = policy_actions_from_model(
            &client,
            &api_key,
            &model,
            &reasoning_effort,
            &readout,
            &mut usage_total,
        )
        .await?;
        calls += 1;
        let model_event = core
            .append_and_broadcast(EventAppend {
                event_id: None,
                session_id: None,
                // Craftax rollout ids are external identities, not rows in the
                // Workshop `runs` table. Bind them in payload to avoid the FK.
                run_id: None,
                source: EventSource::System,
                kind: "eval.policy_model.response".into(),
                payload: json!({
                    "containerId": container_id,
                    "rolloutId": rollout_id,
                    "taskInstanceId": task_instance_id,
                    "provider": "openrouter",
                    "providerResponseId": &planned.response_id,
                    "model": &planned.response_model,
                    "plannedActions": &planned.actions,
                    "call": calls,
                }),
                remote_sequence: None,
                command_id: None,
                created_at: None,
            })
            .await?;
        for action in planned.actions {
            if actions_taken.len() >= max_steps {
                break;
            }
            state = client
                .post(format!("{base}/rollouts/{rollout_id}/step"))
                .json(&json!({"action": action}))
                .send()
                .await?
                .error_for_status()?
                .json::<Value>()
                .await?;
            let step = state
                .pointer("/progress/env_steps")
                .and_then(Value::as_u64)
                .unwrap_or(actions_taken.len() as u64 + 1);
            correlated_model_response_id = Some(planned.response_id.clone());
            correlated_model_event_id = Some(model_event.event_id.clone());
            correlated_model_name = Some(planned.response_model.clone());
            correlated_action = Some(action.clone());
            correlated_step = Some(step);
            actions_taken.push(action);
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
        }
    }

    let event_log = client
        .get(format!("{base}/rollouts/{rollout_id}/event_log"))
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await
        .unwrap_or(json!({}));

    let trace_correlation = build_trace_correlation(
        &client,
        &base,
        container_id,
        &rollout_id,
        &task_instance_id,
        seed,
        &model,
        correlated_model_name.as_deref(),
        &state,
        correlated_action.as_deref(),
        correlated_step,
        correlated_model_response_id.as_deref(),
        correlated_model_event_id.as_deref(),
    )
    .await?;

    let reward = state.get("reward").cloned().unwrap_or(json!(0.0));
    let achievements = state
        .pointer("/readout/observation/achievements")
        .cloned()
        .or_else(|| state.get("achievements").cloned())
        .unwrap_or_else(|| json!([]));

    core.update_container_last_rollout(container_id.to_string(), rollout_id.clone())
        .await?;

    // Journal a durable lifecycle breadcrumb for cross-check (no secrets).
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
                "model": model,
                "reasoningEffort": reasoning_effort,
                "reward": reward,
                "actionCount": actions_taken.len(),
                "calls": calls,
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
        "model": model,
        "reasoningEffort": reasoning_effort,
        "actions": actions_taken,
        "calls": calls,
        "wallS": started.elapsed().as_secs_f64(),
        "score": reward,
        "reward": reward,
        "achievements": achievements,
        "terminated": state.get("terminated").and_then(Value::as_bool).unwrap_or(false),
        "truncated": state.get("truncated").and_then(Value::as_bool).unwrap_or(false),
        "state": state,
        "eventLog": event_log,
        "traceCorrelation": trace_correlation,
        "stream": stream,
        "usage": {
            "promptTokens": usage_total.prompt,
            "completionTokens": usage_total.completion,
            "totalTokens": usage_total.total,
            "costUsd": usage_total.cost_usd,
            "calls": calls,
        }
    }))
}

#[derive(Default)]
struct UsageAcc {
    prompt: u64,
    completion: u64,
    total: u64,
    cost_usd: f64,
}

struct PolicyPlan {
    actions: Vec<String>,
    response_id: String,
    response_model: String,
}

#[allow(clippy::too_many_arguments)]
async fn build_trace_correlation(
    client: &reqwest::Client,
    base: &str,
    container_id: &str,
    rollout_id: &str,
    task_instance_id: &str,
    seed: i64,
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
            "provider": "openrouter",
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

async fn policy_actions_from_model(
    client: &reqwest::Client,
    api_key: &str,
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
    let mut request = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "Return only JSON with an actions array. No prose."},
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.2,
    });
    if !reasoning_effort.is_empty() {
        request["reasoning"] = json!({"effort": reasoning_effort});
        request["reasoning_effort"] = json!(reasoning_effort);
    }
    let response = client
        .post(OPENROUTER_CHAT_URL)
        .bearer_auth(api_key)
        .header("HTTP-Referer", "https://synth.dev")
        .header("X-Title", "Synth Workshop Eval Driver")
        .json(&request)
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    let response_id = response
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("OpenRouter omitted the model response id required for trace correlation")?
        .to_string();
    let response_model = response
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("OpenRouter omitted the model identity required for trace correlation")?
        .to_string();
    if let Some(u) = response.get("usage") {
        usage.prompt += u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0);
        usage.completion += u
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        usage.total += u.get("total_tokens").and_then(Value::as_u64).unwrap_or(0);
        if let Some(cost) = u.get("cost").and_then(Value::as_f64).or_else(|| {
            u.get("cost_usd")
                .and_then(Value::as_f64)
                .or_else(|| u.get("total_cost").and_then(Value::as_f64))
        }) {
            usage.cost_usd += cost;
        }
    }
    let content = response
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or("");
    Ok(PolicyPlan {
        actions: extract_actions(content, &valid),
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

    #[test]
    fn protocol_version_is_stable() {
        assert_eq!(PROTOCOL_VERSION, "synth.eval-driver.v1");
    }

    #[test]
    fn extracts_seed_from_task_instance() {
        assert_eq!(seed_from_task_instance("craftax:test:2001").unwrap(), 2001);
        assert!(seed_from_task_instance("bad").is_err());
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
    fn trace_correlation_payload_binds_every_evidence_kind() {
        let value = trace_correlation_payload(
            "container-1",
            "rollout-1",
            "craftax:test:2001",
            2001,
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
        assert_eq!(value["modelEvent"]["boundRolloutId"], "rollout-1");
        assert!(value["traceDigest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
    }
}
