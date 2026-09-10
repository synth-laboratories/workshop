//! Sidecar-owned `synth-mlx-rl` child: probe, start, dispatch HTTP.
//!
//! `OptimizerService` never calls this. Training recipes talk to the sidecar
//! proxy; the proxy is the only Workshop code allowed to dial MLX loopback.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::process::{Child, Command};
use tokio::time::sleep;
use uuid::Uuid;

const MLX_DEFAULT_URL: &str = "http://127.0.0.1:8787";
pub(super) const TRAINING_MODEL_ID: &str = "Qwen/Qwen3.5-2B";
// First launch loads the managed Qwen weights before FastAPI finishes startup.
// Keep the probe bounded, but allow realistic Apple Silicon cold-start time.
const HEALTH_TRIES: u32 = 480;
const HEALTH_WAIT: Duration = Duration::from_millis(250);
const MLX_RUNTIME_VERSION: &str = "0.6.0";
const MLX_RUNTIME_SOURCE_REVISION: &str = "5d6db14330babcff170d2afbb8535de2138385a9";
const MLX_RUNTIME_LOCK_SHA256: &str =
    "7f14b704ba9a6c30e6ced5cc88fc2ba6a58a936a9531cfaf168cbb664f83c420";
pub const LOCAL_TRAINING_MAX_SEQ_LENGTH: u64 = 1024;
const MLX_RUNTIME_SCHEMA: &str = "synth.mlx-runtime-wheelhouse.v1";
const MLX_RUNTIME_EVENT: &str = "training://mlx-runtime-install";
const MLX_PROCESS_LEASE_SCHEMA: &str = "synth.mlx-runtime-process-lease.v1";

static SUPERVISOR: OnceLock<Mutex<MlxSupervisor>> = OnceLock::new();
static INSTALL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// MLX has no loaded adapter for this `policy_snapshot_id` (404/410).
/// Classified once at the HTTP boundary so inference can load-and-retry
/// without substring-matching the sidecar prose.
#[derive(Debug)]
pub(crate) struct PolicySnapshotMissing;

impl std::fmt::Display for PolicySnapshotMissing {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MLX policy snapshot is not loaded")
    }
}

