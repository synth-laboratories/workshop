use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const MANAGED_TEMPLATE_MAX_BYTES: u64 = 1_500_000;

/// What a template requires before its current revision can be called ready.
///
/// Two different observers answer these. `minimum_rollout_count`,
/// `minimum_rendered_frame_count`, `minimum_semantic_event_count` and
/// `require_terminal` are read from the *rendered* observation the pane
/// publishes: claims about what the projector folded and the DOM then drew.
/// `minimum_transport_envelope_count` is read from the host's own stream
/// receipt at the poll seam: a claim about what arrived, before any fold has an
/// opinion about it. Keeping them separate is the whole point — see that
/// field's note.
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
    /// Non-control envelopes the transport must have delivered, counted by the
    /// host's stream receipt rather than by the pane.
    ///
    /// Deliberately *not* `minimum_semantic_event_count`. That number is a
    /// claim about what the projector produced, and only the fold can answer
    /// it; the receipt counts at the transport level, where a heartbeat and a
    /// verifier result are told apart by envelope kind and nothing more. A
    /// template that renders one summary line out of a hundred envelopes, and a
    /// template that fans one envelope into a hundred rows, both exist — so
    /// satisfying a projector claim with a transport count would certify a fold
    /// nobody ran, and satisfying a transport claim with a projector count
    /// would veto a stream that did arrive. Two observers, two knobs.
    ///
    /// Defaults to 0, so a template that says nothing here keeps exactly the
    /// behaviour it had before the receipt gate existed.
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub minimum_transport_envelope_count: u64,
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
    /// TSX entry point. Bundled families resolve it through Vite's static
    /// graph; a `source_kind: "user"` template is compiled in the pane from
    /// this file through `compileSourcedModule`.
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
    for (_, meta) in build_template_index(&visuals_root())?.templates {
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

/// The user-tier directories this instance had to leave out of the catalog.
///
/// `list_templates` answers "what can I render", and a skipped template is by
/// definition not in that answer. `resolve_template` explains one skip, but
/// only to a caller who already holds the id — and the author whose template
/// just vanished from the catalog is precisely the caller who does not. Without
/// a listing the skip is recorded and unreadable: three audiences were promised
/// the reason and one of them had no way to ask.
///
/// This is that surface, and nothing more: one entry per directory that claimed
/// to be a template and was not a usable one, carrying the refusal that
/// produced it. Bundled and staged tiers never appear here — they still fail
/// the whole index loudly, so there is no such thing as a silently skipped
/// shipped template to list.
///
/// Not yet reachable outside this module: `mod templates` is private, so a
/// caller needs `list_skipped_templates` added to the `pub use templates::{…}`
/// list in `visuals/mod.rs`. Drop the `allow(dead_code)` below with it.
pub fn list_skipped_templates() -> anyhow::Result<Vec<SkippedUserTemplate>> {
    Ok(build_template_index(&visuals_root())?.skipped)
}

pub fn resolve_template(template_id: &str) -> anyhow::Result<TemplateMeta> {
    let id = template_id.trim();
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        anyhow::bail!("invalid template id");
    }
    let mut index = build_template_index(&visuals_root())?;
    if let Some(meta) = index.templates.remove(id) {
        return Ok(meta);
    }
    // Not in the registry. If the user tier had to skip a directory of that
    // name, *why it skipped* is the answer to the question actually asked, and
    // this is where the existing callers already look: `validate_user_template`
    // quotes this error verbatim as its `refused` finding, and `write_verified`
    // rolls a bad save back on it. Saying "unknown" here is precisely what
    // would make a skip invisible.
    if let Some(skipped) = index
        .skipped
        .iter()
        .find(|entry| entry.id.as_deref() == Some(id))
    {
        anyhow::bail!("visual template {id} was skipped: {}", skipped.reason);
    }
    anyhow::bail!("unknown visual template: {id}")
}

