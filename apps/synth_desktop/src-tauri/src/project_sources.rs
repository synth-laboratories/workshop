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
    #[cfg(test)]
    {
        // Tests supply real, isolated config files. Never inherit operator grants
        // or environment roots, and never replace the production root predicate.
        let entries = TEST_SOURCE_CONFIG.try_with(|path| synth_config::project_source_settings_at(path))
            .ok().transpose()?.map(|settings| settings.entries).unwrap_or_default();
        resolve_entries(&entries, capability, &[])
    }
    #[cfg(not(test))]
    {
    let settings = synth_config::project_source_settings()?;
    let containers = env::var_os("SYNTH_CONTAINER_SOURCE_ROOTS");
    let roots = match capability {
        Capability::Containers => containers,
        // Preserve the explicitly configured legacy environment alias only.
        Capability::Recipes => env::var_os("SYNTH_RECIPE_SOURCE_ROOTS").or(containers),
    };
    let environment: Vec<_> = roots
        .as_deref()
        .map(env::split_paths)
        .into_iter()
        .flatten()
        .filter(|path| !path.as_os_str().is_empty())
        .collect();
    resolve_entries(&settings.entries, capability, &environment)
    }
}

#[cfg(test)]
tokio::task_local! { pub(crate) static TEST_SOURCE_CONFIG: PathBuf; }

#[cfg(test)]
pub(crate) fn test_grant(config: &Path, root: &Path, containers: bool, recipes: bool) {
    synth_config::begin_project_source_grant_at(config, ProjectSourceEntry {
        path: root.display().to_string(), containers, recipes,
    }).unwrap();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn capabilities_are_separate_and_no_roots_means_no_ambient_grant() {
        let directory = tempfile::tempdir().unwrap();
        let entries = vec![ProjectSourceEntry {
            path: directory.path().display().to_string(),
            containers: true,
            recipes: false,
        }];
        assert_eq!(
            resolve_entries(&entries, Capability::Containers, &[])
                .unwrap()
                .len(),
            1
        );
        assert!(resolve_entries(&entries, Capability::Recipes, &[])
            .unwrap()
            .is_empty());
        assert!(resolve_entries(&[], Capability::Containers, &[])
            .unwrap()
            .is_empty());
        assert_eq!(
            resolve_entries(&[], Capability::Recipes, &[directory.path().to_owned()])
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn removed_and_missing_sources_have_no_authority() {
        let directory = tempfile::tempdir().unwrap();
        let manifest = directory.path().join("workshop.containers.toml");
        fs::write(&manifest, "# declaration fixture").unwrap();
        let roots = vec![directory.path().canonicalize().unwrap()];
        assert!(require_manifest_in(&manifest, &roots).is_ok());
        assert!(require_manifest_in(&manifest, &[]).is_err());
        let missing = vec![ProjectSourceEntry {
            path: directory.path().join("missing").display().to_string(),
            containers: true,
            recipes: true,
        }];
        assert!(resolve_entries(&missing, Capability::Containers, &[])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn broad_roots_and_relative_admission_refuse() {
        for path in [
            "/",
            "/Users",
            "/private/tmp",
            "/private/var",
            "/usr",
            "/home/test",
        ] {
            assert!(validate_root(Path::new(path), Some(Path::new("/home/test"))).is_err());
        }
        assert!(canonical_project_root(".").is_err());
        assert!(canonical_project_root("").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_aliases_deduplicate_but_manifest_escape_refuses() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        fs::create_dir(&root).unwrap();
        let alias = directory.path().join("alias");
        symlink(&root, &alias).unwrap();
        let roots = resolve_entries(&[], Capability::Containers, &[root.clone(), alias]).unwrap();
        assert_eq!(roots.len(), 1);
        let outside = directory.path().join("outside.toml");
        fs::write(&outside, "# fixture").unwrap();
        let manifest = root.join("workshop.containers.toml");
        symlink(&outside, &manifest).unwrap();
        assert!(require_manifest_in(&manifest, &[root.canonicalize().unwrap()]).is_err());
    }
}
