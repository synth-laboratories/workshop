use crate::laguna::LagunaManager;
use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tauri::Emitter;
use tokio::sync::{watch, Mutex, RwLock};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRequest {
    pub path: String,
    pub method: Option<String>,
    pub body: Option<Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSubscribeRequest {
    pub subscription_id: String,
    pub session_id: String,
    pub after_sequence: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RuntimeConnection {
    url: String,
    token: Option<String>,
}

pub struct RuntimeManager {
    connection: RwLock<Option<RuntimeConnection>>,
    subscriptions: Mutex<HashMap<String, watch::Sender<bool>>>,
    laguna: Arc<LagunaManager>,
    client: Client,
}

impl RuntimeManager {
    pub fn new(laguna: Arc<LagunaManager>) -> Self {
        Self {
            connection: RwLock::new(None),
            subscriptions: Mutex::new(HashMap::new()),
            laguna,
            client: Client::new(),
        }
    }

    async fn connection(&self) -> Result<RuntimeConnection> {
        if let Some(value) = self.connection.read().await.clone() {
            return Ok(value);
        }
        let root = workshop_root()?;
        let laguna_url = self
            .laguna
            .ensure(&root)
            .await?
            .or_else(|| env::var("SYNTH_LAGUNA_BASE_URL").ok());
        if let Ok(url) = env::var("SYNTH_RUNTIME_URL") {
            let connection = RuntimeConnection {
                url: trim_url(url),
                token: env::var("SYNTH_RUNTIME_TOKEN").ok(),
            };
            if !self.healthy(&connection).await {
                return Err(anyhow!(
                    "SYNTH_RUNTIME_URL is not healthy: {}",
                    connection.url
                ));
            }
            *self.connection.write().await = Some(connection.clone());
            return Ok(connection);
        }
        if let Some(existing) = read_connection() {
            if self.healthy(&existing).await
                && (self.local_mode(&existing).await.as_deref() == Some("mlx")
                    || laguna_url.is_none())
            {
                *self.connection.write().await = Some(existing.clone());
                return Ok(existing);
            }
            let _ = fs::remove_file(connection_path());
        }
        spawn_runtime(&root, laguna_url.as_deref(), self.laguna.api_key())?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        while tokio::time::Instant::now() < deadline {
            if let Some(candidate) = read_connection() {
                if self.healthy(&candidate).await {
                    *self.connection.write().await = Some(candidate.clone());
                    return Ok(candidate);
                }
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
        Err(anyhow!(
            "The local runtime did not start. See {}",
            runtime_home().join("runtime.log").display()
        ))
    }

    async fn healthy(&self, connection: &RuntimeConnection) -> bool {
        let mut request = self
            .client
            .get(format!("{}/v1/health", connection.url))
            .timeout(Duration::from_millis(1500));
        if let Some(token) = &connection.token {
            request = request.bearer_auth(token);
        }
        let Ok(response) = request.send().await else {
            return false;
        };
        if !response.status().is_success() {
            return false;
        }
        response
            .json::<Value>()
            .await
            .ok()
            .and_then(|value| {
                value
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .map(|value| value == "synth.desktop-runtime.v1")
            })
            .unwrap_or(false)
    }

    async fn local_mode(&self, connection: &RuntimeConnection) -> Option<String> {
        let mut request = self
            .client
            .get(format!("{}/v1/health", connection.url))
            .timeout(Duration::from_millis(1500));
        if let Some(token) = &connection.token {
            request = request.bearer_auth(token);
        }
        request
            .send()
            .await
            .ok()?
            .json::<Value>()
            .await
            .ok()?
            .pointer("/local/mode")?
            .as_str()
            .map(str::to_owned)
    }

    pub async fn request(&self, request: RuntimeRequest) -> Result<Value> {
        validate_path(&request.path)?;
        let method = request
            .method
            .as_deref()
            .unwrap_or("GET")
            .parse::<Method>()?;
        if !matches!(method, Method::GET | Method::POST | Method::DELETE) {
            return Err(anyhow!("Unsupported runtime method: {method}"));
        }
        let connection = self.connection().await?;
        let mut outgoing = self
            .client
            .request(method, format!("{}{}", connection.url, request.path))
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(60));
        if let Some(token) = connection.token {
            outgoing = outgoing.bearer_auth(token);
        }
        if let Some(body) = request.body {
            outgoing = outgoing.json(&body);
        }
        let response = outgoing.send().await?;
        let status = response.status();
        let text = response.text().await?;
        let payload = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or_else(|_| json!({"raw": text}))
        };
        if !status.is_success() {
            let message = payload
                .pointer("/error/message")
                .and_then(Value::as_str)
                .or_else(|| payload.get("detail").and_then(Value::as_str))
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Runtime request failed ({})", status.as_u16()));
            return Err(anyhow!(message));
        }
        Ok(payload)
    }

    pub async fn subscribe(
        &self,
        app: tauri::AppHandle,
        request: RuntimeSubscribeRequest,
    ) -> Result<Value> {
        let connection = self.connection().await?;
        self.unsubscribe(&request.subscription_id).await;
        let (cancel, receiver) = watch::channel(false);
        self.subscriptions
            .lock()
            .await
            .insert(request.subscription_id.clone(), cancel);
        let client = self.client.clone();
        let id = request.subscription_id.clone();
        tauri::async_runtime::spawn(async move {
            stream(client, app, connection, request, receiver).await;
        });
        Ok(json!({"subscriptionId": id}))
    }

    pub async fn unsubscribe(&self, id: &str) {
        if let Some(sender) = self.subscriptions.lock().await.remove(id) {
            let _ = sender.send(true);
        }
    }
}

async fn stream(
    client: Client,
    app: tauri::AppHandle,
    connection: RuntimeConnection,
    request: RuntimeSubscribeRequest,
    mut cancel: watch::Receiver<bool>,
) {
    let mut cursor = request.after_sequence.unwrap_or(0);
    let mut retry = 500u64;
    while !*cancel.borrow() {
        emit_status(&app, &request.subscription_id, "connecting", None);
        let url = format!(
            "{}/v1/sessions/{}/events/stream?after_sequence={cursor}",
            connection.url,
            percent_segment(&request.session_id)
        );
        let mut outgoing = client.get(url).header("Accept", "text/event-stream");
        if let Some(token) = &connection.token {
            outgoing = outgoing.bearer_auth(token);
        }
        match outgoing.send().await {
            Ok(response) if response.status().is_success() => {
                emit_status(&app, &request.subscription_id, "connected", None);
                retry = 500;
                let mut bytes = response.bytes_stream();
                let mut buffer = String::new();
                loop {
                    tokio::select! {
                        _ = cancel.changed() => break,
                        chunk = bytes.next() => match chunk {
                            Some(Ok(value)) => {
                                buffer.push_str(&String::from_utf8_lossy(&value).replace("\r\n", "\n"));
                                while let Some(boundary) = buffer.find("\n\n") {
                                    let frame = buffer[..boundary].to_owned(); buffer.drain(..boundary + 2);
                                    if let Some((id, data)) = parse_sse(&frame) {
                                        if let Ok(event) = serde_json::from_str::<Value>(&data) {
                                            cursor = event.get("sequence").and_then(Value::as_u64).or(id.and_then(|v| v.parse().ok())).unwrap_or(cursor).max(cursor);
                                            let _ = app.emit("runtime:subscription", json!({"subscriptionId": request.subscription_id, "type": "event", "event": event}));
                                        }
                                    }
                                }
                            },
                            _ => break,
                        }
                    }
                }
            }
            Ok(response) => emit_status(
                &app,
                &request.subscription_id,
                "reconnecting",
                Some(format!("Event stream failed ({})", response.status())),
            ),
            Err(error) => emit_status(
                &app,
                &request.subscription_id,
                "reconnecting",
                Some(error.to_string()),
            ),
        }
        if *cancel.borrow() {
            break;
        }
        tokio::select! { _ = tokio::time::sleep(Duration::from_millis(retry)) => {}, _ = cancel.changed() => break }
        retry = (retry * 2).min(5000);
    }
}

fn emit_status(app: &tauri::AppHandle, id: &str, state: &str, detail: Option<String>) {
    let _ = app.emit("runtime:subscription", json!({"subscriptionId": id, "type": "status", "status": {"state": state, "detail": detail}}));
}
fn parse_sse(frame: &str) -> Option<(Option<String>, String)> {
    let mut id = None;
    let mut data = Vec::new();
    for line in frame.lines() {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = line
            .split_once(':')
            .map(|(a, b)| (a, b.strip_prefix(' ').unwrap_or(b)))
            .unwrap_or((line, ""));
        if field == "id" {
            id = Some(value.into());
        } else if field == "data" {
            data.push(value);
        }
    }
    (!data.is_empty()).then(|| (id, data.join("\n")))
}
fn percent_segment(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn validate_path(path: &str) -> Result<()> {
    if !path.starts_with("/v1/")
        || path.starts_with("//")
        || path.contains('\\')
        || path.contains("..")
    {
        Err(anyhow!(
            "Only safe versioned local runtime paths are allowed"
        ))
    } else {
        Ok(())
    }
}
fn runtime_home() -> PathBuf {
    env::var_os("SYNTH_RUNTIME_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".synth-desktop/runtime")
        })
}
fn connection_path() -> PathBuf {
    runtime_home().join("connection.json")
}
fn read_connection() -> Option<RuntimeConnection> {
    serde_json::from_str(&fs::read_to_string(connection_path()).ok()?).ok()
}
fn trim_url(value: String) -> String {
    value.trim_end_matches('/').to_owned()
}

pub(crate) fn workshop_root() -> Result<PathBuf> {
    if let Some(root) = env::var_os("SYNTH_WORKSHOP_ROOT") {
        let root = PathBuf::from(root);
        return validate_runtime_root(root, "SYNTH_WORKSHOP_ROOT");
    }

    // Packaged Tauri applications put mapped resources in the platform resource
    // directory. Copy the Python/visual assets to application data so visual
    // generation never attempts to mutate a signed/read-only app bundle.
    let executable = env::current_exe().context("resolve Synth Desktop executable path")?;
    for candidate in resource_candidates(&executable) {
        if is_runtime_root(&candidate) {
            return materialize_packaged_root(&candidate);
        }
    }

    // Development fallback: src-tauri -> synth_desktop -> apps -> workshop.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_owned)
        .filter(|p| p.join("services/local-runtime").exists())
        .ok_or_else(|| anyhow!("Synth Desktop runtime resources are missing. Reinstall the app or set SYNTH_WORKSHOP_ROOT to a checkout containing services/local-runtime/src, services/laguna-daemon/laguna_daemon, and visuals."))
}

fn is_runtime_root(root: &Path) -> bool {
    root.join("services/local-runtime/src/synth_local_runtime")
        .is_dir()
        && root.join("services/laguna-daemon/laguna_daemon").is_dir()
        && root.join("visuals").is_dir()
}

fn validate_runtime_root(root: PathBuf, source: &str) -> Result<PathBuf> {
    if is_runtime_root(&root) {
        Ok(root)
    } else {
        Err(anyhow!(
            "{source}={} does not contain the required Synth runtime resources",
            root.display()
        ))
    }
}

fn resource_candidates(executable: &Path) -> Vec<PathBuf> {
    let executable_dir = executable.parent().unwrap_or(Path::new("."));
    let mut candidates = vec![
        executable_dir.to_owned(),
        executable_dir.join("resources"),
        executable_dir.join("Resources"),
    ];
    if let Some(parent) = executable_dir.parent() {
        candidates.push(parent.join("Resources")); // macOS App.app/Contents/Resources
        candidates.push(parent.join("resources"));
    }
    candidates
}

fn materialize_packaged_root(source: &Path) -> Result<PathBuf> {
    let destination = runtime_home().join("bundled-runtime");
    for relative in [
        "services/local-runtime/src",
        "services/laguna-daemon/laguna_daemon",
        "visuals",
    ] {
        copy_tree(&source.join(relative), &destination.join(relative))?;
    }
    validate_runtime_root(destination, "packaged resource cache")
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("create resource directory {}", destination.display()))?;
    for entry in fs::read_dir(source)
        .with_context(|| format!("read bundled resource directory {}", source.display()))?
    {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).with_context(|| {
                format!("copy bundled runtime resource to {}", target.display())
            })?;
        }
    }
    Ok(())
}

