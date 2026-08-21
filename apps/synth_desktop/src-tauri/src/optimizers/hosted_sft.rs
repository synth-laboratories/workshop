//! Hosted SFT recipes backed by the public `synth-optimizers` control plane.
//!
//! Optimizers-beta remains an internal training executor. Workshop starts, watches,
//! cancels, and mirrors only public SFT runs before opening `optimizer.sft.live.v1`.

use super::events::OptimizerEventDraft;
use super::{
    ingest,
    models::{
        OptimizerCapabilities, OptimizerCreateRequest, OptimizerExecutionBinding, OptimizerQuery,
        OptimizerRecipeRunRequest, OptimizerResourceRef, OptimizerRunRecord, OptimizerRunStatus,
    },
    sft_client::SftOptimizerClient,
    sidecar_training::PLACEMENT_TRAINING_SFT_HOSTED,
    OptimizerService,
};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::Duration;
use tokio::{sync::watch, time::sleep};

pub const HOSTED_SFT_RECIPE: &str = "sft.hosted.tinker.v1";
const CHECKPOINT_STEPS: [u32; 3] = [10, 20, 30];
/// Length of training. `optimizers-beta` used to infer this as
/// `max(checkpoint_steps)`, so the checkpoint list silently decided how long a
/// run trained. It is now named separately and required.
const TRAINING_STEPS: u32 = 30;
const CAMPAIGN_ROLLOUTS: u32 = 2;
/// Rows longer than this lose their assistant tokens and are dropped.
const MAX_SEQ_LEN: u32 = 4096;
/// Share of rows allowed to exceed the cap before the run refuses rather than
/// training on whatever happened to be short enough.
const MAX_DROPPED_FRACTION: &str = "0.05";
/// Seconds one checkpoint rollout may take.
const CHECKPOINT_EVALUATION_TIMEOUT_S: u32 = 3600;
/// The Workshop approval broker needs a hard per-run authorization ceiling.
/// Hosted SFT recipes are fixed-shape (model, steps, checkpoints, and rollout
/// counts are product-owned), so cap each paid launch at one fifth of the
/// five-run acceptance budget. The public service remains the execution
/// authority and Workshop reconciles actual usage from its event stream.
const HOSTED_SFT_COST_CEILING_USD: f64 = 10.0;
/// Allowlisted dataset shards. A caller selects one; it cannot supply a path.
const DATASET_SHARDS: [&str; 2] = ["train_a", "train_b"];
/// Torn-tail reads while the producer appends are transient. Give up only
/// after the upstream stays unreadable across this many consecutive polls.
const MAX_CONSECUTIVE_PAGE_ERRORS: u32 = 20;
const HOSTED_SFT_LORA_RANK: u64 = 8;

pub fn recipe_catalog() -> Vec<Value> {
    vec![hosted_tinker_recipe()]
}

fn hosted_tinker_recipe() -> Value {
    let catalog_ok = super::tinker_catalog::TinkerBaseModelCatalog::load().is_ok();
    let availability =
        if catalog_ok && SftOptimizerClient::from_env().is_ok() && training_jsonl().is_ok() {
            "available"
        } else {
            "unavailable"
        };
    json!({
        "id": HOSTED_SFT_RECIPE,
        "title": "Hosted Tinker SFT",
        "algorithmId": "sft",
        "source": "hosted",
        "task": Value::Null,
        "availability": availability,
        "limits": {
            "backend": "tinker",
            "checkpointSteps": CHECKPOINT_STEPS,
            "evaluationPlan": { "phases": ["baseline", "checkpoint", "final"], "checkpointSteps": CHECKPOINT_STEPS, "transport": "tunnel", "metric": "reward" },
            "campaignRolloutsPerCheckpoint": CAMPAIGN_ROLLOUTS,
            "datasetShards": DATASET_SHARDS,
            "evalSeeds": [1, 2],
            "costCeilingUsd": HOSTED_SFT_COST_CEILING_USD,
            "costNotice": "Hosted Tinker training plus checkpoint campaigns against the bound container. Provider charges apply."
        },
        "credentialInputs": [],
        "prerequisites": [
            "SYNTH_OPTIMIZERS_SFT_SERVICE_URL (or local http://127.0.0.1:8878)",
            "SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN",
            "TINKER_API_KEY held by the Optimizers-beta executor",
            "SYNTH_SFT_TRAIN_JSONL",
            "SYNTH_SFT_EVAL_CONTAINER_URL",
            "SYNTH_SFT_EVAL_PLAN_REF",
            "SYNTH_SFT_EVAL_WORLD_REF"
        ],
    })
}

