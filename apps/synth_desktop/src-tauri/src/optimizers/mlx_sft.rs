//! Local Qwen MLX LoRA SFT recipe. Execution lives in the Optimizers sidecar.

use super::models::{
    OptimizerQuery, OptimizerRecipeRunRequest, OptimizerResourceRef, OptimizerRunRecord,
};
use super::sidecar_training::{
    advertised_placement, local_sft_config, optional_jsonl, spawn_watch_worker,
    training_create_request, SidecarTrainingClient, LOCAL_MLX_SFT_RECIPE,
    PLACEMENT_TRAINING_SFT_LOCAL,
};
use super::OptimizerService;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const QWEN_MLX_SFT_RECIPE: &str = LOCAL_MLX_SFT_RECIPE;
const BASE_MODEL: &str = "Qwen/Qwen3.5-0.8B";
const MAX_STEPS: u64 = 4;
const CHECKPOINT_EVERY: u64 = 2;
const LORA_RANK: u64 = 8;
const LORA_ALPHA: f64 = 16.0;
const MAX_SEQ_LENGTH: u64 = 4096;
const CANARY_TRAIN: &str = include_str!("fixtures/qwen_mlx_sft_train.jsonl");
const CANARY_EVAL: &str = include_str!("fixtures/qwen_mlx_sft_eval.jsonl");

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
    if dataset.is_none() || evaluation.is_none() {
        reasons.push(
            "Train/eval JSONL is missing (cookbook, SYNTH_MLX_SFT_*_JSONL, or bundled canary).",
        );
    }
    let available = apple_silicon && model_ready && dataset.is_some() && evaluation.is_some();
    json!({
        "id": QWEN_MLX_SFT_RECIPE,
        "title": "This Mac · Qwen 3.5 0.8B MLX LoRA SFT",
        "algorithmId": "sft",
        "task": "local-qwen",
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
        "prerequisites": ["Optimizers sidecar", "cookbook, SYNTH_MLX_SFT_*_JSONL, or bundled 4-step canary"]
    })
}

pub async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(OptimizerRunRecord, Option<crate::storage::AppEvent>)> {
    super::mlx_runtime::require_training_model()?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("sft_mlx_qwen_{}", &suffix[..12]);
    let (dataset, evaluation, _) = resolve_local_sft_datasets();
    let mut input_refs = Vec::new();
    if let Some(path) = dataset.as_ref() {
        input_refs.push(dataset_ref(path, "train", "Local Qwen SFT dataset"));
    }
    if let Some(path) = evaluation.as_ref() {
        input_refs.push(dataset_ref(
            path,
            "heldout_evaluation",
            "Fixed held-out Qwen evaluation dataset",
        ));
    }
    let create = training_create_request(
        &run_id,
        "sft",
        "qwen35-0.8b-mlx-lora-v1",
        "Local Qwen 3.5 0.8B LoRA SFT on Apple Silicon MLX",
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
        local_sft_config(&run_id, dataset.as_deref(), evaluation.as_deref()),
    )
    .await
}

fn dataset_ref(path: &PathBuf, role: &str, title: &str) -> OptimizerResourceRef {
    OptimizerResourceRef {
        kind: "dataset".into(),
        id: path.display().to_string(),
        digest: None,
        role: Some(role.into()),
        title: Some(title.into()),
        metadata: json!({}),
    }
}

/// Env override, then cookbook JSONL, then the bundled 4-step canary.
pub fn resolve_local_sft_datasets() -> (Option<PathBuf>, Option<PathBuf>, &'static str) {
    if let (Some(train), Some(eval)) = (
        optional_jsonl("SYNTH_MLX_SFT_TRAIN_JSONL"),
        optional_jsonl("SYNTH_MLX_SFT_EVAL_JSONL"),
    ) {
        return (Some(train), Some(eval), "env");
    }
    if let Some(dir) = cookbook_sft_dir() {
        let train = dir.join("train.jsonl");
        let eval = dir.join("eval.jsonl");
        if train.is_file() && eval.is_file() {
            return (Some(train), Some(eval), "cookbook");
        }
    }
    match materialize_canary() {
        Ok((train, eval)) => (Some(train), Some(eval), "canary"),
        Err(_) => (None, None, "missing"),
    }
}

