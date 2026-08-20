//! Sidecar-owned training admission and dispatch.
//!
//! `OptimizerService` talks only to the authenticated sidecar proxy. The proxy
//! derives `sft` / `cispo` placements from the training routes it actually
//! serves, then either runs the documented hosted fixture or fans out to MLX and the
//! public SFT service. Hosted CISPO stays fail-closed until the slime clip
//! canary admits it.

use super::events::OptimizerEventDraft;
use super::mlx_runtime::MlxLoopback;
use super::training_adapter::{
    adapt_source_fact, ingest_ordered_events, promote_hosted_fact, TerminalMapping,
    TRAINING_ARTIFACT_MATERIALIZED,
};
use super::models::{
    OptimizerCapabilities, OptimizerCreateRequest, OptimizerExecutionBinding,
    OptimizerRecipeRunRequest, OptimizerResourceRef, OptimizerRunRecord,
};
use super::sft_client::SftOptimizerClient;
use super::OptimizerService;
use crate::ipc::{JsonHttpRequest, JsonHttpResponse};
use anyhow::{anyhow, bail, Context, Result};
use hyper::StatusCode;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Mutex};
use tokio::time::sleep;

pub const PLACEMENT_SEARCH_GEPA_LOCAL: &str = "search.gepa.local";
pub const PLACEMENT_SEARCH_GELO_HOSTED: &str = "search.gelo.hosted";
pub const PLACEMENT_TRAINING_SFT_LOCAL: &str = "training.sft.local";
pub const PLACEMENT_TRAINING_SFT_HOSTED: &str = "training.sft.hosted";
pub const PLACEMENT_TRAINING_CISPO_LOCAL: &str = "training.cispo.local";
pub const PLACEMENT_TRAINING_CISPO_HOSTED: &str = "training.cispo.hosted";

pub const LOCAL_MLX_SFT_RECIPE: &str = "sft.qwen35-0.8b.mlx.v1";
pub const LOCAL_MLX_CISPO_RECIPE: &str = "cispo.banking77.mlx.v1";
pub const HOSTED_CISPO_RECIPE: &str = "cispo.slime.hosted.v1";

const BASE_MODEL: &str = "Qwen/Qwen3.5-0.8B";
const HOSTED_SFT_FIXTURE_RECIPE: &str = "sft.hosted.fixture.v1";
const MAX_STEPS: u64 = 4;
const CHECKPOINT_EVERY: u64 = 2;
const LORA_RANK: u64 = 8;
const LORA_ALPHA: f64 = 16.0;
const MAX_SEQ_LENGTH: u64 = 4096;
const MAX_PAGE_ERRORS: u32 = 20;

#[derive(Clone, Default)]
pub struct TrainingRuntime {
    jobs: Arc<Mutex<HashMap<String, TrainingJob>>>,
}

#[derive(Clone)]
struct TrainingJob {
    placement: String,
    recipe_id: String,
    status: String,
    events: Vec<Value>,
    handoff: Value,
    cancelled: bool,
}

