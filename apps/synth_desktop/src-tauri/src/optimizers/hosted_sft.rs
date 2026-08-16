//! Hosted SFT recipes backed by the public `synth-optimizers` control plane.
//!
//! Optimizers-beta remains an internal training executor. Workshop starts, watches,
//! cancels, and mirrors only public SFT runs before opening `optimizer.sft.live.v1`.

use super::{
    ingest,
    models::{
        OptimizerCapabilities, OptimizerCreateRequest, OptimizerExecutionBinding,
        OptimizerRecipeRunRequest, OptimizerResourceRef,
    },
    sft_client::SftOptimizerClient,
    OptimizerService,
};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::{sync::watch, time::sleep};

pub const HOSTED_SFT_FIXTURE_RECIPE: &str = "sft.hosted.fixture.v1";
pub const HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE: &str = "sft.craftax.nemotron-nano.tinker.v1";
pub const HOSTED_SFT_BANKING77_RECIPE: &str = "sft.banking77.nemotron-lightning.tinker.v1";
const LOCAL_CRAFTAX_SLOT: &str = "http://127.0.0.1:8098";
const LOCAL_BANKING77_SLOT: &str = "http://127.0.0.1:8110";
/// Checkpoint evaluation campaigns run against `banking77_classify`, whose
/// plan and world are the container's, not something this recipe invents.
const BANKING77_PLAN_REF: &str = "banking77_eval.v1";
const BANKING77_WORLD_REF: &str = "world:banking77@heldout";
const BANKING77_CHECKPOINT_STEPS: [u32; 3] = [10, 20, 30];
/// Length of training. `optimizers-beta` used to infer this as
/// `max(checkpoint_steps)`, so the checkpoint list silently decided how long a
/// run trained. It is now named separately and required.
const BANKING77_TRAINING_STEPS: u32 = 30;
const CRAFTAX_CHECKPOINT_STEPS: [u32; 3] = [16, 33, 66];
/// One pass over the proven 131-row corpus at batch size 2. Tinker samples
/// with replacement, so this is a named optimizer-step budget, not an epoch.
const CRAFTAX_TRAINING_STEPS: u32 = 66;
const BANKING77_CAMPAIGN_ROLLOUTS: u32 = 2;
/// Rows longer than this lose their assistant tokens and are dropped, so the
/// cap decides which of a dataset is trained on. Banking77 rows are single
/// utterances; Craftax ReAct transcripts grow with the episode and need far more.
const BANKING77_MAX_SEQ_LEN: u32 = 4096;
const CRAFTAX_MAX_SEQ_LEN: u32 = 16384;
/// Share of rows allowed to exceed the cap before the run refuses rather than
/// training on whatever happened to be short enough.
const MAX_DROPPED_FRACTION: &str = "0.05";
/// Seconds one checkpoint rollout may take. A Craftax episode runs minutes; the
/// evaluator's HTTP client would otherwise fall back to a 30s default.
const CHECKPOINT_EVALUATION_TIMEOUT_S: u32 = 3600;
/// Allowlisted dataset shards. A caller selects one; it cannot supply a path.
const BANKING77_SHARDS: [&str; 2] = ["train_a", "train_b"];
/// Torn-tail reads while the producer appends are transient. Give up only
/// after the upstream stays unreadable across this many consecutive polls.
const MAX_CONSECUTIVE_PAGE_ERRORS: u32 = 20;

pub fn recipe_catalog() -> Vec<Value> {
    vec![
        fixture_recipe(),
        craftax_nemotron_recipe(),
        banking77_recipe(),
    ]
}

fn fixture_recipe() -> Value {
    let availability = match SftOptimizerClient::from_env() {
        Ok(_) => "available",
        Err(_) => "unavailable",
    };
    json!({
        "id": HOSTED_SFT_FIXTURE_RECIPE,
        "title": "Hosted SFT fixture",
        "algorithmId": "sft",
        "task": "hosted",
        "availability": availability,
        "limits": {
            "backend": "fixture",
            "checkpointSteps": [10, 20],
            "campaignRolloutsPerCheckpoint": 2,
            "costCeilingUsd": null,
            "costNotice": "Fixture backend; no provider charges. Requires the public Optimizers SFT service."
        },
        "credentialInputs": [],
        "prerequisites": ["SYNTH_OPTIMIZERS_SFT_SERVICE_URL", "SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN"],
    })
}

