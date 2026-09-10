use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const MANAGED_TEMPLATE_MAX_BYTES: u64 = 1_500_000;

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    specta::Type,
    schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum AuthoringAffordance {
    TemporalControls,
    TraceInspector,
    RealEvidence,
}

impl AuthoringAffordance {
    pub const fn check_name(self) -> &'static str {
        match self {
            Self::TemporalControls => "temporalControls",
            Self::TraceInspector => "traceInspector",
            Self::RealEvidence => "realEvidence",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
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
    /// Distinct non-control transport envelopes required from the host receipt.
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub minimum_transport_envelope_count: u64,
    #[serde(default)]
    pub require_terminal: bool,
    /// Which evidence affordances this surface actually offers, out of
    /// `temporalControls`, `traceInspector`, `realEvidence`.
    ///
    /// Absent means all three, so no existing template is relaxed by this
    /// field. A template opts out only by declaring the shorter list in its
    /// manifest, which is reviewable — unlike a reviewer ticking a box that is
    /// false. A static analysis projection of immutable sealed evidence has no
    /// temporal control to offer, and demanding one made it uncertifiable.
    #[serde(default)]
    pub authoring_affordances: Option<Vec<AuthoringAffordance>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TemplateObservationContract {
    pub schema_version: String,
    pub readiness: TemplateReadinessContract,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TemplateMeta {
    pub schema_version: String,
    pub id: String,
    /// Digest of every file in this template package. Certification binds to
    /// this value so template changes stale earlier reviews without requiring
    /// a cosmetic visual revision bump.
    #[serde(default)]
    pub template_digest: String,
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
    /// Registered renderer capability. Dispatch is based on this descriptor,
    /// never on a hard-coded template id.
    #[serde(default)]
    pub renderer_kind: Option<String>,
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

pub fn certification_renderer_digest(template: &TemplateMeta, source_revision: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"synth.visual-renderer-contract.v1\0");
    hasher.update(source_revision.as_bytes());
    hasher.update(b"\0");
    hasher.update(template.id.as_bytes());
    hasher.update(b"\0");
    hasher.update(template.template_digest.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn template_package_digest(path: &Path) -> anyhow::Result<String> {
    let canonical_root = fs::canonicalize(path)?;
    let mut pending = vec![path.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                anyhow::bail!(
                    "visual template package refuses symlink: {}",
                    entry.path().display()
                );
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                let canonical = fs::canonicalize(entry.path())?;
                if !canonical.starts_with(&canonical_root) {
                    anyhow::bail!(
                        "visual template package escapes its root: {}",
                        entry.path().display()
                    );
                }
                files.push(entry.path());
            }
        }
    }
    files.sort();
    let mut hasher = Sha256::new();
    hasher.update(b"synth.visual-template-package.v1\0");
    for file in files {
        let relative = file.strip_prefix(path)?;
        let name = relative.to_string_lossy();
        let bytes = fs::read(&file)?;
        hasher.update((name.len() as u64).to_be_bytes());
        hasher.update(name.as_bytes());
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

pub fn visuals_root() -> PathBuf {
    if let Ok(value) = std::env::var("SYNTH_VISUALS_ROOT") {
        return PathBuf::from(value);
    }
    if let Ok(workshop) = std::env::var("SYNTH_WORKSHOP_ROOT") {
        let root = PathBuf::from(workshop);
        let package = root.join("packages/workshop-visuals");
        return if package.join("families").is_dir() { package } else { root.join("visuals") };
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
        .join("packages/workshop-visuals")
}

pub fn list_templates(genre: Option<&str>) -> anyhow::Result<Vec<TemplateMeta>> {
    let filter = genre.map(str::to_owned);
    with_template_index(&visuals_root(), move |templates| {
        let mut out = Vec::new();
        for (_, meta) in templates {
            if let Some(filter) = filter.as_deref() {
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
    })
}

pub fn resolve_template(template_id: &str) -> anyhow::Result<TemplateMeta> {
    let id = template_id.trim();
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        anyhow::bail!("invalid template id");
    }
    resolve_template_inner(&visuals_root(), id)
}

fn resolve_template_inner(visuals_root: &Path, id: &str) -> anyhow::Result<TemplateMeta> {
    // Launch-time template resolution is intentionally a bounded direct lookup.
    // Building and then dropping the complete TemplateMeta index from the
    // optimizer admission future has repeatedly exhausted native worker stacks,
    // even when delegated to a generously sized scoped thread. Directory names
    // are already required to equal template IDs by load_template_meta, so scan
    // paths iteratively and decode only the requested manifest.
    let families_root = visuals_root.join("families");
    let mut resolved = None;
    if families_root.exists() {
        let canonical_root = fs::canonicalize(&families_root)?;
        let mut directories = Vec::new();
        discover_template_directories(&families_root, &canonical_root, &mut directories)?;
        directories.sort();
        if let Some(path) = directories
            .into_iter()
            .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(id))
        {
            resolved = Some(load_template_meta(&path).map(|mut meta| {
                meta.path = Some(path.display().to_string());
                meta
            })?);
        }
    }

    for extra_root_name in ["templates", "templates-internal"] {
        if resolved.is_some() {
            break;
        }
        let path = visuals_root.join(extra_root_name).join(id);
        if path.join("template.json").is_file() {
            resolved = Some(load_template_meta(&path).map(|mut meta| {
                meta.path = Some(path.display().to_string());
                meta
            })?);
        }
    }

    let managed_path = managed_templates_root().join(id);
    if let Some(meta) = instance_template(&managed_path)? {
        if resolved.is_some() {
            anyhow::bail!("managed visual template id collides with bundled template: {id}");
        }
        resolved = Some(meta);
    }

    resolved.ok_or_else(|| anyhow::anyhow!("unknown visual template: {id}"))
}

fn build_template_index(visuals_root: &Path) -> anyhow::Result<BTreeMap<String, TemplateMeta>> {
    with_template_index(visuals_root, Ok)
}

fn with_template_index<T, F>(visuals_root: &Path, consume: F) -> anyhow::Result<T>
where
    T: Send,
    F: FnOnce(BTreeMap<String, TemplateMeta>) -> anyhow::Result<T> + Send,
{
    // Visual creation commonly runs inside a Tokio worker that is already
    // carrying the optimizer admission future. Keep registry discovery and
    // manifest decoding off that comparatively small stack. The complete map
    // must also be consumed and dropped here: returning it to the Tokio worker
    // merely moves the stack-heavy BTreeMap/serde teardown back onto the stack
    // this boundary is intended to protect.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("visual-template-index".into())
            // Debug desktop builds retain substantially larger serde/path
            // frames than release builds.  Recipe admission resolves several
            // templates while its async state is live, and 8 MiB has proven
            // insufficient on macOS (the process aborts instead of returning
            // an ordinary template error).  Keep that work isolated and give
            // the bounded registry scan enough headroom.
            .stack_size(32 * 1024 * 1024)
            .spawn_scoped(scope, move || {
                let templates = build_template_index_inner(visuals_root)?;
                consume(templates)
            })
            .map_err(|error| anyhow::anyhow!("failed to start visual template indexer: {error}"))?
            .join()
            .map_err(|_| anyhow::anyhow!("visual template indexer panicked"))?
    })
}

fn build_template_index_inner(
    visuals_root: &Path,
) -> anyhow::Result<BTreeMap<String, TemplateMeta>> {
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
            let Some(meta) = instance_template(&path)? else { continue; };
            if templates.contains_key(&meta.id) {
                anyhow::bail!(
                    "managed visual template id collides with bundled template: {}",
                    meta.id
                );
            }
            templates.insert(meta.id.clone(), meta);
        }
    }
    Ok(templates)
}

/// Managed templates are one instance's imports, never part of the shipped
/// visuals package.
///
/// This carried its own `SYNTH_DESKTOP_DATA_ROOT` lookup and fell back to
/// `visuals_root()`, so with the variable unset an import wrote into the
/// package directory itself — contaminating the source tree, and adding that
/// instance's imported template to every later build's template index. Resolve
/// the instance the one way the rest of the app resolves it, which also honours
/// a bundle descriptor and the canonical data root.
fn managed_templates_root() -> PathBuf {
    crate::instance::data_root().join("visuals").join("templates")
}

pub(super) fn user_template_path(id: &str) -> anyhow::Result<PathBuf> {
    let root = managed_templates_root();
    let path = root.join(id);
    if path.parent() != Some(root.as_path()) || path.file_name().and_then(|name| name.to_str()) != Some(id) {
        anyhow::bail!("invalid user visual template id");
    }
    Ok(path)
}

fn checked_template_file(path: &Path) -> anyhow::Result<bool> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => anyhow::bail!("template requires a regular file: {}", path.display()),
        Ok(meta) if meta.len() > MANAGED_TEMPLATE_MAX_BYTES => anyhow::bail!("template exceeds size limit"),
        Ok(_) => Ok(true),
    }
}

