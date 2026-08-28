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