pub async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    if request.recipe_id != HOSTED_SFT_RECIPE {
        bail!("unknown hosted SFT recipe: {}", request.recipe_id);
    }
    start_hosted(service, request).await
}

fn content_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn dataset_digest_for_path(path: &std::path::Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("read dataset bytes {}", path.display()))?;
    Ok(content_sha256(&bytes))
}

fn required_env(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("hosted SFT requires {name}"))
}

fn training_jsonl() -> Result<std::path::PathBuf> {
    let path = std::path::PathBuf::from(required_env("SYNTH_SFT_TRAIN_JSONL")?);
    if !path.is_file() {
        bail!("hosted SFT corpus is not a file: {}", path.display());
    }
    Ok(path)
}

fn eval_container_url() -> Result<String> {
    required_env("SYNTH_SFT_EVAL_CONTAINER_URL")
}

fn eval_plan_ref() -> Result<String> {
    required_env("SYNTH_SFT_EVAL_PLAN_REF")
}

fn eval_world_ref() -> Result<String> {
    required_env("SYNTH_SFT_EVAL_WORLD_REF")
}

fn eval_harness() -> String {
    std::env::var("SYNTH_SFT_EVAL_HARNESS")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "rollout".into())
}

/// Materialize one allowlisted shard under the instance data root. Two shards
/// are disjoint halves of the pinned corpus, so two runs train on genuinely
/// different data and the producer digests them differently.
fn materialize_dataset_shard(shard: &str) -> Result<std::path::PathBuf> {
    let index = DATASET_SHARDS
        .iter()
        .position(|candidate| *candidate == shard)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "unknown dataset shard {shard}; allowed: {}",
                DATASET_SHARDS.join(", ")
            )
        })?;
    let source = training_jsonl()?;
    let text = std::fs::read_to_string(&source).context("read hosted SFT corpus")?;
    let rows: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if rows.len() < DATASET_SHARDS.len() {
        bail!("hosted SFT corpus is too small to shard");
    }
    let half = rows.len() / DATASET_SHARDS.len();
    let start = index * half;
    let end = if index + 1 == DATASET_SHARDS.len() {
        rows.len()
    } else {
        start + half
    };
    let destination_dir = crate::instance::data_root().join("optimizers/datasets/sft");
    std::fs::create_dir_all(&destination_dir).context("create SFT shard directory")?;
    let destination = destination_dir.join(format!("{shard}.jsonl"));
    let body = format!("{}\n", rows[start..end].join("\n"));
    std::fs::write(&destination, body).context("write SFT shard")?;
    Ok(destination)
}