pub(super) fn instance_template(path: &Path) -> anyhow::Result<Option<TemplateMeta>> {
    let dir = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        other => other?,
    };
    if dir.file_type().is_symlink() || !dir.is_dir() { anyhow::bail!("template directory must not be a symlink"); }
    if !checked_template_file(&path.join("template.json"))? { return Ok(None); }
    let html = checked_template_file(&path.join("renderer.html"))?;
    let tsx = checked_template_file(&path.join("shell.tsx"))?;
    if html && tsx { anyhow::bail!("template cannot contain both renderer.html and shell.tsx"); }
    if !html && !tsx { return Ok(None); }
    let mut meta = load_template_meta(path)?;
    meta.path = Some(path.display().to_string());
    if tsx {
        meta.source_kind = Some("user".into());
        meta.renderer_kind = Some("template".into());
        meta.shell_path = Some(path.join("shell.tsx").display().to_string());
    } else {
        meta.source_kind = Some("managed".into());
        meta.renderer_path = Some(path.join("renderer.html").display().to_string());
    }
    Ok(Some(meta))
}

pub(super) fn prepare_user_save(id: &str, manifest: &str, source: &str) -> anyhow::Result<PreparedManagedImport> {
    let destination = user_template_path(id)?;
    if destination.join("renderer.html").exists() { anyhow::bail!("cannot replace an HTML package with a TSX template"); }
    if let Ok(existing) = resolve_template(id) {
        if existing.source_kind.as_deref() != Some("user") { anyhow::bail!("cannot overwrite a bundled template; fork under a new id"); }
    }
    if manifest.len() as u64 > MANAGED_TEMPLATE_MAX_BYTES || source.len() > 256 * 1024 { anyhow::bail!("user template exceeds size limit"); }
    let meta = decode_template_meta(&destination, manifest.as_bytes(), String::new())?;
    Ok(PreparedManagedImport { meta, manifest: manifest.as_bytes().to_vec(), renderer: source.as_bytes().to_vec(), destination, renderer_file: "shell.tsx", source_kind: "user" })
}

