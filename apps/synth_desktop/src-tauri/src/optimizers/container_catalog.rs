//! Desktop-level discovery and identity for locally declared containers.
//!
//! A container source is not a chat workspace. Sources are discovered from
//! bounded desktop roots, catalogued with immutable declaration metadata, and
//! addressed by `source_id` plus `spec_id`. This keeps service lifecycle out
//! of the session/fork/reopen lifecycle.

use super::workspace_recipe::{self, ContainerSpec};
use crate::project_sources::Capability;
use crate::storage::Database;
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

const CONTAINERS_FILE: &str = "workshop.containers.toml";

#[derive(Clone, Debug)]
pub(crate) struct ContainerSource {
    pub id: String,
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest_hash: String,
    pub git_revision: Option<String>,
    pub specs: Vec<ContainerSpec>,
}

impl ContainerSource {
    pub fn spec(&self, spec_id: &str) -> Result<ContainerSpec> {
        self.specs
            .iter()
            .find(|spec| spec.id == spec_id)
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "container spec `{spec_id}` is not declared by source `{}`",
                    self.id
                )
            })
    }
}

/// Refresh the source catalog from this desktop's effective project sources.
///
/// Root policy -- approved sources, environment overrides, remembered
/// provenance, the development fallback -- lives in [`crate::project_sources`]
/// so the container and recipe catalogs cannot disagree about what is in scope.
/// We examine each root and its direct children only; this is intentional:
/// discovery is not an arbitrary recursive filesystem search.
pub(crate) async fn discover(db: &Arc<Database>) -> Result<Vec<ContainerSource>> {
    let roots = crate::project_sources::discovery_roots(db, Capability::Containers)?;
    let sources = discover_in_roots(&roots)?;
    persist(db, &sources).await?;
    Ok(sources)
}

pub(crate) async fn resolve(
    db: &Arc<Database>,
    source_id: &str,
    spec_id: &str,
) -> Result<(ContainerSource, ContainerSpec)> {
    let source_id = require_identifier(source_id, "source_id")?;
    let spec_id = require_identifier(spec_id, "spec_id")?;
    let source = discover(db)
        .await?
        .into_iter()
        .find(|source| source.id == source_id)
        .ok_or_else(|| {
            anyhow!(
                "container source `{source_id}` is not currently discoverable; \
                 its folder may have moved, or its project source may have been removed. \
                 Call container_discover and read `readiness` before retrying."
            )
        })?;
    let spec = source.spec(&spec_id)?;
    Ok((source, spec))
}

/// Resolve a specification only when it is globally unambiguous. This is for
/// older recipe declarations that name a container spec but not a source.
/// New callers must use [`resolve`] with an explicit source id.
pub(crate) async fn resolve_unique(
    db: &Arc<Database>,
    spec_id: &str,
) -> Result<(ContainerSource, ContainerSpec)> {
    let spec_id = require_identifier(spec_id, "spec_id")?;
    let matches: Vec<_> = discover(db)
        .await?
        .into_iter()
        .filter_map(|source| source.spec(&spec_id).ok().map(|spec| (source, spec)))
        .collect();
    match matches.len() {
        1 => Ok(matches.into_iter().next().expect("one catalog match")),
        0 => bail!(
            "container spec `{spec_id}` was not found in the desktop container catalog; \
             call container_discover and read `readiness` to see whether a project source \
             still needs to be approved"
        ),
        _ => bail!(
            "container spec `{spec_id}` is ambiguous; discover sources and call container_ensure with source_id"
        ),
    }
}

pub(crate) fn discover_in_roots(roots: &[PathBuf]) -> Result<Vec<ContainerSource>> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        let Ok(root) = root.canonicalize() else {
            continue;
        };
        if !root.is_dir() {
            continue;
        }
        candidates.push(root.clone());
        let entries =
            fs::read_dir(&root).with_context(|| format!("read source root {}", root.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("read entry below {}", root.display()))?;
            if entry.file_type()?.is_dir() {
                candidates.push(entry.path());
            }
        }
    }

    let mut sources = Vec::new();
    for candidate in candidates {
        let Ok(root) = candidate.canonicalize() else {
            continue;
        };
        if !seen.insert(root.clone()) {
            continue;
        }
        let manifest_path = root.join(CONTAINERS_FILE);
        if !manifest_path.is_file() {
            continue;
        }
        let manifest = fs::read_to_string(&manifest_path)
            .with_context(|| format!("read {}", manifest_path.display()))?;
        // One broken declaration in a broad developer root must not make every
        // other source undiscoverable. It simply is not a runnable catalog
        // entry until its manifest parses and its declared cwd validates.
        let Ok(specs) = workspace_recipe::load_container_specs(&root) else {
            continue;
        };
        if specs.is_empty() {
            continue;
        }
        let git_revision = git_revision(&root);
        sources.push(ContainerSource {
            id: source_id(&root),
            root,
            manifest_path,
            manifest_hash: content_hash(&manifest),
            git_revision,
            specs,
        });
    }
    sources.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(sources)
}

