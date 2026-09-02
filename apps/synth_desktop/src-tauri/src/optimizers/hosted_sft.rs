//! Hosted SFT recipes backed by the public `synth-optimizers` `sft service`
//! (`TinkerSftExecutor`). Workshop starts, watches, cancels, and mirrors only
//! public SFT runs before opening `optimizer.sft.live.v1`.

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

pub const HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE: &str = "sft.craftax.nemotron-nano.tinker.v1";
pub const HOSTED_SFT_BANKING77_RECIPE: &str = "sft.banking77.nemotron-lightning.tinker.v1";
const LOCAL_CRAFTAX_SLOT: &str = "http://127.0.0.1:8098";
const LOCAL_BANKING77_SLOT: &str = "http://127.0.0.1:8110";
/// Checkpoint evaluation campaigns run against `banking77_classify`, whose
/// plan and world are the container's, not something this recipe invents.
const BANKING77_PLAN_REF: &str = "banking77_eval.v1";
const BANKING77_WORLD_REF: &str = "world:banking77@heldout";
const BANKING77_CHECKPOINT_STEPS: [u32; 4] = [25, 50, 75, 100];
/// Length of training. `optimizers-beta` used to infer this as
/// `max(checkpoint_steps)`, so the checkpoint list silently decided how long a
/// run trained. It is now named separately and required.
const BANKING77_TRAINING_STEPS: u32 = 100;
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
/// The Workshop approval broker needs a hard per-run authorization ceiling.
/// Hosted SFT recipes are fixed-shape (model, steps, checkpoints, and rollout
/// counts are product-owned), so cap each paid launch at one fifth of the
/// five-run acceptance budget. The public service remains the execution
/// authority and Workshop reconciles actual usage from its event stream.
const HOSTED_SFT_COST_CEILING_USD: f64 = 15.0;
/// Allowlisted dataset shards. A caller selects one; it cannot supply a path.
const BANKING77_SHARDS: [&str; 2] = ["train_a", "train_b"];
/// Torn-tail reads while the producer appends are transient. Give up only
/// after the upstream stays unreadable across this many consecutive polls.
const MAX_CONSECUTIVE_PAGE_ERRORS: u32 = 20;
const HOSTED_SFT_LORA_RANK: u64 = 16;
const BANKING77_SPLIT_SEED: u64 = 20260907;
const BANKING77_SELECTION_SEED: u64 = 20260908;
const BANKING77_HELDOUT_SEED: u64 = 20260906;
const BANKING77_SELECTION_SIZE: u32 = 400;
const BANKING77_HELDOUT_SIZE: u32 = 400;

pub fn recipe_catalog() -> Vec<Value> {
    vec![craftax_nemotron_recipe(), banking77_recipe()]
}

fn craftax_nemotron_recipe() -> Value {
    let catalog_ok = super::tinker_catalog::TinkerBaseModelCatalog::load().is_ok();
    let service_reason = public_sft_service_reason();
    let available = catalog_ok && service_reason.is_none();
    let availability_reason = if available {
        Value::Null
    } else if let Some(reason) = service_reason {
        json!(reason)
    } else {
        json!("Hosted Tinker base-model catalog is unavailable")
    };
    json!({
        "id": HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE,
        "title": "Craftax Nemotron 3.5 Lightning Tinker SFT",
        "algorithmId": "sft",
        "task": "craftax",
        "availability": if available { "available" } else { "unavailable" },
        "availabilityReason": availability_reason,
        "limits": {
            "backend": "tinker",
            "checkpointSteps": CRAFTAX_CHECKPOINT_STEPS,
            "evaluationPlan": { "phases": ["baseline", "checkpoint", "final"], "checkpointSteps": CRAFTAX_CHECKPOINT_STEPS, "transport": "tunnel", "metric": "reward" },
            "trainingSteps": CRAFTAX_TRAINING_STEPS,
            "campaignRolloutsPerCheckpoint": 2,
            "evalSeeds": [501, 502],
            "costCeilingUsd": HOSTED_SFT_COST_CEILING_USD,
            "costNotice": "Hosted Tinker + local Craftax slot. Student id from docs/sft_tinker_base_models.toml (default 3.5 Lightning)."
        },
        "credentialInputs": [],
        "prerequisites": [
            "synth-optimizers sft service --db … --bind 127.0.0.1:8878",
            "SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN",
            "SYNTH_OPTIMIZERS_SFT_SERVICE_URL",
            "SYNTH_OPTIMIZERS_SFT_FIXTURE=1 for unpaid",
            "Craftax gold / GameBench on 127.0.0.1:8098"
        ],
    })
}

