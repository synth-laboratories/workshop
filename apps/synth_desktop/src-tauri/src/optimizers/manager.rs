//! Optimizer sidecar lifecycle, parallel to [`crate::laguna::LagunaManager`].
//!
//! Owns discovery, digest/signature, version pin, process start/stop, health,
//! loopback auth, and recovery. Does **not** own model download/unload, and does
//! **not** own the durable run/event/visual projection — that stays
//! [`super::OptimizerService`]. Stopping or uninstalling a sidecar version must
//! not delete runs, events, visuals, or retained template packages.

use super::events::OptimizerEventDraft;
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
    cell::Cell,
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
use tokio::sync::{broadcast, Mutex, OwnedSemaphorePermit, RwLock, Semaphore};

// Channel pins, the floor, and the vocabulary lists all resolve from the one
// contract table so a version has a single place to be read and changed.
use crate::contract::runtimes::{ReleaseChannel, OPTIMIZERS as OPTIMIZERS_CONTRACT};

pub const OFFICIAL_SIDECAR_VERSION: &str = OPTIMIZERS_CONTRACT.official;
pub const DEV_SIDECAR_VERSION: &str = OPTIMIZERS_CONTRACT.dev;
pub const DEFAULT_SIDECAR_VERSION: &str = OFFICIAL_SIDECAR_VERSION;
pub const DEFAULT_RECIPE_SCHEMA_VERSION: &str = OPTIMIZERS_CONTRACT.recipe_schema;
/// `{package}-{official}`. Spelled out because `format!` is not const and ten
/// call sites want `&'static str`; `algorithm_version_matches_the_contract`
/// fails if it drifts from the table.
pub const DEFAULT_ALGORITHM_VERSION: &str = "synth-optimizers-0.2.14";
/// Optimizer-family visuals bind this slot. `live` and `jobs` are refused.
pub const OPTIMIZER_VISUAL_SLOT: &str = "optimizer_run";
const MAX_CONCURRENT_GEPA_RECIPES: usize = 2;
const SELECTED_VERSION_FILE: &str = "selected_version";
const SIGNING_KEY_FILE: &str = "signing.key";
const API_KEY_FILE: &str = "api_key";
const PAYLOAD_FILE: &str = "payload.json";
const MANIFEST_FILE: &str = "manifest.json";
const WHEELHOUSE_MANIFEST_FILE: &str = "wheelhouse-manifest.json";

thread_local! {
    static TEST_FORCE_DIGEST_MISMATCH: Cell<bool> = const { Cell::new(false) };
    static TEST_INTERRUPT_INSTALL: Cell<bool> = const { Cell::new(false) };
}

fn force_digest_mismatch() -> bool {
    TEST_FORCE_DIGEST_MISMATCH.with(Cell::get)
        || env::var("SYNTH_OPTIMIZER_FORCE_DIGEST_MISMATCH").as_deref() == Ok("1")
}