impl std::error::Error for PolicySnapshotMissing {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeWheel {
    file_name: String,
    sha256: String,
    size_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeWheelhouse {
    schema_version: String,
    package: String,
    version: String,
    source_revision: String,
    lock_sha256: String,
    artifacts: Vec<RuntimeWheel>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MlxProcessLease {
    schema_version: String,
    pid: u32,
    executable: String,
    artifact_root: String,
}

/// Registers the lazily-started MLX runtime with Workshop's application-wide
/// shutdown fence and reconciles a child inherited from an unclean restart.
pub struct MlxRuntimeService;

impl MlxRuntimeService {
    pub fn new() -> Self {
        if let Err(error) = reconcile_process_lease() {
            crate::platform::logging::report(
                "optimizers",
                "eprintln",
                format!("synth-desktop: failed to reconcile MLX runtime lease: {error:#}"),
            );
        }
        Self
    }
}

impl crate::services::ManagedService for MlxRuntimeService {
    fn name(&self) -> &'static str {
        "mlx-training-runtime"
    }

    fn stop(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move { stop_child().await })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MlxRuntimeStatus {
    pub installed: bool,
    pub executable: Option<String>,
    pub version: &'static str,
    pub install_hint: &'static str,
}

/// Read-only preflight used by the Training setup UI. A responsive model
/// catalog does not imply that the separate MLX training executable exists.
pub fn runtime_status() -> MlxRuntimeStatus {
    let executable = resolve_mlx_bin().ok().filter(|path| path.is_file());
    MlxRuntimeStatus {
        installed: executable.is_some(),
        executable: executable.map(|path| path.to_string_lossy().into_owned()),
        version: MLX_RUNTIME_VERSION,
        install_hint: "Install the Synth MLX training runtime, then check again. Training will not start until the runtime is available.",
    }
}

#[tauri::command]
#[specta::specta]
pub fn training_mlx_runtime_status() -> MlxRuntimeStatus {
    runtime_status()
}

#[tauri::command]
#[specta::specta]
pub async fn training_mlx_runtime_install(
    app: AppHandle,
    confirm: bool,
) -> std::result::Result<MlxRuntimeStatus, String> {
    if !confirm {
        return Err("MLX runtime installation requires confirmation".into());
    }
    let progress = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = INSTALL_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|_| anyhow::anyhow!("MLX runtime install lock is poisoned"))?;
        emit_install(
            &progress,
            "verifying",
            "Verifying signed MLX runtime wheelhouse…",
        );
        let result = install_managed_runtime(&progress);
        match &result {
            Ok(_) => emit_install(&progress, "ready", "MLX training runtime installed."),
            Err(error) => emit_install(&progress, "error", &error.to_string()),
        }
        result
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;
    Ok(runtime_status())
}

fn emit_install(app: &AppHandle, phase: &str, detail: &str) {
    let _ = app.emit(
        MLX_RUNTIME_EVENT,
        json!({ "phase": phase, "detail": detail, "version": MLX_RUNTIME_VERSION }),
    );
}

fn managed_runtime_root() -> PathBuf {
    crate::instance::state_root()
        .join("runtime/mlx-rl/versions")
        .join(MLX_RUNTIME_VERSION)
}

fn process_lease_path() -> PathBuf {
    crate::instance::data_root().join("runtime/mlx-rl/process-lease.json")
}

fn write_process_lease(pid: u32, executable: &Path, artifact_root: &Path) -> Result<()> {
    let path = process_lease_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lease = MlxProcessLease {
        schema_version: MLX_PROCESS_LEASE_SCHEMA.into(),
        pid,
        executable: executable.to_string_lossy().into_owned(),
        artifact_root: artifact_root.to_string_lossy().into_owned(),
    };
    let temporary = path.with_extension(format!("json.tmp-{}", Uuid::new_v4()));
    fs::write(&temporary, serde_json::to_vec_pretty(&lease)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn lease_owns_process(lease: &MlxProcessLease) -> bool {
    if lease.schema_version != MLX_PROCESS_LEASE_SCHEMA || lease.pid == 0 {
        return false;
    }
    let output = StdCommand::new("ps")
        .args(["-p", &lease.pid.to_string(), "-o", "command="])
        .output();
    let Ok(output) = output else { return false };
    if !output.status.success() {
        return false;
    }
    let command = String::from_utf8_lossy(&output.stdout);
    command.contains(&lease.executable)
        && command.contains(" serve ")
        && command.contains(&lease.artifact_root)
}

fn reconcile_process_lease() -> Result<()> {
    let path = process_lease_path();
    let lease = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<MlxProcessLease>(&bytes)
            .context("decode MLX runtime process lease")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if lease_owns_process(&lease) {
        // The lease is instance-scoped and the exact executable + artifact root
        // were revalidated above, so this cannot target another Workshop.
        unsafe { libc::kill(lease.pid as i32, libc::SIGTERM) };
    }
    fs::remove_file(path).context("remove reconciled MLX runtime process lease")?;
    Ok(())
}

async fn stop_child() -> Result<()> {
    let child = supervisor().lock().ok().and_then(|mut guard| {
        guard.base_url = None;
        guard.child.take()
    });
    if let Some(mut child) = child {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    match fs::remove_file(process_lease_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn embedded_distribution_root() -> Result<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("SYNTH_MLX_RL_DISTRIBUTION") {
        let path = PathBuf::from(path);
        if path.join("manifest.json").is_file() {
            return Ok(path);
        }
        bail!("SYNTH_MLX_RL_DISTRIBUTION has no manifest.json");
    }
    let executable = std::env::current_exe().context("resolve Workshop executable")?;
    let resources = executable
        .parent()
        .and_then(Path::parent)
        .map(|contents| contents.join("Resources/runtimes/mlx-rl"))
        .ok_or_else(|| anyhow::anyhow!("resolve Workshop Resources directory"))?;
    if !resources.join("manifest.json").is_file() {
        bail!("This Workshop build does not include the MLX training runtime distribution");
    }
    Ok(resources)
}

fn read_verified_wheelhouse(root: &Path) -> Result<RuntimeWheelhouse> {
    let manifest: RuntimeWheelhouse = serde_json::from_slice(
        &fs::read(root.join("manifest.json")).context("read MLX runtime manifest")?,
    )
    .context("decode MLX runtime manifest")?;
    if manifest.schema_version != MLX_RUNTIME_SCHEMA
        || manifest.package != "synth-mlx-rl"
        || manifest.version != MLX_RUNTIME_VERSION
        || manifest.source_revision != MLX_RUNTIME_SOURCE_REVISION
        || manifest.lock_sha256 != MLX_RUNTIME_LOCK_SHA256
    {
        bail!("MLX runtime manifest does not match the pinned catalog");
    }
    if manifest.artifacts.is_empty() {
        bail!("MLX runtime wheelhouse is empty");
    }
    let mut primary = false;
    for artifact in &manifest.artifacts {
        if artifact.file_name.contains('/') || artifact.file_name.contains('\\') {
            bail!("MLX runtime manifest contains an unsafe wheel name");
        }
        primary |= artifact
            .file_name
            .starts_with(&format!("synth_mlx_rl-{MLX_RUNTIME_VERSION}-"));
        let bytes = fs::read(root.join("wheels").join(&artifact.file_name))
            .with_context(|| format!("read MLX runtime wheel {}", artifact.file_name))?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        if bytes.len() as u64 != artifact.size_bytes || digest != artifact.sha256 {
            bail!(
                "MLX runtime wheel `{}` failed digest verification",
                artifact.file_name
            );
        }
    }
    if !primary {
        bail!("MLX runtime wheelhouse omits synth-mlx-rl=={MLX_RUNTIME_VERSION}");
    }
    Ok(manifest)
}

fn install_managed_runtime(app: &AppHandle) -> Result<()> {
    let source = embedded_distribution_root()?;
    let _manifest = read_verified_wheelhouse(&source)?;
    let destination = managed_runtime_root();
    if managed_runtime_is_valid(&destination) {
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow::anyhow!("resolve MLX runtime versions directory"))?;
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(
        ".{}.staging-{}",
        MLX_RUNTIME_VERSION,
        Uuid::new_v4()
    ));
    fs::create_dir_all(&staging)?;
    let outcome = (|| {
        copy_tree(&source, &staging)?;
        read_verified_wheelhouse(&staging)?;
        emit_install(
            app,
            "installing",
            "Installing the verified wheelhouse offline…",
        );
        let uv = super::manager::resolve_uv()?;
        let runtime = staging.join("runtime");
        let status = StdCommand::new(&uv)
            .args(["venv", "--clear"])
            .arg(&runtime)
            .stdin(Stdio::null())
            .status()
            .context("create MLX runtime environment")?;
        if !status.success() {
            bail!("failed to create the MLX runtime environment");
        }
        let python = runtime.join("bin/python");
        let install = StdCommand::new(&uv)
            .args(["pip", "install", "--offline", "--no-index", "--find-links"])
            .arg(staging.join("wheels"))
            .arg("--python")
            .arg(&python)
            .arg(format!("synth-mlx-rl[mlx]=={MLX_RUNTIME_VERSION}"))
            .stdin(Stdio::null())
            .status()
            .context("install the MLX runtime offline")?;
        if !install.success() {
            bail!("failed to install the verified MLX runtime wheelhouse");
        }
        prove_managed_runtime(&staging)?;
        if destination.exists() {
            let retained = parent.join(format!(
                ".{}.invalid-{}",
                MLX_RUNTIME_VERSION,
                Uuid::new_v4()
            ));
            fs::rename(&destination, retained).context("retain invalid MLX runtime")?;
        }
        fs::rename(&staging, &destination).context("activate verified MLX runtime")?;
        rewrite_runtime_prefix(&destination, &staging, &destination)?;
        prove_managed_runtime(&destination)?;
        Ok(())
    })();
    if outcome.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    outcome
}

fn rewrite_runtime_prefix(root: &Path, old: &Path, new: &Path) -> Result<()> {
    let old = old.to_string_lossy();
    let new = new.to_string_lossy();
    for entry in fs::read_dir(root.join("runtime/bin"))? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let bytes = fs::read(&path)?;
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if text.contains(old.as_ref()) {
            fs::write(&path, text.replace(old.as_ref(), new.as_ref()))?;
        }
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&target)?;
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn prove_managed_runtime(root: &Path) -> Result<()> {
    read_verified_wheelhouse(root)?;
    let python = root.join("runtime/bin/python");
    let output = StdCommand::new(python)
        .args([
            "-c",
            "import importlib.metadata; print(importlib.metadata.version('synth-mlx-rl'))",
        ])
        .output()
        .context("prove installed MLX runtime version")?;
    let executable = root.join("runtime/bin/synth-mlx-rl");
    let runnable = executable.is_file()
        && StdCommand::new(&executable)
            .arg("--help")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
    if !output.status.success()
        || String::from_utf8_lossy(&output.stdout).trim() != MLX_RUNTIME_VERSION
        || !runnable
    {
        bail!("installed MLX runtime failed its offline version proof");
    }
    Ok(())
}

fn managed_runtime_is_valid(root: &Path) -> bool {
    prove_managed_runtime(root).is_ok()
}

struct MlxSupervisor {
    base_url: Option<String>,
    child: Option<Child>,
}

#[derive(Clone)]
pub struct MlxLoopback {
    pub base_url: String,
    http: reqwest::Client,
}

impl MlxLoopback {
    fn client(base_url: String) -> Result<Self> {
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()?,
        })
    }

    pub fn from_env() -> Result<Self> {
        let url = supervisor()
            .lock()
            .ok()
            .and_then(|guard| guard.base_url.clone())
            .unwrap_or_else(configured_mlx_url);
        Self::client(url)
    }

    /// Attach only to an operator-pinned service. Normal product operation
    /// always starts the verified, instance-scoped runtime on a free port;
    /// probing a conventional default port could capture another instance.
    pub async fn ensure() -> Result<Self> {
        if let Some(configured) = configured_mlx_url_override() {
            if let Some(url) = probe_url(&configured).await {
                remember_url(&url);
                return Self::client(url);
            }
            bail!("SYNTH_MLX_RL_URL is set but synth-mlx-rl is not reachable");
        }
        start_child().await
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        decode_mlx(
            self.http
                .get(format!("{}{path}", self.base_url))
                .send()
                .await?,
            path,
        )
        .await
    }

    pub async fn post(&self, path: &str, body: Option<&Value>) -> Result<Value> {
        self.post_timed(path, body, Duration::from_secs(10)).await
    }

    async fn post_timed(
        &self,
        path: &str,
        body: Option<&Value>,
        timeout: Duration,
    ) -> Result<Value> {
        let mut request = self
            .http
            .post(format!("{}{path}", self.base_url))
            .timeout(timeout);
        if let Some(value) = body {
            request = request.json(value);
        }
        decode_mlx(request.send().await?, path).await
    }

    pub async fn openai_family(&self, family: &str, body: &Value) -> Result<Value> {
        let path = match family {
            "chat_completions" | "chat" => "/v1/chat/completions",
            "responses" => "/v1/responses",
            other => bail!("unsupported inference family {other}"),
        };
        self.post(path, Some(body)).await
    }

    pub async fn openai_family_raw(&self, family: &str, body: &Value) -> Result<(String, Vec<u8>)> {
        let path = match family {
            "chat_completions" | "chat" => "/v1/chat/completions",
            "responses" => "/v1/responses",
            other => bail!("unsupported inference family {other}"),
        };
        let response = self
            .http
            .post(format!("{}{path}", self.base_url))
            .json(body)
            .send()
            .await?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/json")
            .to_string();
        let bytes = response.bytes().await.context("read MLX body")?.to_vec();
        if !status.is_success() {
            return Err(mlx_http_error(
                path,
                status,
                &String::from_utf8_lossy(&bytes),
            ));
        }
        Ok((content_type, bytes))
    }

    pub async fn openai_family_stream(
        &self,
        family: &str,
        body: &Value,
        mut on_block: impl FnMut(&str) + Send,
    ) -> Result<(String, Vec<u8>)> {
        let path = match family {
            "chat_completions" | "chat" => "/v1/chat/completions",
            "responses" => "/v1/responses",
            other => bail!("unsupported inference family {other}"),
        };
        let mut response = self
            .http
            .post(format!("{}{path}", self.base_url))
            .json(body)
            .send()
            .await?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/json")
            .to_string();
        if !status.is_success() {
            let bytes = response.bytes().await.context("read MLX body")?.to_vec();
            return Err(mlx_http_error(
                path,
                status,
                &String::from_utf8_lossy(&bytes),
            ));
        }
        if !content_type.contains("event-stream") {
            let bytes = response.bytes().await.context("read MLX body")?.to_vec();
            return Ok((content_type, bytes));
        }
        let mut acc = Vec::new();
        let mut buf = String::new();
        while let Some(chunk) = response.chunk().await.context("read MLX stream")? {
            acc.extend_from_slice(&chunk);
            buf.push_str(&String::from_utf8_lossy(&chunk).replace('\r', ""));
            while let Some(idx) = buf.find("\n\n") {
                let block = buf[..idx].to_string();
                buf.drain(..idx + 2);
                if !block.trim().is_empty() {
                    on_block(&block);
                }
            }
        }
        if !buf.trim().is_empty() {
            on_block(&buf);
        }
        Ok((content_type, acc))
    }

    pub async fn load_adapter(&self, name: &str) -> Result<Value> {
        self.post("/v1/checkpoints/load", Some(&json!({ "name": name })))
            .await
    }

    pub async fn chat(&self, message: &str, policy_snapshot_id: &str) -> Result<String> {
        let snapshot_id = policy_snapshot_id.trim();
        if snapshot_id.is_empty() {
            bail!("inference requires a policy_snapshot_id; ambient latest is refused");
        }
        let response = self
            .post_timed(
                "/v1/chat/completions",
                Some(&json!({
                    "messages": [{"role": "user", "content": message}],
                    "policy_snapshot_id": snapshot_id,
                })),
                Duration::from_secs(120),
            )
            .await?;
        if let Some(error) = response.pointer("/error/message").and_then(Value::as_str) {
            bail!("MLX refused the pinned adapter: {error}");
        }
        response
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                anyhow::anyhow!("MLX chat completion omitted choices[0].message.content")
            })
    }