async fn start_hosted(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    let catalog = super::tinker_catalog::TinkerBaseModelCatalog::load()?;
    let model_id = catalog.resolve(request.base_model.as_deref())?;
    let shard = request
        .dataset_shard
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DATASET_SHARDS[0])
        .to_string();
    let shard_path = materialize_dataset_shard(&shard)?;
    let dataset_digest = dataset_digest_for_path(&shard_path)?;
    super::sft_result::validate_dataset_digest(&shard_path, &dataset_digest)?;
    let container_url = eval_container_url()?;
    let plan_ref = eval_plan_ref()?;
    let world_ref = eval_world_ref()?;
    let harness = eval_harness();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("sft_hosted_{}_{}", shard, &suffix[..8]);
    let training_file = format!("file_train_{shard}_{}", &suffix[..8]);
    let config_toml = hosted_config_toml(
        &run_id,
        &training_file,
        &model_id,
        &container_url,
        &shard_path.to_string_lossy(),
        &dataset_digest,
        &harness,
        &plan_ref,
        &world_ref,
    );
    let create = OptimizerCreateRequest {
        algorithm_id: "sft".into(),
        algorithm_version: Some("hosted-tinker-v1".into()),
        objective: Some(
            "Hosted Tinker SFT · checkpoint campaigns against the bound container".into(),
        ),
        source: Some("hosted".into()),
        project_ref: Some("sft@hosted-tinker".into()),
        session_ref: request.session_ref.clone(),
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![
            OptimizerExecutionBinding {
                kind: "optimizer_sidecar".into(),
                id: HOSTED_SFT_RECIPE.into(),
                label: Some("Optimizers sidecar hosted SFT".into()),
                status: Some("admitted".into()),
                metadata: json!({
                    "recipeId": HOSTED_SFT_RECIPE,
                    "backend": "tinker",
                    "datasetFile": training_file,
                    "datasetShard": shard,
                    "baseModel": model_id,
                    "placement": PLACEMENT_TRAINING_SFT_HOSTED,
                }),
            },
            OptimizerExecutionBinding {
                kind: "local_slot".into(),
                id: container_url.clone(),
                label: Some("checkpoint evaluation container".into()),
                status: Some("leased".into()),
                metadata: json!({
                    "role": "checkpoint_evaluation",
                    "worldRef": world_ref,
                    "evaluationPlanRef": plan_ref,
                }),
            },
        ]),
        input_refs: Some(vec![
            OptimizerResourceRef {
                kind: "recipe".into(),
                id: HOSTED_SFT_RECIPE.into(),
                digest: None,
                role: Some("configuration".into()),
                title: Some("Hosted Tinker SFT".into()),
                metadata: json!({"backend": "tinker", "baseModel": model_id}),
            },
            OptimizerResourceRef {
                kind: "dataset".into(),
                id: training_file.clone(),
                digest: Some(dataset_digest.clone()),
                role: Some("train".into()),
                title: Some(format!("SFT corpus · shard {shard}")),
                metadata: json!({"shard": shard, "shards": DATASET_SHARDS, "datasetDigest": dataset_digest}),
            },
        ]),
        capabilities: Some(OptimizerCapabilities::for_algorithm("sft")),
        summary: Some(json!({
            "recipeId": HOSTED_SFT_RECIPE,
            "backend": "tinker",
            "producer": "synth-optimizers",
            "baseModel": model_id,
            "datasetShard": shard,
            "datasetDigest": dataset_digest,
            "adapter": lora_adapter_label(HOSTED_SFT_LORA_RANK),
            "rank": HOSTED_SFT_LORA_RANK,
            "localSlot": container_url,
            "checkpointSteps": CHECKPOINT_STEPS,
        })),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    admit_hosted(service, request, create, config_toml).await
}

