//! Product SFT recipe for the local `synth-mlx-rl` Qwen service.
//!
//! This is intentionally not an MLX scalar smoke. Admission requires the
//! service to advertise the exact resident Qwen LoRA/render contract, and the
//! terminal artifact must be a content-addressed `mlx-lora.v1` adapter.

use super::events::OptimizerEventDraft;
use super::models::{
    OptimizerCapabilities, OptimizerCreateRequest, OptimizerExecutionBinding, OptimizerQuery,
    OptimizerRecipeRunRequest, OptimizerResourceRef, OptimizerRunRecord,
};
use super::OptimizerService;
use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Client, Response, Url};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::{sync::watch, time::sleep};

pub const QWEN_MLX_SFT_RECIPE: &str = "sft.qwen35-0.8b.mlx.v1";
const DEFAULT_URL: &str = "http://127.0.0.1:8787";
const BASE_MODEL: &str = "Qwen/Qwen3.5-0.8B";
const MAX_STEPS: u64 = 4;
const CHECKPOINT_EVERY: u64 = 2;
const LORA_RANK: u64 = 8;
const LORA_ALPHA: f64 = 16.0;
const MAX_SEQ_LENGTH: u64 = 4096;
const MAX_PAGE_ERRORS: u32 = 20;

#[derive(Clone)]
struct MlxClient {
    base_url: String,
    http: Client,
}

impl MlxClient {
    fn from_env() -> Result<Self> {
        let raw = std::env::var("SYNTH_MLX_RL_URL").unwrap_or_else(|_| DEFAULT_URL.into());
        let url = Url::parse(raw.trim()).context("parse SYNTH_MLX_RL_URL")?;
        let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
        if url.scheme() != "http" || !local {
            bail!("SYNTH_MLX_RL_URL must be a loopback http URL");
        }
        Ok(Self {
            base_url: url.as_str().trim_end_matches('/').to_string(),
            http: Client::builder().timeout(Duration::from_secs(10)).build()?,
        })
    }

    async fn get(&self, path: &str) -> Result<Value> {
        decode(
            self.http
                .get(format!("{}{path}", self.base_url))
                .send()
                .await?,
            path,
        )
        .await
    }

    async fn post(&self, path: &str, body: Option<&Value>) -> Result<Value> {
        let mut request = self.http.post(format!("{}{path}", self.base_url));
        if let Some(value) = body {
            request = request.json(value);
        }
        decode(request.send().await?, path).await
    }
}

async fn decode(response: Response, operation: &str) -> Result<Value> {
    let status = response.status();
    let text = response.text().await.context("read MLX response")?;
    if !status.is_success() {
        bail!(
            "MLX service {operation} failed with {status}: {}",
            text.trim()
        );
    }
    serde_json::from_str(&text).with_context(|| format!("decode MLX response for {operation}"))
}

fn dataset_path() -> Result<PathBuf> {
    let raw = std::env::var("SYNTH_MLX_SFT_TRAIN_JSONL")
        .context("local Qwen SFT requires SYNTH_MLX_SFT_TRAIN_JSONL")?;
    let path = PathBuf::from(raw.trim());
    if !path.is_file() {
        bail!("local Qwen SFT dataset is not a file: {}", path.display());
    }
    Ok(path)
}

fn service_reachable() -> bool {
    let Ok(client) = MlxClient::from_env() else {
        return false;
    };
    let Ok(url) = Url::parse(&client.base_url) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let Some(port) = url.port_or_known_default() else {
        return false;
    };
    let Ok(addresses) = std::net::ToSocketAddrs::to_socket_addrs(&(host, port)) else {
        return false;
    };
    addresses.into_iter().any(|address| {
        std::net::TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok()
    })
}

fn sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    Ok(format!("{:x}", Sha256::digest(std::fs::read(path)?)))
}

fn contract_error(capabilities: &Value) -> Option<String> {
    if capabilities
        .pointer("/capabilities/qwen_lora_training/supported")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Some(
            capabilities
                .pointer("/capabilities/qwen_lora_training/reason")
                .and_then(Value::as_str)
                .unwrap_or("real Qwen LoRA is unavailable")
                .into(),
        );
    }
    let Some(contract) = capabilities.get("qwen_lora_contract") else {
        return Some("MLX service omitted the resident Qwen LoRA contract".into());
    };
    let exact = contract.get("backend").and_then(Value::as_str) == Some("qwen_lora")
        && contract.get("base_model").and_then(Value::as_str) == Some(BASE_MODEL)
        && contract.get("lora_rank").and_then(Value::as_u64) == Some(LORA_RANK)
        && contract.get("lora_alpha").and_then(Value::as_f64) == Some(LORA_ALPHA)
        && contract.get("max_seq_length").and_then(Value::as_u64) == Some(MAX_SEQ_LENGTH)
        && contract.get("enable_thinking").and_then(Value::as_bool) == Some(false)
        && contract.get("adapter_kind").and_then(Value::as_str) == Some("mlx-lora.v1")
        && contract.get("renderer").and_then(Value::as_str) == Some("qwen-chat-template.v1");
    (!exact).then(|| "resident MLX service does not match the v0.6 Qwen LoRA contract".into())
}

