//! Bounded CISPO recipes admitted by the Optimizers sidecar.

use super::models::{
    OptimizerQuery, OptimizerRecipeRunRequest, OptimizerResourceRef, OptimizerRunRecord,
};
use super::sidecar_training::{
    advertised_placement, spawn_watch_worker, training_create_request,
    tunneled_banking77_evaluation_plan, SidecarTrainingClient, HOSTED_CISPO_RECIPE,
    LOCAL_MLX_CISPO_RECIPE, PLACEMENT_TRAINING_CISPO_HOSTED, PLACEMENT_TRAINING_CISPO_LOCAL,
};
use super::OptimizerService;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;

struct CispoRollout {
    url: String,
    task_id: String,
    token: Option<String>,
}

fn pinned_cispo_rollout() -> Option<CispoRollout> {
    let rel = Path::new("cookbooks/optimizers/cispo/rollout.json");
    for path in [
        crate::instance::data_root().join(rel),
        crate::instance::state_root().join(rel),
    ] {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(url) = value.get("url").and_then(Value::as_str).filter(|url| !url.is_empty()) else {
            continue;
        };
        return Some(CispoRollout {
            url: url.into(),
            task_id: value
                .get("task_id")
                .and_then(Value::as_str)
                .unwrap_or("banking77")
                .into(),
            token: value
                .get("token")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    None
}

pub fn recipe_catalog() -> Vec<Value> {
    vec![local_mlx_recipe(), hosted_slime_recipe()]
}

fn local_mlx_recipe() -> Value {
    let apple_silicon = cfg!(all(target_os = "macos", target_arch = "aarch64"));
    let model_ready = super::mlx_runtime::require_training_model().is_ok();
    let rollout = pinned_cispo_rollout();
    let available = apple_silicon && model_ready && rollout.is_some();
    json!({
        "id": LOCAL_MLX_CISPO_RECIPE,
        "title": "This Mac · Banking77 CISPO (MLX)",
        "algorithmId": "cispo",
        "task": "banking77",
        "placement": PLACEMENT_TRAINING_CISPO_LOCAL,
        "availability": if available { "available" } else { "unavailable" },
        "availabilityReason": if available { Value::Null } else { json!("Local CISPO requires Apple Silicon, the managed on-device training model, and a pinned rollout target under cookbooks/optimizers/cispo/rollout.json.") },
        "limits": {
            "backend": "cispo",
            "maxSteps": 4,
            "costCeilingUsd": 0.0,
            "costNotice": "Local Apple Silicon MLX compute. Warm-start from a selected SFT artifact id; otherwise the visual reports cispo_no_learning_signal.",
            "resolvedConfig": {
                "baseModel": "Qwen/Qwen3.5-0.8B",
                "task": "banking77",
                "loraDropout": 0.0,
                "warmStart": "training_artifact_id"
            }
        },
        "credentialInputs": [],
        "prerequisites": ["Optimizers sidecar", "optional SFT warm-start adapter"]
    })
}

fn hosted_slime_recipe() -> Value {
    let admitted = advertised_placement(
        &json!({"placements": super::sidecar_training::admitted_placements()}),
        PLACEMENT_TRAINING_CISPO_HOSTED,
    );
    json!({
        "id": HOSTED_CISPO_RECIPE,
        "title": "Hosted CISPO · slime.v1 Banking77",
        "algorithmId": "cispo",
        "task": "banking77",
        "placement": PLACEMENT_TRAINING_CISPO_HOSTED,
        "availability": if admitted { "available" } else { "unavailable" },
        "availabilityReason": if admitted { Value::Null } else { json!("Hosted CISPO stays fail-closed until the slime clip canary (1+eps_high) admits it.") },
        "limits": {
            "backend": "cispo.slime.v1",
            "maxSteps": 2,
            "costCeilingUsd": 10.0,
            "costNotice": "Hosted Tinker + Banking77 container. Sidecar owns the tunnel lease."
        },
        "credentialInputs": [],
        "prerequisites": ["Optimizers sidecar", "SYNTH_OPTIMIZERS_CISPO_HOSTED_ADMITTED after canary"]
    })
}

pub async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(OptimizerRunRecord, Option<crate::storage::AppEvent>)> {
    match request.recipe_id.as_str() {
        LOCAL_MLX_CISPO_RECIPE => start_local(service, request).await,
        HOSTED_CISPO_RECIPE => start_hosted(service, request).await,
        other => anyhow::bail!("unknown CISPO recipe: {other}"),
    }
}

async fn start_local(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(OptimizerRunRecord, Option<crate::storage::AppEvent>)> {
    super::mlx_runtime::require_training_model()?;
    let rollout = pinned_cispo_rollout()
        .ok_or_else(|| anyhow::anyhow!("local CISPO requires a pinned rollout target at cookbooks/optimizers/cispo/rollout.json"))?;
    let evaluation_plan = tunneled_banking77_evaluation_plan(
        Some(rollout.url.clone()),
        "SYNTH_OPTIMIZERS_CISPO_ROLLOUT_URL",
        "SYNTH_OPTIMIZERS_CISPO_ROLLOUT_TOKEN",
        2,
        vec![2, 4],
    );
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("cispo_mlx_{}", &suffix[..12]);
    let output_dir = crate::instance::data_root()
        .join("optimizers/mlx-cispo")
        .join(&run_id);
    let output_parent = output_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("local CISPO output directory has no parent"))?;
    std::fs::create_dir_all(output_parent)?;
    let mut input_refs = Vec::new();
    let warm_start = if let Some(artifact_id) = request.training_artifact_id.as_deref() {
        let artifact = crate::training_artifacts::get(artifact_id)?;
        let path = artifact
            .path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SFT artifact `{artifact_id}` has no adapter path"))?;
        input_refs.push(OptimizerResourceRef {
            kind: "checkpoint".into(),
            id: artifact.id.clone(),
            digest: artifact.digest.clone(),
            role: Some("warm_start".into()),
            title: Some("SFT warm-start adapter".into()),
            metadata: json!({"trainingArtifact": artifact}),
        });
        Some(std::path::PathBuf::from(path))
    } else {
        None
    };
    let create = training_create_request(
        &run_id,
        "cispo",
        "cispo-mlx-banking77-v1",
        "Local Banking77 CISPO on Apple Silicon MLX",
        "local",
        LOCAL_MLX_CISPO_RECIPE,
        &request,
        json!({
            "recipeId": LOCAL_MLX_CISPO_RECIPE,
            "backend": "cispo",
            "placement": PLACEMENT_TRAINING_CISPO_LOCAL,
            "trainingCursor": 0,
            "evaluationPlan": { "phases": ["baseline", "checkpoint", "final"], "checkpointEvery": 2, "transport": "tunnel", "metric": "reward" }
        }),
        input_refs,
    );
    super::sidecar_training::create_and_watch(
        service,
        request,
        create,
        PLACEMENT_TRAINING_CISPO_LOCAL,
        json!({
            "backend": "cispo",
            "base_model": "Qwen/Qwen3.5-0.8B",
            "task": "banking77",
            "rollout": {
                "url": rollout.url,
                "task_id": rollout.task_id,
                "bearer_token": rollout.token,
                "train_instances": 64,
                "heldout_instances": 16
            },
            "output_dir": output_dir,
            "max_steps": 4,
            "checkpoint_every": 2,
            "evaluation": evaluation_plan,
            "lora_rank": 8,
            "lora_alpha": 16.0,
            "lora_dropout": 0.0,
            "max_seq_length": 4096,
            "enable_thinking": false,
            "warm_start": warm_start
        }),
    )
    .await
}

async fn start_hosted(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(OptimizerRunRecord, Option<crate::storage::AppEvent>)> {
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("cispo_hosted_{}", &suffix[..12]);
    let evaluation_plan = tunneled_banking77_evaluation_plan(
        std::env::var("SYNTH_OPTIMIZERS_CISPO_CONTAINER_URL").ok(),
        "SYNTH_OPTIMIZERS_CISPO_CONTAINER_URL",
        "SYNTH_OPTIMIZERS_CISPO_CONTAINER_TOKEN",
        1,
        vec![1, 2],
    );
    let create = training_create_request(
        &run_id,
        "cispo",
        "cispo-slime-hosted-v1",
        "Hosted Banking77 CISPO · slime.v1",
        "hosted",
        HOSTED_CISPO_RECIPE,
        &request,
        json!({
            "recipeId": HOSTED_CISPO_RECIPE,
            "backend": "cispo.slime.v1",
            "placement": PLACEMENT_TRAINING_CISPO_HOSTED,
            "trainingCursor": 0,
            "evaluationPlan": { "phases": ["baseline", "checkpoint", "final"], "checkpointEvery": 1, "transport": "tunnel", "metric": "reward" }
        }),
        vec![OptimizerResourceRef {
            kind: "recipe".into(),
            id: HOSTED_CISPO_RECIPE.into(),
            digest: None,
            role: Some("configuration".into()),
            title: Some("Hosted CISPO slime".into()),
            metadata: json!({"epsHigh": 4.0, "tinkerBound": 5.0}),
        }],
    );
    super::sidecar_training::create_and_watch(
        service,
        request,
        create,
        PLACEMENT_TRAINING_CISPO_HOSTED,
        json!({
            "algorithm": "cispo",
            "implementation": "cispo.slime.v1",
            "task": "banking77",
            "eps_high": 4.0,
            "evaluation": evaluation_plan
        }),
    )
    .await
}

pub async fn restore_mirrors(service: &OptimizerService) {
    let Ok(runs) = service
        .list(OptimizerQuery {
            algorithm_id: Some("cispo".into()),
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
        !matches!(run.status.as_str(), "completed" | "failed" | "cancelled")
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

    #[test]
    fn production_source_does_not_name_mlx_or_tinker_urls() {
        let production = include_str!("cispo.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(!production.contains(&["127.0.0.1:", "8787"].concat()));
        assert!(!production.contains("SYNTH_MLX_CISPO_ROLLOUT_URL"));
        assert!(!production.contains("SYNTH_MLX_CISPO_WARM_START"));
        assert!(production.contains("training_artifact_id"));
        assert!(production.contains("create_and_watch"));
    }

    #[tokio::test]
    #[ignore = "needs synth-mlx-rl, managed Qwen weights, and a rollout service"]
    async fn local_cispo_dispatch_needs_synth_mlx_rl() {
        let dir = tempfile::tempdir().unwrap();
        let storage = crate::storage::Storage::open(dir.path().join("core")).unwrap();
        let journal = crate::storage::EventJournal::new(storage.database().clone());
        let content = crate::storage::ContentStore::new(storage.content_root());
        let visuals = crate::visuals::VisualRegistry::new(
            storage.database().clone(),
            journal.clone(),
            content,
        );
        let (events_tx, _) = tokio::sync::broadcast::channel(16);
        let manager = std::sync::Arc::new(crate::optimizers::OptimizerManager::with_home(
            dir.path().join("optimizer-home"),
        ));
        manager.install(None).unwrap();
        assert_eq!(manager.start().await.unwrap().phase, "ready");
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
                recipe_id: LOCAL_MLX_CISPO_RECIPE.into(),
                session_ref: Some("sess_cispo_e2e".into()),
                open_visual: Some(false),
                base_model: None,
                dataset_shard: None,
                candidate_set_id: None,
                training_artifact_id: None,
                search: None,
            },
        )
        .await
        .unwrap();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(8);
        let terminal = loop {
            let current = service.get(run.id.clone()).await.unwrap();
            if matches!(
                current.status.as_str(),
                "completed" | "failed" | "cancelled"
            ) {
                break current;
            }
            assert!(tokio::time::Instant::now() < deadline, "{}", current.status);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
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
            .any(|event| event.event_type == "cispo.clip.identity"));
        let _ = service.manager().stop().await;
    }

    #[test]
    fn hosted_cispo_is_not_admitted_without_the_canary_gate() {
        std::env::remove_var("SYNTH_OPTIMIZERS_CISPO_HOSTED_ADMITTED");
        assert!(!crate::optimizers::sidecar_training::admitted_placements()
            .contains(&PLACEMENT_TRAINING_CISPO_HOSTED));
    }
}