/// Legacy synchronous seam: no broker means no permission to persist code.
/// The approved path prepares exactly two files, obtains consent, then writes
/// those immutable bytes through `PreparedManagedImport::persist`.
pub fn import_managed_template(source_path: &str) -> anyhow::Result<TemplateMeta> {
    let _ = source_path;
    Err(crate::session::template_persist::unapproved())
}

pub(crate) struct PreparedManagedImport {
    meta: TemplateMeta,
    manifest: Vec<u8>,
    renderer: Vec<u8>,
    destination: PathBuf,
    renderer_file: &'static str,
    source_kind: &'static str,
}

impl PreparedManagedImport {
    pub(crate) fn request(&self) -> anyhow::Result<crate::session::template_persist::PersistRequest> {
        let other_file = if self.source_kind == "user" { "renderer.html" } else { "shell.tsx" };
        if checked_template_file(&self.destination.join(other_file))? {
            anyhow::bail!("template cannot change renderer tier during approval");
        }
        if let Ok(meta) = fs::symlink_metadata(&self.destination) {
            if !meta.is_dir() || meta.file_type().is_symlink() {
                anyhow::bail!("managed template destination must be a real directory");
            }
        }
        let mut digest = Sha256::new();
        digest.update(self.renderer_file.as_bytes());
        digest.update((self.manifest.len() as u64).to_le_bytes());
        digest.update(&self.manifest);
        digest.update((self.renderer.len() as u64).to_le_bytes());
        digest.update(&self.renderer);
        Ok(crate::session::template_persist::PersistRequest {
            template_id: self.meta.id.clone(),
            destination: self.destination.display().to_string(),
            package_digest: format!("sha256:{:x}", digest.finalize()),
            byte_size: (self.manifest.len() + self.renderer.len()) as u64,
            overwrites: self.destination.exists(),
            source_kind: self.source_kind.into(),
        })
    }

