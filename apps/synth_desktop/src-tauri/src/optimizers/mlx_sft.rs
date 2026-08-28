//! Local Qwen MLX LoRA SFT recipe. Execution lives in the Optimizers sidecar.

use super::models::{
    OptimizerQuery, OptimizerRecipeRunRequest, OptimizerResourceRef, OptimizerRunRecord,
};
use super::sidecar_training::{
    local_sft_config, optional_jsonl, spawn_watch_worker, training_create_request,
    SidecarTrainingClient, LOCAL_MLX_SFT_RECIPE, LOCAL_SFT_LEARNING_RATE,
    PLACEMENT_TRAINING_SFT_LOCAL,
};
use super::OptimizerService;
use anyhow::{bail, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

pub const QWEN_MLX_SFT_RECIPE: &str = LOCAL_MLX_SFT_RECIPE;
const BASE_MODEL: &str = "Qwen/Qwen3.5-2B";
const MAX_STEPS: u64 = 4;
const CHECKPOINT_EVERY: u64 = 2;
const LORA_RANK: u64 = 8;
const LORA_ALPHA: f64 = 16.0;
const MAX_SEQ_LENGTH: u64 = super::mlx_runtime::LOCAL_TRAINING_MAX_SEQ_LENGTH;

pub fn recipe_catalog() -> Value {
    let (dataset, evaluation, dataset_source) = resolve_local_sft_datasets();
    let apple_silicon = cfg!(all(target_os = "macos", target_arch = "aarch64"));
    let model_ready = super::mlx_runtime::require_training_model().is_ok();
    let mut reasons = Vec::new();
    if !apple_silicon {
        reasons.push("Local MLX SFT requires Apple Silicon.");
    }
    if !model_ready {
        reasons.push("Download the training model in Settings → Models → On-device training.");
    }
    let available = apple_silicon && model_ready;
    json!({
        "id": QWEN_MLX_SFT_RECIPE,
        "title": "This Mac · Qwen 3.5 2B MLX LoRA SFT",
        "algorithmId": "sft",
        "task": Value::Null,
        "placement": PLACEMENT_TRAINING_SFT_LOCAL,
        "availability": if available { "available" } else { "unavailable" },
        "availabilityReason": if available { Value::Null } else { json!(reasons.join(" ")) },
        "preflight": {
            "appleSilicon": apple_silicon,
            "dataset": dataset.is_some() && evaluation.is_some(),
            "datasetSource": dataset_source,
            "trainingModel": model_ready,
        },
        "limits": {
            "backend": "qwen_lora", "baseModel": BASE_MODEL,
            "maxSteps": MAX_STEPS, "checkpointEvery": CHECKPOINT_EVERY,
            "loraRank": LORA_RANK, "loraAlpha": LORA_ALPHA,
            "maxSeqLength": MAX_SEQ_LENGTH, "enableThinking": false,
            "costCeilingUsd": 0.0,
            "costNotice": "Local Apple Silicon MLX compute; no hosted provider charges."
        },
        "credentialInputs": [],
        "prerequisites": ["Optimizers sidecar", "ready container advertising SFT JSONL or SYNTH_MLX_SFT_*_JSONL"]
    })
}

pub async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(OptimizerRunRecord, Option<crate::storage::AppEvent>)> {
    validate_generation_learning_rate(LOCAL_SFT_LEARNING_RATE)?;
    super::mlx_runtime::require_training_model()?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("sft_mlx_qwen_{}", &suffix[..12]);
    // An explicit Workshop container is an identity/provenance binding, not a
    // hint. Never let ambient SYNTH_MLX_SFT_* paths silently substitute a
    // different workload after the user selected a concrete container.
    let (dataset, evaluation, bind) = if has_explicit_container(&request) {
        let bind =
            super::container_training::bind(service, request.container_id.as_deref()).await?;
        let (train, eval) = super::container_training::materialize_sft_jsonl(&bind).await?;
        (train, eval, Some(bind))
    } else {
        let env_datasets = resolve_local_sft_datasets();
        if let (Some(train), Some(eval)) = (env_datasets.0, env_datasets.1) {
            (train, eval, None)
        } else {
            let bind = super::container_training::bind(service, None).await?;
            let (train, eval) = super::container_training::materialize_sft_jsonl(&bind).await?;
            (train, eval, Some(bind))
        }
    };
    if !dataset.is_file() || !evaluation.is_file() {
        bail!("local MLX SFT requires train and eval JSONL from the bound container or SYNTH_MLX_SFT_*_JSONL");
    }
    let config = local_sft_config(
        &run_id,
        Some(dataset.as_path()),
        Some(evaluation.as_path()),
        bind.as_ref(),
    );
    let mut input_refs = Vec::new();
    input_refs.push(dataset_ref(
        &dataset,
        "train",
        "Local Qwen SFT dataset",
        bind.as_ref(),
    )?);
    input_refs.push(dataset_ref(
        &evaluation,
        "heldout_evaluation",
        "Fixed held-out Qwen evaluation dataset",
        bind.as_ref(),
    )?);
    input_refs.push(config_ref(&config));
    let create = training_create_request(
        &run_id,
        "sft",
        "qwen35-2b-mlx-lora-v1",
        "Local Qwen 3.5 2B LoRA SFT on Apple Silicon MLX",
        "local",
        QWEN_MLX_SFT_RECIPE,
        &request,
        json!({
            "recipeId": QWEN_MLX_SFT_RECIPE,
            "backend": "qwen_lora",
            "baseModel": BASE_MODEL,
            "placement": PLACEMENT_TRAINING_SFT_LOCAL,
            "trainingCursor": 0,
            "adapterKind": "mlx-lora.v1",
            "evaluationPlan": { "phases": ["baseline", "checkpoint", "final"], "checkpointEvery": 2, "transport": "tunnel", "metric": "reward" }
        }),
        input_refs,
    );
    super::sidecar_training::create_and_watch(
        service,
        request,
        create,
        PLACEMENT_TRAINING_SFT_LOCAL,
        config,
    )
    .await
}

fn has_explicit_container(request: &OptimizerRecipeRunRequest) -> bool {
    request
        .container_id
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
}

fn validate_generation_learning_rate(learning_rate: f64) -> Result<()> {
    if learning_rate > 0.0001 {
        bail!(
            "local MLX SFT generation gate rejected learning_rate={learning_rate}: values above 1e-4 can collapse the adapter to EOS"
        );
    }
    Ok(())
}

fn dataset_ref(
    path: &PathBuf,
    role: &str,
    title: &str,
    bind: Option<&super::container_training::ContainerTrainingBind>,
) -> Result<OptimizerResourceRef> {
    let bytes = fs::read(path)?;
    Ok(OptimizerResourceRef {
        kind: "dataset".into(),
        id: path.display().to_string(),
        digest: Some(format!("sha256:{:x}", Sha256::digest(&bytes))),
        role: Some(role.into()),
        title: Some(title.into()),
        metadata: bind.map_or_else(
            || json!({"source": "operator_environment"}),
            |bind| {
                json!({
                    "source": "workshop_container",
                    "containerId": bind.container_id,
                    "taskId": bind.task_id,
                    "baseUrl": bind.base_url,
                })
            },
        ),
    })
}

fn config_ref(config: &Value) -> OptimizerResourceRef {
    let canonical = serde_json::to_vec(config).expect("training config is JSON");
    OptimizerResourceRef {
        kind: "training_configuration".into(),
        id: QWEN_MLX_SFT_RECIPE.into(),
        digest: Some(format!("sha256:{:x}", Sha256::digest(canonical))),
        role: Some("resolved_configuration".into()),
        title: Some("Resolved local MLX SFT configuration".into()),
        metadata: json!({"schemaVersion": "synth-mlx-rl.training-config.v1"}),
    }
}

/// Resolve only operator-provided or installed cookbook datasets.
pub fn resolve_local_sft_datasets() -> (Option<PathBuf>, Option<PathBuf>, &'static str) {
    if let (Some(train), Some(eval)) = (
        optional_jsonl("SYNTH_MLX_SFT_TRAIN_JSONL"),
        optional_jsonl("SYNTH_MLX_SFT_EVAL_JSONL"),
    ) {
        return (Some(train), Some(eval), "env");
    }
    (None, None, "missing")
}

pub async fn reconcile(service: &OptimizerService, run_id: &str) -> Result<OptimizerRunRecord> {
    let current = service.get(run_id.into()).await?;
    if super::sidecar_training::reconcile_persisted_sft(service, run_id, &current.summary).await? {
        return service.get(run_id.into()).await;
    }
    let cursor = current
        .summary
        .get("trainingCursor")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if let Ok(client) = SidecarTrainingClient::from_manager(service.manager()).await {
        spawn_watch_worker(service, client, run_id.to_string(), cursor).await;
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
    let Ok(client) = SidecarTrainingClient::from_manager(service.manager()).await else {
        return;
    };
    for run in runs.into_iter().filter(|run| {
        run.summary.get("recipeId").and_then(Value::as_str) == Some(QWEN_MLX_SFT_RECIPE)
            && !matches!(run.status.as_str(), "completed" | "failed" | "cancelled")
            && !registered.contains(&run.id)
    }) {
        let cursor = run
            .summary
            .get("trainingCursor")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        spawn_watch_worker(service, client.clone(), run.id, cursor).await;
    }
}

