//! Persisted executable-source grants, independent of conversation attachments.
//! Admission belongs to the native picker service; these internal functions
//! only persist its decisions. Never expose a whole-list agent write command.

use super::{config_path, mutate_config, read_toml};
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceEntry {
    pub path: String,
    pub containers: bool,
    pub recipes: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceSettings {
    pub config_path: String,
    pub entries: Vec<ProjectSourceEntry>,
}

pub fn project_source_settings() -> Result<ProjectSourceSettings> {
    settings_at(&config_path())
}

pub(crate) fn settings_at(path: &Path) -> Result<ProjectSourceSettings> {
    Ok(ProjectSourceSettings {
        config_path: path.display().to_string(),
        entries: entries_from_document(&read_toml(path)?)?,
    })
}

pub fn merge_project_source(entry: ProjectSourceEntry) -> Result<ProjectSourceSettings> {
    merge_at(&config_path(), entry)
}

fn merge_at(path: &Path, entry: ProjectSourceEntry) -> Result<ProjectSourceSettings> {
    mutate_at(path, |entries| {
        entries.push(entry);
        Ok(())
    })
}

pub fn forget_project_source(path: &str) -> Result<ProjectSourceSettings> {
    forget_at(&config_path(), path)
}

pub(crate) fn forget_at(config: &Path, path: &str) -> Result<ProjectSourceSettings> {
    // Do not canonicalize: a deleted/unmounted source must still be revocable.
    let path = path.trim();
    mutate_at(config, |entries| {
        entries.retain(|entry| entry.path != path);
        Ok(())
    })
}

/// A compensatable, single-root mutation. Rollback never restores a stale
/// whole-list snapshot or overwrites a newer change to the same root.
pub(crate) struct ProjectSourceChange {
    config: std::path::PathBuf,
    path: String,
    previous: Option<ProjectSourceEntry>,
    written: Option<ProjectSourceEntry>,
}

impl ProjectSourceChange {
    pub(crate) fn rollback(self) -> Result<()> {
        mutate_at(&self.config, |entries| {
            let current = entries
                .iter()
                .find(|entry| entry.path == self.path)
                .cloned();
            if current != self.written {
                bail!("project source changed again; refusing to overwrite the newer grant during rollback");
            }
            entries.retain(|entry| entry.path != self.path);
            if let Some(previous) = self.previous {
                entries.push(previous);
            }
            Ok(())
        })?;
        Ok(())
    }
}

pub(crate) fn begin_project_source_grant(entry: ProjectSourceEntry) -> Result<ProjectSourceChange> {
    change_at(&config_path(), entry.path.trim().to_owned(), Some(entry))
}

fn change_at(
    config: &Path,
    path: String,
    entry: Option<ProjectSourceEntry>,
) -> Result<ProjectSourceChange> {
    let mut previous = None;
    let settings = mutate_at(config, |entries| {
        previous = entries.iter().find(|entry| entry.path == path).cloned();
        match entry {
            Some(entry) => entries.push(entry),
            None => entries.retain(|entry| entry.path != path),
        }
        Ok(())
    })?;
    let written = settings
        .entries
        .into_iter()
        .find(|entry| entry.path == path);
    Ok(ProjectSourceChange {
        config: config.to_owned(),
        path,
        previous,
        written,
    })
}


fn mutate_at(
    path: &Path,
    edit: impl FnOnce(&mut Vec<ProjectSourceEntry>) -> Result<()>,
) -> Result<ProjectSourceSettings> {
    let entries = mutate_config(path, |document| {
        let mut entries = entries_from_document(document)?;
        edit(&mut entries)?;
        let entries = normalize(entries)?;
        let root = document
            .as_table_mut()
            .ok_or_else(|| anyhow!("config must be a table"))?;
        let desktop = root
            .entry("desktop")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .ok_or_else(|| anyhow!("[desktop] must be a table"))?;
        let sources = desktop
            .entry("project_sources")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .ok_or_else(|| anyhow!("[desktop.project_sources] must be a table"))?;
        sources.remove("roots");
        sources.insert("entries".into(), toml::Value::try_from(&entries)?);
        Ok(entries)
    })?;
    Ok(ProjectSourceSettings {
        config_path: path.display().to_string(),
        entries,
    })
}

/// Retain both public spellings, but never turn malformed permission state
/// into an empty list or silently default an incorrectly typed flag to true.
fn entries_from_document(document: &toml::Value) -> Result<Vec<ProjectSourceEntry>> {
    let Some(desktop) = document.get("desktop") else {
        return Ok(Vec::new());
    };
    let desktop = desktop
        .as_table()
        .ok_or_else(|| anyhow!("[desktop] must be a table"))?;
    let Some(sources) = desktop.get("project_sources") else {
        return Ok(Vec::new());
    };
    let sources = sources
        .as_table()
        .ok_or_else(|| anyhow!("[desktop.project_sources] must be a table"))?;
    let mut entries = Vec::new();
    if let Some(roots) = sources.get("roots") {
        for root in roots
            .as_array()
            .ok_or_else(|| anyhow!("project source roots must be an array"))?
        {
            entries.push(ProjectSourceEntry {
                path: root
                    .as_str()
                    .ok_or_else(|| anyhow!("project source root must be a string"))?
                    .to_owned(),
                containers: true,
                recipes: true,
            });
        }
    }
    if let Some(declared) = sources.get("entries") {
        for entry in declared
            .as_array()
            .ok_or_else(|| anyhow!("project source entries must be an array"))?
        {
            let entry = entry
                .as_table()
                .ok_or_else(|| anyhow!("project source entry must be a table"))?;
            let path = entry
                .get("path")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| anyhow!("project source entry requires a path string"))?;
            let capability = |name: &str| -> Result<bool> {
                match entry.get(name) {
                    None => Ok(true), // Published legacy entries grant both by default.
                    Some(value) => value
                        .as_bool()
                        .ok_or_else(|| anyhow!("project source {name} must be a boolean")),
                }
            };
            entries.push(ProjectSourceEntry {
                path: path.to_owned(),
                containers: capability("containers")?,
                recipes: capability("recipes")?,
            });
        }
    }
    normalize(entries)
}

fn normalize(requested: Vec<ProjectSourceEntry>) -> Result<Vec<ProjectSourceEntry>> {
    let mut entries: Vec<ProjectSourceEntry> = Vec::new();
    for mut entry in requested {
        entry.path = entry.path.trim().to_owned();
        if entry.path.is_empty() || !Path::new(&entry.path).is_absolute() {
            bail!("project source paths must be nonempty and absolute");
        }
        if !entry.containers && !entry.recipes {
            bail!("project source must enable containers, recipes, or both");
        }
        if let Some(existing) = entries
            .iter_mut()
            .find(|candidate| candidate.path == entry.path)
        {
            existing.containers |= entry.containers;
            existing.recipes |= entry.recipes;
        } else {
            entries.push(entry);
        }
    }
    Ok(entries)
}

