//! Managed training artifacts (local LoRA adapters and checkpoints).
//!
//! A finished training run is not done until Workshop can select the exact
//! adapter by id, launch inference or Eval against it, and retain that id in
//! the downstream receipt. `output_refs` alone is not that record.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
    #[serde(default)]
    pub dataset_digest: Option<String>,
    #[serde(default)]
    pub config_digest: Option<String>,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    // specta cannot export u64 (BigInt); the wire value is a JSON number.
    #[serde(default)]
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
        if !hex.is_empty() && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) && hex.len() <= 64 {
            return format!("snap_{hex}");
        }
    }
    let slug: String = artifact
        .id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    format!("snap_{slug}")
}

pub fn require_artifact_id(id: &str) -> Result<&str> {
    let id = id.trim();
    if id.is_empty() || id.len() > 128 {
        bail!("training artifact id is invalid");
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") || Path::new(id).is_absolute() {
        bail!("training artifact id must not be a path");
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':'))
    {
        bail!("training artifact id must be alphanumeric");
    }
    Ok(id)
}

fn tombstones_root() -> PathBuf {
    artifacts_root().join("tombstones")
}

fn leases_root() -> PathBuf {
    artifacts_root().join("leases")
}

fn exports_root() -> PathBuf {
    crate::instance::state_root().join("training-exports")
}

fn tombstone_path(id: &str) -> PathBuf {
    tombstones_root().join(format!("{id}.json"))
}

fn lease_path(id: &str) -> PathBuf {
    leases_root().join(id)
}

pub fn get(id: &str) -> Result<TrainingArtifact> {
    let id = require_artifact_id(id)?;
    if tombstone_path(id).is_file() {
        bail!("training artifact `{id}` was deleted");
    }
    load_index()?
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| anyhow::anyhow!("training artifact `{id}` is not in the managed library"))
}

pub fn lease(id: &str, holder: &str) -> Result<()> {
    let id = require_artifact_id(id)?;
    let _ = get(id)?;
    let root = leases_root();
    fs::create_dir_all(&root)?;
    fs::write(lease_path(id), holder.as_bytes())?;
    Ok(())
}

pub fn release(id: &str, holder: &str) -> Result<()> {
    let id = require_artifact_id(id)?;
    let path = lease_path(id);
    if !path.is_file() {
        return Ok(());
    }
    let current = fs::read_to_string(&path).unwrap_or_default();
    if current == holder || current.is_empty() {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

pub fn is_in_use(id: &str) -> bool {
    require_artifact_id(id)
        .ok()
        .is_some_and(|id| lease_path(id).is_file())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactMutationReceipt {
    pub operation: String,
    pub artifact_id: String,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    #[specta(type = f64)]
    pub bytes: Option<u64>,
    #[serde(default)]
    pub destination: Option<String>,
    pub status: String,
}

fn copy_tree_no_symlinks(src: &Path, dest: &Path) -> Result<u64> {
    let src_meta = fs::symlink_metadata(src)
        .with_context(|| format!("stat artifact path {}", src.display()))?;
    if src_meta.file_type().is_symlink() {
        bail!("artifact path refuses symlink {}", src.display());
    }
    if !src_meta.is_dir() {
        bail!(
            "training artifact path is not a directory: {}",
            src.display()
        );
    }
    fs::create_dir_all(dest)
        .with_context(|| format!("create export destination {}", dest.display()))?;
    let mut bytes = 0u64;
    for entry in fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            bail!(
                "artifact contents refuse symlink {}",
                entry.path().display()
            );
        }
        let target = dest.join(entry.file_name());
        if entry.path().is_dir() {
            bytes += copy_tree_no_symlinks(&entry.path(), &target)?;
        } else {
            bytes += fs::copy(entry.path(), &target)?;
        }
    }
    Ok(bytes)
}

fn resolve_export_destination(destination: &str, artifact_id: &str) -> Result<PathBuf> {
    let destination = destination.trim();
    if destination.is_empty() {
        bail!("export destination is required");
    }
    if destination.contains('\0') {
        bail!("export destination is invalid");
    }
    let raw = Path::new(destination);
    for component in raw.components() {
        if matches!(component, std::path::Component::ParentDir) {
            bail!("export destination refuses path traversal");
        }
    }
    let requested = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        let root = exports_root();
        fs::create_dir_all(&root)?;
        root.join(raw)
    };
    let file_name = requested
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("export destination has no file name"))?;
    let parent = requested
        .parent()
        .ok_or_else(|| anyhow::anyhow!("export destination has no parent"))?;
    if parent.as_os_str().is_empty() {
        bail!("export destination has no parent");
    }
    if parent.exists() {
        if fs::symlink_metadata(parent)?.file_type().is_symlink() {
            bail!("export destination refuses symlink parent");
        }
    } else {
        bail!("export destination parent does not exist");
    }
    if requested.exists() && fs::symlink_metadata(&requested)?.file_type().is_symlink() {
        bail!("export destination refuses symlink");
    }
    let canonical_parent = fs::canonicalize(parent)
        .with_context(|| format!("canonicalize export parent {}", parent.display()))?;
    let resolved = canonical_parent.join(file_name);
    if !raw.is_absolute() {
        let root = fs::canonicalize(exports_root())?;
        if !resolved.starts_with(&root) {
            bail!("export destination escapes the managed export root");
        }
    }
    let _ = artifact_id;
    Ok(resolved)
}

