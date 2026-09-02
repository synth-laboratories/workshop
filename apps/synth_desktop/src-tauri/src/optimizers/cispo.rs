//! Bounded CISPO recipes admitted by the Optimizers sidecar.

use super::container_training;
use super::models::{
    OptimizerQuery, OptimizerRecipeRunRequest, OptimizerResourceRef, OptimizerRunRecord,
};
use super::sidecar_training::{
    advertised_placement, spawn_watch_worker, training_create_request, tunneled_evaluation_plan,
    EvaluationContract, SidecarTrainingClient, HOSTED_BANKING77_CISPO_RECIPE, HOSTED_CISPO_RECIPE,
    LOCAL_MLX_CISPO_RECIPE, PLACEMENT_TRAINING_CISPO_HOSTED, PLACEMENT_TRAINING_CISPO_LOCAL,
};
use super::OptimizerService;
use anyhow::Result;
use serde_json::{json, Value};

pub(crate) const BANKING77_LABEL_TAXONOMY: &[&str] = &[
    "Refund_not_showing_up",
    "activate_my_card",
    "age_limit",
    "apple_pay_or_google_pay",
    "atm_support",
    "automatic_top_up",
    "balance_not_updated_after_bank_transfer",
    "balance_not_updated_after_cheque_or_cash_deposit",
    "beneficiary_not_allowed",
    "cancel_transfer",
    "card_about_to_expire",
    "card_acceptance",
    "card_arrival",
    "card_delivery_estimate",
    "card_linking",
    "card_not_working",
    "card_payment_fee_charged",
    "card_payment_not_recognised",
    "card_payment_wrong_exchange_rate",
    "card_swallowed",
    "cash_withdrawal_charge",
    "cash_withdrawal_not_recognised",
    "change_pin",
    "compromised_card",
    "contactless_not_working",
    "country_support",
    "declined_card_payment",
    "declined_cash_withdrawal",
    "declined_transfer",
    "direct_debit_payment_not_recognised",
    "disposable_card_limits",
    "edit_personal_details",
    "exchange_charge",
    "exchange_rate",
    "exchange_via_app",
    "extra_charge_on_statement",
    "failed_transfer",
    "fiat_currency_support",
    "get_disposable_virtual_card",
    "get_physical_card",
    "getting_spare_card",
    "getting_virtual_card",
    "lost_or_stolen_card",
    "lost_or_stolen_phone",
    "order_physical_card",
    "passcode_forgotten",
    "pending_card_payment",
    "pending_cash_withdrawal",
    "pending_top_up",
    "pending_transfer",
    "pin_blocked",
    "receiving_money",
    "request_refund",
    "reverted_card_payment?",
    "supported_cards_and_currencies",
    "terminate_account",
    "top_up_by_bank_transfer_charge",
    "top_up_by_card_charge",
    "top_up_by_cash_or_cheque",
    "top_up_failed",
    "top_up_limits",
    "top_up_reverted",
    "topping_up_by_card",
    "transaction_charged_twice",
    "transfer_fee_charged",
    "transfer_into_account",
    "transfer_not_received_by_recipient",
    "transfer_timing",
    "unable_to_verify_identity",
    "verify_my_identity",
    "verify_source_of_funds",
    "verify_top_up",
    "virtual_card_not_working",
    "visa_or_mastercard",
    "why_verify_identity",
    "wrong_amount_of_cash_received",
    "wrong_exchange_rate_for_cash_withdrawal",
];
use std::collections::HashSet;

