use super::{canonical_project_root, require_manifest_in, resolve_roots, Capability, RootOrigin};
use crate::{optimizers::workspace_recipe, synth_config};
use anyhow::Result;
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceInspection {
    pub path: String,
    pub status: String,
    pub code: Option<String>,
    pub message: Option<String>,
    pub containers: Vec<String>,
    pub recipes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceRow {
    pub path: String,
    pub containers: bool,
    pub recipes: bool,
    pub origin: RootOrigin,
    pub inspection: ProjectSourceInspection,
    pub last_scanned_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceCatalog {
    pub config_path: String,
    pub sources: Vec<ProjectSourceRow>,
    pub implicit_roots: Vec<ProjectSourceRow>,
}

pub fn inspect(root: &Path) -> ProjectSourceInspection {
    let mut result = ProjectSourceInspection {
        path: root.display().to_string(),
        status: "invalid".into(),
        code: None,
        message: None,
        containers: Vec::new(),
        recipes: Vec::new(),
    };
    if !root.is_dir() {
        result.status = "missing".into();
        result.code = Some("source_path_missing".into());
        result.message = Some("Source is not an available directory".into());
        return result;
    }
    let root = match canonical_project_root(&root.display().to_string()) {
        Ok(root) => root,
        Err(error) => {
            result.code = Some("source_path_invalid".into());
            result.message = Some(error.to_string());
            return result;
        }
    };
    let manifest = root.join("workshop.containers.toml");
    if manifest.exists() || manifest.is_symlink() {
        if let Err(error) = require_manifest_in(&manifest, std::slice::from_ref(&root)) {
            result.code = Some("container_manifest_invalid".into());
            result.message = Some(error.to_string());
            return result;
        }
    }
    match workspace_recipe::load_container_specs(&root) {
        Ok(specs) => result.containers = specs.into_iter().map(|spec| spec.id).collect(),
        Err(error) => {
            result.code = Some("container_manifest_invalid".into());
            result.message = Some(error.to_string());
            return result;
        }
    }
    match workspace_recipe::load_recipes(&root) {
        Ok(recipes) => result.recipes = recipes.into_iter().map(|recipe| recipe.id).collect(),
        Err(error) => {
            result.code = Some("recipe_manifest_invalid".into());
            result.message = Some(error.to_string());
            return result;
        }
    }
    if result.containers.is_empty() && result.recipes.is_empty() {
        result.code = Some("no_declaration".into());
        result.message = Some("Source declares no containers or recipes".into());
    } else {
        result.status = "valid".into();
    }
    result
}

pub fn catalog() -> Result<ProjectSourceCatalog> {
    let settings = synth_config::project_source_settings()?;
    let sources = settings
        .entries
        .into_iter()
        .map(|entry| ProjectSourceRow {
            inspection: inspect(Path::new(&entry.path)),
            path: entry.path,
            containers: entry.containers,
            recipes: entry.recipes,
            origin: RootOrigin::Configured,
            // This implementation scans live; it does not fabricate a persisted timestamp.
            last_scanned_at: None,
        })
        .collect();
    let mut implicit: Vec<ProjectSourceRow> = Vec::new();
    for capability in [Capability::Containers, Capability::Recipes] {
        for root in resolve_roots(capability)?
            .into_iter()
            .filter(|root| root.origin == RootOrigin::Environment)
        {
            let path = root.path.display().to_string();
            let index = if let Some(index) = implicit.iter().position(|row| row.path == path) {
                index
            } else {
                implicit.push(ProjectSourceRow {
                    path,
                    containers: false,
                    recipes: false,
                    origin: RootOrigin::Environment,
                    inspection: inspect(&root.path),
                    last_scanned_at: None,
                });
                implicit.len() - 1
            };
            match capability {
                Capability::Containers => implicit[index].containers = true,
                Capability::Recipes => implicit[index].recipes = true,
            }
        }
    }
    Ok(ProjectSourceCatalog {
        config_path: settings.config_path,
        sources,
        implicit_roots: implicit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn inspection_returns_recipe_identity_without_running_a_workload() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("workshop.recipe.toml"),
            r#"
id = "eval.inspection.v1"
algorithm = "eval"
container = "fixture"
provider = "openrouter"
model = "openai/gpt-4.1-nano"
locality = "container"
train_seeds = [0]
[bounds]
max_cost_usd = 0.50
max_total_rollouts = 10
"#,
        )
        .unwrap();
        let result = inspect(directory.path());
        assert_eq!(result.status, "valid", "{result:?}");
        assert_eq!(result.recipes, vec!["eval.inspection.v1"]);
        assert!(result.containers.is_empty());
        assert!(result.code.is_none());
    }

    #[test]
    fn inspection_distinguishes_absence_from_invalid_declarations() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(inspect(&directory.path().join("missing")).status, "missing");
        assert_eq!(
            inspect(directory.path()).code.as_deref(),
            Some("no_declaration")
        );
        fs::write(
            directory.path().join("workshop.containers.toml"),
            "not valid toml [",
        )
        .unwrap();
        assert_eq!(
            inspect(directory.path()).code.as_deref(),
            Some("container_manifest_invalid")
        );
    }

    #[cfg(unix)]
    #[test]
    fn inspection_refuses_external_manifest_before_loading_it() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        fs::create_dir(&root).unwrap();
        let outside = directory.path().join("outside.toml");
        fs::write(&outside, "invalid outside fixture").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("workshop.containers.toml")).unwrap();
        let result = inspect(&root);
        assert_eq!(result.code.as_deref(), Some("container_manifest_invalid"));
        assert!(result
            .message
            .unwrap()
            .contains("launch_source_root_not_approved"));
    }
}
