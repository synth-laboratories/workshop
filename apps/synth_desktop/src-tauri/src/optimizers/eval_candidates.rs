//! Staging policy source into immutable, content-addressed candidate sets.
//!
//! This is why `optimizer_start_recipe` can take a `candidate_set_id` instead
//! of paths or inline code. An agent supplies bounded policy source as data;
//! Workshop writes it directly into the app-owned candidate store, freezes it,
//! and returns an id. Nothing an agent says becomes a path, an image, a
//! command, an environment variable, or a session-workspace lookup.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const CANDIDATE_SET_SCHEMA: &str = "optimizer.policy-candidate-set.v1";
pub const POLICY_CANDIDATE_SCHEMA: &str = "optimizer.policy-candidate.v1";
const MAX_INLINE_SOURCE_BYTES: usize = 1024 * 1024;
const DEFAULT_SOURCE_FILE_NAME: &str = "policy.py";

/// One policy whose source is supplied directly to the host as bounded data.
#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EvalCandidateSource {
    pub label: String,
    pub content: String,
    #[serde(default, alias = "file_name")]
    pub file_name: Option<String>,
    #[serde(default)]
    pub entrypoint: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub baseline: Option<bool>,
    /// Retained only to turn old clients into an actionable migration error.
    #[serde(default, rename = "path")]
    pub legacy_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EvalStageCandidatesRequest {
    pub candidates: Vec<EvalCandidateSource>,
    /// Retained only to turn old clients into an actionable migration error.
    #[serde(default, rename = "sessionRef", alias = "session_ref")]
    pub legacy_session_ref: Option<String>,
}

pub fn store_root() -> PathBuf {
    super::eval_recipes::eval_home().join("candidates")
}

/// Write each inline source into the store, content-address it, and seal the
/// set. This deliberately has no session or workspace dependency.
pub async fn stage(request: EvalStageCandidatesRequest) -> Result<Value> {
    if request.candidates.is_empty() {
        bail!("a candidate set needs at least one candidate");
    }
    if request.candidates.len() > 16 {
        bail!("a candidate set is capped at 16 candidates");
    }
    if request
        .legacy_session_ref
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        bail!(
            "session workspaces are not supported for candidate staging; send candidate content directly"
        );
    }

    let mut labels = std::collections::HashSet::new();
    for candidate in &request.candidates {
        if candidate.label.trim().is_empty() {
            bail!("candidate labels must not be empty");
        }
        if !labels.insert(candidate.label.trim().to_string()) {
            bail!("candidate labels must be unique inside a set");
        }
    }
    if request
        .candidates
        .iter()
        .filter(|c| c.baseline == Some(true))
        .count()
        > 1
    {
        bail!("a candidate set may designate at most one baseline");
    }

    let set_id = format!("policy_set_{}", uuid::Uuid::new_v4().simple());
    let root = store_root().join(&set_id);
    let artifacts = root.join("artifacts");
    fs::create_dir_all(&artifacts).context("create candidate store")?;

    let mut entries = Vec::new();
    let mut baseline_id: Option<String> = None;
    for source in &request.candidates {
        if source
            .legacy_path
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            bail!(
                "candidate paths are no longer supported; send candidate content directly instead"
            );
        }
        let staging = artifacts.join(format!("pending_{}", uuid::Uuid::new_v4().simple()));
        let file_name = write_inline_source(&staging, source)
            .with_context(|| format!("stage candidate {}", source.label))?;
        let digest = digest_tree(&staging)?;
        let hex = digest.trim_start_matches("sha256:").to_string();
        let final_path = artifacts.join(&hex);
        if final_path.exists() {
            let _ = fs::remove_dir_all(&staging); // identical bytes already staged
        } else {
            fs::rename(&staging, &final_path).context("seal staged candidate")?;
            freeze(&final_path)?;
        }
        let id = format!("policy_{}", uuid::Uuid::new_v4().simple());
        if source.baseline == Some(true) {
            baseline_id = Some(id.clone());
        }
        entries.push(json!({
            "schema_version": POLICY_CANDIDATE_SCHEMA,
            "id": id,
            "label": source.label.trim(),
            "kind": source.kind.clone().unwrap_or_else(|| "python-code.v1".into()),
            "artifact": {"uri": format!("local-artifact://sha256/{hex}"), "digest": digest},
            "entrypoint": source.entrypoint.clone().unwrap_or_else(|| "policy:Policy".into()),
            "metadata": {
                "source": {"kind": "inline", "name": file_name},
                "parent_optimizer_run_id": Value::Null
            }
        }));
    }

    let manifest = json!({
        "schema_version": CANDIDATE_SET_SCHEMA,
        "id": set_id,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "baseline_id": baseline_id,
        "candidates": entries,
    });
    fs::write(
        root.join("candidate_set.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )
    .context("write candidate set manifest")?;
    Ok(manifest)
}

pub fn manifest_path(candidate_set_id: &str) -> Result<PathBuf> {
    if candidate_set_id.is_empty()
        || !candidate_set_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        bail!("invalid candidate set id");
    }
    let path = store_root()
        .join(candidate_set_id)
        .join("candidate_set.json");
    if !path.is_file() {
        bail!("unknown candidate set {candidate_set_id}");
    }
    Ok(path)
}

