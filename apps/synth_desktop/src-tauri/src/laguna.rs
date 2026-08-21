use anyhow::{Context, Result};
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    env,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{broadcast, Mutex, RwLock};

/// Canonical ports. A named development instance overrides both through the
/// environment so several Desktops can run at once: they share one models
/// directory of read-only weights, but never a daemon, an engine, a data
/// directory, or a port. See `scripts/desktop-instance.sh`.
const DEFAULT_PORT: u16 = 7333;
/// Cleared before spawning the daemon so no inherited value can resurrect the
/// deleted second-engine path.
const UPSTREAM_ENV_VARS: [&str; 5] = [
    "SYNTH_LAGUNA_EXTERNAL_URL",
    "SYNTH_LAGUNA_EXTERNAL_API_KEY",
    "SYNTH_LAGUNA_UPSTREAM_API_KEY",
    "SYNTH_LAGUNA_UPSTREAM_HOST",
    "SYNTH_LAGUNA_UPSTREAM_PORT",
];
const DEFAULT_MODEL: &str = "poolside/Laguna-XS-2.1-NVFP4-mlx";
const DEFAULT_MODEL_REVISION: &str = "841778bda563a36104dd521e37d99218e46f4f25";
/// This process's Laguna port. Read per call rather than cached so a launcher
/// that sets it after startup is still honored.
fn laguna_port() -> u16 {
    env::var("SYNTH_LAGUNA_PORT")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

/// This process's Laguna base URL: an explicit override, else its own port.
fn laguna_base_url() -> String {
    env::var("SYNTH_LAGUNA_BASE_URL")
        .unwrap_or_else(|_| format!("http://127.0.0.1:{}", laguna_port()))
}
const MODEL_INDEX: &str = "model.safetensors.index.json";
const MAX_SAFETENSORS_HEADER_BYTES: u64 = 100_000_000;
const SELECTED_MODEL_FILE: &str = "selected_model_path";
/// The daemon at `DEFAULT_PORT` is the only local runtime: it owns the weights,
/// the admission slot, the prompt caches, and every telemetry number below.
const INFERENCE_PATH: &str = "/v1/synth/inference";
const INFERENCE_STREAM_PATH: &str = "/v1/synth/inference/stream";
const MODEL_UNLOAD_PATH: &str = "/v1/synth/model/unload";
const SETTINGS_PATH: &str = "/v1/synth/settings";
/// Guards against an SSE peer that never emits an event boundary.
const SSE_BUFFER_LIMIT: usize = 1 << 20;

#[derive(Clone, Copy)]
struct ModelSpec {
    id: &'static str,
    revision: &'static str,
    title: &'static str,
    min_disk_bytes: u64,
    download_bytes: u64,
}

const MODEL_CATALOG: [ModelSpec; 1] = [ModelSpec {
    id: DEFAULT_MODEL,
    revision: DEFAULT_MODEL_REVISION,
    title: "Laguna XS 2.1",
    min_disk_bytes: 24 * 1024 * 1024 * 1024,
    download_bytes: 21_600_000_000,
}];

fn model_spec(model_id: &str) -> Result<ModelSpec> {
    MODEL_CATALOG
        .iter()
        .copied()
        .find(|spec| spec.id == model_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown on-device model `{model_id}`"))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LagunaModelHit {
    pub path: String,
    pub models_root: String,
    pub model_id: String,
    #[specta(type = specta_typescript::Unknown)]
    pub shard_count: usize,
    #[specta(type = specta_typescript::Unknown)]
    pub total_bytes: u64,
    pub selected: bool,
    pub runtime_ready: bool,
    #[specta(type = specta_typescript::Unknown)]
    pub companion_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LagunaStatus {
    pub phase: String,
    pub base_url: Option<String>,
    pub backend: Option<String>,
    pub loaded_model: Option<String>,
    pub detail: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub memory_bytes: Option<u64>,
    #[specta(type = specta_typescript::Unknown)]
    pub idle_seconds: Option<u64>,
    #[specta(type = specta_typescript::Unknown)]
    pub idle_unload_after_seconds: Option<u64>,
    #[specta(type = specta_typescript::Unknown)]
    pub last_used_at: Option<u64>,
    #[specta(type = specta_typescript::Unknown)]
    pub free_at: Option<u64>,
    #[specta(type = specta_typescript::Unknown)]
    pub updated_at: u64,
}

/// One selectable inference policy: the base weights, or those weights with a
/// LoRA attached. Speed fields are `None` until measured — never zero.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LagunaPolicy {
    pub model_id: String,
    pub title: Option<String>,
    pub is_base: bool,
    pub digest: Option<String>,
    pub tokens_per_second_p10: Option<f64>,
    pub delta_vs_base_pct: Option<f64>,
    /// Whether the delta exceeds this policy's own measurement noise. False
    /// means the surface must not render the number, not that it is zero.
    pub delta_is_resolvable: bool,
    #[specta(type = specta_typescript::Unknown)]
    pub token_samples: u64,
}

/// The generation currently holding the daemon's single GPU admission slot.
///
/// Every metric is `Option` on purpose: `null` from the daemon means the number
/// is genuinely unavailable at this phase, and must never be rendered as zero.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct LagunaGeneration {
    pub generation_id: Option<String>,
    /// `queued | loading | compiling | prefill | decode | complete`
    pub phase: Option<String>,
    // The daemon measures monotonic timestamps in fractional milliseconds.
    // Keeping these as floats is part of the wire contract: serde rejects a
    // JSON float for `u64`, which previously discarded every live snapshot.
    pub queued_at: Option<f64>,
    pub started_at: Option<f64>,
    pub first_token_at: Option<f64>,
    pub last_token_at: Option<f64>,
    #[specta(type = specta_typescript::Unknown)]
    pub prompt_tokens: Option<u64>,
    #[specta(type = specta_typescript::Unknown)]
    pub cached_tokens: Option<u64>,
    #[specta(type = specta_typescript::Unknown)]
    pub output_tokens: Option<u64>,
    pub cache_hit_ratio: Option<f64>,
    pub prefill_tokens_per_second: Option<f64>,
    pub decode_tokens_per_second: Option<f64>,
    pub elapsed_ms: Option<f64>,
}

/// Daemon-side rolling aggregates. Percentiles are absent until enough samples
/// exist, which is reported as `null` rather than a fabricated value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct LagunaRollingStats {
    #[specta(type = specta_typescript::Unknown)]
    pub requests_completed: Option<u64>,
    #[specta(type = specta_typescript::Unknown)]
    pub requests_failed: Option<u64>,
    #[specta(type = specta_typescript::Unknown)]
    pub requests_cancelled: Option<u64>,
    #[specta(type = specta_typescript::Unknown)]
    pub input_tokens: Option<u64>,
    #[specta(type = specta_typescript::Unknown)]
    pub output_tokens: Option<u64>,
    #[specta(type = specta_typescript::Unknown)]
    pub cached_tokens: Option<u64>,
    pub ttft_p50_ms: Option<f64>,
    pub ttft_p95_ms: Option<f64>,
    pub decode_tps_p50: Option<f64>,
    pub decode_tps_p95: Option<f64>,
    pub latency_p50_ms: Option<f64>,
    pub latency_p95_ms: Option<f64>,
}

/// One `GET /v1/synth/inference` payload, which is also the per-event shape of
/// `GET /v1/synth/inference/stream`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct LagunaInference {
    pub model: Option<String>,
    pub resident: bool,
    #[specta(type = specta_typescript::Unknown)]
    pub resident_bytes: Option<u64>,
    pub queue_depth: Option<u32>,
    pub queue_capacity: Option<u32>,
    /// `None` while the daemon is idle.
    pub active: Option<LagunaGeneration>,
    pub rolling: LagunaRollingStats,
}

/// Result of `POST /v1/synth/model/unload`. A 409 is an expected answer (a
/// generation holds the slot), not a transport failure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LagunaUnloadOutcome {
    pub released: bool,
    pub conflict: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LagunaLoadOutcome {
    resident: bool,
}

