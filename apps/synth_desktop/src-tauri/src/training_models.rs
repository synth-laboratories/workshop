//! Managed base-model weights for local MLX training.
//!
//! This catalog is intentionally separate from Laguna inference models.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter};

pub const QWEN_TRAINING_MODEL_ID: &str = "Qwen/Qwen3.5-0.8B";
// Verified Hugging Face repository revision published on 2026-03-02.
pub const QWEN_TRAINING_MODEL_REVISION: &str = "2fc06364715b967f1860aea9cf38778875588b17";
pub const QWEN_TRAINING_LICENSE_URL: &str = "https://huggingface.co/Qwen/Qwen3.5-0.8B";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrainingFailureClass {
    DiskSpace,
    IncompleteModel,
    ChecksumMismatch,
    Provider,
    UnknownModel,
    WeightsInUse,
    RuntimeMissing,
    Other,
}

pub fn classify_training_failure(message: &str) -> TrainingFailureClass {
    let lower = message.to_ascii_lowercase();
    if lower.contains("free disk") || lower.contains("disk space") {
        TrainingFailureClass::DiskSpace
    } else if lower.contains("checksum") || lower.contains("sha-256") {
        TrainingFailureClass::ChecksumMismatch
    } else if lower.contains("incomplete") || lower.contains("weight shard is missing") {
        TrainingFailureClass::IncompleteModel
    } else if lower.contains("unknown on-device") {
        TrainingFailureClass::UnknownModel
    } else if lower.contains("has them open") {
        TrainingFailureClass::WeightsInUse
    } else if lower.contains("managed python") || lower.contains("python was not found") {
        TrainingFailureClass::RuntimeMissing
    } else if lower.contains("huggingface")
        || lower.contains("hf_hub")
        || lower.contains("connection")
        || lower.contains("timed out")
        || lower.contains("timeout")
    {
        TrainingFailureClass::Provider
    } else {
        TrainingFailureClass::Other
    }
}

/// Setup-view facts the UI and `inspect_local_mlx` share. Does not download.
pub fn inspect_local_mlx() -> Value {
    let spec = TRAINING_MODEL_CATALOG[0];
    json!({
        "appleSilicon": cfg!(all(target_os = "macos", target_arch = "aarch64")),
        "platform": std::env::consts::OS,
        "architecture": std::env::consts::ARCH,
        "modelId": spec.id,
        "revision": spec.revision,
        "licenseUrl": QWEN_TRAINING_LICENSE_URL,
        "modelPresent": training_model_present(spec.id),
        "modelsRoot": training_models_root().display().to_string(),
        "availableDiskBytes": available_disk_bytes(&training_models_root()),
        "downloadBytes": spec.download_bytes,
        "minDiskBytes": spec.min_disk_bytes,
        "mlxRuntime": "synth-mlx-rl",
        "loraDropoutDefault": 0.0,
        "resumeWithDropout": "refused"
    })
}

#[derive(Clone, Copy)]
struct TrainingModelSpec {
    id: &'static str,
    revision: &'static str,
    title: &'static str,
    min_disk_bytes: u64,
    download_bytes: u64,
}

const TRAINING_MODEL_CATALOG: [TrainingModelSpec; 1] = [TrainingModelSpec {
    id: QWEN_TRAINING_MODEL_ID,
    revision: QWEN_TRAINING_MODEL_REVISION,
    title: "Qwen 3.5 0.8B (MLX training)",
    min_disk_bytes: 3 * 1024 * 1024 * 1024,
    download_bytes: 1_750_000_000,
}];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrainingModelHit {
    pub path: String,
    pub models_root: String,
    pub model_id: String,
    pub revision: String,
    #[specta(type = specta_typescript::Unknown)]
    pub shard_count: usize,
    #[specta(type = specta_typescript::Unknown)]
    pub total_bytes: u64,
}

