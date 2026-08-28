//! Workspace documents: the domain the right panel's document pane answers for.
//!
//! The renderer has no filesystem. Every byte it displays crosses a typed
//! command whose path was first resolved against the conversation's
//! [`crate::workspace_scope`] session roots, which is why this module exists
//! rather than `tauri-plugin-fs`: a plugin grant is a static allowlist decided
//! at package time, and the thing that must decide here is *this conversation's*
//! workspace plus the folders a human attached to it.
//!
//! Two vocabularies, deliberately kept apart:
//!
//! * **Refusal** — the trust boundary. A path outside every session root, or a
//!   conversation with no workspace at all, is a [`StructuredFailure`] with a
//!   stable code. It is not a `DocumentRecord` with a sad face on it, because
//!   describing a file we are not allowed to look at is itself a disclosure.
//! * **Unavailability** — the catalog law. A path inside scope that still
//!   cannot be typeset (missing, a directory, binary, unreadable) becomes a
//!   [`crate::presentation::Presentability::Unavailable`] with a named reason.
//!   The pane and the directory listing both say the reason out loud rather
//!   than omitting the row, exactly as the trace catalog does.
//!
//! [`StructuredFailure`]: crate::error::StructuredFailure

pub mod commands;
pub mod ipc;

use anyhow::{anyhow, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

use crate::error::StructuredFailure;
use crate::presentation::{Pane, Presentability, UnavailableReason};
use crate::storage::Database;
use crate::workspace_scope;

/// Wire schema for one read document. Also the projection schema the document
/// pane's binding declares, so the pane and the command agree by name.
pub const DOCUMENT_SCHEMA: &str = "synth.workspace-document.v1";
/// Wire schema for one directory listing — the breadcrumb's data source.
pub const DIRECTORY_SCHEMA: &str = "synth.workspace-directory.v1";

/// Largest text payload one read returns. Past this the read is *truncated and
/// says so*, which is different from refusing: a 40 MB log still has a
/// legible first page, and the pane states which page it is showing.
pub const DOCUMENT_MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Bytes sniffed to decide whether a file is text at all.
const SNIFF_BYTES: usize = 8 * 1024;
/// Directory rows one listing returns. A truncated listing says so.
pub const DIRECTORY_MAX_ENTRIES: usize = 1_000;

// ---------------------------------------------------------------------------
// Refusals — the scope boundary
// ---------------------------------------------------------------------------

pub const CODE_NO_PATH: &str = "document_path_missing";
pub const CODE_SCOPE_UNBOUND: &str = "document_scope_unbound";
pub const CODE_OUTSIDE_WORKSPACE: &str = "document_outside_workspace";
pub const CODE_UNAVAILABLE: &str = "document_unavailable";

fn scope_unbound(session_id: &str) -> anyhow::Error {
    anyhow!(StructuredFailure::new(
        CODE_SCOPE_UNBOUND,
        format!("conversation `{session_id}` has no workspace"),
        "Open this conversation on a folder, or attach one with Add folder, then try again.",
    )
    .with_details(serde_json::json!({ "sessionId": session_id })))
}

fn outside_workspace(requested: &str, roots: &[PathBuf]) -> anyhow::Error {
    anyhow!(StructuredFailure::new(
        CODE_OUTSIDE_WORKSPACE,
        format!("`{requested}` is outside this conversation's workspace"),
        "Attach the folder to this conversation with Add folder, then try again.",
    )
    .with_details(serde_json::json!({
        "requested": requested,
        "roots": roots.iter().map(|root| root.to_string_lossy()).collect::<Vec<_>>(),
    })))
}

/// An in-scope path that still cannot be presented, raised where a caller
/// expects bytes back. The reason is the same value the catalog shows.
fn unavailable(document: &DocumentRecord, reason: UnavailableReason) -> anyhow::Error {
    anyhow!(StructuredFailure::new(
        CODE_UNAVAILABLE,
        format!("{} cannot be shown: {}", document.name, reason.label()),
        reason.remediation(),
    )
    .with_details(serde_json::json!({
        "path": document.path,
        "reason": reason.label(),
        "byteSize": document.byte_size,
    })))
}

// ---------------------------------------------------------------------------
// The record
// ---------------------------------------------------------------------------

/// What kind of surface a path is, as far as the pane is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    /// Typeset by default, with a View source toggle.
    Markdown,
    /// Syntax highlighted, with a language badge.
    Code,
    /// Monospaced, unhighlighted.
    PlainText,
    /// A folder. Presentable as a listing, never as a document.
    Directory,
}