/// The registry, plus the user-tier directories it had to leave out of it.
///
/// The skip list is not a diagnostic nobody reads. `resolve_template` turns an
/// entry back into the error for that exact id, so the author who just broke a
/// template is told what is wrong with it instead of being told it does not
/// exist.
#[derive(Debug)]
struct TemplateIndex {
    templates: BTreeMap<String, TemplateMeta>,
    skipped: Vec<SkippedUserTemplate>,
}

/// One user-tier directory that claimed to be a template and was not a usable
/// one, kept beside the index instead of thrown away.
///
/// A silently skipped template is its own failure mode — nothing renders and
/// there is nothing to read — so every skip lands in four places with four
/// different audiences: the operational log at the moment of the skip (the
/// operator, who never asked for this template by name), this list (the index
/// itself), the `resolve_template` error for the id (the author, through the
/// authoring tools they are already holding), and `list_skipped_templates`
/// (anyone who has to find the id before they can ask about it).
///
/// Serializable because that last audience reaches it over IPC, where the
/// catalog listing it is missing from already travels as `TemplateMeta`.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkippedUserTemplate {
    /// The id the directory claims by its name. A user template's manifest id
    /// must equal its directory name, so the name is the id to look up even
    /// when the manifest is the broken thing — which is the case that most
    /// needs an answer. `None` only for a non-UTF-8 directory name.
    pub id: Option<String>,
    /// Why it was skipped, verbatim from the refusal that produced it. The
    /// directory is `<user template root>/<id>`, so this does not repeat it.
    pub reason: String,
}

/// Every template tier this instance can render, lowest precedence first:
/// the bundled `families/` recursion, then the `templates`/`templates-internal`
/// overlays beside it, then the instance-local user root.
///
/// Each tier is skipped when its directory is absent, and a missing bundled
/// `families/` is just another absent tier. It used to return an empty map
/// instead, which put every later tier behind a directory none of them live
/// in: a build whose bundled families root had not been staged silently had no
/// user templates at all — the same dead-registry shape as the doubled
/// `visuals/visuals/templates` path that once killed the managed tier.
///
/// **The tiers do not share a failure policy, deliberately.** A duplicate or
/// malformed template under `families/` — or under the `templates` /
/// `templates-internal` overlays staged beside it — is a defect in what we
/// shipped: nobody on this machine can fix it, and building past it would hide
/// it, so it still fails the whole index loudly. A malformed directory under
/// the *instance-local user root* is a document somebody is in the middle of
/// writing. Failing the index there took every other template down with it,
/// bundled families included; and because `visual_template_save` and
/// `visual_template_validate` rebuild this index on every call, one bad
/// directory locked the author out of the only tools that could have fixed it.
/// So the user tier skips the directory and records why. One bad template
/// breaks only itself, visibly.
fn build_template_index(visuals_root: &Path) -> anyhow::Result<TemplateIndex> {
    let mut templates: BTreeMap<String, TemplateMeta> = BTreeMap::new();
    let families_root = visuals_root.join("families");
    if families_root.exists() {
        let canonical_root = fs::canonicalize(&families_root)?;
        let mut directories = Vec::new();
        discover_template_directories(&families_root, &canonical_root, &mut directories)?;
        directories.sort();

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
    let skipped = scan_user_template_root(&user_templates_root(), &mut templates);
    Ok(TemplateIndex { templates, skipped })
}

/// One directory under the user template root is exactly one of two shapes.
///
/// The two are not interchangeable: they are rendered by different machinery
/// and therefore carry different capability models. Which files are present
/// decides which one a directory is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UserTemplateShape {
    /// `template.json` + `renderer.html`. A reviewed, networkless HTML package
    /// rendered in a sandboxed iframe under a CSP, immutable after import.
    Managed,
    /// `template.json` + `shell.tsx`. Agent- or human-authored TSX compiled in
    /// the pane through `compileSourcedModule`, so it inherits the whole
    /// sourced capability model: allowlisted imports, no `fetch` /
    /// `EventSource` / `WebSocket` / `eval` / `window` / `import.meta`.
    User,
}