fn model_spec(model_id: &str) -> Result<TrainingModelSpec> {
    TRAINING_MODEL_CATALOG
        .iter()
        .copied()
        .find(|spec| spec.id == model_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown on-device training model `{model_id}`"))
}

/// Instance-scoped training-model root. Named desktop instances set
/// `SYNTH_DESKTOP_DATA_ROOT`; the unset path is `~/.synth-desktop`.
pub fn training_models_root() -> PathBuf {
    crate::instance::state_root().join("models/training")
}

/// Resolve the managed path for an allowlisted local-training model.
pub fn training_model_dir(id: &str) -> Option<PathBuf> {
    model_spec(id)
        .ok()
        .map(|spec| training_models_root().join(spec.id))
}

/// Whether a complete, validated managed local-training model is present.
pub fn training_model_present(id: &str) -> bool {
    training_model_dir(id)
        .and_then(|path| validate_model_dir(&path).ok())
        .is_some()
}

fn validate_model_dir(model_dir: &Path) -> Result<TrainingModelHit> {
    let canonical = model_dir
        .canonicalize()
        .with_context(|| format!("resolve training model directory {}", model_dir.display()))?;
    let spec = TRAINING_MODEL_CATALOG
        .iter()
        .find(|spec| canonical.ends_with(spec.id))
        .copied()
        .ok_or_else(|| {
            anyhow::anyhow!("{} is not a managed training model", canonical.display())
        })?;
    let config = canonical.join("config.json");
    if !config.is_file() {
        anyhow::bail!("Training model is incomplete: missing {}", config.display());
    }
    let index_path = canonical.join("model.safetensors.index.json");
    let index: Value =
        serde_json::from_str(&fs::read_to_string(&index_path).with_context(|| {
            format!(
                "Training model is incomplete: missing {}",
                index_path.display()
            )
        })?)
        .with_context(|| format!("Invalid JSON in {}", index_path.display()))?;
    let weight_map = index
        .get("weight_map")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("{} has no weight_map object", index_path.display()))?;
    let shards: HashSet<&str> = weight_map.values().filter_map(Value::as_str).collect();
    if shards.is_empty() {
        anyhow::bail!(
            "{} references no training weight shards",
            index_path.display()
        );
    }
    let mut total_bytes = 0;
    for shard in &shards {
        if !shard.ends_with(".safetensors") || shard.contains('/') || shard.contains('\\') {
            anyhow::bail!("Unsafe training weight path `{shard}`");
        }
        total_bytes += fs::metadata(canonical.join(shard))
            .with_context(|| format!("Training weight shard is missing: {shard}"))?
            .len();
    }
    Ok(TrainingModelHit {
        path: canonical.to_string_lossy().into_owned(),
        models_root: training_models_root().to_string_lossy().into_owned(),
        model_id: spec.id.into(),
        revision: spec.revision.into(),
        shard_count: shards.len(),
        total_bytes,
    })
}

fn list_models() -> Vec<TrainingModelHit> {
    TRAINING_MODEL_CATALOG
        .iter()
        .filter_map(|spec| training_model_dir(spec.id))
        .filter_map(|path| validate_model_dir(&path).ok())
        .collect()
}