/// One located path inside the conversation's workspace.
///
/// Locating is what costs a scope check; every question after that — is it
/// eligible, what identity does its pane have, what language badge does it
/// carry — is a pure function of this record. That is what lets the panel host
/// answer for documents the same way it answers for traces.
#[derive(Clone, Debug)]
pub struct DocumentRecord {
    /// Canonical absolute path. Symlinks are already resolved, which is what
    /// makes the scope check meaningful.
    pub path: String,
    /// The session root this path resolved under.
    pub root: String,
    /// `path` relative to `root` — the breadcrumb trail, computed once here so
    /// the renderer does not re-derive a second path helper.
    pub relative_path: String,
    pub name: String,
    pub kind: DocumentKind,
    /// Language id for the badge and the highlighter, e.g. `rust`, `markdown`.
    pub language: String,
    pub byte_size: u64,
    pub exists: bool,
    /// `false` when the first [`SNIFF_BYTES`] are not valid UTF-8 or contain a
    /// NUL. Sniffed at locate time so eligibility stays pure.
    pub is_text: bool,
    /// Set when metadata could be read but the bytes could not.
    pub read_error: Option<String>,
    pub modified_at: Option<String>,
}

impl DocumentRecord {
    /// Whether this path can be typeset in the pane, and when it cannot, why.
    pub fn presentability(&self) -> Presentability {
        Pane::Document(self).presentable()
    }

    /// Deterministic pane identity for this path.
    pub fn viewer_visual_id(&self) -> String {
        Pane::Document(self).visual_id()
    }

    /// Breadcrumb segments, root first. Each carries the absolute path the
    /// "list this directory" command takes, so a segment is clickable without
    /// the renderer rebuilding a path.
    pub fn breadcrumbs(&self) -> Vec<Breadcrumb> {
        let root = PathBuf::from(&self.root);
        let root_label = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.clone());
        let mut trail = vec![Breadcrumb {
            label: root_label,
            path: self.root.clone(),
            is_directory: true,
        }];
        let mut walked = root;
        let segments: Vec<&str> = self
            .relative_path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        let last = segments.len().saturating_sub(1);
        for (index, segment) in segments.iter().enumerate() {
            walked.push(segment);
            trail.push(Breadcrumb {
                label: (*segment).to_owned(),
                path: walked.to_string_lossy().into_owned(),
                is_directory: index < last || self.kind == DocumentKind::Directory,
            });
        }
        trail
    }
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Breadcrumb {
    pub label: String,
    pub path: String,
    pub is_directory: bool,
}

// ---------------------------------------------------------------------------
// Wire payloads
// ---------------------------------------------------------------------------

/// One read document, as the pane receives it.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDocument {
    pub schema_version: String,
    pub path: String,
    pub root: String,
    pub relative_path: String,
    pub name: String,
    pub kind: DocumentKind,
    pub language: String,
    pub text: String,
    /// Size of the file on disk, not of `text`.
    #[specta(type = specta_typescript::Number)]
    pub byte_size: u64,
    /// True when `text` is a prefix. The pane says which prefix rather than
    /// pretending the file ended.
    pub truncated: bool,
    /// sha256 of the returned bytes. When `truncated`, it names the prefix that
    /// was rendered — not the file — which is the only claim it can honestly make.
    pub content_digest: String,
    pub modified_at: Option<String>,
    pub breadcrumbs: Vec<Breadcrumb>,
}

/// One directory row. Rows that cannot be opened are listed with the reason.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    pub kind: DocumentKind,
    pub language: String,
    #[specta(type = specta_typescript::Number)]
    pub byte_size: u64,
    pub modified_at: Option<String>,
    /// Whether opening this row lands on a document.
    pub openable: bool,
    /// Why it does not, when it does not. Never `None` while `openable` is
    /// false: a row the user cannot act on still owes them a sentence.
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDirectory {
    pub schema_version: String,
    pub path: String,
    pub root: String,
    pub relative_path: String,
    pub entries: Vec<DirectoryEntry>,
    pub truncated: bool,
    pub breadcrumbs: Vec<Breadcrumb>,
}

// ---------------------------------------------------------------------------
// Scope resolution
// ---------------------------------------------------------------------------