    pub(crate) fn persist(mut self, consent: crate::session::template_persist::PersistConsent) -> anyhow::Result<TemplateMeta> {
        consent.bind(&self.request()?)?;
        fs::create_dir_all(&self.destination)?;
        for (name, bytes) in [("template.json", &self.manifest), (self.renderer_file, &self.renderer)] {
            // Atomic file replacement does not follow an existing file symlink.
            let mut file = tempfile::NamedTempFile::new_in(&self.destination)?;
            std::io::Write::write_all(&mut file, bytes)?;
            file.persist(self.destination.join(name))?;
        }
        self.meta = load_template_meta(&self.destination)?;
        self.meta.path = Some(self.destination.display().to_string());
        if self.source_kind == "managed" {
            self.meta.renderer_path = Some(self.destination.join(self.renderer_file).display().to_string());
        } else {
            self.meta.renderer_kind = Some("template".into());
            self.meta.shell_path = Some(self.destination.join(self.renderer_file).display().to_string());
        }
        self.meta.source_kind = Some(self.source_kind.into());
        Ok(self.meta)
    }
}

pub(crate) fn prepare_managed_import(source_path: &str) -> anyhow::Result<PreparedManagedImport> {
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
    let manifest_bytes = fs::read(&manifest)?;
    let meta = load_template_meta_bytes(&source, &manifest_bytes)?;
    let renderer_bytes = fs::read(&renderer)?;
    if manifest_bytes.len() as u64 > MANAGED_TEMPLATE_MAX_BYTES || renderer_bytes.len() as u64 > MANAGED_TEMPLATE_MAX_BYTES {
        anyhow::bail!("managed template file exceeds {MANAGED_TEMPLATE_MAX_BYTES} bytes");
    }
    validate_managed_renderer(&renderer_bytes)?;
    let destination = managed_templates_root().join(&meta.id);
    Ok(PreparedManagedImport { meta, manifest: manifest_bytes, renderer: renderer_bytes, destination, renderer_file: "renderer.html", source_kind: "managed" })
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
    load_template_meta_bytes(path, &fs::read(path.join("template.json"))?)
}

fn load_template_meta_bytes(path: &Path, raw: &[u8]) -> anyhow::Result<TemplateMeta> {
    decode_template_meta(path, raw, template_package_digest(path)?)
}

fn decode_template_meta(path: &Path, raw: &[u8], template_digest: String) -> anyhow::Result<TemplateMeta> {
    let value: Value = serde_json::from_slice(raw)?;
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
    let observation_contract: Option<TemplateObservationContract> = value
        .get("observationContract")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .with_context(|| format!("template {id} has an invalid observationContract"))?;
    if let Some(declared) = observation_contract
        .as_ref()
        .and_then(|contract| contract.readiness.authoring_affordances.as_ref())
    {
        if declared.is_empty() {
            anyhow::bail!("template {id} authoringAffordances must not be empty");
        }
        let unique = declared.iter().copied().collect::<BTreeSet<_>>();
        if unique.len() != declared.len() {
            anyhow::bail!("template {id} authoringAffordances contains duplicates");
        }
    }
    let mut meta = TemplateMeta {
        schema_version,
        id,
        template_digest,
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
        renderer_kind: value
            .get("rendererKind")
            .and_then(Value::as_str)
            .map(str::to_string),
        example_binding: None,
        binding_schema: declared.clone(),
        inputs: declared.clone(),
        slots: declared,
        components: value
            .get("components")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        observation_contract,
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