fn download_model_with_progress<F>(model_id: &str, mut progress: F) -> Result<TrainingModelHit>
where
    F: FnMut(&str, &str, u64, u64),
{
    let spec = model_spec(model_id)?;
    let root = training_models_root();
    fs::create_dir_all(&root)?;
    if let Some(available) = available_disk_bytes(&root) {
        if available < spec.min_disk_bytes {
            anyhow::bail!(
                "{} needs at least {:.0} GiB of free disk space; only {:.1} GiB is available.",
                spec.title,
                spec.min_disk_bytes as f64 / 1024f64.powi(3),
                available as f64 / 1024f64.powi(3)
            );
        }
    }
    let target = root.join(spec.id);
    fs::create_dir_all(&target)?;
    progress(
        "preparing",
        "Preparing the managed training download…",
        0,
        spec.download_bytes,
    );
    let python = crate::laguna::managed_python()?;
    let script = r#"from huggingface_hub import HfApi, snapshot_download
import hashlib, json, pathlib, sys
repo, revision, target = sys.argv[1], sys.argv[2], pathlib.Path(sys.argv[3])
snapshot_download(repo_id=repo, revision=revision, local_dir=target)
index = json.loads((target / 'model.safetensors.index.json').read_text())
shards = {v for v in (index.get('weight_map') or {}).values()
          if isinstance(v, str) and v.endswith('.safetensors')}
if not shards:
    raise RuntimeError('training model index references no safetensor shards')
info = HfApi().model_info(repo, revision=revision, files_metadata=True)
expected = {}
for sibling in info.siblings:
    lfs = getattr(sibling, 'lfs', None)
    digest = lfs.get('sha256') if isinstance(lfs, dict) else getattr(lfs, 'sha256', None)
    if isinstance(digest, str) and len(digest) == 64:
        expected[sibling.rfilename] = digest.lower()
for shard in sorted(shards):
    if '/' in shard or '\\' in shard:
        raise RuntimeError(f'unsafe indexed training weight: {shard}')
    if shard not in expected:
        raise RuntimeError(f'provider omitted a SHA-256 for training weight: {shard}')
    hasher = hashlib.sha256()
    with (target / shard).open('rb') as handle:
        for chunk in iter(lambda: handle.read(8 * 1024 * 1024), b''):
            hasher.update(chunk)
    if hasher.hexdigest() != expected[shard]:
        raise RuntimeError(f'provider checksum mismatch for training weight: {shard}')
"#;
    progress(
        "downloading",
        "Downloading training weights…",
        dir_size(&target),
        spec.download_bytes,
    );
    let mut child = Command::new(python)
        .arg("-c")
        .arg(script)
        .arg(spec.id)
        .arg(spec.revision)
        .arg(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("download MLX training weights from Hugging Face")?;
    let status = loop {
        if let Some(status) = child.try_wait().context("check training model download")? {
            break status;
        }
        progress(
            "downloading",
            "Downloading training weights…",
            dir_size(&target),
            spec.download_bytes,
        );
        thread::sleep(Duration::from_millis(500));
    };
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    if !status.success() {
        anyhow::bail!(
            "Training model download failed: {}",
            stderr.trim().chars().take(500).collect::<String>()
        );
    }
    let hit = validate_model_dir(&target)?;
    progress(
        "ready",
        "Training weights are ready.",
        hit.total_bytes,
        hit.total_bytes,
    );
    Ok(hit)
}

fn delete_model(model_id: &str) -> Result<()> {
    let target = training_model_dir(model_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown on-device training model `{model_id}`"))?;
    if target.exists() && model_is_mapped(&target) {
        anyhow::bail!("Training weights cannot be deleted while a training process has them open.");
    }
    if target.exists() {
        fs::remove_dir_all(&target)
            .with_context(|| format!("remove training weights {}", target.display()))?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn training_models_list() -> Vec<TrainingModelHit> {
    list_models()
}

#[tauri::command]
#[specta::specta]
pub async fn training_models_download(
    app: AppHandle,
    model_id: String,
) -> std::result::Result<TrainingModelHit, String> {
    let spec = model_spec(&model_id).map_err(|error| error.to_string())?;
    let progress_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        download_model_with_progress(spec.id, |phase, detail, downloaded, total| {
            let _ = progress_app.emit(
                crate::contract::events::EventChannel::TRAINING_MODELS_DOWNLOAD,
                serde_json::json!({
                    "phase": phase,
                    "detail": detail,
                    "modelId": spec.id,
                    "downloadedBytes": downloaded,
                    "totalBytes": total,
                }),
            );
        })
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string());
    let payload = match &result {
        Ok(hit) => serde_json::json!({
            "phase": "ready",
            "detail": format!("{} download complete.", spec.title),
            "path": hit.path,
            "modelId": spec.id,
        }),
        Err(error) => serde_json::json!({
            "phase": "error",
            "detail": error,
            "failureClass": classify_training_failure(error),
            "modelId": spec.id,
        }),
    };
    let _ = app.emit(
        crate::contract::events::EventChannel::TRAINING_MODELS_DOWNLOAD,
        payload,
    );
    result
}

#[tauri::command]
#[specta::specta]
pub async fn training_models_delete(model_id: String) -> std::result::Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || delete_model(&model_id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

fn model_is_mapped(path: &Path) -> bool {
    Command::new("/usr/sbin/lsof")
        .arg("+D")
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            entry
                .metadata()
                .map(|metadata| {
                    if metadata.is_dir() {
                        dir_size(&entry.path())
                    } else {
                        metadata.len()
                    }
                })
                .unwrap_or(0)
        })
        .sum()
}

fn available_disk_bytes(path: &Path) -> Option<u64> {
    let output = Command::new("/bin/df")
        .args(["-Pk"])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let body = String::from_utf8(output.stdout).ok()?;
    let fields: Vec<&str> = body.lines().last()?.split_whitespace().collect();
    fields.get(3)?.parse::<u64>().ok()?.checked_mul(1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_failures_are_typed() {
        assert_eq!(
            classify_training_failure(
                "Qwen 3.5 0.8B (MLX training) needs at least 3 GiB of free disk space; only 0.4 GiB is available."
            ),
            TrainingFailureClass::DiskSpace
        );
        assert_eq!(
            classify_training_failure("provider checksum mismatch for training weight: model.safetensors"),
            TrainingFailureClass::ChecksumMismatch
        );
        assert_eq!(
            classify_training_failure("Training model is incomplete: missing config.json"),
            TrainingFailureClass::IncompleteModel
        );
        assert_eq!(
            classify_training_failure("Training weights cannot be deleted while a training process has them open."),
            TrainingFailureClass::WeightsInUse
        );
        assert_eq!(
            classify_training_failure("Unknown on-device training model `foo`"),
            TrainingFailureClass::UnknownModel
        );
    }

    #[test]
    fn inspect_local_mlx_names_the_pin_without_downloading() {
        let ready = inspect_local_mlx();
        assert_eq!(ready["modelId"], QWEN_TRAINING_MODEL_ID);
        assert_eq!(ready["revision"], QWEN_TRAINING_MODEL_REVISION);
        assert_eq!(ready["licenseUrl"], QWEN_TRAINING_LICENSE_URL);
        assert_eq!(ready["resumeWithDropout"], "refused");
        assert_eq!(ready["loraDropoutDefault"], 0.0);
        assert!(ready.get("modelPresent").and_then(Value::as_bool).is_some());
    }

    #[test]
    fn training_catalog_excludes_laguna() {
        assert_eq!(TRAINING_MODEL_CATALOG.len(), 1);
        assert_eq!(TRAINING_MODEL_CATALOG[0].id, QWEN_TRAINING_MODEL_ID);
        assert!(!TRAINING_MODEL_CATALOG
            .iter()
            .any(|spec| spec.id.contains("Laguna")));
    }

    #[test]
    fn training_models_root_follows_instance_data_root() {
        let isolated = std::env::temp_dir().join(format!(
            "synth-desktop-training-models-root-{}",
            std::process::id()
        ));
        let previous = std::env::var_os(crate::instance::DATA_ROOT_ENV);
        std::env::set_var(crate::instance::DATA_ROOT_ENV, &isolated);
        let root = training_models_root();
        assert_eq!(root, isolated.join("models/training"));
        let expected = root.join(QWEN_TRAINING_MODEL_ID);
        assert_eq!(
            training_model_dir(QWEN_TRAINING_MODEL_ID),
            Some(expected)
        );
        match previous {
            Some(value) => std::env::set_var(crate::instance::DATA_ROOT_ENV, value),
            None => std::env::remove_var(crate::instance::DATA_ROOT_ENV),
        }
    }
}