impl TrainingRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn handle(&self, request: &JsonHttpRequest) -> JsonHttpResponse {
        let (path, query) = split_path_query(&request.path);
        let method = request.method.as_str();
        match (method, path) {
            ("POST", "/v1/training/jobs") => self.create_job(&request.body).await,
            ("GET", path) if path.starts_with("/v1/training/jobs/") => {
                let rest = &path["/v1/training/jobs/".len()..];
                if let Some(id) = rest.strip_suffix("/events") {
                    self.job_events(id, query).await
                } else if let Some(id) = rest.strip_suffix("/handoff") {
                    self.job_handoff(id).await
                } else {
                    self.job_status(rest).await
                }
            }
            ("POST", path) if path.starts_with("/v1/training/jobs/") => {
                let rest = &path["/v1/training/jobs/".len()..];
                if let Some(id) = rest.strip_suffix("/cancel") {
                    self.cancel_job(id).await
                } else if let Some(id) = rest.strip_suffix("/resume") {
                    self.resume_job(id).await
                } else if let Some(id) = rest.strip_suffix("/chat") {
                    self.chat_checkpoint(id, &request.body).await
                } else {
                    JsonHttpResponse::error(StatusCode::NOT_FOUND, "not found")
                }
            }
            ("POST", path)
                if path.starts_with("/v1/training/artifacts/") && path.ends_with("/chat") =>
            {
                let id = path
                    .trim_start_matches("/v1/training/artifacts/")
                    .trim_end_matches("/chat")
                    .trim_end_matches('/');
                self.chat_artifact(id, &request.body).await
            }
            _ => JsonHttpResponse::error(StatusCode::NOT_FOUND, "not found"),
        }
    }

    async fn create_job(&self, body: &Value) -> JsonHttpResponse {
        let placement = body
            .get("placement")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if !admitted_placements().iter().any(|item| *item == placement) {
            let reason = if placement == PLACEMENT_TRAINING_CISPO_HOSTED {
                "hosted CISPO is fail-closed until the slime clip canary admits it"
            } else {
                "sidecar does not admit this training placement"
            };
            return JsonHttpResponse::error(StatusCode::CONFLICT, reason);
        }
        if matches!(
            placement.as_str(),
            PLACEMENT_TRAINING_SFT_LOCAL | PLACEMENT_TRAINING_CISPO_LOCAL
        ) {
            if let Err(error) = super::mlx_runtime::require_training_model() {
                return JsonHttpResponse::error(StatusCode::CONFLICT, error.to_string());
            }
        }
        let recipe_id = body
            .get("recipe_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let job_id = body
            .get("job_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("train_{}", uuid::Uuid::new_v4().simple()));
        if recipe_id.is_empty() {
            return JsonHttpResponse::error(StatusCode::BAD_REQUEST, "recipe_id is required");
        }
        {
            let mut jobs = self.jobs.lock().await;
            if jobs.contains_key(&job_id) {
                return JsonHttpResponse::error(
                    StatusCode::CONFLICT,
                    "training job already exists",
                );
            }
            jobs.insert(
                job_id.clone(),
                TrainingJob {
                    placement: placement.clone(),
                    recipe_id: recipe_id.clone(),
                    status: "queued".into(),
                    events: Vec::new(),
                    handoff: json!({}),
                    cancelled: false,
                },
            );
        }
        let runtime = self.clone();
        let config = body.get("config").cloned().unwrap_or_else(|| json!({}));
        let drive_id = job_id.clone();
        let drive_placement = placement.clone();
        let drive_recipe = recipe_id.clone();
        tokio::spawn(async move {
            if let Err(error) = runtime
                .drive_job(&drive_id, &drive_placement, &drive_recipe, config)
                .await
            {
                let mut jobs = runtime.jobs.lock().await;
                if let Some(job) = jobs.get_mut(&drive_id) {
                    job.status = "failed".into();
                    append_job_event(job, "job.failed", json!({"error": error.to_string()}));
                }
            }
        });
        JsonHttpResponse::ok(json!({
            "job_id": job_id,
            "status": "queued",
            "placement": placement,
            "recipe_id": recipe_id,
        }))
    }

    async fn drive_job(
        &self,
        job_id: &str,
        placement: &str,
        recipe_id: &str,
        config: Value,
    ) -> Result<()> {
        if placement == PLACEMENT_TRAINING_SFT_HOSTED
            && recipe_id == HOSTED_SFT_FIXTURE_RECIPE
            && simulate_training()
        {
            return self.simulate_job(job_id, placement, recipe_id).await;
        }
        match placement {
            PLACEMENT_TRAINING_SFT_LOCAL | PLACEMENT_TRAINING_CISPO_LOCAL => {
                drive_mlx_job(self, job_id, placement, &config).await
            }
            PLACEMENT_TRAINING_SFT_HOSTED => drive_hosted_sft_job(self, job_id, &config).await,
            PLACEMENT_TRAINING_CISPO_HOSTED => drive_hosted_cispo_job(self, job_id, &config).await,
            _ => bail!("unsupported training placement {placement}"),
        }
    }

    async fn simulate_job(&self, job_id: &str, placement: &str, recipe_id: &str) -> Result<()> {
        let algorithm = if placement.contains("cispo") {
            "cispo"
        } else {
            "sft"
        };
        {
            let mut jobs = self.jobs.lock().await;
            let job = jobs
                .get_mut(job_id)
                .ok_or_else(|| anyhow!("training job disappeared"))?;
            job.status = "running".into();
            append_job_event(
                job,
                "job.started",
                json!({"backend": algorithm, "recipe_id": recipe_id, "fixture": true}),
            );
            append_job_event(
                job,
                "training.metric",
                json!({
                    "step": 2,
                    "loss": 0.42,
                    "learning_rate": 0.00005,
                    "throughput_steps_per_second": 1.0
                }),
            );
            append_job_event(
                job,
                "checkpoint.created",
                json!({
                    "checkpoint_id": format!("{job_id}-ckpt-2"),
                    "step": 2,
                    "path": format!("/tmp/{job_id}/adapter"),
                    "sha256": "abc123",
                    "bytes": 128
                }),
            );
            append_job_event(
                job,
                "evaluation.completed",
                json!({
                    "schema_version": "synth_mlx_rl.paired_evaluation.v1",
                    "baseline_loss": 1.2,
                    "trained_loss": 0.4,
                    "item_count": 4
                }),
            );
            append_job_event(
                job,
                "heldout_eval.completed",
                json!({
                    "baseline_reward": 0.25,
                    "trained_reward": 0.75,
                    "heldout_instances": 4,
                    "world_ref": "world:fixture@heldout"
                }),
            );
            if algorithm == "cispo" {
                append_job_event(
                    job,
                    "training.clip",
                    json!({"eps_high": 4.0, "tinker_bound": 5.0, "identity": "1+eps_high"}),
                );
            }
            job.handoff = json!({
                "inference": {"kind": "mlx-lora.v1"},
                "checkpoint": {
                    "checkpoint_id": format!("{job_id}-terminal"),
                    "sha256": "deadbeef",
                    "path": format!("/tmp/{job_id}/terminal")
                },
                "policy_snapshot_id": format!("{job_id}-snap")
            });
            job.status = "succeeded".into();
            append_job_event(job, "job.succeeded", json!({"handoff": job.handoff}));
        }
        Ok(())
    }

    async fn job_status(&self, job_id: &str) -> JsonHttpResponse {
        let jobs = self.jobs.lock().await;
        match jobs.get(job_id) {
            Some(job) => JsonHttpResponse::ok(json!({
                "job_id": job_id,
                "status": job.status,
                "placement": job.placement,
                "recipe_id": job.recipe_id,
                "event_count": job.events.len(),
            })),
            None => JsonHttpResponse::error(StatusCode::NOT_FOUND, "training job not found"),
        }
    }

    async fn job_events(&self, job_id: &str, query: &str) -> JsonHttpResponse {
        let after = query_u64(query, "after").unwrap_or(0);
        let jobs = self.jobs.lock().await;
        let Some(job) = jobs.get(job_id) else {
            return JsonHttpResponse::error(StatusCode::NOT_FOUND, "training job not found");
        };
        let events = job
            .events
            .iter()
            .filter(|event| {
                event
                    .get("sequence")
                    .and_then(Value::as_u64)
                    .is_some_and(|sequence| sequence > after)
            })
            .cloned()
            .collect::<Vec<_>>();
        JsonHttpResponse::ok(json!({
            "job_id": job_id,
            "events": events,
            "status": job.status,
        }))
    }

    async fn job_handoff(&self, job_id: &str) -> JsonHttpResponse {
        let jobs = self.jobs.lock().await;
        match jobs.get(job_id) {
            Some(job) if job.status == "succeeded" => JsonHttpResponse::ok(job.handoff.clone()),
            Some(_) => JsonHttpResponse::error(StatusCode::CONFLICT, "handoff is not ready"),
            None => JsonHttpResponse::error(StatusCode::NOT_FOUND, "training job not found"),
        }
    }

    async fn cancel_job(&self, job_id: &str) -> JsonHttpResponse {
        let mut jobs = self.jobs.lock().await;
        let Some(job) = jobs.get_mut(job_id) else {
            return JsonHttpResponse::error(StatusCode::NOT_FOUND, "training job not found");
        };
        job.cancelled = true;
        if !matches!(job.status.as_str(), "succeeded" | "failed" | "cancelled") {
            job.status = "cancelled".into();
            append_job_event(job, "job.cancelled", json!({}));
        }
        let status = job.status.clone();
        let fixture = is_hosted_fixture(&job.recipe_id, &job.placement);
        drop(jobs);
        if !fixture {
            if let Ok(client) = MlxLoopback::from_env() {
                let _ = client
                    .post(&format!("/v1/jobs/{job_id}/cancel"), None)
                    .await;
            }
        }
        JsonHttpResponse::ok(json!({"job_id": job_id, "status": status}))
    }

    async fn resume_job(&self, job_id: &str) -> JsonHttpResponse {
        let fixture = {
            let jobs = self.jobs.lock().await;
            jobs.get(job_id)
                .is_some_and(|job| is_hosted_fixture(&job.recipe_id, &job.placement))
        };
        if !fixture {
            if let Err(error) = resume_mlx_job(job_id).await {
                return JsonHttpResponse::error(StatusCode::BAD_GATEWAY, error.to_string());
            }
        }
        let mut jobs = self.jobs.lock().await;
        let Some(job) = jobs.get_mut(job_id) else {
            return JsonHttpResponse::error(StatusCode::NOT_FOUND, "training job not found");
        };
        job.status = "running".into();
        append_job_event(job, "job.resumed", json!({"from_checkpoint": true}));
        JsonHttpResponse::ok(json!({"job_id": job_id, "status": "running"}))
    }

    async fn chat_checkpoint(&self, job_id: &str, body: &Value) -> JsonHttpResponse {
        match pin_for_job_chat(job_id, body) {
            Ok(pin) => self.chat_pinned(&pin, body).await,
            Err(error) => JsonHttpResponse::error(StatusCode::BAD_REQUEST, error.to_string()),
        }
    }

    async fn chat_artifact(&self, artifact_id: &str, body: &Value) -> JsonHttpResponse {
        match pin_for_artifact(artifact_id) {
            Ok(pin) => self.chat_pinned(&pin, body).await,
            Err(error) => JsonHttpResponse::error(StatusCode::BAD_REQUEST, error.to_string()),
        }
    }

    async fn chat_pinned(&self, pin: &InferencePin, body: &Value) -> JsonHttpResponse {
        let prompt = body
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("hello")
            .to_string();
        if pin.fixture {
            return JsonHttpResponse::ok(json!({
                "artifact_id": pin.artifact_id,
                "policy_snapshot_id": pin.snapshot_id,
                "reply": format!("fixture checkpoint reply to {prompt}")
            }));
        }
        match pin_and_chat(pin, &prompt).await {
            Ok(reply) => JsonHttpResponse::ok(json!({
                "artifact_id": pin.artifact_id,
                "policy_snapshot_id": pin.snapshot_id,
                "reply": reply
            })),
            Err(error) => JsonHttpResponse::error(StatusCode::BAD_GATEWAY, error.to_string()),
        }
    }
}

#[derive(Clone, Debug)]
struct InferencePin {
    artifact_id: Option<String>,
    snapshot_id: String,
    policy_dir: Option<PathBuf>,
    digest: Option<String>,
    fixture: bool,
}

fn optional_id(body: &Value, key: &str) -> Option<String> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn pin_for_artifact(artifact_id: &str) -> Result<InferencePin> {
    let artifact = crate::training_artifacts::get(artifact_id)?;
    if !artifact.is_inference_ready() {
        bail!(
            "training artifact `{artifact_id}` is not inference-ready ({})",
            artifact.integrity
        );
    }
    let policy_dir = artifact
        .path
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_dir());
    if policy_dir.is_none() {
        bail!("training artifact `{artifact_id}` has no adapter directory to load");
    }
    Ok(InferencePin {
        artifact_id: Some(artifact.id.clone()),
        snapshot_id: crate::training_artifacts::snapshot_id_for(&artifact),
        policy_dir,
        digest: artifact.digest.clone(),
        fixture: false,
    })
}