pub fn recipe_catalog() -> Vec<Value> {
    vec![
        local_mlx_recipe(),
        hosted_cispo_recipe(HOSTED_BANKING77_CISPO_RECIPE, "Banking77 Tinker CISPO"),
        hosted_cispo_recipe(HOSTED_CISPO_RECIPE, "Hosted CISPO · Tinker"),
    ]
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

fn hosted_cispo_recipe(id: &str, title: &str) -> Value {
    let (available, reason) = hosted_cispo_availability();
    json!({
        "id": id,
        "title": title,
        "algorithmId": "cispo",
        "task": "banking77",
        "placement": PLACEMENT_TRAINING_CISPO_HOSTED,
        "availability": if available { "available" } else { "unavailable" },
        "availabilityReason": reason,
        "limits": {
            "backend": "cispo.slime.v1",
            "implementation": "slime-reference",
            "implementationVersion": "cispo.slime.v1",
            "maxSteps": 50,
            "groupSize": 64,
            "promptsPerUpdate": 3,
            "estimatedRollouts": 9600,
            "selectionExamples": 400,
            "heldoutExamples": 400,
            "costCeilingUsd": 35.0,
            "costNotice": "Hosted Tinker via the public CISPO service. Receipt-gated; unpaid fixture with SYNTH_OPTIMIZERS_CISPO_FIXTURE=1."
        },
        "credentialInputs": [],
        "prerequisites": hosted_cispo_prerequisites()
    })
}

fn hosted_cispo_prerequisites() -> Vec<&'static str> {
    vec![
        "synth-optimizers-cispo service --db … --bind 127.0.0.1:8880",
        "SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN",
        "SYNTH_OPTIMIZERS_CISPO_SERVICE_URL (default http://127.0.0.1:8880)",
        "TINKER_CISPO_VALIDATION_RECEIPT",
        "SYNTH_BANKING77_TRAIN_CSV + SYNTH_BANKING77_HELDOUT_CSV",
        "SYNTH_BANKING77_CISPO_PARENT_JSON (Tinker SFT state + sampler receipt)",
        "SYNTH_OPTIMIZERS_CISPO_FIXTURE=1 for unpaid",
    ]
}

fn hosted_cispo_availability() -> (bool, Value) {
    let admitted = advertised_placement(
        &json!({"placements": super::sidecar_training::admitted_placements()}),
        PLACEMENT_TRAINING_CISPO_HOSTED,
    );
    let client_ok = super::cispo_client::CispoOptimizerClient::from_env().is_ok();
    let reference_ok = super::hosted_sft::banking77_reference_sources().is_ok()
        && banking77_cispo_parent().is_ok();
    if admitted && client_ok && reference_ok {
        return (true, Value::Null);
    }
    if !admitted {
        return (
            false,
            json!("Hosted CISPO stays fail-closed until TINKER_CISPO_VALIDATION_RECEIPT points at a paid slime-canary receipt (validated=true, paid_update=true)."),
        );
    }
    if std::env::var("SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .is_none()
    {
        return (
            false,
            json!("SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN is required to reach the public CISPO service."),
        );
    }
    if std::env::var("SYNTH_OPTIMIZERS_CISPO_SERVICE_URL")
        .ok()
        .map(|value| value.trim().is_empty())
        .unwrap_or(false)
    {
        return (
            false,
            json!("SYNTH_OPTIMIZERS_CISPO_SERVICE_URL is empty; default is http://127.0.0.1:8880."),
        );
    }
    if !reference_ok {
        return (
            false,
            json!("Hosted Banking77 CISPO requires the NanoClassify train/heldout CSVs and SYNTH_BANKING77_CISPO_PARENT_JSON."),
        );
    }
    (
        false,
        json!("Public CISPO service client is not configured."),
    )
}

fn banking77_cispo_parent() -> Result<Value> {
    let raw = std::env::var("SYNTH_BANKING77_CISPO_PARENT_JSON")?;
    let path = std::path::PathBuf::from(raw.trim());
    let receipt: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    let state = receipt
        .get("state_checkpoint")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("CISPO parent receipt omitted state_checkpoint"))?;
    Ok(json!({
        "checkpoint_id": "nanoclassify-sft-parent",
        "provider_reference": state,
        "resume_token": state,
        "step": receipt.get("updates").and_then(Value::as_u64).unwrap_or(100),
        "digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "kind": "training"
    }))
}