fn craftax_nemotron_recipe() -> Value {
    let catalog_ok = super::tinker_catalog::TinkerBaseModelCatalog::load().is_ok();
    let availability = if catalog_ok && SftOptimizerClient::from_env().is_ok() {
        "available"
    } else {
        "unavailable"
    };
    json!({
        "id": HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE,
        "title": "Craftax Nemotron 3.5 Lightning Tinker SFT",
        "algorithmId": "sft",
        "task": "craftax",
        "availability": availability,
        "limits": {
            "backend": "tinker",
            "checkpointSteps": CRAFTAX_CHECKPOINT_STEPS,
            "trainingSteps": CRAFTAX_TRAINING_STEPS,
            "campaignRolloutsPerCheckpoint": 2,
            "evalSeeds": [501, 502],
            "costCeilingUsd": null,
            "costNotice": "Hosted Tinker + local Craftax slot. Student id from docs/sft_tinker_base_models.toml (default 3.5 Lightning)."
        },
        "credentialInputs": [],
        "prerequisites": [
            "SYNTH_OPTIMIZERS_SFT_SERVICE_URL (or local http://127.0.0.1:8878)",
            "SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN",
            "TINKER_API_KEY held by the Optimizers-beta executor",
            "Craftax gold / GameBench on 127.0.0.1:8098"
        ],
    })
}

