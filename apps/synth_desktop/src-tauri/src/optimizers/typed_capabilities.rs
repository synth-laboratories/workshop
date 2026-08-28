//! Typed agent capabilities for local MLX training (P1-13).
//!
//! These are the nine named operations. Mutating ones require `confirm=true`.
//! Nothing here downloads weights unless `install_model_or_runtime` is called
//! with confirm — and even then HF Hub offline remains on the MLX child.

use super::eval_recipes::EVAL_MLX_LOCAL_RECIPE;
use super::sidecar_training::{LOCAL_MLX_CISPO_RECIPE, LOCAL_MLX_SFT_RECIPE};
use anyhow::{bail, Result};
use serde_json::{json, Value};

pub fn require_confirm(confirm: bool, capability: &str) -> Result<()> {
    if !confirm {
        bail!("{capability} requires confirm=true");
    }
    Ok(())
}

pub fn inspect_local_mlx() -> Value {
    crate::training_models::inspect_local_mlx()
}

pub fn plan_model_install(model_id: Option<&str>) -> Result<Value> {
    let ready = inspect_local_mlx();
    let expected = crate::training_models::QWEN_TRAINING_MODEL_ID;
    if let Some(id) = model_id.map(str::trim).filter(|value| !value.is_empty()) {
        if id != expected {
            bail!("v0.7 local training installs exactly `{expected}`");
        }
    }
    Ok(json!({
        "modelId": expected,
        "revision": ready["revision"],
        "licenseUrl": ready["licenseUrl"],
        "downloadBytes": ready["downloadBytes"],
        "minDiskBytes": ready["minDiskBytes"],
        "availableDiskBytes": ready["availableDiskBytes"],
        "alreadyPresent": ready["modelPresent"],
        "source": "huggingface",
        "silentDownload": false
    }))
}

pub fn install_model_or_runtime(model_id: Option<&str>, confirm: bool) -> Result<Value> {
    require_confirm(confirm, "install_model_or_runtime")?;
    let plan = plan_model_install(model_id)?;
    if plan["alreadyPresent"] == true {
        return Ok(json!({
            "status": "present",
            "plan": plan,
            "downloaded": false
        }));
    }
    let hit = crate::training_models::download_training_model(
        crate::training_models::QWEN_TRAINING_MODEL_ID,
    )?;
    Ok(json!({
        "status": "installed",
        "plan": plan,
        "downloaded": true,
        "path": hit.path,
        "revision": hit.revision,
        "totalBytes": hit.total_bytes
    }))
}

pub fn create_training_plan(recipe_id: &str) -> Result<Value> {
    let recipe = match recipe_id {
        LOCAL_MLX_SFT_RECIPE => super::mlx_sft::recipe_catalog(),
        LOCAL_MLX_CISPO_RECIPE => super::cispo::recipe_catalog()
            .into_iter()
            .find(|item| item.get("id").and_then(Value::as_str) == Some(LOCAL_MLX_CISPO_RECIPE))
            .ok_or_else(|| anyhow::anyhow!("local CISPO recipe missing from catalog"))?,
        other => bail!("create_training_plan admits `{LOCAL_MLX_SFT_RECIPE}` or `{LOCAL_MLX_CISPO_RECIPE}`, not `{other}`"),
    };
    let limits = recipe.get("limits").cloned().unwrap_or(Value::Null);
    let resolved_config = limits
        .get("resolvedConfig")
        .cloned()
        .unwrap_or_else(|| limits.clone());
    Ok(json!({
        "recipeId": recipe_id,
        "startsRun": false,
        "resolvedConfig": resolved_config,
        "availability": recipe.get("availability"),
        "availabilityReason": recipe.get("availabilityReason"),
        "preflight": recipe.get("preflight")
    }))
}

pub fn list_training_artifacts() -> Result<Value> {
    Ok(json!({ "artifacts": crate::training_artifacts::list()? }))
}

pub fn inspect_training_artifact(id: &str) -> Result<Value> {
    let artifact = crate::training_artifacts::get(id)?;
    Ok(json!({
        "artifact": artifact,
        "snapshotId": crate::training_artifacts::snapshot_id_for(&artifact),
        "inferenceReady": artifact.is_inference_ready()
    }))
}

pub fn launch_artifact_eval_request(
    artifact_id: &str,
    recipe_id: Option<&str>,
    confirm: bool,
) -> Result<Value> {
    require_confirm(confirm, "launch_artifact_eval")?;
    let recipe = recipe_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(EVAL_MLX_LOCAL_RECIPE);
    if recipe != EVAL_MLX_LOCAL_RECIPE {
        bail!("launch_artifact_eval admits `{EVAL_MLX_LOCAL_RECIPE}`");
    }
    let _ = crate::training_artifacts::get(artifact_id)?;
    Ok(json!({
        "recipeId": recipe,
        "trainingArtifactId": artifact_id,
        "confirm": true
    }))
}

pub fn export_or_delete_artifact(
    id: &str,
    operation: &str,
    confirm: bool,
    destination: Option<&str>,
    expected_digest: Option<&str>,
) -> Result<Value> {
    require_confirm(confirm, "export_or_delete_artifact")?;
    match operation {
        "export" => {
            let dest = destination
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("export destination is required"))?;
            let receipt = crate::training_artifacts::export_to(id, dest, expected_digest)?;
            Ok(serde_json::to_value(receipt)?)
        }
        "delete" => {
            let receipt = crate::training_artifacts::delete(id)?;
            Ok(serde_json::to_value(receipt)?)
        }
        other => {
            bail!("export_or_delete_artifact operation must be export or delete, not `{other}`")
        }
    }
}

