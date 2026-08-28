//! Read the `shell.tsx` of a user-authored visual template.
//!
//! `templates.rs` decides *what* a user template is: a directory under
//! `<state root>/visuals/templates` holding `template.json` plus `shell.tsx`,
//! indexed with `source_kind: "user"` and `shell_path` set. This module answers
//! the single question the pane then asks — "give me that file's text" — and
//! nothing else.
//!
//! It deliberately does not parse or lint the TSX. The import allowlist, the
//! forbidden-token scan and the 256 KiB pane cap live once, in
//! `visuals/runtime/sourcedValidate.ts`, which fails closed and renders
//! `sourcedInvalidShell` with the exact message a human editing the file sees.
//! A second copy of that rule here would be a second thing to drift.
//!
//! **The structural gate runs before any byte is read.** `resolve_template`
//! rebuilds the whole index on every call — there is no cache — and the user
//! tier runs `templates.rs::checked_template_file` over `template.json` and
//! `shell.tsx` as it scans. A symlink, a non-regular file, or a file above
//! `MANAGED_TEMPLATE_MAX_BYTES` therefore fails this command with templates.rs's
//! own message, from templates.rs's own check, before `read_to_string` is
//! reached. Calling that gate a second time directly would be the clearer
//! spelling; it is private to `templates.rs`, which this change may not edit.

use super::templates::TemplateMeta;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};
use tauri::Emitter;

/// `source_kind` tag `templates.rs` puts on a `template.json` + `shell.tsx`
/// directory. The pane branches on this, never on a template id.
pub const USER_SOURCE_KIND: &str = "user";

/// TSX source of one user-authored template's shell.
///
/// Refuses anything that is not `source_kind: "user"` — a bundled family
/// resolves its shell through Vite's static graph, and a `managed` package is
/// `renderer.html` rendered in a sandboxed iframe under a CSP. Handing either
/// one's source to `compileSourcedModule` would run it under a capability model
/// its author never agreed to.
pub fn shell_source(template_id: &str) -> Result<String> {
    // Also the id gate: `resolve_template` refuses an empty id, a separator and
    // `..` before it looks anything up, and an id it does not know.
    let meta = super::templates::resolve_template(template_id)?;

    let source_kind = meta.source_kind.as_deref().unwrap_or("bundled");
    if source_kind != USER_SOURCE_KIND {
        bail!(
            "visual template {template_id} has source kind {source_kind:?}; \
             only a user-authored template exposes shell source"
        );
    }

    let shell_path = meta
        .shell_path
        .as_deref()
        .ok_or_else(|| anyhow!("user visual template {template_id} declares no shell.tsx"))?;
    let shell = Path::new(shell_path);

    // Containment against the one rule that owns this tier's location, rather
    // than against a property it happens to imply. `user_templates_root()` is
    // `templates.rs`'s and is now `pub(super)` precisely so this check does not
    // re-derive the path — re-deriving it is what `template_root_join` in
    // `scripts/conform-desktop.sh` counts, and what item 23 was.
    let root = super::templates::user_templates_root();
    if !shell.starts_with(&root) {
        bail!(
            "user visual template shell is outside the user template root: {}",
            shell.display()
        );
    }

    // The scan gated this file microseconds ago; re-check the one property an
    // attacker could flip in between, because the swap costs nothing and the
    // consequence is reading a file outside the root. This is the same refusal
    // with the same words, not a second policy: the size cap already ran in the
    // scan, and the pane re-caps at 256 KiB.
    let metadata =
        fs::symlink_metadata(shell).with_context(|| format!("reading {}", shell.display()))?;
    if metadata.file_type().is_symlink() {
        bail!(
            "user visual template registry refuses symlink: {}",
            shell.display()
        );
    }
    if !metadata.is_file() {
        bail!(
            "user visual template entry must be a regular file: {}",
            shell.display()
        );
    }

    fs::read_to_string(shell).with_context(|| {
        format!(
            "user visual template shell must be UTF-8: {}",
            shell.display()
        )
    })
}

// ---------------------------------------------------------------------------
// Authoring: promote a one-off into a durable template (item 26).
// ---------------------------------------------------------------------------