fn banking77_recipe() -> Value {
    let catalog_ok = super::tinker_catalog::TinkerBaseModelCatalog::load().is_ok();
    let availability =
        if catalog_ok && SftOptimizerClient::from_env().is_ok() && banking77_source().is_ok() {
            "available"
        } else {
            "unavailable"
        };
    json!({
        "id": HOSTED_SFT_BANKING77_RECIPE,
        "title": "Banking77 Nemotron Lightning Tinker SFT",
        "algorithmId": "sft",
        "task": "banking77",
        "availability": availability,
        "limits": {
            "backend": "tinker",
            "checkpointSteps": BANKING77_CHECKPOINT_STEPS,
            "campaignRolloutsPerCheckpoint": BANKING77_CAMPAIGN_ROLLOUTS,
            "datasetShards": BANKING77_SHARDS,
            "evalSeeds": [1, 2],
            "evaluationPlanRef": BANKING77_PLAN_REF,
            "costCeilingUsd": null,
            "costNotice": "Hosted Tinker training plus banking77_classify campaign rollouts. Provider charges apply."
        },
        "credentialInputs": [],
        "prerequisites": [
            "SYNTH_OPTIMIZERS_SFT_SERVICE_URL (or local http://127.0.0.1:8878)",
            "SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN",
            "TINKER_API_KEY held by the Optimizers-beta executor",
            "SYNTH_SFT_BANKING77_TRAIN_JSONL",
            "banking77_classify container on 127.0.0.1:8110"
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
    match request.recipe_id.as_str() {
        HOSTED_SFT_FIXTURE_RECIPE => start_fixture(service, request).await,
        HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE => start_craftax_nemotron(service, request).await,
        HOSTED_SFT_BANKING77_RECIPE => start_banking77(service, request).await,
        other => bail!("unknown hosted SFT recipe: {other}"),
    }
}

/// Pinned Banking77 SFT corpus. The recipe owns the path; a caller picks only
/// which allowlisted shard to train on.
fn banking77_source() -> Result<std::path::PathBuf> {
    let raw = std::env::var("SYNTH_SFT_BANKING77_TRAIN_JSONL")
        .context("Banking77 SFT requires SYNTH_SFT_BANKING77_TRAIN_JSONL")?;
    let path = std::path::PathBuf::from(raw.trim());
    if !path.is_file() {
        bail!("Banking77 SFT corpus is not a file: {}", path.display());
    }
    Ok(path)
}

fn banking77_slot_url() -> String {
    std::env::var("SYNTH_CONTAINERS_BANKING77_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| LOCAL_BANKING77_SLOT.into())
}

/// Materialize one allowlisted shard under the instance data root. Two shards
/// are disjoint halves of the pinned corpus, so two runs train on genuinely
/// different data and the producer digests them differently.
fn materialize_banking77_shard(shard: &str) -> Result<std::path::PathBuf> {
    let index = BANKING77_SHARDS
        .iter()
        .position(|candidate| *candidate == shard)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "unknown Banking77 dataset shard {shard}; allowed: {}",
                BANKING77_SHARDS.join(", ")
            )
        })?;
    let source = banking77_source()?;
    let text = std::fs::read_to_string(&source).context("read Banking77 SFT corpus")?;
    let rows: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if rows.len() < BANKING77_SHARDS.len() {
        bail!("Banking77 SFT corpus is too small to shard");
    }
    let half = rows.len() / BANKING77_SHARDS.len();
    let start = index * half;
    let end = if index + 1 == BANKING77_SHARDS.len() {
        rows.len()
    } else {
        start + half
    };
    let destination_dir = crate::instance::data_root().join("optimizers/datasets/banking77");
    std::fs::create_dir_all(&destination_dir).context("create Banking77 shard directory")?;
    let destination = destination_dir.join(format!("{shard}.jsonl"));
    let body = format!("{}\n", rows[start..end].join("\n"));
    std::fs::write(&destination, body).context("write Banking77 shard")?;
    Ok(destination)
}

async fn start_banking77(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    let catalog = super::tinker_catalog::TinkerBaseModelCatalog::load()?;
    let model_id = catalog.resolve(request.base_model.as_deref())?;
    let client = SftOptimizerClient::from_env()?;
    let shard = request
        .dataset_shard
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(BANKING77_SHARDS[0])
        .to_string();
    let shard_path = materialize_banking77_shard(&shard)?;
    let container_url = banking77_slot_url();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("sft_banking77_{}_{}", shard, &suffix[..8]);
    let training_file = format!("file_train_banking77_{shard}_{}", &suffix[..8]);
    let config_toml = banking77_config_toml(
        &run_id,
        &training_file,
        &model_id,
        &container_url,
        &shard_path.to_string_lossy(),
    );
    let create = OptimizerCreateRequest {
        algorithm_id: "sft".into(),
        algorithm_version: Some("banking77-nemotron-lightning-tinker-v1".into()),
        objective: Some(
            "Banking77 intent SFT · hosted Tinker · banking77_classify checkpoint campaigns".into(),
        ),
        source: Some("hosted".into()),
        project_ref: Some("banking77@nemotron-lightning-tinker".into()),
        session_ref: request.session_ref,
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![
            OptimizerExecutionBinding {
                kind: "synth_optimizers_sft".into(),
                id: client.base_url.clone(),
                label: Some("public Optimizers hosted SFT".into()),
                status: Some("starting".into()),
                metadata: json!({
                    "recipeId": HOSTED_SFT_BANKING77_RECIPE,
                    "backend": "tinker",
                    "datasetFile": training_file,
                    "datasetShard": shard,
                    "baseModel": model_id,
                }),
            },
            OptimizerExecutionBinding {
                kind: "local_slot".into(),
                id: container_url.clone(),
                label: Some("banking77_classify container".into()),
                status: Some("leased".into()),
                metadata: json!({
                    "role": "checkpoint_evaluation",
                    "worldRef": BANKING77_WORLD_REF,
                    "evaluationPlanRef": BANKING77_PLAN_REF,
                }),
            },
        ]),
        input_refs: Some(vec![
            OptimizerResourceRef {
                kind: "recipe".into(),
                id: HOSTED_SFT_BANKING77_RECIPE.into(),
                digest: None,
                role: Some("configuration".into()),
                title: Some("Banking77 Nemotron Lightning Tinker SFT".into()),
                metadata: json!({"backend": "tinker", "baseModel": model_id}),
            },
            OptimizerResourceRef {
                kind: "dataset".into(),
                id: training_file.clone(),
                digest: None,
                role: Some("train".into()),
                title: Some(format!("Banking77 SFT corpus · shard {shard}")),
                metadata: json!({"shard": shard, "shards": BANKING77_SHARDS}),
            },
        ]),
        capabilities: Some(OptimizerCapabilities::for_algorithm("sft")),
        summary: Some(json!({
            "recipeId": HOSTED_SFT_BANKING77_RECIPE,
            "backend": "tinker",
            "producer": "synth-optimizers",
            "baseModel": model_id,
            "datasetShard": shard,
            "localSlot": container_url,
            "checkpointSteps": BANKING77_CHECKPOINT_STEPS,
        })),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    let (run, event) = service.create(create).await?;
    spawn_hosted_worker(service, client, run_id, config_toml).await;
    Ok((run, event))
}

fn banking77_config_toml(
    run_id: &str,
    training_file: &str,
    model_id: &str,
    container_url: &str,
    training_jsonl: &str,
) -> String {
    let steps = BANKING77_CHECKPOINT_STEPS
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"run_id = "{run_id}"
backend = "tinker"
base_model = "{model_id}"
adapter = "lora_r16"
training_file_id = "{training_file}"
training_jsonl = "{training_jsonl}"
selection_file_id = "file_selection"
heldout_file_id = "file_heldout"
accelerator_slots = 1
checkpoint_steps = [{steps}]
training_steps = {BANKING77_TRAINING_STEPS}
max_seq_len = {BANKING77_MAX_SEQ_LEN}
max_dropped_fraction = {MAX_DROPPED_FRACTION}
campaign_rollouts_per_checkpoint = {BANKING77_CAMPAIGN_ROLLOUTS}
evaluator_version = "{BANKING77_PLAN_REF}"
container_url = "{container_url}"
checkpoint_evaluation_seeds = [1, 2]
checkpoint_evaluation_policy_harness = "classify"
checkpoint_evaluation_plan_ref = "{BANKING77_PLAN_REF}"
checkpoint_evaluation_world_ref = "{BANKING77_WORLD_REF}"
checkpoint_evaluation_timeout_s = {CHECKPOINT_EVALUATION_TIMEOUT_S}

# The classify harness resolves the checkpoint through evaluator-owned keys
# (inference_target, sampler_path, …), which this recipe must not set. It still
# has to declare a policy: an empty one is refused, because the policy under
# test is never defaulted.
[checkpoint_evaluation_policy]
api_key_env = "TINKER_API_KEY"

[hyperparameters]
rank = 8
batch_size = 2
lr = 0.001
"#
    )
}

