use anyhow::{Context, Result};
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashSet,
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{broadcast, Mutex, RwLock};

const DEFAULT_PORT: u16 = 7333;
const DEFAULT_MODEL: &str = "poolside/Laguna-XS-2.1-NVFP4-mlx";
const MODEL_INDEX: &str = "model.safetensors.index.json";
const SELECTED_MODEL_FILE: &str = "selected_model_path";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LagunaModelHit {
    pub path: String,
    pub models_root: String,
    pub model_id: String,
    pub shard_count: usize,
    pub total_bytes: u64,
    pub selected: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LagunaStatus {
    pub phase: String,
    pub base_url: Option<String>,
    pub backend: Option<String>,
    pub loaded_model: Option<String>,
    pub detail: Option<String>,
    pub memory_bytes: Option<u64>,
    pub idle_seconds: Option<u64>,
    pub idle_unload_after_seconds: Option<u64>,
    pub last_used_at: Option<u64>,
    pub free_at: Option<u64>,
    pub updated_at: u64,
}

pub struct LagunaManager {
    status: RwLock<LagunaStatus>,
    ensure_lock: Mutex<()>,
    updates: broadcast::Sender<LagunaStatus>,
    client: Client,
}

impl LagunaManager {
    pub fn new() -> Self {
        let (updates, _) = broadcast::channel(32);
        Self {
            status: RwLock::new(LagunaStatus {
                phase: "unknown".into(),
                base_url: None,
                backend: None,
                loaded_model: None,
                detail: None,
                memory_bytes: None,
                idle_seconds: None,
                idle_unload_after_seconds: None,
                last_used_at: None,
                free_at: None,
                updated_at: now_ms(),
            }),
            ensure_lock: Mutex::new(()),
            updates,
            client: Client::new(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LagunaStatus> {
        self.updates.subscribe()
    }
    pub async fn status(&self) -> LagunaStatus {
        self.status.read().await.clone()
    }

    pub async fn set_status(&self, mut status: LagunaStatus) {
        status.updated_at = now_ms();
        *self.status.write().await = status.clone();
        let _ = self.updates.send(status);
    }

    pub async fn set_error(&self, detail: String) {
        let mut status = self.status().await;
        status.phase = "error".into();
        status.detail = Some(detail);
        self.set_status(status).await;
    }

    pub async fn refresh(&self) -> LagunaStatus {
        let current = self.status().await;
        let Some(base_url) = current.base_url.as_deref() else {
            return current;
        };
        let Some(api_key) = self.api_key() else {
            return current;
        };
        if let Some(status) = self.probe(base_url, &api_key).await {
            self.set_status(status).await;
        }
        self.status().await
    }

    /// Restart the Synth-managed Laguna sidecar, then wait for the freshly
    /// started daemon to report its current residency. External upstreams are
    /// never killed; in that configuration this still performs a fresh probe.
    pub async fn reload(&self, workshop_root: &Path) -> Result<LagunaStatus> {
        self.set_status(LagunaStatus {
            phase: "starting".into(),
            base_url: env::var("SYNTH_LAGUNA_BASE_URL").ok().map(trim_url),
            backend: None,
            loaded_model: None,
            detail: Some("Reloading Laguna XS…".into()),
            memory_bytes: None,
            idle_seconds: None,
            idle_unload_after_seconds: None,
            last_used_at: None,
            free_at: None,
            updated_at: now_ms(),
        })
        .await;
        if stop_managed_sidecar()? {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        self.ensure(workshop_root).await?;
        Ok(self.status().await)
    }

    pub fn api_key(&self) -> Option<String> {
        env::var("SYNTH_LAGUNA_API_KEY").ok()
    }

    pub fn discover_models(&self) -> Result<Vec<LagunaModelHit>> {
        discover_models()
    }

    pub fn select_model(&self, path: &Path) -> Result<LagunaModelHit> {
        let mut hit = validate_model_input(path)?;
        fs::create_dir_all(home())?;
        fs::write(home().join(SELECTED_MODEL_FILE), format!("{}\n", hit.path))?;
        hit.selected = true;
        Ok(hit)
    }

    pub fn clear_selected_model(&self) -> Result<()> {
        match fs::remove_file(home().join(SELECTED_MODEL_FILE)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn ensure(&self, workshop_root: &Path) -> Result<Option<String>> {
        let _ensure_guard = self.ensure_lock.lock().await;
        if env::var("SYNTH_LAGUNA_AUTO_START").as_deref() == Ok("0") {
            let mut status = self.status().await;
            status.phase = "unavailable".into();
            status.detail = Some("Auto-start disabled (SYNTH_LAGUNA_AUTO_START=0)".into());
            self.set_status(status).await;
            return Ok(env::var("SYNTH_LAGUNA_BASE_URL").ok().map(trim_url));
        }

        let api_key = ensure_api_key()?;
        let base_url = trim_url(
            env::var("SYNTH_LAGUNA_BASE_URL")
                .unwrap_or_else(|_| format!("http://127.0.0.1:{DEFAULT_PORT}")),
        );
        self.set_status(LagunaStatus {
            phase: "starting".into(),
            base_url: Some(base_url.clone()),
            backend: None,
            loaded_model: None,
            detail: Some("Checking Laguna XS…".into()),
            memory_bytes: None,
            idle_seconds: None,
            idle_unload_after_seconds: None,
            last_used_at: None,
            free_at: None,
            updated_at: now_ms(),
        })
        .await;

        if let Some(status) = self.probe(&base_url, &api_key).await {
            if matches!(status.phase.as_str(), "ready" | "unloaded") {
                self.set_status(status).await;
                write_env_sh(&api_key, &base_url)?;
                return Ok(Some(base_url));
            }
            if status
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("does not support the Responses API"))
            {
                if stop_managed_sidecar()? {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                } else {
                    self.set_status(status).await;
                    return Ok(None);
                }
            }
        }

        // A selected directory must be loaded by our MLX backend. An already
        // running Poolside upstream cannot be instructed to switch weights.
        let upstream = if read_selected_model_path()?.is_some() {
            None
        } else {
            discover_poolside_upstream(&self.client).await
        };
        let backend = if upstream.is_some() {
            "external"
        } else if cfg!(target_os = "macos") {
            "mlx_lm"
        } else {
            "auto"
        };
        let mut status = self.status().await;
        status.phase = "loading".into();
        status.backend = Some(backend.into());
        status.detail = Some(
            if upstream.is_some() {
                "Connecting to local Laguna engine…"
            } else {
                "Starting Laguna sidecar…"
            }
            .into(),
        );
        self.set_status(status).await;
        write_env_sh(&api_key, &base_url)?;
        spawn_sidecar(workshop_root, &api_key, backend, upstream.as_ref())?;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
        while tokio::time::Instant::now() < deadline {
            if let Some(status) = self.probe(&base_url, &api_key).await {
                let done = status.phase == "ready" || status.phase == "error";
                self.set_status(status.clone()).await;
                if done {
                    return Ok((status.phase == "ready").then_some(base_url));
                }
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        self.set_error(format!("Timed out waiting for Laguna at {base_url}"))
            .await;
        Ok(None)
    }

    async fn probe(&self, base_url: &str, api_key: &str) -> Option<LagunaStatus> {
        let response = self
            .client
            .get(format!("{base_url}/health"))
            .bearer_auth(api_key)
            .timeout(Duration::from_millis(1200))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body: Value = response.json().await.ok()?;
        let observed_at = now_ms();
        let residency = residency_from_health(&body, observed_at);
        if body.get("responsesApi").and_then(Value::as_bool) != Some(true) {
            return Some(LagunaStatus {
                phase: "error".into(),
                base_url: Some(base_url.into()),
                backend: body
                    .get("backend")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                loaded_model: body
                    .get("loadedModel")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                detail: Some(
                    "The process on the Laguna port does not support the Responses API. Stop the stale sidecar and restart Synth Desktop."
                        .into(),
                ),
                memory_bytes: body.get("memoryBytes").and_then(Value::as_u64),
                idle_seconds: residency.idle_seconds,
                idle_unload_after_seconds: residency.idle_unload_after_seconds,
                last_used_at: residency.last_used_at,
                free_at: residency.free_at,
                updated_at: observed_at,
            });
        }
        let raw = body.get("status").and_then(Value::as_str).unwrap_or("");
        let phase = match raw {
            "ok" | "ready" => "ready",
            "loading" => "loading",
            "unloaded" => "unloaded",
            "error" => "error",
            _ => "starting",
        };
        Some(LagunaStatus {
            phase: phase.into(),
            base_url: Some(base_url.into()),
            backend: body
                .get("backend")
                .and_then(Value::as_str)
                .map(str::to_owned),
            loaded_model: body
                .get("loadedModel")
                .and_then(Value::as_str)
                .map(str::to_owned),
            detail: Some(if phase == "ready" {
                "Laguna XS ready".into()
            } else {
                format!("sidecar {phase}")
            }),
            memory_bytes: body.get("memoryBytes").and_then(Value::as_u64),
            idle_seconds: residency.idle_seconds,
            idle_unload_after_seconds: residency.idle_unload_after_seconds,
            last_used_at: residency.last_used_at,
            free_at: residency.free_at,
            updated_at: observed_at,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResidencyTiming {
    idle_seconds: Option<u64>,
    idle_unload_after_seconds: Option<u64>,
    last_used_at: Option<u64>,
    free_at: Option<u64>,
}

fn residency_from_health(body: &Value, observed_at: u64) -> ResidencyTiming {
    let idle_seconds = nonnegative_u64(body.get("idleSeconds"));
    let idle_unload_after_seconds = nonnegative_u64(body.get("idleUnloadAfterSeconds"));
    let last_used_at = nonnegative_u64(body.get("lastUsedAt"))
        .or_else(|| idle_seconds.map(|idle| observed_at.saturating_sub(idle.saturating_mul(1000))));
    let free_at = nonnegative_u64(body.get("freeAt")).or_else(|| {
        match (idle_seconds, idle_unload_after_seconds) {
            (Some(idle), Some(limit)) if limit > 0 => {
                Some(observed_at.saturating_add(limit.saturating_sub(idle).saturating_mul(1000)))
            }
            _ => None,
        }
    });
    ResidencyTiming {
        idle_seconds,
        idle_unload_after_seconds,
        last_used_at,
        free_at,
    }
}

fn nonnegative_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|v| u64::try_from(v).ok()))
    })
}

fn home() -> PathBuf {
    env::var_os("SYNTH_LAGUNA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".synth-desktop/laguna")
        })
}
fn models_dir() -> Result<PathBuf> {
    if let Some(selected) = read_selected_model_path()? {
        return Ok(validate_model_input(&selected)?.models_root.into());
    }
    if let Some(path) = env::var_os("SYNTH_LAGUNA_MODELS_DIR") {
        return Ok(validate_model_input(Path::new(&path))?.models_root.into());
    }
    let poolside = dirs::home_dir()
        .unwrap_or_default()
        .join(".config/poolside/models");
    if poolside.join("poolside/Laguna-XS-2.1-NVFP4-mlx").exists() {
        Ok(poolside)
    } else {
        Ok(dirs::home_dir()
            .unwrap_or_default()
            .join(".synth-desktop/models"))
    }
}

fn read_selected_model_path() -> Result<Option<PathBuf>> {
    match fs::read_to_string(home().join(SELECTED_MODEL_FILE)) {
        Ok(value) if !value.trim().is_empty() => Ok(Some(PathBuf::from(value.trim()))),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn validate_model_input(input: &Path) -> Result<LagunaModelHit> {
    let model_dir = if input.join("config.json").is_file() {
        input.to_owned()
    } else {
        let nested = input.join(DEFAULT_MODEL);
        if nested.join("config.json").is_file() {
            nested
        } else {
            return Err(anyhow::anyhow!("{} is neither a Laguna model directory nor a models root containing {DEFAULT_MODEL}", input.display()));
        }
    };
    validate_model_dir(&model_dir)
}

fn validate_model_dir(model_dir: &Path) -> Result<LagunaModelHit> {
    let config = model_dir.join("config.json");
    if !config.is_file() {
        return Err(anyhow::anyhow!("Missing {}", config.display()));
    }
    let index_path = model_dir.join(MODEL_INDEX);
    let index: Value = serde_json::from_str(
        &fs::read_to_string(&index_path)
            .with_context(|| format!("Missing or unreadable {}", index_path.display()))?,
    )
    .with_context(|| format!("Invalid JSON in {}", index_path.display()))?;
    let weight_map = index
        .get("weight_map")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("{} has no weight_map object", index_path.display()))?;
    let shards: HashSet<&str> = weight_map.values().filter_map(Value::as_str).collect();
    if shards.is_empty() {
        return Err(anyhow::anyhow!(
            "{} references no safetensor shards",
            index_path.display()
        ));
    }
    let mut total_bytes = 0;
    for shard in &shards {
        if !shard.ends_with(".safetensors") || shard.contains('/') || shard.contains('\\') {
            return Err(anyhow::anyhow!(
                "Unsafe shard path `{shard}` in {}",
                index_path.display()
            ));
        }
        let path = model_dir.join(shard);
        total_bytes += fs::metadata(&path)
            .with_context(|| format!("Referenced model shard is missing: {}", path.display()))?
            .len();
    }
    let canonical = model_dir
        .canonicalize()
        .with_context(|| format!("Resolve model directory {}", model_dir.display()))?;
    let suffix_depth = Path::new(DEFAULT_MODEL).components().count();
    let models_root = if canonical.ends_with(DEFAULT_MODEL) {
        canonical
            .ancestors()
            .nth(suffix_depth)
            .unwrap_or(&canonical)
            .to_owned()
    } else {
        // The daemon also accepts a flat model directory as models_dir.
        canonical.clone()
    };
    Ok(LagunaModelHit {
        path: canonical.to_string_lossy().into_owned(),
        models_root: models_root.to_string_lossy().into_owned(),
        model_id: DEFAULT_MODEL.into(),
        shard_count: shards.len(),
        total_bytes,
        selected: false,
    })
}

fn discover_models() -> Result<Vec<LagunaModelHit>> {
    let user = dirs::home_dir().unwrap_or_default();
    let mut candidates = vec![
        user.join(".config/poolside/models").join(DEFAULT_MODEL),
        user.join(".synth-desktop/models").join(DEFAULT_MODEL),
    ];
    if let Ok(repositories) = fs::read_dir(user.join(".cache/huggingface/hub")) {
        for repository in repositories.flatten().filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("models--poolside--Laguna")
        }) {
            if let Ok(snapshots) = fs::read_dir(repository.path().join("snapshots")) {
                candidates.extend(snapshots.flatten().map(|entry| entry.path()));
            }
        }
    }
    if let Ok(extra) = env::var("SYNTH_LAGUNA_MODELS_DIR") {
        candidates.push(PathBuf::from(extra));
    }
    if let Some(selected) = read_selected_model_path()? {
        candidates.push(selected);
    }
    let selected = read_selected_model_path()?.and_then(|path| path.canonicalize().ok());
    let mut seen = HashSet::new();
    let mut hits = Vec::new();
    for candidate in candidates {
        if let Ok(mut hit) = validate_model_input(&candidate) {
            if seen.insert(hit.path.clone()) {
                hit.selected = selected
                    .as_ref()
                    .is_some_and(|path| path == Path::new(&hit.path));
                hits.push(hit);
            }
        }
    }
    hits.sort_by(|left, right| {
        right
            .selected
            .cmp(&left.selected)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(hits)
}
fn trim_url(value: String) -> String {
    value.trim_end_matches('/').to_owned()
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn ensure_api_key() -> Result<String> {
    if let Ok(key) = env::var("SYNTH_LAGUNA_API_KEY") {
        if !key.trim().is_empty() {
            return Ok(key.trim().into());
        }
    }
    fs::create_dir_all(home())?;
    let path = home().join("api_key");
    if let Ok(key) = fs::read_to_string(&path) {
        if !key.trim().is_empty() {
            let key = key.trim().to_owned();
            env::set_var("SYNTH_LAGUNA_API_KEY", &key);
            return Ok(key);
        }
    }
    let mut bytes = [0u8; 24];
    rand::rng().fill_bytes(&mut bytes);
    let key = format!(
        "synth-local-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    write_secret(&path, &key)?;
    env::set_var("SYNTH_LAGUNA_API_KEY", &key);
    Ok(key)
}

fn write_secret(path: &Path, value: &str) -> Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    writeln!(options.open(path)?, "{value}")?;
    Ok(())
}

fn write_env_sh(api_key: &str, base_url: &str) -> Result<()> {
    fs::create_dir_all(home())?;
    let body = format!("export SYNTH_LAGUNA_HOST=\"127.0.0.1\"\nexport SYNTH_LAGUNA_BASE_URL=\"{base_url}\"\nexport SYNTH_LAGUNA_API_KEY=\"{api_key}\"\nexport SYNTH_LAGUNA_BACKEND=\"{}\"\nexport SYNTH_LAGUNA_DEFAULT_MODEL=\"{DEFAULT_MODEL}\"\nexport SYNTH_LAGUNA_MODELS_DIR=\"{}\"\nexport SYNTH_LAGUNA_AUTO_LOAD=\"1\"\nexport PATH=\"$HOME/.synth-desktop/laguna/.venv/bin:$PATH\"\n", env::var("SYNTH_LAGUNA_BACKEND").unwrap_or_else(|_| "auto".into()), models_dir()?.display());
    fs::write(home().join("env.sh"), body)?;
    Ok(())
}

#[derive(Clone)]
struct Upstream {
    url: String,
    api_key: String,
}

async fn discover_poolside_upstream(client: &Client) -> Option<Upstream> {
    let process_key = Command::new("ps")
        .args(["-axo", "command="])
        .output()
        .ok()
        .and_then(|o| discover_key(&String::from_utf8_lossy(&o.stdout)));
    let saved = fs::read_to_string(home().join("poolside_sidecar_api_key"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let api_key = env::var("SYNTH_LAGUNA_UPSTREAM_API_KEY")
        .ok()
        .or_else(|| env::var("SYNTH_LAGUNA_EXTERNAL_API_KEY").ok())
        .or(saved)
        .or(process_key)?;
    let mut candidates = vec![
        "http://127.0.0.1:63300".into(),
        "http://127.0.0.1:49600".into(),
    ];
    if let Ok(url) = env::var("SYNTH_LAGUNA_EXTERNAL_URL") {
        candidates.insert(0, trim_url(url));
    }
    for url in candidates {
        let healthy = client
            .get(format!("{url}/health"))
            .bearer_auth(&api_key)
            .timeout(Duration::from_millis(1200))
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false);
        if healthy {
            let _ = fs::create_dir_all(home());
            let _ = write_secret(&home().join("poolside_sidecar_api_key"), &api_key);
            return Some(Upstream { url, api_key });
        }
    }
    None
}

fn discover_key(output: &str) -> Option<String> {
    for line in output.lines().filter(|line| line.contains("poolside-mlx")) {
        let parts: Vec<_> = line.split_whitespace().collect();
        for (index, part) in parts.iter().enumerate() {
            if *part == "--api-key" {
                if let Some(value) = parts.get(index + 1) {
                    return Some((*value).into());
                }
            }
            if let Some(value) = part.strip_prefix("--api-key=") {
                return Some(value.into());
            }
        }
    }
    None
}

fn spawn_sidecar(
    root: &Path,
    api_key: &str,
    backend: &str,
    upstream: Option<&Upstream>,
) -> Result<()> {
    fs::create_dir_all(home())?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(home().join("desktop-sidecar.log"))?;
    let python = {
        let candidate = home().join(".venv/bin/python");
        if candidate.exists() {
            candidate
        } else {
            PathBuf::from(env::var("SYNTH_PYTHON").unwrap_or_else(|_| "python3".into()))
        }
    };
    validate_python(&python)?;
    let daemon = root.join("services/laguna-daemon");
    let mut command = Command::new(python);
    command
        .args(["-m", "laguna_daemon"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    command.env(
        "PYTHONPATH",
        append_path(&daemon, env::var("PYTHONPATH").ok()),
    );
    command.envs([
        ("SYNTH_LAGUNA_HOST", "127.0.0.1"),
        ("SYNTH_LAGUNA_PORT", "7333"),
        ("SYNTH_LAGUNA_API_KEY", api_key),
        ("SYNTH_LAGUNA_BACKEND", backend),
        ("SYNTH_LAGUNA_DEFAULT_MODEL", DEFAULT_MODEL),
        ("SYNTH_LAGUNA_AUTO_LOAD", "1"),
        ("SYNTH_LAGUNA_REQUIRE_AUTH", "1"),
    ]);
    command
        .env("SYNTH_LAGUNA_MODELS_DIR", models_dir()?)
        .env("SYNTH_LAGUNA_DATA_DIR", home());
    if let Some(value) = upstream {
        command
            .env("SYNTH_LAGUNA_EXTERNAL_URL", &value.url)
            .env("SYNTH_LAGUNA_UPSTREAM_API_KEY", &value.api_key);
    }
    detach(&mut command);
    let child = command.spawn().context("spawn Laguna sidecar")?;
    fs::write(home().join("sidecar.pid"), child.id().to_string())?;
    Ok(())
}

fn stop_managed_sidecar() -> Result<bool> {
    let path = home().join("sidecar.pid");
    let Ok(raw) = fs::read_to_string(&path) else {
        return Ok(false);
    };
    let pid: u32 = raw
        .trim()
        .parse()
        .context("invalid Synth-managed Laguna sidecar pid")?;
    if pid == 0 {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        let command = Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
            .context("inspect stale Synth-managed Laguna sidecar")?;
        if !String::from_utf8_lossy(&command.stdout).contains("laguna_daemon") {
            let _ = fs::remove_file(path);
            return Ok(false);
        }
        let status = Command::new("/bin/kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .context("stop stale Synth-managed Laguna sidecar")?;
        if status.success() {
            let _ = fs::remove_file(path);
            return Ok(true);
        }
    }
    Ok(false)
}

fn validate_python(python: &Path) -> Result<()> {
    let status = Command::new(python)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| {
            format!(
                "Laguna requires a Python environment, but `{}` could not be started. Create ~/.synth-desktop/laguna/.venv with the Laguna dependencies, or set SYNTH_PYTHON to a usable interpreter.",
                python.display()
            )
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Laguna Python interpreter `{}` exited unsuccessfully",
            python.display()
        ))
    }
}

fn append_path(prefix: &Path, existing: Option<String>) -> String {
    existing
        .map(|v| format!("{}:{v}", prefix.display()))
        .unwrap_or_else(|| prefix.display().to_string())
}
#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            libc_detach();
            Ok(())
        });
    }
}
#[cfg(not(unix))]
fn detach(_: &mut Command) {}
#[cfg(unix)]
fn libc_detach() {
    unsafe {
        extern "C" {
            fn setsid() -> i32;
        }
        let _ = setsid();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let root =
            env::temp_dir().join(format!("synth-laguna-model-test-{}", uuid::Uuid::new_v4()));
        let model = root.join(DEFAULT_MODEL);
        fs::create_dir_all(&model).unwrap();
        fs::write(model.join("config.json"), "{}").unwrap();
        fs::write(model.join("a.safetensors"), b"abc").unwrap();
        fs::write(model.join("b.safetensors"), b"12345").unwrap();
        fs::write(
            model.join(MODEL_INDEX),
            r#"{"weight_map":{"a":"a.safetensors","b":"b.safetensors","c":"a.safetensors"}}"#,
        )
        .unwrap();
        root
    }
    #[test]
    fn finds_poolside_key_forms() {
        assert_eq!(
            discover_key("/x/poolside-mlx --api-key secret"),
            Some("secret".into())
        );
        assert_eq!(
            discover_key("poolside-mlx-sidecar --api-key=token"),
            Some("token".into())
        );
    }
    #[test]
    fn trims_only_trailing_slashes() {
        assert_eq!(
            trim_url("http://127.0.0.1:7333///".into()),
            "http://127.0.0.1:7333"
        );
    }
    #[test]
    fn validates_model_root_and_parent_root() {
        let root = fixture();
        let direct = validate_model_input(&root.join(DEFAULT_MODEL)).unwrap();
        let parent = validate_model_input(&root).unwrap();
        assert_eq!(direct, parent);
        assert_eq!(direct.shard_count, 2);
        assert_eq!(direct.total_bytes, 8);
        assert_eq!(Path::new(&direct.models_root), root.canonicalize().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_missing_referenced_shard() {
        let root = fixture();
        fs::remove_file(root.join(DEFAULT_MODEL).join("b.safetensors")).unwrap();
        let error = validate_model_input(&root).unwrap_err().to_string();
        assert!(error.contains("Referenced model shard is missing"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_unsafe_shard_path() {
        let root = fixture();
        fs::write(
            root.join(DEFAULT_MODEL).join(MODEL_INDEX),
            r#"{"weight_map":{"a":"../escape.safetensors"}}"#,
        )
        .unwrap();
        let error = validate_model_input(&root).unwrap_err().to_string();
        assert!(error.contains("Unsafe shard path"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn accepts_flat_user_selected_model_directory() {
        let root = fixture();
        let flat = root.join("custom-laguna");
        fs::rename(root.join(DEFAULT_MODEL), &flat).unwrap();
        let hit = validate_model_input(&flat).unwrap();
        assert_eq!(Path::new(&hit.models_root), flat.canonicalize().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn derives_residency_timestamps_from_health() {
        let body = serde_json::json!({
            "idleSeconds": 30,
            "idleUnloadAfterSeconds": 120
        });
        assert_eq!(
            residency_from_health(&body, 1_000_000),
            ResidencyTiming {
                idle_seconds: Some(30),
                idle_unload_after_seconds: Some(120),
                last_used_at: Some(970_000),
                free_at: Some(1_090_000),
            }
        );
    }

    #[test]
    fn prefers_authoritative_residency_timestamps_from_health() {
        let body = serde_json::json!({
            "idleSeconds": 30,
            "idleUnloadAfterSeconds": 120,
            "lastUsedAt": 123_000,
            "freeAt": 243_000
        });
        let timing = residency_from_health(&body, 1_000_000);
        assert_eq!(timing.last_used_at, Some(123_000));
        assert_eq!(timing.free_at, Some(243_000));
    }

    #[test]
    fn residency_without_positive_unload_limit_has_no_free_at() {
        let body = serde_json::json!({ "idleSeconds": 5, "idleUnloadAfterSeconds": 0 });
        let timing = residency_from_health(&body, 10_000);
        assert_eq!(timing.last_used_at, Some(5_000));
        assert_eq!(timing.free_at, None);
    }

    #[test]
    fn status_serializes_residency_fields_as_camel_case() {
        let status = LagunaStatus {
            phase: "ready".into(),
            base_url: None,
            backend: None,
            loaded_model: None,
            detail: None,
            memory_bytes: Some(1),
            idle_seconds: Some(2),
            idle_unload_after_seconds: Some(3),
            last_used_at: Some(4),
            free_at: Some(5),
            updated_at: 6,
        };
        let value = serde_json::to_value(status).unwrap();
        assert_eq!(value["idleSeconds"], 2);
        assert_eq!(value["idleUnloadAfterSeconds"], 3);
        assert_eq!(value["lastUsedAt"], 4);
        assert_eq!(value["freeAt"], 5);
    }
}
