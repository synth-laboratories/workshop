use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const MANAGED_TEMPLATE_MAX_BYTES: u64 = 1_500_000;

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplateReadinessContract {
    #[serde(default)]
    pub reject_transport_states: Vec<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub minimum_rollout_count: u64,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub minimum_rendered_frame_count: u64,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub minimum_semantic_event_count: u64,
    #[serde(default)]
    pub require_terminal: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplateObservationContract {
    pub schema_version: String,
    pub readiness: TemplateReadinessContract,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplateMeta {
    pub schema_version: String,
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub genre: Option<String>,
    /// Optional container/eval family this live template is registered to
    /// represent. Tags remain descriptive/search metadata and are not an
    /// ownership claim when this field is present on another template.
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub shell_path: Option<String>,
    /// `renderer.html` packages are imported into the instance-local managed
    /// registry. They are rendered in a sandbox rather than Vite's static TSX
    /// graph, so the renderer source remains immutable after import.
    #[serde(default)]
    pub renderer_path: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub example_binding: Option<Value>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub inputs: Vec<Value>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub slots: Vec<Value>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub components: Vec<Value>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub binding_schema: Vec<Value>,
    #[serde(default)]
    pub observation_contract: Option<TemplateObservationContract>,
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
    for extra_root_name in ["templates", "templates-internal"] {
        let extra_root = visuals_root.join(extra_root_name);
        if !extra_root.exists() {
            continue;
        }
        let mut entries: Vec<_> = fs::read_dir(&extra_root)?
            .filter_map(|entry| entry.ok())
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if !path.is_dir() || !path.join("template.json").exists() {
                continue;
            }
            let mut meta = load_template_meta(&path)?;
            if templates.contains_key(&meta.id) {
                continue;
            }
            meta.path = Some(path.display().to_string());
            templates.insert(meta.id.clone(), meta);
        }
    }
    let managed_root = managed_templates_root();
    if managed_root.exists() {
        let mut entries: Vec<_> = fs::read_dir(&managed_root)?
            .filter_map(|entry| entry.ok())
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if !path.is_dir()
                || !path.join("template.json").is_file()
                || !path.join("renderer.html").is_file()
            {
                continue;
            }
            let mut meta = load_template_meta(&path)?;
            if templates.contains_key(&meta.id) {
                anyhow::bail!(
                    "managed visual template id collides with bundled template: {}",
                    meta.id
                );
            }
            meta.path = Some(path.display().to_string());
            meta.renderer_path = Some(path.join("renderer.html").display().to_string());
            meta.source_kind = Some("managed".into());
            templates.insert(meta.id.clone(), meta);
        }
    }
    Ok(templates)
}

fn managed_templates_root() -> PathBuf {
    std::env::var("SYNTH_DESKTOP_DATA_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| visuals_root())
        .join("visuals")
        .join("templates")
}

/// Copy one reviewed, networkless HTML visual package into this instance's
/// managed registry. This is intentionally a two-file contract: accepting a
/// directory tree would turn import into an unbounded code and asset loader.
pub fn import_managed_template(source_path: &str) -> anyhow::Result<TemplateMeta> {
    let source = Path::new(source_path);
    if !source.is_absolute() {
        anyhow::bail!("source_path must be an absolute directory");
    }
    let source = fs::canonicalize(source)
        .map_err(|_| anyhow::anyhow!("source_path does not exist or is not readable"))?;
    let metadata = fs::symlink_metadata(&source)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        anyhow::bail!("source_path must be a real directory, not a symlink");
    }
    let manifest = source.join("template.json");
    let renderer = source.join("renderer.html");
    for file in [&manifest, &renderer] {
        let metadata = fs::symlink_metadata(file).map_err(|_| {
            anyhow::anyhow!("managed template requires template.json and renderer.html")
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            anyhow::bail!("managed template files must be regular files, not symlinks");
        }
        if metadata.len() > MANAGED_TEMPLATE_MAX_BYTES {
            anyhow::bail!("managed template file exceeds {MANAGED_TEMPLATE_MAX_BYTES} bytes");
        }
    }
    let mut meta = load_template_meta(&source)?;
    let renderer_bytes = fs::read(&renderer)?;
    validate_managed_renderer(&renderer_bytes)?;
    let destination = managed_templates_root().join(&meta.id);
    fs::create_dir_all(&destination)?;
    fs::write(destination.join("template.json"), fs::read(&manifest)?)?;
    fs::write(destination.join("renderer.html"), renderer_bytes)?;
    meta.path = Some(destination.display().to_string());
    meta.renderer_path = Some(destination.join("renderer.html").display().to_string());
    meta.source_kind = Some("managed".into());
    Ok(meta)
}

fn validate_managed_renderer(bytes: &[u8]) -> anyhow::Result<()> {
    let source = std::str::from_utf8(bytes).context("renderer.html must be UTF-8")?;
    let lower = source.to_ascii_lowercase();
    // Do not reject a URL-shaped string everywhere: compiled Preact embeds the
    // SVG namespace (`http://www.w3.org/2000/svg`) as a plain string.  Reject
    // the places that could actually initiate a request instead.  The iframe
    // CSP is the runtime backstop; this check keeps an unsafe package out of
    // the managed registry in the first place.
    for forbidden in [
        "<script src",
        "fetch(",
        "xmlhttprequest",
        "eventsource",
        "websocket(",
        "navigator.sendbeacon",
        "import(",
        "url(http",
        "url(//",
        "url(\\\"http",
        "url('http",
    ] {
        if lower.contains(forbidden) {
            anyhow::bail!("renderer.html is not networkless: forbidden token {forbidden:?}");
        }
    }
    for attribute in [
        "src",
        "href",
        "action",
        "formaction",
        "poster",
        "data",
        "srcset",
    ] {
        if contains_external_url_attribute(&lower, attribute) {
            anyhow::bail!(
                "renderer.html is not networkless: external URL in {attribute} attribute"
            );
        }
    }
    Ok(())
}

fn contains_external_url_attribute(source: &str, attribute: &str) -> bool {
    let mut remainder = source;
    while let Some(offset) = remainder.find(attribute) {
        let before = &remainder[..offset];
        let after = &remainder[offset + attribute.len()..];
        // Attribute names must have a boundary; this excludes e.g. `dataUrl`.
        let bounded_before = before
            .chars()
            .last()
            .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '-');
        if bounded_before {
            let value = after.trim_start();
            if let Some(value) = value.strip_prefix('=') {
                let value = value.trim_start().trim_start_matches(['\'', '\"']);
                if value.starts_with("http://")
                    || value.starts_with("https://")
                    || value.starts_with("//")
                {
                    return true;
                }
            }
        }
        remainder = &after[1..];
    }
    false
}