fn pin_for_job_chat(job_id: &str, body: &Value) -> Result<InferencePin> {
    if let Some(artifact_id) = optional_id(body, "artifact_id") {
        return pin_for_artifact(&artifact_id);
    }
    if let Some(snapshot_id) = optional_id(body, "policy_snapshot_id") {
        return Ok(InferencePin {
            artifact_id: None,
            snapshot_id,
            policy_dir: None,
            digest: None,
            fixture: false,
        });
    }
    if let Some(artifact) = crate::training_artifacts::list()?
        .into_iter()
        .find(|item| item.producing_run_id == job_id)
    {
        return pin_for_artifact(&artifact.id);
    }
    bail!("inference requires artifact_id or policy_snapshot_id; ambient latest is refused")
}

async fn pin_and_chat(pin: &InferencePin, prompt: &str) -> Result<String> {
    let client = MlxLoopback::ensure().await?;
    let snapshot_id = if let Some(dir) = &pin.policy_dir {
        client
            .register_policy(dir, &pin.snapshot_id, pin.digest.as_deref())
            .await?
    } else {
        pin.snapshot_id.clone()
    };
    client.chat(prompt, &snapshot_id).await
}

pub(crate) async fn launch_artifact_inference(artifact_id: &str, message: &str) -> Result<Value> {
    let pin = pin_for_artifact(artifact_id)?;
    let artifact = crate::training_artifacts::get(artifact_id)?;
    let reply = pin_and_chat(&pin, message).await?;
    Ok(json!({
        "artifactId": artifact.id,
        "policySnapshotId": pin.snapshot_id,
        "reply": reply,
        "baseModelId": artifact.base_model_id,
        "producingRunId": artifact.producing_run_id,
        "configDigest": artifact.config_digest,
        "digest": artifact.digest,
    }))
}