fn hosted_config_toml(
    run_id: &str,
    training_file: &str,
    model_id: &str,
    container_url: &str,
    training_jsonl: &str,
    dataset_digest: &str,
    harness: &str,
    plan_ref: &str,
    world_ref: &str,
) -> String {
    let steps = CHECKPOINT_STEPS
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let adapter = lora_adapter_label(HOSTED_SFT_LORA_RANK);
    format!(
        r#"run_id = "{run_id}"
backend = "tinker"
base_model = "{model_id}"
adapter = "{adapter}"
training_file_id = "{training_file}"
training_jsonl = "{training_jsonl}"
dataset_digest = "{dataset_digest}"
selection_file_id = "file_selection"
heldout_file_id = "file_heldout"
accelerator_slots = 1
checkpoint_steps = [{steps}]
training_steps = {TRAINING_STEPS}
max_seq_len = {MAX_SEQ_LEN}
max_dropped_fraction = {MAX_DROPPED_FRACTION}
campaign_rollouts_per_checkpoint = {CAMPAIGN_ROLLOUTS}
evaluator_version = "{plan_ref}"
container_url = "{container_url}"
checkpoint_evaluation_seeds = [1, 2]
checkpoint_evaluation_policy_harness = "{harness}"
checkpoint_evaluation_plan_ref = "{plan_ref}"
checkpoint_evaluation_world_ref = "{world_ref}"
checkpoint_evaluation_timeout_s = {CHECKPOINT_EVALUATION_TIMEOUT_S}

[metadata]
evaluation_schema = "training.evaluation.plan.v1"
evaluation_phases = ["baseline", "checkpoint", "final"]
evaluation_transport = "tunnel"
evaluation_metric = "reward"
evaluation_required = true
evaluation_exact_checkpoint_required = true
evaluation_auth = "sidecar_tunnel_lease"
evaluation_harness = "{harness}"
evaluation_plan_ref = "{plan_ref}"
evaluation_world_ref = "{world_ref}"
evaluation_sample_count = 16
evaluation_timeout_s = {CHECKPOINT_EVALUATION_TIMEOUT_S}

[checkpoint_evaluation_policy]
api_key_env = "TINKER_API_KEY"

[hyperparameters]
rank = 8
batch_size = 2
lr = 0.001
"#
    )
}

async fn admit_hosted(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
    create: OptimizerCreateRequest,
    config_toml: String,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    super::sidecar_training::create_and_watch(
        service,
        request,
        create,
        PLACEMENT_TRAINING_SFT_HOSTED,
        json!({ "config_toml": config_toml }),
    )
    .await
}

#[allow(dead_code)]
async fn spawn_hosted_worker(
    service: &OptimizerService,
    client: SftOptimizerClient,
    run_id: String,
    config_toml: Option<String>,
    start_cursor: u64,
) {
    let (cancel_tx, cancel_rx) = watch::channel(false);
    service
        .register_local_recipe(run_id.clone(), cancel_tx)
        .await;
    let _ = persist_hosted_cursor(service, &run_id, start_cursor, true).await;
    let worker = service.clone();
    tokio::spawn(async move {
        if let Err(error) = run_hosted_worker(
            worker.clone(),
            client,
            run_id.clone(),
            config_toml,
            start_cursor,
            cancel_rx,
        )
        .await
        {
            eprintln!("hosted SFT worker {run_id} failed: {error:#}");
            // A failure the viewer cannot read is not evidence. Carry the
            // reason onto the terminal event instead of dropping it on stderr.
            let _ = append_failure(&worker, &run_id, &format!("{error:#}")).await;
        }
        worker.unregister_local_recipe(&run_id).await;
    });
}

pub async fn restore_hosted_mirrors(service: &OptimizerService) {
    let Ok(runs) = service
        .list(OptimizerQuery {
            algorithm_id: Some("sft".into()),
            source: Some("hosted".into()),
            ..OptimizerQuery::default()
        })
        .await
    else {
        return;
    };
    let registered = service.registered_local_recipes().await;
    let Ok(client) =
        super::sidecar_training::SidecarTrainingClient::from_manager(service.manager()).await
    else {
        return;
    };
    for (run_id, cursor) in hosted_runs_needing_restore(&runs, &registered) {
        super::sidecar_training::spawn_watch_worker(service, client.clone(), run_id, cursor).await;
    }
}

pub(crate) fn hosted_runs_needing_restore(
    runs: &[OptimizerRunRecord],
    registered: &HashSet<String>,
) -> Vec<(String, u64)> {
    runs.iter()
        .filter(|run| {
            run.source == "hosted"
                && run.algorithm_id == "sft"
                && !OptimizerRunStatus::str_is_terminal(&run.status)
                && !registered.contains(&run.id)
        })
        .map(|run| (run.id.clone(), resume_cursor(run)))
        .collect()
}

