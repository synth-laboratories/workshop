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

/// Why the Eval runtime is unusable, as a code a caller can act on.
///
/// "The local Optimizers runtime is not installed" used to be the answer to
/// every one of these, including cases where the runtime was installed and
/// working. A wrong diagnosis costs more than no diagnosis: it sends someone
/// to reinstall a package that was never the problem.
pub mod fault {
    /// No Optimizers sidecar is installed or selected.
    pub const PLUGIN_NOT_INSTALLED: &str = "plugin_not_installed";
    /// A sidecar is installed, but Eval has no manifest pinning it.
    pub const NOT_PROVISIONED: &str = "eval_runtime_not_provisioned";
    /// The manifest names an interpreter that is not there.
    pub const INTERPRETER_MISSING: &str = "eval_runtime_interpreter_missing";
    /// The interpreter exists but cannot import `synth_optimizers.eval`.
    pub const IMPORT_FAILED: &str = "eval_runtime_import_failed";
    /// The manifest and the installed sidecar disagree about the digest.
    pub const DIGEST_MISMATCH: &str = "eval_runtime_digest_mismatch";
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvalRuntimeFault {
    pub code: &'static str,
    pub message: String,
}

impl EvalRuntimeFault {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for EvalRuntimeFault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for EvalRuntimeFault {}

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

/// Provision Eval from a freshly installed sidecar and prove it works.
///
/// Installation is only complete when the thing it installed can be used, so
/// this runs the checks in the order that a failure is cheapest to explain:
/// the interpreter exists, it can import `synth_optimizers.eval`, the manifest
/// writes, and the written manifest resolves back to the same interpreter and
/// digest. A caller that treats Eval as an optional sub-capability can report
/// the returned fault without failing the whole plugin install.
pub fn provision_and_verify(
    sidecar: &OptimizerSidecarVersion,
) -> std::result::Result<EvalRuntimeManifest, EvalRuntimeFault> {
    let candidate = sidecar_to_manifest(sidecar)
        .map_err(|error| EvalRuntimeFault::new(fault::NOT_PROVISIONED, error.to_string()))?;
    let python = candidate
        .python
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .ok_or_else(|| {
            EvalRuntimeFault::new(
                fault::INTERPRETER_MISSING,
                format!(
                    "optimizer sidecar {} ships no versioned interpreter under {}/runtime",
                    sidecar.version, sidecar.path
                ),
            )
        })?;
    verify_import(&python)?;
    let written = write_manifest(candidate)
        .map_err(|error| EvalRuntimeFault::new(fault::NOT_PROVISIONED, error.to_string()))?;
    // Read it back rather than trusting what we just wrote: a manifest that
    // does not round-trip is the failure this whole check exists to catch.
    let reloaded = load_manifest()
        .map_err(|error| EvalRuntimeFault::new(fault::NOT_PROVISIONED, error.to_string()))?
        .ok_or_else(|| {
            EvalRuntimeFault::new(
                fault::NOT_PROVISIONED,
                "the eval runtime manifest disappeared immediately after it was written",
            )
        })?;
    if reloaded.digest != sidecar.digest {
        return Err(EvalRuntimeFault::new(
            fault::DIGEST_MISMATCH,
            format!(
                "eval runtime manifest pins digest {} but the installed sidecar is {}",
                reloaded.digest, sidecar.digest
            ),
        ));
    }
    if reloaded.python.as_deref() != python.to_str() {
        return Err(EvalRuntimeFault::new(
            fault::INTERPRETER_MISSING,
            "the eval runtime manifest resolves to a different interpreter than the one verified",
        ));
    }
    Ok(written)
}

/// Prove this interpreter can actually run Eval.
///
/// An executable file at the pinned path is not evidence: a partially
/// extracted install has the interpreter and not the package, which is exactly
/// the state that used to be reported as "runtime is not installed".
pub fn verify_import(python: &std::path::Path) -> std::result::Result<(), EvalRuntimeFault> {
    let output = std::process::Command::new(python)
        .arg("-c")
        .arg("import synth_optimizers.eval")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| {
            EvalRuntimeFault::new(
                fault::INTERPRETER_MISSING,
                format!("{} could not be executed: {error}", python.display()),
            )
        })?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    let detail = detail.trim();
    // The traceback can be long and carries local paths; the last line names
    // the module that failed, which is the part worth reporting.
    let summary = detail.lines().last().unwrap_or("no diagnostic output");
    Err(EvalRuntimeFault::new(
        fault::IMPORT_FAILED,
        format!(
            "{} cannot import synth_optimizers.eval: {summary}",
            python.display()
        ),
    ))
}

/// The Eval interpreter to run, or a structured reason there is none.
///
/// Provisions lazily from an installed sidecar when the manifest is missing,
/// which is what makes Eval usable right after a plugin install that predates
/// [`provision_and_verify`].
pub fn ready_python() -> std::result::Result<PathBuf, EvalRuntimeFault> {
    let manifest = match load_manifest() {
        Ok(Some(manifest)) => manifest,
        Ok(None) | Err(_) => provision_from_disk().map_err(|error| {
            let message = error.to_string();
            // `provision_from_disk` reads the selected version first, so a
            // missing selection is a missing plugin, not a missing Eval pin.
            let code = if message.contains("installed Optimizers sidecar")
                || message.contains("selected sidecar version")
            {
                fault::PLUGIN_NOT_INSTALLED
            } else {
                fault::NOT_PROVISIONED
            };
            EvalRuntimeFault::new(code, message)
        })?,
    };
    let python = manifest
        .python
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .ok_or_else(|| {
            EvalRuntimeFault::new(
                fault::INTERPRETER_MISSING,
                format!(
                    "the eval runtime manifest at {} names no existing interpreter",
                    manifest_path().display()
                ),
            )
        })?;
    Ok(python)
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

    fn sidecar_at(path: &std::path::Path, version: &str) -> OptimizerSidecarVersion {
        OptimizerSidecarVersion {
            version: version.into(),
            digest: "sha256:abc123".into(),
            signature: "sig".into(),
            algorithm_id: "gepa".into(),
            algorithm_version: format!("synth-optimizers-{version}"),
            recipe_schema_version: "gepa.recipe.v1".into(),
            selected: true,
            path: path.display().to_string(),
        }
    }

    #[cfg(unix)]
    fn executable(path: &std::path::Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A sidecar that ships no versioned interpreter is not "the plugin is not
    /// installed" -- it is installed, and incomplete. The codes differ because
    /// the fixes differ.
    #[test]
    fn a_sidecar_without_an_interpreter_reports_interpreter_missing() {
        let root = tempfile::tempdir().unwrap();
        let error = provision_and_verify(&sidecar_at(root.path(), EVAL.official)).unwrap_err();
        assert_eq!(error.code, fault::INTERPRETER_MISSING);
        assert!(error.message.contains("versioned interpreter"), "{error}");
    }

    #[test]
    fn a_sidecar_below_the_floor_cannot_provision_eval() {
        let root = tempfile::tempdir().unwrap();
        let error = provision_and_verify(&sidecar_at(root.path(), "0.2.14")).unwrap_err();
        assert_eq!(error.code, fault::NOT_PROVISIONED);
        assert!(error.message.contains("floor"), "{error}");
    }

    /// An executable at the pinned path proves nothing until it imports.
    #[cfg(unix)]
    #[test]
    fn an_interpreter_that_cannot_import_eval_is_reported_as_an_import_failure() {
        let root = tempfile::tempdir().unwrap();
        let python = root.path().join("bin/python3");
        executable(
            &python,
            "#!/bin/sh\necho \"ModuleNotFoundError: No module named 'synth_optimizers'\" >&2\nexit 1\n",
        );
        let error = verify_import(&python).unwrap_err();
        assert_eq!(error.code, fault::IMPORT_FAILED);
        assert!(error.message.contains("synth_optimizers"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn an_interpreter_that_imports_eval_verifies() {
        let root = tempfile::tempdir().unwrap();
        let python = root.path().join("bin/python3");
        executable(&python, "#!/bin/sh\nexit 0\n");
        assert!(verify_import(&python).is_ok());
    }

    #[test]
    fn a_missing_interpreter_file_is_not_reported_as_an_import_failure() {
        let error = verify_import(std::path::Path::new("/definitely/not/here")).unwrap_err();
        assert_eq!(error.code, fault::INTERPRETER_MISSING);
    }
}