fn banking77_recipe() -> Value {
    let catalog_ok = super::tinker_catalog::TinkerBaseModelCatalog::load().is_ok();
    let service_reason = public_sft_service_reason();
    let jsonl_ok = banking77_source().is_ok() || banking77_reference_sources().is_ok();
    let available = catalog_ok && service_reason.is_none() && jsonl_ok;
    let availability_reason = if available {
        Value::Null
    } else if let Some(reason) = service_reason {
        json!(reason)
    } else if !jsonl_ok {
        json!("Set SYNTH_BANKING77_TRAIN_CSV and SYNTH_BANKING77_HELDOUT_CSV for the NanoClassify reference split, or SYNTH_SFT_BANKING77_TRAIN_JSONL for a smoke corpus")
    } else {
        json!("Hosted Tinker base-model catalog is unavailable")
    };
    let reference_mode = banking77_reference_sources().is_ok();
    json!({
        "id": HOSTED_SFT_BANKING77_RECIPE,
        "title": if reference_mode { "Banking77 GPT-OSS 20B Tinker SFT" } else { "Banking77 Nemotron Lightning Tinker SFT" },
        "algorithmId": "sft",
        "task": "banking77",
        "availability": if available { "available" } else { "unavailable" },
        "availabilityReason": availability_reason,
        "limits": {
            "backend": "tinker",
            "checkpointSteps": BANKING77_CHECKPOINT_STEPS,
            "trainingSteps": BANKING77_TRAINING_STEPS,
            "batchSize": 64,
            "rank": HOSTED_SFT_LORA_RANK,
            "selectionExamples": BANKING77_SELECTION_SIZE,
            "heldoutExamples": BANKING77_HELDOUT_SIZE,
            "evaluationPlan": { "phases": ["baseline", "checkpoint", "final"], "checkpointSteps": BANKING77_CHECKPOINT_STEPS, "transport": "tunnel", "metric": "reward" },
            "campaignRolloutsPerCheckpoint": BANKING77_CAMPAIGN_ROLLOUTS,
            "datasetShards": BANKING77_SHARDS,
            "evalSeeds": [1, 2],
            "evaluationPlanRef": BANKING77_PLAN_REF,
            "costCeilingUsd": HOSTED_SFT_COST_CEILING_USD,
            "costNotice": "Hosted Tinker training plus banking77_classify campaign rollouts. Provider charges apply."
        },
        "credentialInputs": [],
        "prerequisites": [
            "synth-optimizers sft service --db … --bind 127.0.0.1:8878",
            "SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN",
            "SYNTH_OPTIMIZERS_SFT_SERVICE_URL",
            "SYNTH_OPTIMIZERS_SFT_FIXTURE=1 for unpaid",
            "SYNTH_BANKING77_TRAIN_CSV + SYNTH_BANKING77_HELDOUT_CSV (reference)",
            "optional SYNTH_BANKING77_HELDOUT_INDICES_JSON for sealed membership",
            "banking77_classify container on 127.0.0.1:8110"
        ],
    })
}