fn resume_cursor(run: &OptimizerRunRecord) -> u64 {
    run.summary
        .get("hostedMirror")
        .and_then(|value| value.get("cursor"))
        .and_then(Value::as_u64)
        .unwrap_or(run.cursor_seq)
}

fn lora_adapter_label(rank: u64) -> String {
    format!("lora_r{rank}")
}

fn content_digest_for_path(path: &std::path::Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(super::sft_result::sha256_bytes(&bytes))
}

async fn persist_hosted_cursor(
    service: &OptimizerService,
    run_id: &str,
    cursor: u64,
    attached: bool,
) -> Result<()> {
    let mut run = service.get(run_id.to_string()).await?;
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert(
        "hostedMirror".into(),
        json!({
            "cursor": cursor,
            "attached": attached,
        }),
    );
    run.summary = Value::Object(summary);
    service.persist_run(run).await?;
    Ok(())
}

#[allow(dead_code)]
async fn run_hosted_worker(
    service: OptimizerService,
    client: SftOptimizerClient,
    run_id: String,
    config_toml: Option<String>,
    start_cursor: u64,
    mut cancel: watch::Receiver<bool>,
) -> Result<()> {
    if let Some(toml) = config_toml.as_deref() {
        client.submit_toml(&run_id, toml).await?;
    }
    let mut upstream_cursor = start_cursor;
    // The producer appends to its log while we page it. A read that lands on a
    // half-written record is a retry, not a dead run — the cursor has not moved
    // and the next page re-reads the same bytes. Only a persistent failure ends
    // the mirror, so a producer that succeeds is never reported as failed.
    let mut consecutive_page_errors = 0u32;
    loop {
        match client
            .optimizer_events_after(&run_id, upstream_cursor, 500)
            .await
        {
            Ok(page) => {
                consecutive_page_errors = 0;
                ingest::ingest_event_page(&service, &run_id, "sft", &page, &mut upstream_cursor)
                    .await?;
                let _ = persist_hosted_cursor(&service, &run_id, upstream_cursor, true).await;
            }
            Err(error) => {
                consecutive_page_errors += 1;
                if consecutive_page_errors > MAX_CONSECUTIVE_PAGE_ERRORS {
                    return Err(error.context(format!(
                        "hosted SFT event paging failed {consecutive_page_errors} times in a row \
                         at cursor {upstream_cursor}"
                    )));
                }
                sleep(Duration::from_millis(750)).await;
                continue;
            }
        }
        let remote = client.get_run(&run_id).await?;
        let status = remote
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("running");
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_ok() && *cancel.borrow() {
                    let _ = client.cancel(&run_id).await;
                    append_status(&service, &run_id, "optimizer.run.cancelled", "cancelled").await?;
                    return Ok(());
                }
            }
            _ = sleep(Duration::from_millis(750)) => {}
        }
        if OptimizerRunStatus::str_is_terminal(status) {
            ingest::ingest_event_page(
                &service,
                &run_id,
                "sft",
                &client
                    .optimizer_events_after(&run_id, upstream_cursor, 2_000)
                    .await?,
                &mut upstream_cursor,
            )
            .await?;
            persist_remote_terminal(
                &service,
                &run_id,
                status,
                remote.get("error").and_then(Value::as_str),
            )
            .await?;
            return Ok(());
        }
    }
}

/// Terminal failure with a readable reason. `optimizer.run.failed` carrying an
/// empty delta tells a viewer nothing and hides producer-side success.
async fn append_failure(service: &OptimizerService, run_id: &str, reason: &str) -> Result<()> {
    // The event is what makes the run failed. Writing the status first and the
    // event second let a status exist with no evidence behind it.
    service
        .append_event_payloads(
            run_id.to_string(),
            vec![OptimizerEventDraft::new("optimizer.run.failed", "sft")
                .idempotency_key("hosted:optimizer.run.failed")
                .level("error")
                .delta(serde_json::Map::from_iter([(
                    "status".into(),
                    json!("failed"),
                )]))
                .error(json!({ "message": reason }))
                .raw(json!({"source": "hosted_sft"}))],
        )
        .await?;
    Ok(())
}