/// Lexical normalization, with no filesystem access.
///
/// Done before touching the disk so a `..` cannot be smuggled past the scope
/// check by a path that does not exist yet, and so the check does not depend on
/// whether the caller's shell already collapsed the segments.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve one caller-supplied path against the conversation's session roots.
///
/// Canonicalizing the deepest *existing* ancestor is what makes this safe for
/// paths that do not exist: a symlinked ancestor pointing out of the workspace
/// is resolved and then rejected, and only after the scope check does the
/// caller learn that the leaf is missing. Refusing first and reporting absence
/// second is deliberate — "no such file" outside the workspace is already a
/// disclosure about a filesystem the conversation cannot see.
fn resolve_in_scope(roots: &[PathBuf], requested: &str) -> Result<(PathBuf, PathBuf)> {
    let raw = Path::new(requested.trim());
    if raw.as_os_str().is_empty() {
        // Distinct from "outside the workspace": nothing was asked for, and
        // saying it was refused would misname the problem.
        return Err(anyhow!(StructuredFailure::new(
            CODE_NO_PATH,
            "no document path was given",
            "Name a file inside this conversation's workspace.",
        )));
    }
    let absolute = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        // A relative path is relative to the conversation workspace, which is
        // the first root by construction in `workspace_scope`.
        roots
            .first()
            .ok_or_else(|| outside_workspace(requested, roots))?
            .join(raw)
    };
    let normalized = normalize(&absolute);

    let mut existing = normalized.clone();
    let mut tail: Vec<OsString> = Vec::new();
    while !existing.exists() {
        let Some(name) = existing.file_name().map(|name| name.to_os_string()) else {
            return Err(outside_workspace(requested, roots));
        };
        tail.push(name);
        let Some(parent) = existing.parent().map(Path::to_path_buf) else {
            return Err(outside_workspace(requested, roots));
        };
        existing = parent;
    }
    let mut resolved = existing
        .canonicalize()
        .map_err(|_| outside_workspace(requested, roots))?;
    for name in tail.iter().rev() {
        resolved.push(name);
    }

    let root = roots
        .iter()
        .find(|root| resolved == **root || resolved.starts_with(root))
        .cloned()
        .ok_or_else(|| outside_workspace(requested, roots))?;
    Ok((resolved, root))
}

fn session_roots(db: &Database, session_id: &str) -> Result<Vec<PathBuf>> {
    let roots = workspace_scope::approved_search_roots(db, session_id)
        .map_err(|_| scope_unbound(session_id))?;
    if roots.is_empty() {
        return Err(scope_unbound(session_id));
    }
    Ok(roots)
}

// ---------------------------------------------------------------------------
// Locating
// ---------------------------------------------------------------------------

fn modified_at(metadata: &std::fs::Metadata) -> Option<String> {
    let modified = metadata.modified().ok()?;
    Some(chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339())
}

/// Sniff whether a file is text. A NUL byte or invalid UTF-8 in the first
/// [`SNIFF_BYTES`] means the pane would render mojibake, so the record says
/// binary and the pane offers Open externally instead.
fn sniff_text(path: &Path) -> (bool, Option<String>) {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) => return (false, Some(error.to_string())),
    };
    let mut buffer = vec![0_u8; SNIFF_BYTES];
    let read = match file.read(&mut buffer) {
        Ok(read) => read,
        Err(error) => return (false, Some(error.to_string())),
    };
    buffer.truncate(read);
    if buffer.contains(&0) {
        return (false, None);
    }
    match std::str::from_utf8(&buffer) {
        Ok(_) => (true, None),
        // A multi-byte character straddling the sniff boundary is not binary.
        Err(error) if error.error_len().is_none() && error.valid_up_to() + 4 >= buffer.len() => {
            (true, None)
        }
        Err(_) => (false, None),
    }
}

/// Locate one path inside the conversation's workspace.
///
/// Returns a record for anything in scope — including a path that does not
/// exist — because absence is a state the pane must name, and it can only name
/// it for a file it is allowed to talk about.
pub fn locate(db: &Database, session_id: &str, requested: &str) -> Result<DocumentRecord> {
    let roots = session_roots(db, session_id)?;
    let (path, root) = resolve_in_scope(&roots, requested)?;
    Ok(record_for(&path, &root))
}