pub fn recipe_catalog() -> Value {
    let dataset = dataset_path();
    let available = cfg!(all(target_os = "macos", target_arch = "aarch64"))
        && dataset.is_ok()
        && service_reachable();
    json!({
        "id": QWEN_MLX_SFT_RECIPE,
        "title": "Local Qwen 3.5 0.8B MLX LoRA SFT",
        "algorithmId": "sft",
        "task": "local-qwen",
        "availability": if available { "available" } else { "unavailable" },
        "availabilityReason": if available { Value::Null } else { json!("Requires Apple Silicon, a local synth-mlx-rl service, and SYNTH_MLX_SFT_TRAIN_JSONL.") },
        "limits": {
            "backend": "qwen_lora", "baseModel": BASE_MODEL,
            "maxSteps": MAX_STEPS, "checkpointEvery": CHECKPOINT_EVERY,
            "loraRank": LORA_RANK, "loraAlpha": LORA_ALPHA,
            "maxSeqLength": MAX_SEQ_LENGTH, "enableThinking": false,
            "costCeilingUsd": 0.0,
            "costNotice": "Local Apple Silicon MLX compute; no hosted provider charges."
        },
        "credentialInputs": [],
        "prerequisites": ["synth-mlx-rl on 127.0.0.1:8787", "SYNTH_MLX_SFT_TRAIN_JSONL"]
    })
}

fn request_payload(run_id: &str, dataset: &Path, output: &Path) -> Value {
    json!({
        "job_id": run_id,
        "config": {
            "backend": "qwen_lora", "base_model": BASE_MODEL,
            "dataset": {"path": dataset, "sha256": sha256(dataset).ok()},
            "output_dir": output, "max_steps": MAX_STEPS,
            "checkpoint_every": CHECKPOINT_EVERY, "learning_rate": 0.00005,
            "lora_rank": LORA_RANK, "lora_alpha": LORA_ALPHA,
            "max_seq_length": MAX_SEQ_LENGTH, "enable_thinking": false,
            "seed": 0, "max_disk_bytes": 8589934592_u64
        }
    })
}

pub async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(OptimizerRunRecord, Option<crate::storage::AppEvent>)> {
    let client = MlxClient::from_env()?;
    let dataset = dataset_path()?;
    let capabilities = client.get("/v1/capabilities").await?;
    if let Some(reason) = contract_error(&capabilities) {
        bail!("{reason}");
    }
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("sft_mlx_qwen_{}", &suffix[..12]);
    let output = crate::instance::data_root()
        .join("optimizers/mlx-sft")
        .join(&run_id);
    let payload = request_payload(&run_id, &dataset, &output);
    let preflight = client.post("/v1/jobs/preflight", Some(&payload)).await?;
    if preflight.get("accepted").and_then(Value::as_bool) != Some(true) {
        bail!(
            "MLX Qwen preflight rejected the fixed recipe: {}",
            preflight["checks"]
        );
    }
    let digest = sha256(&dataset)?;
    let create = OptimizerCreateRequest {
        algorithm_id: "sft".into(),
        algorithm_version: Some("qwen35-0.8b-mlx-lora-v1".into()),
        objective: Some("Local Qwen 3.5 0.8B LoRA SFT on Apple Silicon MLX".into()),
        source: Some("local".into()),
        project_ref: Some("qwen35-0.8b@mlx-lora".into()),
        session_ref: request.session_ref,
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![OptimizerExecutionBinding {
            kind: "synth_mlx_rl".into(),
            id: client.base_url.clone(),
            label: Some("local Qwen MLX training service".into()),
            status: Some("preflighted".into()),
            metadata: json!({"serviceVersion": capabilities["service_version"], "contract": capabilities["qwen_lora_contract"]}),
        }]),
        input_refs: Some(vec![OptimizerResourceRef {
            kind: "dataset".into(),
            id: dataset.display().to_string(),
            digest: Some(format!("sha256:{digest}")),
            role: Some("train".into()),
            title: Some("Local Qwen SFT dataset".into()),
            metadata: json!({}),
        }]),
        capabilities: Some(OptimizerCapabilities {
            cancel: true,
            stream_events: true,
            checkpoints: true,
            inference_endpoint: true,
            ..OptimizerCapabilities::default()
        }),
        summary: Some(
            json!({"recipeId": QWEN_MLX_SFT_RECIPE, "backend": "qwen_lora", "baseModel": BASE_MODEL,
            "datasetDigest": format!("sha256:{digest}"), "mlxJobId": run_id, "mlxCursor": 0,
            "outputDir": output, "preflight": preflight, "adapterKind": "mlx-lora.v1"}),
        ),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    let (run, event) = service.create(create).await?;
    spawn_worker(service, client, run_id, Some(payload), 0).await;
    Ok((run, event))
}