fn discover_template_directories(
    directory: &Path,
    canonical_root: &Path,
    out: &mut Vec<PathBuf>,
) -> anyhow::Result<()> {
    // This runs while an optimizer launch future is already carrying a large
    // amount of state. Recursive filesystem descent can exhaust a Tokio
    // worker's comparatively small stack even for an ordinary registry. Keep
    // traversal state on the heap instead.
    let mut pending = vec![directory.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let metadata = fs::symlink_metadata(&directory)?;
        if metadata.file_type().is_symlink() {
            anyhow::bail!(
                "visual template registry refuses symlink: {}",
                directory.display()
            );
        }
        let canonical = fs::canonicalize(&directory)?;
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
            out.push(directory);
            continue;
        }

        let mut entries = fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries.into_iter().rev() {
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                anyhow::bail!(
                    "visual template registry refuses symlink: {}",
                    entry.path().display()
                );
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            }
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
    let declared = match (value.get("inputs"), value.get("slots")) {
        (Some(a), Some(b)) if a != b => {
            anyhow::bail!("template {id} inputs and slots disagree")
        }
        (Some(a), _) => a.as_array().cloned().unwrap_or_default(),
        (_, Some(b)) => b.as_array().cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut meta = TemplateMeta {
        schema_version,
        id,
        title,
        genre: value
            .get("genre")
            .and_then(Value::as_str)
            .map(str::to_string),
        family: value
            .get("family")
            .and_then(Value::as_str)
            .map(str::to_string),
        version,
        description: value
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string),
        tags: value
            .get("tags")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        path: None,
        shell_path: None,
        renderer_path: None,
        source_kind: None,
        example_binding: None,
        binding_schema: declared.clone(),
        inputs: declared.clone(),
        slots: declared,
        components: value
            .get("components")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        observation_contract: value
            .get("observationContract")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?,
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
            let experiment = templates
                .iter()
                .find(|template| template.id == "experiment.overview.v1")
                .expect("experiment.overview.v1");
            assert_eq!(experiment.slots, experiment.binding_schema);
            assert_eq!(experiment.inputs, experiment.slots);
            assert!(experiment.example_binding.is_some());
            let analysis = templates
                .iter()
                .find(|template| template.id == "analysis.visual.v1")
                .expect("analysis.visual.v1");
            assert!(analysis.example_binding.is_some());
            let accepts = analysis.slots[0]["accepts"]
                .as_array()
                .expect("analysis accepts");
            assert!(accepts.iter().any(|value| value == "inline"));
            let compose = templates
                .iter()
                .find(|template| template.id == "compose.visual.v1")
                .expect("compose.visual.v1");
            assert!(compose.example_binding.is_some());
            let compose_slots: Vec<&str> = compose
                .slots
                .iter()
                .filter_map(|slot| slot.get("name").and_then(Value::as_str))
                .collect();
            assert_eq!(compose_slots, ["spec", "stream", "optimizer_run"]);
            assert_eq!(compose.inputs, compose.slots);
            let compose_components: Vec<&str> = compose
                .components
                .iter()
                .filter_map(|row| row.get("id").and_then(Value::as_str))
                .collect();
            assert_eq!(
                compose_components,
                [
                    "event_stream.v1",
                    "detail_modal.v1",
                    "metrics.v1",
                    "scrubber.v1",
                    "candidate_inspector.v1"
                ]
            );
            let sourced = templates
                .iter()
                .find(|template| template.id == "sourced.visual.v1")
                .expect("sourced.visual.v1");
            assert!(sourced.example_binding.is_some());
            let sourced_slots: Vec<&str> = sourced
                .slots
                .iter()
                .filter_map(|slot| slot.get("name").and_then(Value::as_str))
                .collect();
            assert_eq!(sourced_slots, ["stream"]);
            let sourced_components: Vec<&str> = sourced
                .components
                .iter()
                .filter_map(|row| row.get("id").and_then(Value::as_str))
                .collect();
            assert_eq!(sourced_components, ["event_stream.v1", "detail_modal.v1"]);
            assert!(analysis.components.is_empty());
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
        assert!(indexed["example.v1"].components.is_empty());
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
