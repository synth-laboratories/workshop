//! Packaged-resource discovery for the optional Python MLX/Laguna sidecar.
//!
//! The desktop product runtime is Rust-owned. This module intentionally has no
//! process supervisor or HTTP compatibility proxy for `services/local-runtime`.

use anyhow::{anyhow, Context, Result};
use std::{
    env,
    path::{Path, PathBuf},
};

pub(crate) fn workshop_root() -> Result<PathBuf> {
    if let Some(root) = env::var_os("SYNTH_WORKSHOP_ROOT") {
        return validate_resource_root(PathBuf::from(root), "SYNTH_WORKSHOP_ROOT");
    }

    let executable = env::current_exe().context("resolve Synth Desktop executable path")?;
    for candidate in resource_candidates(&executable) {
        if is_resource_root(&candidate) {
            return Ok(candidate);
        }
    }

    // Development fallback: src-tauri -> synth_desktop -> apps -> workshop.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_owned)
        .filter(|root| is_resource_root(root))
        .ok_or_else(|| {
            anyhow!(
                "Synth Desktop Laguna resources are missing. Reinstall the app or set \
             SYNTH_WORKSHOP_ROOT to a checkout containing services/laguna-daemon/laguna_daemon."
            )
        })
}

fn is_resource_root(root: &Path) -> bool {
    root.join("services/laguna-daemon/laguna_daemon").is_dir()
}

fn validate_resource_root(root: PathBuf, source: &str) -> Result<PathBuf> {
    if is_resource_root(&root) {
        Ok(root)
    } else {
        Err(anyhow!(
            "{source}={} does not contain the required Laguna sidecar resources",
            root.display()
        ))
    }
}

fn resource_candidates(executable: &Path) -> Vec<PathBuf> {
    let executable_dir = executable.parent().unwrap_or(Path::new("."));
    let mut candidates = vec![
        executable_dir.to_owned(),
        executable_dir.join("resources"),
        executable_dir.join("Resources"),
    ];
    if let Some(parent) = executable_dir.parent() {
        candidates.push(parent.join("Resources"));
        candidates.push(parent.join("resources"));
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_resource_directory_is_considered() {
        let candidates = resource_candidates(Path::new(
            "/Applications/Synth Desktop.app/Contents/MacOS/synth-desktop",
        ));
        assert!(candidates.contains(&PathBuf::from(
            "/Applications/Synth Desktop.app/Contents/Resources"
        )));
    }

    #[test]
    fn desktop_never_packages_the_python_product_runtime() {
        let runtime_source = include_str!("runtime.rs");
        let lib_source = include_str!("lib.rs");
        let config = include_str!("../tauri.conf.json");
        let legacy_module = ["synth", "_local", "_runtime"].concat();
        let legacy_resource = ["services/", "local-runtime", "/src"].concat();
        let legacy_command = ["runtime", "_request"].concat();
        assert!(!runtime_source.contains(&legacy_module));
        assert!(!lib_source.contains(&legacy_command));
        assert!(!config.contains(&legacy_resource));
    }
}