fn spawn_runtime(root: &Path, laguna_url: Option<&str>, api_key: Option<String>) -> Result<()> {
    let home = runtime_home();
    fs::create_dir_all(home.join("data"))?;
    let _ = fs::remove_file(connection_path());
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("runtime.log"))?;
    let source = root.join("services/local-runtime/src");
    let token = Uuid::new_v4().to_string();
    let python = PathBuf::from(env::var("SYNTH_PYTHON").unwrap_or_else(|_| "python3".into()));
    validate_python(&python, "local runtime")?;
    let mut command = Command::new(&python);
    command
        .args([
            "-m",
            "synth_local_runtime",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--data-dir",
        ])
        .arg(home.join("data"))
        .arg("--connection-file")
        .arg(connection_path())
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    command
        .env(
            "PYTHONPATH",
            env::var("PYTHONPATH")
                .map(|v| format!("{}:{v}", source.display()))
                .unwrap_or_else(|_| source.display().to_string()),
        )
        .env("SYNTH_RUNTIME_TOKEN", token)
        .env("SYNTH_WORKSHOP_ROOT", root)
        .env(
            "SYNTH_VISUALS_ROOT",
            env::var_os("SYNTH_VISUALS_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("visuals")),
        );
    if let Some(url) = laguna_url {
        command.env("SYNTH_LAGUNA_BASE_URL", url);
    }
    if let Some(key) = api_key {
        command.env("SYNTH_LAGUNA_API_KEY", key);
    }
    detach(&mut command);
    command.spawn().context("spawn local runtime")?;
    Ok(())
}