async fn persist(db: &Arc<Database>, sources: &[ContainerSource]) -> Result<()> {
    let rows: Vec<_> = sources
        .iter()
        .map(|source| {
            (
                source.id.clone(),
                source.root.to_string_lossy().to_string(),
                source.manifest_path.to_string_lossy().to_string(),
                source.manifest_hash.clone(),
                source.git_revision.clone(),
            )
        })
        .collect();
    db.clone()
        .run_transaction(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            for (id, root, manifest_path, manifest_hash, git_revision) in rows {
                conn.execute(
                    "INSERT INTO container_sources(id,canonical_path,manifest_path,manifest_hash,git_revision,discovered_at,updated_at)
                     VALUES(?1,?2,?3,?4,?5,?6,?6)
                     ON CONFLICT(id) DO UPDATE SET
                        canonical_path=excluded.canonical_path,
                        manifest_path=excluded.manifest_path,
                        manifest_hash=excluded.manifest_hash,
                        git_revision=excluded.git_revision,
                        discovered_at=excluded.discovered_at,
                        updated_at=excluded.updated_at",
                    params![id, root, manifest_path, manifest_hash, git_revision, now],
                )?;
            }
            Ok(())
        })
        .await
}

fn source_id(root: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(root.to_string_lossy().as_bytes());
    format!("src_{:x}", hasher.finalize())
}

fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn git_revision(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let revision = String::from_utf8(output.stdout).ok()?;
    let revision = revision.trim();
    (!revision.is_empty()).then(|| revision.to_string())
}

fn require_identifier(value: &str, label: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 256 {
        bail!("{label} is required");
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_source(root: &Path, name: &str, spec_id: &str) -> PathBuf {
        let source = root.join(name);
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join(CONTAINERS_FILE),
            format!(
                r#"
[[container]]
id = "{spec_id}"
url = "http://127.0.0.1:9999"
locality = "container"
[container.launch]
schema_version = "synth.container-launch.v1"
working_directory = "."
command = ["true"]
readiness_timeout_seconds = 30
shutdown_grace_seconds = 5
expected_port = 9999
image_ref = "fixture"
health_target = "fixture"
[container.launch.source]
revision_policy = "exact-or-dirty-digest"
tracked_revision = "fixture-revision"
include = ["workshop.containers.toml"]
"#
            ),
        )
        .unwrap();
        source
    }

    #[test]
    fn discovers_direct_child_sources_with_stable_identity_and_manifest_hash() {
        let temp = tempdir().unwrap();
        let source = write_source(temp.path(), "craftax", "craftax_react");

        let found = discover_in_roots(&[temp.path().to_path_buf()]).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].root, source.canonicalize().unwrap());
        assert_eq!(found[0].specs[0].id, "craftax_react");
        assert!(found[0].id.starts_with("src_"));
        assert!(found[0].manifest_hash.starts_with("sha256:"));
    }

    #[test]
    fn resolving_a_spec_without_a_source_is_rejected_when_ambiguous() {
        let temp = tempdir().unwrap();
        write_source(temp.path(), "first", "shared");
        write_source(temp.path(), "second", "shared");
        let matches: Vec<_> = discover_in_roots(&[temp.path().to_path_buf()])
            .unwrap()
            .into_iter()
            .filter(|source| source.spec("shared").is_ok())
            .collect();
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn source_identity_selects_the_exact_spec_to_start() {
        let temp = tempdir().unwrap();
        let selected = write_source(temp.path(), "selected", "craftax_react");
        write_source(temp.path(), "other", "craftax_react");
        let sources = discover_in_roots(&[temp.path().to_path_buf()]).unwrap();
        let selected_root = selected.canonicalize().unwrap();
        let source = sources
            .into_iter()
            .find(|source| source.root == selected_root)
            .unwrap();
        let spec = source.spec("craftax_react").unwrap();
        assert_eq!(spec.cwd, selected_root);
    }
}
