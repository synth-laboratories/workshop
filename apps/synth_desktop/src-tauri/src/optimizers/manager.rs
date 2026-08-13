//! Optimizer sidecar lifecycle, parallel to [`crate::laguna::LagunaManager`].
//!
//! Owns discovery, digest/signature, version pin, process start/stop, health,
//! loopback auth, and recovery. Does **not** own model download/unload, and does
//! **not** own the durable run/event/visual projection — that stays
//! [`super::OptimizerService`]. Stopping or uninstalling a sidecar version must
//! not delete runs, events, visuals, or retained template packages.

use super::models::{
    OptimizerEventEnvelope, OptimizerExecutionBinding, OptimizerRunRecord,
    OPTIMIZER_EVENT_SCHEMA_VERSION,
};
use super::OptimizerService;
use crate::error::AppError;
use crate::ipc::{serve_json, JsonHttpRequest, JsonHttpResponse};
use anyhow::{anyhow, bail, Context, Result};
use hyper::StatusCode;
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::State;
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, Mutex, RwLock};

pub const DEFAULT_SIDECAR_VERSION: &str = "0.2.0";
pub const DEFAULT_ALGORITHM_VERSION: &str = "synth-optimizers-0.2.0";
pub const DEFAULT_RECIPE_SCHEMA_VERSION: &str = "gepa.recipe.v1";
/// Optimizer-family visuals bind this slot. `live` and `jobs` are refused.
pub const OPTIMIZER_VISUAL_SLOT: &str = "optimizer_run";
const SELECTED_VERSION_FILE: &str = "selected_version";
const SIGNING_KEY_FILE: &str = "signing.key";
const API_KEY_FILE: &str = "api_key";
const PAYLOAD_FILE: &str = "payload.json";
const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug)]
struct OptimizerEventRunNotFound {
    run_id: String,
}

impl std::fmt::Display for OptimizerEventRunNotFound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "optimizer event producer has not indexed run `{}`",
            self.run_id
        )
    }
}