async fn append_status(
    service: &OptimizerService,
    run_id: &str,
    event_type: &str,
    status: &str,
) -> Result<()> {
    service
        .append_event_payloads(
            run_id.to_string(),
            vec![OptimizerEventDraft::new(event_type, "sft")
                .idempotency_key(format!("hosted:{event_type}"))
                .level("info")
                .delta(serde_json::Map::from_iter([(
                    "status".into(),
                    json!(status),
                )]))
                .raw(json!({"source": "hosted_sft"}))],
        )
        .await?;
    Ok(())
}

async fn persist_remote_terminal(
    service: &OptimizerService,
    run_id: &str,
    remote_status: &str,
    error: Option<&str>,
) -> Result<()> {
    // Backend P0-3 is the producer authority: it no longer emits `succeeded`.
    // The one remaining fold is `OptimizerRunStatus::parse` — do not keep a
    // second remote-status rewrite arm here.
    let status = OptimizerRunStatus::parse(remote_status)
        .with_context(|| format!("{remote_status} is not an OptimizerRunStatus"))?;
    let mapped = status.as_str();
    let mut run = service.get(run_id.to_string()).await?;
    if run.status != mapped {
        run.status = mapped.into();
    }
    if status != OptimizerRunStatus::Completed {
        if let Some(message) = error.filter(|value| !value.is_empty()) {
            run.error = Some(json!({"message": message}));
        } else if run.error.is_none() {
            run.error = Some(json!({"message": remote_status}));
        }
    }
    service.persist_run(run).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn training_length_is_never_left_to_the_checkpoint_list() {
        let toml = hosted_config_toml(
            "sft_b",
            "file_b",
            "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16",
            "http://127.0.0.1:9",
            "/tmp/train_a.jsonl",
            "sha256:deadbeef",
            "rollout",
            "eval.v1",
            "world:task@heldout",
        );
        assert!(toml.contains("training_steps ="));
        assert!(!toml.contains("n_epochs"));
    }

    #[test]
    fn hosted_toml_pins_campaigns_from_the_bound_container() {
        let toml = hosted_config_toml(
            "sft_train_a_ab12cd34",
            "file_train_train_a_ab12cd34",
            "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16",
            "http://127.0.0.1:9",
            "/tmp/train_a.jsonl",
            "sha256:content",
            "rollout",
            "eval.v1",
            "world:task@heldout",
        );
        assert!(toml.contains("backend = \"tinker\""));
        assert!(toml.contains("checkpoint_steps = [10, 20, 30]"));
        assert!(toml.contains("campaign_rollouts_per_checkpoint = 2"));
        assert!(toml.contains("checkpoint_evaluation_plan_ref = \"eval.v1\""));
        assert!(toml.contains("checkpoint_evaluation_world_ref = \"world:task@heldout\""));
        assert!(toml.contains("evaluation_phases = [\"baseline\", \"checkpoint\", \"final\"]"));
        assert!(toml.contains("evaluation_transport = \"tunnel\""));
        assert!(toml.contains("evaluation_required = true"));
        assert!(toml.contains("evaluation_exact_checkpoint_required = true"));
        assert!(toml.contains("evaluation_auth = \"sidecar_tunnel_lease\""));
        assert!(toml.contains("evaluation_sample_count = 16"));
        assert!(!toml.contains("goex.sft"));
    }

    #[test]
    fn shard_selection_is_allowlisted_not_a_path() {
        let error = materialize_dataset_shard("../../etc/passwd")
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown dataset shard"), "{error}");
        assert_eq!(DATASET_SHARDS.len(), 2);
    }

    #[test]
    fn hosted_sft_recipes_declare_an_explicit_cost_ceiling() {
        let recipe = hosted_tinker_recipe();
        assert_eq!(
            recipe["limits"]["costCeilingUsd"],
            HOSTED_SFT_COST_CEILING_USD
        );
        assert_eq!(HOSTED_SFT_COST_CEILING_USD, 10.0);
        assert_eq!(recipe["id"], HOSTED_SFT_RECIPE);
        assert_eq!(recipe["source"], "hosted");
    }

    #[test]
    fn identical_dataset_bytes_produce_the_same_content_digest() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("a.jsonl");
        let second = dir.path().join("run-specific-file-id.jsonl");
        let bytes = b"{\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}\n";
        std::fs::write(&first, bytes).unwrap();
        std::fs::write(&second, bytes).unwrap();
        let left = dataset_digest_for_path(&first).unwrap();
        let right = dataset_digest_for_path(&second).unwrap();
        assert_eq!(left, right);
        assert!(left.starts_with("sha256:"));
        assert_ne!(left, content_sha256(b"file_train_run_a"));
        let toml = hosted_config_toml(
            "sft_run_a",
            "file_train_run_a",
            "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16",
            "http://127.0.0.1:9",
            &first.to_string_lossy(),
            &left,
            "rollout",
            "eval.v1",
            "world:task@heldout",
        );
        assert!(toml.contains(&format!("dataset_digest = \"{left}\"")));
        assert!(!toml.contains("file_train_run_a") || toml.contains("training_file_id"));
    }

    #[test]
    fn hosted_toml_adapter_rank_agrees_with_s7_authority() {
        let toml = hosted_config_toml(
            "sft_b",
            "file_b",
            "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16",
            "http://127.0.0.1:9",
            "/tmp/train_a.jsonl",
            "sha256:abc",
            "rollout",
            "eval.v1",
            "world:task@heldout",
        );
        assert!(toml.contains("adapter = \"lora_r8\""), "{toml}");
        assert!(
            toml.contains("rank = 8") || toml.contains("[hyperparameters]\nrank = 8"),
            "{toml}"
        );
        assert!(!toml.contains("lora_r16"), "{toml}");
    }

    #[test]
    fn dataset_digest_is_stable_for_identical_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("train.jsonl");
        std::fs::write(&path, b"{\"messages\":[]}\n").unwrap();
        let first = dataset_digest_for_path(&path).unwrap();
        let second = dataset_digest_for_path(&path).unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with("sha256:"));
        crate::optimizers::sft_result::validate_dataset_digest(&path, &first).unwrap();
    }

    fn hosted_run(id: &str, status: &str, cursor: u64) -> OptimizerRunRecord {
        serde_json::from_value(json!({
            "schemaVersion": "optimizer_run.v1",
            "id": id,
            "algorithmId": "sft",
            "status": status,
            "source": "hosted",
            "createdAt": "2026-08-17T00:00:00Z",
            "cursorSeq": cursor,
            "summary": { "hostedMirror": { "cursor": cursor } }
        }))
        .unwrap()
    }

    #[test]
    fn restore_selects_nonterminal_hosted_sft_and_skips_terminal_or_attached() {
        let running = hosted_run("sft_live", "running", 12);
        let queued = hosted_run("sft_queued", "queued", 0);
        let done = hosted_run("sft_done", "completed", 40);
        let mut registered = HashSet::new();
        registered.insert("sft_queued".into());
        let restore = hosted_runs_needing_restore(&[running, queued, done], &registered);
        assert_eq!(restore, vec![("sft_live".into(), 12)]);
    }

    #[test]
    fn start_paths_do_not_dial_the_public_sft_loopback() {
        let production = include_str!("hosted_sft.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(!production.contains("client.base_url"));
        assert!(production.contains("admit_hosted"));
        assert!(production.contains("PLACEMENT_TRAINING_SFT_HOSTED"));
    }
}
