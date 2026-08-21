//! Sidecar-owned training admission and dispatch.
//!
//! `OptimizerService` talks only to the authenticated sidecar proxy. The proxy
//! derives `sft` / `cispo` placements from the training routes it actually
//! serves, then fans out to MLX and the public SFT service. Hosted CISPO stays
//! fail-closed until the slime clip
//! canary admits it.

use super::events::OptimizerEventDraft;
use super::mlx_runtime::MlxLoopback;
use super::models::{
    CheckpointInferRequest, OptimizerCapabilities, OptimizerCreateRequest,
    OptimizerExecutionBinding, OptimizerRecipeRunRequest, OptimizerResourceRef, OptimizerRunRecord,
    SavedLoraCheckpoint, TrainingJobStatus,
};
use super::sft_client::SftOptimizerClient;
use super::training_adapter::TerminalMapping;
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
pub const LOCAL_MLX_CISPO_RECIPE: &str = "cispo.mlx.v1";
pub const HOSTED_CISPO_RECIPE: &str = "cispo.slime.hosted.v1";

const BASE_MODEL: &str = "Qwen/Qwen3.5-0.8B";
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
    status: TrainingJobStatus,
    events: Vec<Value>,
    handoff: Value,
    cancelled: bool,
    error: Option<String>,
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
            ("POST", "/v1/inference/chat/completions") => {
                self.infer_family("chat_completions", &request.body).await
            }
            ("POST", "/v1/inference/responses") => {
                self.infer_family("responses", &request.body).await
            }
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
            _ => JsonHttpResponse::error(StatusCode::NOT_FOUND, "not found"),
        }
    }

    async fn infer_family(&self, family: &str, body: &Value) -> JsonHttpResponse {
        match infer_from_sidecar_envelope(family, body).await {
            Ok(outcome) => {
                if let Some(sse) = outcome.sse {
                    JsonHttpResponse::sse(sse)
                } else {
                    JsonHttpResponse::ok(outcome.json)
                }
            }
            Err(error) => {
                let message = error.to_string();
                let status = if message.contains("does not advertise") {
                    StatusCode::NOT_IMPLEMENTED
                } else {
                    StatusCode::BAD_GATEWAY
                };
                JsonHttpResponse::error(status, message)
            }
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
                    status: TrainingJobStatus::Queued,
                    events: Vec::new(),
                    handoff: json!({}),
                    cancelled: false,
                    error: None,
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
                    job.status = TrainingJobStatus::Failed;
                    let message = format!("{error:#}");
                    job.error = Some(message.clone());
                    append_job_event(job, "job.failed", json!({"error": message}));
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
        _recipe_id: &str,
        config: Value,
    ) -> Result<()> {
        match placement {
            PLACEMENT_TRAINING_SFT_LOCAL | PLACEMENT_TRAINING_CISPO_LOCAL => {
                drive_mlx_job(self, job_id, placement, &config).await
            }
            PLACEMENT_TRAINING_SFT_HOSTED => drive_hosted_sft_job(self, job_id, &config).await,
            PLACEMENT_TRAINING_CISPO_HOSTED => drive_hosted_cispo_job(self, job_id, &config).await,
            _ => bail!("unsupported training placement {placement}"),
        }
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
                "error": job.error,
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
            Some(job) if job.status == TrainingJobStatus::Succeeded => {
                JsonHttpResponse::ok(job.handoff.clone())
            }
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
        if !job.status.is_terminal() {
            job.status = TrainingJobStatus::Cancelled;
            append_job_event(job, "job.cancelled", json!({}));
        }
        let status = job.status.clone();
        let local = matches!(
            job.placement.as_str(),
            PLACEMENT_TRAINING_SFT_LOCAL | PLACEMENT_TRAINING_CISPO_LOCAL
        );
        drop(jobs);
        if local {
            if let Ok(client) = MlxLoopback::from_env() {
                let _ = client
                    .post(&format!("/v1/jobs/{job_id}/cancel"), None)
                    .await;
            }
        }
        JsonHttpResponse::ok(json!({"job_id": job_id, "status": status}))
    }

    async fn resume_job(&self, job_id: &str) -> JsonHttpResponse {
        let local = {
            let jobs = self.jobs.lock().await;
            jobs.get(job_id).is_some_and(|job| {
                matches!(
                    job.placement.as_str(),
                    PLACEMENT_TRAINING_SFT_LOCAL | PLACEMENT_TRAINING_CISPO_LOCAL
                )
            })
        };
        if !local {
            return JsonHttpResponse::error(
                StatusCode::CONFLICT,
                "resume is available only for real local MLX training jobs",
            );
        }
        if let Err(error) = resume_mlx_job(job_id).await {
            return JsonHttpResponse::error(StatusCode::BAD_GATEWAY, error.to_string());
        }
        let mut jobs = self.jobs.lock().await;
        let Some(job) = jobs.get_mut(job_id) else {
            return JsonHttpResponse::error(StatusCode::NOT_FOUND, "training job not found");
        };
        job.status = TrainingJobStatus::Running;
        append_job_event(job, "job.resumed", json!({"from_checkpoint": true}));
        JsonHttpResponse::ok(json!({"job_id": job_id, "status": "running"}))
    }

    async fn chat_checkpoint(&self, job_id: &str, body: &Value) -> JsonHttpResponse {
        let jobs = self.jobs.lock().await;
        let Some(job) = jobs.get(job_id) else {
            return JsonHttpResponse::error(StatusCode::NOT_FOUND, "training job not found");
        };
        let snapshot = job
            .handoff
            .get("policy_snapshot_id")
            .cloned()
            .unwrap_or_else(|| json!(format!("{job_id}-snap")));
        let prompt = body
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("hello")
            .to_string();
        let local = matches!(
            job.placement.as_str(),
            PLACEMENT_TRAINING_SFT_LOCAL | PLACEMENT_TRAINING_CISPO_LOCAL
        );
        drop(jobs);
        if local {
            let snapshot_id = snapshot.as_str().unwrap_or(job_id);
            match MlxLoopback::from_env() {
                Ok(client) => match client.chat(&prompt, snapshot_id).await {
                    Ok(reply) => {
                        return JsonHttpResponse::ok(json!({
                            "job_id": job_id,
                            "policy_snapshot_id": snapshot,
                            "reply": reply
                        }));
                    }
                    Err(error) => {
                        return JsonHttpResponse::error(StatusCode::BAD_GATEWAY, error.to_string());
                    }
                },
                Err(error) => {
                    return JsonHttpResponse::error(StatusCode::BAD_GATEWAY, error.to_string());
                }
            }
        }
        JsonHttpResponse::error(
            StatusCode::CONFLICT,
            "checkpoint chat is available only for real local MLX training jobs",
        )
    }
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

fn hosted_cispo_admitted() -> bool {
    std::env::var("SYNTH_OPTIMIZERS_CISPO_HOSTED_ADMITTED").as_deref() == Ok("1")
}

fn append_job_event(job: &mut TrainingJob, kind: &str, payload: Value) {
    let sequence = job.events.len() as u64 + 1;
    job.events.push(json!({
        "sequence": sequence,
        "type": kind,
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
    if !service
        .try_register_local_recipe(run_id.clone(), cancel_tx)
        .await
    {
        return;
    }
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
        for event in page
            .get("events")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let sequence = event
                .get("sequence")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("training event omitted sequence"))?;
            if sequence != cursor + 1 {
                bail!("training event sequence gap after {cursor}: {sequence}");
            }
            append_mapped_event(&service, &run_id, &algorithm, &event, sequence).await?;
            if event.get("type").and_then(Value::as_str) == Some("checkpoint.created") {
                let payload = event.get("payload").cloned().unwrap_or_else(|| json!({}));
                let _ = service
                    .upsert_local_lora_from_event(run_id.clone(), payload)
                    .await;
            }
            cursor = sequence;
        }
        persist_cursor(&service, &run_id, cursor).await?;
        let job = client.job(&run_id).await?;
        match job
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("running")
        {
            "succeeded" => {
                persist_handoff(&service, &client, &run_id).await?;
                append_status(&service, &run_id, "optimizer.run.completed", "completed").await?;
                return Ok(());
            }
            "failed" => {
                let detail = job
                    .get("error")
                    .and_then(|value| {
                        value
                            .get("error")
                            .and_then(Value::as_str)
                            .or_else(|| value.get("error_code").and_then(Value::as_str))
                            .or_else(|| value.as_str())
                            .map(str::to_string)
                            .or_else(|| Some(value.to_string()))
                    })
                    .unwrap_or_else(|| "unknown error".into());
                append_terminal_mapping(&service, &run_id, TerminalMapping::failed(&detail))
                    .await?;
                return Err(anyhow!("training job failed: {detail}"));
            }
            "cancelled" => {
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
    sequence: u64,
) -> Result<()> {
    let kind = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("job.event");
    let payload = event.get("payload").cloned().unwrap_or_else(|| json!({}));
    let draft = mapped_event_draft(kind, algorithm, &payload);
    service
        .append_event_payloads(
            run_id.into(),
            vec![draft
                .idempotency_key(format!("sidecar-training:{sequence}"))
                .raw(event.clone())],
        )
        .await?;
    Ok(())
}

fn mapped_event_draft(kind: &str, algorithm: &str, payload: &Value) -> OptimizerEventDraft {
    match kind {
        "job.started" => OptimizerEventDraft::new("optimizer.run.started", algorithm)
            .delta(Map::from_iter([("status".into(), json!("running"))])),
        "job.resumed" => OptimizerEventDraft::new("optimizer.run.resumed", algorithm)
            .delta(Map::from_iter([("status".into(), json!("running"))])),
        "training.metric" => {
            OptimizerEventDraft::new("sft.training.metrics", algorithm).delta(Map::from_iter([
                ("step".into(), payload["step"].clone()),
                ("train_loss".into(), payload["loss"].clone()),
                ("learning_rate".into(), payload["learning_rate"].clone()),
                ("throughput".into(), payload["tokens_per_second"].clone()),
            ]))
        }
        "checkpoint.created" => OptimizerEventDraft::new("sft.checkpoint.ready", algorithm)
            .item(json!({
                "id": payload["checkpoint_id"],
                "step": payload["step"],
                "status": "ready",
                "ready": true,
                "path": payload["path"],
                "sha256": payload["sha256"],
                "bytes": payload["bytes"],
                "kind": "mlx-lora.v1",
                "raw": payload
            }))
            .artifact_refs(vec![json!({
                "kind": "checkpoint",
                "id": payload["checkpoint_id"],
                "uri": payload["path"],
                "digest": payload["sha256"]
            })]),
        kind if kind.ends_with("evaluation.completed") || kind.ends_with("eval.completed") => {
            let detail = payload.get("delta").unwrap_or(payload);
            let phase = evaluation_phase(kind, detail);
            let checkpoint_id = detail
                .get("checkpoint_id")
                .or_else(|| detail.get("artifact_id"))
                .cloned()
                .unwrap_or(Value::Null);
            OptimizerEventDraft::new("training.evaluation.completed", algorithm)
                .delta(Map::from_iter([
                    ("phase".into(), json!(phase)),
                    ("algorithm".into(), json!(algorithm)),
                    ("checkpoint_id".into(), checkpoint_id),
                    (
                        "evaluation".into(),
                        normalized_sidecar_evaluation(kind, algorithm, detail),
                    ),
                ]))
                .item(normalized_sidecar_evaluation(kind, algorithm, detail))
        }
        "training.clip" => OptimizerEventDraft::new("cispo.clip.identity", algorithm)
            .delta(Map::from_iter([("clip".into(), payload.clone())])),
        "cispo.no_learning_signal" => {
            OptimizerEventDraft::new("cispo.no_learning_signal", algorithm)
                .level("error")
                .error(payload.clone())
        }
        _ => OptimizerEventDraft::new(format!("training.{kind}"), algorithm),
    }
}

fn evaluation_phase<'a>(kind: &str, payload: &'a Value) -> &'a str {
    let phase = payload
        .get("evaluation_phase")
        .or_else(|| payload.get("role"))
        .or_else(|| payload.get("phase"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            if kind.contains("baseline") {
                "baseline"
            } else if kind.contains("checkpoint") {
                "checkpoint"
            } else {
                "final"
            }
        });
    match phase {
        "before" | "base" | "untrained" => "baseline",
        "trained" | "terminal" | "heldout" => "final",
        other => other,
    }
}

fn normalized_sidecar_evaluation(kind: &str, algorithm: &str, payload: &Value) -> Value {
    let checkpoint_id = payload
        .get("checkpoint_id")
        .or_else(|| payload.get("artifact_id"))
        .cloned()
        .unwrap_or(Value::Null);
    let artifact_digest = payload
        .get("artifact_digest")
        .or_else(|| payload.get("sha256"))
        .cloned()
        .unwrap_or(Value::Null);
    let score = payload
        .get("score")
        .or_else(|| payload.get("reward"))
        .or_else(|| payload.get("mean_reward"))
        .or_else(|| payload.pointer("/metrics/score"))
        .cloned()
        .unwrap_or(Value::Null);
    let baseline = payload
        .get("baseline_score")
        .or_else(|| payload.pointer("/comparison/baseline_score"))
        .cloned()
        .unwrap_or(Value::Null);
    let delta = payload
        .get("delta")
        .or_else(|| payload.pointer("/comparison/delta"))
        .cloned()
        .unwrap_or_else(|| match (score.as_f64(), baseline.as_f64()) {
            (Some(score), Some(baseline)) => json!(score - baseline),
            _ => Value::Null,
        });
    json!({
        "schema_version": "training.evaluation.v1",
        "phase": evaluation_phase(kind, payload),
        "algorithm": algorithm,
        "placement": payload.get("placement").cloned().unwrap_or(Value::Null),
        "checkpoint_id": checkpoint_id,
        "artifact_digest": artifact_digest,
        "candidate": {
            "exact_checkpoint": !checkpoint_id.is_null() && !artifact_digest.is_null(),
            "checkpoint_id": checkpoint_id,
            "artifact_digest": artifact_digest
        },
        "step": payload.get("step").cloned().unwrap_or(Value::Null),
        "evaluator": payload.get("evaluator").or_else(|| payload.get("plan_ref")).cloned().unwrap_or(Value::Null),
        "container": payload.get("container").or_else(|| payload.get("container_url")).cloned().unwrap_or(Value::Null),
        "metric": payload.get("metric").cloned().unwrap_or_else(|| json!("reward")),
        "score": score,
        "loss": payload.get("loss").or_else(|| payload.pointer("/metrics/loss")).cloned().unwrap_or(Value::Null),
        "baseline_score": baseline,
        "delta": delta,
        "sample_count": payload.get("sample_count").or_else(|| payload.get("samples")).or_else(|| payload.get("instances")).cloned().unwrap_or(Value::Null),
        "status": payload.get("status").cloned().unwrap_or_else(|| json!("completed")),
        "detail": payload,
    })
}

async fn persist_handoff(
    service: &OptimizerService,
    client: &SidecarTrainingClient,
    run_id: &str,
) -> Result<()> {
    let Ok(handoff) = client.handoff(run_id).await else {
        return Ok(());
    };
    let mut run = service.get(run_id.into()).await?;
    if handoff.pointer("/inference/kind").and_then(Value::as_str) == Some("mlx-lora.v1") {
        run.output_refs.push(OptimizerResourceRef {
            kind: "checkpoint".into(),
            id: handoff["checkpoint"]["checkpoint_id"]
                .as_str()
                .unwrap_or("terminal")
                .into(),
            digest: handoff["checkpoint"]["sha256"]
                .as_str()
                .map(|value| format!("sha256:{value}")),
            role: Some("terminal_adapter".into()),
            title: Some("Training adapter".into()),
            metadata: handoff.clone(),
        });
        let mut payload = handoff
            .get("checkpoint")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if let Some(object) = payload.as_object_mut() {
            if let Some(path) = handoff.pointer("/inference/path").and_then(Value::as_str) {
                object.insert("path".into(), json!(path));
            }
            if let Some(sha) = handoff
                .pointer("/checkpoint/sha256")
                .and_then(Value::as_str)
            {
                object.insert("sha256".into(), json!(sha));
            }
        }
        let _ = service
            .upsert_local_lora_from_event(run_id.to_string(), payload)
            .await;
    }
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert("adapterHandoff".into(), handoff);
    run.summary = Value::Object(summary);
    service.persist_run(run).await?;
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

pub fn local_sft_config(
    run_id: &str,
    dataset: Option<&Path>,
    evaluation: Option<&Path>,
    bind: Option<&super::container_training::ContainerTrainingBind>,
) -> Value {
    let task = bind
        .map(|bind| bind.task_id.clone())
        .or_else(|| {
            std::env::var("SYNTH_MLX_SFT_TASK_ID")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "local".into());
    let evaluator = bind
        .and_then(|bind| bind.cispo.as_ref())
        .map(|cispo| EvaluationContract {
            task: task.clone(),
            harness: cispo.harness.clone(),
            plan_ref: cispo.plan_ref.clone(),
            world_ref: cispo.heldout_world_ref.clone(),
        })
        .unwrap_or_else(|| EvaluationContract::from_task(&task));
    let container_url = bind
        .map(|bind| bind.base_url.clone())
        .or_else(|| std::env::var("SYNTH_MLX_SFT_EVAL_URL").ok());
    let evaluation_plan = tunneled_evaluation_plan(
        container_url,
        "SYNTH_MLX_SFT_EVAL_URL",
        "SYNTH_MLX_SFT_EVAL_TOKEN",
        CHECKPOINT_EVERY,
        vec![CHECKPOINT_EVERY, MAX_STEPS],
        evaluator,
    );
    json!({
        "job_id": run_id,
        "config": {
            "backend": "qwen_lora",
            "base_model": BASE_MODEL,
            "task_id": task,
            "dataset": dataset.map(|path| json!({"path": path})),
            "evaluation_dataset": evaluation.map(|path| json!({"path": path})),
            "output_dir": crate::instance::data_root().join("optimizers/mlx-sft").join(run_id),
            "max_steps": MAX_STEPS,
            "checkpoint_every": CHECKPOINT_EVERY,
            "evaluation": evaluation_plan,
            "learning_rate": 0.00005,
            "lora_rank": LORA_RANK,
            "lora_alpha": LORA_ALPHA,
            "max_seq_length": MAX_SEQ_LENGTH,
            "enable_thinking": false,
            "seed": 0,
            "max_disk_bytes": 8589934592_u64
        }
    })
}

pub struct EvaluationContract {
    pub task: String,
    pub harness: String,
    pub plan_ref: String,
    pub world_ref: String,
}

impl EvaluationContract {
    pub fn from_task(task: &str) -> Self {
        let harness = std::env::var("SYNTH_TRAINING_EVAL_HARNESS")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "rollout".into());
        let plan_ref = std::env::var("SYNTH_TRAINING_EVAL_PLAN_REF")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("{task}_eval.v1"));
        let world_ref = std::env::var("SYNTH_TRAINING_EVAL_WORLD_REF")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("world:{task}@heldout"));
        Self {
            task: task.to_string(),
            harness,
            plan_ref,
            world_ref,
        }
    }
}

pub fn tunneled_evaluation_plan(
    container_url: Option<String>,
    container_url_env: &str,
    bearer_token_env: &str,
    checkpoint_every: u64,
    checkpoint_steps: Vec<u64>,
    evaluator: EvaluationContract,
) -> Value {
    json!({
        "schema_version": "training.evaluation.plan.v1",
        "required": true,
        "transport": "tunnel",
        "container": {
            "url": container_url,
            "url_env": container_url_env,
            "auth_bearer_env": bearer_token_env,
            "lease_owner": "optimizers_sidecar"
        },
        "schedule": {
            "phases": ["baseline", "checkpoint", "final"],
            "checkpoint_every": checkpoint_every,
            "checkpoint_steps": checkpoint_steps
        },
        "candidate": {
            "exact_checkpoint_required": true,
            "baseline_target": "base_model",
            "checkpoint_target": "immutable_artifact",
            "final_target": "terminal_artifact"
        },
        "evaluator": {
            "task": evaluator.task,
            "harness": evaluator.harness,
            "plan_ref": evaluator.plan_ref,
            "world_ref": evaluator.world_ref,
            "metric": "reward",
            "seeds": [1, 2],
            "sample_count": 16,
            "timeout_s": 120
        }
    })
}

fn validate_tunneled_evaluation_plan(config: &Value) -> Result<()> {
    let plan = config
        .get("evaluation")
        .ok_or_else(|| anyhow!("training request omitted required evaluation plan"))?;
    for pointer in [
        "/schema_version",
        "/transport",
        "/container/url_env",
        "/container/auth_bearer_env",
        "/schedule/phases",
        "/schedule/checkpoint_steps",
        "/candidate/exact_checkpoint_required",
        "/evaluator/harness",
        "/evaluator/plan_ref",
        "/evaluator/world_ref",
        "/evaluator/metric",
        "/evaluator/seeds",
        "/evaluator/sample_count",
        "/evaluator/timeout_s",
    ] {
        if plan.pointer(pointer).map_or(true, Value::is_null) {
            bail!("training evaluation plan omitted {pointer}");
        }
    }
    if plan.get("required").and_then(Value::as_bool) != Some(true)
        || plan.get("transport").and_then(Value::as_str) != Some("tunnel")
        || plan
            .pointer("/candidate/exact_checkpoint_required")
            .and_then(Value::as_bool)
            != Some(true)
        || plan.pointer("/schedule/phases") != Some(&json!(["baseline", "checkpoint", "final"]))
    {
        bail!("training evaluation plan must require tunneled baseline/checkpoint/final exact-artifact evaluation");
    }
    Ok(())
}

async fn drive_mlx_job(
    runtime: &TrainingRuntime,
    job_id: &str,
    placement: &str,
    config: &Value,
) -> Result<()> {
    let inner_config = config.get("config").unwrap_or(config);
    validate_tunneled_evaluation_plan(inner_config)?;
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
            job.status = TrainingJobStatus::Failed;
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
            job.status = TrainingJobStatus::Running;
            for event in page
                .get("events")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                let sequence = event.get("sequence").and_then(Value::as_u64).unwrap_or(0);
                if sequence > cursor {
                    job.events.push(event);
                    cursor = sequence;
                }
            }
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
                    job.status = TrainingJobStatus::Succeeded;
                    append_job_event(job, "job.succeeded", json!({}));
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
                    job.status = TrainingJobStatus::Cancelled;
                }
                return Ok(());
            }
            _ => sleep(Duration::from_millis(300)).await,
        }
    }
}