impl UserTemplateShape {
    fn source_kind(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::User => "user",
        }
    }
}

/// Instance-local templates a user or agent wrote, in either shape.
///
/// **Rust does structural validation only** — manifest schema version, id
/// equals directory, regular files, no symlinks, size cap. It deliberately does
/// not parse or lint `shell.tsx`. The import allowlist and forbidden-token scan
/// live in `visuals/runtime/sourcedValidate.ts` and run in the pane, which
/// fails closed and renders `sourcedInvalidShell` with an exact message. A
/// second copy of that rule here would be a second implementation to drift, and
/// removing exactly that class of duplicate is the point of this work.
///
/// Returns the directories it could not index, and why — never an error. This
/// tier cannot fail the index; see `build_template_index` for why the two tiers
/// are trusted differently.
fn scan_user_template_root(
    root: &Path,
    templates: &mut BTreeMap<String, TemplateMeta>,
) -> Vec<SkippedUserTemplate> {
    let mut skipped = Vec::new();
    if !root.exists() {
        return skipped;
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        // The root itself is unreadable. That is one directory failing, not a
        // reason for the bundled families to stop resolving, so it is recorded
        // like any other user-tier skip — but with no id, because the root is
        // not a template and must never answer a `resolve_template` lookup for
        // one named after it.
        Err(error) => {
            record_skip(
                &mut skipped,
                root,
                None,
                anyhow::Error::new(error).context(format!("reading {}", root.display())),
            );
            return skipped;
        }
    };
    let mut entries: Vec<_> = entries.filter_map(|entry| entry.ok()).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        match index_user_template(&path, templates) {
            Ok(Some(meta)) => {
                templates.insert(meta.id.clone(), meta);
            }
            Ok(None) => {}
            Err(error) => {
                let id = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string);
                record_skip(&mut skipped, &path, id, error)
            }
        }
    }
    skipped
}

/// Index one directory under the user template root.
///
/// `Ok(None)` means there is nothing here to index and nothing wrong: a stray
/// file, a directory with no manifest, a manifest with no source yet. Those
/// were never errors and are not skips either — recording them would bury the
/// real ones.
///
/// `Err` means this directory claims to be a template and is not a valid one.
/// Every refusal below used to abort the whole index; the caller now turns it
/// into a recorded skip. Note that none of them get *weaker* by being scoped:
/// the symlink refusals still refuse — a symlinked user template is still never
/// followed, still never read, still never indexed. What changed is only the
/// blast radius, from "no template on this machine resolves" to "this one does
/// not".
fn index_user_template(
    path: &Path,
    indexed: &BTreeMap<String, TemplateMeta>,
) -> anyhow::Result<Option<TemplateMeta>> {
    // This root is agent-writable, so the scan refuses symlinks the way the
    // families recursion and `import_managed_template` always have, rather than
    // following one out of the instance state root.
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("reading {}", path.display()))?;
    if metadata.file_type().is_symlink() {
        anyhow::bail!(
            "user visual template registry refuses symlink: {}",
            path.display()
        );
    }
    if !metadata.is_dir() {
        return Ok(None);
    }
    if checked_template_file(&path.join("template.json"))?.is_none() {
        return Ok(None);
    }
    let renderer = path.join("renderer.html");
    let shell = path.join("shell.tsx");
    let has_renderer = checked_template_file(&renderer)?.is_some();
    let has_shell = checked_template_file(&shell)?.is_some();
    let shape = match (has_renderer, has_shell) {
        // Ambiguous, so fail closed rather than pick. The two shapes render
        // through different machinery under different capability models;
        // silently preferring one would mean the file the author edits is
        // not the file that runs, and would let whoever can write only the
        // other file flip which model applies to an existing template.
        (true, true) => anyhow::bail!(
            "user visual template declares both renderer.html and shell.tsx: {}",
            path.display()
        ),
        (true, false) => UserTemplateShape::Managed,
        (false, true) => UserTemplateShape::User,
        // A manifest with neither source is a scaffold, not a template yet.
        // Skipped as it always has been, never an error.
        (false, false) => return Ok(None),
    };
    let mut meta = load_template_meta(path)?;
    if indexed.contains_key(&meta.id) {
        anyhow::bail!(
            "managed visual template id collides with bundled template: {}",
            meta.id
        );
    }
    meta.path = Some(path.display().to_string());
    match shape {
        UserTemplateShape::Managed => {
            meta.renderer_path = Some(renderer.display().to_string());
            meta.shell_path = None;
        }
        UserTemplateShape::User => {
            meta.shell_path = Some(shell.display().to_string());
            meta.renderer_path = None;
        }
    }
    meta.source_kind = Some(shape.source_kind().into());
    Ok(Some(meta))
}

