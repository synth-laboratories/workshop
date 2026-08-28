//! Bounded CISPO recipes admitted by the Optimizers sidecar.

use super::container_training;
use super::models::{
    OptimizerQuery, OptimizerRecipeRunRequest, OptimizerResourceRef, OptimizerRunRecord,
};
use super::sidecar_training::{
    advertised_placement, spawn_watch_worker, training_create_request, tunneled_evaluation_plan,
    EvaluationContract, SidecarTrainingClient, HOSTED_CISPO_RECIPE, LOCAL_MLX_CISPO_RECIPE,
    PLACEMENT_TRAINING_CISPO_HOSTED, PLACEMENT_TRAINING_CISPO_LOCAL,
};
use super::OptimizerService;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashSet;

pub fn recipe_catalog() -> Vec<Value> {
    vec![local_mlx_recipe(), hosted_slime_recipe()]
}

fn local_mlx_recipe() -> Value {
    let apple_silicon = cfg!(all(target_os = "macos", target_arch = "aarch64"));
    let model_ready = super::mlx_runtime::require_training_model().is_ok();
    let available = apple_silicon && model_ready;
    json!({
        "id": LOCAL_MLX_CISPO_RECIPE,
        "title": "This Mac · CISPO (MLX)",
        "algorithmId": "cispo",
        "task": Value::Null,
        "placement": PLACEMENT_TRAINING_CISPO_LOCAL,
        "availability": if available { "available" } else { "unavailable" },
        "availabilityReason": if available { Value::Null } else { json!("Local CISPO requires Apple Silicon and the managed on-device training model. Start binds a ready container's advertised CISPO contract.") },
        "limits": {
            "backend": "cispo",
            "maxSteps": 1,
            "costCeilingUsd": 0.0,
            "maxTotalEnvironmentRollouts": 128,
            "costNotice": "Local Apple Silicon MLX compute. Warm-start from a selected SFT artifact id; otherwise the visual reports cispo_no_learning_signal.",
            "resolvedConfig": {
                "baseModel": "Qwen/Qwen3.5-2B",
                "task": "container",
                "loraDropout": 0.0,
                "warmStart": "training_artifact_id"
            }
        },
        "credentialInputs": [],
        "prerequisites": ["Optimizers sidecar", "ready container advertising a CISPO contract", "optional SFT warm-start adapter"]
    })
}