async fn start_fixture(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    let client = SftOptimizerClient::from_env()?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("sft_hosted_{}", &suffix[..8]);
    let training_file = format!("file_train_{}", &suffix[..8]);
    let config_toml = fixture_config_toml(&run_id, &training_file);
    let create = OptimizerCreateRequest {
        algorithm_id: "sft".into(),
        algorithm_version: Some("hosted-fixture-v1".into()),
        objective: Some("Hosted SFT fixture · streamed from public Optimizers".into()),
        source: Some("hosted".into()),
        project_ref: Some("sft@hosted-fixture".into()),
        session_ref: request.session_ref,
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![OptimizerExecutionBinding {
            kind: "synth_optimizers_sft".into(),
            id: client.base_url.clone(),
            label: Some("public Optimizers hosted SFT".into()),
            status: Some("starting".into()),
            metadata: json!({
                "recipeId": HOSTED_SFT_FIXTURE_RECIPE,
                "backend": "fixture",
                "datasetFile": training_file,
            }),
        }]),
        input_refs: Some(vec![OptimizerResourceRef {
            kind: "recipe".into(),
            id: HOSTED_SFT_FIXTURE_RECIPE.into(),
            digest: None,
            role: Some("configuration".into()),
            title: Some("Hosted SFT fixture".into()),
            metadata: json!({"backend": "fixture"}),
        }]),
        capabilities: Some(OptimizerCapabilities::for_algorithm("sft")),
        summary: Some(json!({
            "recipeId": HOSTED_SFT_FIXTURE_RECIPE,
            "backend": "fixture",
            "producer": "synth-optimizers",
        })),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    let (run, event) = service.create(create).await?;
    spawn_hosted_worker(service, client, run_id, config_toml).await;
    Ok((run, event))
}

