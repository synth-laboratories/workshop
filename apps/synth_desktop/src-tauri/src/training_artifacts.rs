//! Managed training artifacts (local LoRA adapters and checkpoints).
//!
//! A finished training run is not done until Workshop can select the exact
//! adapter by id, launch inference or Eval against it, and retain that id in
//! the downstream receipt. `output_refs` alone is not that record.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const ARTIFACT_SCHEMA: &str = "synth.training-artifact.v1";
pub const ADAPTER_KIND_MLX_LORA: &str = "mlx-lora.v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrainingArtifact {
    pub schema_version: String,
    pub id: String,
    pub adapter_kind: String,
    pub base_model_id: String,
    pub producing_run_id: String,
    pub producing_algorithm: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    // specta cannot export u64 (BigInt); the wire value is a JSON number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = f64)]
    pub size_bytes: Option<u64>,
    pub integrity: String,
    pub compatible_inference: Vec<String>,
    pub created_at: String,
}

impl TrainingArtifact {
    pub fn from_mlx_handoff(
        run_id: &str,
        algorithm: &str,
        base_model_id: &str,
        handoff: &Value,
        dataset_digest: Option<String>,
        config_digest: Option<String>,
    ) -> Result<Self> {
        let adapter_kind = handoff
            .pointer("/inference/kind")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("training handoff omitted inference.kind"))?
            .to_string();
        if adapter_kind != ADAPTER_KIND_MLX_LORA {
            bail!("unsupported training adapter kind `{adapter_kind}`");
        }
        let checkpoint = handoff
            .get("checkpoint")
            .cloned()
            .unwrap_or_else(|| json_object());
        let id = checkpoint
            .get("checkpoint_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                handoff
                    .get("policy_snapshot_id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
            })
            .ok_or_else(|| anyhow::anyhow!("training handoff omitted checkpoint identity"))?
            .to_string();
        let sha = checkpoint.get("sha256").and_then(Value::as_str);
        let digest = sha.map(|value| {
            if value.starts_with("sha256:") {
                value.to_string()
            } else {
                format!("sha256:{value}")
            }
        });
        let path = checkpoint
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_string);
        let size_bytes = checkpoint.get("bytes").and_then(Value::as_u64).or_else(|| {
            path.as_deref()
                .and_then(|raw| fs::metadata(raw).ok().map(|meta| meta.len()))
        });
        let path_ok = path.as_deref().map(Path::new).is_some_and(Path::is_dir);
        let integrity = if digest.is_some() && (path.is_none() || path_ok) {
            "verified".into()
        } else if path_ok {
            "present".into()
        } else {
            "unavailable".into()
        };
        Ok(Self {
            schema_version: ARTIFACT_SCHEMA.into(),
            id,
            adapter_kind,
            base_model_id: base_model_id.into(),
            producing_run_id: run_id.into(),
            producing_algorithm: algorithm.into(),
            dataset_digest,
            config_digest,
            digest,
            path,
            size_bytes,
            integrity,
            compatible_inference: vec!["mlx-loopback".into()],
            created_at: now_rfc3339(),
        })
    }

    pub fn is_inference_ready(&self) -> bool {
        self.integrity == "verified" || self.integrity == "present"
    }
}

fn json_object() -> Value {
    Value::Object(serde_json::Map::new())
}

fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub fn artifacts_root() -> PathBuf {
    crate::instance::state_root().join("training-artifacts")
}

fn index_path() -> PathBuf {
    artifacts_root().join("index.json")
}

fn load_index() -> Result<Vec<TrainingArtifact>> {
    let path = index_path();
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("read training artifact index {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("parse training artifact index {}", path.display()))
}

fn save_index(artifacts: &[TrainingArtifact]) -> Result<()> {
    let root = artifacts_root();
    fs::create_dir_all(&root)
        .with_context(|| format!("create training artifact root {}", root.display()))?;
    let path = index_path();
    fs::write(
        &path,
        serde_json::to_vec_pretty(artifacts).context("serialize training artifact index")?,
    )
    .with_context(|| format!("write training artifact index {}", path.display()))
}

pub fn register(artifact: TrainingArtifact) -> Result<TrainingArtifact> {
    let mut artifacts = load_index()?;
    if let Some(existing) = artifacts.iter_mut().find(|item| item.id == artifact.id) {
        *existing = artifact.clone();
    } else {
        artifacts.push(artifact.clone());
    }
    save_index(&artifacts)?;
    Ok(artifact)
}

pub fn list() -> Result<Vec<TrainingArtifact>> {
    load_index()
}

pub fn snapshot_id_for(artifact: &TrainingArtifact) -> String {
    if let Some(digest) = artifact.digest.as_deref() {
        let hex = digest.trim_start_matches("sha256:");
        if !hex.is_empty()
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            && hex.len() <= 64
        {
            return format!("snap_{hex}");
        }
    }
    let slug: String = artifact
        .id
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
        .collect();
    format!("snap_{slug}")
}

pub fn get(id: &str) -> Result<TrainingArtifact> {
    load_index()?
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| anyhow::anyhow!("training artifact `{id}` is not in the managed library"))
}