fn hosted_slime_recipe() -> Value {
    let admitted = advertised_placement(
        &json!({"placements": super::sidecar_training::admitted_placements()}),
        PLACEMENT_TRAINING_CISPO_HOSTED,
    );
    json!({
        "id": HOSTED_CISPO_RECIPE,
        "title": "Hosted CISPO · slime.v1",
        "algorithmId": "cispo",
        "task": Value::Null,
        "placement": PLACEMENT_TRAINING_CISPO_HOSTED,
        "availability": if admitted { "available" } else { "unavailable" },
        "availabilityReason": if admitted { Value::Null } else { json!("Hosted CISPO stays fail-closed until the slime clip canary (1+eps_high) admits it.") },
        "limits": {
            "backend": "cispo.slime.v1",
            "maxSteps": 2,
            "costCeilingUsd": 10.0,
            "costNotice": "Hosted Tinker plus the bound container. Sidecar owns the tunnel lease."
        },
        "credentialInputs": [],
        "prerequisites": ["Optimizers sidecar", "authenticated sidecar capability projecting a durable signed slime-canary admission receipt"]
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
    let (bind, cispo) =
        container_training::bind_cispo(service, request.container_id.as_deref()).await?;
    let evaluation_plan = tunneled_evaluation_plan(
        Some(cispo.rollout_url.clone()),
        "SYNTH_OPTIMIZERS_CISPO_ROLLOUT_URL",
        "SYNTH_OPTIMIZERS_CISPO_ROLLOUT_TOKEN",
        1,
        vec![1, 2],
        EvaluationContract {
            task: bind.task_id.clone(),
            harness: cispo.harness.clone(),
            plan_ref: cispo.plan_ref.clone(),
            world_ref: cispo.heldout_world_ref.clone(),
        },
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
    if let Some(dataset_digest) = bind.dataset_digest.clone() {
        input_refs.push(OptimizerResourceRef {
            kind: "dataset".into(),
            id: format!("{}:workload", bind.task_id),
            digest: Some(dataset_digest),
            role: Some("rollout_workload".into()),
            title: Some("Container rollout workload".into()),
            metadata: json!({
                "containerId": bind.container_id,
                "taskId": bind.task_id,
                "source": "workshop_container"
            }),
        });
    }
    let warm_start = if let Some(artifact_id) = request.training_artifact_id.as_deref() {
        let mut artifact = crate::training_artifacts::get(artifact_id)?;
        if artifact.dataset_digest.is_none() || artifact.config_digest.is_none() {
            let producing_run = service.get(artifact.producing_run_id.clone()).await?;
            artifact.dataset_digest = artifact.dataset_digest.or_else(|| {
                producing_run
                    .input_refs
                    .iter()
                    .find(|item| item.kind == "dataset" && item.role.as_deref() == Some("train"))
                    .and_then(|item| item.digest.clone())
            });
            artifact.config_digest = artifact.config_digest.or_else(|| {
                producing_run
                    .input_refs
                    .iter()
                    .find(|item| item.kind == "training_configuration")
                    .and_then(|item| item.digest.clone())
            });
            crate::training_artifacts::register(artifact.clone())?;
        }
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
        crate::training_artifacts::lease(artifact_id, &run_id)?;
        Some(std::path::PathBuf::from(path))
    } else {
        None
    };
    let create = training_create_request(
        &run_id,
        "cispo",
        "cispo-mlx-v1",
        "Local CISPO on Apple Silicon MLX",
        "local",
        LOCAL_MLX_CISPO_RECIPE,
        &request,
        json!({
            "recipeId": LOCAL_MLX_CISPO_RECIPE,
            "backend": "cispo",
            "placement": PLACEMENT_TRAINING_CISPO_LOCAL,
            "containerId": bind.container_id,
            "trainingCursor": 0,
            "evaluationPlan": { "phases": ["baseline", "checkpoint", "final"], "checkpointEvery": 1, "transport": "tunnel", "metric": "reward" }
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
            "base_model": "Qwen/Qwen3.5-2B",
            "task": bind.task_id.clone(),
            "implementation": cispo.implementation,
            "rollout": {
                // MLX appends its fixed `/training/*` routes. This must be the
                // registered service base, never an operation like `/rollout`.
                "url": bind.base_url,
                "task_id": bind.task_id,
                // The MLX preflight reserves prompt plus generation. Leaving
                // the runtime's 512-token default implicit makes a one-row
                // micro-batch exceed supported unified-memory budgets. A 256
                // token completion still covers text trajectories and is far
                // above the Banking77 label contract.
                "max_tokens": 256,
                "bearer_token": cispo.token,
                "reward_url": cispo.reward_url,
                "train_world_ref": cispo.train_world_ref,
                "heldout_world_ref": cispo.heldout_world_ref,
                "train_instances": 16,
                "heldout_instances": 16
            },
            "output_dir": output_dir,
            "max_steps": 1,
            "checkpoint_every": 1,
            "signal_attempts": 24,
            // One 16-member on-policy group reproduces the accepted Banking77
            // flow; the MLX service accumulates it at micro-batch size one.
            "group_size": 16,
            "learning_rate": 0.00005,
            "evaluation": evaluation_plan,
            "lora_rank": 8,
            "lora_alpha": 16.0,
            "lora_dropout": 0.0,
            "max_seq_length": super::mlx_runtime::LOCAL_TRAINING_MAX_SEQ_LENGTH,
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
    let (bind, cispo) =
        container_training::bind_cispo(service, request.container_id.as_deref()).await?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("cispo_hosted_{}", &suffix[..12]);
    let evaluation_plan = tunneled_evaluation_plan(
        Some(cispo.rollout_url.clone()),
        "SYNTH_OPTIMIZERS_CISPO_CONTAINER_URL",
        "SYNTH_OPTIMIZERS_CISPO_CONTAINER_TOKEN",
        1,
        vec![1, 2],
        EvaluationContract {
            task: bind.task_id.clone(),
            harness: cispo.harness.clone(),
            plan_ref: cispo.plan_ref.clone(),
            world_ref: cispo.heldout_world_ref.clone(),
        },
    );
    let create = training_create_request(
        &run_id,
        "cispo",
        "cispo-slime-hosted-v1",
        "Hosted CISPO · slime.v1",
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
            "implementation": cispo.implementation,
            "task": bind.task_id,
            "eps_high": 4.0,
            "evaluation": evaluation_plan,
            "rollout": {
                "url": cispo.rollout_url,
                "reward_url": cispo.reward_url,
                "train_world_ref": cispo.train_world_ref,
                "heldout_world_ref": cispo.heldout_world_ref
            }
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