/// The two file names `templates.rs` looks for in a user template directory.
///
/// These are copies of literals that belong to `templates.rs`, which owns what
/// a user template *is*. They are copied because `templates.rs` names them
/// inline rather than as constants and this change may not edit that file — a
/// writer has to name the file it creates before the scanner can find it.
/// Folding both into `pub(super) const` there is a two-line change and is the
/// only correct home for them.
const MANIFEST_FILE: &str = "template.json";
const SHELL_FILE: &str = "shell.tsx";

/// The other shape a directory under the same root can take. Never written
/// here — `import_managed_template` owns it — but named so the writer can say
/// why it refuses a directory that already holds one.
const RENDERER_FILE: &str = "renderer.html";

/// Consent to persist code that the app compiles at every launch.
///
/// Writing `shell.tsx` under the instance state root is a different act from
/// rendering TSX in the pane. The pane compiles a string that dies with the
/// view; this writes a file the registry scans on every launch, for every
/// future session, until someone deletes it. `internal_readme.md` fixes the
/// precedent for that class — entering an API key "creates persistent access
/// and must follow the Computer Use confirmation policy" — and part V trap 2
/// names the mechanism: a new `ApprovalKind` variant through the existing
/// `ApprovalBroker`, modeled on `PluginLifecycle`.
///
/// **This type is a speed bump, not that gate.** It carries no receipt and
/// checks nothing. What it does is put the question in the signature of every
/// write, so the seam that first lets an *agent* reach these functions cannot
/// be written without either adding a variant here or claiming, in reviewable
/// source, that a human asked for it. Today only the Tauri command layer calls
/// them, which is the person driving this window; `visuals_ipc.rs` — the seam
/// the MCP tools proxy through — has no route to them at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistConsent {
    /// The person driving this window asked for the write, in this window.
    /// Their click is the confirmation.
    HumanInPane,
    // Item 29 adds `ApprovedByBroker(ApprovalReceipt)` here, and the agent
    // seam constructs that one. Adding the agent seam without adding this
    // variant is the mistake this enum exists to make visible.
}

/// Structural verdict on one user template directory. A verdict rather than a
/// `Result` because "manifest is fine, no shell.tsx yet" is the normal
/// mid-authoring state, not an error.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UserTemplateValidation {
    pub schema_version: String,
    pub id: String,
    /// Absolute directory this id names, even when nothing is there yet.
    pub path: String,
    /// True when the registry would index this directory as a user template.
    pub ok: bool,
    /// `templates.rs`'s tag, present only once the directory indexes.
    pub source_kind: Option<String>,
    pub findings: Vec<UserTemplateFinding>,
    /// Where the import allowlist and forbidden-token scan actually run, said
    /// out loud so silence here cannot read as approval.
    pub source_scan: String,
}

/// One reason a directory is not, or not yet, a user template.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UserTemplateFinding {
    /// Stable machine tag. `message` is what a person reads.
    pub code: String,
    /// Verbatim from `templates.rs` wherever it came from there, so an author
    /// reads the same sentence the registry used to refuse the file.
    pub message: String,
}

const VALIDATION_SCHEMA: &str = "synth.user-template-validation.v1";
const SOURCE_SCAN_OWNER: &str = "Import allowlist and forbidden-token scan run in the pane, in \
     visuals/runtime/sourcedValidate.ts. Open the template to see them; a \
     violation renders in the pane through sourcedInvalidShell with the same \
     message this command would have printed.";

/// The directory one id names, refused unless it is a direct child of the root.
///
/// This is path algebra, not a character blocklist: after `join`, the result's
/// parent must still be the root and its final component must still be the id.
/// An absolute id, `a/b`, `..`, `.`, and `""` all fail that, and none of them
/// needs to be enumerated. `templates.rs` has an id gate of its own on the read
/// path; this one exists because a write must be contained *before* the write,
/// not verified after it.
fn template_directory(id: &str) -> Result<PathBuf> {
    let root = super::templates::user_templates_root();
    let directory = root.join(id);
    let named = directory
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == id)
        .unwrap_or(false);
    if !named || directory.parent() != Some(root.as_path()) {
        bail!("invalid user visual template id: {id:?}");
    }
    Ok(directory)
}