/// Inspect-only export used by older callers. Prefer [`export_to`].
pub fn export(id: &str) -> Result<TrainingArtifact> {
    get(id)
}

pub fn export_to(
    id: &str,
    destination: &str,
    expected_digest: Option<&str>,
) -> Result<ArtifactMutationReceipt> {
    let artifact = get(id)?;
    if let Some(expected) = expected_digest
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let actual = artifact
            .digest
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("training artifact `{id}` has no digest to verify"))?;
        if actual != expected
            && actual.trim_start_matches("sha256:") != expected.trim_start_matches("sha256:")
        {
            bail!("export digest does not match training artifact `{id}`");
        }
    }
    let dest = resolve_export_destination(destination, &artifact.id)?;
    let source = artifact
        .path
        .as_deref()
        .map(Path::new)
        .filter(|path| path.is_dir())
        .ok_or_else(|| {
            anyhow::anyhow!("training artifact `{id}` has no local adapter directory")
        })?;
    if dest.exists() {
        let receipt_path = dest.join(".synth-export-receipt.json");
        if receipt_path.is_file() {
            if let Ok(existing) =
                serde_json::from_str::<ArtifactMutationReceipt>(&fs::read_to_string(&receipt_path)?)
            {
                if existing.digest == artifact.digest {
                    return Ok(existing);
                }
            }
        }
        bail!("export destination already exists for a different artifact");
    }
    let bytes = copy_tree_no_symlinks(source, &dest)?;
    let receipt = ArtifactMutationReceipt {
        operation: "export".into(),
        artifact_id: artifact.id.clone(),
        digest: artifact.digest.clone(),
        bytes: Some(bytes),
        destination: Some(dest.to_string_lossy().into_owned()),
        status: "exported".into(),
    };
    fs::write(
        dest.join(".synth-export-receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok(receipt)
}

pub fn delete(id: &str) -> Result<ArtifactMutationReceipt> {
    let id = require_artifact_id(id)?;
    if tombstone_path(id).is_file() {
        bail!("training artifact `{id}` was deleted");
    }
    if is_in_use(id) {
        bail!("training artifact `{id}` is in use and cannot be deleted");
    }
    let mut artifacts = load_index()?;
    let Some(index) = artifacts.iter().position(|item| item.id == id) else {
        bail!("training artifact `{id}` is not in the managed library");
    };
    let removed = artifacts.remove(index);
    save_index(&artifacts)?;
    fs::create_dir_all(tombstones_root())?;
    let receipt = ArtifactMutationReceipt {
        operation: "delete".into(),
        artifact_id: removed.id.clone(),
        digest: removed.digest.clone(),
        bytes: removed.size_bytes,
        destination: None,
        status: "deleted".into(),
    };
    fs::write(
        tombstone_path(id),
        serde_json::to_vec_pretty(&json!({
            "schemaVersion": "synth.training-artifact-tombstone.v1",
            "artifact": removed,
            "receipt": receipt,
            "deletedAt": now_rfc3339()
        }))?,
    )?;
    Ok(receipt)
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

#[tauri::command]
#[specta::specta]
pub fn training_artifacts_export(
    id: String,
    destination: String,
    expected_digest: Option<String>,
    confirm: bool,
) -> std::result::Result<ArtifactMutationReceipt, String> {
    if !confirm {
        return Err("export_or_delete_artifact requires confirm=true".into());
    }
    export_to(&id, &destination, expected_digest.as_deref()).map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn training_artifacts_delete(
    id: String,
    confirm: bool,
) -> std::result::Result<ArtifactMutationReceipt, String> {
    if !confirm {
        return Err("export_or_delete_artifact requires confirm=true".into());
    }
    delete(&id).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    /// Serialises the tests that repoint the data root.
    ///
    /// `state_root()` reads a process-global variable, so two tests holding
    /// different roots cannot run at the same time however carefully each one
    /// cleans up after itself.
    static ROOT_SEQUENCE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// A private data root for one test, restored when the test ends.
    ///
    /// The directory is unique per call: keying it on the process id alone
    /// gave every test in this module the same path, and each one deletes that
    /// path on entry and exit, so concurrent tests removed each other's files
    /// mid-run. Restoration is a `Drop` rather than a call at the end of the
    /// test, because a panicking test used to leave the variable pointing at a
    /// directory it had already deleted, failing whatever ran next.
    struct IsolatedRoot {
        path: PathBuf,
        previous: Option<std::ffi::OsString>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl Drop for IsolatedRoot {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var(crate::instance::DATA_ROOT_ENV, value),
                None => std::env::remove_var(crate::instance::DATA_ROOT_ENV),
            }
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn isolated_root() -> IsolatedRoot {
        let guard = crate::instance::environment_lock();
        let ordinal = ROOT_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "synth-desktop-training-artifacts-{}-{ordinal}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        let previous = std::env::var_os(crate::instance::DATA_ROOT_ENV);
        std::env::set_var(crate::instance::DATA_ROOT_ENV, &path);
        IsolatedRoot {
            path,
            previous,
            _guard: guard,
        }
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
        assert_eq!(snapshot_id_for(&artifact), "snap_deadbeef");
    }

    #[test]
    fn registry_is_instance_scoped_and_idempotent() {
        let isolated = isolated_root();
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
        assert!(index_path().starts_with(&isolated.path));
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

    fn adapter_dir(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("adapter_config.json"), b"{\"r\":8}").unwrap();
        fs::write(dir.join("weights.safetensors"), b"lora").unwrap();
        dir
    }

    fn registered(root: &Path, id: &str, digest: &str) -> TrainingArtifact {
        let dir = adapter_dir(root, &format!("src-{id}"));
        let artifact = TrainingArtifact::from_mlx_handoff(
            "run-export",
            "sft",
            crate::training_models::QWEN_TRAINING_MODEL_ID,
            &json!({
                "inference": {"kind": "mlx-lora.v1"},
                "checkpoint": {
                    "checkpoint_id": id,
                    "sha256": digest,
                    "path": dir.to_string_lossy(),
                    "bytes": 4
                }
            }),
            None,
            None,
        )
        .unwrap();
        register(artifact.clone()).unwrap();
        artifact
    }

    #[test]
    fn artifact_id_must_not_be_a_path() {
        for id in ["../escape", "a/b", "/tmp/x", "id with space"] {
            let error = require_artifact_id(id).unwrap_err().to_string();
            assert!(
                error.contains("path")
                    || error.contains("alphanumeric")
                    || error.contains("invalid"),
                "{id}: {error}"
            );
        }
        assert_eq!(require_artifact_id("ckpt_10k").unwrap(), "ckpt_10k");
        assert_eq!(
            require_artifact_id("sft_mlx_qwen_57c9c1e7762a:step-4").unwrap(),
            "sft_mlx_qwen_57c9c1e7762a:step-4"
        );
    }

    #[test]
    fn export_refuses_traversal_symlink_wrong_digest_and_duplicate_conflict() {
        let isolated = isolated_root();
        let artifact = registered(&isolated.path, "cap-export", "abcd");
        let dest_parent = isolated.path.join("exports");
        fs::create_dir_all(&dest_parent).unwrap();

        let traversal = export_to(
            "cap-export",
            dest_parent.join("../outside").to_str().unwrap(),
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(traversal.contains("traversal"), "{traversal}");

        let link = isolated.path.join("link-dest");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&dest_parent, &link).unwrap();
            let escaped = export_to("cap-export", link.join("cap").to_str().unwrap(), None)
                .unwrap_err()
                .to_string();
            assert!(escaped.contains("symlink"), "{escaped}");
        }

        let missing = export_to("missing-art", dest_parent.join("x").to_str().unwrap(), None)
            .unwrap_err()
            .to_string();
        assert!(missing.contains("not in the managed library"), "{missing}");

        let wrong = export_to(
            "cap-export",
            dest_parent.join("wrong").to_str().unwrap(),
            Some("sha256:ffff"),
        )
        .unwrap_err()
        .to_string();
        assert!(wrong.contains("digest"), "{wrong}");

        let dest = dest_parent.join("cap-export");
        let first = export_to(
            "cap-export",
            dest.to_str().unwrap(),
            Some(&artifact.digest.clone().unwrap()),
        )
        .unwrap();
        assert_eq!(first.operation, "export");
        assert_eq!(first.artifact_id, "cap-export");
        assert!(first.bytes.unwrap() >= 4);
        assert!(dest.join("weights.safetensors").is_file());

        let duplicate = export_to("cap-export", dest.to_str().unwrap(), None).unwrap();
        assert_eq!(duplicate.digest, first.digest);

        fs::write(
            dest.join(".synth-export-receipt.json"),
            br#"{"operation":"export","artifactId":"other","status":"exported"}"#,
        )
        .unwrap();
        let conflict = export_to("cap-export", dest.to_str().unwrap(), None)
            .unwrap_err()
            .to_string();
        assert!(conflict.contains("already exists"), "{conflict}");
    }

    #[test]
    fn delete_refuses_in_use_and_records_a_tombstone() {
        let isolated = isolated_root();
        registered(&isolated.path, "cap-del", "abcd");
        lease("cap-del", "cispo_run").unwrap();
        let in_use = delete("cap-del").unwrap_err().to_string();
        assert!(in_use.contains("in use"), "{in_use}");
        release("cap-del", "cispo_run").unwrap();
        let receipt = delete("cap-del").unwrap();
        assert_eq!(receipt.status, "deleted");
        assert!(tombstone_path("cap-del").is_file());
        let missing = get("cap-del").unwrap_err().to_string();
        assert!(missing.contains("deleted"), "{missing}");
    }
}
