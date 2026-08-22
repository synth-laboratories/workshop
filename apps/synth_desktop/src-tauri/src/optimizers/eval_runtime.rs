//! Desktop-owned Eval runtime pin.
//!
//! Eval is not a second Python distribution. It consumes the same 0.2.19
//! `synth-optimizers` install GEPA uses, but it has its own manifest, digest,
//! and About row so a missing pin is visible instead of resolving from a
//! developer `.venv`.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use super::manager::OptimizerSidecarVersion;
use crate::contract::runtimes::EVAL;

pub const EVAL_RUNTIME_SCHEMA: &str = "synth.eval-runtime.v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvalRuntimeManifest {
    pub schema_version: String,
    pub package: String,
    pub version: String,
    pub digest: String,
    pub python: Option<String>,
    pub sidecar_path: String,
    pub provisioned_at: String,
}

impl EvalRuntimeManifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != EVAL_RUNTIME_SCHEMA {
            bail!(
                "unsupported eval runtime schema {} (expected {EVAL_RUNTIME_SCHEMA})",
                self.schema_version
            );
        }
        if self.version != EVAL.official && self.version != EVAL.dev {
            bail!(
                "eval runtime {} is not the pinned {} / {}",
                self.version,
                EVAL.official,
                EVAL.dev
            );
        }
        if !EVAL.meets_floor(&self.version) {
            bail!(
                "eval runtime {} is below the floor {}",
                self.version,
                EVAL.min_supported
            );
        }
        Ok(())
    }
}

pub fn manifest_path() -> PathBuf {
    crate::instance::data_root()
        .join("runtime")
        .join("eval")
        .join("manifest.json")
}

pub fn load_manifest() -> Result<Option<EvalRuntimeManifest>> {
    let path = manifest_path();
    if !path.is_file() {
        return Ok(None);
    }
    let manifest: EvalRuntimeManifest = serde_json::from_slice(&fs::read(&path)?)
        .with_context(|| format!("decode eval runtime manifest {}", path.display()))?;
    manifest.validate()?;
    Ok(Some(manifest))
}

pub fn installed_version() -> Option<String> {
    load_manifest().ok().flatten().map(|manifest| manifest.version)
}

pub fn provisioned_python() -> Option<PathBuf> {
    load_manifest()
        .ok()
        .flatten()
        .and_then(|manifest| manifest.python)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

pub fn provision_from_sidecar(
    sidecar: &OptimizerSidecarVersion,
) -> Result<EvalRuntimeManifest> {
    sidecar_to_manifest(sidecar).and_then(write_manifest)
}

pub fn provision_from_disk() -> Result<EvalRuntimeManifest> {
    let home = crate::instance::state_root().join("optimizers");
    let selected = fs::read_to_string(home.join("selected_version"))
        .context("eval provisioning requires an installed Optimizers sidecar")?
        .trim()
        .to_string();
    if selected.is_empty() {
        bail!("eval provisioning requires a selected sidecar version");
    }
    let dir = home.join("versions").join(&selected);
    let sidecar_manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(dir.join("manifest.json"))
            .with_context(|| format!("read sidecar manifest {}", dir.display()))?,
    )?;
    let sidecar = OptimizerSidecarVersion {
        version: sidecar_manifest["version"]
            .as_str()
            .unwrap_or(&selected)
            .to_string(),
        digest: sidecar_manifest["digest"]
            .as_str()
            .ok_or_else(|| anyhow!("sidecar manifest omitted digest"))?
            .to_string(),
        signature: sidecar_manifest["signature"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        algorithm_id: sidecar_manifest["algorithmId"]
            .as_str()
            .unwrap_or("gepa")
            .to_string(),
        algorithm_version: sidecar_manifest["algorithmVersion"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        recipe_schema_version: sidecar_manifest["recipeSchemaVersion"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        selected: true,
        path: dir.display().to_string(),
    };
    provision_from_sidecar(&sidecar)
}

fn sidecar_to_manifest(sidecar: &OptimizerSidecarVersion) -> Result<EvalRuntimeManifest> {
    if !EVAL.meets_floor(&sidecar.version) {
        bail!(
            "sidecar {} cannot provision Eval (floor {})",
            sidecar.version,
            EVAL.min_supported
        );
    }
    let python = python_beside_sidecar(&PathBuf::from(&sidecar.path));
    Ok(EvalRuntimeManifest {
        schema_version: EVAL_RUNTIME_SCHEMA.into(),
        package: EVAL.package.into(),
        version: sidecar.version.clone(),
        digest: sidecar.digest.clone(),
        python: python
            .as_ref()
            .map(|path| path.display().to_string()),
        sidecar_path: sidecar.path.clone(),
        provisioned_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    })
}

fn python_beside_sidecar(sidecar_dir: &std::path::Path) -> Option<PathBuf> {
    let runtime = sidecar_dir.join("runtime");
    [
        runtime.join("bin/python3"),
        runtime.join("bin/python"),
        runtime.join("Scripts/python.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn write_manifest(manifest: EvalRuntimeManifest) -> Result<EvalRuntimeManifest> {
    manifest.validate()?;
    let path = manifest_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create eval runtime directory {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("write eval runtime manifest {}", path.display()))?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_eval_runtime_is_managed_at_0_2_19() {
        assert!(EVAL.provisioned_by_desktop);
        assert_eq!(EVAL.official, "0.2.19");
        assert_eq!(EVAL.min_supported, "0.2.19");
        assert!(EVAL.meets_floor("0.2.19"));
        assert!(!EVAL.meets_floor("0.2.14"));
    }

    #[test]
    fn manifest_round_trip_retains_digest_and_python() {
        let root = tempfile::tempdir().unwrap();
        let runtime = root.path().join("runtime/bin");
        fs::create_dir_all(&runtime).unwrap();
        let python = runtime.join("python3");
        fs::write(&python, b"#!/bin/sh\nexit 0\n").unwrap();
        let sidecar = OptimizerSidecarVersion {
            version: "0.2.19".into(),
            digest: "abc123".into(),
            signature: "sig".into(),
            algorithm_id: "gepa".into(),
            algorithm_version: "synth-optimizers-0.2.19".into(),
            recipe_schema_version: "gepa.recipe.v1".into(),
            selected: true,
            path: root.path().display().to_string(),
        };
        let runtime = root.path().join("runtime/bin");
        fs::create_dir_all(&runtime).unwrap();
        let python = runtime.join("python3");
        fs::write(&python, b"#!/bin/sh\nexit 0\n").unwrap();
        let manifest = sidecar_to_manifest(&sidecar).unwrap();
        assert_eq!(manifest.version, "0.2.19");
        assert_eq!(manifest.digest, "abc123");
        assert_eq!(
            manifest.python.as_deref(),
            Some(python.to_str().unwrap())
        );
        manifest.validate().unwrap();
    }

    #[test]
    fn a_stale_sidecar_cannot_provision_eval() {
        let sidecar = OptimizerSidecarVersion {
            version: "0.2.14".into(),
            digest: "old".into(),
            signature: "sig".into(),
            algorithm_id: "gepa".into(),
            algorithm_version: "synth-optimizers-0.2.14".into(),
            recipe_schema_version: "gepa.recipe.v1".into(),
            selected: true,
            path: "/tmp/sidecar".into(),
        };
        let error = sidecar_to_manifest(&sidecar).unwrap_err().to_string();
        assert!(error.contains("0.2.14"), "{error}");
        assert!(error.contains("floor"), "{error}");
    }
}