fn hosted_cispo_config_json() -> Result<Value> {
    let sources = super::hosted_sft::banking77_reference_sources()?;
    let labels = BANKING77_LABEL_TAXONOMY;
    let heldout_indices = sources
        .heldout_indices_json
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    Ok(json!({
        "schema_version": "cispo.request.v1",
        "algorithm_id": "cispo",
        "implementation": "slime-reference",
        "implementation_version": "cispo.slime.v1",
        "provider": "tinker",
        "model_id": "openai/gpt-oss-20b",
        "base_model": "openai/gpt-oss-20b",
        "renderer_version": "renderers.gpt-oss.low.v1",
        "runner_version": "synth-optimizers.training.v1",
        "seed": 20260902,
        "repeat_index": 0,
        "mode": "canonical",
        "rank": 16,
        "parent_checkpoint": banking77_cispo_parent()?,
        "dataset": {
            "recipe_id": "banking77.cispo.nanoclassify.v1",
            "split_strategy": "banking77.nanoclassify.v1",
            "train_csv": sources.train_csv,
            "heldout_csv": sources.heldout_csv,
            "heldout_indices_json": heldout_indices,
            "split_seed": 20260907,
            "selection_seed": 20260908,
            "heldout_seed": 20260906,
            "dev_per_class": 10,
            "selection_size": 400,
            "heldout_size": 400,
            "label_taxonomy": labels,
            "system_prompt": format!(
                "Classify the customer banking message. Return exactly one label from this list, with no explanation or punctuation:\n{}",
                labels.join(", ")
            ),
            "scorer_version": "banking77.exact_label.v1",
            "heldout_locked": true
        },
        "training": {
            "updates": 50,
            "group_size": 64,
            "prompts_per_update": 3,
            "max_sample_tokens": 24,
            "temperature": 1.0,
            "learning_rate": 0.000001,
            "eps_clip": 1.0,
            "eps_clip_high": 4.0,
            "normalize_group_rewards": true,
            "checkpoint_every_updates": 10
        },
        "reward": { "version": "banking77.exact_label.v1", "task": "banking77" },
        "evaluation": {
            "scorer_version": "banking77.exact_label.v1",
            "heldout_locked": true,
            "mode": "canonical",
            "max_tokens": 64,
            "confidence": 0.95,
            "bootstrap_resamples": 4000,
            "minimum_claim_uplift": 0.01,
            "minimum_paired_examples": 400
        }
    }))
}