fn record_for(path: &Path, root: &Path) -> DocumentRecord {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let relative_path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let metadata = std::fs::metadata(path).ok();
    let exists = metadata.is_some();
    let is_directory = metadata.as_ref().is_some_and(std::fs::Metadata::is_dir);
    let byte_size = metadata.as_ref().map(std::fs::Metadata::len).unwrap_or(0);
    let (is_text, read_error) = if !exists || is_directory {
        (false, None)
    } else {
        sniff_text(path)
    };
    let language = language_for(path, is_directory);
    DocumentRecord {
        path: path.to_string_lossy().into_owned(),
        root: root.to_string_lossy().into_owned(),
        relative_path,
        name,
        kind: kind_for(&language, is_directory),
        language,
        byte_size,
        exists,
        is_text,
        read_error,
        modified_at: metadata.as_ref().and_then(modified_at),
    }
}

// ---------------------------------------------------------------------------
// Language
// ---------------------------------------------------------------------------

/// Extension → language id. The id is the badge text, the highlighter's key,
/// and the fenced-code language name, so there is one spelling of "rust" in
/// the product rather than three.
const LANGUAGES: &[(&str, &str)] = &[
    ("md", "markdown"),
    ("markdown", "markdown"),
    ("mdx", "markdown"),
    ("rs", "rust"),
    ("ts", "typescript"),
    ("tsx", "tsx"),
    ("js", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("jsx", "jsx"),
    ("py", "python"),
    ("toml", "toml"),
    ("json", "json"),
    ("jsonc", "json"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("sh", "shell"),
    ("bash", "shell"),
    ("zsh", "shell"),
    ("fish", "shell"),
    ("sql", "sql"),
    ("css", "css"),
    ("html", "html"),
    ("svg", "html"),
    ("go", "go"),
    ("c", "c"),
    ("h", "c"),
    ("cc", "cpp"),
    ("cpp", "cpp"),
    ("hpp", "cpp"),
    ("java", "java"),
    ("rb", "ruby"),
    ("swift", "swift"),
    ("kt", "kotlin"),
    ("tf", "hcl"),
    ("ini", "ini"),
    ("cfg", "ini"),
    ("csv", "csv"),
    ("diff", "diff"),
    ("patch", "diff"),
    ("txt", "text"),
    ("log", "text"),
];

/// Filenames with no extension that still name a language.
const NAMED_FILES: &[(&str, &str)] = &[
    ("dockerfile", "docker"),
    ("makefile", "make"),
    ("cargo.lock", "toml"),
    ("license", "text"),
    ("notice", "text"),
];

pub fn language_for(path: &Path, is_directory: bool) -> String {
    if is_directory {
        return "directory".to_owned();
    }
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if let Some((_, language)) = NAMED_FILES.iter().find(|(candidate, _)| *candidate == name) {
        return (*language).to_owned();
    }
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    LANGUAGES
        .iter()
        .find(|(candidate, _)| *candidate == extension)
        .map(|(_, language)| (*language).to_owned())
        .unwrap_or_else(|| "text".to_owned())
}

fn kind_for(language: &str, is_directory: bool) -> DocumentKind {
    if is_directory {
        return DocumentKind::Directory;
    }
    match language {
        "markdown" => DocumentKind::Markdown,
        "text" | "csv" => DocumentKind::PlainText,
        _ => DocumentKind::Code,
    }
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Read one in-scope document for display.
///
/// Truncation is disclosed rather than hidden; unavailability is named rather
/// than returned as an empty string.
pub fn read(db: &Database, session_id: &str, requested: &str) -> Result<WorkspaceDocument> {
    let document = locate(db, session_id, requested)?;
    match document.presentability() {
        Presentability::Present => {}
        Presentability::Unavailable(reason) => return Err(unavailable(&document, reason)),
    }

    let path = PathBuf::from(&document.path);
    let unreadable = |error: std::io::Error| {
        anyhow!(StructuredFailure::new(
            CODE_UNAVAILABLE,
            format!("{} could not be read: {error}", document.name),
            "Check the file's permissions, then try again.",
        )
        .retryable(true))
    };
    // Bounded read, not read-then-truncate: a multi-gigabyte log must not be
    // resident in memory just to render its first page.
    let mut bytes = Vec::new();
    {
        use std::io::Read;
        let file = std::fs::File::open(&path).map_err(&unreadable)?;
        file.take(DOCUMENT_MAX_BYTES)
            .read_to_end(&mut bytes)
            .map_err(&unreadable)?;
    }

    let truncated = document.byte_size > DOCUMENT_MAX_BYTES;
    let slice = if truncated {
        // Never split a character in half: cut back to the last boundary.
        let end = match std::str::from_utf8(&bytes) {
            Ok(_) => bytes.len(),
            Err(error) => error.valid_up_to(),
        };
        &bytes[..end]
    } else {
        &bytes[..]
    };
    let text = String::from_utf8_lossy(slice).into_owned();
    let content_digest = format!("sha256:{:x}", Sha256::digest(slice));

    Ok(WorkspaceDocument {
        schema_version: DOCUMENT_SCHEMA.to_owned(),
        path: document.path.clone(),
        root: document.root.clone(),
        relative_path: document.relative_path.clone(),
        name: document.name.clone(),
        kind: document.kind,
        language: document.language.clone(),
        text,
        byte_size: document.byte_size,
        truncated,
        content_digest,
        modified_at: document.modified_at.clone(),
        breadcrumbs: document.breadcrumbs(),
    })
}

/// List one in-scope directory.
///
/// Every child is a row. A child that cannot be opened keeps its row and
/// carries the reason, because a directory that silently hides its binaries
/// tells the reader the folder is empty when it is not.
pub fn list_dir(db: &Database, session_id: &str, requested: &str) -> Result<WorkspaceDirectory> {
    let directory = locate(db, session_id, requested)?;
    if !directory.exists {
        return Err(unavailable(&directory, UnavailableReason::Missing));
    }
    if directory.kind != DocumentKind::Directory {
        return Err(unavailable(&directory, UnavailableReason::NotADirectory));
    }

    let root = PathBuf::from(&directory.root);
    let mut rows: Vec<DirectoryEntry> = Vec::new();
    let mut truncated = false;
    let reader = std::fs::read_dir(&directory.path).map_err(|error| {
        anyhow!(StructuredFailure::new(
            CODE_UNAVAILABLE,
            format!("{} could not be listed: {error}", directory.name),
            "Check the folder's permissions, then try again.",
        )
        .retryable(true))
    })?;
    for entry in reader.flatten() {
        if rows.len() >= DIRECTORY_MAX_ENTRIES {
            truncated = true;
            break;
        }
        let child = record_for(&entry.path(), &root);
        let presentability = child.presentability();
        let openable = presentability.eligible() || child.kind == DocumentKind::Directory;
        rows.push(DirectoryEntry {
            name: child.name.clone(),
            path: child.path.clone(),
            kind: child.kind,
            language: child.language.clone(),
            byte_size: child.byte_size,
            modified_at: child.modified_at.clone(),
            openable,
            reason: if openable {
                None
            } else {
                Some(presentability.label().to_owned())
            },
        });
    }
    // Folders first, then files, each alphabetically — the order a reader
    // scanning for a filename expects, and stable across calls.
    rows.sort_by(|left, right| {
        let left_dir = left.kind == DocumentKind::Directory;
        let right_dir = right.kind == DocumentKind::Directory;
        right_dir
            .cmp(&left_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    Ok(WorkspaceDirectory {
        schema_version: DIRECTORY_SCHEMA.to_owned(),
        path: directory.path.clone(),
        root: directory.root.clone(),
        relative_path: directory.relative_path.clone(),
        entries: rows,
        truncated,
        breadcrumbs: directory.breadcrumbs(),
    })
}

// ---------------------------------------------------------------------------
// Showing
// ---------------------------------------------------------------------------

/// What the pane receives when a document is opened: the durable pane record
/// and the first read, in one round trip.
///
/// Two calls would let the pane render a viewer whose document then refuses,
/// and the reader would watch an empty pane appear before the reason arrived.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentShown {
    pub visual: crate::visuals::VisualRecord,
    pub document: WorkspaceDocument,
}

/// Open one workspace document in the right panel.
///
/// One implementation, two callers: the Tauri command the renderer's Open
/// affordance uses, and the agent-facing `document_show` IPC route. A second
/// copy on the agent path is exactly what `presentation` was extracted to
/// prevent.
pub async fn show(
    core: &crate::core_runtime::CoreRuntime,
    session_id: &str,
    requested: &str,
) -> Result<DocumentShown> {
    let visual = crate::presentation::ensure_document_viewer(core, session_id, requested).await?;
    let path = crate::presentation::document_path_binding(&visual)
        .ok_or_else(|| anyhow!("document viewer `{}` declares no path", visual.id))?;
    let document = read(core.storage().database(), session_id, &path)?;
    let (shown, event) = core
        .visuals()
        .show(visual.id.clone(), Some(session_id.to_owned()))
        .await?;
    core.broadcast_committed(Some(serde_json::from_value(event)?));
    Ok(DocumentShown {
        visual: shown,
        document,
    })
}