/// Record one skipped user-tier directory, and say so out loud.
///
/// The log line is the surface for the case `resolve_template` cannot cover:
/// nobody asks a broken template for its id when they do not know it broke.
fn record_skip(
    skipped: &mut Vec<SkippedUserTemplate>,
    path: &Path,
    id: Option<String>,
    error: anyhow::Error,
) {
    let reason = format!("{error:#}");
    eprintln!(
        "synth-desktop: skipped user visual template {}: {reason}",
        path.display()
    );
    skipped.push(SkippedUserTemplate { id, reason });
}

/// Structural check for one file in a user template directory: absent is
/// `Ok(None)`, a usable regular file is `Ok(Some(len))`, and anything else is a
/// named error rather than a silent skip.
///
/// `MANAGED_TEMPLATE_MAX_BYTES` gated only `import_managed_template` before, so
/// a hand-edited or agent-written file entered the registry uncapped. It is
/// applied here too, to the manifest and to whichever source the directory
/// declares. The pane keeps its own, stricter 256 KiB cap on sourced TSX; this
/// one is the structural backstop, not a replacement for it.
fn checked_template_file(path: &Path) -> anyhow::Result<Option<u64>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", path.display()));
        }
    };
    if metadata.file_type().is_symlink() {
        anyhow::bail!(
            "user visual template registry refuses symlink: {}",
            path.display()
        );
    }
    if !metadata.is_file() {
        anyhow::bail!(
            "user visual template entry must be a regular file: {}",
            path.display()
        );
    }
    if metadata.len() > MANAGED_TEMPLATE_MAX_BYTES {
        anyhow::bail!(
            "user visual template file exceeds {MANAGED_TEMPLATE_MAX_BYTES} bytes: {}",
            path.display()
        );
    }
    Ok(Some(metadata.len()))
}

/// User-authored visual templates for this instance, beside `config.toml` and
/// `.env`: `<state root>/visuals/templates`.
///
/// This resolved the data root itself, and a local copy of that rule drifted
/// exactly the way `instance_paths.rs` says local copies do. It read
/// `SYNTH_DESKTOP_DATA_ROOT` directly and fell back to `visuals_root()`, which
/// already ends in `visuals`, then joined `visuals/templates` onto it. Unset
/// environment therefore produced `<root>/visuals/visuals/templates`, a path
/// that cannot exist: every install that is not the dev launcher — canonical
/// installs and descriptor-launched bundles, which is to say production — ran
/// with the whole registry silently dead. In a packaged app it was worse than
/// dead. `visuals_root()` there is `<App>.app/Contents/Resources/visuals`, so
/// `import_managed_template` wrote into the signed application bundle and
/// invalidated its signature.
///
/// `instance::state_root()` is the one rule: the instance data root when a
/// descriptor or `SYNTH_DESKTOP_DATA_ROOT` names one, `~/.synth-desktop`
/// otherwise. Resolve from there, never from where the shipped visuals live.
pub(super) fn user_templates_root() -> PathBuf {
    crate::instance::state_root()
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
    let destination = user_templates_root().join(&meta.id);
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