pub fn load(candidate_set_id: &str) -> Result<Value> {
    let path = manifest_path(candidate_set_id)?;
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

/// Stage a managed training artifact as an `mlx-lora.v1` candidate set.
///
/// Paths come from the instance-owned artifact record, never from an agent.
pub fn stage_training_artifact(
    artifact: &crate::training_artifacts::TrainingArtifact,
) -> Result<Value> {
    if artifact.adapter_kind != "mlx-lora.v1" {
        bail!(
            "training artifact {} is {}, not mlx-lora.v1",
            artifact.id,
            artifact.adapter_kind
        );
    }
    if !artifact.is_inference_ready() {
        bail!(
            "training artifact {} is not inference-ready ({})",
            artifact.id,
            artifact.integrity
        );
    }
    let origin = artifact
        .path
        .as_deref()
        .map(Path::new)
        .ok_or_else(|| anyhow!("training artifact {} has no adapter path", artifact.id))?;
    let origin = origin.canonicalize().with_context(|| {
        format!("training artifact {} path is missing: {}", artifact.id, origin.display())
    })?;
    let state = crate::instance::state_root();
    let data = crate::instance::data_root();
    if !(origin.starts_with(&state) || origin.starts_with(&data)) {
        bail!("training artifact {} path escapes the instance roots", artifact.id);
    }

    let set_id = format!(
        "policy_set_{}",
        artifact
            .id
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect::<String>()
    );
    let root = store_root().join(&set_id);
    if root.join("candidate_set.json").is_file() {
        return load(&set_id);
    }
    let artifacts = root.join("artifacts");
    fs::create_dir_all(&artifacts).context("create candidate store")?;
    let staging = artifacts.join(format!("pending_{}", uuid::Uuid::new_v4().simple()));
    copy_tree(&origin, &staging).context("copy training artifact into the candidate store")?;
    if !staging.join("policy.json").is_file() {
        let digest = format!(
            "sha256:{:x}",
            Sha256::digest(b"synth.workshop.v07.qwen35-0.8b.thinking-off")
        );
        let policy = json!({
            "schema_version": "eval.mlx-lora-policy.v1",
            "base_model": artifact.base_model_id,
            "adapter": true,
            "chat_template_digest": digest,
            "thinking_mode": "off",
            "rank": 8,
        });
        fs::write(staging.join("policy.json"), serde_json::to_vec_pretty(&policy)?)?;
    }
    let digest = digest_tree(&staging)?;
    let hex = digest.trim_start_matches("sha256:").to_string();
    let final_path = artifacts.join(&hex);
    if final_path.exists() {
        let _ = fs::remove_dir_all(&staging);
    } else {
        fs::rename(&staging, &final_path).context("seal staged training artifact")?;
        freeze(&final_path)?;
    }
    let candidate_id = format!("policy_{hex}");
    let manifest = json!({
        "schema_version": CANDIDATE_SET_SCHEMA,
        "id": set_id,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "baseline_id": serde_json::Value::Null,
        "candidates": [{
            "schema_version": POLICY_CANDIDATE_SCHEMA,
            "id": candidate_id,
            "label": artifact.id,
            "kind": "mlx-lora.v1",
            "artifact": {"uri": format!("local-artifact://sha256/{hex}"), "digest": digest},
            "entrypoint": "policy.json",
            "metadata": {
                "source": {"kind": "training_artifact", "id": artifact.id},
                "parent_optimizer_run_id": artifact.producing_run_id,
                "base_model_id": artifact.base_model_id,
                "config_digest": artifact.config_digest,
                "dataset_digest": artifact.dataset_digest,
                "training_artifact": artifact,
            }
        }],
    });
    fs::write(
        root.join("candidate_set.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )
    .context("write candidate set manifest")?;
    Ok(manifest)
}

fn write_inline_source(staging: &Path, source: &EvalCandidateSource) -> Result<String> {
    if source.content.is_empty() {
        bail!("candidate source content must not be empty");
    }
    if source.content.len() > MAX_INLINE_SOURCE_BYTES {
        bail!(
            "candidate source exceeds the {} byte limit",
            MAX_INLINE_SOURCE_BYTES
        );
    }
    let file_name = source
        .file_name
        .as_deref()
        .unwrap_or(DEFAULT_SOURCE_FILE_NAME)
        .trim();
    if file_name.is_empty()
        || file_name == "."
        || file_name == ".."
        || file_name.len() > 128
        || !file_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        bail!("candidate file_name must be a simple file name using letters, numbers, '.', '_' or '-'");
    }
    fs::create_dir_all(staging)?;
    fs::write(staging.join(file_name), source.content.as_bytes())?;
    Ok(file_name.to_string())
}

fn copy_tree(origin: &Path, destination: &Path) -> Result<()> {
    if origin.is_file() {
        fs::create_dir_all(destination)?;
        let name = origin
            .file_name()
            .ok_or_else(|| anyhow!("policy source has no file name"))?;
        fs::copy(origin, destination.join(name))?;
        return Ok(());
    }
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(origin)? {
        let entry = entry?;
        let source = entry.path();
        if source.is_symlink() {
            continue; // a symlink is a way out of the staged bytes
        }
        let target = destination.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &target)?;
        } else {
            fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

/// Sorted relative path plus file bytes: the same rule the Python runner uses,
/// so a mismatch surfaces immediately as a refused run rather than silently.
fn digest_tree(root: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect(root, root, &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (relative, path) in files {
        hasher.update(relative.as_bytes());
        hasher.update([0u8]);
        hasher.update(Sha256::digest(fs::read(path)?));
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push((relative, path));
        }
    }
    Ok(())
}

fn freeze(root: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut stack = vec![root.to_path_buf()];
        let mut directories = Vec::new();
        while let Some(current) = stack.pop() {
            for entry in fs::read_dir(&current)? {
                let path = entry?.path();
                if path.is_dir() {
                    stack.push(path.clone());
                    directories.push(path);
                } else {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o444))?;
                }
            }
        }
        for directory in directories.into_iter().rev() {
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o555))?;
        }
        fs::set_permissions(root, fs::Permissions::from_mode(0o555))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_source_file_names_cannot_escape_the_candidate_store() {
        for file_name in ["", ".", "..", "../secrets", "policy/source.py", "policy\\source.py"] {
            let source = EvalCandidateSource {
                label: "test".into(),
                content: "pass\n".into(),
                file_name: Some(file_name.into()),
                entrypoint: None,
                kind: None,
                baseline: None,
                legacy_path: None,
            };
            let staging = std::env::temp_dir().join(format!("eval-inline-{}", uuid::Uuid::new_v4()));
            assert!(write_inline_source(&staging, &source).is_err());
            let _ = fs::remove_dir_all(staging);
        }
    }

    #[test]
    fn inline_source_is_written_as_app_owned_policy_data() {
        let staging = std::env::temp_dir().join(format!("eval-inline-{}", uuid::Uuid::new_v4()));
        let source = EvalCandidateSource {
            label: "baseline".into(),
            content: "def choose_actions(*args):\n    return []\n".into(),
            file_name: None,
            entrypoint: Some("policy:choose_actions".into()),
            kind: Some("python-code.craftax-choose-actions.v1".into()),
            baseline: Some(true),
            legacy_path: None,
        };
        assert_eq!(write_inline_source(&staging, &source).unwrap(), "policy.py");
        assert_eq!(fs::read_to_string(staging.join("policy.py")).unwrap(), source.content);
        let _ = fs::remove_dir_all(staging);
    }

    /// The Python runner recomputes this digest before it starts a container
    /// and refuses the run on a mismatch, so the two implementations must agree
    /// byte for byte. This value comes from `digest_of_tree` in
    /// `synth_optimizers.eval.models`; if it changes, one side has drifted.
    #[test]
    fn tree_digest_matches_the_python_runner() {
        let root = std::env::temp_dir().join(format!("eval-digest-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("policy.py"), "x\n").unwrap();
        fs::write(root.join("nested/helper.py"), "y\n").unwrap();
        let digest = digest_tree(&root).unwrap();
        let _ = fs::remove_dir_all(&root);
        assert_eq!(
            digest,
            "sha256:c2f99a007689fb434e533427fcb53f19d724a4be9498b8efb2fe1b518574a119"
        );
    }

    #[test]
    fn candidate_set_ids_are_constrained() {
        assert!(manifest_path("../etc/passwd").is_err());
        assert!(manifest_path("").is_err());
    }

    #[test]
    fn staging_a_training_artifact_retains_identity() {
        let isolated = std::env::temp_dir().join(format!(
            "synth-desktop-eval-artifact-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let _ = fs::remove_dir_all(&isolated);
        fs::create_dir_all(&isolated).unwrap();
        let previous = std::env::var_os(crate::instance::DATA_ROOT_ENV);
        std::env::set_var(crate::instance::DATA_ROOT_ENV, &isolated);
        let adapter = isolated.join("adapter");
        fs::create_dir_all(&adapter).unwrap();
        fs::write(adapter.join("adapter_config.json"), b"{\"rank\":8}").unwrap();
        fs::write(adapter.join("adapters.safetensors"), b"lora-weights").unwrap();
        let artifact = crate::training_artifacts::TrainingArtifact {
            schema_version: crate::training_artifacts::ARTIFACT_SCHEMA.into(),
            id: "run-9-terminal".into(),
            adapter_kind: "mlx-lora.v1".into(),
            base_model_id: crate::training_models::QWEN_TRAINING_MODEL_ID.into(),
            producing_run_id: "run-9".into(),
            producing_algorithm: "sft".into(),
            dataset_digest: Some("sha256:dataset".into()),
            config_digest: Some("sha256:config".into()),
            digest: Some("sha256:deadbeef".into()),
            path: Some(adapter.to_string_lossy().into_owned()),
            size_bytes: Some(12),
            integrity: "present".into(),
            compatible_inference: vec!["mlx-loopback".into()],
            created_at: "1".into(),
        };
        let staged = stage_training_artifact(&artifact).unwrap();
        assert_eq!(staged["candidates"][0]["metadata"]["parent_optimizer_run_id"], "run-9");
        assert_eq!(staged["candidates"][0]["metadata"]["base_model_id"], artifact.base_model_id);
        assert_eq!(staged["candidates"][0]["metadata"]["config_digest"], "sha256:config");
        assert_eq!(staged["candidates"][0]["kind"], "mlx-lora.v1");
        match previous {
            Some(value) => std::env::set_var(crate::instance::DATA_ROOT_ENV, value),
            None => std::env::remove_var(crate::instance::DATA_ROOT_ENV),
        }
        let _ = fs::remove_dir_all(isolated);
    }
}