async fn start_craftax_nemotron(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    let catalog = super::tinker_catalog::TinkerBaseModelCatalog::load()?;
    let model_id = catalog.resolve(request.base_model.as_deref())?;
    let client = SftOptimizerClient::from_env()?;
    let container_url = local_craftax_slot_url()?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("sft_craftax_nemo_{}", &suffix[..8]);
    let training_file = format!("file_train_{}", &suffix[..8]);
    let training_jsonl = std::env::var("SYNTH_SFT_TRAIN_JSONL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let config_toml = craftax_nemotron_config_toml(
        &run_id,
        &training_file,
        &model_id,
        &container_url,
        training_jsonl.as_deref(),
    );
    let create = OptimizerCreateRequest {
        algorithm_id: "sft".into(),
        algorithm_version: Some("craftax-nemotron-nano-tinker-v1".into()),
        objective: Some(
            "Craftax Nemotron 3.5 Lightning Tinker SFT · streamed from public Optimizers".into(),
        ),
        source: Some("hosted".into()),
        project_ref: Some("craftax@nemotron-nano-tinker".into()),
        session_ref: request.session_ref,
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![
            OptimizerExecutionBinding {
                kind: "synth_optimizers_sft".into(),
                id: client.base_url.clone(),
                label: Some("public Optimizers hosted SFT".into()),
                status: Some("starting".into()),
                metadata: json!({
                    "recipeId": HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE,
                    "backend": "tinker",
                    "datasetFile": training_file,
                    "baseModel": model_id,
                }),
            },
            OptimizerExecutionBinding {
                kind: "local_slot".into(),
                id: container_url.clone(),
                label: Some("Craftax local slot".into()),
                status: Some("leased".into()),
                metadata: json!({
                    "role": "checkpoint_evaluation",
                    "worldRef": "world:craftax",
                }),
            },
        ]),
        input_refs: Some(vec![OptimizerResourceRef {
            kind: "recipe".into(),
            id: HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE.into(),
            digest: None,
            role: Some("configuration".into()),
            title: Some("Craftax Nemotron 3.5 Lightning Tinker SFT".into()),
            metadata: json!({"backend": "tinker", "baseModel": model_id}),
        }]),
        capabilities: Some(OptimizerCapabilities::for_algorithm("sft")),
        summary: Some(json!({
            "recipeId": HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE,
            "backend": "tinker",
            "producer": "synth-optimizers",
            "baseModel": model_id,
            "localSlot": container_url,
            "checkpointSteps": CRAFTAX_CHECKPOINT_STEPS,
            "trainingSteps": CRAFTAX_TRAINING_STEPS,
        })),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    let (run, event) = service.create(create).await?;
    spawn_hosted_worker(service, client, run_id, config_toml).await;
    Ok((run, event))
}

fn local_craftax_slot_url() -> Result<String> {
    let url = std::env::var("CRAFTAX_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| LOCAL_CRAFTAX_SLOT.into());
    let host_port = url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    let address: std::net::SocketAddr = host_port
        .parse()
        .with_context(|| format!("Craftax local slot URL is not a loopback host:port: {url}"))?;
    if !address.ip().is_loopback() {
        bail!("Craftax local slot must be loopback; got {url}");
    }
    if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(400)).is_err() {
        bail!(
            "Craftax local slot is not listening at {url}; start gold Craftax or set CRAFTAX_URL"
        );
    }
    Ok(url)
}