/// One `/v1/synth/settings` exchange. A 404 is an expected answer from a
/// daemon build that predates the runtime-settings API, and a 400 carries the
/// daemon's typed validation envelope — both are data for the renderer, not
/// transport failures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LagunaSettingsExchange {
    pub supported: bool,
    pub status: u16,
    /// Parsed JSON body — the settings envelope on success, the typed error
    /// envelope on rejection, `null` when the body was not JSON.
    #[specta(type = specta_typescript::Unknown)]
    pub body: Value,
}

pub struct LagunaManager {
    status: RwLock<LagunaStatus>,
    ensure_lock: Mutex<()>,
    updates: broadcast::Sender<LagunaStatus>,
    inference: RwLock<Option<LagunaInference>>,
    inference_updates: broadcast::Sender<LagunaInference>,
    inference_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    client: Client,
    adapter_path: Mutex<Option<PathBuf>>,
}

impl LagunaManager {
    pub fn new() -> Self {
        let (updates, _) = broadcast::channel(32);
        let (inference_updates, _) = broadcast::channel(64);
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
            inference: RwLock::new(None),
            inference_updates,
            inference_task: Mutex::new(None),
            client: crate::http::http_client(),
            adapter_path: Mutex::new(None),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LagunaStatus> {
        self.updates.subscribe()
    }

    /// High-frequency generation activity. `/health` remains the authoritative
    /// low-frequency residency source and is deliberately not merged into this.
    pub fn subscribe_inference(&self) -> broadcast::Receiver<LagunaInference> {
        self.inference_updates.subscribe()
    }

    pub async fn last_inference(&self) -> Option<LagunaInference> {
        self.inference.read().await.clone()
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

    /// Register an adapter under a model id clients can ask for.
    ///
    /// Registration is not selection. Which policy a turn runs under is
    /// decided by the `model` on that request, so registering `…-ft` cannot
    /// change what an already-open conversation is doing.
    pub async fn register_policy(
        &self,
        model_id: &str,
        adapter_path: &Path,
        digest: Option<&str>,
    ) -> Result<LagunaPolicy> {
        let base_url = self
            .status()
            .await
            .base_url
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Laguna is not running"))?;
        let api_key = env::var("SYNTH_LAGUNA_API_KEY").unwrap_or_default();
        let response = self
            .client
            .post(format!("{}/v1/synth/policies", trim_url(base_url)))
            .bearer_auth(api_key)
            .json(&json!({
                "model_id": model_id,
                "adapter_path": adapter_path.display().to_string(),
                "digest": digest,
            }))
            .send()
            .await
            .context("Laguna policy registration is unreachable")?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            let message = body
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("Laguna refused the policy");
            return Err(anyhow::anyhow!("{message}"));
        }
        let policy = body.get("policy").cloned().unwrap_or_else(|| json!({}));
        Ok(LagunaPolicy {
            model_id: policy
                .get("model_id")
                .and_then(Value::as_str)
                .unwrap_or(model_id)
                .to_string(),
            title: policy.get("title").and_then(Value::as_str).map(str::to_string),
            is_base: policy.get("is_base").and_then(Value::as_bool).unwrap_or(false),
            digest: policy.get("digest").and_then(Value::as_str).map(str::to_string),
            tokens_per_second_p10: None,
            delta_vs_base_pct: None,
            delta_is_resolvable: false,
            token_samples: 0,
        })
    }

    /// Selectable policies, joined to whatever the daemon has actually measured.
    ///
    /// Speed fields stay `None` until the daemon has enough samples, and the
    /// delta is only marked resolvable when it exceeds that policy's own
    /// measurement noise. A caller must not invent a number for a blank.
    pub async fn policies(&self) -> Result<Vec<LagunaPolicy>> {
        let base_url = self
            .status()
            .await
            .base_url
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Laguna is not running"))?;
        let base_url = trim_url(base_url);
        let api_key = env::var("SYNTH_LAGUNA_API_KEY").unwrap_or_default();
        let listed: Value = self
            .client
            .get(format!("{base_url}/v1/synth/policies"))
            .bearer_auth(&api_key)
            .send()
            .await
            .context("Laguna policy list is unreachable")?
            .json()
            .await
            .unwrap_or_else(|_| json!({}));
        // Telemetry is enrichment: a policy list still renders when the
        // daemon has measured nothing yet, with blanks where numbers go.
        let measured: Value = match self
            .client
            .get(format!("{base_url}{INFERENCE_PATH}"))
            .bearer_auth(&api_key)
            .send()
            .await
        {
            Ok(response) => response.json().await.unwrap_or_else(|_| json!({})),
            Err(_) => json!({}),
        };
        let rows = measured.pointer("/policies/policies").cloned().unwrap_or_else(|| json!({}));
        let mut policies = Vec::new();
        for entry in listed
            .get("policies")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let model_id = entry
                .get("model_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let stats = rows.get(&model_id).cloned().unwrap_or_else(|| json!({}));
            policies.push(LagunaPolicy {
                title: entry.get("title").and_then(Value::as_str).map(str::to_string),
                is_base: entry.get("is_base").and_then(Value::as_bool).unwrap_or(false),
                digest: entry.get("digest").and_then(Value::as_str).map(str::to_string),
                tokens_per_second_p10: stats
                    .get("tokensPerSecondP10")
                    .and_then(Value::as_f64),
                delta_vs_base_pct: stats.get("deltaVsBasePct").and_then(Value::as_f64),
                delta_is_resolvable: stats
                    .get("deltaIsResolvable")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                token_samples: stats
                    .get("tokenSamples")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                model_id,
            });
        }
        Ok(policies)
    }

    pub async fn set_adapter(&self, adapter_path: Option<&Path>) -> Result<LagunaStatus> {
        let previous = self.adapter_path.lock().await.clone();
        let base_url = self
            .status()
            .await
            .base_url
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Laguna is not running"))?;
        let api_key = env::var("SYNTH_LAGUNA_API_KEY").unwrap_or_default();
        let model = selected_model_id().unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        match self
            .load_model_at(&base_url, &api_key, &model, adapter_path)
            .await
        {
            Ok(()) => {
                *self.adapter_path.lock().await = adapter_path.map(Path::to_path_buf);
                self.refresh().await;
                Ok(self.status().await)
            }
            Err(error) => {
                let _ = self
                    .load_model_at(&base_url, &api_key, &model, previous.as_deref())
                    .await;
                Err(error)
            }
        }
    }

    pub fn api_key(&self) -> Option<String> {
        env::var("SYNTH_LAGUNA_API_KEY").ok()
    }

    /// Model identity owned by the same selection logic used to launch and
    /// load the daemon. Renderer-friendly aliases must not cross the provider
    /// boundary as serving model ids.
    pub fn configured_model_id(&self) -> Result<String> {
        selected_model_id()
    }

    /// Read the daemon-owned native Codex model envelope over the same
    /// authenticated loopback transport used for inference. This is the
    /// production source for local model instructions and capabilities.
    pub async fn codex_model_catalog(&self, base_url: &str, api_key: &str) -> Result<Value> {
        let response = self
            .client
            .get(format!("{}/v1/models", base_url.trim_end_matches('/')))
            .bearer_auth(api_key)
            .timeout(crate::limits::LAGUNA_HEALTH_TIMEOUT)
            .send()
            .await
            .with_context(|| format!("Laguna model catalog is unreachable at {base_url}"))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .context("Laguna model catalog returned an unreadable payload")?;
        if !status.is_success() {
            anyhow::bail!("Laguna model catalog returned status {}", status.as_u16());
        }
        serde_json::from_slice(&body).context("Laguna model catalog returned invalid JSON")
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

    pub fn download_model_with_progress<F>(
        &self,
        model_id: &str,
        mut progress: F,
    ) -> Result<LagunaModelHit>
    where
        F: FnMut(&str, &str, u64, u64),
    {
        let spec = model_spec(model_id)?;
        progress(
            "preparing",
            "Preparing the managed model runtime…",
            0,
            spec.download_bytes,
        );
        let models_root = dirs::home_dir()
            .unwrap_or_default()
            .join(".synth-desktop/models");
        fs::create_dir_all(&models_root)?;
        if let Some(available) = available_disk_bytes(&models_root) {
            if available < spec.min_disk_bytes {
                return Err(anyhow::anyhow!(
                    "{} needs at least {:.0} GiB of free disk space; only {:.1} GiB is available.",
                    spec.title,
                    spec.min_disk_bytes as f64 / 1024f64.powi(3),
                    available as f64 / 1024f64.powi(3)
                ));
            }
        }
        let model_dir = models_root.join(spec.id);
        fs::create_dir_all(&model_dir)?;
        let python = LagunaRuntimeState::detect().require_ready()?;
        let script = r#"from huggingface_hub import HfApi, snapshot_download
import hashlib, json, pathlib, sys
repo, revision, target = sys.argv[1], sys.argv[2], pathlib.Path(sys.argv[3])
snapshot_download(repo_id=repo, revision=revision, local_dir=target)
index = json.loads((target / 'model.safetensors.index.json').read_text())
shards = {v for v in (index.get('weight_map') or {}).values()
          if isinstance(v, str) and v.endswith('.safetensors')}
if not shards:
    raise RuntimeError('model index references no safetensor shards')
info = HfApi().model_info(repo, revision=revision, files_metadata=True)
expected = {}
for sibling in info.siblings:
    lfs = getattr(sibling, 'lfs', None)
    digest = lfs.get('sha256') if isinstance(lfs, dict) else getattr(lfs, 'sha256', None)
    if isinstance(digest, str) and len(digest) == 64:
        expected[sibling.rfilename] = digest.lower()
for shard in sorted(shards):
    if '/' in shard or '\\' in shard:
        raise RuntimeError(f'unsafe indexed model shard: {shard}')
    if shard not in expected:
        raise RuntimeError(f'provider omitted a SHA-256 for model shard: {shard}')
    hasher = hashlib.sha256()
    with (target / shard).open('rb') as handle:
        for chunk in iter(lambda: handle.read(8 * 1024 * 1024), b''):
            hasher.update(chunk)
    if hasher.hexdigest() != expected[shard]:
        raise RuntimeError(f'provider checksum mismatch for model shard: {shard}')
"#;
        progress(
            "downloading",
            "Downloading model weights…",
            dir_size(&model_dir),
            spec.download_bytes,
        );
        let mut child = Command::new(&python)
            .arg("-c")
            .arg(script)
            .arg(spec.id)
            .arg(spec.revision)
            .arg(&model_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("download Laguna XS from Hugging Face")?;
        let status = loop {
            if let Some(status) = child.try_wait().context("check model download")? {
                break status;
            }
            progress(
                "downloading",
                "Downloading model weights…",
                dir_size(&model_dir),
                spec.download_bytes,
            );
            thread::sleep(Duration::from_millis(500));
        };
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        if !status.success() {
            return Err(anyhow::anyhow!(
                "Laguna download failed: {}",
                stderr.trim().chars().take(500).collect::<String>()
            ));
        }
        let hit = self.select_model(&model_dir)?;
        progress(
            "ready",
            "Model and runtime are ready.",
            spec.download_bytes,
            spec.download_bytes,
        );
        Ok(hit)
    }

    pub fn delete_model(&self, model_id: &str) -> Result<()> {
        let spec = model_spec(model_id)?;
        let models_root = dirs::home_dir()
            .unwrap_or_default()
            .join(".synth-desktop/models");
        let model_dir = models_root.join(spec.id);
        let selected = read_selected_model_path()?.and_then(|path| path.canonicalize().ok());
        if selected.as_ref().is_some_and(|path| {
            path == &model_dir
                .canonicalize()
                .unwrap_or_else(|_| model_dir.clone())
        }) {
            stop_managed_sidecar()?;
            self.clear_selected_model()?;
        }
        if model_dir.exists() {
            fs::remove_dir_all(&model_dir)
                .with_context(|| format!("remove {}", model_dir.display()))?;
        }
        Ok(())
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
        let base_url = trim_url(laguna_base_url());
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
            if matches!(
                status.phase.as_str(),
                "ready" | "unloaded" | "not_installed"
            ) {
                self.set_status(status).await;
                write_env_sh(&api_key, &base_url)?;
                return Ok((self.status().await.phase == "ready").then_some(base_url));
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

        // The Synth-managed daemon is the only local runtime: it loads the
        // weights in-process. There is no second engine to discover or proxy
        // through — Poolside's own sidecar is not ours and is never reused.
        let backend = if cfg!(target_os = "macos") {
            "mlx_lm"
        } else {
            "auto"
        };
        let mut status = self.status().await;
        status.phase = "loading".into();
        status.backend = Some(backend.into());
        status.detail = Some("Starting Laguna sidecar…".into());
        self.set_status(status).await;
        write_env_sh(&api_key, &base_url)?;
        spawn_sidecar(workshop_root, &api_key, backend)?;

        let deadline = tokio::time::Instant::now() + crate::limits::LAGUNA_READY_WAIT;
        while tokio::time::Instant::now() < deadline {
            if let Some(status) = self.probe(&base_url, &api_key).await {
                let done = matches!(status.phase.as_str(), "ready" | "error" | "not_installed");
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

    /// Prepare the production local provider for a model turn. `/health`
    /// intentionally remains ready while weights are evicted, so daemon
    /// readiness alone cannot prove that an already-attached Codex session may
    /// send. The control-plane load operation is idempotent and is the daemon's
    /// authoritative way to restore residency after manual or idle unload.
    pub async fn ensure_for_turn(&self, workshop_root: &Path) -> Result<Option<String>> {
        let Some(base_url) = self.ensure(workshop_root).await? else {
            return Ok(None);
        };
        let api_key = self
            .api_key()
            .context("Laguna daemon credential is unavailable after ensure")?;
        let model = selected_model_id()?;
        if self.status().await.loaded_model.as_deref() == Some(model.as_str()) {
            return Ok(Some(base_url));
        }
        let mut loading = self.status().await;
        loading.phase = "loading".into();
        loading.detail = Some("Loading Laguna weights for the next turn…".into());
        self.set_status(loading).await;
        let adapter = self.adapter_path.lock().await.clone();
        if let Err(error) = self
            .load_model_at(&base_url, &api_key, &model, adapter.as_deref())
            .await
        {
            self.set_error(error.to_string()).await;
            return Err(error);
        }
        // The load response's `resident: true` is authoritative. Publish it
        // immediately, then enrich timing/memory fields from health when that
        // follow-up probe is available.
        let mut ready = self.status().await;
        ready.phase = "ready".into();
        ready.loaded_model = Some(model);
        ready.detail = Some("Laguna XS ready".into());
        self.set_status(ready).await;
        self.refresh().await;
        Ok(Some(base_url))
    }

    async fn load_model_at(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        adapter_path: Option<&Path>,
    ) -> Result<()> {
        let mut url = reqwest::Url::parse(base_url).context("invalid Laguna base URL")?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("Laguna base URL cannot carry path segments"))?;
            segments.pop_if_empty();
            segments.extend(["v1", "synth", "models"]);
            segments.extend(model.split('/'));
            segments.push("load");
        }
        let body = json!({
            "adapter_path": adapter_path.map(|path| path.display().to_string())
        });
        let response = self
            .client
            .post(url)
            .bearer_auth(api_key)
            .timeout(crate::limits::LAGUNA_READY_WAIT)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Laguna model load is unreachable at {base_url}"))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .context("Laguna model load returned an unreadable payload")?;
        if !status.is_success() {
            let code = serde_json::from_slice::<Value>(&body)
                .ok()
                .and_then(|value| {
                    value
                        .pointer("/error/code")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "load_failed".into());
            anyhow::bail!("Laguna model load returned {} ({code})", status.as_u16());
        }
        let outcome: LagunaLoadOutcome = serde_json::from_slice(&body)
            .context("Laguna model load returned an unreadable success payload")?;
        if !outcome.resident {
            anyhow::bail!("Laguna model load completed without resident weights");
        }
        Ok(())
    }

    async fn probe(&self, base_url: &str, api_key: &str) -> Option<LagunaStatus> {
        let response = self
            .client
            .get(format!("{base_url}/health"))
            .bearer_auth(api_key)
            .timeout(crate::limits::LAGUNA_HEALTH_TIMEOUT)
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
            "not_installed" => "not_installed",
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
            detail: Some(match phase {
                "ready" => "Laguna XS ready".into(),
                "not_installed" => "Laguna XS is not installed".into(),
                _ => format!("sidecar {phase}"),
            }),
            memory_bytes: body.get("memoryBytes").and_then(Value::as_u64),
            idle_seconds: residency.idle_seconds,
            idle_unload_after_seconds: residency.idle_unload_after_seconds,
            last_used_at: residency.last_used_at,
            free_at: residency.free_at,
            updated_at: observed_at,
        })
    }

    /// Base URL for the telemetry endpoints. Falls back to the configured
    /// default so a monitor can render before `ensure` has completed.
    async fn inference_base_url(&self) -> String {
        if let Some(url) = self.status().await.base_url {
            return url;
        }
        trim_url(laguna_base_url())
    }

    async fn record_inference(&self, snapshot: LagunaInference) {
        *self.inference.write().await = Some(snapshot.clone());
        let _ = self.inference_updates.send(snapshot);
    }

    /// One-shot `GET /v1/synth/inference`.
    pub async fn inference_snapshot(&self) -> Result<LagunaInference> {
        let base_url = self.inference_base_url().await;
        let response = self
            .client
            .get(format!("{base_url}{INFERENCE_PATH}"))
            .bearer_auth(self.api_key().unwrap_or_default())
            .timeout(crate::limits::LAGUNA_ADMIN_TIMEOUT)
            .send()
            .await
            .with_context(|| format!("Laguna inference telemetry is unreachable at {base_url}"))?;
        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Laguna inference telemetry returned {}",
                response.status().as_u16()
            ));
        }
        let snapshot: LagunaInference = response
            .json()
            .await
            .context("Laguna inference telemetry returned an unreadable payload")?;
        self.record_inference(snapshot.clone()).await;
        Ok(snapshot)
    }

    /// `POST /v1/synth/model/unload`. A 409 is reported as a conflict outcome
    /// rather than an error: a generation legitimately holds the slot.
    pub async fn unload_model(&self) -> Result<LagunaUnloadOutcome> {
        let base_url = self.inference_base_url().await;
        let response = self
            .client
            .post(format!("{base_url}{MODEL_UNLOAD_PATH}"))
            .bearer_auth(self.api_key().unwrap_or_default())
            .timeout(crate::limits::LAGUNA_INFERENCE_TIMEOUT)
            .send()
            .await
            .with_context(|| format!("Laguna is unreachable at {base_url}"))?;
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        let outcome = unload_outcome(status, &body).map_err(anyhow::Error::msg)?;
        if outcome.released {
            // Residency truth lives in /health; refresh it rather than guessing.
            self.refresh().await;
        }
        Ok(outcome)
    }

    /// One-shot `GET /v1/synth/settings`.
    pub async fn settings_snapshot(&self) -> Result<LagunaSettingsExchange> {
        let base_url = self.inference_base_url().await;
        let response = self
            .client
            .get(format!("{base_url}{SETTINGS_PATH}"))
            .bearer_auth(self.api_key().unwrap_or_default())
            .timeout(crate::limits::LAGUNA_ADMIN_TIMEOUT)
            .send()
            .await
            .with_context(|| format!("Laguna settings are unreachable at {base_url}"))?;
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        Ok(settings_exchange(status, &body))
    }

    /// `PUT /v1/synth/settings` with a partial update. The daemon answers with
    /// the full effective settings, or a typed validation envelope on 400.
    pub async fn settings_update(&self, patch: Value) -> Result<LagunaSettingsExchange> {
        let base_url = self.inference_base_url().await;
        let response = self
            .client
            .put(format!("{base_url}{SETTINGS_PATH}"))
            .bearer_auth(self.api_key().unwrap_or_default())
            .json(&patch)
            .timeout(crate::limits::LAGUNA_STOP_TIMEOUT)
            .send()
            .await
            .with_context(|| format!("Laguna settings are unreachable at {base_url}"))?;
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        Ok(settings_exchange(status, &body))
    }

    /// Starts the SSE subscription if it is not already running. `emit` receives
    /// every decoded event so the caller can forward it to the webview.
    pub async fn start_inference_stream<E>(self: &Arc<Self>, emit: E)
    where
        E: Fn(LagunaInference) + Send + Sync + 'static,
    {
        let mut task = self.inference_task.lock().await;
        if task
            .as_ref()
            .is_some_and(|handle| !handle.inner().is_finished())
        {
            return;
        }
        let manager = Arc::clone(self);
        *task = Some(tauri::async_runtime::spawn(async move {
            manager.run_inference_stream(emit).await;
        }));
    }

    pub async fn stop_inference_stream(&self) {
        if let Some(handle) = self.inference_task.lock().await.take() {
            handle.abort();
        }
    }

    async fn run_inference_stream<E>(self: Arc<Self>, emit: E)
    where
        E: Fn(LagunaInference) + Send + Sync + 'static,
    {
        let mut backoff = Duration::from_millis(500);
        loop {
            match self.read_inference_stream(&emit).await {
                Ok(()) => backoff = Duration::from_millis(500),
                Err(_) => backoff = (backoff * 2).min(Duration::from_secs(10)),
            }
            tokio::time::sleep(backoff).await;
        }
    }

    async fn read_inference_stream<E>(&self, emit: &E) -> Result<()>
    where
        E: Fn(LagunaInference) + Send + Sync + 'static,
    {
        let base_url = self.inference_base_url().await;
        let mut response = self
            .client
            .get(format!("{base_url}{INFERENCE_STREAM_PATH}"))
            .bearer_auth(self.api_key().unwrap_or_default())
            .header("accept", "text/event-stream")
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Laguna inference stream returned {}",
                response.status().as_u16()
            ));
        }
        let mut buffer = String::new();
        while let Some(chunk) = response.chunk().await? {
            // Bare CRs never survive JSON encoding, so dropping them lets the
            // frame splitter work on a single line-ending form.
            buffer.push_str(&String::from_utf8_lossy(&chunk).replace('\r', ""));
            if buffer.len() > SSE_BUFFER_LIMIT {
                buffer.clear();
                continue;
            }
            for payload in take_sse_payloads(&mut buffer) {
                if payload.trim() == "[DONE]" {
                    continue;
                }
                if let Ok(snapshot) = serde_json::from_str::<LagunaInference>(&payload) {
                    self.record_inference(snapshot.clone()).await;
                    emit(snapshot);
                }
            }
        }
        Ok(())
    }
}