/// Bytes a write is about to replace, and whether the directory is ours.
///
/// Capture refuses to overwrite anything that is not a regular file. That is
/// not a second copy of `checked_template_file`, which gates *reading* and can
/// therefore run after the fact: a write through a symlink has already escaped
/// the state root by the time any verification runs, so containment has to be
/// decided before the first byte.
struct DirectoryRestore {
    directory: PathBuf,
    created_directory: bool,
    previous: Vec<(PathBuf, Option<Vec<u8>>)>,
}

impl DirectoryRestore {
    fn capture(directory: &Path, files: &[&str]) -> Result<Self> {
        let created_directory = match fs::symlink_metadata(directory) {
            Ok(metadata) if metadata.file_type().is_symlink() => bail!(
                "user visual template registry refuses symlink: {}",
                directory.display()
            ),
            Ok(metadata) if metadata.is_dir() => false,
            Ok(_) => bail!(
                "user visual template path is not a directory: {}",
                directory.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", directory.display()))
            }
        };
        let mut previous = Vec::new();
        for name in files {
            let path = directory.join(name);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => bail!(
                    "user visual template registry refuses symlink: {}",
                    path.display()
                ),
                Ok(metadata) if metadata.is_file() => {
                    let bytes =
                        fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
                    previous.push((path, Some(bytes)));
                }
                Ok(_) => bail!(
                    "user visual template entry must be a regular file: {}",
                    path.display()
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    previous.push((path, None));
                }
                Err(error) => {
                    return Err(error).with_context(|| format!("reading {}", path.display()))
                }
            }
        }
        Ok(Self {
            directory: directory.to_path_buf(),
            created_directory,
            previous,
        })
    }

    /// Put the directory back the way it was. Best effort by construction: this
    /// runs while another failure is already being reported, and a rollback
    /// error would replace the real one with a worse one.
    fn restore(self) {
        for (path, bytes) in self.previous {
            match bytes {
                Some(bytes) => {
                    let _ = fs::write(&path, bytes);
                }
                None => {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        if self.created_directory {
            let _ = fs::remove_dir(&self.directory);
        }
    }
}

/// Write `files`, then make the registry itself say whether the result is a
/// template. Roll back and surface its refusal verbatim if it is not.
///
/// This is the whole reason nothing structural is re-derived here.
/// `resolve_template` rebuilds the index, which runs `checked_template_file`
/// (symlink, regular file, size cap), `load_template_meta` (schema version,
/// id equals directory, semantic version), the ambiguous-shape refusal and the
/// bundled-id collision refusal over exactly the bytes just written. A writer
/// that pre-validated with its own copies of those rules would be a second
/// implementation that drifts; a writer that validates by *asking the reader*
/// cannot drift, because there is only one reader.
fn write_verified(directory: &Path, id: &str, files: &[(&str, Vec<u8>)]) -> Result<TemplateMeta> {
    let names: Vec<&str> = files.iter().map(|(name, _)| *name).collect();
    let restore = DirectoryRestore::capture(directory, &names)?;
    let written = (|| -> Result<()> {
        fs::create_dir_all(directory)
            .with_context(|| format!("creating {}", directory.display()))?;
        for (name, bytes) in files {
            let path = directory.join(name);
            fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
        }
        Ok(())
    })();
    if let Err(error) = written {
        restore.restore();
        return Err(error);
    }
    match super::templates::resolve_template(id) {
        Ok(meta) if meta.source_kind.as_deref() == Some(USER_SOURCE_KIND) => Ok(meta),
        Ok(meta) => {
            restore.restore();
            let kind = meta.source_kind.as_deref().unwrap_or("bundled");
            bail!(
                "visual template {id} is already a {kind} template; user templates may add an \
                 id, never redefine one. Fork it under a new id instead."
            )
        }
        Err(error) => {
            restore.restore();
            Err(error)
        }
    }
}

/// Rewrite one manifest so it declares `id`, and record where it came from.
///
/// `load_template_meta` refuses a manifest whose `id` is not its directory, so
/// a fork that kept the family's id would be refused after the write and rolled
/// back — correct, but a worse error than simply stamping the id the caller
/// asked for. `forkedFrom` is provenance the scanner ignores today; item 28
/// wants it when a seal has to embed which shipped family a user template
/// diverged from.
fn manifest_for(
    mut manifest: Value,
    id: &str,
    title: Option<&str>,
    forked_from: Option<&TemplateMeta>,
) -> Result<Vec<u8>> {
    let object = manifest
        .as_object_mut()
        .ok_or_else(|| anyhow!("visual template manifest must be a JSON object"))?;
    // An author writing a manifest must not disagree with the directory they
    // are writing into: silently rewriting their declared id would make the
    // file on disk differ from the file they wrote. A fork is the opposite
    // case — its manifest is a *copy* whose id still names the origin, and
    // rewriting it is the entire point — so the guard applies only when this
    // is not a fork. `load_template_meta` enforces id-equals-directory on the
    // bytes either way, so neither path can produce an unindexable template.
    if forked_from.is_none() {
        if let Some(declared) = object.get("id").and_then(Value::as_str) {
            if declared != id {
                bail!("visual template manifest declares id {declared:?}, not {id:?}");
            }
        }
    }
    object.insert("id".into(), json!(id));
    if let Some(title) = title {
        object.insert("title".into(), json!(title));
    }
    if let Some(source) = forked_from {
        object.insert(
            "forkedFrom".into(),
            json!({ "templateId": source.id.as_str(), "version": source.version.as_deref() }),
        );
    }
    let mut bytes = serde_json::to_vec_pretty(&manifest)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Persist one user-authored template: `<user template root>/<id>/`.
///
/// `manifest_json` is the raw `template.json` text rather than a typed struct,
/// because the manifest schema is the template package's, not the host's —
/// `load_template_meta` reads a handful of keys and carries the rest through,
/// and a typed argument here would quietly drop every key the host does not
/// happen to know.
pub fn save(
    id: &str,
    manifest_json: &str,
    source: &str,
    consent: PersistConsent,
) -> Result<TemplateMeta> {
    // Exhaustive on purpose: adding an approval variant must not compile
    // until every writer says what it does with it.
    match consent {
        PersistConsent::HumanInPane => {}
    }
    let directory = template_directory(id)?;
    if directory.join(RENDERER_FILE).exists() {
        bail!(
            "visual template {id} is a {RENDERER_FILE} package; a template is one shape or the \
             other, never both. Fork it under a new id instead."
        );
    }
    let manifest: Value = serde_json::from_str(manifest_json)
        .with_context(|| format!("visual template {id} manifest is not JSON"))?;
    let manifest = manifest_for(manifest, id, None, None)?;
    write_verified(
        &directory,
        id,
        &[
            (MANIFEST_FILE, manifest),
            (SHELL_FILE, source.as_bytes().to_vec()),
        ],
    )
}

/// Scaffold a new user template by copying a shipped family.
///
/// Fork, never shadow (part V trap 3): the copy lands under a new id, so a
/// shipped id keeps meaning exactly one thing. Copying an existing template is
/// what makes the scaffold worth having — it starts from a manifest whose
/// inputs, slots and observation contract already agree with a shell that
/// renders them, which is the part an author cannot guess.
pub fn create_from(
    id: &str,
    from_template_id: &str,
    title: Option<&str>,
    consent: PersistConsent,
) -> Result<TemplateMeta> {
    // Exhaustive on purpose: adding an approval variant must not compile
    // until every writer says what it does with it.
    match consent {
        PersistConsent::HumanInPane => {}
    }
    let directory = template_directory(id)?;
    let source = super::templates::resolve_template(from_template_id)?;
    let source_directory =
        source.path.as_deref().map(PathBuf::from).ok_or_else(|| {
            anyhow!("visual template {from_template_id} has no directory to copy")
        })?;
    let shell_path = source.shell_path.as_deref().ok_or_else(|| {
        anyhow!(
            "visual template {from_template_id} has no {SHELL_FILE} to copy; only a TSX template \
             can be forked (a {RENDERER_FILE} package is imported, not authored)"
        )
    })?;
    let manifest_text = fs::read_to_string(source_directory.join(MANIFEST_FILE))
        .with_context(|| format!("reading {from_template_id} {MANIFEST_FILE}"))?;
    let shell =
        fs::read(shell_path).with_context(|| format!("reading {from_template_id} {SHELL_FILE}"))?;
    let manifest: Value = serde_json::from_str(&manifest_text)
        .with_context(|| format!("visual template {from_template_id} manifest is not JSON"))?;
    let manifest = manifest_for(manifest, id, title, Some(&source))?;
    write_verified(
        &directory,
        id,
        &[(MANIFEST_FILE, manifest), (SHELL_FILE, shell)],
    )
}

/// Structural verdict on one user template directory.
///
/// **The allowlist scan is not here, on purpose.** `visuals/runtime/
/// sourcedValidate.ts` owns the eleven allowed imports, the forbidden-token
/// list, the 256 KiB pane cap and the guessed-stream-URL refusal; the pane runs
/// it on the exact bytes it is about to compile and renders any failure through
/// `sourcedInvalidShell`. Reimplementing that here would be a second scanner
/// with a second regex set on the same file — the duplicate this whole plan
/// exists to delete — and worse, one that could pass while the pane refuses, or
/// refuse while the pane runs the code. So Rust answers only what Rust can
/// answer alone: does this directory index, and as what. `source_scan` says so
/// out loud rather than letting silence read as approval.
pub fn validate(id: &str) -> UserTemplateValidation {
    let mut findings = Vec::new();
    let directory = match template_directory(id) {
        Ok(directory) => directory,
        Err(error) => {
            return UserTemplateValidation {
                schema_version: VALIDATION_SCHEMA.into(),
                id: id.into(),
                path: String::new(),
                ok: false,
                source_kind: None,
                findings: vec![UserTemplateFinding {
                    code: "invalid_id".into(),
                    message: error.to_string(),
                }],
                source_scan: SOURCE_SCAN_OWNER.into(),
            }
        }
    };
    let mut source_kind = None;
    match super::templates::resolve_template(id) {
        Ok(meta) if meta.source_kind.as_deref() == Some(USER_SOURCE_KIND) => {
            source_kind = meta.source_kind.clone();
        }
        Ok(meta) => {
            source_kind = Some(meta.source_kind.clone().unwrap_or_else(|| "bundled".into()));
            findings.push(UserTemplateFinding {
                code: "not_user_template".into(),
                message: format!(
                    "visual template {id} resolves to a {} template, not a user-authored one",
                    source_kind.as_deref().unwrap_or("bundled")
                ),
            });
        }
        Err(error) => {
            // Three different situations reach here and they need different
            // words: nothing on disk, a manifest with no source yet, and a
            // directory the registry actively refused. Only the last one has a
            // message worth quoting, and it is quoted verbatim.
            let manifest_present = is_regular_file(&directory.join(MANIFEST_FILE));
            let shell_present = is_regular_file(&directory.join(SHELL_FILE));
            let renderer_present = is_regular_file(&directory.join(RENDERER_FILE));
            if !directory.is_dir() {
                findings.push(UserTemplateFinding {
                    code: "missing_directory".into(),
                    message: format!(
                        "no user visual template directory at {}",
                        directory.display()
                    ),
                });
            } else if !manifest_present {
                findings.push(UserTemplateFinding {
                    code: "missing_manifest".into(),
                    message: format!(
                        "user visual template {id} has no {MANIFEST_FILE}; the registry skips a \
                         directory without one"
                    ),
                });
            } else if !shell_present && !renderer_present {
                findings.push(UserTemplateFinding {
                    code: "missing_shell".into(),
                    message: format!(
                        "user visual template {id} has a {MANIFEST_FILE} but no {SHELL_FILE}; the \
                         registry treats it as a scaffold and does not index it"
                    ),
                });
            } else {
                findings.push(UserTemplateFinding {
                    code: "refused".into(),
                    message: error.to_string(),
                });
            }
        }
    }
    UserTemplateValidation {
        schema_version: VALIDATION_SCHEMA.into(),
        id: id.into(),
        path: directory.display().to_string(),
        ok: findings.is_empty(),
        source_kind,
        findings,
        source_scan: SOURCE_SCAN_OWNER.into(),
    }
}

/// Present, and a regular file rather than a symlink or a directory.
///
/// Used only to choose which of several diagnoses to report, never to decide
/// whether a file may be read: a symlinked `template.json` must not read as
/// "present" here, or a directory the registry refused for that exact reason
/// would be reported as merely missing its shell.
fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Hot reload: notice an edit made outside the app (item 27).
// ---------------------------------------------------------------------------

/// How often the user template root is restatted.
///
/// This is a poll, not an fs event, because `notify` is not a dependency of
/// this crate and adding one is a lockfile change this work could not build or
/// verify. The seam is the right one either way: a real watcher replaces the
/// body of [`spawn_watcher`] and nothing downstream moves. The cost is a
/// `read_dir` over a directory that holds a handful of entries, which is
/// cheaper than the registry rebuild any of its consumers already do per call.
pub const WATCH_INTERVAL: Duration = Duration::from_millis(750);

/// Payload schema on [`EventChannel::VISUAL_TEMPLATES`].
pub const TEMPLATES_CHANGED_SCHEMA: &str = "synth.visual-templates-changed.v1";

/// Everything the registry would read out of the user template root, as one
/// digest: each directory entry's name, and for each of the three files it may
/// hold, its size and modification time.
///
/// Size and mtime rather than content because the question is "did anything
/// change", not "what changed", and the answer only has to be *different* — the
/// renderer re-reads and recompiles from disk on every bump. `symlink_metadata`
/// so that replacing a file with a symlink registers as a change rather than
/// silently reporting the target's stats.
fn root_fingerprint() -> String {
    let root = super::templates::user_templates_root();
    let mut hasher = Sha256::new();
    let mut entries: Vec<PathBuf> = match fs::read_dir(&root) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect(),
        // An absent root is a stable state with a stable digest, and it becomes
        // a change the moment the first template is saved.
        Err(_) => return format!("{:x}", Sha256::digest(b"absent")),
    };
    entries.sort();
    for entry in entries {
        hasher.update(entry.display().to_string().as_bytes());
        for name in [MANIFEST_FILE, SHELL_FILE, RENDERER_FILE] {
            let path = entry.join(name);
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                hasher.update(b"\0-");
                continue;
            };
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|since| since.as_nanos())
                .unwrap_or(0);
            hasher.update(format!("\0{name}:{}:{modified}", metadata.len()).as_bytes());
        }
    }
    format!("{:x}", hasher.finalize())
}

