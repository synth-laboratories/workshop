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

fn settings_at(path: &Path) -> Result<ProjectSourceSettings> {
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

fn forget_at(config: &Path, path: &str) -> Result<ProjectSourceSettings> {
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

#[cfg(test)]
pub(crate) fn begin_project_source_grant_at(
    config: &Path,
    entry: ProjectSourceEntry,
) -> Result<ProjectSourceChange> {
    change_at(config, entry.path.trim().to_owned(), Some(entry))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn grant_rollback_removes_only_its_new_root_and_refuses_newer_changes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let change =
            begin_project_source_grant_at(&path, grant("/projects/first", false, true)).unwrap();
        merge_at(&path, grant("/projects/other", true, false)).unwrap();
        change.rollback().unwrap();
        assert_eq!(
            settings_at(&path).unwrap().entries,
            vec![grant("/projects/other", true, false)]
        );
        let change =
            begin_project_source_grant_at(&path, grant("/projects/first", false, true)).unwrap();
        merge_at(&path, grant("/projects/first", true, false)).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        assert!(change.rollback().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    fn grant(path: &str, containers: bool, recipes: bool) -> ProjectSourceEntry {
        ProjectSourceEntry {
            path: path.into(),
            containers,
            recipes,
        }
    }

    #[test]
    fn legacy_forms_merge_without_losing_capability_flags() {
        let document: toml::Value = r#"
[desktop.project_sources]
roots = ["/projects/legacy"]
[[desktop.project_sources.entries]]
path = "/projects/one"
containers = true
recipes = false
[[desktop.project_sources.entries]]
path = " /projects/one "
containers = false
recipes = true
[[desktop.project_sources.entries]]
path = "/projects/default"
"#
        .parse()
        .unwrap();
        assert_eq!(
            entries_from_document(&document).unwrap(),
            vec![
                grant("/projects/legacy", true, true),
                grant("/projects/one", true, true),
                grant("/projects/default", true, true),
            ]
        );
    }

    #[test]
    fn malformed_grants_fail_closed_and_are_not_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        for body in [
            "desktop = false", "[desktop]\nproject_sources = []",
            "[desktop.project_sources]\nroots = false",
            "[desktop.project_sources]\nroots = [7]",
            "[desktop.project_sources]\nentries = false",
            "[desktop.project_sources]\nentries = [7]",
            "[[desktop.project_sources.entries]]\ncontainers = true",
            "[[desktop.project_sources.entries]]\npath = '/projects/one'\ncontainers = 'false'",
            "[[desktop.project_sources.entries]]\npath = '/projects/one'\ncontainers = false\nrecipes = false",
            "[desktop.project_sources]\nroots = ['relative']",
            "[desktop.project_sources]\nroots = ['']",
        ] {
            fs::write(&path, body).unwrap();
            assert!(settings_at(&path).is_err(), "{body}");
            assert!(merge_at(&path, grant("/projects/new", true, true)).is_err(), "{body}");
            assert_eq!(fs::read_to_string(&path).unwrap(), body);
        }
    }

    #[test]
    fn mutation_preserves_unrelated_settings_and_revokes_missing_sources() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "[models.default]\nmodel = 'keep'\n[desktop.project_sources]\nroots = ['/missing/old']\nscan_hint = 'keep'").unwrap();
        merge_at(&path, grant("/missing/new", true, false)).unwrap();
        merge_at(&path, grant("/missing/new", false, true)).unwrap();
        let result = forget_at(&path, " /missing/old ").unwrap();
        assert_eq!(result.entries, vec![grant("/missing/new", true, true)]);
        let document = read_toml(&path).unwrap();
        assert_eq!(
            document["models"]["default"]["model"].as_str(),
            Some("keep")
        );
        assert_eq!(
            document["desktop"]["project_sources"]["scan_hint"].as_str(),
            Some("keep")
        );
        assert!(document["desktop"]["project_sources"]
            .get("roots")
            .is_none());
        assert_eq!(settings_at(&path).unwrap(), result);
    }

    #[test]
    fn concurrent_admissions_preserve_every_grant() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let threads: Vec<_> = (0..12)
            .map(|index| {
                let path = path.clone();
                std::thread::spawn(move || {
                    merge_at(&path, grant(&format!("/projects/{index}"), true, false)).unwrap()
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let entries = settings_at(&path).unwrap().entries;
        assert_eq!(entries.len(), 12);
        for index in 0..12 {
            assert!(entries.contains(&grant(&format!("/projects/{index}"), true, false)));
        }
    }
}