impl crate::services::ManagedService for LagunaManager {
    fn name(&self) -> &'static str {
        "laguna"
    }

    fn stop(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            self.stop_inference_stream().await;
            stop_managed_sidecar()?;
            Ok(())
        })
    }
}

/// Splits complete SSE frames out of `buffer` and returns their `data:` bodies.
/// Partial frames stay in the buffer for the next chunk.
fn take_sse_payloads(buffer: &mut String) -> Vec<String> {
    let mut payloads = Vec::new();
    while let Some(index) = buffer.find("\n\n") {
        let frame = buffer[..index].to_owned();
        buffer.drain(..index + 2);
        if let Some(payload) = sse_frame_data(&frame) {
            payloads.push(payload);
        }
    }
    payloads
}

fn sse_frame_data(frame: &str) -> Option<String> {
    let mut data = String::new();
    for line in frame.lines() {
        if line.starts_with(':') {
            continue;
        }
        let Some(rest) = line.strip_prefix("data:") else {
            continue;
        };
        if !data.is_empty() {
            data.push('\n');
        }
        data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
    }
    (!data.trim().is_empty()).then_some(data)
}

fn settings_exchange(status: u16, body: &str) -> LagunaSettingsExchange {
    LagunaSettingsExchange {
        supported: status != 404,
        status,
        body: serde_json::from_str(body).unwrap_or(Value::Null),
    }
}