impl std::error::Error for OptimizerEventRunNotFound {}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerSidecarStatus {
    pub phase: String,
    pub base_url: Option<String>,
    pub version: Option<String>,
    pub digest: Option<String>,
    pub detail: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerSidecarVersion {
    pub version: String,
    pub digest: String,
    pub signature: String,
    pub algorithm_id: String,
    pub algorithm_version: String,
    pub recipe_schema_version: String,
    pub selected: bool,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerRunPin {
    pub optimizer_run_id: String,
    pub sidecar_version: String,
    pub algorithm_version: String,
    pub recipe_version: String,
    pub digest: String,
    pub spool_path: String,
}

#[derive(Clone, Debug)]
pub struct OptimizerSidecarInstallSpec {
    pub version: String,
    pub algorithm_id: String,
    pub algorithm_version: String,
    pub recipe_schema_version: String,
    pub payload: Value,
    pub template_ids: Vec<String>,
}

impl Default for OptimizerSidecarInstallSpec {
    fn default() -> Self {
        catalog_spec(DEFAULT_SIDECAR_VERSION)
    }
}

fn catalog_spec(version: &str) -> OptimizerSidecarInstallSpec {
    OptimizerSidecarInstallSpec {
        version: version.to_owned(),
        algorithm_id: "gepa".into(),
        algorithm_version: DEFAULT_ALGORITHM_VERSION.into(),
        recipe_schema_version: DEFAULT_RECIPE_SCHEMA_VERSION.into(),
        payload: json!({
            "sidecarVersion": version,
            "algorithms": [{
                "id": "gepa",
                "version": DEFAULT_ALGORITHM_VERSION
            }],
            "recipeSchemaVersion": DEFAULT_RECIPE_SCHEMA_VERSION,
            "templates": ["optimizer.gepa.live.v1", "optimizer.run.v1"],
            "health": true,
            "cancellation": true,
            "eventReplay": true,
        }),
        template_ids: vec!["optimizer.gepa.live.v1".into(), "optimizer.run.v1".into()],
    }
}

struct SidecarRuntime {
    proxy_task: tokio::task::JoinHandle<()>,
    child: Option<Child>,
    upstream_task: Option<tokio::task::JoinHandle<()>>,
    base_url: String,
    api_key: String,
    version: String,
    digest: String,
}

/// One in-process event spool per `optimizer_run_id`. Two campaigns never share
/// this map entry; reading one page does not seal the other.
struct RunSpoolState {
    events: Vec<Value>,
    sealed: bool,
}

pub struct OptimizerManager {
    home: PathBuf,
    status: RwLock<OptimizerSidecarStatus>,
    ensure_lock: Mutex<()>,
    updates: broadcast::Sender<OptimizerSidecarStatus>,
    runtime: Mutex<Option<SidecarRuntime>>,
    /// Concurrent GEPA recipe workers, keyed by run id. Not a singleton.
    /// Process-group leaders for active recipe workers. Tracking only logical
    /// run ids is insufficient: a Tauri exit can outlive the task that owns
    /// the `Child`, leaving `uv` descendants orphaned. Every production worker
    /// is its own process group and the supervisor drains these groups first.
    gepa_workers: Mutex<HashMap<String, GepaWorkerState>>,
    run_spools: Arc<Mutex<HashMap<String, RunSpoolState>>>,
    client: Client,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GepaWorkerState {
    /// The run id is atomically reserved while its process is being spawned.
    Starting,
    /// The isolated process group is owned by this supervisor.
    Running { pid: u32 },
}

impl OptimizerManager {
    pub fn new() -> Self {
        Self::with_home(default_home())
    }

    pub fn with_home(home: PathBuf) -> Self {
        let (updates, _) = broadcast::channel(32);
        Self {
            home,
            status: RwLock::new(OptimizerSidecarStatus {
                phase: "unknown".into(),
                base_url: None,
                version: None,
                digest: None,
                detail: None,
                updated_at: now_ms(),
            }),
            ensure_lock: Mutex::new(()),
            updates,
            runtime: Mutex::new(None),
            gepa_workers: Mutex::new(HashMap::new()),
            run_spools: Arc::new(Mutex::new(HashMap::new())),
            client: crate::http::http_client(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<OptimizerSidecarStatus> {
        self.updates.subscribe()
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// One spool directory per `optimizer_run_id`. Two run ids never share a spool.
    pub fn spool_id(run_id: &str) -> String {
        format!("{OPTIMIZER_EVENT_SCHEMA_VERSION}:{run_id}")
    }

    pub async fn status(&self) -> OptimizerSidecarStatus {
        self.status.read().await.clone()
    }

    /// Read one durable, run-scoped cursor page from the managed sidecar.
    /// Flip-reading campaign A then B does not seal A.
    pub async fn optimizer_events_after(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Value> {
        validate_optimizer_run_id(run_id)?;
        let (base_url, api_key) = {
            let runtime = self.runtime.lock().await;
            let runtime = runtime
                .as_ref()
                .ok_or_else(|| anyhow!("optimizer sidecar is not running"))?;
            (runtime.base_url.clone(), runtime.api_key.clone())
        };
        let response = self
            .client
            .get(format!("{base_url}/runs/{run_id}/optimizer-events"))
            .query(&[
                ("after_sequence", after_sequence.to_string()),
                ("limit", limit.clamp(1, 2_000).to_string()),
            ])
            .bearer_auth(api_key)
            .send()
            .await
            .context("poll managed optimizer event endpoint")?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .context("decode managed optimizer event page")?;
        if status == StatusCode::NOT_FOUND {
            return Err(OptimizerEventRunNotFound {
                run_id: run_id.to_string(),
            }
            .into());
        }
        if !status.is_success() {
            bail!("optimizer event endpoint returned {status}: {body}");
        }
        if body.get("run_id").and_then(Value::as_str) != Some(run_id) {
            bail!("optimizer event endpoint returned a cross-run page");
        }
        if let Some(slot) = body.get("slot").and_then(Value::as_str) {
            if slot == "live" || slot == "jobs" {
                bail!("optimizer event page used forbidden slot `{slot}`");
            }
            if slot != OPTIMIZER_VISUAL_SLOT {
                bail!("optimizer event page slot must be {OPTIMIZER_VISUAL_SLOT}");
            }
        }
        Ok(body)
    }

    pub(crate) fn optimizer_run_not_indexed(error: &anyhow::Error) -> bool {
        error.downcast_ref::<OptimizerEventRunNotFound>().is_some()
    }

    pub async fn set_status(&self, mut status: OptimizerSidecarStatus) {
        status.updated_at = now_ms();
        *self.status.write().await = status.clone();
        let _ = self.updates.send(status);
    }

    /// Discover installed versions, then probe a live sidecar if one is running.
    pub async fn refresh(&self) -> OptimizerSidecarStatus {
        let discovered = self.discover().unwrap_or_default();
        let selected = discovered.iter().find(|hit| hit.selected).cloned();
        if self.runtime.lock().await.is_some() {
            if let Some(probed) = self.probe().await {
                self.set_status(probed).await;
                return self.status().await;
            }
            self.abort_runtime().await;
        }
        let phase = if selected.is_some() {
            "stopped"
        } else if discovered.is_empty() {
            "not_installed"
        } else {
            "installed"
        };
        self.set_status(OptimizerSidecarStatus {
            phase: phase.into(),
            base_url: None,
            version: selected.as_ref().map(|hit| hit.version.clone()),
            digest: selected.as_ref().map(|hit| hit.digest.clone()),
            detail: Some(match phase {
                "not_installed" => "Optimizer sidecar is not installed".into(),
                "stopped" => "Optimizer sidecar is installed and stopped".into(),
                _ => "Optimizer sidecar versions are installed; none selected".into(),
            }),
            updated_at: now_ms(),
        })
        .await;
        self.status().await
    }

    pub fn discover(&self) -> Result<Vec<OptimizerSidecarVersion>> {
        let selected = read_selected_version(&self.home)?;
        let versions_root = self.home.join("versions");
        let Ok(entries) = fs::read_dir(&versions_root) else {
            return Ok(Vec::new());
        };
        let mut hits = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            match load_verified_manifest(&self.home, &path) {
                Ok(mut hit) => {
                    hit.selected = selected.as_deref() == Some(hit.version.as_str());
                    hits.push(hit);
                }
                Err(error) => {
                    eprintln!(
                        "synth-desktop: skip optimizer sidecar at {}: {error:#}",
                        path.display()
                    );
                }
            }
        }
        hits.sort_by(|a, b| a.version.cmp(&b.version));
        Ok(hits)
    }

    pub fn version(&self) -> Result<Option<OptimizerSidecarVersion>> {
        let selected = read_selected_version(&self.home)?;
        Ok(self
            .discover()?
            .into_iter()
            .find(|hit| selected.as_deref() == Some(hit.version.as_str()) || hit.selected))
    }

    pub fn select_version(&self, version: &str) -> Result<OptimizerSidecarVersion> {
        let hit = self
            .discover()?
            .into_iter()
            .find(|hit| hit.version == version)
            .ok_or_else(|| anyhow!("optimizer sidecar version `{version}` is not installed"))?;
        fs::create_dir_all(&self.home)?;
        fs::write(
            self.home.join(SELECTED_VERSION_FILE),
            format!("{version}\n"),
        )?;
        Ok(OptimizerSidecarVersion {
            selected: true,
            ..hit
        })
    }

    pub fn install(&self, version: Option<&str>) -> Result<OptimizerSidecarVersion> {
        let version = version.unwrap_or(DEFAULT_SIDECAR_VERSION);
        if version != DEFAULT_SIDECAR_VERSION {
            bail!("unknown optimizer sidecar version `{version}`");
        }
        self.install_spec(catalog_spec(version))
    }

    pub fn install_spec(
        &self,
        spec: OptimizerSidecarInstallSpec,
    ) -> Result<OptimizerSidecarVersion> {
        validate_version_id(&spec.version)?;
        fs::create_dir_all(&self.home)?;
        let signing_key = ensure_signing_key(&self.home)?;
        let payload = serde_json::to_vec(&spec.payload).context("encode sidecar payload")?;
        let digest = sha256_hex(&payload);
        let signature = sign_manifest(&signing_key, &spec.version, &digest);
        let dir = self.home.join("versions").join(&spec.version);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join(PAYLOAD_FILE), &payload)?;
        let manifest = json!({
            "version": spec.version,
            "digest": digest,
            "signature": signature,
            "algorithmId": spec.algorithm_id,
            "algorithmVersion": spec.algorithm_version,
            "recipeSchemaVersion": spec.recipe_schema_version,
            "templates": spec.template_ids,
        });
        fs::write(
            dir.join(MANIFEST_FILE),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        for template_id in &spec.template_ids {
            retain_template_package(&self.home, template_id, &spec.version, &digest)?;
        }
        self.select_version(&spec.version)
    }

    pub async fn start(&self) -> Result<OptimizerSidecarStatus> {
        let _guard = self.ensure_lock.lock().await;
        let selected = read_selected_version(&self.home)?
            .ok_or_else(|| anyhow!("Optimizer sidecar is not installed"))?;
        let dir = self.home.join("versions").join(&selected);
        let hit = load_verified_manifest(&self.home, &dir)?;
        if let Some(status) = self.probe().await {
            if status.phase == "ready" && status.version.as_deref() == Some(hit.version.as_str()) {
                self.set_status(status).await;
                return Ok(self.status().await);
            }
            self.abort_runtime().await;
        }
        self.set_status(OptimizerSidecarStatus {
            phase: "starting".into(),
            base_url: None,
            version: Some(hit.version.clone()),
            digest: Some(hit.digest.clone()),
            detail: Some("Starting optimizer sidecar…".into()),
            updated_at: now_ms(),
        })
        .await;
        let api_key = ensure_api_key(&self.home)?;
        let (child, upstream_base_url, upstream_task) =
            launch_sidecar_upstream(&self.home, &hit, self.run_spools.clone()).await?;
        let listener = tokio::net::TcpListener::bind(bind_addr())
            .await
            .context("bind optimizer sidecar auth proxy")?;
        let addr = listener.local_addr()?;
        let base_url = format!("http://{addr}");
        let health = health_body(&hit);
        let serve_key = api_key.clone();
        let proxy_client = self.client.clone();
        let proxy_task = tokio::spawn(async move {
            let result = serve_json(listener, move |request| {
                let token = serve_key.clone();
                let body = health.clone();
                let client = proxy_client.clone();
                let upstream = upstream_base_url.clone();
                async move { route_sidecar(request, &token, &upstream, &client, body).await }
            })
            .await;
            if let Err(error) = result {
                eprintln!("synth-desktop: optimizer auth proxy stopped: {error:#}");
            }
        });
        *self.runtime.lock().await = Some(SidecarRuntime {
            proxy_task,
            child,
            upstream_task,
            base_url: base_url.clone(),
            api_key: api_key.clone(),
            version: hit.version.clone(),
            digest: hit.digest.clone(),
        });
        write_env_sh(&self.home, &api_key, &base_url, &hit.version)?;
        let deadline = tokio::time::Instant::now() + crate::limits::OPTIMIZER_SIDECAR_READY_WAIT;
        while tokio::time::Instant::now() < deadline {
            if let Some(status) = self.probe().await {
                if status.phase == "ready" {
                    self.set_status(status).await;
                    return Ok(self.status().await);
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.abort_runtime().await;
        self.set_status(OptimizerSidecarStatus {
            phase: "error".into(),
            base_url: Some(base_url.clone()),
            version: Some(hit.version),
            digest: Some(hit.digest),
            detail: Some(format!(
                "Timed out waiting for optimizer sidecar at {base_url}"
            )),
            updated_at: now_ms(),
        })
        .await;
        bail!("Timed out waiting for optimizer sidecar at {base_url}");
    }

    /// Install the product-pinned sidecar when needed and return only after
    /// the real optimizer service (behind the authenticated loopback proxy) is
    /// healthy. Recipe entry points use this rather than spawning a package on
    /// their own.
    pub async fn ensure_ready(&self) -> Result<OptimizerSidecarStatus> {
        if self.version()?.is_none() {
            self.install(None)?;
        }
        self.start().await
    }

    /// Spawn the one allowlisted local GEPA recipe command. Callers provide
    /// only product-resolved paths and the one allowlisted credential; package
    /// version, executable, and subcommand stay owned by this manager.
    ///
    /// Two `optimizer_run_id`s may be live at once. This is a map of workers,
    /// not a singleton that would serialize campaigns.
    pub async fn spawn_gepa_recipe(
        &self,
        run_id: &str,
        cookbook: &Path,
        config_path: &Path,
        stdout: fs::File,
        stderr: fs::File,
        openai_api_key: &str,
    ) -> Result<Child> {
        validate_optimizer_run_id(run_id)?;
        if self.runtime.lock().await.is_none() {
            bail!(
                "optimizer sidecar is not running; call ensure_ready before spawning a GEPA recipe"
            );
        }
        let selected = self
            .version()?
            .ok_or_else(|| anyhow!("Optimizer sidecar is not installed"))?;
        {
            use std::collections::hash_map::Entry;
            let mut workers = self.gepa_workers.lock().await;
            match workers.entry(run_id.to_string()) {
                Entry::Vacant(entry) => {
                    entry.insert(GepaWorkerState::Starting);
                }
                Entry::Occupied(_) => bail!("GEPA recipe for `{run_id}` is already running"),
            }
        }
        self.ensure_memory_spool(run_id).await;
        match launch_gepa_recipe_process(
            &selected.version,
            cookbook,
            config_path,
            stdout,
            stderr,
            openai_api_key,
        ) {
            Ok(mut child) => {
                let pid = child
                    .id()
                    .ok_or_else(|| anyhow!("spawned GEPA recipe omitted its process id"))?;
                let promoted = {
                    let mut workers = self.gepa_workers.lock().await;
                    match workers.get_mut(run_id) {
                        Some(state @ GepaWorkerState::Starting) => {
                            *state = GepaWorkerState::Running { pid };
                            true
                        }
                        Some(GepaWorkerState::Running { .. }) | None => false,
                    }
                };
                if promoted {
                    Ok(child)
                } else {
                    terminate_child(&mut child).await;
                    bail!("GEPA supervisor drained `{run_id}` while its process was starting")
                }
            }
            Err(error) => {
                self.gepa_workers.lock().await.remove(run_id);
                Err(error)
            }
        }
    }

    pub async fn release_gepa_recipe(&self, run_id: &str) {
        // A recipe leader can exit while descendants remain in its process
        // group. Releasing ownership must reap that whole group, not merely
        // forget the pid and orphan paid work.
        self.terminate_gepa_recipe(run_id).await;
    }

    pub async fn terminate_gepa_recipe(&self, run_id: &str) {
        let state = self.gepa_workers.lock().await.remove(run_id);
        if let Some(GepaWorkerState::Running { pid }) = state {
            terminate_process_groups(&[pid]).await;
        }
    }

    pub async fn active_gepa_run_ids(&self) -> Vec<String> {
        let mut ids = self
            .gepa_workers
            .lock()
            .await
            .iter()
            .filter_map(|(run_id, state)| {
                matches!(state, GepaWorkerState::Running { .. }).then(|| run_id.clone())
            })
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }

    /// Append one producer event to the in-process spool for `run_id`.
    /// Reading another run's page does not seal this spool.
    pub async fn append_spool_event(&self, run_id: &str, event: Value) -> Result<()> {
        validate_optimizer_run_id(run_id)?;
        let event_run = event
            .get("run_id")
            .or_else(|| event.get("optimizer_run_id"))
            .and_then(Value::as_str);
        if event_run.is_some_and(|value| value != run_id) {
            bail!("refusing to append a cross-run event onto spool {run_id}");
        }
        if event
            .get("sequence_number")
            .or_else(|| event.get("sequenceNumber"))
            .and_then(Value::as_u64)
            .is_none()
        {
            bail!("optimizer spool event omitted sequence_number");
        }
        let mut spools = self.run_spools.lock().await;
        let spool = spools.entry(run_id.to_string()).or_insert(RunSpoolState {
            events: Vec::new(),
            sealed: false,
        });
        if spool.sealed {
            bail!("optimizer spool {run_id} is sealed");
        }
        spool.events.push(event);
        Ok(())
    }

    async fn ensure_memory_spool(&self, run_id: &str) {
        let mut spools = self.run_spools.lock().await;
        spools.entry(run_id.to_string()).or_insert(RunSpoolState {
            events: Vec::new(),
            sealed: false,
        });
    }

    pub async fn stop(&self) -> Result<OptimizerSidecarStatus> {
        let _guard = self.ensure_lock.lock().await;
        self.abort_runtime().await;
        Ok(self.refresh().await)
    }

    pub async fn uninstall(
        &self,
        version: &str,
        service: &OptimizerService,
    ) -> Result<OptimizerSidecarStatus> {
        validate_version_id(version)?;
        if let Some(active) = active_run_for_version(service, version).await? {
            bail!(
                "cannot uninstall optimizer sidecar `{version}` while it owns active run {}",
                active.id
            );
        }
        let _guard = self.ensure_lock.lock().await;
        let selected = read_selected_version(&self.home)?;
        if selected.as_deref() == Some(version) {
            self.abort_runtime().await;
        }
        let dir = self.home.join("versions").join(version);
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .with_context(|| format!("remove optimizer sidecar {}", dir.display()))?;
        }
        if selected.as_deref() == Some(version) {
            let _ = fs::remove_file(self.home.join(SELECTED_VERSION_FILE));
        }
        Ok(self.refresh().await)
    }

    /// Record sidecar × algorithm × recipe identity on a run and open its spool.
    /// Does not start compute; recipes remain the allowlisted worker boundary.
    pub async fn pin_run(
        &self,
        service: &OptimizerService,
        run_id: &str,
        recipe_version: &str,
    ) -> Result<(OptimizerRunRecord, OptimizerRunPin)> {
        let hit = self
            .version()?
            .ok_or_else(|| anyhow!("Optimizer sidecar is not installed"))?;
        let mut run = service.get(run_id.to_string()).await?;
        if run.algorithm_version.is_none() {
            run.algorithm_version = Some(hit.algorithm_version.clone());
        }
        let pin = OptimizerRunPin {
            optimizer_run_id: run.id.clone(),
            sidecar_version: hit.version.clone(),
            algorithm_version: run
                .algorithm_version
                .clone()
                .unwrap_or_else(|| hit.algorithm_version.clone()),
            recipe_version: recipe_version.to_owned(),
            digest: hit.digest.clone(),
            spool_path: String::new(),
        };
        let pin = self.open_spool(&pin)?;
        self.ensure_memory_spool(&pin.optimizer_run_id).await;
        merge_pin_into_run(&mut run, &pin);
        service.persist_run(run.clone()).await?;
        let event = OptimizerEventEnvelope {
            schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: Some(format!("{}:sidecar-pin", run.id)),
            event_type: "optimizer.run.pinned".into(),
            sequence_number: run.cursor_seq + 1,
            occurred_at: chrono::Utc::now().to_rfc3339(),
            optimizer_run_id: run.id.clone(),
            algorithm_id: run.algorithm_id.clone(),
            level: Some("info".into()),
            item: None,
            delta: json!({
                "sidecarVersion": pin.sidecar_version,
                "algorithmVersion": pin.algorithm_version,
                "recipeVersion": pin.recipe_version,
                "sidecarDigest": pin.digest,
                "spoolPath": pin.spool_path,
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
            snapshot: Some(
                json!({ "summary": run.summary.clone() })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            ),
            usage_delta: None,
            artifact_refs: vec![],
            error: None,
            raw: json!({
                "sidecarVersion": pin.sidecar_version,
                "algorithmVersion": pin.algorithm_version,
                "recipeVersion": pin.recipe_version
            }),
        };
        let (run, _) = service.append_events(run.id.clone(), vec![event]).await?;
        Ok((run, pin))
    }

    pub fn open_spool(&self, pin: &OptimizerRunPin) -> Result<OptimizerRunPin> {
        let spool = self.home.join("spools").join(&pin.optimizer_run_id);
        fs::create_dir_all(&spool)?;
        let identity = json!({
            "optimizerRunId": pin.optimizer_run_id,
            "spoolId": Self::spool_id(&pin.optimizer_run_id),
            "sidecarVersion": pin.sidecar_version,
            "algorithmVersion": pin.algorithm_version,
            "recipeVersion": pin.recipe_version,
            "digest": pin.digest,
        });
        fs::write(
            spool.join("identity.json"),
            serde_json::to_vec_pretty(&identity)?,
        )?;
        Ok(OptimizerRunPin {
            spool_path: spool.display().to_string(),
            ..pin.clone()
        })
    }

    pub fn retained_template_path(&self, template_id: &str, sidecar_version: &str) -> PathBuf {
        template_package_dir(&self.home, template_id, sidecar_version)
    }

    async fn probe(&self) -> Option<OptimizerSidecarStatus> {
        let (base_url, api_key, version, digest) = {
            let mut runtime = self.runtime.lock().await;
            let runtime = runtime.as_mut()?;
            if let Some(child) = runtime.child.as_mut() {
                if child.try_wait().ok().flatten().is_some() {
                    return None;
                }
            }
            (
                runtime.base_url.clone(),
                runtime.api_key.clone(),
                runtime.version.clone(),
                runtime.digest.clone(),
            )
        };
        let response = self
            .client
            .get(format!("{base_url}/health"))
            .bearer_auth(&api_key)
            .timeout(crate::limits::OPTIMIZER_SIDECAR_HEALTH_TIMEOUT)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body: Value = response.json().await.ok()?;
        if body.get("status").and_then(Value::as_str) != Some("ok") {
            return None;
        }
        Some(OptimizerSidecarStatus {
            phase: "ready".into(),
            base_url: Some(base_url),
            version: Some(version),
            digest: Some(digest),
            detail: Some("Optimizer sidecar ready".into()),
            updated_at: now_ms(),
        })
    }

    async fn abort_runtime(&self) {
        let worker_pids = self
            .gepa_workers
            .lock()
            .await
            .drain()
            .filter_map(|(_, state)| match state {
                GepaWorkerState::Starting => None,
                GepaWorkerState::Running { pid } => Some(pid),
            })
            .collect::<Vec<_>>();
        terminate_process_groups(&worker_pids).await;
        if let Some(mut runtime) = self.runtime.lock().await.take() {
            runtime.proxy_task.abort();
            if let Some(child) = runtime.child.as_mut() {
                terminate_child(child).await;
            }
            if let Some(task) = runtime.upstream_task.take() {
                task.abort();
            }
        }
    }
}

impl Default for OptimizerManager {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::services::ManagedService for OptimizerManager {
    fn name(&self) -> &'static str {
        "optimizer"
    }

    fn stop(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let _ = OptimizerManager::stop(self).await;
            Ok(())
        })
    }
}

fn default_home() -> PathBuf {
    env::var_os("SYNTH_OPTIMIZER_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::instance::state_root().join("optimizers"))
}

fn bind_addr() -> std::net::SocketAddr {
    let port = env::var("SYNTH_OPTIMIZER_PORT")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0u16);
    std::net::SocketAddr::from(([127, 0, 0, 1], port))
}

fn resolve_uv() -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("SYNTH_OPTIMIZER_UV_PATH") {
        candidates.push(PathBuf::from(path));
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/uv"),
        PathBuf::from("/usr/local/bin/uv"),
    ]);
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin/uv"));
        candidates.push(home.join(".cargo/bin/uv"));
    }
    for path in candidates {
        if path.is_file() {
            return path
                .canonicalize()
                .with_context(|| format!("canonicalize trusted uv path {}", path.display()));
        }
    }
    bail!(
        "Optimizer sidecar requires uv; install it or set SYNTH_OPTIMIZER_UV_PATH in the Desktop process"
    )
}

fn optimizer_project_root() -> Result<Option<PathBuf>> {
    let Some(path) = env::var_os("SYNTH_OPTIMIZER_PROJECT_ROOT").map(PathBuf::from) else {
        return Ok(None);
    };
    let path = path
        .canonicalize()
        .with_context(|| format!("canonicalize optimizer project root {}", path.display()))?;
    let manifest = fs::read_to_string(path.join("pyproject.toml"))
        .context("read optimizer project pyproject.toml")?;
    if !manifest.lines().any(|line| {
        let normalized = line.split('#').next().unwrap_or("").trim();
        normalized == "name = \"synth-optimizers\""
            || normalized == "name='synth-optimizers'"
            || normalized == "name = 'synth-optimizers'"
    }) {
        bail!("SYNTH_OPTIMIZER_PROJECT_ROOT must identify the synth-optimizers project");
    }
    Ok(Some(path))
}

fn optimizer_command(version: &str) -> Result<Command> {
    validate_version_id(version)?;
    let uv = resolve_uv()?;
    let mut command = Command::new(uv);
    if let Some(project) = optimizer_project_root()? {
        command.args(["run", "--project"]).arg(project);
    } else {
        command.args([
            "run",
            "--no-project",
            "--with",
            &format!("synth-optimizers=={version}"),
        ]);
    }
    command.arg("synth-optimizers");
    Ok(command)
}

fn launch_gepa_recipe_process(
    version: &str,
    cookbook: &Path,
    config_path: &Path,
    stdout: fs::File,
    stderr: fs::File,
    openai_api_key: &str,
) -> Result<Child> {
    #[cfg(test)]
    {
        if env::var("SYNTH_OPTIMIZER_LIVE_SIDECAR").as_deref() != Ok("1") {
            let _ = (version, cookbook, openai_api_key, config_path);
            let mut command = Command::new("/usr/bin/true");
            command
                .stdin(Stdio::null())
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(stderr))
                .kill_on_drop(true);
            return command
                .spawn()
                .context("launch in-process GEPA recipe stand-in");
        }
    }
    let mut command = optimizer_command(version)?;
    isolate_process_group(&mut command);
    command
        .args(["gepa", "run", "--config"])
        .arg(config_path)
        .current_dir(cookbook)
        .env("OPENAI_API_KEY", openai_api_key)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true);
    command
        .spawn()
        .context("launch Desktop-managed Banking77 GEPA recipe")
}

async fn launch_sidecar_upstream(
    home: &Path,
    hit: &OptimizerSidecarVersion,
    run_spools: Arc<Mutex<HashMap<String, RunSpoolState>>>,
) -> Result<(Option<Child>, String, Option<tokio::task::JoinHandle<()>>)> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .context("reserve optimizer service loopback")?;
    let addr = listener.local_addr()?;
    let upstream_base_url = format!("http://{addr}");

    #[cfg(test)]
    {
        if env::var("SYNTH_OPTIMIZER_LIVE_SIDECAR").as_deref() != Ok("1") {
            let _ = (home, hit);
            let body = json!({"status":"ok"});
            let task = tokio::spawn(async move {
                let _ = serve_json(listener, move |request| {
                    let body = body.clone();
                    let run_spools = run_spools.clone();
                    async move {
                        let path = request.path.split('?').next().unwrap_or(&request.path);
                        if request.method == hyper::Method::GET && path == "/health" {
                            JsonHttpResponse::ok(body)
                        } else if request.method == hyper::Method::GET
                            && path.starts_with("/runs/")
                            && path.ends_with("/optimizer-events")
                        {
                            serve_in_process_spool_page(&run_spools, &request.path).await
                        } else {
                            JsonHttpResponse::error(StatusCode::NOT_FOUND, "not found")
                        }
                    }
                })
                .await;
            });
            return Ok((None, upstream_base_url, Some(task)));
        }
    }
    let _ = run_spools;

    drop(listener);
    let runtime_dir = home.join("runtime");
    fs::create_dir_all(&runtime_dir)?;
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("sidecar.log"))?;
    let mut command = optimizer_command(&hit.version)?;
    isolate_process_group(&mut command);
    command
        .args(["gepa", "service", "--db"])
        .arg(runtime_dir.join("gepa.sqlite"))
        .arg("--bind")
        .arg(addr.to_string())
        .env("SYNTH_OPTIMIZER_API_KEY", ensure_api_key(home)?)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .kill_on_drop(true);
    let child = command
        .spawn()
        .context("spawn allowlisted synth-optimizers GEPA service")?;
    Ok((Some(child), upstream_base_url, None))
}

#[cfg(unix)]
fn isolate_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
}

#[cfg(not(unix))]
fn isolate_process_group(_command: &mut Command) {}

#[cfg(unix)]
async fn terminate_process_groups(pids: &[u32]) {
    for &pid in pids {
        // Negative pid addresses the entire process group created at spawn.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
    }
    if !pids.is_empty() {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    for &pid in pids {
        unsafe {
            if libc::kill(-(pid as i32), 0) == 0 {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}

#[cfg(not(unix))]
async fn terminate_process_groups(_pids: &[u32]) {}

async fn terminate_child(child: &mut Child) {
    if let Some(pid) = child.id() {
        terminate_process_groups(&[pid]).await;
    }
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

fn validate_version_id(version: &str) -> Result<()> {
    if version.is_empty()
        || version.len() > 64
        || version == "."
        || version == ".."
        || Path::new(version).components().count() != 1
    {
        bail!("invalid optimizer sidecar version `{version}`");
    }
    if !version
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    {
        bail!("invalid optimizer sidecar version `{version}`");
    }
    Ok(())
}

fn validate_optimizer_run_id(run_id: &str) -> Result<()> {
    if run_id.is_empty()
        || !run_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        bail!("invalid optimizer run id");
    }
    Ok(())
}

#[cfg(test)]
async fn serve_in_process_spool_page(
    run_spools: &Mutex<HashMap<String, RunSpoolState>>,
    path_and_query: &str,
) -> JsonHttpResponse {
    let Some((run_id, after_sequence, limit)) = parse_optimizer_events_request(path_and_query)
    else {
        return JsonHttpResponse::error(StatusCode::NOT_FOUND, "not found");
    };
    let spools = run_spools.lock().await;
    let Some(spool) = spools.get(&run_id) else {
        return JsonHttpResponse::error(StatusCode::NOT_FOUND, "not found");
    };
    let events = spool
        .events
        .iter()
        .filter(|event| {
            event
                .get("sequence_number")
                .or_else(|| event.get("sequenceNumber"))
                .and_then(Value::as_u64)
                .is_some_and(|sequence| sequence > after_sequence)
        })
        .take(limit)
        .cloned()
        .collect::<Vec<_>>();
    let next_sequence = events
        .last()
        .and_then(|event| {
            event
                .get("sequence_number")
                .or_else(|| event.get("sequenceNumber"))
                .and_then(Value::as_u64)
        })
        .unwrap_or(after_sequence);
    JsonHttpResponse::ok(json!({
        "schema_version": "optimizer_event_page.v1",
        "run_id": run_id,
        "after_sequence": after_sequence,
        "next_sequence": next_sequence,
        "terminal": spool.sealed,
        "slot": OPTIMIZER_VISUAL_SLOT,
        "events": events,
    }))
}

#[cfg(test)]
fn parse_optimizer_events_request(path_and_query: &str) -> Option<(String, u64, usize)> {
    let (path, query) = path_and_query
        .split_once('?')
        .unwrap_or((path_and_query, ""));
    let rest = path.strip_prefix("/runs/")?;
    let run_id = rest.strip_suffix("/optimizer-events")?;
    if run_id.is_empty() {
        return None;
    }
    let mut after_sequence = 0u64;
    let mut limit = 500usize;
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "after_sequence" => {
                after_sequence = value.parse().unwrap_or(after_sequence);
            }
            "limit" => {
                limit = value.parse().ok().unwrap_or(limit).clamp(1, 2_000);
            }
            _ => {}
        }
    }
    Some((run_id.to_string(), after_sequence, limit))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        let hashed = Sha256::digest(key);
        key_block[..hashed.len()].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    outer.finalize().into()
}

fn sign_manifest(key: &[u8], version: &str, digest: &str) -> String {
    let mac = hmac_sha256(key, format!("{version}\n{digest}").as_bytes());
    format!(
        "synth-local-hmac:{}",
        mac.iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn ensure_signing_key(home: &Path) -> Result<Vec<u8>> {
    let path = home.join(SIGNING_KEY_FILE);
    if let Ok(existing) = fs::read(&path) {
        if !existing.is_empty() {
            return Ok(existing);
        }
    }
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    write_secret(&path, &bytes, false)?;
    Ok(bytes.to_vec())
}

fn ensure_api_key(home: &Path) -> Result<String> {
    let path = home.join(API_KEY_FILE);
    if let Ok(existing) = fs::read_to_string(&path) {
        let existing = existing.trim();
        if !existing.is_empty() {
            return Ok(existing.to_owned());
        }
    }
    if let Ok(value) = env::var("SYNTH_OPTIMIZER_API_KEY") {
        if !value.trim().is_empty() {
            write_secret(&path, value.as_bytes(), true)?;
            return Ok(value);
        }
    }
    let mut bytes = [0u8; 24];
    rand::rng().fill_bytes(&mut bytes);
    let key = format!(
        "synth-opt-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    write_secret(&path, key.as_bytes(), true)?;
    Ok(key)
}

fn write_secret(path: &Path, value: &[u8], newline: bool) -> Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(value)?;
    if newline && !value.ends_with(b"\n") {
        file.write_all(b"\n")?;
    }
    Ok(())
}

fn write_env_sh(home: &Path, api_key: &str, base_url: &str, version: &str) -> Result<()> {
    fs::create_dir_all(home)?;
    let body = format!(
        "export SYNTH_OPTIMIZER_HOST=\"127.0.0.1\"\nexport SYNTH_OPTIMIZER_BASE_URL=\"{base_url}\"\nexport SYNTH_OPTIMIZER_API_KEY=\"{api_key}\"\nexport SYNTH_OPTIMIZER_VERSION=\"{version}\"\n"
    );
    fs::write(home.join("env.sh"), body)?;
    Ok(())
}

fn read_selected_version(home: &Path) -> Result<Option<String>> {
    match fs::read_to_string(home.join(SELECTED_VERSION_FILE)) {
        Ok(value) => {
            let value = value.trim();
            Ok((!value.is_empty()).then(|| value.to_owned()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn load_verified_manifest(home: &Path, dir: &Path) -> Result<OptimizerSidecarVersion> {
    let manifest: Value = serde_json::from_slice(
        &fs::read(dir.join(MANIFEST_FILE)).context("read sidecar manifest")?,
    )?;
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("sidecar manifest missing version"))?
        .to_owned();
    let digest = manifest
        .get("digest")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("sidecar manifest missing digest"))?
        .to_owned();
    let signature = manifest
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("sidecar manifest missing signature"))?
        .to_owned();
    let payload = fs::read(dir.join(PAYLOAD_FILE)).context("read sidecar payload")?;
    let actual = sha256_hex(&payload);
    if actual != digest {
        bail!("optimizer sidecar `{version}` digest mismatch");
    }
    let key = ensure_signing_key(home)?;
    let expected = sign_manifest(&key, &version, &digest);
    if expected != signature {
        bail!("optimizer sidecar `{version}` signature mismatch");
    }
    Ok(OptimizerSidecarVersion {
        version,
        digest,
        signature,
        algorithm_id: manifest
            .get("algorithmId")
            .and_then(Value::as_str)
            .unwrap_or("gepa")
            .to_owned(),
        algorithm_version: manifest
            .get("algorithmVersion")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_ALGORITHM_VERSION)
            .to_owned(),
        recipe_schema_version: manifest
            .get("recipeSchemaVersion")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_RECIPE_SCHEMA_VERSION)
            .to_owned(),
        selected: false,
        path: dir.display().to_string(),
    })
}

fn template_package_dir(home: &Path, template_id: &str, sidecar_version: &str) -> PathBuf {
    home.join("templates")
        .join(format!("{template_id}@{sidecar_version}"))
}

fn retain_template_package(
    home: &Path,
    template_id: &str,
    sidecar_version: &str,
    digest: &str,
) -> Result<()> {
    let dir = template_package_dir(home, template_id, sidecar_version);
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join("package.json"),
        serde_json::to_vec_pretty(&json!({
            "templateId": template_id,
            "sidecarVersion": sidecar_version,
            "digest": digest,
            "retained": true,
        }))?,
    )?;
    Ok(())
}

fn health_body(hit: &OptimizerSidecarVersion) -> Value {
    json!({
        "status": "ok",
        "version": hit.version,
        "digest": hit.digest,
        "algorithmId": hit.algorithm_id,
        "algorithmVersion": hit.algorithm_version,
        "recipeSchemaVersion": hit.recipe_schema_version,
    })
}

async fn route_sidecar(
    request: JsonHttpRequest,
    token: &str,
    upstream_base_url: &str,
    client: &Client,
    mut health: Value,
) -> JsonHttpResponse {
    let auth = request
        .authorization
        .as_deref()
        .and_then(|value| value.strip_prefix("Bearer ").map(str::trim));
    if auth != Some(token) {
        return JsonHttpResponse::error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let path = request
        .path
        .split('?')
        .next()
        .unwrap_or(request.path.as_str());
    match (request.method.as_str(), path) {
        ("GET", "/health") | ("GET", "/v1/optimizer/capabilities") => {
            let upstream = client
                .get(format!("{upstream_base_url}/health"))
                .timeout(crate::limits::OPTIMIZER_SIDECAR_HEALTH_TIMEOUT)
                .send()
                .await;
            let Ok(upstream) = upstream else {
                return JsonHttpResponse::error(
                    StatusCode::BAD_GATEWAY,
                    "optimizer service is unavailable",
                );
            };
            if !upstream.status().is_success() {
                return JsonHttpResponse::error(
                    StatusCode::BAD_GATEWAY,
                    "optimizer service health check failed",
                );
            }
            let Ok(upstream_body) = upstream.json::<Value>().await else {
                return JsonHttpResponse::error(
                    StatusCode::BAD_GATEWAY,
                    "optimizer service returned invalid health data",
                );
            };
            if upstream_body.get("status").and_then(Value::as_str) != Some("ok") {
                return JsonHttpResponse::error(
                    StatusCode::BAD_GATEWAY,
                    "optimizer service is not ready",
                );
            }
            if let Some(object) = health.as_object_mut() {
                object.insert("service".into(), upstream_body);
                object.insert("processOwned".into(), Value::Bool(true));
            }
            JsonHttpResponse::ok(health)
        }
        ("GET", path) if path.starts_with("/runs/") && path.ends_with("/optimizer-events") => {
            let upstream = client
                .get(format!("{upstream_base_url}{}", request.path))
                .timeout(crate::limits::OPTIMIZER_SIDECAR_HEALTH_TIMEOUT)
                .send()
                .await;
            let Ok(upstream) = upstream else {
                return JsonHttpResponse::error(
                    StatusCode::BAD_GATEWAY,
                    "optimizer event endpoint is unavailable",
                );
            };
            let status = upstream.status();
            let Ok(body) = upstream.json::<Value>().await else {
                return JsonHttpResponse::error(
                    StatusCode::BAD_GATEWAY,
                    "optimizer event endpoint returned invalid JSON",
                );
            };
            if status.is_success() {
                JsonHttpResponse::ok(body)
            } else {
                JsonHttpResponse::error(status, body.to_string())
            }
        }
        _ => JsonHttpResponse::error(StatusCode::NOT_FOUND, "not found"),
    }
}

fn merge_pin_into_run(run: &mut OptimizerRunRecord, pin: &OptimizerRunPin) {
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert("sidecarVersion".into(), json!(pin.sidecar_version));
    summary.insert("algorithmVersion".into(), json!(pin.algorithm_version));
    summary.insert("recipeVersion".into(), json!(pin.recipe_version));
    summary.insert("sidecarDigest".into(), json!(pin.digest));
    summary.insert("spoolPath".into(), json!(pin.spool_path));
    run.summary = Value::Object(summary);
    run.algorithm_version = Some(pin.algorithm_version.clone());
    let binding = OptimizerExecutionBinding {
        kind: "optimizer_sidecar".into(),
        id: pin.sidecar_version.clone(),
        label: Some("Desktop-managed optimizer sidecar".into()),
        status: Some("pinned".into()),
        metadata: json!({
            "sidecarVersion": pin.sidecar_version,
            "algorithmVersion": pin.algorithm_version,
            "recipeVersion": pin.recipe_version,
            "digest": pin.digest,
            "spoolPath": pin.spool_path,
        }),
    };
    run.execution_bindings
        .retain(|existing| existing.kind != "optimizer_sidecar");
    run.execution_bindings.push(binding);
}

async fn active_run_for_version(
    service: &OptimizerService,
    version: &str,
) -> Result<Option<OptimizerRunRecord>> {
    let runs = service
        .list(super::models::OptimizerQuery {
            limit: Some(500),
            ..Default::default()
        })
        .await?;
    Ok(runs.into_iter().find(|run| {
        run.summary.get("sidecarVersion").and_then(Value::as_str) == Some(version)
            && matches!(
                run.status.as_str(),
                "queued" | "starting" | "running" | "paused"
            )
    }))
}

#[tauri::command]
#[specta::specta]
pub async fn optimizer_sidecar_status(
    state: State<'_, Arc<OptimizerManager>>,
) -> Result<OptimizerSidecarStatus, AppError> {
    Ok(state.refresh().await)
}

#[tauri::command]
#[specta::specta]
pub async fn optimizer_sidecar_install(
    state: State<'_, Arc<OptimizerManager>>,
    version: Option<String>,
) -> Result<OptimizerSidecarVersion, AppError> {
    state.install(version.as_deref()).map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn optimizer_sidecar_start(
    state: State<'_, Arc<OptimizerManager>>,
) -> Result<OptimizerSidecarStatus, AppError> {
    state.start().await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn optimizer_sidecar_stop(
    state: State<'_, Arc<OptimizerManager>>,
) -> Result<OptimizerSidecarStatus, AppError> {
    state.stop().await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn optimizer_sidecar_version(
    state: State<'_, Arc<OptimizerManager>>,
) -> Result<Option<OptimizerSidecarVersion>, AppError> {
    state.version().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn optimizer_sidecar_uninstall(
    state: State<'_, Arc<OptimizerManager>>,
    core: State<'_, Arc<crate::CoreRuntime>>,
    version: String,
) -> Result<OptimizerSidecarStatus, AppError> {
    state
        .uninstall(&version, core.optimizers())
        .await
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::super::models::OptimizerCreateRequest;
    use super::*;
    use crate::storage::{ContentStore, EventJournal, Storage};
    use crate::visuals::VisualRegistry;

    async fn service() -> (
        OptimizerService,
        tempfile::TempDir,
        tokio::sync::broadcast::Receiver<crate::storage::AppEvent>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path().join("core")).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        let content = ContentStore::new(storage.content_root());
        let visuals = VisualRegistry::new(storage.database().clone(), journal.clone(), content);
        let (events_tx, events_rx) = tokio::sync::broadcast::channel(16);
        (
            OptimizerService::new(storage.database().clone(), journal, visuals, events_tx),
            dir,
            events_rx,
        )
    }

    fn manager() -> (OptimizerManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (OptimizerManager::with_home(dir.path().to_path_buf()), dir)
    }

    async fn seed_run(service: &OptimizerService) -> OptimizerRunRecord {
        seed_named_run(service, "opt_w2_lifecycle").await
    }

    async fn seed_named_run(service: &OptimizerService, id: &str) -> OptimizerRunRecord {
        service
            .create(OptimizerCreateRequest {
                algorithm_id: "gepa".into(),
                algorithm_version: Some(DEFAULT_ALGORITHM_VERSION.into()),
                objective: Some("W2 sidecar lifecycle".into()),
                source: Some("local".into()),
                project_ref: None,
                session_ref: None,
                id: Some(id.into()),
                execution_bindings: None,
                input_refs: None,
                capabilities: None,
                summary: Some(json!({ "task": "banking77" })),
                open_visual: Some(true),
                seed_fixture: None,
                cloud_config: None,
                local_path: None,
            })
            .await
            .unwrap()
            .0
    }

    fn gepa_spool_event(
        run_id: &str,
        sequence: u64,
        event_type: &str,
        delta: Value,
        usage_delta: Option<Value>,
    ) -> Value {
        let mut event = json!({
            "schema_version": "optimizer_event.v1",
            "type": event_type,
            "sequence_number": sequence,
            "created_at": "2026-08-12T20:00:00Z",
            "run_id": run_id,
            "algorithm_id": "gepa",
            "slot": OPTIMIZER_VISUAL_SLOT,
            "delta": delta,
        });
        if let Some(usage) = usage_delta {
            event["usage_delta"] = usage;
        }
        event
    }

    #[test]
    fn two_run_ids_never_share_a_spool() {
        let a = OptimizerManager::spool_id("opt_run_a");
        let b = OptimizerManager::spool_id("opt_run_b");
        assert_eq!(a, "optimizer_event.v1:opt_run_a");
        assert_eq!(b, "optimizer_event.v1:opt_run_b");
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn start_stop_does_not_wipe_optimizer_service_rows() {
        let (svc, _store, mut rx) = service().await;
        let (mgr, _home) = manager();
        let run = seed_run(&svc).await;
        mgr.install(None).unwrap();
        let started = mgr.start().await.unwrap();
        assert_eq!(started.phase, "ready");
        assert!(started.base_url.is_some());
        let (pinned, pin) = mgr
            .pin_run(&svc, &run.id, "gepa.banking77.smoke.v1")
            .await
            .unwrap();
        assert_eq!(pinned.id, run.id);
        assert!(Path::new(&pin.spool_path).join("identity.json").is_file());
        let stopped = mgr.stop().await.unwrap();
        assert_ne!(stopped.phase, "ready");
        let kept = svc.get(run.id.clone()).await.unwrap();
        assert_eq!(kept.id, run.id);
        assert!(kept.cursor_seq >= 1);
        let events = svc.events_after(run.id.clone(), 0, None).await.unwrap();
        assert!(events
            .iter()
            .any(|event| event.event_type == "optimizer.run.pinned"));
        let mut saw_bus = false;
        while let Ok(event) = rx.try_recv() {
            if event.kind == "optimizer.run.updated" {
                saw_bus = true;
            }
        }
        assert!(saw_bus, "pin must still publish optimizer.run.updated");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn supervisor_drain_kills_active_recipe_process_group() {
        let (mgr, _home) = manager();
        let manager = Arc::new(mgr);
        let mut command = Command::new("/bin/sh");
        isolate_process_group(&mut command);
        command
            .args(["-c", "sleep 30 & wait"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().unwrap();
        let pid = child.id().unwrap();
        manager.gepa_workers.lock().await.insert(
            "gepa_shutdown_probe".into(),
            GepaWorkerState::Running { pid },
        );

        let supervisor = crate::services::ServiceSupervisor::new();
        supervisor.register(manager.clone());
        supervisor.drain_all().await;

        let status = tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .expect("managed recipe process did not terminate")
            .unwrap();
        assert!(!status.success());
        assert!(manager.active_gepa_run_ids().await.is_empty());
        unsafe {
            assert_ne!(
                libc::kill(-(pid as i32), 0),
                0,
                "recipe process group survived supervisor drain"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn recipe_release_kills_the_owned_process_group() {
        let (mgr, _home) = manager();
        let mut command = Command::new("/bin/sh");
        isolate_process_group(&mut command);
        command
            .args(["-c", "sleep 30 & wait"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().unwrap();
        let pid = child.id().unwrap();
        mgr.gepa_workers.lock().await.insert(
            "gepa_release_probe".into(),
            GepaWorkerState::Running { pid },
        );

        mgr.release_gepa_recipe("gepa_release_probe").await;

        let status = tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .expect("released recipe process did not terminate")
            .unwrap();
        assert!(!status.success());
        assert!(mgr.active_gepa_run_ids().await.is_empty());
        unsafe {
            assert_ne!(
                libc::kill(-(pid as i32), 0),
                0,
                "recipe process group survived release"
            );
        }
    }

    #[tokio::test]
    async fn version_pin_is_recorded_on_run_and_spool() {
        let (svc, _store, _) = service().await;
        let (mgr, _home) = manager();
        let run = seed_run(&svc).await;
        let installed = mgr.install(None).unwrap();
        assert_eq!(installed.version, DEFAULT_SIDECAR_VERSION);
        assert!(installed.selected);
        mgr.start().await.unwrap();
        let (pinned, pin) = mgr
            .pin_run(&svc, &run.id, "gepa.banking77.smoke.v1")
            .await
            .unwrap();
        assert_eq!(pin.sidecar_version, DEFAULT_SIDECAR_VERSION);
        assert_eq!(pin.algorithm_version, DEFAULT_ALGORITHM_VERSION);
        assert_eq!(pin.recipe_version, "gepa.banking77.smoke.v1");
        assert_eq!(
            pinned.summary.get("sidecarVersion").and_then(Value::as_str),
            Some(DEFAULT_SIDECAR_VERSION)
        );
        assert_eq!(
            pinned.summary.get("recipeVersion").and_then(Value::as_str),
            Some("gepa.banking77.smoke.v1")
        );
        assert_eq!(
            pinned.algorithm_version.as_deref(),
            Some(DEFAULT_ALGORITHM_VERSION)
        );
        assert!(pinned
            .execution_bindings
            .iter()
            .any(|binding| binding.kind == "optimizer_sidecar"));
        let identity: Value = serde_json::from_slice(
            &fs::read(Path::new(&pin.spool_path).join("identity.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            identity.get("sidecarVersion").and_then(Value::as_str),
            Some(DEFAULT_SIDECAR_VERSION)
        );
        assert_eq!(
            identity.get("algorithmVersion").and_then(Value::as_str),
            Some(DEFAULT_ALGORITHM_VERSION)
        );
        assert_eq!(
            identity.get("recipeVersion").and_then(Value::as_str),
            Some("gepa.banking77.smoke.v1")
        );
        let _ = mgr.stop().await;
    }

    #[tokio::test]
    async fn uninstall_leaves_events_visuals_and_retained_templates() {
        let (svc, _store, _) = service().await;
        let (mgr, _home) = manager();
        let run = seed_run(&svc).await;
        assert!(!run.visual_refs.is_empty());
        let visual_id = run.visual_refs[0].id.clone();
        mgr.install(None).unwrap();
        mgr.start().await.unwrap();
        let (pinned, pin) = mgr
            .pin_run(&svc, &run.id, "gepa.banking77.smoke.v1")
            .await
            .unwrap();
        let event_count = svc
            .events_after(pinned.id.clone(), 0, None)
            .await
            .unwrap()
            .len();
        assert!(event_count >= 1);
        let template =
            mgr.retained_template_path("optimizer.gepa.live.v1", DEFAULT_SIDECAR_VERSION);
        assert!(template.join("package.json").is_file());
        let mut completed = svc.get(pinned.id.clone()).await.unwrap();
        completed.status = "completed".into();
        svc.persist_run(completed).await.unwrap();
        mgr.uninstall(DEFAULT_SIDECAR_VERSION, &svc).await.unwrap();
        assert!(mgr.discover().unwrap().is_empty());
        let kept = svc.get(run.id.clone()).await.unwrap();
        assert_eq!(kept.id, run.id);
        let events = svc.events_after(run.id.clone(), 0, None).await.unwrap();
        assert_eq!(events.len(), event_count);
        assert!(events
            .iter()
            .any(|event| event.event_type == "optimizer.run.pinned"));
        assert!(Path::new(&pin.spool_path).join("identity.json").is_file());
        assert!(template.join("package.json").is_file());
        assert!(mgr.home().join("templates").exists());
        assert_eq!(kept.visual_refs[0].id, visual_id);
        assert_eq!(kept.visual_refs[0].kind, "visual");
    }

    #[tokio::test]
    async fn uninstall_is_blocked_while_a_pinned_run_is_active() {
        let (svc, _store, _) = service().await;
        let (mgr, _home) = manager();
        let run = seed_run(&svc).await;
        mgr.install(None).unwrap();
        mgr.start().await.unwrap();
        let (pinned, _) = mgr
            .pin_run(&svc, &run.id, "gepa.banking77.smoke.v1")
            .await
            .unwrap();
        assert_eq!(pinned.status, "queued");
        let error = mgr
            .uninstall(DEFAULT_SIDECAR_VERSION, &svc)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("owns active run"));
        assert_eq!(mgr.discover().unwrap().len(), 1);
        let kept = svc.get(run.id.clone()).await.unwrap();
        assert_eq!(kept.id, run.id);
        let _ = mgr.stop().await;
    }

    #[tokio::test]
    async fn tampered_payload_fails_digest_verification() {
        let (mgr, _home) = manager();
        let installed = mgr.install(None).unwrap();
        fs::write(
            Path::new(&installed.path).join(PAYLOAD_FILE),
            b"{\"tampered\":true}",
        )
        .unwrap();
        let error = mgr.start().await.unwrap_err();
        assert!(error.to_string().contains("digest mismatch"));
    }

    #[tokio::test]
    async fn loopback_health_requires_bearer_token_and_live_service() {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let upstream = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let _ = serve_json(listener, move |request| async move {
                if request.method == hyper::Method::GET && request.path == "/health" {
                    JsonHttpResponse::ok(json!({"status":"ok"}))
                } else if request.method == hyper::Method::GET
                    && request
                        .path
                        .starts_with("/runs/gepa_luna/optimizer-events?")
                {
                    JsonHttpResponse::ok(json!({
                        "schema_version": "optimizer_event_page.v1",
                        "run_id": "gepa_luna",
                        "next_sequence": 2,
                        "events": [{
                            "schema_version": "optimizer_event.v1",
                            "type": "optimizer.candidate.updated",
                            "sequence_number": 2,
                            "run_id": "gepa_luna",
                            "algorithm_id": "gepa",
                            "slot": "optimizer_run"
                        }]
                    }))
                } else {
                    JsonHttpResponse::error(StatusCode::NOT_FOUND, "not found")
                }
            })
            .await;
        });
        let client = crate::http::http_client();
        let health = json!({"status":"ok"});
        let denied = route_sidecar(
            JsonHttpRequest {
                method: hyper::Method::GET,
                path: "/health".into(),
                authorization: None,
                body: Value::Null,
                raw_headers: hyper::HeaderMap::new(),
            },
            "secret",
            &upstream,
            &client,
            health.clone(),
        )
        .await;
        assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
        let allowed = route_sidecar(
            JsonHttpRequest {
                method: hyper::Method::GET,
                path: "/health".into(),
                authorization: Some("Bearer secret".into()),
                body: Value::Null,
                raw_headers: hyper::HeaderMap::new(),
            },
            "secret",
            &upstream,
            &client,
            health,
        )
        .await;
        assert_eq!(allowed.status, StatusCode::OK);
        let events = route_sidecar(
            JsonHttpRequest {
                method: hyper::Method::GET,
                path: "/runs/gepa_luna/optimizer-events?after_sequence=1&limit=500".into(),
                authorization: Some("Bearer secret".into()),
                body: Value::Null,
                raw_headers: hyper::HeaderMap::new(),
            },
            "secret",
            &upstream,
            &client,
            json!({"status":"ok"}),
        )
        .await;
        assert_eq!(events.status, StatusCode::OK);
        assert_eq!(events.body["run_id"], "gepa_luna");
        assert_eq!(events.body["events"][0]["sequence_number"], 2);
        task.abort();
    }

    #[tokio::test]
    async fn spawn_gepa_recipe_requires_ensure_ready() {
        let (mgr, home) = manager();
        mgr.install(None).unwrap();
        let stdout = fs::File::create(home.path().join("out.log")).unwrap();
        let stderr = fs::File::create(home.path().join("err.log")).unwrap();
        let error = mgr
            .spawn_gepa_recipe(
                "gepa_luna",
                home.path(),
                &home.path().join("recipe.toml"),
                stdout,
                stderr,
                "sk-test",
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("ensure_ready"), "{error:#}");
    }

    #[tokio::test]
    async fn optimizer_event_404_is_not_a_successful_empty_page() {
        let (mgr, _home) = manager();
        mgr.install(None).unwrap();
        mgr.start().await.unwrap();

        let error = mgr
            .optimizer_events_after("not_indexed_yet", 0, 500)
            .await
            .unwrap_err();
        assert!(OptimizerManager::optimizer_run_not_indexed(&error));
        assert!(error.to_string().contains("not_indexed_yet"));

        let _ = mgr.stop().await;
    }

    #[tokio::test]
    async fn two_gepa_recipe_spawns_are_not_serialized_behind_a_singleton_worker() {
        let (mgr, home) = manager();
        mgr.install(None).unwrap();
        mgr.start().await.unwrap();
        let config = home.path().join("recipe.toml");
        fs::write(&config, "").unwrap();
        let child_luna = mgr
            .spawn_gepa_recipe(
                "gepa_luna",
                home.path(),
                &config,
                fs::File::create(home.path().join("luna.out")).unwrap(),
                fs::File::create(home.path().join("luna.err")).unwrap(),
                "sk-test",
            )
            .await
            .unwrap();
        let child_sol = mgr
            .spawn_gepa_recipe(
                "gepa_sol",
                home.path(),
                &config,
                fs::File::create(home.path().join("sol.out")).unwrap(),
                fs::File::create(home.path().join("sol.err")).unwrap(),
                "sk-test",
            )
            .await
            .unwrap();
        let active = mgr.active_gepa_run_ids().await;
        assert_eq!(
            active,
            vec!["gepa_luna".to_string(), "gepa_sol".to_string()]
        );
        drop(child_luna);
        drop(child_sol);
        assert_eq!(mgr.active_gepa_run_ids().await.len(), 2);
        mgr.release_gepa_recipe("gepa_luna").await;
        mgr.release_gepa_recipe("gepa_sol").await;
        assert!(mgr.active_gepa_run_ids().await.is_empty());
        let _ = mgr.stop().await;
    }

    #[tokio::test]
    async fn two_gepa_campaigns_have_isolated_spools_and_flip_read_does_not_seal() {
        let (svc, _store, _) = service().await;
        let (mgr, _home) = manager();
        mgr.install(None).unwrap();
        mgr.start().await.unwrap();
        let luna = seed_named_run(&svc, "gepa_luna").await;
        let sol = seed_named_run(&svc, "gepa_sol").await;
        let (pinned_luna, pin_luna) = mgr
            .pin_run(&svc, &luna.id, "gepa.banking77.luna.v1")
            .await
            .unwrap();
        let (pinned_sol, pin_sol) = mgr
            .pin_run(&svc, &sol.id, "gepa.banking77.sol.v1")
            .await
            .unwrap();
        assert_ne!(luna.id, sol.id);
        assert_ne!(pin_luna.spool_path, pin_sol.spool_path);
        assert_eq!(pin_luna.sidecar_version, DEFAULT_SIDECAR_VERSION);
        assert_eq!(pin_luna.algorithm_version, DEFAULT_ALGORITHM_VERSION);
        assert_eq!(pin_luna.recipe_version, "gepa.banking77.luna.v1");
        assert_eq!(pin_sol.recipe_version, "gepa.banking77.sol.v1");
        assert_eq!(
            pinned_luna
                .summary
                .get("sidecarVersion")
                .and_then(Value::as_str),
            Some(DEFAULT_SIDECAR_VERSION)
        );
        assert_eq!(pinned_luna.visual_refs[0].kind, "visual");
        assert_eq!(
            svc.open_visual(luna.id.clone())
                .await
                .unwrap()
                .0
                .visual_refs[0]
                .kind,
            "visual"
        );

        mgr.append_spool_event(
            &luna.id,
            gepa_spool_event(
                &luna.id,
                1,
                "optimizer.run.started",
                json!({}),
                Some(json!({ "cost_usd": 0.11 })),
            ),
        )
        .await
        .unwrap();
        mgr.append_spool_event(
            &luna.id,
            gepa_spool_event(
                &luna.id,
                2,
                "proposer.delta",
                json!({ "generation": 1, "channel": "critique", "text": "luna" }),
                None,
            ),
        )
        .await
        .unwrap();
        mgr.append_spool_event(
            &luna.id,
            gepa_spool_event(
                &luna.id,
                3,
                "optimizer.child_rollout.attached",
                json!({
                    "child_resource_ref": {
                        "schema": "synth.resource-ref.v1",
                        "kind": "container_rollout",
                        "id": "rollout_luna",
                        "attributes": {
                            "stream_id": "stream:luna",
                            "reward_url": "/reward?rollout_id=rollout_luna"
                        }
                    }
                }),
                None,
            ),
        )
        .await
        .unwrap();
        mgr.append_spool_event(
            &sol.id,
            gepa_spool_event(
                &sol.id,
                1,
                "optimizer.run.started",
                json!({}),
                Some(json!({ "cost_usd": 0.22 })),
            ),
        )
        .await
        .unwrap();
        mgr.append_spool_event(
            &sol.id,
            gepa_spool_event(
                &sol.id,
                2,
                "proposer.delta",
                json!({ "generation": 1, "channel": "critique", "text": "sol" }),
                None,
            ),
        )
        .await
        .unwrap();
        mgr.append_spool_event(
            &sol.id,
            gepa_spool_event(
                &sol.id,
                3,
                "optimizer.child_rollout.attached",
                json!({
                    "child_resource_ref": {
                        "schema": "synth.resource-ref.v1",
                        "kind": "container_rollout",
                        "id": "rollout_sol",
                        "attributes": {
                            "stream_id": "stream:sol",
                            "reward_url": "/reward?rollout_id=rollout_sol"
                        }
                    }
                }),
                None,
            ),
        )
        .await
        .unwrap();

        let page_luna = mgr.optimizer_events_after(&luna.id, 0, 500).await.unwrap();
        let page_sol = mgr.optimizer_events_after(&sol.id, 0, 500).await.unwrap();
        assert_eq!(page_luna["run_id"], luna.id);
        assert_eq!(page_sol["run_id"], sol.id);
        assert_eq!(page_luna["slot"], OPTIMIZER_VISUAL_SLOT);
        assert_eq!(page_sol["slot"], OPTIMIZER_VISUAL_SLOT);
        assert_eq!(page_luna["terminal"], false);
        assert_eq!(page_sol["terminal"], false);
        assert_eq!(page_luna["events"].as_array().unwrap().len(), 3);
        assert_eq!(page_sol["events"].as_array().unwrap().len(), 3);
        for event in page_luna["events"].as_array().unwrap() {
            assert_eq!(event["run_id"], luna.id);
            assert_ne!(event["run_id"], sol.id);
        }
        for event in page_sol["events"].as_array().unwrap() {
            assert_eq!(event["run_id"], sol.id);
        }
        assert_eq!(page_luna["events"][0]["usage_delta"]["cost_usd"], 0.11);
        assert_eq!(page_sol["events"][0]["usage_delta"]["cost_usd"], 0.22);
        assert_eq!(page_luna["events"][1]["delta"]["text"], "luna");
        assert_eq!(page_sol["events"][1]["delta"]["text"], "sol");
        assert_eq!(
            page_luna["events"][2]["delta"]["child_resource_ref"]["id"],
            "rollout_luna"
        );
        assert_eq!(
            page_sol["events"][2]["delta"]["child_resource_ref"]["id"],
            "rollout_sol"
        );

        mgr.append_spool_event(
            &luna.id,
            gepa_spool_event(&luna.id, 4, "optimizer.candidate.updated", json!({}), None),
        )
        .await
        .unwrap();
        let page_sol_again = mgr.optimizer_events_after(&sol.id, 0, 500).await.unwrap();
        assert_eq!(page_sol_again["events"].as_array().unwrap().len(), 3);
        assert_eq!(page_sol_again["terminal"], false);
        let page_luna_tail = mgr.optimizer_events_after(&luna.id, 3, 500).await.unwrap();
        assert_eq!(page_luna_tail["events"].as_array().unwrap().len(), 1);
        assert_eq!(page_luna_tail["events"][0]["sequence_number"], 4);

        let stopped = mgr.stop().await.unwrap();
        assert_ne!(stopped.phase, "ready");
        assert_eq!(svc.get(luna.id.clone()).await.unwrap().id, luna.id);
        assert_eq!(svc.get(sol.id.clone()).await.unwrap().id, sol.id);
        assert!(Path::new(&pin_luna.spool_path)
            .join("identity.json")
            .is_file());
        assert!(Path::new(&pin_sol.spool_path)
            .join("identity.json")
            .is_file());
        let _ = pinned_sol;
    }
}