pub fn admitted_placements() -> Vec<&'static str> {
    let mut placements = vec![
        PLACEMENT_SEARCH_GEPA_LOCAL,
        PLACEMENT_SEARCH_GELO_HOSTED,
        PLACEMENT_TRAINING_SFT_LOCAL,
        PLACEMENT_TRAINING_SFT_HOSTED,
        PLACEMENT_TRAINING_CISPO_LOCAL,
    ];
    if hosted_cispo_admitted() {
        placements.push(PLACEMENT_TRAINING_CISPO_HOSTED);
    }
    placements
}

pub fn merge_training_capabilities(mut upstream: Value) -> Value {
    let object = match upstream.as_object_mut() {
        Some(object) => object,
        None => return upstream,
    };
    let mut algorithms = object
        .get("algorithms")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for algorithm in ["gepa", "sft", "cispo"] {
        if !algorithms
            .iter()
            .any(|item| item.as_str() == Some(algorithm))
        {
            algorithms.push(json!(algorithm));
        }
    }
    object.insert("algorithms".into(), Value::Array(algorithms));
    object.insert(
        "placements".into(),
        json!(admitted_placements()
            .into_iter()
            .map(Value::from)
            .collect::<Vec<_>>()),
    );
    object.insert("training".into(), json!(true));
    object.insert("replay".into(), json!(true));
    object.insert("cancellation".into(), json!(true));
    if object.get("contractVersion").is_none() {
        object.insert("contractVersion".into(), json!("optimizer.contract.v1"));
    }
    upstream
}

pub fn advertised_placement(capabilities: &Value, placement: &str) -> bool {
    capabilities
        .get("placements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|item| item == placement)
}

pub fn require_placement(capabilities: &Value, placement: &str) -> Result<()> {
    if advertised_placement(capabilities, placement) {
        return Ok(());
    }
    bail!("optimizer sidecar does not advertise placement `{placement}`")
}

pub fn simulate_training() -> bool {
    cfg!(test) || std::env::var("SYNTH_OPTIMIZER_TRAINING_FIXTURE").as_deref() == Ok("1")
}

fn is_hosted_fixture(recipe_id: &str, placement: &str) -> bool {
    placement == PLACEMENT_TRAINING_SFT_HOSTED && recipe_id == HOSTED_SFT_FIXTURE_RECIPE
}

fn hosted_cispo_admitted() -> bool {
    std::env::var("SYNTH_OPTIMIZERS_CISPO_HOSTED_ADMITTED").as_deref() == Ok("1")
}

fn append_job_event(job: &mut TrainingJob, kind: &str, payload: Value) {
    let sequence = job
        .events
        .iter()
        .filter_map(|event| event.get("sequence").and_then(Value::as_u64))
        .max()
        .unwrap_or(0)
        + 1;
    job.events.push(json!({
        "sequence": sequence,
        "type": kind,
        "kind": kind,
        "event_id": format!("{kind}:{sequence}"),
        "attempt_id": "attempt-1",
        "payload": payload,
    }));
}

fn split_path_query(path: &str) -> (&str, &str) {
    path.split_once('?').unwrap_or((path, ""))
}

fn query_u64(query: &str, key: &str) -> Option<u64> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| value.parse().ok())?
    })
}

#[derive(Clone)]
pub struct SidecarTrainingClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl SidecarTrainingClient {
    pub async fn from_manager(manager: &super::OptimizerManager) -> Result<Self> {
        let (base_url, token) = manager.sidecar_http().await?;
        Ok(Self {
            base_url,
            token,
            http: crate::http::http_client(),
        })
    }

    pub async fn create_job(&self, body: &Value) -> Result<Value> {
        self.post("/v1/training/jobs", Some(body)).await
    }

    pub async fn job(&self, job_id: &str) -> Result<Value> {
        self.get(&format!("/v1/training/jobs/{job_id}")).await
    }

    pub async fn events_after(&self, job_id: &str, after: u64) -> Result<Value> {
        self.get(&format!("/v1/training/jobs/{job_id}/events?after={after}"))
            .await
    }

    pub async fn handoff(&self, job_id: &str) -> Result<Value> {
        self.get(&format!("/v1/training/jobs/{job_id}/handoff"))
            .await
    }

    pub async fn cancel(&self, job_id: &str) -> Result<Value> {
        self.post(&format!("/v1/training/jobs/{job_id}/cancel"), None)
            .await
    }

    pub async fn resume(&self, job_id: &str) -> Result<Value> {
        self.post(&format!("/v1/training/jobs/{job_id}/resume"), None)
            .await
    }