/// Watch the user template root and tell the renderer when it changes.
///
/// The renderer answers by calling `refreshRuntimeTemplates()`, which bumps the
/// registry generation, which re-runs `VisualHost`'s load effect, which re-reads
/// `shell.tsx` and recompiles it. So an author saves the file in their editor
/// and the pane remounts — including when the new source is invalid, which
/// renders through `sourcedInvalidShell` with the validator's exact message
/// rather than blanking.
///
/// Emits only on change, and never on start: the registry already loads once at
/// bridge install, and a spurious first event would remount every open pane for
/// nothing.
pub fn spawn_watcher(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last = root_fingerprint();
        loop {
            tokio::time::sleep(WATCH_INTERVAL).await;
            let next = root_fingerprint();
            if next == last {
                continue;
            }
            last = next;
            let _ = app.emit(
                crate::contract::events::EventChannel::VISUAL_TEMPLATES,
                json!({
                    "schemaVersion": TEMPLATES_CHANGED_SCHEMA,
                    "root": super::templates::user_templates_root().display().to_string(),
                }),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A checkout without staged bundled families has no bundled template to
    /// assert against. The user tier itself is independent of it.
    fn bundled_families_present() -> bool {
        super::super::templates::visuals_root()
            .join("families")
            .exists()
    }

    fn write_user_template(id: &str, shell: Option<&str>) -> PathBuf {
        let path = crate::instance::state_root()
            .join("visuals")
            .join("templates")
            .join(id);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("template.json"),
            format!(
                r#"{{"schemaVersion":"synth.visual-template.v1","id":"{id}","version":"1.0.0"}}"#
            ),
        )
        .unwrap();
        if let Some(source) = shell {
            fs::write(path.join("shell.tsx"), source).unwrap();
        }
        path
    }

    #[test]
    fn reads_user_shell_source() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-shell-source");
        let source = "export default function Shell() { return null; }\n";
        write_user_template("user.readable.v1", Some(source));
        assert_eq!(shell_source("user.readable.v1").unwrap(), source);
    }

    #[test]
    fn unknown_template_id_fails_closed() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-shell-unknown");
        let error = shell_source("user.absent.v1").unwrap_err().to_string();
        assert!(error.contains("unknown visual template"), "{error}");
    }

    #[test]
    fn bundled_template_has_no_shell_source() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-shell-bundled");
        if !bundled_families_present() {
            return;
        }
        let error = shell_source("analysis.visual.v1").unwrap_err().to_string();
        assert!(error.contains("source kind"), "{error}");
    }

    #[test]
    fn scaffold_without_shell_is_not_readable() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-shell-scaffold");
        // A manifest with no source is skipped by the scan, so the id never
        // enters the index -- the same refusal an unknown id gets.
        write_user_template("user.scaffold.v1", None);
        let error = shell_source("user.scaffold.v1").unwrap_err().to_string();
        assert!(error.contains("unknown visual template"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_shell_fails_closed_before_reading() {
        use std::os::unix::fs::symlink;
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-shell-symlink");
        let outside = tempfile::tempdir().unwrap();
        let planted = outside.path().join("shell.tsx");
        fs::write(&planted, "export default () => null;\n").unwrap();
        let path = write_user_template("user.linked.v1", None);
        symlink(&planted, path.join("shell.tsx")).unwrap();
        let error = shell_source("user.linked.v1").unwrap_err().to_string();
        assert!(error.contains("refuses symlink"), "{error}");
    }

    fn manifest(id: &str) -> String {
        format!(r#"{{"schemaVersion":"synth.visual-template.v1","id":"{id}","version":"1.0.0"}}"#)
    }

    const SHELL: &str = "export default function Shell() { return null; }\n";

    #[test]
    fn save_writes_a_template_the_registry_then_indexes() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-save-template");
        let meta = save(
            "user.saved.v1",
            &manifest("user.saved.v1"),
            SHELL,
            PersistConsent::HumanInPane,
        )
        .unwrap();
        assert_eq!(meta.source_kind.as_deref(), Some(USER_SOURCE_KIND));
        assert_eq!(shell_source("user.saved.v1").unwrap(), SHELL);
        assert!(validate("user.saved.v1").ok);
    }

    #[test]
    fn save_refuses_a_manifest_that_names_another_id() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-save-wrong-id");
        let error = save(
            "user.declared.v1",
            &manifest("user.other.v1"),
            SHELL,
            PersistConsent::HumanInPane,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("declares id"), "{error}");
    }

    /// The registry's refusal is the writer's refusal, and nothing survives it.
    /// A half-written directory would poison `build_template_index` for every
    /// template, not just this one.
    #[test]
    fn a_manifest_the_registry_refuses_leaves_nothing_behind() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-save-rollback");
        let error = save(
            "user.rejected.v1",
            r#"{"schemaVersion":"synth.visual-template.v0","id":"user.rejected.v1","version":"1.0.0"}"#,
            SHELL,
            PersistConsent::HumanInPane,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("unsupported schemaVersion"), "{error}");
        assert!(!template_directory("user.rejected.v1").unwrap().exists());
        // And the registry still builds, which is the point of rolling back.
        assert!(super::super::templates::list_templates(None).is_ok());
    }

    /// Overwriting keeps the previous bytes when the new ones are refused.
    #[test]
    fn a_refused_rewrite_restores_the_previous_source() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-save-restore");
        save(
            "user.kept.v1",
            &manifest("user.kept.v1"),
            SHELL,
            PersistConsent::HumanInPane,
        )
        .unwrap();
        let refused = save(
            "user.kept.v1",
            r#"{"schemaVersion":"synth.visual-template.v1","id":"user.kept.v1","version":"bad"}"#,
            "export default function Broken() { return null; }\n",
            PersistConsent::HumanInPane,
        );
        assert!(refused.is_err());
        assert_eq!(shell_source("user.kept.v1").unwrap(), SHELL);
    }

    #[test]
    fn save_refuses_a_renderer_html_directory() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-save-managed");
        let path = write_user_template("user.managed.v1", None);
        fs::write(path.join("renderer.html"), "<html></html>").unwrap();
        let error = save(
            "user.managed.v1",
            &manifest("user.managed.v1"),
            SHELL,
            PersistConsent::HumanInPane,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("renderer.html"), "{error}");
    }

    #[test]
    fn ids_that_escape_the_root_are_refused_before_any_write() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-save-escape");
        for id in ["", ".", "..", "a/b", "../escape", "/absolute"] {
            let error = save(id, &manifest(id), SHELL, PersistConsent::HumanInPane)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("invalid user visual template id"),
                "{id}: {error}"
            );
        }
    }

    #[test]
    fn create_from_forks_under_a_new_id_and_records_provenance() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-create-template");
        save(
            "user.origin.v1",
            &manifest("user.origin.v1"),
            SHELL,
            PersistConsent::HumanInPane,
        )
        .unwrap();
        let fork = create_from(
            "user.fork.v1",
            "user.origin.v1",
            Some("Forked"),
            PersistConsent::HumanInPane,
        )
        .unwrap();
        assert_eq!(fork.id, "user.fork.v1");
        assert_eq!(fork.title, "Forked");
        assert_eq!(fork.source_kind.as_deref(), Some(USER_SOURCE_KIND));
        // The shell is copied verbatim; the fork starts from working code.
        assert_eq!(shell_source("user.fork.v1").unwrap(), SHELL);
        // The origin is untouched -- fork, never shadow.
        assert_eq!(shell_source("user.origin.v1").unwrap(), SHELL);
        let written: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                template_directory("user.fork.v1")
                    .unwrap()
                    .join(MANIFEST_FILE),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(written["forkedFrom"]["templateId"], "user.origin.v1");
    }

    #[test]
    fn create_from_refuses_an_unknown_source() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-create-unknown");
        let error = create_from(
            "user.fork.v1",
            "user.absent.v1",
            None,
            PersistConsent::HumanInPane,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("unknown visual template"), "{error}");
    }

    #[test]
    fn validate_reports_a_scaffold_as_missing_its_shell() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-validate-scaffold");
        write_user_template("user.scaffolded.v1", None);
        let report = validate("user.scaffolded.v1");
        assert!(!report.ok);
        assert_eq!(report.findings[0].code, "missing_shell");
    }

    #[test]
    fn validate_reports_an_absent_directory() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-validate-absent");
        let report = validate("user.nowhere.v1");
        assert!(!report.ok);
        assert_eq!(report.findings[0].code, "missing_directory");
    }

    /// The registry's own words, not a paraphrase: an author reading this
    /// report and an author reading the pane must be reading the same sentence.
    #[test]
    fn validate_quotes_the_registrys_refusal_verbatim() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-validate-refused");
        let path = write_user_template("user.broken.v1", Some(SHELL));
        fs::write(
            path.join(MANIFEST_FILE),
            r#"{"schemaVersion":"synth.visual-template.v1","id":"user.broken.v1","version":"1"}"#,
        )
        .unwrap();
        let report = validate("user.broken.v1");
        assert_eq!(report.findings[0].code, "refused");
        assert!(
            report.findings[0]
                .message
                .contains("requires a semantic version"),
            "{}",
            report.findings[0].message
        );
    }

    /// Silence would read as approval, so the report says where the import
    /// allowlist actually runs even when it has nothing else to say.
    #[test]
    fn validate_names_the_pane_as_the_source_scanner() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-validate-scan-owner");
        save(
            "user.scanned.v1",
            &manifest("user.scanned.v1"),
            "import { evil } from \"node:fs\";\nexport default function S() { return null; }\n",
            PersistConsent::HumanInPane,
        )
        .unwrap();
        let report = validate("user.scanned.v1");
        // Structurally fine. The forbidden import is the pane's call, not ours.
        assert!(report.ok);
        assert!(
            report.source_scan.contains("sourcedValidate.ts"),
            "{}",
            report.source_scan
        );
    }

    #[test]
    fn the_fingerprint_moves_when_a_shell_is_edited() {
        let _isolated = crate::instance::IsolatedDataRoot::new("visual-watch-fingerprint");
        let empty = root_fingerprint();
        save(
            "user.watched.v1",
            &manifest("user.watched.v1"),
            SHELL,
            PersistConsent::HumanInPane,
        )
        .unwrap();
        let saved = root_fingerprint();
        assert_ne!(empty, saved);
        assert_eq!(saved, root_fingerprint());
        fs::write(
            template_directory("user.watched.v1")
                .unwrap()
                .join(SHELL_FILE),
            "export default function Edited() { return null; }\n",
        )
        .unwrap();
        assert_ne!(saved, root_fingerprint());
    }
}