async fn spawn_worker(
    service: &OptimizerService,
    client: MlxClient,
    run_id: String,
    payload: Option<Value>,
    cursor: u64,
) {
    let (cancel_tx, cancel_rx) = watch::channel(false);
    service
        .register_local_recipe(run_id.clone(), cancel_tx)
        .await;
    let worker = service.clone();
    tokio::spawn(async move {
        if let Err(error) = run_worker(
            worker.clone(),
            client,
            run_id.clone(),
            payload,
            cursor,
            cancel_rx,
        )
        .await
        {
            let _ = append_failure(&worker, &run_id, &format!("{error:#}")).await;
        }
        worker.unregister_local_recipe(&run_id).await;
    });
}

async fn run_worker(
    service: OptimizerService,
    client: MlxClient,
    run_id: String,
    payload: Option<Value>,
    mut cursor: u64,
    mut cancel: watch::Receiver<bool>,
) -> Result<()> {
    if let Some(body) = payload.as_ref() {
        client.post("/v1/jobs", Some(body)).await?;
        client
            .post(&format!("/v1/jobs/{run_id}/launch"), None)
            .await?;
    }
    let mut errors = 0;
    loop {
        if *cancel.borrow() {
            let _ = client
                .post(&format!("/v1/jobs/{run_id}/cancel"), None)
                .await;
        }
        let page = match client
            .get(&format!("/v1/jobs/{run_id}/events?after={cursor}"))
            .await
        {
            Ok(page) => {
                errors = 0;
                page
            }
            Err(_error) if errors < MAX_PAGE_ERRORS => {
                errors += 1;
                sleep(Duration::from_millis(500)).await;
                continue;
            }
            Err(error) => return Err(error.context("MLX event polling stayed unavailable")),
        };
        for event in page
            .get("events")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let sequence = event
                .get("sequence")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("MLX event omitted sequence"))?;
            if sequence != cursor + 1 {
                bail!("MLX event sequence gap after {cursor}: {sequence}");
            }
            append_mlx_event(&service, &run_id, &event, sequence).await?;
            cursor = sequence;
        }
        persist_cursor(&service, &run_id, cursor).await?;
        let job = client.get(&format!("/v1/jobs/{run_id}")).await?;
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
            "failed" | "interrupted" => {
                return Err(anyhow!(
                    "MLX job failed: {}",
                    job.get("error_detail")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                ))
            }
            "cancelled" => {
                append_status(&service, &run_id, "optimizer.run.cancelled", "cancelled").await?;
                return Ok(());
            }
            _ => {}
        }
        tokio::select! { _ = cancel.changed() => {}, _ = sleep(Duration::from_millis(300)) => {} }
    }
}

async fn append_mlx_event(
    service: &OptimizerService,
    run_id: &str,
    event: &Value,
    sequence: u64,
) -> Result<()> {
    let kind = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("job.event");
    let payload = event.get("payload").cloned().unwrap_or_else(|| json!({}));
    let draft = match kind {
        "job.started" => OptimizerEventDraft::new("optimizer.run.started", "sft").delta(Map::from_iter([("status".into(), json!("running"))])),
        "training.metric" => OptimizerEventDraft::new("sft.training.metrics", "sft").delta(Map::from_iter([
            ("step".into(), payload["step"].clone()), ("train_loss".into(), payload["loss"].clone()),
            ("learning_rate".into(), payload["learning_rate"].clone()), ("throughput".into(), payload["throughput_steps_per_second"].clone())])),
        "checkpoint.created" => OptimizerEventDraft::new("sft.checkpoint.ready", "sft").item(json!({
            "id": payload["checkpoint_id"], "step": payload["step"], "status": "ready", "ready": true,
            "path": payload["path"], "sha256": payload["sha256"], "bytes": payload["bytes"], "kind": "mlx-lora.v1", "raw": payload
        })).artifact_refs(vec![json!({"kind":"checkpoint", "id": payload["checkpoint_id"], "uri": payload["path"], "digest": payload["sha256"]})]),
        _ => OptimizerEventDraft::new(format!("mlx.{kind}"), "sft"),
    };
    service
        .append_event_payloads(
            run_id.into(),
            vec![draft
                .idempotency_key(format!("mlx:{sequence}"))
                .raw(event.clone())],
        )
        .await?;
    Ok(())
}