    pub async fn chat(&self, job_id: &str, message: &str) -> Result<Value> {
        self.post(
            &format!("/v1/training/jobs/{job_id}/chat"),
            Some(&json!({ "message": message })),
        )
        .await
    }

    async fn get(&self, path: &str) -> Result<Value> {
        decode(
            self.http
                .get(format!("{}{path}", self.base_url))
                .bearer_auth(&self.token)
                .send()
                .await?,
            path,
        )
        .await
    }

    async fn post(&self, path: &str, body: Option<&Value>) -> Result<Value> {
        let mut request = self
            .http
            .post(format!("{}{path}", self.base_url))
            .bearer_auth(&self.token);
        if let Some(value) = body {
            request = request.json(value);
        }
        decode(request.send().await?, path).await
    }
}

async fn decode(response: reqwest::Response, operation: &str) -> Result<Value> {
    let status = response.status();
    let text = response
        .text()
        .await
        .context("read sidecar training body")?;
    if !status.is_success() {
        bail!(
            "sidecar training {operation} failed ({status}): {}",
            text.trim()
        );
    }
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&text).with_context(|| format!("decode sidecar training {operation}"))
}

pub async fn require_training_ready(
    service: &OptimizerService,
    placement: &str,
) -> Result<SidecarTrainingClient> {
    super::recipes::require_plugin_ready(service.manager()).await?;
    let capabilities = service.manager().advertised_capabilities();
    let algorithm = algorithm_for_placement(placement);
    let algorithms = capabilities
        .get("algorithms")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    if !algorithms
        .iter()
        .any(|item| *item == algorithm || *item == algorithm.split('.').next().unwrap_or(algorithm))
    {
        bail!("optimizer runtime does not advertise algorithm `{algorithm}`");
    }
    require_placement(&capabilities, placement)?;
    SidecarTrainingClient::from_manager(service.manager()).await
}

fn algorithm_for_placement(placement: &str) -> &'static str {
    if placement.contains("cispo") {
        "cispo"
    } else if placement.contains("sft") {
        "sft"
    } else {
        "gepa"
    }
}

pub async fn create_and_watch(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
    create: OptimizerCreateRequest,
    placement: &str,
    config: Value,
) -> Result<(OptimizerRunRecord, Option<crate::storage::AppEvent>)> {
    let client = require_training_ready(service, placement).await?;
    let run_id = create
        .id
        .clone()
        .ok_or_else(|| anyhow!("training recipe omitted run id"))?;
    client
        .create_job(&json!({
            "job_id": run_id,
            "placement": placement,
            "recipe_id": request.recipe_id,
            "config": config,
        }))
        .await?;
    let (run, event) = service.create(create).await?;
    spawn_watch_worker(service, client, run_id, 0).await;
    Ok((run, event))
}

pub async fn spawn_watch_worker(
    service: &OptimizerService,
    client: SidecarTrainingClient,
    run_id: String,
    cursor: u64,
) {
    let (cancel_tx, cancel_rx) = watch::channel(false);
    service
        .register_local_recipe(run_id.clone(), cancel_tx)
        .await;
    let worker = service.clone();
    tokio::spawn(async move {
        if let Err(error) =
            watch_job(worker.clone(), client, run_id.clone(), cursor, cancel_rx).await
        {
            let _ = append_failure(&worker, &run_id, &format!("{error:#}")).await;
        }
        worker.unregister_local_recipe(&run_id).await;
    });
}

async fn watch_job(
    service: OptimizerService,
    client: SidecarTrainingClient,
    run_id: String,
    mut cursor: u64,
    mut cancel: watch::Receiver<bool>,
) -> Result<()> {
    let mut errors = 0;
    loop {
        if *cancel.borrow() {
            let _ = client.cancel(&run_id).await;
        }
        let page = match client.events_after(&run_id, cursor).await {
            Ok(page) => {
                errors = 0;
                page
            }
            Err(_error) if errors < MAX_PAGE_ERRORS => {
                errors += 1;
                sleep(Duration::from_millis(200)).await;
                continue;
            }
            Err(error) => {
                return Err(error.context("sidecar training event polling stayed unavailable"))
            }
        };
        let algorithm = service.get(run_id.clone()).await?.algorithm_id;
        let (next, events) = ingest_ordered_events(
            cursor,
            page.get("events")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        )?;
        for event in events {
            append_mapped_event(&service, &run_id, &algorithm, &event).await?;
        }
        cursor = next;
        persist_cursor(&service, &run_id, cursor).await?;
        let job = client.job(&run_id).await?;
        match job
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("running")
        {
            "succeeded" => {
                settle_successful_job(&service, &client, &run_id).await?;
                return Ok(());
            }
            "failed" => {
                let reason = job
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                append_terminal_mapping(&service, &run_id, TerminalMapping::failed(reason)).await?;
                return Err(anyhow!("training job failed: {reason}"));
            }
            "cancelled" => {
                append_terminal_mapping(&service, &run_id, TerminalMapping::cancelled()).await?;
                append_status(&service, &run_id, "optimizer.run.cancelled", "cancelled").await?;
                return Ok(());
            }
            _ => {}
        }
        tokio::select! {
            _ = cancel.changed() => {}
            _ = sleep(Duration::from_millis(150)) => {}
        }
    }
}

pub async fn append_mapped_event(
    service: &OptimizerService,
    run_id: &str,
    algorithm: &str,
    event: &Value,
) -> Result<()> {
    let adapted = adapt_source_fact(algorithm, event)?;
    service
        .append_event_payloads(run_id.into(), vec![adapted.draft])
        .await?;
    Ok(())
}

