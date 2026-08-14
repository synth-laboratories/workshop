use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
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
            if bundled.join("templates").is_dir() {
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
    // Public templates are authoritative. An optional build-time internal
    // overlay may add IDs, but can never shadow a distributed template.
    for root in ["templates", "templates-internal"] {
        let root = visuals_root().join(root);
        if !root.exists() {
            continue;
        }
        let mut entries: Vec<_> = fs::read_dir(&root)?
            .filter_map(|entry| entry.ok())
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if !path.is_dir() || !path.join("template.json").exists() {
                continue;
            }
            let mut meta = load_template_meta(&path)?;
            if out
                .iter()
                .any(|existing: &TemplateMeta| existing.id == meta.id)
            {
                continue;
            }
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
            meta.path = Some(path.display().to_string());
            out.push(meta);
        }
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

pub fn resolve_template(template_id: &str) -> anyhow::Result<TemplateMeta> {
    let id = template_id.trim();
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        anyhow::bail!("invalid template id");
    }
    let root = visuals_root();
    let path = ["templates", "templates-internal"]
        .into_iter()
        .map(|dir| root.join(dir).join(id))
        .find(|candidate| candidate.join("template.json").exists())
        .ok_or_else(|| anyhow::anyhow!("unknown visual template: {id}"))?;
    let mut meta = load_template_meta(&path)?;
    meta.path = Some(path.display().to_string());
    Ok(meta)
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
        if visuals_root().join("templates").exists() {
            assert!(!templates.is_empty());
        }
    }
}