fn unload_outcome(status: u16, body: &str) -> std::result::Result<LagunaUnloadOutcome, String> {
    let detail = serde_json::from_str::<Value>(body).ok().and_then(|value| {
        ["detail", "message", "error"]
            .iter()
            .find_map(|key| value.get(*key).and_then(Value::as_str).map(str::to_owned))
    });
    match status {
        200..=299 => Ok(LagunaUnloadOutcome {
            released: true,
            conflict: false,
            detail,
        }),
        409 => Ok(LagunaUnloadOutcome {
            released: false,
            conflict: true,
            detail: Some(
                detail
                    .unwrap_or_else(|| "A generation is active; the model stays resident.".into()),
            ),
        }),
        other => Err(format!("Laguna refused to free the model ({other})")),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn laguna_inference_snapshot(
    state: State<'_, Arc<LagunaManager>>,
) -> std::result::Result<LagunaInference, String> {
    state
        .inference_snapshot()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn laguna_inference_stream_start(
    app: AppHandle,
    state: State<'_, Arc<LagunaManager>>,
) -> std::result::Result<(), String> {
    let manager = state.inner().clone();
    manager
        .start_inference_stream(move |snapshot| {
            let _ = app.emit(
                crate::contract::events::EventChannel::LAGUNA_INFERENCE,
                snapshot,
            );
        })
        .await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn laguna_inference_stream_stop(
    state: State<'_, Arc<LagunaManager>>,
) -> std::result::Result<(), String> {
    state.stop_inference_stream().await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn laguna_model_unload(
    state: State<'_, Arc<LagunaManager>>,
) -> std::result::Result<LagunaUnloadOutcome, String> {
    state
        .unload_model()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn laguna_settings_snapshot(
    state: State<'_, Arc<LagunaManager>>,
) -> std::result::Result<LagunaSettingsExchange, String> {
    state
        .settings_snapshot()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn laguna_settings_update(
    state: State<'_, Arc<LagunaManager>>,
    patch: crate::contract::specta::OpaqueJson,
) -> std::result::Result<LagunaSettingsExchange, String> {
    state
        .settings_update(patch.0)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn laguna_model_download(
    app: AppHandle,
    state: State<'_, Arc<LagunaManager>>,
    model_id: String,
) -> std::result::Result<LagunaModelHit, String> {
    let spec = model_spec(&model_id).map_err(|error| error.to_string())?;
    let _ = app.emit(
        crate::contract::events::EventChannel::LAGUNA_DOWNLOAD,
        serde_json::json!({"phase":"downloading","detail":format!("Downloading {} from Hugging Face…", spec.title), "modelId":model_id}),
    );
    let manager = state.inner().clone();
    let progress_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        manager.download_model_with_progress(spec.id, |phase, detail, downloaded, total| {
            let _ = progress_app.emit(
                crate::contract::events::EventChannel::LAGUNA_DOWNLOAD,
                serde_json::json!({
                    "phase": phase,
                    "detail": detail,
                    "modelId": spec.id,
                    "downloadedBytes": downloaded,
                    "totalBytes": total,
                }),
            );
        })
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string());
    let payload = match &result {
        Ok(hit) => {
            serde_json::json!({"phase":"ready","detail":format!("{} download complete.", spec.title),"path":hit.path,"modelId":spec.id})
        }
        Err(error) => serde_json::json!({"phase":"error","detail":error}),
    };
    let _ = app.emit(
        crate::contract::events::EventChannel::LAGUNA_DOWNLOAD,
        payload,
    );
    result
}

#[tauri::command]
#[specta::specta]
pub async fn laguna_model_delete(
    state: State<'_, Arc<LagunaManager>>,
    model_id: String,
) -> std::result::Result<(), String> {
    let manager = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.delete_model(&model_id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
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
    if let Some(selected) = selected_model_hit(read_selected_model_path()?)? {
        // A managed model can be removed outside the app (or restored from an
        // older profile after the weights have been deleted).  In that case
        // the selection file is only a stale preference, not a reason to make
        // the local runtime fail before the user asks to use it.  Existing,
        // malformed model directories still fail loudly so partial/corrupt
        // downloads are never mistaken for a usable model.
        return Ok(selected.models_root.into());
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

fn selected_model_id() -> Result<String> {
    if let Some(selected) = selected_model_hit(read_selected_model_path()?)? {
        return Ok(selected.model_id);
    }
    Ok(DEFAULT_MODEL.into())
}

fn selected_model_hit(selected: Option<PathBuf>) -> Result<Option<LagunaModelHit>> {
    match selected {
        Some(path) if path.exists() => validate_model_input(&path).map(Some),
        Some(_) | None => Ok(None),
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
        MODEL_CATALOG
            .iter()
            .map(|spec| input.join(spec.id))
            .find(|candidate| candidate.join("config.json").is_file())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} is neither a supported model directory nor a models root",
                    input.display()
                )
            })?
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
    if weight_map.values().any(|value| !value.is_string()) {
        return Err(anyhow::anyhow!(
            "{} contains a non-string shard reference",
            index_path.display()
        ));
    }
    let shards: HashSet<&str> = weight_map.values().filter_map(Value::as_str).collect();
    if shards.is_empty() {
        return Err(anyhow::anyhow!(
            "{} references no safetensor shards",
            index_path.display()
        ));
    }
    let mut total_bytes = 0;
    let mut payload_bytes = 0;
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
        payload_bytes += safetensors_payload_bytes(&path)?;
    }
    if let Some(declared) = index
        .get("metadata")
        .and_then(|metadata| metadata.get("total_size"))
        .and_then(Value::as_u64)
    {
        if payload_bytes != declared {
            return Err(anyhow::anyhow!(
                "Model tensor payload bytes do not match {}: expected {declared}, found {payload_bytes}",
                index_path.display()
            ));
        }
    }
    let canonical = model_dir
        .canonicalize()
        .with_context(|| format!("Resolve model directory {}", model_dir.display()))?;
    let spec = MODEL_CATALOG
        .iter()
        .find(|spec| canonical.ends_with(spec.id))
        .copied()
        .or_else(|| {
            let config: Value = serde_json::from_str(&fs::read_to_string(&config).ok()?).ok()?;
            match config.get("model_type").and_then(Value::as_str) {
                Some("laguna") => model_spec(DEFAULT_MODEL).ok(),
                _ => None,
            }
        })
        .ok_or_else(|| {
            anyhow::anyhow!("{} is not a supported Workshop model", canonical.display())
        })?;
    let suffix_depth = Path::new(spec.id).components().count();
    let models_root = if canonical.ends_with(spec.id) {
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
        model_id: spec.id.into(),
        shard_count: shards.len(),
        total_bytes,
        selected: false,
        runtime_ready: true,
        companion_bytes: 0,
    })
}

fn safetensors_payload_bytes(path: &Path) -> Result<u64> {
    let file_size = fs::metadata(path)
        .with_context(|| format!("Read safetensors metadata: {}", path.display()))?
        .len();
    let mut file = fs::File::open(path)
        .with_context(|| format!("Open safetensors shard: {}", path.display()))?;
    let mut encoded_header_size = [0u8; 8];
    file.read_exact(&mut encoded_header_size)
        .with_context(|| format!("Read safetensors header size: {}", path.display()))?;
    let header_size = u64::from_le_bytes(encoded_header_size);
    if header_size == 0
        || header_size > MAX_SAFETENSORS_HEADER_BYTES
        || 8u64.saturating_add(header_size) > file_size
    {
        return Err(anyhow::anyhow!(
            "Invalid safetensors header size in {}",
            path.display()
        ));
    }
    let mut encoded_header = vec![0u8; header_size as usize];
    file.read_exact(&mut encoded_header)
        .with_context(|| format!("Read safetensors header: {}", path.display()))?;
    let header: Value = serde_json::from_slice(&encoded_header)
        .with_context(|| format!("Invalid safetensors header JSON: {}", path.display()))?;
    let object = header.as_object().ok_or_else(|| {
        anyhow::anyhow!("Safetensors header is not an object: {}", path.display())
    })?;
    let mut ranges = Vec::new();
    for (name, tensor) in object {
        if name == "__metadata__" {
            continue;
        }
        let offsets = tensor
            .get("data_offsets")
            .and_then(Value::as_array)
            .filter(|offsets| offsets.len() == 2)
            .ok_or_else(|| {
                anyhow::anyhow!("Invalid safetensors tensor offsets in {}", path.display())
            })?;
        let start = offsets[0].as_u64().ok_or_else(|| {
            anyhow::anyhow!("Invalid safetensors tensor offset in {}", path.display())
        })?;
        let end = offsets[1].as_u64().ok_or_else(|| {
            anyhow::anyhow!("Invalid safetensors tensor offset in {}", path.display())
        })?;
        if end < start {
            return Err(anyhow::anyhow!(
                "Invalid safetensors tensor range in {}",
                path.display()
            ));
        }
        ranges.push((start, end));
    }
    if ranges.is_empty() {
        return Err(anyhow::anyhow!(
            "Safetensors shard contains no tensors: {}",
            path.display()
        ));
    }
    ranges.sort_unstable();
    let mut cursor = 0;
    for (start, end) in ranges {
        if start != cursor {
            return Err(anyhow::anyhow!(
                "Safetensors tensor data is not contiguous in {}",
                path.display()
            ));
        }
        cursor = end;
    }
    if 8u64.saturating_add(header_size).saturating_add(cursor) != file_size {
        return Err(anyhow::anyhow!(
            "Safetensors payload length does not match file size: {}",
            path.display()
        ));
    }
    Ok(cursor)
}

fn discover_models() -> Result<Vec<LagunaModelHit>> {
    let user = dirs::home_dir().unwrap_or_default();
    let mut candidates = vec![user.join(".config/poolside/models").join(DEFAULT_MODEL)];
    candidates.extend(
        MODEL_CATALOG
            .iter()
            .map(|spec| user.join(".synth-desktop/models").join(spec.id)),
    );
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
    let model_id = selected_model_id()?;
    let body = format!("export SYNTH_LAGUNA_HOST=\"127.0.0.1\"\nexport SYNTH_LAGUNA_BASE_URL=\"{base_url}\"\nexport SYNTH_LAGUNA_API_KEY=\"{api_key}\"\nexport SYNTH_LAGUNA_BACKEND=\"{}\"\nexport SYNTH_LAGUNA_DEFAULT_MODEL=\"{model_id}\"\nexport SYNTH_LAGUNA_MODELS_DIR=\"{}\"\nexport SYNTH_LAGUNA_AUTO_LOAD=\"1\"\nexport PATH=\"$HOME/.synth-desktop/laguna/.venv/bin:$PATH\"\n", env::var("SYNTH_LAGUNA_BACKEND").unwrap_or_else(|_| "auto".into()), models_dir()?.display());
    fs::write(home().join("env.sh"), body)?;
    Ok(())
}

fn spawn_sidecar(root: &Path, api_key: &str, backend: &str) -> Result<()> {
    fs::create_dir_all(home())?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(home().join("desktop-sidecar.log"))?;
    let python = LagunaRuntimeState::detect().require_ready()?;
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
    // Keep explicit ownership even though the daemon has its own process group:
    // if Desktop crashes, the large MLX allocation must not survive it.
    command.env("SYNTH_DESKTOP_PARENT_PID", std::process::id().to_string());
    apply_daemon_env(&mut command, api_key, backend, &models_dir()?);
    detach(&mut command);
    let child = command.spawn().context("spawn Laguna sidecar")?;
    fs::write(home().join("sidecar.pid"), child.id().to_string())?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LagunaRuntimeState {
    Ready { python: PathBuf },
    Missing { expected: PathBuf },
    Invalid { expected: PathBuf, detail: String },
}

impl LagunaRuntimeState {
    /// The runtime is machine-owned and shared. Instance homes contain mutable
    /// daemon state only; they never select an interpreter.
    fn authoritative_python() -> PathBuf {
        dirs::home_dir()
            .expect("Laguna runtime discovery requires an operating-system home directory")
            .join(".synth-desktop/laguna/.venv/bin/python")
    }

    fn detect() -> Self {
        Self::detect_at(Self::authoritative_python())
    }

    fn detect_at(expected: PathBuf) -> Self {
        if !expected.is_file() {
            return Self::Missing { expected };
        }
        match validate_python(&expected) {
            Ok(()) => Self::Ready { python: expected },
            Err(error) => Self::Invalid {
                expected,
                detail: format!("{error:#}"),
            },
        }
    }

    fn require_ready(self) -> Result<PathBuf> {
        match self {
            Self::Ready { python } => Ok(python),
            Self::Missing { expected } => Err(anyhow::anyhow!(
                "Laguna runtime is missing at `{}`. Install the Workshop-managed Laguna runtime in Settings → Services; no alternate interpreter will be used.",
                expected.display()
            )),
            Self::Invalid { expected, detail } => Err(anyhow::anyhow!(
                "Laguna runtime at `{}` is invalid: {detail}. Repair it in Settings → Services; no alternate interpreter will be used.",
                expected.display()
            )),
        }
    }
}

/// The Workshop-managed Python used for verified Hugging Face downloads.
pub(crate) fn managed_python() -> Result<PathBuf> {
    match LagunaRuntimeState::detect() {
        LagunaRuntimeState::Ready { python } => Ok(python),
        LagunaRuntimeState::Missing { expected } => Err(anyhow::anyhow!(
            "The Workshop-managed model runtime is missing at `{}`. Install it in Settings → Services before downloading training weights.",
            expected.display()
        )),
        LagunaRuntimeState::Invalid { expected, detail } => Err(anyhow::anyhow!(
            "The Workshop-managed model runtime at `{}` is invalid: {detail}. Repair it in Settings → Services before downloading training weights.",
            expected.display()
        )),
    }
}

/// Environment for the Synth-managed daemon. The upstream/external variables are
/// actively cleared: an inherited `SYNTH_LAGUNA_EXTERNAL_URL` (or the legacy
/// `:7334` upstream port) would otherwise make the daemon proxy to a second
/// engine instead of owning the weights itself.
fn apply_daemon_env(command: &mut Command, api_key: &str, backend: &str, models_dir: &Path) {
    let model_id = selected_model_id().unwrap_or_else(|_| DEFAULT_MODEL.into());
    let laguna_port_string = laguna_port().to_string();
    command.envs([
        ("PYTHONDONTWRITEBYTECODE", "1"),
        ("SYNTH_LAGUNA_HOST", "127.0.0.1"),
        ("SYNTH_LAGUNA_PORT", laguna_port_string.as_str()),
        ("SYNTH_LAGUNA_API_KEY", api_key),
        ("SYNTH_LAGUNA_BACKEND", backend),
        ("SYNTH_LAGUNA_DEFAULT_MODEL", model_id.as_str()),
        ("SYNTH_LAGUNA_AUTO_LOAD", "1"),
        ("SYNTH_LAGUNA_REQUIRE_AUTH", "1"),
    ]);
    command
        .env("SYNTH_LAGUNA_MODELS_DIR", models_dir)
        .env("SYNTH_LAGUNA_DATA_DIR", home());
    for legacy in UPSTREAM_ENV_VARS {
        command.env_remove(legacy);
    }
}

/// Only a process we started, and that is still the Laguna daemon, may be
/// signalled. Poolside's `poolside-mlx-sidecar` and any stock `mlx_lm.server`
/// belong to someone else and are never ours to stop.
fn is_managed_sidecar_command(command: &str) -> bool {
    command.contains("laguna_daemon")
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
        if !is_managed_sidecar_command(&String::from_utf8_lossy(&command.stdout)) {
            let _ = fs::remove_file(path);
            return Ok(false);
        }
        // The daemon is a session/process-group leader. Signal the whole group
        // so an MLX worker cannot survive its parent, then verify termination
        // and escalate after a bounded grace period.
        let process_group = format!("-{pid}");
        let status = Command::new("/bin/kill")
            .args(["-TERM", "--", process_group.as_str()])
            .status()
            .context("stop stale Synth-managed Laguna sidecar")?;
        if !status.success() {
            return Ok(false);
        }
        for _ in 0..100 {
            if !process_is_alive(pid) {
                let _ = fs::remove_file(path);
                return Ok(true);
            }
            thread::sleep(Duration::from_millis(50));
        }
        let killed = Command::new("/bin/kill")
            .args(["-KILL", "--", process_group.as_str()])
            .status()
            .context("force-stop unresponsive Synth-managed Laguna sidecar")?;
        if !killed.success() {
            return Ok(false);
        }
        for _ in 0..40 {
            if !process_is_alive(pid) {
                let _ = fs::remove_file(path);
                return Ok(true);
            }
            thread::sleep(Duration::from_millis(50));
        }
        anyhow::bail!("Synth-managed Laguna sidecar {pid} did not terminate after SIGKILL");
    }
    #[cfg(not(unix))]
    {
        Ok(false)
    }
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn validate_python(python: &Path) -> Result<()> {
    let status = Command::new(python)
        .args([
            "-c",
            "import fastapi, huggingface_hub, uvicorn; assert fastapi and huggingface_hub and uvicorn",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| {
            format!(
                "Workshop could not start the authoritative Laguna runtime `{}`",
                python.display()
            )
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Laguna runtime `{}` is missing required packages (fastapi, huggingface_hub, or uvicorn)",
            python.display()
        ))
    }
}

fn append_path(prefix: &Path, existing: Option<String>) -> String {
    existing
        .map(|v| format!("{}:{v}", prefix.display()))
        .unwrap_or_else(|| prefix.display().to_string())
}

fn parse_df_available_bytes(output: &str) -> Option<u64> {
    let fields: Vec<&str> = output.lines().last()?.split_whitespace().collect();
    fields.get(3)?.parse::<u64>().ok()?.checked_mul(1024)
}

fn available_disk_bytes(path: &Path) -> Option<u64> {
    let output = Command::new("/bin/df")
        .args(["-Pk"])
        .arg(path)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| parse_df_available_bytes(&String::from_utf8_lossy(&output.stdout)))
        .flatten()
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(metadata) if metadata.is_dir() => dir_size(&entry.path()),
            Ok(metadata) => metadata.len(),
            Err(_) => 0,
        })
        .sum()
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

    #[test]
    fn inference_catalog_excludes_training_model() {
        assert!(!MODEL_CATALOG
            .iter()
            .any(|spec| spec.id == crate::training_models::QWEN_TRAINING_MODEL_ID));
    }

    async fn serve_model_load(
        status: u16,
        body: &'static str,
    ) -> (String, String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let credential = uuid::Uuid::new_v4().to_string();
        let expected_credential = credential.clone();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 16 * 1024];
            let length = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..length]);
            assert!(request.starts_with(
                "POST /v1/synth/models/poolside/Laguna-XS-2.1-NVFP4-mlx/load HTTP/1.1"
            ));
            assert!(request
                .to_ascii_lowercase()
                .contains(&format!("authorization: bearer {}", expected_credential)));
            let reason = if status == 200 {
                "OK"
            } else {
                "Service Unavailable"
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        (format!("http://{address}"), credential, server)
    }

    async fn serve_model_catalog(
        body: &'static str,
    ) -> (String, String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let credential = uuid::Uuid::new_v4().to_string();
        let expected_credential = credential.clone();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 16 * 1024];
            let length = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..length]);
            assert!(request.starts_with("GET /v1/models HTTP/1.1"));
            assert!(request
                .to_ascii_lowercase()
                .contains(&format!("authorization: bearer {}", expected_credential)));
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        (format!("http://{address}"), credential, server)
    }

    #[tokio::test]
    async fn codex_catalog_uses_authenticated_daemon_native_envelope() {
        let body = r#"{"models":[{"slug":"poolside/Laguna-XS-2.1-NVFP4-mlx","base_instructions":"daemon-owned"}]}"#;
        let (base_url, credential, server) = serve_model_catalog(body).await;
        let catalog = LagunaManager::new()
            .codex_model_catalog(&base_url, &credential)
            .await
            .unwrap();
        assert_eq!(catalog["models"][0]["base_instructions"], "daemon-owned");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn production_load_control_restores_residency_after_unload() {
        let (base_url, credential, server) = serve_model_load(
            200,
            r#"{"operation_id":"op_test","model":"poolside/Laguna-XS-2.1-NVFP4-mlx","state":"resident_idle","resident":true,"already_resident":false}"#,
        )
        .await;
        LagunaManager::new()
            .load_model_at(&base_url, &credential, DEFAULT_MODEL, None)
            .await
            .expect("the production load control response restores residency");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn model_load_failure_is_typed_and_does_not_echo_daemon_detail() {
        let (base_url, credential, server) = serve_model_load(
            503,
            r#"{"error":{"code":"insufficient_memory","message":"sensitive daemon detail"}}"#,
        )
        .await;
        let error = LagunaManager::new()
            .load_model_at(&base_url, &credential, DEFAULT_MODEL, None)
            .await
            .expect_err("a rejected load must fail the turn preflight")
            .to_string();
        assert!(error.contains("503 (insufficient_memory)"));
        assert!(!error.contains("sensitive daemon detail"));
        assert!(!error.contains(&credential));
        server.await.unwrap();
    }

    #[test]
    fn parses_portable_df_capacity_for_download_preflight() {
        let output = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s5 100000000 1000 50331648 1% /System/Volumes/Data\n";
        assert_eq!(
            parse_df_available_bytes(output),
            Some(48 * 1024 * 1024 * 1024)
        );
    }
    fn fixture() -> PathBuf {
        let root =
            env::temp_dir().join(format!("synth-laguna-model-test-{}", uuid::Uuid::new_v4()));
        let model = root.join(DEFAULT_MODEL);
        fs::create_dir_all(&model).unwrap();
        fs::write(model.join("config.json"), r#"{"model_type":"laguna"}"#).unwrap();
        write_safetensors(&model.join("a.safetensors"), b"abc");
        write_safetensors(&model.join("b.safetensors"), b"12345");
        fs::write(
            model.join(MODEL_INDEX),
            r#"{"metadata":{"total_size":8},"weight_map":{"a":"a.safetensors","b":"b.safetensors","c":"a.safetensors"}}"#,
        )
        .unwrap();
        root
    }

    fn write_safetensors(path: &Path, payload: &[u8]) {
        let header = serde_json::to_vec(&serde_json::json!({
            "weight": {
                "dtype": "U8",
                "shape": [payload.len()],
                "data_offsets": [0, payload.len()]
            }
        }))
        .unwrap();
        let mut bytes = Vec::with_capacity(8 + header.len() + payload.len());
        bytes.extend_from_slice(&(header.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(payload);
        fs::write(path, bytes).unwrap();
    }
    #[test]
    fn settings_exchange_marks_missing_endpoint_as_unsupported() {
        let exchange = settings_exchange(404, "Not Found");
        assert!(!exchange.supported);
        assert_eq!(exchange.status, 404);
        assert_eq!(exchange.body, Value::Null);
    }

    #[test]
    fn settings_exchange_passes_settings_envelope_through() {
        let body = r#"{"schema_version":"1.0","settings":{"default_temperature":1.0}}"#;
        let exchange = settings_exchange(200, body);
        assert!(exchange.supported);
        assert_eq!(
            exchange.body["settings"]["default_temperature"],
            Value::from(1.0)
        );
    }

    #[test]
    fn settings_exchange_keeps_typed_validation_envelope_intact() {
        let body = r#"{"error":{"code":"invalid_setting","message":"top_k must be an integer between 0 and 8192","retryable":false,"details":{"field":"default_top_k"}},"request_id":"req-1"}"#;
        let exchange = settings_exchange(400, body);
        assert!(exchange.supported);
        assert_eq!(exchange.status, 400);
        assert_eq!(exchange.body["error"]["code"], "invalid_setting");
        assert_eq!(exchange.body["error"]["details"]["field"], "default_top_k");
    }

    #[test]
    fn settings_exchange_tolerates_non_json_bodies() {
        let exchange = settings_exchange(502, "<html>bad gateway</html>");
        assert!(exchange.supported);
        assert_eq!(exchange.body, Value::Null);
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
        let model = root.join(DEFAULT_MODEL);
        let on_disk = fs::metadata(model.join("a.safetensors")).unwrap().len()
            + fs::metadata(model.join("b.safetensors")).unwrap().len();
        assert_eq!(direct.total_bytes, on_disk);
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
    fn missing_managed_runtime_never_falls_back() {
        let missing = env::temp_dir().join(format!("synth-missing-laguna-runtime-{}", now_ms()));
        let state = LagunaRuntimeState::detect_at(missing.clone());
        assert_eq!(state, LagunaRuntimeState::Missing { expected: missing });
        assert!(state
            .require_ready()
            .unwrap_err()
            .to_string()
            .contains("no alternate interpreter"));
    }

    #[test]
    fn ignores_a_stale_selected_model_path() {
        let stale = env::temp_dir().join(format!(
            "synth-stale-model-{}-{}",
            std::process::id(),
            now_ms()
        ));
        assert!(!stale.exists());
        assert!(selected_model_hit(Some(stale)).unwrap().is_none());
    }

    #[test]
    fn preserves_validation_for_an_existing_broken_selection() {
        let selected = env::temp_dir().join(format!(
            "synth-broken-model-{}-{}",
            std::process::id(),
            now_ms()
        ));
        fs::create_dir_all(&selected).unwrap();
        let error = selected_model_hit(Some(selected.clone()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("neither a supported model directory nor a models root"));
        fs::remove_dir_all(selected).unwrap();
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

    fn live_payload() -> &'static str {
        r#"{
            "model": "poolside/Laguna-XS-2.1-NVFP4-mlx",
            "resident": true,
            "residentBytes": 21568899389,
            "queueDepth": 2,
            "queueCapacity": 8,
            "active": {
                "generationId": "sha256:short-redacted-id",
                "phase": "prefill",
                "queuedAt": 10.125, "startedAt": 20.25, "firstTokenAt": null, "lastTokenAt": null,
                "promptTokens": 12198, "cachedTokens": 0, "outputTokens": 0,
                "cacheHitRatio": 0.0,
                "prefillTokensPerSecond": null, "decodeTokensPerSecond": null,
                "elapsedMs": 31.875
            },
            "rolling": {
                "requestsCompleted": 31, "requestsFailed": 1, "requestsCancelled": 2,
                "inputTokens": 500, "outputTokens": 226, "cachedTokens": 8420,
                "ttftP50Ms": 1840.0, "ttftP95Ms": null,
                "decodeTpsP50": 12.4, "decodeTpsP95": null,
                "latencyP50Ms": null, "latencyP95Ms": null
            }
        }"#
    }

    #[test]
    fn parses_the_pinned_inference_contract() {
        let snapshot: LagunaInference = serde_json::from_str(live_payload()).unwrap();
        assert!(snapshot.resident);
        assert_eq!(snapshot.resident_bytes, Some(21_568_899_389));
        assert_eq!(snapshot.queue_depth, Some(2));
        assert_eq!(snapshot.queue_capacity, Some(8));
        let active = snapshot.active.expect("active generation");
        assert_eq!(active.phase.as_deref(), Some("prefill"));
        assert_eq!(active.queued_at, Some(10.125));
        assert_eq!(active.started_at, Some(20.25));
        assert_eq!(active.elapsed_ms, Some(31.875));
        assert_eq!(active.prompt_tokens, Some(12_198));
        // Null must survive as absent, never as a fabricated zero.
        assert_eq!(active.first_token_at, None);
        assert_eq!(active.decode_tokens_per_second, None);
        assert_eq!(active.cache_hit_ratio, Some(0.0));
        assert_eq!(snapshot.rolling.ttft_p50_ms, Some(1840.0));
        assert_eq!(snapshot.rolling.ttft_p95_ms, None);
        assert_eq!(snapshot.rolling.requests_failed, Some(1));
    }

    #[test]
    fn idle_and_sparse_payloads_deserialize_without_invented_values() {
        let snapshot: LagunaInference =
            serde_json::from_str(r#"{"model":"m","resident":false,"active":null}"#).unwrap();
        assert!(!snapshot.resident);
        assert!(snapshot.active.is_none());
        assert_eq!(snapshot.resident_bytes, None);
        assert_eq!(snapshot.queue_depth, None);
        assert_eq!(snapshot.rolling.requests_completed, None);
    }

    #[test]
    fn inference_round_trips_as_camel_case() {
        let snapshot: LagunaInference = serde_json::from_str(live_payload()).unwrap();
        let value = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(value["residentBytes"], 21_568_899_389u64);
        assert_eq!(value["queueCapacity"], 8);
        assert_eq!(value["active"]["generationId"], "sha256:short-redacted-id");
        assert_eq!(value["active"]["promptTokens"], 12_198);
        assert!(value["active"]["prefillTokensPerSecond"].is_null());
        assert_eq!(value["rolling"]["decodeTpsP50"], 12.4);
        assert!(value["rolling"]["latencyP95Ms"].is_null());
        assert_eq!(
            serde_json::from_value::<LagunaInference>(value).unwrap(),
            snapshot
        );
    }

    #[test]
    fn splits_sse_frames_and_keeps_partial_tails() {
        let mut buffer =
            String::from("event: inference\ndata: {\"a\":1}\n\n: keepalive\n\ndata: {\"b\"");
        let payloads = take_sse_payloads(&mut buffer);
        assert_eq!(payloads, vec!["{\"a\":1}".to_string()]);
        assert_eq!(buffer, "data: {\"b\"");
        buffer.push_str(":2}\n\n");
        assert_eq!(
            take_sse_payloads(&mut buffer),
            vec!["{\"b\":2}".to_string()]
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn joins_multiline_sse_data_and_ignores_empty_frames() {
        assert_eq!(sse_frame_data("data: a\ndata: b"), Some("a\nb".into()));
        assert_eq!(sse_frame_data(": comment only"), None);
        assert_eq!(sse_frame_data("event: ping"), None);
    }

    #[test]
    fn unload_conflict_is_an_outcome_not_an_error() {
        let conflict = unload_outcome(409, r#"{"detail":"generation in flight"}"#).unwrap();
        assert_eq!(
            conflict,
            LagunaUnloadOutcome {
                released: false,
                conflict: true,
                detail: Some("generation in flight".into()),
            }
        );
        let released = unload_outcome(200, "{}").unwrap();
        assert!(released.released && !released.conflict);
        assert!(unload_outcome(500, "").is_err());
    }

    #[test]
    fn conflict_without_a_body_still_explains_itself() {
        let conflict = unload_outcome(409, "").unwrap();
        assert!(conflict.conflict);
        assert!(conflict.detail.is_some_and(|detail| !detail.is_empty()));
    }

    #[test]
    fn daemon_env_pins_7333_and_clears_every_upstream_variable() {
        let mut command = Command::new("/usr/bin/true");
        apply_daemon_env(&mut command, "key", "mlx_lm", Path::new("/models"));
        let envs: Vec<_> = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect();
        let get = |name: &str| {
            envs.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(get("SYNTH_LAGUNA_PORT"), Some(Some("7333".into())));
        assert_eq!(get("SYNTH_LAGUNA_BACKEND"), Some(Some("mlx_lm".into())));
        assert_eq!(
            get("PYTHONDONTWRITEBYTECODE"),
            Some(Some("1".into())),
            "the bundled daemon must not mutate the signed app with __pycache__ files"
        );
        for legacy in UPSTREAM_ENV_VARS {
            assert_eq!(get(legacy), Some(None), "{legacy} must be cleared");
        }
        assert!(
            !envs.iter().any(|(_, value)| value
                .as_deref()
                .is_some_and(|value| value.contains("7334") || value.contains("63300"))),
            "no legacy or Poolside engine port may reach the daemon"
        );
    }

    #[test]
    fn ports_come_from_the_environment_so_instances_do_not_share_a_daemon() {
        // The canonical app uses the default while a named instance overrides
        // it through the environment.
        assert_eq!(DEFAULT_PORT, 7333);
        match env::var("SYNTH_LAGUNA_PORT") {
            Ok(value) => assert_eq!(laguna_port().to_string(), value.trim()),
            Err(_) => assert_eq!(laguna_port(), DEFAULT_PORT),
        }
        let mut command = Command::new("/usr/bin/true");
        apply_daemon_env(&mut command, "key", "mlx_lm", Path::new("/models"));
        assert_eq!(
            command
                .get_envs()
                .find(|(key, _)| key.to_string_lossy() == "SYNTH_LAGUNA_PORT")
                .and_then(|(_, value)| value)
                .map(|value| value.to_string_lossy().into_owned()),
            Some(laguna_port().to_string())
        );
    }

    #[test]
    fn only_our_own_laguna_daemon_may_be_stopped() {
        assert!(is_managed_sidecar_command(
            "/x/.venv/bin/python -m laguna_daemon"
        ));
        // Poolside's sidecar and a stock mlx_lm.server are never ours to kill.
        assert!(!is_managed_sidecar_command(
            "/Applications/Poolside.app/Contents/MacOS/poolside-mlx-sidecar --port 63300"
        ));
        assert!(!is_managed_sidecar_command(
            "python -m mlx_lm.server --port 7334"
        ));
    }
}