fn interrupt_install() -> bool {
    TEST_INTERRUPT_INSTALL.with(Cell::get)
        || env::var("SYNTH_OPTIMIZER_INTERRUPT_INSTALL").as_deref() == Ok("1")
}

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct WheelArtifact {
    file_name: String,
    sha256: String,
    size_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct WheelhouseManifest {
    schema_version: String,
    artifacts: Vec<WheelArtifact>,
}

impl Default for OptimizerSidecarInstallSpec {
    fn default() -> Self {
        catalog_spec(DEFAULT_SIDECAR_VERSION)
    }
}

fn catalog_spec(version: &str) -> OptimizerSidecarInstallSpec {
    let algorithm_version = format!("synth-optimizers-{version}");
    OptimizerSidecarInstallSpec {
        version: version.to_owned(),
        algorithm_id: "gepa".into(),
        algorithm_version: algorithm_version.clone(),
        recipe_schema_version: DEFAULT_RECIPE_SCHEMA_VERSION.into(),
        payload: json!({
            "sidecarVersion": version,
            "algorithms": OPTIMIZERS_CONTRACT
                .algorithms
                .iter()
                .map(|id| json!({ "id": id, "version": algorithm_version }))
                .collect::<Vec<_>>(),
            "recipeSchemaVersion": DEFAULT_RECIPE_SCHEMA_VERSION,
            "templates": OPTIMIZERS_CONTRACT.templates,
            // Desktop's expectation of the pinned version, not a claim about
            // the artifact on disk. The install payload asserting
            // `eventReplay: true` for a wheel that served no events route is
            // how this incident was seeded; only the handshake settles it.
            "health": true,
            "cancellation": true,
            "eventReplay": true,
        }),
        template_ids: OPTIMIZERS_CONTRACT
            .templates
            .iter()
            .map(|id| (*id).to_owned())
            .collect(),
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
    gepa_capacity: Arc<Semaphore>,
    run_spools: Arc<Mutex<HashMap<String, RunSpoolState>>>,
    client: Client,
    /// Attached by the composition root once diagnostics exist.
    diagnostics: Arc<std::sync::OnceLock<Arc<crate::diagnostics::DiagnosticsService>>>,
}

#[derive(Debug)]
enum GepaWorkerState {
    /// The run id is atomically reserved while its process is being spawned.
    Starting,
    /// The isolated process group is owned by this supervisor.
    Running {
        pid: u32,
        _permit: Option<OwnedSemaphorePermit>,
    },
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
            gepa_capacity: Arc::new(Semaphore::new(MAX_CONCURRENT_GEPA_RECIPES)),
            run_spools: Arc::new(Mutex::new(HashMap::new())),
            client: crate::http::http_client(),
            diagnostics: Arc::new(std::sync::OnceLock::new()),
        }
    }

    /// Wire diagnostics in after both services exist. Idempotent.
    pub fn attach_diagnostics(&self, service: Arc<crate::diagnostics::DiagnosticsService>) {
        let _ = self.diagnostics.set(service);
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

    pub async fn is_running(&self) -> bool {
        self.runtime.lock().await.is_some() && self.status().await.phase == "ready"
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
            if events_not_found_is_missing_route(&body) {
                bail!(
                    "optimizer runtime does not serve the optimizer-events route; \
                     install a sidecar version that implements it"
                );
            }
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
        let previous = {
            let mut current = self.status.write().await;
            let previous = current.phase.clone();
            *current = status.clone();
            previous
        };
        // This runs on every renderer poll. Only a *transition* is news; the
        // steady state is what `diagnostics_status` is for.
        if previous != status.phase {
            self.diagnose_phase(&previous, &status);
        }
        let _ = self.updates.send(status);
    }

    fn diagnose_phase(&self, previous: &str, status: &OptimizerSidecarStatus) {
        let Some(service) = self.diagnostics.get() else {
            return;
        };
        let unavailable = matches!(
            status.phase.as_str(),
            "failed" | "error" | "unavailable" | "crashed" | "stopped"
        );
        let mut input = crate::diagnostics::DiagnosticInput::new(
            if unavailable {
                crate::diagnostics::Severity::Error
            } else {
                crate::diagnostics::Severity::Info
            },
            "optimizer-sidecar",
            "optimizer.sidecar.phase",
            if unavailable {
                crate::diagnostics::codes::OPTIMIZER_SIDECAR_UNAVAILABLE
            } else {
                "optimizer_sidecar_phase"
            },
            status
                .detail
                .clone()
                .unwrap_or_else(|| format!("optimizer sidecar is {}", status.phase)),
        )
        .retryable(unavailable);
        input
            .details
            .insert("phase".into(), serde_json::json!(status.phase));
        input
            .details
            .insert("previous_phase".into(), serde_json::json!(previous));
        if let Some(version) = status.version.as_ref() {
            input
                .details
                .insert("version".into(), serde_json::json!(version));
        }
        service.emit(input);
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
            // Status is a read path. A transient missed probe must not SIGTERM
            // a live paid run merely because the renderer polls this method.
            // Explicit start/stop and process-exit reconciliation own teardown.
            let mut current = self.status().await;
            current.detail =
                Some("Optimizer health probe was missed; retaining the managed runtime".into());
            self.set_status(current).await;
            return self.status().await;
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
        enforce_version_floor(version)?;
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
        // The stored handshake belongs to the version that proved it. Leaving it
        // behind lets a previous install's capabilities satisfy the digest pin
        // and template negotiation for a version that never completed a
        // handshake of its own.
        clear_stored_capabilities(&self.home);
        Ok(OptimizerSidecarVersion {
            selected: true,
            ..hit
        })
    }

    pub fn install(&self, version: Option<&str>) -> Result<OptimizerSidecarVersion> {
        let version = version.unwrap_or(DEFAULT_SIDECAR_VERSION);
        if !matches!(version, OFFICIAL_SIDECAR_VERSION | DEV_SIDECAR_VERSION) {
            bail!("unknown optimizer sidecar version `{version}`");
        }
        self.install_spec(catalog_spec(version))
    }

    pub async fn set_status_phase(&self, phase: &str, detail: Option<&str>) {
        let current = self.status().await;
        self.set_status(OptimizerSidecarStatus {
            phase: phase.into(),
            detail: detail.map(str::to_string).or(current.detail),
            updated_at: now_ms(),
            ..current
        })
        .await;
    }

    pub fn advertised_capabilities(&self) -> Value {
        read_capabilities(&self.home).unwrap_or_else(|| {
            json!({
                "algorithms": [],
                "controls": [],
                "replay": false,
                "cancellation": false
            })
        })
    }

    pub fn has_offline_runtime(&self, version: &str) -> bool {
        installed_runtime_bin(&self.home, version).is_ok()
    }

    pub fn install_spec(
        &self,
        spec: OptimizerSidecarInstallSpec,
    ) -> Result<OptimizerSidecarVersion> {
        validate_version_id(&spec.version)?;
        enforce_version_floor(&spec.version)?;
        fs::create_dir_all(&self.home)?;
        let previous_selected = read_selected_version(&self.home)?;
        let staging_name = format!(
            ".staging-{}-{}",
            spec.version,
            uuid::Uuid::new_v4().simple()
        );
        let staging = self.home.join("versions").join(&staging_name);
        fs::create_dir_all(&staging)
            .with_context(|| format!("create optimizer staging {}", staging.display()))?;
        let installed = match materialize_verified_distribution(&self.home, &staging, &spec) {
            Ok(hit) => hit,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                return Err(error);
            }
        };
        if interrupt_install() {
            bail!("install interrupted before activation");
        }
        let dest = self.home.join("versions").join(&spec.version);
        if dest.exists() {
            fs::remove_dir_all(&dest).with_context(|| {
                format!("replace optimizer version directory {}", dest.display())
            })?;
        }
        fs::rename(&staging, &dest)
            .with_context(|| format!("activate optimizer version {}", dest.display()))?;
        for template_id in &spec.template_ids {
            retain_template_package(&self.home, template_id, &spec.version, &installed.digest)?;
        }
        let selected = self.select_version(&spec.version)?;
        debug_assert_eq!(
            read_selected_version(&self.home).ok().flatten(),
            Some(spec.version.clone())
        );
        let _ = previous_selected;
        Ok(selected)
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
        // env.sh is written after the handshake, not here. Publishing the
        // address before the service has proven anything is what left a
        // convincing file pointing at a dead port.
        let runtime_epoch = uuid::Uuid::new_v4().simple().to_string();
        let deadline = tokio::time::Instant::now() + crate::limits::OPTIMIZER_SIDECAR_READY_WAIT;
        while tokio::time::Instant::now() < deadline {
            if let Some(status) = self.probe().await {
                if status.phase == "ready" {
                    let capabilities = match self.fetch_handshake_capabilities().await {
                        Ok(capabilities) => capabilities,
                        Err(error) => {
                            self.abort_runtime().await;
                            self.set_status(OptimizerSidecarStatus {
                                phase: "error".into(),
                                base_url: None,
                                version: Some(hit.version.clone()),
                                digest: Some(hit.digest.clone()),
                                detail: Some(format!(
                                    "Capability check failed: {}",
                                    diagnostic_error_message(&error)
                                )),
                                updated_at: now_ms(),
                            })
                            .await;
                            return Err(error);
                        }
                    };
                    self.store_handshake_capabilities(&capabilities)?;
                    write_env_sh(
                        &self.home,
                        &api_key,
                        &base_url,
                        &hit.version,
                        &runtime_epoch,
                    )?;
                    self.set_status(status).await;
                    return Ok(self.status().await);
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.abort_runtime().await;
        self.set_status(OptimizerSidecarStatus {
            phase: "error".into(),
            base_url: None,
            version: Some(hit.version),
            digest: Some(hit.digest),
            detail: Some("Timed out waiting for optimizer sidecar".into()),
            updated_at: now_ms(),
        })
        .await;
        bail!("Timed out waiting for optimizer sidecar");
    }

    async fn fetch_handshake_capabilities(&self) -> Result<Value> {
        let (base_url, api_key) = {
            let runtime = self.runtime.lock().await;
            let runtime = runtime
                .as_ref()
                .ok_or_else(|| anyhow!("optimizer runtime disappeared before handshake"))?;
            (runtime.base_url.clone(), runtime.api_key.clone())
        };
        let response = self
            .client
            .get(format!("{base_url}/v1/optimizer/capabilities"))
            .bearer_auth(api_key)
            .send()
            .await
            .context("read optimizer capability handshake")?;
        if !response.status().is_success() {
            // A bare status was what made this failure so expensive to read:
            // "HTTP 502" named neither the missing route nor the runtime that
            // lacked it. Carry the proxy's reason through.
            let status = response.status();
            let detail = upstream_failure_detail(response).await;
            if status == StatusCode::NOT_FOUND {
                bail!(
                    "optimizer_capability_route_missing: this sidecar version does not serve \
                     /v1/optimizer/capabilities; install a version that does ({detail})"
                );
            }
            bail!("optimizer capability handshake failed: {detail}");
        }
        let mut capabilities: Value = response
            .json()
            .await
            .context("parse optimizer capability handshake")?;
        {
            let object = capabilities
                .as_object_mut()
                .ok_or_else(|| anyhow!("optimizer capability handshake was not an object"))?;
            // A runtime answers only for itself. `recipes` and
            // `compatibleTemplateIds` are Desktop vocabulary — those recipe ids
            // and visual template ids are defined here and appear nowhere in
            // the plugin — so requiring a runtime to echo them back proves only
            // that it was told what to say, and would force a plugin release
            // for every new host template. They are resolved host-side now;
            // ask the runtime for the one list it can actually own.
            let algorithms_valid = object
                .get("algorithms")
                .and_then(Value::as_array)
                .is_some_and(|items| {
                    !items.is_empty() && items.iter().all(|item| item.as_str().is_some())
                });
            if !algorithms_valid {
                bail!("optimizer capability handshake omitted algorithms");
            }
            for field in ["replay", "cancellation"] {
                if object.get(field).and_then(Value::as_bool).is_none() {
                    bail!("optimizer capability handshake omitted {field}");
                }
            }
            object.remove("digest");
        }
        let digest = sha256_hex(&serde_json::to_vec(&capabilities)?);
        capabilities
            .as_object_mut()
            .expect("validated capability object")
            .insert("digest".into(), json!(format!("sha256:{digest}")));
        Ok(capabilities)
    }

    fn store_handshake_capabilities(&self, capabilities: &Value) -> Result<()> {
        fs::write(
            self.home.join("capabilities.json"),
            serde_json::to_vec_pretty(capabilities)?,
        )?;
        Ok(())
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
    /// At most two `optimizer_run_id`s execute at once. Additional submitted
    /// runs wait here in their durable `queued` state instead of launching
    /// colliding child processes against the same optimizer workspace.
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
                "optimizer sidecar is not running; the Optimizers plugin must be started before spawning a GEPA recipe"
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
        let permit = self
            .gepa_capacity
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| anyhow!("GEPA admission queue closed while `{run_id}` was waiting"))?;
        if !matches!(
            self.gepa_workers.lock().await.get(run_id),
            Some(GepaWorkerState::Starting)
        ) {
            bail!("GEPA supervisor cancelled `{run_id}` while it was queued");
        }
        match launch_gepa_recipe_process(
            &self.home,
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
                            *state = GepaWorkerState::Running {
                                pid,
                                _permit: Some(permit),
                            };
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
        if let Some(GepaWorkerState::Running { pid, .. }) = state {
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
        // The in-process sidecar stand-in registers a run the instant it is
        // spawned, so under test every run is indexed immediately and the
        // "service never sees this run" failure is unreachable — the same shape
        // of blind spot as a fake that serves an endpoint the real artifact
        // lacks. This makes the stand-in *less* generous on request, so that
        // failure can be exercised. It only ever withholds; it never invents.
        #[cfg(test)]
        {
            if std::env::var("SYNTH_OPTIMIZER_TEST_SUPPRESS_SPOOL").as_deref() == Ok("1") {
                return;
            }
        }
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
            // Uninstalling the version that proved these capabilities must not
            // leave them behind to vouch for whatever is installed next.
            clear_stored_capabilities(&self.home);
            clear_env_sh(&self.home);
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
        let draft = OptimizerEventDraft::new("optimizer.run.pinned", run.algorithm_id.clone())
            // One pin per run: re-pinning re-offers the same fact rather than
            // minting a second sequence for it.
            .idempotency_key("sidecar-pin")
            .level("info")
            .delta(
                json!({
                    "sidecarVersion": pin.sidecar_version,
                    "algorithmVersion": pin.algorithm_version,
                    "recipeVersion": pin.recipe_version,
                    "sidecarDigest": pin.digest,
                    "spoolPath": pin.spool_path,
                })
                .as_object()
                .cloned()
                .unwrap_or_default(),
            )
            .snapshot(
                json!({ "summary": run.summary.clone() })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            )
            .raw(json!({
                "sidecarVersion": pin.sidecar_version,
                "algorithmVersion": pin.algorithm_version,
                "recipeVersion": pin.recipe_version
            }));
        let (run, _) = service
            .append_event_payloads(run.id.clone(), vec![draft])
            .await?;
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
        // The exported address describes a service that is about to stop
        // existing. Every teardown goes through here.
        clear_env_sh(&self.home);
        let worker_pids = self
            .gepa_workers
            .lock()
            .await
            .drain()
            .filter_map(|(_, state)| match state {
                GepaWorkerState::Starting => None,
                GepaWorkerState::Running { pid, .. } => Some(pid),
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

fn optimizer_command(home: &Path, version: &str) -> Result<Command> {
    validate_version_id(version)?;
    if developer_uv_mode()? {
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
        return Ok(command);
    }
    let bin = installed_runtime_bin(home, version)?;
    Ok(Command::new(bin))
}

fn optimizer_gepa_home(home: &Path) -> PathBuf {
    home.join("runtime/gepa-home")
}

fn optimizer_gepa_db(home: &Path) -> PathBuf {
    home.join("runtime/gepa.sqlite")
}

fn developer_uv_mode() -> Result<bool> {
    if env::var("SYNTH_OPTIMIZER_DEV_MODE").as_deref() == Ok("1") {
        return Ok(true);
    }
    Ok(optimizer_project_root()?.is_some())
}

fn installed_runtime_bin(home: &Path, version: &str) -> Result<PathBuf> {
    validate_version_id(version)?;
    let runtime = home.join("versions").join(version).join("runtime");
    for candidate in [
        runtime.join("bin/synth-optimizers"),
        runtime.join("Scripts/synth-optimizers.exe"),
        runtime.join("synth-optimizers"),
    ] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!("installed optimizer runtime for `{version}` is missing; install the plugin before start")
}

fn launch_gepa_recipe_process(
    home: &Path,
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
            let _ = (home, version, cookbook, openai_api_key, config_path);
            // The stand-in normally exits at once. Tests that need to observe
            // what the supervisor does to a *live* child — the never-indexed
            // bound, cancellation — ask for one that outlives the assertion.
            let mut command = match env::var("SYNTH_OPTIMIZER_TEST_CHILD_SLEEP_SECS") {
                Ok(seconds) if !seconds.is_empty() => {
                    let mut sleeper = Command::new("/bin/sleep");
                    sleeper.arg(seconds);
                    sleeper
                }
                _ => Command::new("/usr/bin/true"),
            };
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
    let mut command = optimizer_command(home, version)?;
    isolate_process_group(&mut command);
    command
        .args(["gepa", "run", "--config"])
        .arg(config_path)
        .current_dir(cookbook)
        // The service discovers live child runs through GEPA_HOME/index.jsonl.
        // Pin both processes to the same instance-owned directory instead of
        // relying on whichever user-global HOME the Desktop inherited.
        .env("GEPA_HOME", optimizer_gepa_home(home))
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
                            && path == "/v1/optimizer/capabilities"
                        {
                            JsonHttpResponse::ok(json!({
                                "status": "ok",
                                "algorithms": OPTIMIZERS_CONTRACT.algorithms,
                                "contractVersion": "optimizer.contract.v1",
                                "serviceVersion": OPTIMIZERS_CONTRACT.official,
                                "replay": true,
                                "cancellation": true
                            }))
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

    let gepa_home = optimizer_gepa_home(home);
    let db_path = optimizer_gepa_db(home);
    fs::create_dir_all(&gepa_home)?;
    let api_key = ensure_api_key(home)?;
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("sidecar.log"))?;
    let mut command = optimizer_command(home, &hit.version)?;
    isolate_process_group(&mut command);
    command
        .args(["gepa", "service", "--db"])
        .arg(&db_path)
        .args(["--bind", &addr.to_string()])
        .env("GEPA_HOME", &gepa_home)
        .env("SYNTH_OPTIMIZER_API_KEY", &api_key)
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
fn owned_process_group(pid: u32) -> Option<libc::pid_t> {
    // `kill(-1, signal)` broadcasts to every process the caller may signal.
    // Never allow sentinel/system PIDs or a lossy u32 -> pid_t conversion to
    // reach the negative-pid process-group API.
    let pid = libc::pid_t::try_from(pid).ok().filter(|pid| *pid > 1)?;
    unsafe {
        // A recipe is spawned with process_group(0), so its pid must equal its
        // pgid. Refuse stale, reused, or non-isolated PIDs instead of guessing.
        let pgid = libc::getpgid(pid);
        if pgid <= 1 || pgid != pid || pgid == libc::getpgrp() {
            return None;
        }
        Some(pgid)
    }
}

#[cfg(unix)]
async fn terminate_process_groups(pids: &[u32]) {
    let groups = pids
        .iter()
        .filter_map(|&pid| {
            let group = owned_process_group(pid);
            if group.is_none() {
                eprintln!(
                    "refusing to terminate optimizer process group for unsafe or unowned pid {pid}"
                );
            }
            group
        })
        .collect::<Vec<_>>();
    for &pgid in &groups {
        // Negative pid addresses only the verified isolated process group.
        unsafe {
            libc::kill(-pgid, libc::SIGTERM);
        }
    }
    if !groups.is_empty() {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    for &pgid in &groups {
        unsafe {
            if libc::kill(-pgid, 0) == 0 {
                libc::kill(-pgid, libc::SIGKILL);
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

/// Refuse a runtime older than the contract floor.
///
/// Install-time UX, not a safety property: it explains the refusal before a
/// download instead of after a failed handshake. The handshake stays the gate,
/// because a version number is the runtime's claim about itself and the
/// capability response is the only thing that demonstrates anything.
fn enforce_version_floor(version: &str) -> Result<()> {
    if OPTIMIZERS_CONTRACT.meets_floor(version) {
        return Ok(());
    }
    bail!(
        "optimizer sidecar `{version}` is older than the supported floor \
         `{floor}`; install {floor} or newer",
        floor = OPTIMIZERS_CONTRACT.min_supported
    )
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

fn distribution_digest(payload: &[u8], wheelhouse_manifest: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update((payload.len() as u64).to_be_bytes());
    digest.update(payload);
    digest.update((wheelhouse_manifest.len() as u64).to_be_bytes());
    digest.update(wheelhouse_manifest);
    format!("{:x}", digest.finalize())
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

/// Export the live sidecar's address for a human or an agent to source.
///
/// Written only after the capability handshake succeeds, because this file is
/// the most convincing thing in the directory and nothing else about it says
/// whether the service behind it is alive. A copy left over from a previous
/// run is what made the original incident read as "the sidecar never started":
/// the port belonged to an in-process proxy that dies with the host, so after
/// the fact it always looks dead. `writtenAt` and the epoch are here so a stale
/// copy is self-evidently stale rather than merely wrong.
///
/// Mode 0600: it carries the bearer token in cleartext.
fn write_env_sh(
    home: &Path,
    api_key: &str,
    base_url: &str,
    version: &str,
    epoch: &str,
) -> Result<()> {
    fs::create_dir_all(home)?;
    let written_at = chrono::Utc::now().to_rfc3339();
    let body = format!(
        "# Written after a successful capability handshake. Removed on stop.\n\
         # A copy of this file is not evidence that the service is running.\n\
         export SYNTH_OPTIMIZER_HOST=\"127.0.0.1\"\n\
         export SYNTH_OPTIMIZER_BASE_URL=\"{base_url}\"\n\
         export SYNTH_OPTIMIZER_API_KEY=\"{api_key}\"\n\
         export SYNTH_OPTIMIZER_VERSION=\"{version}\"\n\
         export SYNTH_OPTIMIZER_RUNTIME_EPOCH=\"{epoch}\"\n\
         export SYNTH_OPTIMIZER_WRITTEN_AT=\"{written_at}\"\n"
    );
    write_secret(&home.join("env.sh"), body.as_bytes(), false)
}

/// Remove the exported address. Paired with every teardown path, so the file
/// never outlives the service it describes.
fn clear_env_sh(home: &Path) {
    let _ = fs::remove_file(home.join("env.sh"));
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
    let wheelhouse_manifest = fs::read(dir.join(WHEELHOUSE_MANIFEST_FILE))
        .context("read optimizer wheelhouse manifest")?;
    let wheelhouse: WheelhouseManifest = serde_json::from_slice(&wheelhouse_manifest)
        .context("decode optimizer wheelhouse manifest")?;
    verify_wheelhouse(dir, &wheelhouse)?;
    let actual = distribution_digest(&payload, &wheelhouse_manifest);
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
    let staging_templates = home
        .join("versions")
        .join(format!("{sidecar_version}"))
        .join("templates");
    let _ = staging_templates;
    Ok(())
}

fn materialize_verified_distribution(
    home: &Path,
    staging: &Path,
    spec: &OptimizerSidecarInstallSpec,
) -> Result<OptimizerSidecarVersion> {
    if force_digest_mismatch() {
        fs::write(staging.join(PAYLOAD_FILE), b"tampered")?;
        fs::write(
            staging.join(WHEELHOUSE_MANIFEST_FILE),
            serde_json::to_vec_pretty(&WheelhouseManifest {
                schema_version: "synth.optimizer-wheelhouse.v1".into(),
                artifacts: Vec::new(),
            })?,
        )?;
        fs::write(
            staging.join(MANIFEST_FILE),
            serde_json::to_vec_pretty(&json!({
                "version": spec.version,
                "digest": "deadbeef",
                "signature": "invalid",
            }))?,
        )?;
        return load_verified_manifest(home, staging);
    }
    let payload = serde_json::to_vec(&spec.payload).context("encode sidecar payload")?;
    let artifacts = if fixture_install() {
        materialize_fixture_runtime(staging, spec)?;
        Vec::new()
    } else {
        materialize_uv_runtime(staging, spec)?
    };
    let wheelhouse = WheelhouseManifest {
        schema_version: "synth.optimizer-wheelhouse.v1".into(),
        artifacts,
    };
    let wheelhouse_manifest = serde_json::to_vec_pretty(&wheelhouse)?;
    fs::write(staging.join(WHEELHOUSE_MANIFEST_FILE), &wheelhouse_manifest)?;
    let digest = distribution_digest(&payload, &wheelhouse_manifest);
    let signing_key = ensure_signing_key(home)?;
    let signature = sign_manifest(&signing_key, &spec.version, &digest);
    let manifest = json!({
        "version": spec.version,
        "digest": digest,
        "signature": signature,
        "algorithmId": spec.algorithm_id,
        "algorithmVersion": spec.algorithm_version,
        "recipeSchemaVersion": spec.recipe_schema_version,
        "templates": spec.template_ids,
        "package": "synth-optimizers",
        "publisher": "Synth Laboratories",
        "networkHost": "pypi.org",
        "platform": std::env::consts::OS,
        "workshopCompat": OPTIMIZERS_CONTRACT.workshop_compat,
    });
    fs::write(staging.join(PAYLOAD_FILE), &payload)?;
    fs::write(
        staging.join(MANIFEST_FILE),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    fs::write(
        staging.join("package-lock.json"),
        serde_json::to_vec_pretty(&json!({
            "package": "synth-optimizers",
            "version": spec.version,
            "digest": format!("sha256:{digest}"),
            "offline": true,
            "wheelhouseManifest": WHEELHOUSE_MANIFEST_FILE,
            "artifacts": wheelhouse.artifacts,
        }))?,
    )?;
    let templates = staging.join("templates");
    fs::create_dir_all(&templates)?;
    for template_id in &spec.template_ids {
        fs::write(
            templates.join(format!("{template_id}.json")),
            serde_json::to_vec_pretty(&json!({
                "templateId": template_id,
                "sidecarVersion": spec.version,
                "digest": format!("sha256:{digest}"),
            }))?,
        )?;
    }
    prove_offline_version(staging, spec)?;
    load_verified_manifest(home, staging)
}

fn fixture_install() -> bool {
    if env::var("SYNTH_OPTIMIZER_LIVE_INSTALL").as_deref() == Ok("1") {
        return false;
    }
    cfg!(test) || env::var("SYNTH_OPTIMIZER_FIXTURE_INSTALL").as_deref() == Ok("1")
}

fn materialize_fixture_runtime(staging: &Path, spec: &OptimizerSidecarInstallSpec) -> Result<()> {
    let bin_dir = staging.join("runtime/bin");
    fs::create_dir_all(&bin_dir)?;
    let bin = bin_dir.join("synth-optimizers");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(
            &bin,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo synth-optimizers {}; exit 0; fi\nexit 0\n",
                spec.version
            ),
        )?;
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    {
        fs::write(&bin, spec.version.as_bytes())?;
    }
    Ok(())
}

fn materialize_uv_runtime(
    staging: &Path,
    spec: &OptimizerSidecarInstallSpec,
) -> Result<Vec<WheelArtifact>> {
    let uv = resolve_uv()?;
    let runtime = staging.join("runtime");
    let wheels = staging.join("wheels");
    fs::create_dir_all(&wheels)?;
    let status = std::process::Command::new(&uv)
        .args(["venv", "--clear"])
        .arg(&runtime)
        .status()
        .context("create optimizer runtime venv")?;
    if !status.success() {
        bail!("failed to create optimizer runtime venv");
    }
    let python = runtime.join("bin/python");
    let download = std::process::Command::new(&uv)
        .args([
            "run",
            "--no-project",
            "--with",
            "pip",
            "python",
            "-m",
            "pip",
            "download",
            "--only-binary=:all:",
            "-d",
        ])
        .arg(&wheels)
        .arg(format!("synth-optimizers=={}", spec.version))
        .status()
        .context("download optimizer wheel")?;
    if !download.success() {
        bail!("failed to download synth-optimizers=={}", spec.version);
    }
    let artifacts = collect_wheel_artifacts(&wheels)?;
    let optimizer_prefix = format!("synth_optimizers-{}-", spec.version);
    if !artifacts
        .iter()
        .any(|artifact| artifact.file_name.starts_with(&optimizer_prefix))
    {
        bail!(
            "optimizer wheelhouse omitted synth-optimizers=={}",
            spec.version
        );
    }
    let install = std::process::Command::new(&uv)
        .args([
            "pip",
            "install",
            "--prerelease=allow",
            "--offline",
            "--no-index",
            "--find-links",
        ])
        .arg(&wheels)
        .arg("--python")
        .arg(&python)
        .arg(format!("synth-optimizers=={}", spec.version))
        .status()
        .context("install optimizer wheel offline")?;
    if !install.success() {
        bail!(
            "failed to install synth-optimizers=={} offline",
            spec.version
        );
    }
    write_relocatable_optimizer_launcher(&runtime)?;
    Ok(artifacts)
}

#[cfg(unix)]
fn write_relocatable_optimizer_launcher(runtime: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let launcher = runtime.join("bin/synth-optimizers");
    fs::write(
        &launcher,
        b"#!/bin/sh\nset -eu\nbin_dir=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nexec \"$bin_dir/python\" -m synth_optimizers.cli \"$@\"\n",
    )
    .context("write relocatable synth-optimizers launcher")?;
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755))
        .context("mark relocatable synth-optimizers launcher executable")?;
    Ok(())
}

#[cfg(not(unix))]
fn write_relocatable_optimizer_launcher(_runtime: &Path) -> Result<()> {
    Ok(())
}

fn collect_wheel_artifacts(wheels: &Path) -> Result<Vec<WheelArtifact>> {
    let mut artifacts = Vec::new();
    for entry in fs::read_dir(wheels).context("read optimizer wheelhouse")? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("whl") {
            continue;
        }
        let bytes =
            fs::read(&path).with_context(|| format!("hash optimizer wheel {}", path.display()))?;
        artifacts.push(WheelArtifact {
            file_name: entry.file_name().to_string_lossy().into_owned(),
            sha256: sha256_hex(&bytes),
            size_bytes: bytes.len() as u64,
        });
    }
    artifacts.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    if artifacts.is_empty() {
        bail!("optimizer wheelhouse is empty");
    }
    Ok(artifacts)
}

fn verify_wheelhouse(root: &Path, manifest: &WheelhouseManifest) -> Result<()> {
    if manifest.schema_version != "synth.optimizer-wheelhouse.v1" {
        bail!("optimizer wheelhouse manifest schema is unsupported");
    }
    for artifact in &manifest.artifacts {
        if artifact.file_name.contains('/') || artifact.file_name.contains('\\') {
            bail!("optimizer wheelhouse manifest contains an invalid file name");
        }
        let path = root.join("wheels").join(&artifact.file_name);
        let bytes =
            fs::read(&path).with_context(|| format!("read optimizer wheel {}", path.display()))?;
        if bytes.len() as u64 != artifact.size_bytes || sha256_hex(&bytes) != artifact.sha256 {
            bail!("optimizer wheel `{}` digest mismatch", artifact.file_name);
        }
    }
    Ok(())
}

fn prove_offline_version(staging: &Path, spec: &OptimizerSidecarInstallSpec) -> Result<()> {
    let bin = staging.join("runtime/bin/synth-optimizers");
    if !bin.is_file() {
        bail!("optimizer runtime omitted synth-optimizers executable");
    }
    if fixture_install() {
        let output = std::process::Command::new(&bin)
            .arg("--version")
            .output()
            .context("prove fixture optimizer runtime")?;
        if !output.status.success()
            || !String::from_utf8_lossy(&output.stdout).contains(&spec.version)
        {
            bail!(
                "fixture optimizer runtime did not identify {}",
                spec.version
            );
        }
        return Ok(());
    }
    let python = staging.join("runtime/bin/python");
    if !python.is_file() {
        bail!("optimizer runtime omitted its Python executable");
    }
    let output = std::process::Command::new(&python)
        .args([
            "-c",
            "import importlib.metadata as m; import synth_optimizers; print(m.version('synth-optimizers'))",
        ])
        .env("UV_OFFLINE", "1")
        .env("UV_NO_NETWORK", "1")
        .output()
        .context("prove installed synth-optimizers metadata offline")?;
    if !output.status.success() {
        bail!("installed synth-optimizers import failed offline");
    }
    if String::from_utf8_lossy(&output.stdout).trim() != spec.version {
        bail!(
            "installed synth-optimizers metadata did not identify {}",
            spec.version
        );
    }
    let cli = std::process::Command::new(&bin)
        .arg("--help")
        .env("UV_OFFLINE", "1")
        .env("UV_NO_NETWORK", "1")
        .output()
        .context("prove installed synth-optimizers CLI offline")?;
    if !cli.status.success() {
        bail!("installed synth-optimizers CLI failed offline");
    }
    Ok(())
}

fn read_capabilities(home: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(home.join("capabilities.json")).ok()?).ok()
}

/// Drop the stored handshake. Capabilities are evidence about one installed
/// version; they are lifecycle state, not durable configuration, and outliving
/// their version is how a stale attestation keeps satisfying gates.
fn clear_stored_capabilities(home: &Path) {
    let _ = fs::remove_file(home.join("capabilities.json"));
}

/// Does this 404 mean "no such route" rather than "no such run"?
///
/// The two arrive as the same status but mean opposite things for a live run:
/// a run that is not indexed yet may appear at any moment, while a route that
/// does not exist will never appear, so retrying it only pays for rollouts
/// nobody can ingest. `synth-optimizers` 0.2.5 labels both `run_not_found` —
/// its unknown-route fallback reuses the run code — so the distinction is only
/// available from runtimes that report it, and absence means "assume the
/// retryable one" rather than guessing.
fn events_not_found_is_missing_route(body: &Value) -> bool {
    body.get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        == Some("route_not_found")
}

/// Bound a diagnostic string and strip anything credential-shaped. Upstream
/// bodies are echoed into errors and logs, and the sidecar's bearer token is
/// the one secret in scope, so a stack trace that quotes a request must not
/// carry it along.
fn truncate_detail(detail: &str) -> String {
    const LIMIT: usize = 320;
    let redacted = redact_optimizer_secrets(detail.trim());
    if redacted.chars().count() <= LIMIT {
        return redacted;
    }
    let head: String = redacted.chars().take(LIMIT).collect();
    format!("{head}… (truncated)")
}

fn redact_optimizer_secrets(detail: &str) -> String {
    let mut out = detail.to_owned();
    // The key is minted by this process, so redact the live value rather than
    // pattern-matching a format that may change.
    if let Ok(key) = env::var("SYNTH_OPTIMIZER_API_KEY") {
        if key.len() >= 8 {
            out = out.replace(&key, "[redacted]");
        }
    }
    out
}

async fn upstream_failure_detail(response: reqwest::Response) -> String {
    let status = response.status();
    match response.text().await {
        Ok(text) if !text.trim().is_empty() => {
            format!("upstream {status}: {}", truncate_detail(&text))
        }
        _ => format!("upstream {status}"),
    }
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

fn truncate_diagnostic(value: &str, max_chars: usize) -> String {
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        truncated.push_str("… (truncated)");
    }
    truncated
}

fn diagnostic_error_message(error: &anyhow::Error) -> String {
    truncate_diagnostic(&error.to_string(), 2_000)
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
        ("GET", "/health") => {
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
        ("GET", "/v1/optimizer/capabilities") => {
            let upstream = client
                .get(format!("{upstream_base_url}/v1/optimizer/capabilities"))
                .timeout(crate::limits::OPTIMIZER_SIDECAR_HEALTH_TIMEOUT)
                .send()
                .await;
            let Ok(upstream) = upstream else {
                return JsonHttpResponse::error(
                    StatusCode::BAD_GATEWAY,
                    "optimizer capability endpoint is unavailable",
                );
            };
            // Preserve the upstream status. Collapsing everything to 502 made a
            // runtime that never implemented this route indistinguishable from
            // one that had crashed — the difference between "upgrade the
            // plugin" and "restart the service", reported identically.
            let status = upstream.status();
            if !status.is_success() {
                let detail = upstream_failure_detail(upstream).await;
                if status == StatusCode::NOT_FOUND {
                    return JsonHttpResponse::error(
                        StatusCode::NOT_FOUND,
                        format!("optimizer_capability_route_missing: {detail}"),
                    );
                }
                return JsonHttpResponse::error(
                    status,
                    format!("optimizer capability endpoint failed: {detail}"),
                );
            }
            match upstream.json::<Value>().await {
                Ok(body) => JsonHttpResponse::ok(body),
                Err(_) => JsonHttpResponse::error(
                    StatusCode::BAD_GATEWAY,
                    "optimizer service returned invalid capabilities",
                ),
            }
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
            // A non-JSON body must not rewrite the status. Reporting a plain
            // 404 as 502 turned a retryable "run not indexed yet" into a fatal
            // error on the poll loop's first tick, killing runs that were only
            // a moment from registering.
            let status = upstream.status();
            let text = upstream.text().await.unwrap_or_default();
            let parsed = serde_json::from_str::<Value>(&text).ok();
            match (status.is_success(), parsed) {
                (true, Some(body)) => JsonHttpResponse::ok(body),
                (true, None) => JsonHttpResponse::error(
                    StatusCode::BAD_GATEWAY,
                    "optimizer event endpoint returned invalid JSON",
                ),
                (false, Some(body)) => JsonHttpResponse::error(status, body.to_string()),
                (false, None) => JsonHttpResponse::error(status, truncate_detail(&text)),
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
    app: tauri::AppHandle,
    state: State<'_, Arc<OptimizerManager>>,
    codex: State<'_, Arc<crate::codex::CodexManager>>,
    version: Option<String>,
) -> Result<OptimizerSidecarVersion, AppError> {
    authorize_sidecar(&app, &codex, "install").await?;
    state.install(version.as_deref()).map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn optimizer_sidecar_start(
    app: tauri::AppHandle,
    state: State<'_, Arc<OptimizerManager>>,
    codex: State<'_, Arc<crate::codex::CodexManager>>,
) -> Result<OptimizerSidecarStatus, AppError> {
    authorize_sidecar(&app, &codex, "start").await?;
    state.start().await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn optimizer_sidecar_stop(
    app: tauri::AppHandle,
    state: State<'_, Arc<OptimizerManager>>,
    codex: State<'_, Arc<crate::codex::CodexManager>>,
) -> Result<OptimizerSidecarStatus, AppError> {
    authorize_sidecar(&app, &codex, "stop").await?;
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
    app: tauri::AppHandle,
    state: State<'_, Arc<OptimizerManager>>,
    core: State<'_, Arc<crate::CoreRuntime>>,
    codex: State<'_, Arc<crate::codex::CodexManager>>,
    version: String,
) -> Result<OptimizerSidecarStatus, AppError> {
    authorize_sidecar(&app, &codex, "uninstall").await?;
    state
        .uninstall(&version, core.optimizers())
        .await
        .map_err(AppError::from)
}

async fn authorize_sidecar(
    app: &tauri::AppHandle,
    codex: &crate::codex::CodexManager,
    action: &str,
) -> Result<(), AppError> {
    codex
        .approvals
        .authorize_host(
            app,
            None,
            crate::session::approval::ApprovalKind::SidecarLifecycle {
                sidecar: "optimizers".into(),
                action: action.into(),
            },
        )
        .await
        .map(|_| ())
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

    #[tokio::test]
    async fn gepa_admission_queue_bounds_concurrent_children() {
        let (manager, _) = manager();
        let first = manager.gepa_capacity.clone().acquire_owned().await.unwrap();
        let second = manager.gepa_capacity.clone().acquire_owned().await.unwrap();
        assert!(tokio::time::timeout(
            Duration::from_millis(10),
            manager.gepa_capacity.clone().acquire_owned()
        )
        .await
        .is_err());
        drop(first);
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            manager.gepa_capacity.clone().acquire_owned()
        )
        .await
        .is_ok());
        drop(second);
    }

    #[cfg(unix)]
    #[test]
    fn optimizer_launcher_survives_staging_activation() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join(".staging-runtime");
        let runtime = staging.join("runtime");
        let bin = runtime.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let python = bin.join("python");
        fs::write(&python, b"#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
        fs::set_permissions(&python, fs::Permissions::from_mode(0o755)).unwrap();

        write_relocatable_optimizer_launcher(&runtime).unwrap();
        let activated = dir.path().join("0.2.9.dev20260814");
        fs::rename(&staging, &activated).unwrap();
        let output = std::process::Command::new(activated.join("runtime/bin/synth-optimizers"))
            .arg("--help")
            .output()
            .unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("-m"));
        assert!(stdout.contains("synth_optimizers.cli"));
        assert!(stdout.contains("--help"));
        assert!(!stdout.contains(".staging-runtime"));
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
        assert!(!mgr.home().join("env.sh").exists());
        let started = mgr.start().await.unwrap();
        assert_eq!(started.phase, "ready");
        assert!(started.base_url.is_some());
        let env_path = mgr.home().join("env.sh");
        let env_body = fs::read_to_string(&env_path).unwrap();
        assert!(env_body.contains("SYNTH_OPTIMIZER_RUNTIME_EPOCH"));
        assert!(env_body.contains("SYNTH_OPTIMIZER_WRITTEN_AT"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&env_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let (pinned, pin) = mgr
            .pin_run(&svc, &run.id, "gepa.banking77.smoke.v1")
            .await
            .unwrap();
        assert_eq!(pinned.id, run.id);
        assert!(Path::new(&pin.spool_path).join("identity.json").is_file());
        let stopped = mgr.stop().await.unwrap();
        assert_ne!(stopped.phase, "ready");
        assert!(!env_path.exists());
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
    async fn process_group_cleanup_refuses_host_and_sentinel_pids() {
        assert_eq!(owned_process_group(0), None);
        assert_eq!(owned_process_group(1), None);
        assert_eq!(owned_process_group(u32::MAX), None);
        assert_eq!(owned_process_group(std::process::id()), None);

        // This child inherits the test runner's process group. Cleanup must
        // refuse it rather than signaling the test runner and its host apps.
        let mut child = Command::new("/bin/sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        assert_eq!(owned_process_group(pid), None);
        terminate_process_groups(&[0, 1, u32::MAX, std::process::id(), pid]).await;
        assert!(child.try_wait().unwrap().is_none());
        child.kill().await.unwrap();
        child.wait().await.unwrap();
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
            GepaWorkerState::Running { pid, _permit: None },
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
            GepaWorkerState::Running { pid, _permit: None },
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
        assert!(mgr.home().join("capabilities.json").is_file());
        assert!(mgr.home().join("env.sh").is_file());
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
        assert!(!mgr.home().join("capabilities.json").exists());
        assert!(!mgr.home().join("env.sh").exists());
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
        assert!(error.to_string().contains("plugin"), "{error:#}");
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

    /// A5. A missing route and a missing run share a status and must not share
    /// a fate: one is worth retrying, the other is worth failing on. 0.2.5
    /// cannot express the difference — its unknown-route fallback reuses the
    /// `run_not_found` code — so an unlabelled 404 stays retryable and only an
    /// explicit `route_not_found` short-circuits.
    ///
    /// Classification is asserted directly rather than through the in-process
    /// fake: teaching the fake to emit a code the pinned artifact never emits
    /// would make it diverge from the thing it stands in for, which is how this
    /// subsystem's suite came to pass over a missing endpoint in the first
    /// place. End-to-end coverage belongs to the real-artifact contract test.
    #[test]
    fn a_missing_route_is_distinguished_from_a_missing_run() {
        assert!(events_not_found_is_missing_route(&json!({
            "error": { "code": "route_not_found", "message": "route not found: GET /runs/x" }
        })));

        // What 0.2.5 actually returns for BOTH cases — verified live against the
        // installed wheel. Must stay retryable, or every run on a current
        // sidecar dies on its first poll.
        assert!(!events_not_found_is_missing_route(&json!({
            "error": { "code": "run_not_found", "message": "route not found: GET /runs/x" }
        })));

        // Nothing to go on: assume the retryable reading.
        assert!(!events_not_found_is_missing_route(&json!({})));
        assert!(!events_not_found_is_missing_route(&json!({ "error": {} })));
    }

    /// A5. Diagnostics quote upstream bodies, and the sidecar bearer token is
    /// the one secret in scope.
    #[test]
    fn upstream_detail_is_bounded_and_redacted() {
        let key = "synth-opt-0123456789abcdef0123456789abcdef";
        env::set_var("SYNTH_OPTIMIZER_API_KEY", key);
        let detail = truncate_detail(&format!("unauthorized for bearer {key} on /health"));
        assert!(
            !detail.contains(key),
            "token leaked into a diagnostic: {detail}"
        );
        assert!(detail.contains("[redacted]"));
        env::remove_var("SYNTH_OPTIMIZER_API_KEY");

        let long = "x".repeat(4096);
        let bounded = truncate_detail(&long);
        assert!(bounded.chars().count() < 400);
        assert!(bounded.ends_with("… (truncated)"));
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

    #[test]
    fn digest_mismatch_leaves_no_installed_version() {
        let (mgr, home) = manager();
        TEST_FORCE_DIGEST_MISMATCH.with(|flag| flag.set(true));
        let error = mgr.install(None).unwrap_err();
        TEST_FORCE_DIGEST_MISMATCH.with(|flag| flag.set(false));
        assert!(error.to_string().contains("digest") || error.to_string().contains("signature"));
        assert!(read_selected_version(home.path()).unwrap().is_none());
        assert!(!home
            .path()
            .join("versions")
            .join(DEFAULT_SIDECAR_VERSION)
            .exists());
    }

    #[test]
    fn interrupted_download_leaves_selected_version_unchanged() {
        let (mgr, home) = manager();
        mgr.install(None).unwrap();
        assert_eq!(
            read_selected_version(home.path()).unwrap().as_deref(),
            Some(DEFAULT_SIDECAR_VERSION)
        );
        TEST_INTERRUPT_INSTALL.with(|flag| flag.set(true));
        let error = mgr.install(None).unwrap_err();
        TEST_INTERRUPT_INSTALL.with(|flag| flag.set(false));
        assert!(error.to_string().contains("interrupted"));
        assert_eq!(
            read_selected_version(home.path()).unwrap().as_deref(),
            Some(DEFAULT_SIDECAR_VERSION)
        );
        assert!(mgr.has_offline_runtime(DEFAULT_SIDECAR_VERSION));
    }

    #[tokio::test]
    async fn installed_service_has_offline_runtime() {
        let (mgr, _home) = manager();
        let installed = mgr.install(None).unwrap();
        assert!(mgr.has_offline_runtime(&installed.version));
        assert!(Path::new(&installed.path)
            .join("runtime/bin/synth-optimizers")
            .is_file());
        assert!(Path::new(&installed.path)
            .join("package-lock.json")
            .is_file());
        assert!(Path::new(&installed.path)
            .join(WHEELHOUSE_MANIFEST_FILE)
            .is_file());
        let started = mgr.start().await.unwrap();
        assert_eq!(started.phase, "ready");
        let caps = mgr.advertised_capabilities();
        assert_eq!(caps["algorithms"][0], "gepa");
        assert!(
            caps.get("compatibleTemplateIds").is_none() && caps.get("recipes").is_none(),
            "the runtime-authored handshake must not echo Desktop vocabulary"
        );
        assert_eq!(caps["contractVersion"], "optimizer.contract.v1");
        assert_eq!(caps["serviceVersion"], OPTIMIZERS_CONTRACT.official);
        assert!(caps["digest"].as_str().unwrap().starts_with("sha256:"));
    }

    #[test]
    fn dev_catalog_version_installs_as_an_immutable_selection() {
        let (mgr, home) = manager();
        let installed = mgr.install(Some(DEV_SIDECAR_VERSION)).unwrap();
        assert_eq!(installed.version, DEV_SIDECAR_VERSION);
        assert_eq!(
            read_selected_version(home.path()).unwrap().as_deref(),
            Some(DEV_SIDECAR_VERSION)
        );
        assert!(home
            .path()
            .join("versions")
            .join(DEV_SIDECAR_VERSION)
            .join(MANIFEST_FILE)
            .is_file());
    }

    #[test]
    fn wheelhouse_manifest_tampering_fails_closed() {
        let (mgr, _home) = manager();
        let installed = mgr.install(None).unwrap();
        fs::write(
            Path::new(&installed.path).join(WHEELHOUSE_MANIFEST_FILE),
            br#"{"schemaVersion":"tampered","artifacts":[]}"#,
        )
        .unwrap();
        let error = load_verified_manifest(mgr.home(), Path::new(&installed.path)).unwrap_err();
        assert!(error.to_string().contains("schema") || error.to_string().contains("digest"));
    }
}
