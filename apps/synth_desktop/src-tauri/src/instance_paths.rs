//! Where a Synth Desktop process finds its instance.
//!
//! This file is compiled twice on purpose: once as `crate::instance::paths`
//! inside the app, and once into every stdio MCP adapter in `src/bin/` through
//! `#[path = "../instance_paths.rs"]`. The adapters are deliberately built
//! without linking the app library, and before this file each of them carried
//! its own copy of the `SYNTH_DESKTOP_DATA_ROOT → ~/Library/Application
//! Support/Synth Desktop` fallback. Nine copies of one rule is nine places for
//! it to drift, and it did: none of them knew about the bundle descriptor.
//!
//! Rules for this file: no `crate::` paths, no dependencies beyond `serde`,
//! `serde_json`, and `dirs`, and nothing that needs Tauri. A test in
//! `crate::instance` fails if an adapter grows its own fallback again.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub const DATA_ROOT_ENV: &str = "SYNTH_DESKTOP_DATA_ROOT";

/// Written into the bundle at build time by the instance launcher
/// (`scripts/desktop-instance.sh cua-build`). A bundle that carries one knows
/// which instance it is without any environment, so a LaunchServices launch
/// (`open -b`, Finder, a relaunch) opens the same data root as the launcher.
pub const DESCRIPTOR_SCHEMA_VERSION: &str = "synth.desktop.instance-descriptor.v1";
pub const DESCRIPTOR_RELATIVE_PATH: &str = "Contents/Resources/instance.json";

const CANONICAL_DATA_DIR_NAME: &str = "Synth Desktop";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct InstanceDescriptor {
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    pub instance_id: String,
    #[serde(default)]
    pub instance_root: Option<PathBuf>,
    #[serde(default)]
    pub config_path: Option<PathBuf>,
    pub data_root: PathBuf,
    pub bundle_id: String,
    #[serde(default)]
    pub release_line: Option<String>,
    #[serde(default)]
    pub source_revision: Option<String>,
    #[serde(default)]
    pub generated_at: Option<String>,
}

/// Walk up from `…/Foo.app/Contents/MacOS/Foo` (or any executable nested
/// deeper inside the bundle, such as an adapter) to `…/Foo.app`.
pub fn bundle_root(executable: &Path) -> Option<PathBuf> {
    let mut current = executable;
    while let Some(parent) = current.parent() {
        if parent.extension().and_then(|ext| ext.to_str()) == Some("app") {
            return Some(parent.to_path_buf());
        }
        current = parent;
    }
    None
}

/// The bundle this process runs from, if it runs from one at all. A `cargo
/// run` or `tauri dev` binary is not in a bundle and gets `None`.
pub fn running_bundle_root() -> Option<PathBuf> {
    bundle_root(&env::current_exe().ok()?)
}

pub fn descriptor_path(bundle: &Path) -> PathBuf {
    bundle.join(DESCRIPTOR_RELATIVE_PATH)
}

/// Parse a descriptor, refusing anything that is not the schema we know.
/// `Ok(None)` means there is no descriptor; `Err` means there is one and it is
/// unusable, which the app treats as a refusal rather than a fallback — a
/// corrupt descriptor must never quietly become the canonical profile.
pub fn read_descriptor(path: &Path) -> Result<Option<InstanceDescriptor>, String> {
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let descriptor: InstanceDescriptor = serde_json::from_slice(&raw)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if descriptor.schema_version != DESCRIPTOR_SCHEMA_VERSION {
        return Err(format!(
            "{}: schemaVersion {:?} is not {DESCRIPTOR_SCHEMA_VERSION:?}",
            path.display(),
            descriptor.schema_version
        ));
    }
    if descriptor.instance_id.trim().is_empty()
        || descriptor.bundle_id.trim().is_empty()
        || descriptor.data_root.as_os_str().is_empty()
    {
        return Err(format!(
            "{}: instance_id, bundle_id, and data_root are required",
            path.display()
        ));
    }
    Ok(Some(descriptor))
}

/// The descriptor of the bundle this process runs from, if any.
pub fn running_bundle_descriptor() -> Result<Option<InstanceDescriptor>, String> {
    match running_bundle_root() {
        Some(bundle) => read_descriptor(&descriptor_path(&bundle)),
        None => Ok(None),
    }
}

/// The installed-app location. Only reached when nothing names an instance.
pub fn canonical_data_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(CANONICAL_DATA_DIR_NAME)
}

/// Product-owned durable data for the instance this process belongs to:
/// the environment if the launcher set it, else the bundle descriptor, else
/// the canonical installed-app root.
///
/// Adapters run as children of Codex inside the app's environment, so the env
/// answers first; a descriptor-only launch (no env) still finds its instance
/// because the adapters live inside the same bundle.
pub fn data_root() -> PathBuf {
    if let Some(root) = env::var_os(DATA_ROOT_ENV).filter(|value| !value.is_empty()) {
        return PathBuf::from(root);
    }
    if let Ok(Some(descriptor)) = running_bundle_descriptor() {
        return descriptor.data_root;
    }
    canonical_data_root()
}

/// Loopback IPC descriptor for an adapter: the first of `env_names` that is
/// set wins; otherwise `file_name` inside the instance data root.
pub fn ipc_connection_file(env_names: &[&str], file_name: &str) -> PathBuf {
    for name in env_names {
        if let Ok(path) = env::var(name) {
            if !path.trim().is_empty() {
                return PathBuf::from(path);
            }
        }
    }
    data_root().join(file_name)
}