    pub async fn register_policy(
        &self,
        policy_dir: &Path,
        snapshot_id: &str,
        artifact_digest: Option<&str>,
    ) -> Result<String> {
        let mut body = json!({
            "policy_dir": policy_dir,
            "snapshot_id": snapshot_id,
        });
        if let Some(digest) = artifact_digest {
            body["artifact_digest"] = json!(digest);
        }
        let response = self
            .post_timed(
                "/v1/synth/policies/register",
                Some(&body),
                Duration::from_secs(120),
            )
            .await?;
        response
            .get("policy_snapshot_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("MLX policy registration omitted policy_snapshot_id"))
    }
}

fn nonempty_mlx_url(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn configured_mlx_url_override() -> Option<String> {
    nonempty_mlx_url(std::env::var("SYNTH_MLX_RL_URL").ok())
}

pub fn configured_mlx_url() -> String {
    configured_mlx_url_override().unwrap_or_else(|| MLX_DEFAULT_URL.into())
}

pub fn mlx_serve_command(port: u16, root: &Path) -> Result<Command> {
    let model_path = require_training_model()?;
    mlx_serve_command_with_model(port, root, &model_path)
}

fn mlx_serve_command_with_model(port: u16, root: &Path, model_path: &Path) -> Result<Command> {
    let mut command = Command::new(mlx_bin()?);
    command
        .arg("serve")
        .args(["--host", "127.0.0.1"])
        .args(["--port", &port.to_string()])
        .args(["--root", &root.display().to_string()])
        .args(["--model", &model_path.display().to_string()])
        // The managed 2B runtime is a local-training service, not a 4K chat
        // server. Keeping its resident render contract at 1024 makes the
        // preflight's memory estimate honest on supported Apple Silicon while
        // leaving ample room for classification and text-trajectory recipes.
        .args([
            "--max-seq-length",
            &LOCAL_TRAINING_MAX_SEQ_LENGTH.to_string(),
        ]);
    command.env("SYNTH_MLX_RL_MODEL_PATH", model_path);
    command.env("HF_HUB_OFFLINE", "1");
    command.kill_on_drop(true);
    Ok(command)
}

pub fn training_model_path() -> PathBuf {
    crate::training_models::training_models_root().join(TRAINING_MODEL_ID)
}

pub fn require_training_model() -> Result<PathBuf> {
    let path = training_model_path();
    if !path.is_dir() {
        bail!(
            "download the training model in Settings → Models → On-device training ({TRAINING_MODEL_ID})"
        );
    }
    Ok(path)
}

fn mlx_bin() -> Result<PathBuf> {
    resolve_mlx_bin()
}

fn resolve_mlx_bin() -> Result<PathBuf> {
    #[cfg(debug_assertions)]
    if let Ok(raw) = std::env::var("SYNTH_MLX_RL_BIN") {
        let path = PathBuf::from(raw.trim());
        if path.as_os_str().is_empty() {
            bail!("SYNTH_MLX_RL_BIN is empty");
        }
        if path.is_file() {
            return Ok(path);
        }
        bail!("SYNTH_MLX_RL_BIN {} does not exist", path.display());
    }
    #[cfg(debug_assertions)]
    if let Ok(root) = std::env::var("SYNTH_MLX_RL_ROOT") {
        for candidate in [
            PathBuf::from(root.trim()).join(".venv/bin/synth-mlx-rl"),
            PathBuf::from(root.trim()).join(".venv/Scripts/synth-mlx-rl.exe"),
        ] {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        bail!(
            "SYNTH_MLX_RL_ROOT {} has no prepared .venv/bin/synth-mlx-rl",
            root
        );
    }
    let managed = managed_runtime_root().join("runtime/bin/synth-mlx-rl");
    if managed.is_file() && managed_runtime_is_valid(&managed_runtime_root()) {
        return Ok(managed);
    }
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("synth-mlx-rl"))
            .find(|candidate| candidate.is_file())
    }) {
        return Ok(path);
    }
    bail!("Synth MLX training runtime is not installed")
}

fn supervisor() -> &'static Mutex<MlxSupervisor> {
    SUPERVISOR.get_or_init(|| {
        Mutex::new(MlxSupervisor {
            base_url: None,
            child: None,
        })
    })
}