async fn spawn_hosted_worker(
    service: &OptimizerService,
    client: SftOptimizerClient,
    run_id: String,
    config_toml: String,
) {
    let (cancel_tx, cancel_rx) = watch::channel(false);
    service
        .register_local_recipe(run_id.clone(), cancel_tx)
        .await;
    let worker = service.clone();
    tokio::spawn(async move {
        if let Err(error) = run_hosted_worker(
            worker.clone(),
            client,
            run_id.clone(),
            config_toml,
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

async fn run_hosted_worker(
    service: OptimizerService,
    client: SftOptimizerClient,
    run_id: String,
    config_toml: String,
    mut cancel: watch::Receiver<bool>,
) -> Result<()> {
    client.submit_toml(&run_id, &config_toml).await?;
    let mut upstream_cursor = 0u64;
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
        if matches!(status, "succeeded" | "failed" | "cancelled") {
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

fn fixture_config_toml(run_id: &str, training_file: &str) -> String {
    format!(
        r#"run_id = "{run_id}"
backend = "fixture"
base_model = "openai/gpt-oss-20b"
adapter = "lora_r16"
training_file_id = "{training_file}"
selection_file_id = "file_selection"
heldout_file_id = "file_heldout"
accelerator_slots = 1
checkpoint_steps = [10, 20]
campaign_rollouts_per_checkpoint = 2
evaluator_version = "hosted_fixture.v1"
"#
    )
}

fn craftax_nemotron_config_toml(
    run_id: &str,
    training_file: &str,
    model_id: &str,
    container_url: &str,
    training_jsonl: Option<&str>,
) -> String {
    let checkpoint_steps = CRAFTAX_CHECKPOINT_STEPS
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let training_jsonl_line = training_jsonl
        .map(|path| format!("training_jsonl = \"{path}\"\n"))
        .unwrap_or_default();
    format!(
        r#"run_id = "{run_id}"
backend = "tinker"
base_model = "{model_id}"
adapter = "lora_r16"
training_file_id = "{training_file}"
selection_file_id = "file_selection"
heldout_file_id = "file_heldout"
{training_jsonl_line}accelerator_slots = 1
checkpoint_steps = [{checkpoint_steps}]
training_steps = {CRAFTAX_TRAINING_STEPS}
max_seq_len = {CRAFTAX_MAX_SEQ_LEN}
max_dropped_fraction = {MAX_DROPPED_FRACTION}
campaign_rollouts_per_checkpoint = 2
evaluator_version = "craftax_gamebench.v1"
container_url = "{container_url}"
checkpoint_evaluation_seeds = [501, 502]
checkpoint_evaluation_policy_harness = "react"
checkpoint_evaluation_plan_ref = "craftax_eval.v1"
checkpoint_evaluation_world_ref = "world:craftax"
checkpoint_evaluation_timeout_s = {CHECKPOINT_EVALUATION_TIMEOUT_S}

# Every field the ReAct harness needs, named. It refuses to default any of
# them: they define the policy under test, and a wrong one is indistinguishable
# from a failing checkpoint downstream.
[checkpoint_evaluation_policy]
provider = "tinker"
api_key_env = "TINKER_API_KEY"
effort = "medium"
max_tokens = 1024
parse_retries = 0
context_token_budget = 16000
compact_at = 0.7
keep_recent_messages = 8
keep_recent_frames = 2
observation_mode = "text"
sampler_ready_timeout_s = 300
system_prompt = "You play Craftax. Reply with JSON only."

[hyperparameters]
rank = 8
batch_size = 2
lr = 0.001
"#
    )
}

/// Terminal failure with a readable reason. `optimizer.run.failed` carrying an
/// empty delta tells a viewer nothing and hides producer-side success.
async fn append_failure(service: &OptimizerService, run_id: &str, reason: &str) -> Result<()> {
    let mut run = service.get(run_id.to_string()).await?;
    run.status = "failed".into();
    service.persist_run(run).await?;
    let seq = service.get(run_id.to_string()).await?.cursor_seq + 1;
    service
        .append_events(
            run_id.to_string(),
            vec![super::models::OptimizerEventEnvelope {
                schema_version: super::models::OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
                event_id: Some(format!("{run_id}:hosted:optimizer.run.failed")),
                event_type: "optimizer.run.failed".into(),
                sequence_number: seq,
                occurred_at: chrono::Utc::now().to_rfc3339(),
                optimizer_run_id: run_id.into(),
                algorithm_id: "sft".into(),
                level: Some("error".into()),
                item: None,
                delta: serde_json::Map::from_iter([("status".into(), json!("failed"))]),
                snapshot: None,
                usage_delta: None,
                artifact_refs: vec![],
                error: Some(json!({ "message": reason })),
                raw: json!({"source": "hosted_sft"}),
            }],
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
    let mut run = service.get(run_id.to_string()).await?;
    run.status = status.into();
    service.persist_run(run).await?;
    let seq = service.get(run_id.to_string()).await?.cursor_seq + 1;
    service
        .append_events(
            run_id.to_string(),
            vec![super::models::OptimizerEventEnvelope {
                schema_version: super::models::OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
                event_id: Some(format!("{run_id}:hosted:{event_type}")),
                event_type: event_type.into(),
                sequence_number: seq,
                occurred_at: chrono::Utc::now().to_rfc3339(),
                optimizer_run_id: run_id.into(),
                algorithm_id: "sft".into(),
                level: Some("info".into()),
                item: None,
                delta: serde_json::Map::from_iter([("status".into(), json!(status))]),
                snapshot: None,
                usage_delta: None,
                artifact_refs: vec![],
                error: None,
                raw: json!({"source": "hosted_sft"}),
            }],
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
    let mapped = match remote_status {
        "succeeded" => "completed",
        other => other,
    };
    let mut run = service.get(run_id.to_string()).await?;
    if run.status != mapped {
        run.status = mapped.into();
    }
    if mapped != "completed" {
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
        // `steps = max(checkpoint_steps)` used to be the training length, so a
        // checkpoint list decided how long a run trained.
        for (label, toml) in [
            (
                "banking77",
                banking77_config_toml(
                    "sft_b",
                    "file_b",
                    "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16",
                    "http://127.0.0.1:8110",
                    "/tmp/train_a.jsonl",
                ),
            ),
            (
                "craftax",
                craftax_nemotron_config_toml(
                    "sft_c",
                    "file_c",
                    "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16",
                    "http://127.0.0.1:8098",
                    Some("/tmp/craftax.jsonl"),
                ),
            ),
        ] {
            assert!(
                toml.contains("training_steps ="),
                "{label} must name its training length"
            );
            // n_epochs is never read by the Tinker loop; batches are sampled
            // with replacement. Carrying it implies a schedule that never runs.
            assert!(
                !toml.contains("n_epochs"),
                "{label} still sets n_epochs, which the Tinker loop ignores"
            );
        }
    }

    #[test]
    fn fixture_toml_is_algorithm_sft_not_goex_plugin() {
        let toml = fixture_config_toml("sft_hosted_ab12cd34", "file_train_ab12cd34");
        assert!(toml.contains("backend = \"fixture\""));
        assert!(toml.contains("run_id = \"sft_hosted_ab12cd34\""));
        assert!(!toml.contains("goex.sft"));
        assert!(!toml.contains("go-ex"));
    }

    #[test]
    fn banking77_toml_pins_campaigns_through_banking77_classify() {
        let toml = banking77_config_toml(
            "sft_banking77_train_a_ab12cd34",
            "file_train_banking77_train_a_ab12cd34",
            "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16",
            "http://127.0.0.1:8110",
            "/tmp/train_a.jsonl",
        );
        assert!(toml.contains("backend = \"tinker\""));
        assert!(toml.contains("checkpoint_steps = [10, 20, 30]"));
        assert!(toml.contains("campaign_rollouts_per_checkpoint = 2"));
        assert!(toml.contains("checkpoint_evaluation_plan_ref = \"banking77_eval.v1\""));
        assert!(toml.contains("checkpoint_evaluation_world_ref = \"world:banking77@heldout\""));
        assert!(toml.contains("container_url = \"http://127.0.0.1:8110\""));
        assert!(!toml.contains("goex.sft"));
    }

    #[test]
    fn banking77_shard_selection_is_allowlisted_not_a_path() {
        let error = materialize_banking77_shard("../../etc/passwd")
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown Banking77 dataset shard"), "{error}");
        assert_eq!(BANKING77_SHARDS.len(), 2);
    }

    #[test]
    fn craftax_nemotron_recipe_is_unavailable_without_public_sft_token() {
        let recipe = craftax_nemotron_recipe();
        assert_eq!(recipe["id"], HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE);
        assert_eq!(recipe["algorithmId"], "sft");
        assert_eq!(recipe["limits"]["backend"], "tinker");
        assert_ne!(recipe["id"], "goex.sft.v1");
        if std::env::var("SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_none()
        {
            assert_eq!(recipe["availability"], "unavailable");
        }
    }

    #[test]
    fn craftax_nemotron_toml_is_tinker_not_goex() {
        let toml = craftax_nemotron_config_toml(
            "sft_craftax_nemo_ab12cd34",
            "file_train_ab12cd34",
            "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16",
            "http://127.0.0.1:8098",
            None,
        );
        assert!(toml.contains("backend = \"tinker\""));
        assert!(toml.contains("base_model = \"nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16\""));
        assert!(toml.contains("evaluator_version = \"craftax_gamebench.v1\""));
        assert!(toml.contains("checkpoint_evaluation_seeds = [501, 502]"));
        assert!(toml.contains("checkpoint_steps = [16, 33, 66]"));
        assert!(toml.contains("training_steps = 66"));
        assert!(toml.contains("world:craftax"));
        assert!(!toml.contains("goex.sft"));
        assert!(!toml.contains("UNPINNED"));
        assert!(!toml.contains("nvidia/nemotron-3-nano-30b-a3b"));
    }
}