#[tauri::command]
#[specta::specta]
pub fn training_artifacts_list() -> Vec<TrainingArtifact> {
    list().unwrap_or_default()
}

#[tauri::command]
#[specta::specta]
pub fn training_artifacts_get(id: String) -> std::result::Result<TrainingArtifact, String> {
    get(&id).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn isolated_root() -> (PathBuf, Option<std::ffi::OsString>) {
        let isolated = std::env::temp_dir().join(format!(
            "synth-desktop-training-artifacts-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&isolated);
        fs::create_dir_all(&isolated).unwrap();
        let previous = std::env::var_os(crate::instance::DATA_ROOT_ENV);
        std::env::set_var(crate::instance::DATA_ROOT_ENV, &isolated);
        (isolated, previous)
    }

    fn restore(previous: Option<std::ffi::OsString>, isolated: PathBuf) {
        match previous {
            Some(value) => std::env::set_var(crate::instance::DATA_ROOT_ENV, value),
            None => std::env::remove_var(crate::instance::DATA_ROOT_ENV),
        }
        let _ = fs::remove_dir_all(isolated);
    }

    #[test]
    fn mlx_handoff_record_retains_base_model_run_and_digests() {
        let artifact = TrainingArtifact::from_mlx_handoff(
            "run-1",
            "sft",
            crate::training_models::QWEN_TRAINING_MODEL_ID,
            &json!({
                "inference": {"kind": "mlx-lora.v1"},
                "checkpoint": {
                    "checkpoint_id": "run-1-terminal",
                    "sha256": "deadbeef",
                    "path": "/tmp/missing-adapter",
                    "bytes": 128
                },
                "policy_snapshot_id": "run-1-snap"
            }),
            Some("sha256:dataset".into()),
            Some("sha256:config".into()),
        )
        .unwrap();
        assert_eq!(artifact.id, "run-1-terminal");
        assert_eq!(artifact.adapter_kind, ADAPTER_KIND_MLX_LORA);
        assert_eq!(
            artifact.base_model_id,
            crate::training_models::QWEN_TRAINING_MODEL_ID
        );
        assert_eq!(artifact.producing_run_id, "run-1");
        assert_eq!(artifact.digest.as_deref(), Some("sha256:deadbeef"));
        assert_eq!(artifact.dataset_digest.as_deref(), Some("sha256:dataset"));
        assert_eq!(artifact.config_digest.as_deref(), Some("sha256:config"));
        assert_eq!(artifact.size_bytes, Some(128));
        assert_eq!(artifact.integrity, "unavailable");
        assert!(!artifact.is_inference_ready());
        assert_eq!(
            snapshot_id_for(&artifact),
            "snap_deadbeef"
        );
    }

    #[test]
    fn registry_is_instance_scoped_and_idempotent() {
        let (isolated, previous) = isolated_root();
        let artifact = TrainingArtifact::from_mlx_handoff(
            "run-2",
            "cispo",
            crate::training_models::QWEN_TRAINING_MODEL_ID,
            &json!({
                "inference": {"kind": "mlx-lora.v1"},
                "checkpoint": {
                    "checkpoint_id": "run-2-terminal",
                    "sha256": "abc123"
                }
            }),
            None,
            None,
        )
        .unwrap();
        register(artifact.clone()).unwrap();
        register(artifact.clone()).unwrap();
        let listed = list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "run-2-terminal");
        assert_eq!(listed[0].producing_algorithm, "cispo");
        assert!(index_path().starts_with(&isolated));
        restore(previous, isolated);
    }

    #[test]
    fn missing_kind_is_a_visible_error() {
        let error = TrainingArtifact::from_mlx_handoff(
            "run-3",
            "sft",
            crate::training_models::QWEN_TRAINING_MODEL_ID,
            &json!({"checkpoint": {"checkpoint_id": "x"}}),
            None,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("inference.kind"), "{error}");
    }
}