async fn resume_mlx_job(job_id: &str) -> Result<()> {
    let client = MlxLoopback::ensure().await?;
    client
        .post(&format!("/v1/jobs/{job_id}/resume"), None)
        .await?;
    Ok(())
}

pub(crate) async fn launch_artifact_inference(artifact_id: &str, message: &str) -> Result<Value> {
    let artifact = crate::training_artifacts::get(artifact_id)?;
    let policy_dir = artifact
        .path
        .as_deref()
        .ok_or_else(|| anyhow!("training artifact `{artifact_id}` has no local adapter path"))?;
    let requested_snapshot = crate::training_artifacts::snapshot_id_for(&artifact);
    let client = MlxLoopback::ensure().await?;
    let policy_snapshot_id = client
        .register_policy(
            std::path::Path::new(policy_dir),
            &requested_snapshot,
            artifact.digest.as_deref(),
        )
        .await?;
    let reply = client.chat(message, &policy_snapshot_id).await?;
    Ok(json!({
        "artifactId": artifact.id,
        "policySnapshotId": policy_snapshot_id,
        "reply": reply,
        "baseModelId": artifact.base_model_id,
        "producingRunId": artifact.producing_run_id,
        "configDigest": artifact.config_digest,
        "digest": artifact.digest,
    }))
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
    let mut page_errors = 0u32;
    loop {
        {
            let jobs = runtime.jobs.lock().await;
            if jobs.get(job_id).is_some_and(|job| job.cancelled) {
                let _ = client.cancel(job_id).await;
            }
        }
        let page = match client.optimizer_events_after(job_id, cursor, 500).await {
            Ok(page) => {
                page_errors = 0;
                page
            }
            Err(_error) if page_errors < MAX_PAGE_ERRORS => {
                page_errors += 1;
                sleep(Duration::from_millis(250)).await;
                continue;
            }
            Err(error) => {
                return Err(error.context("hosted SFT event polling stayed unavailable"));
            }
        };
        {
            let mut jobs = runtime.jobs.lock().await;
            let job = jobs
                .get_mut(job_id)
                .ok_or_else(|| anyhow!("training job disappeared"))?;
            job.status = TrainingJobStatus::Running;
            for event in page
                .get("events")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                let sequence = event
                    .get("sequence_number")
                    .or_else(|| event.get("sequence"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if sequence > cursor {
                    job.events.push(json!({
                        "sequence": sequence,
                        "type": event.get("type").cloned().unwrap_or_else(|| json!("hosted.event")),
                        "payload": event,
                    }));
                    cursor = sequence;
                }
            }
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
                    job.status = TrainingJobStatus::Succeeded;
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
                    job.status = TrainingJobStatus::Cancelled;
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
    validate_tunneled_evaluation_plan(config)?;
    if !hosted_cispo_admitted() {
        bail!("hosted CISPO is fail-closed until the slime clip canary admits it");
    }
    let client = super::hosted_client::HostedOptimizerClient::from_env()?;
    client.submit_json("cispo", job_id, config.clone()).await?;
    let mut jobs = runtime.jobs.lock().await;
    if let Some(job) = jobs.get_mut(job_id) {
        job.status = TrainingJobStatus::Running;
        append_job_event(job, "job.started", json!({"backend": "hosted-cispo"}));
    }
    Ok(())
}

pub async fn infer_checkpoint<F>(
    service: &OptimizerService,
    request: CheckpointInferRequest,
    mut on_delta: F,
) -> Result<Value>
where
    F: FnMut(&str) + Send,
{
    let family = normalize_family(&request.family)?;
    if let Some(checkpoint) = service
        .get_local_lora(request.checkpoint_id.clone())
        .await?
    {
        return Ok(
            infer_local(&checkpoint, family, &request.body, &mut on_delta)
                .await?
                .json,
        );
    }
    Ok(
        infer_hosted(&request.checkpoint_id, family, &request.body, &mut on_delta)
            .await?
            .json,
    )
}

struct InferOutcome {
    json: Value,
    sse: Option<Vec<u8>>,
}

async fn infer_from_sidecar_envelope(family: &str, envelope: &Value) -> Result<InferOutcome> {
    let family = normalize_family(family)?;
    let body = envelope
        .get("body")
        .cloned()
        .unwrap_or_else(|| envelope.clone());
    let placement = envelope
        .get("placement")
        .and_then(Value::as_str)
        .unwrap_or("this_mac");
    if placement == "hosted" {
        let checkpoint_id = envelope
            .get("checkpoint_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("hosted inference requires checkpoint_id"))?;
        return infer_hosted(checkpoint_id, family, &body, &mut (|_| {})).await;
    }
    let pin = envelope
        .get("policy_snapshot_id")
        .and_then(Value::as_str)
        .or_else(|| body.get("policy_snapshot_id").and_then(Value::as_str))
        .ok_or_else(|| anyhow!("local inference requires policy_snapshot_id"))?;
    let adapter_path = envelope
        .get("adapter_path")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    infer_mlx(pin, adapter_path.as_deref(), family, &body, &mut (|_| {})).await
}

fn normalize_family(family: &str) -> Result<&'static str> {
    match family.trim() {
        "chat_completions" | "chat" => Ok("chat_completions"),
        "responses" => Ok("responses"),
        other => bail!("family must be chat_completions or responses, got {other}"),
    }
}

fn wants_stream(body: &Value) -> bool {
    body.get("stream").and_then(Value::as_bool) == Some(true)
}

async fn infer_local<F>(
    checkpoint: &SavedLoraCheckpoint,
    family: &str,
    body: &Value,
    on_delta: &mut F,
) -> Result<InferOutcome>
where
    F: FnMut(&str) + Send,
{
    if family == "chat_completions" && !checkpoint.inference_chat_completions {
        bail!("this checkpoint does not advertise chat completions");
    }
    if family == "responses" && !checkpoint.inference_responses {
        bail!("this checkpoint does not advertise responses");
    }
    let pin = checkpoint
        .storage
        .sha256
        .clone()
        .unwrap_or_else(|| checkpoint.checkpoint_id.clone());
    infer_mlx(
        &pin,
        Some(Path::new(&checkpoint.storage.key)),
        family,
        body,
        on_delta,
    )
    .await
}

async fn infer_mlx<F>(
    pin: &str,
    adapter_path: Option<&Path>,
    family: &str,
    body: &Value,
    on_delta: &mut F,
) -> Result<InferOutcome>
where
    F: FnMut(&str) + Send,
{
    let client = MlxLoopback::ensure().await?;
    let mut payload = body.clone();
    if let Some(object) = payload.as_object_mut() {
        object.insert("policy_snapshot_id".into(), json!(pin));
    }
    let stream = wants_stream(&payload);
    let load_if_missing = |error: &anyhow::Error| error.to_string().contains("policy_snapshot");
    if stream {
        let (content_type, bytes) =
            match mlx_family_stream(&client, family, &payload, on_delta).await {
                Ok(value) => value,
                Err(error) if load_if_missing(&error) => {
                    let name = adapter_path
                        .and_then(|path| path.file_name())
                        .and_then(|value| value.to_str())
                        .ok_or_else(|| {
                            anyhow!("adapter path is required to load a missing snapshot")
                        })?;
                    client.load_adapter(name).await?;
                    mlx_family_stream(&client, family, &payload, on_delta).await?
                }
                Err(error) => return Err(error),
            };
        if content_type.contains("event-stream") {
            let text = String::from_utf8_lossy(&bytes);
            return Ok(InferOutcome {
                json: assemble_family_json(family, &text)?,
                sse: Some(bytes),
            });
        }
        let json: Value = serde_json::from_slice(&bytes).context("decode MLX JSON")?;
        let text = family_text(family, &json);
        if !text.is_empty() {
            on_delta(&text);
        }
        return Ok(InferOutcome { json, sse: None });
    }
    match client.openai_family(family, &payload).await {
        Ok(value) => {
            let text = family_text(family, &value);
            if !text.is_empty() {
                on_delta(&text);
            }
            Ok(InferOutcome {
                json: value,
                sse: None,
            })
        }
        Err(error) if load_if_missing(&error) => {
            let name = adapter_path
                .and_then(|path| path.file_name())
                .and_then(|value| value.to_str())
                .ok_or_else(|| anyhow!("adapter path is required to load a missing snapshot"))?;
            client.load_adapter(name).await?;
            let json = client.openai_family(family, &payload).await?;
            let text = family_text(family, &json);
            if !text.is_empty() {
                on_delta(&text);
            }
            Ok(InferOutcome { json, sse: None })
        }
        Err(error) => Err(error),
    }
}

async fn mlx_family_stream<F>(
    client: &MlxLoopback,
    family: &str,
    payload: &Value,
    on_delta: &mut F,
) -> Result<(String, Vec<u8>)>
where
    F: FnMut(&str) + Send,
{
    client
        .openai_family_stream(family, payload, |block| {
            if let Some(text) = sse_text_delta(family, block) {
                on_delta(&text);
            }
        })
        .await
}

async fn infer_hosted<F>(
    checkpoint_id: &str,
    family: &str,
    body: &Value,
    on_delta: &mut F,
) -> Result<InferOutcome>
where
    F: FnMut(&str) + Send,
{
    let stream = wants_stream(body);
    let mut sample_body = body.clone();
    if let Some(object) = sample_body.as_object_mut() {
        object.remove("stream");
    }
    let checkpoint = super::cloud::CloudOptimizerClient::from_config()?
        .saved_lora_checkpoint(checkpoint_id)
        .await?;
    let sampler = checkpoint
        .provider_checkpoint_reference
        .as_deref()
        .or(checkpoint.lineage.provider_checkpoint_reference.as_deref())
        .ok_or_else(|| anyhow!("hosted checkpoint has no tinker sampler path"))?;
    if !sampler.starts_with("tinker://") {
        bail!("hosted inference requires a tinker:// sampler path");
    }
    if checkpoint.checkpoint_kind != "inference" {
        bail!("training-kind checkpoints are resume-only");
    }
    let json = SftOptimizerClient::from_env()?
        .infer_checkpoint(
            family,
            sampler,
            checkpoint.run_id.as_deref().unwrap_or(checkpoint_id),
            checkpoint_id,
            &sample_body,
        )
        .await?;
    let text = family_text(family, &json);
    if !text.is_empty() {
        on_delta(&text);
    }
    let sse = stream.then(|| family_sse(family, &json).into_bytes());
    Ok(InferOutcome { json, sse })
}

fn family_text(family: &str, payload: &Value) -> String {
    let pointer = if family == "responses" {
        "/output/0/content/0/text"
    } else {
        "/choices/0/message/content"
    };
    payload
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn sse_text_delta(family: &str, block: &str) -> Option<String> {
    let mut event = None;
    let mut data = None;
    for line in block.lines() {
        if let Some(value) = line.strip_prefix("event: ") {
            event = Some(value.trim());
        } else if let Some(value) = line.strip_prefix("data: ") {
            data = Some(value.trim());
        }
    }
    let raw = data?;
    if raw == "[DONE]" {
        return None;
    }
    let payload: Value = serde_json::from_str(raw).ok()?;
    if family == "responses" {
        let is_delta = event == Some("response.output_text.delta")
            || payload.get("type").and_then(Value::as_str) == Some("response.output_text.delta");
        if !is_delta {
            return None;
        }
        return payload
            .get("delta")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
    }
    payload
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn assemble_family_json(family: &str, sse: &str) -> Result<Value> {
    if family == "responses" {
        for block in sse.split("\n\n") {
            let mut event = None;
            let mut data = None;
            for line in block.lines() {
                if let Some(value) = line.strip_prefix("event: ") {
                    event = Some(value.trim());
                } else if let Some(value) = line.strip_prefix("data: ") {
                    data = Some(value.trim());
                }
            }
            if event == Some("response.completed") {
                if let Some(raw) = data {
                    let payload: Value = serde_json::from_str(raw)?;
                    if let Some(response) = payload.get("response") {
                        return Ok(response.clone());
                    }
                    return Ok(payload);
                }
            }
        }
        bail!("streamed responses completed event was missing");
    }
    let mut id = json!("chatcmpl-stream");
    let mut created = json!(0);
    let mut model = json!("");
    let mut text = String::new();
    for block in sse.split("\n\n") {
        for line in block.lines() {
            let Some(raw) = line.strip_prefix("data: ") else {
                continue;
            };
            if raw.trim() == "[DONE]" {
                continue;
            }
            let chunk: Value = serde_json::from_str(raw).unwrap_or(json!({}));
            if chunk.get("id").is_some() {
                id = chunk["id"].clone();
            }
            if chunk.get("created").is_some() {
                created = chunk["created"].clone();
            }
            if chunk.get("model").is_some() {
                model = chunk["model"].clone();
            }
            if let Some(delta) = chunk
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
            {
                text.push_str(delta);
            }
        }
    }
    Ok(json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": "stop"
        }]
    }))
}

fn family_sse(family: &str, payload: &Value) -> String {
    if family == "responses" {
        let text = payload
            .pointer("/output/0/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or("");
        let created = json!({
            "type": "response.created",
            "response": payload
        });
        let delta = json!({
            "type": "response.output_text.delta",
            "delta": text
        });
        let completed = json!({
            "type": "response.completed",
            "response": payload
        });
        format!(
            "event: response.created\ndata: {}\n\n\
event: response.output_text.delta\ndata: {}\n\n\
event: response.completed\ndata: {}\n\n",
            created, delta, completed
        )
    } else {
        let text = payload
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or("");
        let id = payload
            .get("id")
            .cloned()
            .unwrap_or(json!("chatcmpl-hosted"));
        let created = payload.get("created").cloned().unwrap_or(json!(0));
        let model = payload.get("model").cloned().unwrap_or(json!("hosted"));
        let chunk = |delta: Value, finish: Value| {
            json!({
                "id": id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]
            })
        };
        format!(
            "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            chunk(json!({"role": "assistant"}), Value::Null),
            chunk(json!({"content": text}), Value::Null),
            chunk(json!({}), json!("stop")),
        )
    }
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
                json!({"step":2,"epoch":1,"loss":0.42,"learning_rate":0.00005,"tokens":128.0,"step_seconds":2.0,"tokens_per_second":64.0,"memory_bytes":1048576}),
                "sft.training.metrics",
            ),
            (
                "checkpoint.created",
                json!({"checkpoint_id":"job:step-2","step":2,"path":"/tmp/adapter","sha256":"abc123","bytes":128,"created_at":"2026-08-20T12:00:00Z","kind":"mlx-lora.v1"}),
                "sft.checkpoint.ready",
            ),
            (
                "heldout_eval.completed",
                json!({"phase":"trained","instances":4,"mean_reward":0.75,"rewards":[1.0,1.0,0.0,1.0]}),
                "training.evaluation.completed",
            ),
            (
                "training.clip",
                json!({"eps_low":1.0,"eps_high":4.0,"identity":"cispo_minimax"}),
                "cispo.clip.identity",
            ),
        ];
        for (kind, payload, expected) in golden {
            assert_eq!(
                mapped_event_draft(kind, "sft", &payload).event_type,
                expected
            );
        }
    }

    #[test]
    fn maps_all_training_evaluations_to_shared_comparison_evidence() {
        for (kind, phase) in [
            ("baseline_eval.completed", "baseline"),
            ("checkpoint_eval.completed", "checkpoint"),
            ("final_eval.completed", "final"),
        ] {
            let payload = json!({
                "checkpoint_id": "run:step-2", "sha256": "abc",
                "score": 0.8, "baseline_score": 0.5, "sample_count": 16,
                "container_url": "https://tunnel.invalid"
            });
            let draft = mapped_event_draft(kind, "cispo", &payload);
            assert_eq!(draft.event_type, "training.evaluation.completed");
            assert_eq!(draft.delta["phase"], phase);
            assert_eq!(draft.item.as_ref().unwrap()["algorithm"], "cispo");
            let delta = draft.item.as_ref().unwrap()["delta"].as_f64().unwrap();
            assert!((delta - 0.3).abs() < f64::EPSILON * 2.0);
            assert_eq!(draft.item.as_ref().unwrap()["artifact_digest"], "abc");
        }
        let hosted = mapped_event_draft(
            "sft.checkpoint_evaluation.completed",
            "sft",
            &json!({"delta": {"checkpoint_id": "ckpt-10", "step": 10, "score": 0.7}}),
        );
        assert_eq!(hosted.event_type, "training.evaluation.completed");
        assert_eq!(hosted.item.as_ref().unwrap()["checkpoint_id"], "ckpt-10");
        assert_eq!(hosted.item.as_ref().unwrap()["score"], 0.7);
    }

    #[test]
    fn local_sft_requests_tunneled_before_checkpoint_and_final_evaluations() {
        let config = local_sft_config("run", None, None, None);
        assert_eq!(config["config"]["evaluation"]["transport"], "tunnel");
        assert_eq!(
            config["config"]["evaluation"]["schedule"]["phases"],
            json!(["baseline", "checkpoint", "final"])
        );
        assert_eq!(
            config["config"]["evaluation"]["schedule"]["checkpoint_every"],
            2
        );
        assert_eq!(
            config["config"]["evaluation"]["schedule"]["checkpoint_steps"],
            json!([2, 4])
        );
        assert_eq!(
            config["config"]["evaluation"]["candidate"]["exact_checkpoint_required"],
            true
        );
        assert_eq!(
            config["config"]["evaluation"]["evaluator"]["plan_ref"],
            "local_eval.v1"
        );
        assert_eq!(
            config["config"]["evaluation"]["evaluator"]["sample_count"],
            16
        );
        validate_tunneled_evaluation_plan(&config["config"]).unwrap();
    }

    #[test]
    fn training_evaluation_plan_fails_closed_when_checkpoint_identity_is_missing() {
        let mut config = json!({
            "evaluation": tunneled_evaluation_plan(
                Some("https://tunnel.invalid".into()), "URL_ENV", "TOKEN_ENV", 2, vec![2, 4],
                EvaluationContract::from_task("local")
            )
        });
        config["evaluation"]["candidate"]["exact_checkpoint_required"] = json!(false);
        assert!(validate_tunneled_evaluation_plan(&config)
            .unwrap_err()
            .to_string()
            .contains("exact-artifact"));
    }

    #[test]
    fn checkpoint_created_maps_to_digest_catalog_identity() {
        let payload = json!({
            "checkpoint_id": "job:step-2",
            "step": 2,
            "path": "/tmp/adapter",
            "sha256": "abc123",
            "bytes": 128
        });
        let row =
            super::super::local_lora::LocalLoraUpsert::from_checkpoint_event("run1", &payload)
                .expect("checkpoint event");
        assert_eq!(row.sha256, "sha256:abc123");
        assert_eq!(row.adapter_path.as_os_str(), "/tmp/adapter");
    }

    #[test]
    fn inference_families_are_peers_and_stream_is_native() {
        assert_eq!(normalize_family("chat").unwrap(), "chat_completions");
        assert_eq!(normalize_family("responses").unwrap(), "responses");
        assert!(wants_stream(&json!({"stream": true})));
        assert!(!wants_stream(&json!({"stream": false})));
        let sse = family_sse(
            "chat_completions",
            &json!({
                "id": "chatcmpl-1",
                "created": 1,
                "model": "m",
                "choices": [{"message": {"content": "hi"}}]
            }),
        );
        assert!(sse.contains("chat.completion.chunk"));
        assert!(sse.contains("data: [DONE]"));
        let sse = family_sse(
            "responses",
            &json!({
                "id": "resp_1",
                "output": [{"content": [{"text": "hi"}]}]
            }),
        );
        assert!(sse.contains("event: response.completed"));
        assert!(!sse.contains("chat.completion.chunk"));
        assert_eq!(
            sse_text_delta(
                "chat_completions",
                r#"data: {"choices":[{"delta":{"content":"hi"}}]}"#
            )
            .as_deref(),
            Some("hi")
        );
        assert_eq!(
            sse_text_delta(
                "responses",
                "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"yo\"}"
            )
            .as_deref(),
            Some("yo")
        );
        assert_eq!(sse_text_delta("chat_completions", "data: [DONE]"), None);
    }
}
