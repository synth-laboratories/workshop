//! Authenticated loopback IPC so the visual MCP adapter never opens SQLite.

use crate::core_runtime::CoreRuntime;
use crate::inventory::ContainerRegisterRequest;
use crate::visuals::{VisualCreateRequest, VisualQuery, VisualUpdateRequest};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;

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

pub async fn spawn(core: Arc<CoreRuntime>, root: PathBuf) -> Result<VisualsIpcConnection> {
    let token = format!("synth_vis_{}", Uuid::new_v4());
    let listener = TcpListener::bind("127.0.0.1:0")
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
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let core = serve_core.clone();
            let token = token.clone();
            tauri::async_runtime::spawn(async move {
                let _ = handle_connection(stream, core, token).await;
            });
        }
    });
    Ok(connection)
}

async fn handle_connection(
    mut stream: TcpStream,
    core: Arc<CoreRuntime>,
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
                anyhow::bail!("visuals IPC headers exceed limit");
            }
            if let Some(end) = header_end {
                let headers = std::str::from_utf8(&buffer[..end])
                    .context("visuals IPC headers are not UTF-8")?;
                expected_len = Some(parse_content_length(headers)?);
                if expected_len.unwrap_or(0) > MAX_BODY_BYTES {
                    anyhow::bail!("visuals IPC body exceeds limit");
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
    let (status, reason, body) = match dispatch_http(&buffer, &core, &token).await {
        Ok(value) => (200, "OK", value),
        Err(error) if error.to_string().contains("unauthorized") => {
            (401, "Unauthorized", json!({"error": error.to_string()}))
        }
        Err(error) => (400, "Bad Request", json!({"error": error.to_string()})),
    };
    let payload = serde_json::to_vec(&body)?;
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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

async fn dispatch_http(raw: &[u8], core: &CoreRuntime, token: &str) -> Result<Value> {
    let header_end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .context("incomplete visuals IPC headers")?;
    let headers =
        std::str::from_utf8(&raw[..header_end]).context("visuals IPC headers are not UTF-8")?;
    let content_length = parse_content_length(headers)?;
    if raw.len() < header_end + content_length {
        anyhow::bail!("incomplete visuals IPC body");
    }
    let mut lines = headers.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let target = parts.next().unwrap_or("/");
    let (path, query_string) = target.split_once('?').unwrap_or((target, ""));
    let mut auth = None;
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
    }
    if auth.as_deref() != Some(token) {
        anyhow::bail!("unauthorized visuals IPC request");
    }
    let body = &raw[header_end..header_end + content_length];
    let json_body: Value = if body.is_empty() {
        query_json(query_string)
    } else {
        serde_json::from_slice(body).context("invalid visuals IPC JSON body")?
    };
    dispatch(method, path, json_body, core).await
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

async fn register_hydrated_container(
    core: &CoreRuntime,
    request: ContainerRegisterRequest,
) -> Result<crate::inventory::ContainerDeployment> {
    if !(request.base_url.starts_with("http://") || request.base_url.starts_with("https://")) {
        anyhow::bail!("container baseUrl must start with http:// or https://");
    }
    let base = request.base_url.trim_end_matches('/');
    let client = reqwest::Client::new();
    let health_response = client
        .get(format!("{base}/health"))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await;
    let (status, health) = match health_response {
        Ok(response) => {
            let code = response.status();
            let payload = response.json::<Value>().await.unwrap_or(json!({}));
            (
                if code.is_success() {
                    "ready"
                } else {
                    "unhealthy"
                },
                json!({"ok":code.is_success(),"status":code.as_u16(),"payload":payload}),
            )
        }
        Err(error) => ("unhealthy", json!({"ok":false,"error":error.to_string()})),
    };
    let mut info = None;
    for route in ["info", "metadata"] {
        if let Ok(response) = client
            .get(format!("{base}/{route}"))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
        {
            if response.status().is_success() {
                info = response.json::<Value>().await.ok();
                if info.is_some() {
                    break;
                }
            }
        }
    }
    let family = info
        .as_ref()
        .and_then(|value| value.get("env_family").or_else(|| value.get("task_family")))
        .and_then(Value::as_str)
        .map(str::to_string)
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
    if let Some(value) = info {
        metadata.insert("info".into(), value);
    }
    for (route, key) in [
        ("task_catalog", "taskCatalog"),
        ("task_info", "taskInfo"),
        ("program", "program"),
        ("dataset", "dataset"),
    ] {
        if let Ok(response) = client
            .get(format!("{base}/{route}"))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
        {
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
    match (method, path) {
        ("GET", "/health") => Ok(json!({"ok": true, "service": "synth-visuals-ipc"})),
        ("GET", "/v1/containers") => {
            Ok(json!({"containers": core.inventory().list_containers().await?}))
        }
        ("POST", "/v1/containers") => {
            let request: ContainerRegisterRequest = serde_json::from_value(body)?;
            let container = register_hydrated_container(core, request).await?;
            Ok(json!({"container":container}))
        }
        ("GET", path) if path.starts_with("/v1/containers/") && !path.ends_with("/probe") => {
            let id = path.trim_start_matches("/v1/containers/");
            Ok(json!({"container": core.inventory().get_container(id.to_string()).await?}))
        }
        ("POST", path) if path.starts_with("/v1/containers/") && path.ends_with("/probe") => {
            let id = path
                .trim_start_matches("/v1/containers/")
                .trim_end_matches("/probe")
                .trim_end_matches('/');
            let container = core.inventory().get_container(id.to_string()).await?;
            let base = container
                .base_url
                .as_deref()
                .context("container has no base URL")?
                .trim_end_matches('/');
            let client = reqwest::Client::new();
            let health_response = client
                .get(format!("{base}/health"))
                .timeout(std::time::Duration::from_secs(3))
                .send()
                .await;
            let (status, health) = match health_response {
                Ok(response) => {
                    let code = response.status();
                    let payload = response.json::<Value>().await.unwrap_or(json!({}));
                    (
                        if code.is_success() {
                            "ready"
                        } else {
                            "unhealthy"
                        },
                        json!({"ok":code.is_success(),"status":code.as_u16(),"payload":payload}),
                    )
                }
                Err(error) => ("unhealthy", json!({"ok":false,"error":error.to_string()})),
            };
            let info = match client
                .get(format!("{base}/info"))
                .timeout(std::time::Duration::from_secs(3))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => {
                    response.json::<Value>().await.ok()
                }
                _ => None,
            };
            let mut metadata = container.metadata.as_object().cloned().unwrap_or_default();
            metadata.insert("hydratedAt".into(), json!(chrono::Utc::now().to_rfc3339()));
            if let Some(value) = info.clone() {
                metadata.insert("info".into(), value);
            }
            for (route, key) in [
                ("task_catalog", "taskCatalog"),
                ("task_info", "taskInfo"),
                ("program", "program"),
                ("dataset", "dataset"),
            ] {
                if let Ok(response) = client
                    .get(format!("{base}/{route}"))
                    .timeout(std::time::Duration::from_secs(3))
                    .send()
                    .await
                {
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
        ("GET", path) if path.starts_with("/v1/visuals/") && !path.ends_with("/show") => {
            let id = path.trim_start_matches("/v1/visuals/");
            if id.contains('/') {
                anyhow::bail!("unsupported visuals path");
            }
            Ok(json!({"visual": registry.get(id.to_string()).await?}))
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
        ("POST", path) if path.starts_with("/v1/visuals/") => {
            let id = path.trim_start_matches("/v1/visuals/");
            let request: VisualUpdateRequest = serde_json::from_value(body)?;
            let (visual, event) = registry.update(id.to_string(), request).await?;
            Ok(json!({"visual": visual, "event": event}))
        }
        _ => anyhow::bail!("unsupported visuals IPC route {method} {path}"),
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
    fn parses_case_insensitive_lengths_and_query_values() {
        assert_eq!(
            parse_content_length("GET / HTTP/1.1\r\ncontent-length: 12\r\n\r\n").unwrap(),
            12
        );
        assert_eq!(
            query_json("search=reward+chart&limit=5&offset=2"),
            json!({
                "search": "reward chart", "limit": 5, "offset": 2
            })
        );
    }
}