async fn persist_handoff(
    service: &OptimizerService,
    client: &SidecarTrainingClient,
    run_id: &str,
) -> Result<Option<crate::training_artifacts::TrainingArtifact>> {
    let handoff = match client.handoff(run_id).await {
        Ok(handoff) => handoff,
        Err(error) => {
            let run = service.get(run_id.into()).await?;
            if run.source == "local" {
                return Err(error.context(
                    "training succeeded but the adapter handoff was unreachable; refusing optimizer.run.completed",
                ));
            }
            return Ok(None);
        }
    };
    let mut run = service.get(run_id.into()).await?;
    let mut materialized = None;
    if handoff.pointer("/inference/kind").and_then(Value::as_str) == Some("mlx-lora.v1") {
        let dataset_digest = run
            .summary
            .pointer("/datasetDigest")
            .and_then(Value::as_str)
            .map(str::to_string);
        let config_digest = run
            .summary
            .pointer("/configDigest")
            .and_then(Value::as_str)
            .map(str::to_string);
        let artifact = crate::training_artifacts::TrainingArtifact::from_mlx_handoff(
            run_id,
            &run.algorithm_id,
            crate::training_models::QWEN_TRAINING_MODEL_ID,
            &handoff,
            dataset_digest,
            config_digest,
        )
        .context(
            "training succeeded but the adapter could not be materialized; refusing optimizer.run.completed",
        )?;
        let stored = crate::training_artifacts::register(artifact.clone()).unwrap_or(artifact);
        run.output_refs.push(OptimizerResourceRef {
            kind: "checkpoint".into(),
            id: stored.id.clone(),
            digest: stored.digest.clone(),
            role: Some("terminal_adapter".into()),
            title: Some("Training adapter".into()),
            metadata: json!({
                "handoff": handoff,
                "trainingArtifact": stored,
            }),
        });
        materialized = Some(stored);
    }
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert("adapterHandoff".into(), handoff);
    run.summary = Value::Object(summary);
    service.persist_run(run).await?;
    Ok(materialized)
}

async fn settle_successful_job(
    service: &OptimizerService,
    client: &SidecarTrainingClient,
    run_id: &str,
) -> Result<()> {
    let run = service.get(run_id.into()).await?;
    let local = run.source == "local";
    let algorithm = run.algorithm_id.clone();
    let artifact = persist_handoff(service, client, run_id).await?;
    if local && artifact.is_none() {
        bail!(
            "training succeeded but the adapter artifact was not materialized; refusing optimizer.run.completed"
        );
    }
    let mapping = if let Some(artifact) = artifact.as_ref() {
        service
            .append_event_payloads(
                run_id.into(),
                vec![OptimizerEventDraft::new(TRAINING_ARTIFACT_MATERIALIZED, algorithm.clone())
                    .idempotency_key("training:artifact-materialized")
                    .item(json!({
                        "id": artifact.id,
                        "kind": artifact.adapter_kind,
                        "baseModel": artifact.base_model_id,
                        "digest": artifact.digest,
                        "producingRunId": artifact.producing_run_id,
                        "configDigest": artifact.config_digest,
                    }))
                    .artifact_refs(vec![json!({
                        "kind": "checkpoint",
                        "id": artifact.id,
                        "digest": artifact.digest
                    })])],
            )
            .await?;
        TerminalMapping::completed_after_artifact(&artifact.id)
    } else {
        TerminalMapping::completed_without_local_artifact()
    };
    append_terminal_mapping(service, run_id, mapping).await?;
    append_status(service, run_id, "optimizer.run.completed", "completed").await?;
    Ok(())
}

async fn append_terminal_mapping(
    service: &OptimizerService,
    run_id: &str,
    mapping: TerminalMapping,
) -> Result<()> {
    let algorithm = service.get(run_id.into()).await?.algorithm_id;
    service
        .append_event_payloads(
            run_id.into(),
            vec![mapping
                .draft(&algorithm)
                .idempotency_key(format!("training:terminal-mapped:{}", mapping.mapped_to))],
        )
        .await?;
    Ok(())
}

async fn persist_cursor(service: &OptimizerService, run_id: &str, cursor: u64) -> Result<()> {
    let mut run = service.get(run_id.into()).await?;
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert("trainingCursor".into(), json!(cursor));
    run.summary = Value::Object(summary);
    service.persist_run(run).await?;
    Ok(())
}

async fn append_status(
    service: &OptimizerService,
    run_id: &str,
    kind: &str,
    status: &str,
) -> Result<()> {
    let algorithm = service.get(run_id.into()).await?.algorithm_id;
    service
        .append_event_payloads(
            run_id.into(),
            vec![OptimizerEventDraft::new(kind, algorithm)
                .idempotency_key(format!("sidecar-training:{kind}"))
                .delta(Map::from_iter([("status".into(), json!(status))]))
                .raw(json!({"source":"sidecar-training"}))],
        )
        .await?;
    Ok(())
}

async fn append_failure(service: &OptimizerService, run_id: &str, reason: &str) -> Result<()> {
    let algorithm = service
        .get(run_id.into())
        .await
        .map(|run| run.algorithm_id)
        .unwrap_or_else(|_| "sft".into());
    service
        .append_event_payloads(
            run_id.into(),
            vec![OptimizerEventDraft::new("optimizer.run.failed", algorithm)
                .idempotency_key("sidecar-training:optimizer.run.failed")
                .level("error")
                .delta(Map::from_iter([("status".into(), json!("failed"))]))
                .error(json!({"message": reason}))
                .raw(json!({"source":"sidecar-training"}))],
        )
        .await?;
    Ok(())
}