fn public_sft_service_reason() -> Option<String> {
    if SftOptimizerClient::from_env().is_ok() {
        return None;
    }
    if std::env::var("SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .is_none()
    {
        return Some(
            "SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN is required to reach the public SFT service."
                .into(),
        );
    }
    if std::env::var("SYNTH_OPTIMIZERS_SFT_SERVICE_URL")
        .ok()
        .map(|value| value.trim().is_empty())
        .unwrap_or(false)
    {
        return Some("SYNTH_OPTIMIZERS_SFT_SERVICE_URL is empty.".into());
    }
    Some(
        "Public SFT service client is not configured (SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN / SYNTH_OPTIMIZERS_SFT_SERVICE_URL)."
            .into(),
    )
}

pub async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    match request.recipe_id.as_str() {
        HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE => start_craftax_nemotron(service, request).await,
        HOSTED_SFT_BANKING77_RECIPE => start_banking77(service, request).await,
        other => bail!("unknown hosted SFT recipe: {other}"),
    }
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

#[derive(Clone, Debug)]
pub(crate) struct Banking77ReferenceSources {
    pub(crate) train_csv: std::path::PathBuf,
    pub(crate) heldout_csv: std::path::PathBuf,
    pub(crate) heldout_indices_json: Option<std::path::PathBuf>,
}

pub(crate) fn banking77_reference_sources() -> Result<Banking77ReferenceSources> {
    let required = |name: &str| -> Result<std::path::PathBuf> {
        let raw = std::env::var(name).with_context(|| format!("{name} is required"))?;
        let path = std::path::PathBuf::from(raw.trim());
        if !path.is_file() {
            bail!("{name} is not a file: {}", path.display());
        }
        Ok(path)
    };
    let heldout_indices_json = std::env::var("SYNTH_BANKING77_HELDOUT_INDICES_JSON")
        .ok()
        .map(|raw| std::path::PathBuf::from(raw.trim()))
        .filter(|path| path.is_file());
    Ok(Banking77ReferenceSources {
        train_csv: required("SYNTH_BANKING77_TRAIN_CSV")?,
        heldout_csv: required("SYNTH_BANKING77_HELDOUT_CSV")?,
        heldout_indices_json,
    })
}

fn banking77_slot_url() -> String {
    std::env::var("SYNTH_CONTAINERS_BANKING77_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| LOCAL_BANKING77_SLOT.into())
}

fn normalize_banking77_row(line: &str) -> Result<String> {
    let mut row: serde_json::Value =
        serde_json::from_str(line).context("decode Banking77 SFT row")?;
    let object = row
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Banking77 SFT row must be a JSON object"))?;
    if !object.get("text").is_some_and(serde_json::Value::is_string) {
        let text = object
            .get("query")
            .filter(|value| value.is_string())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Banking77 SFT row is missing text/query"))?;
        object.insert("text".into(), text);
    }
    if !object
        .get("category")
        .is_some_and(serde_json::Value::is_string)
    {
        let category = object
            .get("expected")
            .filter(|value| value.is_string())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Banking77 SFT row is missing category/expected"))?;
        object.insert("category".into(), category);
    }
    serde_json::to_string(&row).context("encode normalized Banking77 SFT row")
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
    let rows: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(normalize_banking77_row)
        .collect::<Result<_>>()?;
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
    let reference = banking77_reference_sources().ok();
    let model_id = if reference.is_some() {
        // NanoClassify's public reference result is a GPT-OSS 20B result. A
        // different catalog default makes both its prompt renderer and uplift
        // comparison invalid, even when the CSV split is identical.
        "openai/gpt-oss-20b".to_string()
    } else {
        let catalog = super::tinker_catalog::TinkerBaseModelCatalog::load()?;
        catalog.resolve(request.base_model.as_deref())?
    };
    let reference_mode = reference.is_some();
    let model_title = if reference_mode {
        "Banking77 GPT-OSS 20B Tinker SFT"
    } else {
        "Banking77 Nemotron Lightning Tinker SFT"
    };
    let (shard, shard_path, dataset_digest) = if let Some(sources) = reference.as_ref() {
        let train_digest = dataset_digest_for_path(&sources.train_csv)?;
        let heldout_digest = dataset_digest_for_path(&sources.heldout_csv)?;
        (
            "nanoclassify_reference".to_string(),
            sources.train_csv.clone(),
            content_sha256(format!("{train_digest}\n{heldout_digest}\n").as_bytes()),
        )
    } else {
        let shard = request
            .dataset_shard
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(BANKING77_SHARDS[0])
            .to_string();
        let path = materialize_banking77_shard(&shard)?;
        let digest = dataset_digest_for_path(&path)?;
        super::sft_result::validate_dataset_digest(&path, &digest)?;
        (shard, path, digest)
    };
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
        &dataset_digest,
        reference.as_ref(),
    );
    let create = OptimizerCreateRequest {
        algorithm_id: "sft".into(),
        algorithm_version: Some(if reference_mode {
            "banking77-gpt-oss-20b-tinker-v1".into()
        } else {
            "banking77-nemotron-lightning-tinker-v1".into()
        }),
        objective: Some(
            "Banking77 intent SFT · hosted Tinker · banking77_classify checkpoint campaigns".into(),
        ),
        source: Some("hosted".into()),
        project_ref: Some(if reference_mode {
            "banking77@gpt-oss-20b-tinker".into()
        } else {
            "banking77@nemotron-lightning-tinker".into()
        }),
        session_ref: request.session_ref.clone(),
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![
            OptimizerExecutionBinding {
                kind: "optimizer_sidecar".into(),
                id: HOSTED_SFT_BANKING77_RECIPE.into(),
                label: Some("Optimizers sidecar hosted SFT".into()),
                status: Some("admitted".into()),
                metadata: json!({
                    "recipeId": HOSTED_SFT_BANKING77_RECIPE,
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
                title: Some(model_title.into()),
                metadata: json!({"backend": "tinker", "baseModel": model_id}),
            },
            OptimizerResourceRef {
                kind: "dataset".into(),
                id: training_file.clone(),
                digest: Some(dataset_digest.clone()),
                role: Some("train".into()),
                title: Some(format!("Banking77 SFT corpus · {shard}")),
                metadata: json!({"shard": shard, "shards": BANKING77_SHARDS, "datasetDigest": dataset_digest, "splitStrategy": reference.as_ref().map(|_| "banking77.nanoclassify.v1")}),
            },
        ]),
        capabilities: Some(OptimizerCapabilities::for_algorithm("sft")),
        summary: Some(json!({
            "recipeId": HOSTED_SFT_BANKING77_RECIPE,
            "backend": "tinker",
            "producer": "synth-optimizers",
            "baseModel": model_id,
            "datasetShard": shard,
            "datasetDigest": dataset_digest,
            "adapter": lora_adapter_label(HOSTED_SFT_LORA_RANK),
            "rank": HOSTED_SFT_LORA_RANK,
            "localSlot": container_url,
            "checkpointSteps": BANKING77_CHECKPOINT_STEPS,
            "trainingSteps": BANKING77_TRAINING_STEPS,
            "batchSize": 64,
            "selectionExamples": BANKING77_SELECTION_SIZE,
            "heldoutExamples": BANKING77_HELDOUT_SIZE,
            "heldoutSealed": reference.as_ref().is_some_and(|sources| sources.heldout_indices_json.is_some()),
        })),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    admit_hosted(service, request, create, config_toml).await
}

fn banking77_config_toml(
    run_id: &str,
    training_file: &str,
    model_id: &str,
    container_url: &str,
    training_jsonl: &str,
    dataset_digest: &str,
    reference: Option<&Banking77ReferenceSources>,
) -> String {
    let steps = BANKING77_CHECKPOINT_STEPS
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let adapter = lora_adapter_label(HOSTED_SFT_LORA_RANK);
    let label_taxonomy = super::cispo::BANKING77_LABEL_TAXONOMY
        .iter()
        .map(|label| format!("{label:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let reference_contract = reference
        .map(|_| {
            let system_prompt = format!(
                "Classify the customer banking message. Return exactly one label from this list, with no explanation or punctuation:\n{}",
                super::cispo::BANKING77_LABEL_TAXONOMY.join(", ")
            );
            format!(
                "renderer_version = \"renderers.gpt-oss.low.v1\"\nsystem_prompt = {system_prompt:?}\n"
            )
        })
        .unwrap_or_default();
    let reference_dataset = reference
        .map(|sources| {
            let heldout_indices = sources
                .heldout_indices_json
                .as_ref()
                .map(|path| format!("heldout_indices_json = {:?}\n", path.to_string_lossy()))
                .unwrap_or_default();
            format!(
                r#"
[dataset]
recipe_id = "banking77.sft.nanoclassify.v1"
split_strategy = "banking77.nanoclassify.v1"
train_csv = {:?}
heldout_csv = {:?}
{heldout_indices}split_seed = {BANKING77_SPLIT_SEED}
selection_seed = {BANKING77_SELECTION_SEED}
heldout_seed = {BANKING77_HELDOUT_SEED}
dev_per_class = 10
selection_size = {BANKING77_SELECTION_SIZE}
heldout_size = {BANKING77_HELDOUT_SIZE}
label_taxonomy = [{label_taxonomy}]
"#,
                sources.train_csv.to_string_lossy(),
                sources.heldout_csv.to_string_lossy(),
            )
        })
        .unwrap_or_default();
    format!(
        r#"run_id = "{run_id}"
backend = "tinker"
{reference_contract}base_model = "{model_id}"
adapter = "{adapter}"
training_file_id = "{training_file}"
training_jsonl = "{training_jsonl}"
dataset_digest = "{dataset_digest}"
selection_file_id = "file_selection"
heldout_file_id = "file_heldout"
accelerator_slots = 1
rank = {HOSTED_SFT_LORA_RANK}
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

[training]
steps = {BANKING77_TRAINING_STEPS}
batch_size = 64
learning_rate = 0.00002
checkpoint_every_steps = 25
eval_every_steps = 25

[evaluation]
max_tokens = 1024
confidence = 0.95
bootstrap_resamples = 4000
minimum_claim_uplift = 0.01
minimum_paired_examples = 400
{reference_dataset}

[metadata]
evaluation_schema = "training.evaluation.plan.v1"
evaluation_phases = ["baseline", "checkpoint", "final"]
evaluation_transport = "tunnel"
evaluation_metric = "reward"
evaluation_required = true
evaluation_exact_checkpoint_required = true
evaluation_auth = "sidecar_tunnel_lease"
evaluation_harness = "classify"
evaluation_plan_ref = "{BANKING77_PLAN_REF}"
evaluation_world_ref = "{BANKING77_WORLD_REF}"
evaluation_sample_count = 16
evaluation_timeout_s = {CHECKPOINT_EVALUATION_TIMEOUT_S}

# The classify harness resolves the checkpoint through evaluator-owned keys
# (inference_target, sampler_path, …), which this recipe must not set. It still
# has to declare a policy: an empty one is refused, because the policy under
# test is never defaulted.
[checkpoint_evaluation_policy]
api_key_env = "TINKER_API_KEY"

[hyperparameters]
rank = 16
batch_size = 64
lr = 0.00002
"#
    )
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
    let container_url = local_craftax_slot_url()?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("sft_craftax_nemo_{}", &suffix[..8]);
    let training_file = format!("file_train_{}", &suffix[..8]);
    let training_jsonl = std::env::var("SYNTH_SFT_TRAIN_JSONL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let dataset_digest = match training_jsonl.as_deref() {
        Some(path) => dataset_digest_for_path(std::path::Path::new(path))?,
        None => content_sha256(b"craftax-sft-unspecified-dataset.v1\n"),
    };
    if let Some(path) = training_jsonl.as_deref() {
        super::sft_result::validate_dataset_digest(std::path::Path::new(path), &dataset_digest)?;
    }
    let config_toml = craftax_nemotron_config_toml(
        &run_id,
        &training_file,
        &model_id,
        &container_url,
        training_jsonl.as_deref(),
        &dataset_digest,
    );
    let create = OptimizerCreateRequest {
        algorithm_id: "sft".into(),
        algorithm_version: Some("craftax-nemotron-nano-tinker-v1".into()),
        objective: Some(
            "Craftax Nemotron 3.5 Lightning Tinker SFT · streamed from public Optimizers".into(),
        ),
        source: Some("hosted".into()),
        project_ref: Some("craftax@nemotron-nano-tinker".into()),
        session_ref: request.session_ref.clone(),
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![
            OptimizerExecutionBinding {
                kind: "optimizer_sidecar".into(),
                id: HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE.into(),
                label: Some("Optimizers sidecar hosted SFT".into()),
                status: Some("admitted".into()),
                metadata: json!({
                    "recipeId": HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE,
                    "backend": "tinker",
                    "datasetFile": training_file,
                    "baseModel": model_id,
                    "placement": PLACEMENT_TRAINING_SFT_HOSTED,
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
            "adapter": lora_adapter_label(HOSTED_SFT_LORA_RANK),
            "rank": HOSTED_SFT_LORA_RANK,
            "datasetDigest": dataset_digest,
            "localSlot": container_url,
            "checkpointSteps": CRAFTAX_CHECKPOINT_STEPS,
            "evaluationPlan": { "phases": ["baseline", "checkpoint", "final"], "checkpointSteps": CRAFTAX_CHECKPOINT_STEPS, "transport": "tunnel", "metric": "reward" },
            "trainingSteps": CRAFTAX_TRAINING_STEPS,
            "datasetDigest": dataset_digest,
        })),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    admit_hosted(service, request, create, config_toml).await
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
    let (cancel_tx, cancel_rx) = watch::channel(None);
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
            crate::platform::logging::report(
                "optimizers",
                "eprintln",
                format!("hosted SFT worker {run_id} failed: {error:#}"),
            );
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
    mut cancel: super::CancelObserver,
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
                if changed.is_ok() && cancel.borrow().is_some() {
                    let _ = client.cancel(&run_id).await;
                    service
                        .settle_run(
                            run_id.clone(),
                            super::kernel::SettleCause::Cancelled {
                                request: std::sync::Arc::new(
                                    super::kernel::CancellationRequest::new(
                                        super::kernel::CancellationCause::UserRequested,
                                        "hosted-sft:watcher",
                                        format!("run:{run_id}"),
                                    ),
                                ),
                            },
                            None,
                        )
                        .await?;
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

fn craftax_nemotron_config_toml(
    run_id: &str,
    training_file: &str,
    model_id: &str,
    container_url: &str,
    training_jsonl: Option<&str>,
    dataset_digest: &str,
) -> String {
    let checkpoint_steps = CRAFTAX_CHECKPOINT_STEPS
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let adapter = lora_adapter_label(HOSTED_SFT_LORA_RANK);
    let training_jsonl_line = training_jsonl
        .map(|path| format!("training_jsonl = \"{path}\"\n"))
        .unwrap_or_default();
    format!(
        r#"run_id = "{run_id}"
backend = "tinker"
base_model = "{model_id}"
adapter = "{adapter}"
training_file_id = "{training_file}"
selection_file_id = "file_selection"
heldout_file_id = "file_heldout"
dataset_digest = "{dataset_digest}"
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

[metadata]
evaluation_schema = "training.evaluation.plan.v1"
evaluation_phases = ["baseline", "checkpoint", "final"]
evaluation_transport = "tunnel"
evaluation_metric = "reward"
evaluation_required = true
evaluation_exact_checkpoint_required = true
evaluation_auth = "sidecar_tunnel_lease"
evaluation_harness = "react"
evaluation_plan_ref = "craftax_eval.v1"
evaluation_world_ref = "world:craftax"
evaluation_sample_count = 2
evaluation_timeout_s = {CHECKPOINT_EVALUATION_TIMEOUT_S}

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
    service
        .settle_run(
            run_id.to_string(),
            super::kernel::SettleCause::Failed {
                detail: reason.to_string(),
            },
            Some(json!({ "message": reason, "source": "hosted_sft" })),
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
    let detail = error
        .filter(|value| !value.is_empty())
        .unwrap_or(remote_status)
        .to_string();
    let cause = match status {
        OptimizerRunStatus::Completed => super::kernel::SettleCause::Completed,
        OptimizerRunStatus::Degraded => super::kernel::SettleCause::Degraded {
            detail: detail.clone(),
        },
        OptimizerRunStatus::Cancelled => super::kernel::SettleCause::Cancelled {
            request: std::sync::Arc::new(super::kernel::CancellationRequest::new(
                super::kernel::CancellationCause::ContainerRequested,
                "hosted-sft:remote",
                format!("run:{run_id}"),
            )),
        },
        OptimizerRunStatus::Failed => super::kernel::SettleCause::Failed {
            detail: detail.clone(),
        },
        _ => bail!("{remote_status} is not terminal"),
    };
    let error_payload = (status != OptimizerRunStatus::Completed)
        .then(|| json!({"message": detail, "source": "hosted_sft"}));
    service
        .settle_run(run_id.to_string(), cause, error_payload)
        .await?;
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
                    "sha256:deadbeef",
                    None,
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
                    "sha256:deadbeef",
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
    fn banking77_toml_pins_campaigns_through_banking77_classify() {
        let toml = banking77_config_toml(
            "sft_banking77_train_a_ab12cd34",
            "file_train_banking77_train_a_ab12cd34",
            "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16",
            "http://127.0.0.1:8110",
            "/tmp/train_a.jsonl",
            "sha256:content",
            None,
        );
        assert!(toml.contains("backend = \"tinker\""));
        assert!(toml.contains("checkpoint_steps = [25, 50, 75, 100]"));
        assert!(toml.contains("batch_size = 64"));
        assert!(toml.contains("minimum_paired_examples = 400"));
        assert!(toml.contains("campaign_rollouts_per_checkpoint = 2"));
        assert!(toml.contains("checkpoint_evaluation_plan_ref = \"banking77_eval.v1\""));
        assert!(toml.contains("checkpoint_evaluation_world_ref = \"world:banking77@heldout\""));
        assert!(toml.contains("evaluation_phases = [\"baseline\", \"checkpoint\", \"final\"]"));
        assert!(toml.contains("evaluation_transport = \"tunnel\""));
        assert!(toml.contains("evaluation_required = true"));
        assert!(toml.contains("evaluation_exact_checkpoint_required = true"));
        assert!(toml.contains("evaluation_auth = \"sidecar_tunnel_lease\""));
        assert!(toml.contains("evaluation_sample_count = 16"));
        assert!(toml.contains("container_url = \"http://127.0.0.1:8110\""));
        assert!(!toml.contains("goex.sft"));
    }

    #[test]
    fn banking77_reference_toml_is_valid_and_pins_nanoclassify_split() {
        let sources = Banking77ReferenceSources {
            train_csv: std::path::PathBuf::from("/tmp/banking77-train.csv"),
            heldout_csv: std::path::PathBuf::from("/tmp/banking77-heldout.csv"),
            heldout_indices_json: Some(std::path::PathBuf::from("/tmp/heldout-indices.json")),
        };
        let raw = banking77_config_toml(
            "sft_banking77_reference_ab12cd34",
            "banking77.sft.nanoclassify.v1",
            "openai/gpt-oss-20b",
            "http://127.0.0.1:8110",
            "/tmp/banking77-train.csv",
            "sha256:content",
            Some(&sources),
        );
        let parsed: toml::Value = toml::from_str(&raw).unwrap();
        assert_eq!(parsed["base_model"].as_str(), Some("openai/gpt-oss-20b"));
        assert_eq!(
            parsed["renderer_version"].as_str(),
            Some("renderers.gpt-oss.low.v1")
        );
        assert!(parsed["system_prompt"].as_str().is_some_and(|value| value
            .starts_with("Classify the customer banking message. Return exactly one label")));
        assert_eq!(
            parsed["dataset"]["split_strategy"].as_str(),
            Some("banking77.nanoclassify.v1")
        );
        assert_eq!(parsed["dataset"]["selection_size"].as_integer(), Some(400));
        assert_eq!(parsed["dataset"]["heldout_size"].as_integer(), Some(400));
        assert_eq!(
            parsed["dataset"]["label_taxonomy"].as_array().map(Vec::len),
            Some(77)
        );
        assert_eq!(parsed["training"]["steps"].as_integer(), Some(100));
        assert_eq!(parsed["training"]["batch_size"].as_integer(), Some(64));
        assert_eq!(parsed["evaluation"]["max_tokens"].as_integer(), Some(1024));
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
    fn banking77_rows_are_normalized_for_the_public_trainer() {
        let normalized = normalize_banking77_row(
            r#"{"id":"mipro_1","query":"My card is declined","expected":"card_not_working"}"#,
        )
        .unwrap();
        let row: Value = serde_json::from_str(&normalized).unwrap();
        assert_eq!(row["id"], "mipro_1");
        assert_eq!(row["text"], "My card is declined");
        assert_eq!(row["category"], "card_not_working");
    }

    #[test]
    fn banking77_rows_without_labels_fail_closed() {
        let error = normalize_banking77_row(r#"{"query":"My card is declined"}"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("category/expected"), "{error}");
    }

    #[test]
    fn hosted_sft_recipes_declare_an_explicit_cost_ceiling() {
        let craftax = craftax_nemotron_recipe();
        assert_eq!(
            craftax["limits"]["costCeilingUsd"],
            HOSTED_SFT_COST_CEILING_USD
        );
        let banking77 = banking77_recipe();
        assert_eq!(
            banking77["limits"]["costCeilingUsd"],
            HOSTED_SFT_COST_CEILING_USD
        );
        assert_eq!(HOSTED_SFT_COST_CEILING_USD, 15.0);
    }

    #[test]
    fn hosted_sft_prerequisites_name_the_public_sft_service() {
        for recipe in [craftax_nemotron_recipe(), banking77_recipe()] {
            let text = serde_json::to_string(&recipe).unwrap();
            assert!(!text.contains("Optimizers-beta"), "{}", recipe["id"]);
            let prerequisites = recipe["prerequisites"].as_array().unwrap();
            assert!(prerequisites.iter().any(|item| {
                item.as_str() == Some("synth-optimizers sft service --db … --bind 127.0.0.1:8878")
            }));
            assert!(prerequisites
                .iter()
                .any(|item| item.as_str() == Some("SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN")));
            assert!(prerequisites
                .iter()
                .any(|item| item.as_str() == Some("SYNTH_OPTIMIZERS_SFT_SERVICE_URL")));
            assert!(prerequisites
                .iter()
                .any(|item| item.as_str() == Some("SYNTH_OPTIMIZERS_SFT_FIXTURE=1 for unpaid")));
        }
        let craftax = serde_json::to_string(&craftax_nemotron_recipe()).unwrap();
        assert!(craftax.contains("127.0.0.1:8098"));
        let banking = serde_json::to_string(&banking77_recipe()).unwrap();
        assert!(banking.contains("SYNTH_SFT_BANKING77_TRAIN_JSONL"));
        assert!(banking.contains("127.0.0.1:8110"));
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
            "sha256:content",
        );
        assert!(toml.contains("backend = \"tinker\""));
        assert!(toml.contains("base_model = \"nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16\""));
        assert!(toml.contains("evaluator_version = \"craftax_gamebench.v1\""));
        assert!(toml.contains("checkpoint_evaluation_seeds = [501, 502]"));
        assert!(toml.contains("evaluation_phases = [\"baseline\", \"checkpoint\", \"final\"]"));
        assert!(toml.contains("checkpoint_steps = [16, 33, 66]"));
        assert!(toml.contains("training_steps = 66"));
        assert!(toml.contains("world:craftax"));
        assert!(!toml.contains("goex.sft"));
        assert!(!toml.contains("UNPINNED"));
        assert!(!toml.contains("nvidia/nemotron-3-nano-30b-a3b"));
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
        let toml = banking77_config_toml(
            "sft_run_a",
            "file_train_run_a",
            "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16",
            "http://127.0.0.1:8110",
            &first.to_string_lossy(),
            &left,
            None,
        );
        assert!(toml.contains(&format!("dataset_digest = \"{left}\"")));
        assert!(!toml.contains("file_train_run_a") || toml.contains("training_file_id"));
    }

    #[test]
    fn hosted_toml_adapter_rank_agrees_with_s7_authority() {
        let banking = banking77_config_toml(
            "sft_b",
            "file_b",
            "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16",
            "http://127.0.0.1:8110",
            "/tmp/train_a.jsonl",
            "sha256:abc",
            None,
        );
        let craftax = craftax_nemotron_config_toml(
            "sft_c",
            "file_c",
            "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16",
            "http://127.0.0.1:8098",
            None,
            "sha256:abc",
        );
        assert!(banking.contains("adapter = \"lora_r16\""), "{banking}");
        assert!(banking.contains("rank = 16"), "{banking}");
        assert!(craftax.contains("adapter = \"lora_r8\""), "{craftax}");
        assert!(craftax.contains("rank = 8"), "{craftax}");
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
