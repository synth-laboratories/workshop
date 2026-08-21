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

pub fn launch_artifact_eval_request(artifact_id: &str, recipe_id: Option<&str>, confirm: bool) -> Result<Value> {
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
        other => bail!("export_or_delete_artifact operation must be export or delete, not `{other}`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::training_artifacts::TrainingArtifact;
    use serde_json::json;
    use std::fs;

    fn isolated_root() -> (std::path::PathBuf, Option<std::ffi::OsString>) {
        let isolated = std::env::temp_dir().join(format!(
            "synth-desktop-typed-caps-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&isolated);
        fs::create_dir_all(&isolated).unwrap();
        let previous = std::env::var_os(crate::instance::DATA_ROOT_ENV);
        std::env::set_var(crate::instance::DATA_ROOT_ENV, &isolated);
        (isolated, previous)
    }

    fn restore(previous: Option<std::ffi::OsString>, isolated: std::path::PathBuf) {
        match previous {
            Some(value) => std::env::set_var(crate::instance::DATA_ROOT_ENV, value),
            None => std::env::remove_var(crate::instance::DATA_ROOT_ENV),
        }
        let _ = fs::remove_dir_all(isolated);
    }

    #[test]
    fn mutating_capabilities_refuse_without_confirm() {
        let err = install_model_or_runtime(None, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("confirm=true"), "{err}");
        assert!(launch_artifact_eval_request("x", None, false).is_err());
        assert!(export_or_delete_artifact("x", "delete", false, None, None).is_err());
    }

    #[test]
    fn inspect_and_plan_never_download() {
        let inspect = inspect_local_mlx();
        assert_eq!(
            inspect["modelId"],
            crate::training_models::QWEN_TRAINING_MODEL_ID
        );
        let plan = plan_model_install(None).unwrap();
        assert_eq!(plan["silentDownload"], false);
        assert_eq!(plan["alreadyPresent"], inspect["modelPresent"]);
        let err = plan_model_install(Some("someone/else"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("Qwen/Qwen3.5-0.8B"), "{err}");
    }

    #[test]
    fn training_plan_is_preview_only() {
        let plan = create_training_plan(LOCAL_MLX_SFT_RECIPE).unwrap();
        assert_eq!(plan["startsRun"], false);
        assert_eq!(plan["recipeId"], LOCAL_MLX_SFT_RECIPE);
        assert_eq!(
            plan["resolvedConfig"]["baseModel"],
            crate::training_models::QWEN_TRAINING_MODEL_ID
        );
        assert!(create_training_plan("gepa.banking77.smoke.v1").is_err());
    }

    #[test]
    fn artifact_list_inspect_export_delete() {
        let (isolated, previous) = isolated_root();
        let adapter = isolated.join("adapter");
        fs::create_dir_all(&adapter).unwrap();
        fs::write(adapter.join("weights.safetensors"), b"lora").unwrap();
        let artifact = TrainingArtifact::from_mlx_handoff(
            "run-cap",
            "sft",
            crate::training_models::QWEN_TRAINING_MODEL_ID,
            &json!({
                "inference": {"kind": "mlx-lora.v1"},
                "checkpoint": {
                    "checkpoint_id": "cap-1",
                    "sha256": "abcd",
                    "path": adapter.to_string_lossy(),
                    "bytes": 4
                }
            }),
            None,
            None,
        )
        .unwrap();
        crate::training_artifacts::register(artifact).unwrap();
        let listed = list_training_artifacts().unwrap();
        assert_eq!(listed["artifacts"].as_array().unwrap().len(), 1);
        let inspected = inspect_training_artifact("cap-1").unwrap();
        assert_eq!(inspected["artifact"]["id"], "cap-1");
        let dest = isolated.join("exports");
        fs::create_dir_all(&dest).unwrap();
        let dest_path = dest.join("cap-1");
        let exported = export_or_delete_artifact(
            "cap-1",
            "export",
            true,
            Some(dest_path.to_str().unwrap()),
            Some("sha256:abcd"),
        )
        .unwrap();
        assert_eq!(exported["operation"], "export");
        assert_eq!(exported["artifactId"], "cap-1");
        assert!(dest_path.join("weights.safetensors").is_file());
        export_or_delete_artifact("cap-1", "delete", true, None, None).unwrap();
        assert!(inspect_training_artifact("cap-1").is_err());
        restore(previous, isolated);
    }

    #[test]
    fn export_without_destination_or_confirm_fails_closed() {
        assert!(export_or_delete_artifact("cap-1", "export", true, None, None).is_err());
        assert!(export_or_delete_artifact("cap-1", "export", false, Some("/tmp/x"), None).is_err());
    }

    #[test]
    fn eval_launch_retains_artifact_id() {
        let (isolated, previous) = isolated_root();
        let artifact = TrainingArtifact::from_mlx_handoff(
            "run-eval",
            "sft",
            crate::training_models::QWEN_TRAINING_MODEL_ID,
            &json!({
                "inference": {"kind": "mlx-lora.v1"},
                "checkpoint": {"checkpoint_id": "eval-art", "sha256": "ef"}
            }),
            None,
            None,
        )
        .unwrap();
        crate::training_artifacts::register(artifact).unwrap();
        let body = launch_artifact_eval_request("eval-art", None, true).unwrap();
        assert_eq!(body["recipeId"], EVAL_MLX_LOCAL_RECIPE);
        assert_eq!(body["trainingArtifactId"], "eval-art");
        restore(previous, isolated);
    }
}