pub fn training_create_request(
    run_id: &str,
    algorithm: &str,
    version: &str,
    objective: &str,
    source: &str,
    recipe_id: &str,
    request: &OptimizerRecipeRunRequest,
    summary: Value,
    input_refs: Vec<OptimizerResourceRef>,
) -> OptimizerCreateRequest {
    OptimizerCreateRequest {
        algorithm_id: algorithm.into(),
        algorithm_version: Some(version.into()),
        objective: Some(objective.into()),
        source: Some(source.into()),
        project_ref: Some(recipe_id.into()),
        session_ref: request.session_ref.clone(),
        id: Some(run_id.into()),
        execution_bindings: Some(vec![OptimizerExecutionBinding {
            kind: "optimizer_sidecar".into(),
            id: recipe_id.into(),
            label: Some("Optimizers sidecar training".into()),
            status: Some("admitted".into()),
            metadata: json!({"recipeId": recipe_id, "source": source}),
        }]),
        input_refs: Some(input_refs),
        capabilities: Some(OptimizerCapabilities::for_algorithm(algorithm)),
        summary: Some(summary),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    }
}

pub fn local_sft_config(run_id: &str, dataset: Option<&Path>, evaluation: Option<&Path>) -> Value {
    json!({
        "job_id": run_id,
        "config": {
            "backend": "qwen_lora",
            "base_model": BASE_MODEL,
            "dataset": dataset.map(|path| json!({"path": path})),
            "evaluation_dataset": evaluation.map(|path| json!({"path": path})),
            "output_dir": crate::instance::data_root().join("optimizers/mlx-sft").join(run_id),
            "max_steps": MAX_STEPS,
            "checkpoint_every": CHECKPOINT_EVERY,
            "learning_rate": 0.00005,
            "lora_rank": LORA_RANK,
            "lora_alpha": LORA_ALPHA,
            "lora_dropout": 0.0,
            "max_seq_length": MAX_SEQ_LENGTH,
            "enable_thinking": false,
            "seed": 0,
            "max_disk_bytes": 8589934592_u64
        }
    })
}

async fn drive_mlx_job(
    runtime: &TrainingRuntime,
    job_id: &str,
    placement: &str,
    config: &Value,
) -> Result<()> {
    super::mlx_runtime::require_training_model()?;
    let client = MlxLoopback::ensure().await?;
    let capabilities = client.get("/v1/capabilities").await?;
    if placement == PLACEMENT_TRAINING_CISPO_LOCAL
        && capabilities
            .pointer("/capabilities/cispo_training/supported")
            .and_then(Value::as_bool)
            != Some(true)
    {
        let mut jobs = runtime.jobs.lock().await;
        if let Some(job) = jobs.get_mut(job_id) {
            append_job_event(
                job,
                "cispo.no_learning_signal",
                json!({
                    "code": "cispo_training_unavailable",
                    "reason": capabilities.pointer("/capabilities/cispo_training/reason")
                }),
            );
            job.status = "failed".into();
        }
        bail!("MLX CISPO training is not advertised");
    }
    let payload = config
        .get("config")
        .cloned()
        .map(|inner| json!({"job_id": job_id, "config": inner}))
        .unwrap_or_else(|| json!({"job_id": job_id, "config": config}));
    client.post("/v1/jobs/preflight", Some(&payload)).await?;
    client.post("/v1/jobs", Some(&payload)).await?;
    client
        .post(&format!("/v1/jobs/{job_id}/launch"), None)
        .await?;
    let mut cursor = 0u64;
    loop {
        {
            let jobs = runtime.jobs.lock().await;
            if jobs.get(job_id).is_some_and(|job| job.cancelled) {
                let _ = client
                    .post(&format!("/v1/jobs/{job_id}/cancel"), None)
                    .await;
            }
        }
        let page = client
            .get(&format!("/v1/jobs/{job_id}/events?after={cursor}"))
            .await?;
        {
            let mut jobs = runtime.jobs.lock().await;
            let job = jobs
                .get_mut(job_id)
                .ok_or_else(|| anyhow!("training job disappeared"))?;
            job.status = "running".into();
            let (next, events) = ingest_ordered_events(
                cursor,
                page.get("events")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            )?;
            job.events.extend(events);
            cursor = next;
        }
        let remote = client.get(&format!("/v1/jobs/{job_id}")).await?;
        match remote
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("running")
        {
            "succeeded" => {
                let handoff = client
                    .get(&format!("/v1/jobs/{job_id}/handoff"))
                    .await
                    .unwrap_or_else(|_| json!({}));
                let mut jobs = runtime.jobs.lock().await;
                if let Some(job) = jobs.get_mut(job_id) {
                    job.handoff = handoff;
                    job.status = "succeeded".into();
                    if !job.events.iter().any(|event| {
                        event
                            .get("type")
                            .or_else(|| event.get("kind"))
                            .and_then(Value::as_str)
                            == Some("job.succeeded")
                    }) {
                        append_job_event(job, "job.succeeded", json!({}));
                    }
                }
                return Ok(());
            }
            "failed" | "interrupted" => bail!(
                "{}",
                remote
                    .get("error_detail")
                    .and_then(Value::as_str)
                    .unwrap_or("MLX job failed")
            ),
            "cancelled" => {
                let mut jobs = runtime.jobs.lock().await;
                if let Some(job) = jobs.get_mut(job_id) {
                    job.status = "cancelled".into();
                }
                return Ok(());
            }
            _ => sleep(Duration::from_millis(300)).await,
        }
    }
}

async fn resume_mlx_job(job_id: &str) -> Result<()> {
    let client = MlxLoopback::ensure().await?;
    let remote = client.get(&format!("/v1/jobs/{job_id}")).await?;
    refuse_resume_if_dropout(&remote)?;
    client
        .post(&format!("/v1/jobs/{job_id}/resume"), None)
        .await?;
    Ok(())
}