async fn persist_handoff(
    service: &OptimizerService,
    client: &MlxClient,
    run_id: &str,
) -> Result<()> {
    let handoff = client.get(&format!("/v1/jobs/{run_id}/handoff")).await?;
    if handoff.pointer("/inference/kind").and_then(Value::as_str) != Some("mlx-lora.v1") {
        bail!("MLX job did not produce an mlx-lora.v1 handoff");
    }
    let mut run = service.get(run_id.into()).await?;
    run.output_refs.push(OptimizerResourceRef {
        kind: "checkpoint".into(),
        id: handoff["checkpoint"]["checkpoint_id"]
            .as_str()
            .unwrap_or("terminal")
            .into(),
        digest: handoff["checkpoint"]["sha256"]
            .as_str()
            .map(|v| format!("sha256:{v}")),
        role: Some("terminal_adapter".into()),
        title: Some("Qwen MLX LoRA adapter".into()),
        metadata: handoff.clone(),
    });
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert("adapterHandoff".into(), handoff);
    run.summary = Value::Object(summary);
    service.persist_run(run).await?;
    Ok(())
}

async fn persist_cursor(service: &OptimizerService, run_id: &str, cursor: u64) -> Result<()> {
    let mut run = service.get(run_id.into()).await?;
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert("mlxCursor".into(), json!(cursor));
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
    service
        .append_event_payloads(
            run_id.into(),
            vec![OptimizerEventDraft::new(kind, "sft")
                .idempotency_key(format!("mlx:{kind}"))
                .delta(Map::from_iter([("status".into(), json!(status))]))
                .raw(json!({"source":"synth-mlx-rl"}))],
        )
        .await?;
    Ok(())
}

async fn append_failure(service: &OptimizerService, run_id: &str, reason: &str) -> Result<()> {
    service
        .append_event_payloads(
            run_id.into(),
            vec![OptimizerEventDraft::new("optimizer.run.failed", "sft")
                .idempotency_key("mlx:optimizer.run.failed")
                .level("error")
                .delta(Map::from_iter([("status".into(), json!("failed"))]))
                .error(json!({"message":reason}))
                .raw(json!({"source":"synth-mlx-rl"}))],
        )
        .await?;
    Ok(())
}

pub async fn reconcile(service: &OptimizerService, run_id: &str) -> Result<OptimizerRunRecord> {
    let client = MlxClient::from_env()?;
    let job = client.get(&format!("/v1/jobs/{run_id}")).await?;
    let cursor = service
        .get(run_id.into())
        .await?
        .summary
        .get("mlxCursor")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let page = client
        .get(&format!("/v1/jobs/{run_id}/events?after={cursor}"))
        .await?;
    let mut next = cursor;
    for event in page
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let sequence = event
            .get("sequence")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("MLX event omitted sequence"))?;
        if sequence != next + 1 {
            bail!("MLX event sequence gap after {next}: {sequence}");
        }
        append_mlx_event(service, run_id, &event, sequence).await?;
        next = sequence;
    }
    persist_cursor(service, run_id, next).await?;
    if job.get("status").and_then(Value::as_str) == Some("succeeded") {
        persist_handoff(service, &client, run_id).await?;
        append_status(service, run_id, "optimizer.run.completed", "completed").await?;
    }
    service.get(run_id.into()).await
}

pub async fn restore_mirrors(service: &OptimizerService) {
    let Ok(runs) = service
        .list(OptimizerQuery {
            algorithm_id: Some("sft".into()),
            source: Some("local".into()),
            ..OptimizerQuery::default()
        })
        .await
    else {
        return;
    };
    let registered: HashSet<String> = service.registered_local_recipes().await;
    let Ok(client) = MlxClient::from_env() else {
        return;
    };
    for run in runs.into_iter().filter(|run| {
        run.summary.get("recipeId").and_then(Value::as_str) == Some(QWEN_MLX_SFT_RECIPE)
            && !matches!(run.status.as_str(), "completed" | "failed" | "cancelled")
            && !registered.contains(&run.id)
    }) {
        let cursor = run
            .summary
            .get("mlxCursor")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        spawn_worker(service, client.clone(), run.id, None, cursor).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_contract_is_required() {
        let good = json!({"capabilities":{"qwen_lora_training":{"supported":true}}, "qwen_lora_contract":{
            "backend":"qwen_lora","base_model":BASE_MODEL,"lora_rank":8,"lora_alpha":16.0,
            "max_seq_length":4096,"enable_thinking":false,"adapter_kind":"mlx-lora.v1","renderer":"qwen-chat-template.v1"}});
        assert_eq!(contract_error(&good), None);
        let mut bad = good;
        bad["qwen_lora_contract"]["enable_thinking"] = json!(true);
        assert!(contract_error(&bad).unwrap().contains("does not match"));
    }
}