fn validate_python(python: &Path, purpose: &str) -> Result<()> {
    Command::new(python)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("Python is required for the {purpose} compatibility service, but `{}` could not be started. Install Python 3 or set SYNTH_PYTHON to a usable interpreter.", python.display()))
        .and_then(|status| if status.success() { Ok(()) } else { Err(anyhow!("Python interpreter `{}` exited unsuccessfully while checking the {purpose} compatibility service", python.display())) })
}
#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            extern "C" {
                fn setsid() -> i32;
            }
            let _ = setsid();
            Ok(())
        });
    }
}
#[cfg(not(unix))]
fn detach(_: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_runtime_paths() {
        assert!(validate_path("/v1/sessions").is_ok());
        assert!(validate_path("https://evil.test/v1/x").is_err());
        assert!(validate_path("/v1/../health").is_err());
    }
    #[test]
    fn parses_multiline_sse() {
        assert_eq!(
            parse_sse("id: 4\nevent: item\ndata: {\"a\":\ndata: 1}"),
            Some((Some("4".into()), "{\"a\":\n1}".into()))
        );
    }
    #[test]
    fn encodes_session_segment() {
        assert_eq!(percent_segment("a/b c"), "a%2Fb%20c");
    }

    #[test]
    fn macos_resource_directory_is_considered() {
        let candidates = resource_candidates(Path::new(
            "/Applications/Synth Desktop.app/Contents/MacOS/synth-desktop",
        ));
        assert!(candidates.contains(&PathBuf::from(
            "/Applications/Synth Desktop.app/Contents/Resources"
        )));
    }
}
