//! Executable project-source authority. Conversation read/write attachments
//! are deliberately not inputs. Native picker admission owns persisted grants.

use crate::synth_config::{self, ProjectSourceEntry};
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::{
    collections::HashSet,
    env,
    path::{Path, PathBuf},
};

mod approval;
pub mod commands;
mod inspection;
pub mod requests;
pub use approval::{approve, ProjectSourceApproval};
pub use inspection::{catalog, ProjectSourceCatalog};

async fn admit_picked_root(
    db: &std::sync::Arc<crate::storage::Database>,
    path: &str,
    containers: bool,
    recipes: bool,
) -> Result<ProjectSourceCatalog> {
    let _resolution = requests::RESOLUTION.lock().await;
    if !containers && !recipes {
        bail!("choose containers, recipes, or both");
    }
    let root = canonical_project_root(path)?;
    let inspection = inspection::inspect(&root);
    if inspection.status != "valid" {
        bail!(
            "{}: {}",
            inspection.code.as_deref().unwrap_or("source_invalid"),
            inspection.message.as_deref().unwrap_or("invalid source")
        );
    }
    let change = synth_config::begin_project_source_grant(ProjectSourceEntry {
        path: root.display().to_string(),
        containers,
        recipes,
    })?;
    if let Err(error) = requests::audit(db, "project_source.approved", serde_json::json!({
        "path": root.display().to_string(), "containers": inspection.containers,
        "recipes": inspection.recipes, "grant": { "containers": containers, "recipes": recipes },
        "method": "native_picker"
    })).await { return Err(compensate(change, error)); }
    catalog()
}

async fn remove_root(
    db: &std::sync::Arc<crate::storage::Database>,
    path: &str,
) -> Result<ProjectSourceCatalog> {
    let _resolution = requests::RESOLUTION.lock().await;
    if path.trim().is_empty() {
        bail!("a project source path is required");
    }
    synth_config::forget_project_source(path)?;
    requests::audit(
        db,
        "project_source.removed",
        serde_json::json!({ "path": path.trim() }),
    )
    .await
    .context("source was revoked, but its journal event could not be recorded")?;
    catalog()
}

fn compensate(change: synth_config::ProjectSourceChange, error: anyhow::Error) -> anyhow::Error {
    match change.rollback() {
        Ok(()) => error.context("source change was rolled back because its durable decision could not be recorded"),
        Err(rollback) => anyhow!("source decision failed: {error}; rollback failed: {rollback}; inspect current source permissions before retrying"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    Containers,
    Recipes,
}

impl Capability {
    fn enabled(self, entry: &ProjectSourceEntry) -> bool {
        match self {
            Self::Containers => entry.containers,
            Self::Recipes => entry.recipes,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RootOrigin {
    Configured,
    Environment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRoot {
    pub path: PathBuf,
    pub origin: RootOrigin,
}

/// Reject ambient machine-wide roots even when they arrive through an alias.
/// A missing path is not admissible, but persisted missing grants remain visible
/// to Settings and may be removed without canonicalizing them.
pub fn canonical_project_root(raw: &str) -> Result<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() || !Path::new(raw).is_absolute() {
        bail!("project source path must be nonempty and absolute");
    }
    let root = Path::new(raw)
        .canonicalize()
        .context("project source path is unavailable")?;
    if !root.is_dir() {
        bail!("project source must be a directory");
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok());
    validate_root(&root, home.as_deref())?;
    Ok(root)
}

fn validate_root(root: &Path, home: Option<&Path>) -> Result<()> {
    let broad = [
        "/",
        "/Users",
        "/home",
        "/Applications",
        "/Library",
        "/System",
        "/Volumes",
        "/private",
        "/private/tmp",
        "/private/var",
        "/private/etc",
        "/tmp",
        "/var",
        "/etc",
        "/usr",
        "/opt",
        "/bin",
        "/sbin",
    ];
    if root.parent().is_none()
        || home == Some(root)
        || broad.iter().any(|path| root == Path::new(path))
    {
        bail!("project source must be a specific project folder, not a machine-wide root");
    }
    Ok(())
}

pub fn resolve_roots(capability: Capability) -> Result<Vec<ResolvedRoot>> {

}


fn resolve_entries(
    entries: &[ProjectSourceEntry],
    capability: Capability,
    environment: &[PathBuf],
) -> Result<Vec<ResolvedRoot>> {
    let mut seen = HashSet::new();
    let mut roots = Vec::new();
    let requested = entries
        .iter()
        .filter(|entry| capability.enabled(entry))
        .map(|entry| (PathBuf::from(&entry.path), RootOrigin::Configured))
        .chain(
            environment
                .iter()
                .cloned()
                .map(|path| (path, RootOrigin::Environment)),
        );
    for (path, origin) in requested {
        // Missing/unmounted roots have no current execution authority. Other
        // malformed roots fail closed rather than invoking an ambient fallback.
        match std::fs::metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let canonical = canonical_project_root(
            path.to_str()
                .ok_or_else(|| anyhow!("project source path must be UTF-8"))?,
        )?;
        if seen.insert(canonical.clone()) {
            roots.push(ResolvedRoot {
                path: canonical,
                origin,
            });
        }
    }
    Ok(roots)
}

pub fn discovery_roots(capability: Capability) -> Result<Vec<PathBuf>> {
    Ok(resolve_roots(capability)?
        .into_iter()
        .map(|root| root.path)
        .collect())
}

pub fn require_manifest(manifest: &Path, capability: Capability) -> Result<PathBuf> {
    require_manifest_in(manifest, &discovery_roots(capability)?)
}

fn require_manifest_in(manifest: &Path, roots: &[PathBuf]) -> Result<PathBuf> {
    let canonical = manifest
        .canonicalize()
        .context("project source manifest is unavailable")?;
    if !canonical.is_file() || !roots.iter().any(|root| canonical.starts_with(root)) {
        bail!(
            "launch_source_root_not_approved: manifest {} requires a project-source grant",
            manifest.display()
        );
    }
    Ok(canonical)
}