pub(super) fn refuse_resume_if_dropout(job: &Value) -> Result<()> {
    let dropout = job
        .pointer("/config/lora_dropout")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if dropout > 0.0 {
        bail!(
            "resume with lora_dropout={dropout} is refused; the MLX RNG stream is not persisted"
        );
    }
    Ok(())
}

async fn drive_hosted_sft_job(
    runtime: &TrainingRuntime,
    job_id: &str,
    config: &Value,
) -> Result<()> {
    let client = SftOptimizerClient::from_env()?;
    let toml = config
        .get("config_toml")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("hosted SFT job omitted config_toml"))?;
    client.submit_toml(job_id, toml).await?;
    let mut cursor = 0u64;
    loop {
        {
            let jobs = runtime.jobs.lock().await;
            if jobs.get(job_id).is_some_and(|job| job.cancelled) {
                let _ = client.cancel(job_id).await;
            }
        }
        let page = client.optimizer_events_after(job_id, cursor, 500).await?;
        {
            let mut jobs = runtime.jobs.lock().await;
            let job = jobs
                .get_mut(job_id)
                .ok_or_else(|| anyhow!("training job disappeared"))?;
            job.status = "running".into();
            let promoted = page
                .get("events")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(promote_hosted_fact)
                .collect::<Result<Vec<_>>>()?;
            let (next, events) = ingest_ordered_events(cursor, promoted)?;
            job.events.extend(events);
            cursor = next;
        }
        let remote = client.get_run(job_id).await?;
        match remote
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("running")
        {
            "succeeded" | "completed" => {
                let mut jobs = runtime.jobs.lock().await;
                if let Some(job) = jobs.get_mut(job_id) {
                    job.status = "succeeded".into();
                    append_job_event(job, "job.succeeded", json!({}));
                }
                return Ok(());
            }
            "failed" => bail!(
                "{}",
                remote
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("hosted SFT failed")
            ),
            "cancelled" => {
                let mut jobs = runtime.jobs.lock().await;
                if let Some(job) = jobs.get_mut(job_id) {
                    job.status = "cancelled".into();
                }
                return Ok(());
            }
            _ => sleep(Duration::from_millis(400)).await,
        }
    }
}

async fn drive_hosted_cispo_job(
    runtime: &TrainingRuntime,
    job_id: &str,
    config: &Value,
) -> Result<()> {
    if !hosted_cispo_admitted() {
        bail!("hosted CISPO is fail-closed until the slime clip canary admits it");
    }
    let client = super::hosted_client::HostedOptimizerClient::from_env()?;
    client.submit_json("cispo", job_id, config.clone()).await?;
    let mut jobs = runtime.jobs.lock().await;
    if let Some(job) = jobs.get_mut(job_id) {
        job.status = "running".into();
        append_job_event(job, "job.started", json!({"backend": "hosted-cispo"}));
    }
    Ok(())
}

pub fn optional_jsonl(var: &str) -> Option<PathBuf> {
    std::env::var(var).ok().and_then(|raw| {
        let path = PathBuf::from(raw.trim());
        path.is_file().then_some(path)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_merge_keeps_gepa_first_and_adds_training() {
        let merged = merge_training_capabilities(json!({
            "algorithms": ["gepa"],
            "replay": true,
            "cancellation": true
        }));
        assert_eq!(merged["algorithms"][0], "gepa");
        assert!(merged["algorithms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "sft"));
        assert!(merged["algorithms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "cispo"));
        assert!(merged["placements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == PLACEMENT_TRAINING_SFT_LOCAL));
        assert!(merged.get("recipes").is_none());
        assert!(merged.get("compatibleTemplateIds").is_none());
    }

    #[test]
    fn local_cispo_is_admitted_and_hosted_cispo_follows_the_canary_gate() {
        assert!(admitted_placements().contains(&PLACEMENT_TRAINING_CISPO_LOCAL));
        assert_eq!(
            admitted_placements().contains(&PLACEMENT_TRAINING_CISPO_HOSTED),
            hosted_cispo_admitted()
        );
    }

    #[test]
    fn real_mlx_event_payloads_map_to_workshop_events() {
        let golden = [
            (
                "training.metric",
                "sft.training.metrics",
            ),
            (
                "checkpoint.created",
                "sft.checkpoint.ready",
            ),
            (
                "heldout_eval.completed",
                "sft.heldout_evaluation.completed",
            ),
            (
                "training.clip",
                "cispo.clip.identity",
            ),
            (
                "job.succeeded",
                super::super::training_adapter::TRAINING_JOB_COMPLETED,
            ),
        ];
        for (kind, expected) in golden {
            assert_eq!(
                super::super::training_adapter::mapped_event_type("sft", kind),
                expected
            );
        }
        assert_eq!(
            super::super::training_adapter::mapped_event_type("cispo", "training.metric"),
            "training.metrics"
        );
    }

    #[test]
    fn resume_with_dropout_is_refused() {
        let error = refuse_resume_if_dropout(&json!({"config": {"lora_dropout": 0.1}}))
            .unwrap_err()
            .to_string();
        assert!(error.contains("lora_dropout"), "{error}");
        assert!(refuse_resume_if_dropout(&json!({"config": {"lora_dropout": 0.0}})).is_ok());
    }

    #[test]
    fn inference_without_an_artifact_or_snapshot_is_refused() {
        let error = pin_for_job_chat("job-missing", &json!({ "message": "hello" }))
            .unwrap_err()
            .to_string();
        assert!(error.contains("ambient latest"), "{error}");
        assert!(!error.contains("job-missing-snap"), "{error}");
    }
}
