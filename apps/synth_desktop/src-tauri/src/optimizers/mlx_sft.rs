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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::sidecar_training::advertised_placement;
    use crate::storage::{ContentStore, EventJournal, Storage};
    use crate::visuals::VisualRegistry;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::time::sleep;

    #[test]
    fn production_source_does_not_dial_mlx_loopback() {
        let production = include_str!("mlx_sft.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(!production.contains(&["127.0.0.1:", "8787"].concat()));
        assert!(!production.contains("SYNTH_MLX_RL_URL"));
        assert!(production.contains("PLACEMENT_TRAINING_SFT_LOCAL"));
        assert!(production.contains("create_and_watch"));
        assert!(production.contains("resolve_local_sft_datasets"));
    }

    #[test]
    fn missing_real_dataset_fails_closed() {
        std::env::remove_var("SYNTH_MLX_SFT_TRAIN_JSONL");
        std::env::remove_var("SYNTH_MLX_SFT_EVAL_JSONL");
        std::env::remove_var("SYNTH_MLX_SFT_COOKBOOK");
        let (train, eval, source) = resolve_local_sft_datasets();
        assert_eq!(source, "missing");
        assert!(train.is_none());
        assert!(eval.is_none());
    }

    #[test]
    fn recipe_card_is_listed_without_a_prebound_dataset() {
        let recipe = recipe_catalog();
        assert_eq!(recipe["id"], QWEN_MLX_SFT_RECIPE);
        assert!(recipe["prerequisites"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str().unwrap().contains("container")));
    }

    #[test]
    fn generation_gate_accepts_safe_1e_5_and_rejects_unsafe_1e_3() {
        assert!(validate_generation_learning_rate(1e-5).is_ok());
        let error = validate_generation_learning_rate(1e-3)
            .expect_err("unsafe learning rate must fail closed")
            .to_string();
        assert!(error.contains("generation gate rejected"));
        assert!(error.contains("collapse the adapter to EOS"));
    }

    #[test]
    fn explicit_container_is_a_strict_dataset_binding() {
        let request = recipe_request(Some("ctr_alfworld"));
        assert!(has_explicit_container(&request));

        let ambient_only = recipe_request(Some("   "));
        assert!(!has_explicit_container(&ambient_only));
    }

    #[test]
    fn dataset_and_config_refs_are_digest_bound() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("train.jsonl");
        fs::write(&path, b"{\"messages\":[]}\n").unwrap();
        let path = PathBuf::from(path);
        let reference = dataset_ref(&path, "train", "dataset", None).unwrap();
        assert_eq!(
            reference.digest.as_deref(),
            Some("sha256:967f89089aeadc7e90a8ecac9d3c9aca28ee83f59003525afa418983f5afd4b3")
        );
        assert_eq!(reference.metadata["source"], "operator_environment");

        let config = json!({"b": 2, "a": 1});
        let reference = config_ref(&config);
        assert_eq!(
            reference.digest.as_deref(),
            Some("sha256:43258cff783fe7036d8a43033f830adfc60ec037382473548ac742b888292777")
        );
    }

    fn recipe_request(container_id: Option<&str>) -> OptimizerRecipeRunRequest {
        OptimizerRecipeRunRequest {
            training_artifact_id: None,
            recipe_id: QWEN_MLX_SFT_RECIPE.into(),
            session_ref: None,
            open_visual: Some(false),
            base_model: None,
            dataset_shard: None,
            candidate_set_id: None,
            container_id: container_id.map(str::to_string),
            plan_override: None,
            search: None,
        }
    }

    #[tokio::test]
    #[ignore = "needs synth-mlx-rl and managed Qwen training weights"]
    async fn local_sft_dispatch_needs_synth_mlx_rl() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path().join("core")).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        let content = ContentStore::new(storage.content_root());
        let visuals =
            VisualRegistry::new(storage.database().clone(), journal.clone(), content.clone());
        let (events_tx, _) = tokio::sync::broadcast::channel(16);
        let manager = Arc::new(crate::optimizers::OptimizerManager::with_home(
            dir.path().join("optimizer-home"),
        ));
        manager.install(None).unwrap();
        let started = manager.start().await.unwrap();
        assert_eq!(started.phase, "ready");
        let caps = manager.advertised_capabilities();
        assert!(advertised_placement(&caps, PLACEMENT_TRAINING_SFT_LOCAL));
        let service = OptimizerService::new_with_manager(
            storage.database().clone(),
            journal,
            content,
            visuals,
            events_tx,
            manager,
        );
        let (run, _) = start(
            &service,
            OptimizerRecipeRunRequest {
                training_artifact_id: None,
                recipe_id: QWEN_MLX_SFT_RECIPE.into(),
                session_ref: Some("sess_training_e2e".into()),
                open_visual: Some(false),
                base_model: None,
                dataset_shard: None,
                candidate_set_id: None,
                container_id: None,
                plan_override: None,
                search: None,
            },
        )
        .await
        .unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1800);
        let terminal = loop {
            let current = service.get(run.id.clone()).await.unwrap();
            if matches!(
                current.status.as_str(),
                "completed" | "failed" | "cancelled"
            ) {
                break current;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "local SFT timed out at {}",
                current.status
            );
            sleep(Duration::from_secs(2)).await;
        };
        assert_eq!(terminal.status, "completed", "{:?}", terminal.summary);
        let events = service
            .events_after(run.id.clone(), 0, Some(500))
            .await
            .unwrap();
        assert!(events
            .iter()
            .any(|event| event.event_type == "training.evaluation.completed"));
        assert!(events
            .iter()
            .any(|event| event.event_type == "sft.checkpoint.ready"));
        assert!(events
            .iter()
            .any(|event| event.event_type == "sft.training.metrics"));
        let client = SidecarTrainingClient::from_manager(service.manager())
            .await
            .unwrap();
        let chat = client
            .chat(&run.id, "Classify: I want to check my balance.")
            .await
            .unwrap();
        let reply = chat["reply"].as_str().unwrap_or("");
        assert!(
            !reply.trim().is_empty(),
            "checkpoint chat returned empty reply: {chat}"
        );
        let _ = service.manager().stop().await;
    }
}