pub async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(OptimizerRunRecord, Option<crate::storage::AppEvent>)> {
    match request.recipe_id.as_str() {
        LOCAL_MLX_CISPO_RECIPE => start_local(service, request).await,
        id if super::sidecar_training::is_hosted_cispo_recipe(id) => {
            start_hosted(service, request).await
        }
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
    super::cispo_client::CispoOptimizerClient::from_env()?;
    let recipe_id = request.recipe_id.clone();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("cispo_hosted_{}", &suffix[..12]);
    let config_json = hosted_cispo_config_json()?;
    let create = training_create_request(
        &run_id,
        "cispo",
        "cispo.tinker.v1",
        "Hosted CISPO · Tinker",
        "hosted",
        &recipe_id,
        &request,
        json!({
            "recipeId": recipe_id,
            "backend": "cispo.slime.v1",
            "implementation": "slime-reference",
            "implementationVersion": "cispo.slime.v1",
            "placement": PLACEMENT_TRAINING_CISPO_HOSTED,
            "trainingCursor": 0,
            "evaluationPlan": {
                "scorer_version": "banking77.exact_label.v1",
                "heldout_locked": true,
                "mode": "learning_signal"
            }
        }),
        vec![OptimizerResourceRef {
            kind: "recipe".into(),
            id: recipe_id.clone(),
            digest: None,
            role: Some("configuration".into()),
            title: Some("Hosted CISPO slime".into()),
            metadata: json!({
                "epsHigh": 4.0,
                "tinkerBound": 5.0,
                "algorithmId": "cispo",
                "implementation": "slime-reference",
                "implementationVersion": "cispo.slime.v1"
            }),
        }],
    );
    super::sidecar_training::create_and_watch(
        service,
        request,
        create,
        PLACEMENT_TRAINING_CISPO_HOSTED,
        json!({ "config_json": config_json }),
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
        let start_local = production
            .split("async fn start_local")
            .nth(1)
            .unwrap()
            .split("async fn start_hosted")
            .next()
            .unwrap();
        let start_hosted = production.split("async fn start_hosted").nth(1).unwrap();
        assert!(!production.contains(&["127.0.0.1:", "8787"].concat()));
        assert!(!production.contains("SYNTH_MLX_CISPO_ROLLOUT_URL"));
        assert!(!production.contains("SYNTH_MLX_CISPO_WARM_START"));
        assert!(!start_local.contains("banking77"));
        assert!(!production.contains("alfworld"));
        assert!(!production.contains("cookbooks/optimizers/cispo/rollout.json"));
        assert!(start_local.contains("bind_cispo"));
        assert!(!start_hosted.contains("bind_cispo"));
        assert!(start_hosted.contains("hosted_cispo_config_json"));
        assert!(production.contains("training_artifact_id"));
        assert!(production.contains("\"signal_attempts\": 24"));
        assert!(production.contains("\"group_size\": 16"));
        assert!(production.contains("\"max_tokens\": 256"));
        assert!(production.contains("\"learning_rate\": 0.00005"));
        assert!(production.contains("\"checkpoint_every\": 1"));
        assert!(production.contains("create_and_watch"));
    }

    fn paid_slime_receipt_path() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cispo.slime.v1.receipt.json");
        std::fs::write(
            &path,
            r#"{
              "schema_version": "tinker.capability_validation.v1",
              "capability": "cispo.slime.v1",
              "validated": true,
              "paid_update": true
            }"#,
        )
        .unwrap();
        (dir, path)
    }

    fn reference_paths() -> (
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let train = dir.path().join("train.csv");
        let heldout = dir.path().join("heldout.csv");
        let parent = dir.path().join("sft.json");
        std::fs::write(&train, "text,category\nhello,a\nworld,b\n").unwrap();
        std::fs::write(&heldout, "text,category\nheld,a\nout,b\n").unwrap();
        std::fs::write(&parent, r#"{"state_checkpoint":"tinker://state","sampler_checkpoint":"tinker://sampler","updates":100}"#).unwrap();
        (dir, train, heldout, parent)
    }

    #[test]
    fn hosted_recipe_catalog_is_available_only_with_receipt_and_cispo_token() {
        let previous_receipt = std::env::var("TINKER_CISPO_VALIDATION_RECEIPT").ok();
        let previous_token = std::env::var("SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN").ok();
        std::env::remove_var("TINKER_CISPO_VALIDATION_RECEIPT");
        std::env::remove_var("SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN");
        let recipes = recipe_catalog();
        let hosted: Vec<_> = recipes
            .iter()
            .filter(|recipe| {
                matches!(
                    recipe["id"].as_str(),
                    Some(HOSTED_CISPO_RECIPE) | Some(HOSTED_BANKING77_CISPO_RECIPE)
                )
            })
            .collect();
        assert_eq!(hosted.len(), 2);
        for recipe in &hosted {
            assert_eq!(recipe["availability"], "unavailable");
            assert!(
                recipe["availabilityReason"]
                    .as_str()
                    .unwrap()
                    .contains("TINKER_CISPO_VALIDATION_RECEIPT"),
                "{}",
                recipe["availabilityReason"]
            );
            let prerequisites = recipe["prerequisites"].as_array().unwrap();
            assert!(prerequisites.iter().any(|item| {
                item.as_str() == Some("synth-optimizers-cispo service --db … --bind 127.0.0.1:8880")
            }));
            assert!(prerequisites
                .iter()
                .any(|item| item.as_str() == Some("SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN")));
            assert!(!serde_json::to_string(recipe)
                .unwrap()
                .contains("tunnel lease"));
        }

        let (_dir, receipt) = paid_slime_receipt_path();
        std::env::set_var("TINKER_CISPO_VALIDATION_RECEIPT", receipt.to_str().unwrap());
        let with_receipt = recipe_catalog();
        let alias = with_receipt
            .iter()
            .find(|recipe| recipe["id"] == HOSTED_CISPO_RECIPE)
            .unwrap();
        assert_eq!(alias["availability"], "unavailable");
        assert!(
            alias["availabilityReason"]
                .as_str()
                .unwrap()
                .contains("SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN"),
            "{}",
            alias["availabilityReason"]
        );

        std::env::set_var("SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN", "local-qa-token");
        let (_reference_dir, train, heldout, parent) = reference_paths();
        std::env::set_var("SYNTH_BANKING77_TRAIN_CSV", train);
        std::env::set_var("SYNTH_BANKING77_HELDOUT_CSV", heldout);
        std::env::set_var("SYNTH_BANKING77_CISPO_PARENT_JSON", parent);
        let admitted = recipe_catalog();
        for id in [HOSTED_CISPO_RECIPE, HOSTED_BANKING77_CISPO_RECIPE] {
            let recipe = admitted.iter().find(|recipe| recipe["id"] == id).unwrap();
            assert_eq!(recipe["availability"], "available", "{id}");
            assert!(recipe["availabilityReason"].is_null());
        }

        match previous_receipt {
            Some(value) => std::env::set_var("TINKER_CISPO_VALIDATION_RECEIPT", value),
            None => std::env::remove_var("TINKER_CISPO_VALIDATION_RECEIPT"),
        }
        match previous_token {
            Some(value) => std::env::set_var("SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN", value),
            None => std::env::remove_var("SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN"),
        }
        std::env::remove_var("SYNTH_BANKING77_TRAIN_CSV");
        std::env::remove_var("SYNTH_BANKING77_HELDOUT_CSV");
        std::env::remove_var("SYNTH_BANKING77_CISPO_PARENT_JSON");
    }

    #[test]
    fn hosted_cispo_config_json_is_cispo_request_v1() {
        let (_dir, train, heldout, parent) = reference_paths();
        std::env::set_var("SYNTH_BANKING77_TRAIN_CSV", train);
        std::env::set_var("SYNTH_BANKING77_HELDOUT_CSV", heldout);
        std::env::set_var("SYNTH_BANKING77_CISPO_PARENT_JSON", parent);
        let request = hosted_cispo_config_json().unwrap();
        assert_eq!(request["schema_version"], "cispo.request.v1");
        assert_eq!(request["algorithm_id"], "cispo");
        assert_eq!(request["implementation"], "slime-reference");
        assert_eq!(request["implementation_version"], "cispo.slime.v1");
        assert_eq!(request["provider"], "tinker");
        assert_eq!(request["model_id"], "openai/gpt-oss-20b");
        assert_eq!(request["mode"], "canonical");
        assert_eq!(request["renderer_version"], "renderers.gpt-oss.low.v1");
        assert_eq!(
            request["dataset"]["recipe_id"],
            "banking77.cispo.nanoclassify.v1"
        );
        assert_eq!(
            request["dataset"]["split_strategy"],
            "banking77.nanoclassify.v1"
        );
        assert_eq!(request["dataset"]["heldout_locked"], true);
        assert_eq!(
            request["dataset"]["label_taxonomy"]
                .as_array()
                .unwrap()
                .len(),
            77
        );
        assert!(request["dataset"]["system_prompt"]
            .as_str()
            .unwrap()
            .contains("card_arrival"));
        assert_eq!(request["training"]["updates"], 50);
        assert_eq!(request["training"]["group_size"], 64);
        assert_eq!(request["training"]["prompts_per_update"], 3);
        assert_eq!(request["training"]["eps_clip"], 1.0);
        assert_eq!(request["training"]["eps_clip_high"], 4.0);
        assert_eq!(request["training"]["checkpoint_every_updates"], 10);
        assert_eq!(request["reward"]["version"], "banking77.exact_label.v1");
        assert_eq!(request["reward"]["task"], "banking77");
        assert_eq!(
            request["evaluation"]["scorer_version"],
            "banking77.exact_label.v1"
        );
        assert_eq!(request["evaluation"]["heldout_locked"], true);
        assert_eq!(request["evaluation"]["mode"], "canonical");
        assert_eq!(request["evaluation"]["minimum_paired_examples"], 400);
        assert!(request["evaluation"].get("transport").is_none());
        assert!(request["evaluation"].get("container").is_none());
        std::env::remove_var("SYNTH_BANKING77_TRAIN_CSV");
        std::env::remove_var("SYNTH_BANKING77_HELDOUT_CSV");
        std::env::remove_var("SYNTH_BANKING77_CISPO_PARENT_JSON");
    }

    #[test]
    fn local_recipe_publishes_its_worst_case_rollout_bound() {
        let recipe = local_mlx_recipe();
        assert_eq!(recipe["limits"]["maxTotalEnvironmentRollouts"], 128);
        assert_eq!(recipe["limits"]["maxSteps"], 1);
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
                container_id: None,
                training_artifact_id: None,
                plan_override: None,
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
    fn hosted_cispo_is_not_admitted_even_when_the_legacy_environment_flag_is_set() {
        std::env::set_var("SYNTH_OPTIMIZERS_CISPO_HOSTED_ADMITTED", "1");
        std::env::remove_var("TINKER_CISPO_VALIDATION_RECEIPT");
        assert!(!crate::optimizers::sidecar_training::admitted_placements()
            .contains(&PLACEMENT_TRAINING_CISPO_HOSTED));
        std::env::remove_var("SYNTH_OPTIMIZERS_CISPO_HOSTED_ADMITTED");
    }
}