fn cookbook_sft_dir() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("SYNTH_MLX_SFT_COOKBOOK") {
        let path = PathBuf::from(raw.trim());
        if path.is_dir() {
            return Some(path);
        }
    }
    let rel = Path::new("cookbooks/optimizers/sft/qwen35_mlx");
    let mut candidates = Vec::new();
    candidates.push(crate::instance::data_root().join(rel));
    candidates.push(crate::instance::state_root().join(rel));
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        candidates.push(
            PathBuf::from(manifest)
                .join("generated-resources")
                .join(rel),
        );
    }
    candidates
        .into_iter()
        .find(|path| path.join("train.jsonl").is_file() && path.join("eval.jsonl").is_file())
}

fn materialize_canary() -> Result<(PathBuf, PathBuf)> {
    let dir = crate::instance::data_root().join("optimizers/mlx-sft/canary");
    std::fs::create_dir_all(&dir).context("create bundled MLX SFT canary directory")?;
    let train = dir.join("train.jsonl");
    let eval = dir.join("eval.jsonl");
    if !train.is_file() {
        std::fs::write(&train, CANARY_TRAIN).context("write bundled MLX SFT train canary")?;
    }
    if !eval.is_file() {
        std::fs::write(&eval, CANARY_EVAL).context("write bundled MLX SFT eval canary")?;
    }
    Ok((train, eval))
}

pub async fn reconcile(service: &OptimizerService, run_id: &str) -> Result<OptimizerRunRecord> {
    let cursor = service
        .get(run_id.into())
        .await?
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
    fn bundled_canary_is_used_when_env_and_cookbook_are_absent() {
        std::env::remove_var("SYNTH_MLX_SFT_TRAIN_JSONL");
        std::env::remove_var("SYNTH_MLX_SFT_EVAL_JSONL");
        std::env::remove_var("SYNTH_MLX_SFT_COOKBOOK");
        let (train, eval, source) = resolve_local_sft_datasets();
        assert_eq!(source, "canary");
        assert!(train.unwrap().is_file());
        assert!(eval.unwrap().is_file());
    }

    #[test]
    fn recipe_card_reports_preflight_and_is_not_env_only() {
        let recipe = recipe_catalog();
        assert_eq!(recipe["id"], QWEN_MLX_SFT_RECIPE);
        assert!(recipe["preflight"]["dataset"].as_bool().unwrap());
        assert_ne!(recipe["preflight"]["datasetSource"], "missing");
        assert!(recipe["prerequisites"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str().unwrap().contains("cookbook")));
    }

    #[tokio::test]
    #[ignore = "needs synth-mlx-rl and managed Qwen training weights"]
    async fn local_sft_dispatch_needs_synth_mlx_rl() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path().join("core")).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        let content = ContentStore::new(storage.content_root());
        let visuals = VisualRegistry::new(storage.database().clone(), journal.clone(), content);
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
            visuals,
            events_tx,
            manager,
        );
        let (run, _) = start(
            &service,
            OptimizerRecipeRunRequest {
                recipe_id: QWEN_MLX_SFT_RECIPE.into(),
                session_ref: Some("sess_training_e2e".into()),
                open_visual: Some(false),
                base_model: None,
                dataset_shard: None,
                candidate_set_id: None,
                search: None,
            },
        )
        .await
        .unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
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
                "local SFT sidecar fixture timed out at {}",
                current.status
            );
            sleep(Duration::from_millis(50)).await;
        };
        assert_eq!(terminal.status, "completed");
        let events = service
            .events_after(run.id.clone(), 0, Some(500))
            .await
            .unwrap();
        assert!(events
            .iter()
            .any(|event| event.event_type == "sft.heldout_evaluation.completed"));
        assert!(events
            .iter()
            .any(|event| event.event_type == "sft.checkpoint.ready"));
        assert!(events
            .iter()
            .any(|event| event.event_type == "sft.training.metrics"));
        let client = SidecarTrainingClient::from_manager(service.manager())
            .await
            .unwrap();
        let chat = client.chat(&run.id, "hello from checkpoint").await.unwrap();
        assert!(chat["reply"]
            .as_str()
            .unwrap()
            .contains("hello from checkpoint"));
        let _ = client.resume(&run.id).await.unwrap();
        let _ = service.manager().stop().await;
    }
}
