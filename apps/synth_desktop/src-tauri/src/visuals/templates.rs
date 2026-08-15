use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplateMeta {
    pub schema_version: String,
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub shell_path: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub example_binding: Option<Value>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub slots: Vec<Value>,
}

pub fn visuals_root() -> PathBuf {
    if let Ok(value) = std::env::var("SYNTH_VISUALS_ROOT") {
        return PathBuf::from(value);
    }
    if let Ok(workshop) = std::env::var("SYNTH_WORKSHOP_ROOT") {
        return PathBuf::from(workshop).join("visuals");
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(macos_dir) = executable.parent() {
            let bundled = macos_dir.join("../Resources/visuals");
            if bundled.join("families").is_dir() {
                return bundled;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("visuals")
}

pub fn list_templates(genre: Option<&str>) -> anyhow::Result<Vec<TemplateMeta>> {
    let mut out = Vec::new();
    for (_, meta) in build_template_index(&visuals_root())? {
        if let Some(filter) = genre {
            let matches = meta
                .genre
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case(filter))
                .unwrap_or(false)
                || meta.id.to_lowercase().contains(&filter.to_lowercase());
            if !matches {
                continue;
            }
        }
        out.push(meta);
    }
    Ok(out)
}

pub fn resolve_template(template_id: &str) -> anyhow::Result<TemplateMeta> {
    let id = template_id.trim();
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        anyhow::bail!("invalid template id");
    }
    build_template_index(&visuals_root())?
        .remove(id)
        .ok_or_else(|| anyhow::anyhow!("unknown visual template: {id}"))
}

fn build_template_index(visuals_root: &Path) -> anyhow::Result<BTreeMap<String, TemplateMeta>> {
    let families_root = visuals_root.join("families");
    if !families_root.exists() {
        return Ok(BTreeMap::new());
    }
    let canonical_root = fs::canonicalize(&families_root)?;
    let mut directories = Vec::new();
    discover_template_directories(&families_root, &canonical_root, &mut directories)?;
    directories.sort();

    let mut templates: BTreeMap<String, TemplateMeta> = BTreeMap::new();
    for directory in directories {
        let mut meta = load_template_meta(&directory)?;
        if let Some(existing) = templates.get(&meta.id) {
            anyhow::bail!(
                "duplicate visual template id {:?} in {} and {}",
                meta.id,
                existing.path.as_deref().unwrap_or("<unknown>"),
                directory.display()
            );
        }
        meta.path = Some(directory.display().to_string());
        templates.insert(meta.id.clone(), meta);
    }
    Ok(templates)
}

fn discover_template_directories(
    directory: &Path,
    canonical_root: &Path,
    out: &mut Vec<PathBuf>,
) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(directory)?;
    if metadata.file_type().is_symlink() {
        anyhow::bail!(
            "visual template registry refuses symlink: {}",
            directory.display()
        );
    }
    let canonical = fs::canonicalize(directory)?;
    if !canonical.starts_with(canonical_root) {
        anyhow::bail!(
            "visual template path escapes family root: {}",
            directory.display()
        );
    }

    let manifest = directory.join("template.json");
    if manifest.exists() {
        let manifest_metadata = fs::symlink_metadata(&manifest)?;
        if manifest_metadata.file_type().is_symlink() {
            anyhow::bail!(
                "visual template registry refuses symlink: {}",
                manifest.display()
            );
        }
        out.push(directory.to_path_buf());
        return Ok(());
    }

    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            anyhow::bail!(
                "visual template registry refuses symlink: {}",
                entry.path().display()
            );
        }
        if file_type.is_dir() {
            discover_template_directories(&entry.path(), canonical_root, out)?;
        }
    }
    Ok(())
}

fn load_template_meta(path: &Path) -> anyhow::Result<TemplateMeta> {
    let raw = fs::read_to_string(path.join("template.json"))?;
    let value: Value = serde_json::from_str(&raw)?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown")
                .to_string()
        });
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_string();
    let schema_version = value
        .get("schemaVersion")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if schema_version != "synth.visual-template.v1" {
        anyhow::bail!("template {id} has unsupported schemaVersion");
    }
    if path.file_name().and_then(|name| name.to_str()) != Some(id.as_str()) {
        anyhow::bail!("template id does not match directory: {id}");
    }
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .map(str::to_string);
    if version.as_deref().unwrap_or_default().split('.').count() != 3 {
        anyhow::bail!("template {id} requires a semantic version");
    }
    let slots = value
        .get("slots")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut meta = TemplateMeta {
        schema_version,
        id,
        title,
        genre: value
            .get("genre")
            .and_then(Value::as_str)
            .map(str::to_string),
        version,
        description: value
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string),
        path: None,
        shell_path: None,
        example_binding: None,
        slots,
    };
    let shell = path.join("shell.tsx");
    if shell.exists() {
        meta.shell_path = Some(shell.display().to_string());
    }
    let example = path.join("examples").join("fixture_binding.json");
    if example.exists() {
        meta.example_binding = serde_json::from_str(&fs::read_to_string(example)?)?;
    }
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_bundled_templates_when_present() {
        let templates = list_templates(None).unwrap();
        if visuals_root().join("families").exists() {
            assert!(!templates.is_empty());
        }
    }

    fn write_template(path: &Path, id: &str) {
        fs::create_dir_all(path).unwrap();
        fs::write(
            path.join("template.json"),
            format!(
                r#"{{"schemaVersion":"synth.visual-template.v1","id":"{id}","version":"1.0.0"}}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn recursively_indexes_templates_by_manifest_id() {
        let temp = tempfile::tempdir().unwrap();
        write_template(
            &temp.path().join("families/analysis/example.v1"),
            "example.v1",
        );
        let indexed = build_template_index(temp.path()).unwrap();
        assert_eq!(indexed.keys().collect::<Vec<_>>(), vec!["example.v1"]);
        assert!(indexed["example.v1"]
            .path
            .as_deref()
            .unwrap()
            .contains("families/analysis/example.v1"));
    }

    #[test]
    fn duplicate_ids_fail_with_both_paths() {
        let temp = tempfile::tempdir().unwrap();
        write_template(
            &temp.path().join("families/one/duplicate.v1"),
            "duplicate.v1",
        );
        write_template(
            &temp.path().join("families/two/duplicate.v1"),
            "duplicate.v1",
        );
        let error = build_template_index(temp.path()).unwrap_err().to_string();
        assert!(error.contains("duplicate visual template id"));
        assert!(error.contains("families/one/duplicate.v1"));
        assert!(error.contains("families/two/duplicate.v1"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_family_paths_fail_closed() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write_template(&outside.path().join("escaped.v1"), "escaped.v1");
        fs::create_dir_all(temp.path().join("families")).unwrap();
        symlink(outside.path(), temp.path().join("families/escaped")).unwrap();
        let error = build_template_index(temp.path()).unwrap_err().to_string();
        assert!(error.contains("refuses symlink"));
    }
}