fn remember_url(url: &str) {
    if let Ok(mut guard) = supervisor().lock() {
        guard.base_url = Some(url.to_string());
    }
}

async fn start_child() -> Result<MlxLoopback> {
    {
        let cached = supervisor()
            .lock()
            .ok()
            .and_then(|guard| guard.base_url.clone());
        if let Some(url) = cached {
            if probe_url(&url).await.is_some() {
                return MlxLoopback::client(url);
            }
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("bind a loopback port for synth-mlx-rl")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let root = crate::instance::data_root().join("optimizers/mlx-rl");
    std::fs::create_dir_all(&root).context("create synth-mlx-rl artifact root")?;
    reconcile_process_lease()?;
    let executable = mlx_bin()?;
    let mut command = mlx_serve_command(port, &root)?;
    let child = command
        .spawn()
        .context("start synth-mlx-rl as an Optimizers sidecar child")?;
    let pid = child
        .id()
        .ok_or_else(|| anyhow::anyhow!("started synth-mlx-rl without a process id"))?;
    if let Err(error) = write_process_lease(pid, &executable, &root) {
        let mut child = child;
        let _ = child.kill().await;
        let _ = child.wait().await;
        return Err(error).context("persist synth-mlx-rl process lease");
    }
    let url = format!("http://127.0.0.1:{port}");
    if let Ok(mut guard) = supervisor().lock() {
        guard.child = Some(child);
        guard.base_url = Some(url.clone());
    }
    for _ in 0..HEALTH_TRIES {
        if probe_url(&url).await.is_some() {
            return MlxLoopback::client(url);
        }
        sleep(HEALTH_WAIT).await;
    }
    bail!("sidecar started synth-mlx-rl at {url} but /v1/capabilities never answered");
}

async fn probe_url(raw: &str) -> Option<String> {
    let url = match reqwest::Url::parse(raw.trim()) {
        Ok(url) => url,
        Err(_) => return None,
    };
    let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if url.scheme() != "http" || !local {
        return None;
    }
    let base = url.as_str().trim_end_matches('/').to_string();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    let response = client
        .get(format!("{base}/v1/capabilities"))
        .send()
        .await
        .ok()?;
    response.status().is_success().then_some(base)
}

async fn decode_mlx(response: reqwest::Response, operation: &str) -> Result<Value> {
    let status = response.status();
    let text = response.text().await.context("read MLX response")?;
    if !status.is_success() {
        return Err(mlx_http_error(operation, status, &text));
    }
    serde_json::from_str(&text).with_context(|| format!("decode MLX response for {operation}"))
}

fn mlx_http_error(operation: &str, status: reqwest::StatusCode, body: &str) -> anyhow::Error {
    let message = format!(
        "MLX service {operation} failed with {status}: {}",
        body.trim()
    );
    if policy_snapshot_missing_from_body(body) {
        anyhow::Error::new(PolicySnapshotMissing).context(message)
    } else {
        anyhow::anyhow!(message)
    }
}

fn policy_snapshot_missing_from_body(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    let code = value
        .pointer("/detail/error_code")
        .or_else(|| value.get("error_code"))
        .and_then(Value::as_str)
        .unwrap_or("");
    matches!(
        code,
        "policy_snapshot_not_found" | "policy_snapshot_evicted"
    )
}

