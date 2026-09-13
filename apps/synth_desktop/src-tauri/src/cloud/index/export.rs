//! Explicit selected-file collection for a Contribution export. Pure and local:
//! no network, no workspace crawl. Every file is named by the user, must be a
//! regular non-symlink file inside the chosen root, and must not look like a
//! credential. Bytes stay in Rust; the renderer only sees the manifest preview.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fmt,
    path::{Component, Path, PathBuf},
};

pub const MAX_FILES: usize = 1024;
/// Matches the SDK's bounded in-memory transfer; larger data needs streaming.
pub const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
pub const ASSET_ROLES: &[&str] = &[
    "summary",
    "report",
    "code",
    "data",
    "model",
    "environment",
    "evidence",
    "reproduce",
];

#[derive(Debug, PartialEq, Eq)]
pub enum ExportError {
    Invalid(String),
    Io(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(f, "invalid Contribution export: {message}"),
            Self::Io(message) => write!(f, "Contribution export file error: {message}"),
        }
    }
}

impl std::error::Error for ExportError {}

fn invalid(message: impl Into<String>) -> ExportError {
    ExportError::Invalid(message.into())
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexSelectedFile {
    /// Path relative to the export root.
    pub path: String,
    pub role: String,
    /// Declared logical path; defaults to the normalized relative path.
    pub logical_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SelectedAsset {
    pub asset_id: String,
    pub role: String,
    pub logical_path: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub digest_sha256: String,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Same normalization rules as the backend Artifact logical path validator.
pub fn validate_logical_path(value: &str) -> Result<(), ExportError> {
    if value.is_empty()
        || value.len() > 1024
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains('\\')
        || value.contains('\0')
        || value == "contribution.json"
        || value.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(invalid(format!("logical path {value:?} is not normalized")));
    }
    Ok(())
}

fn asset_id_for(logical_path: &str, taken: &mut BTreeSet<String>) -> String {
    let mut base: String = logical_path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_.-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !base.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        base.insert(0, 'a');
    }
    base.truncate(120);
    let mut candidate = base.clone();
    let mut counter = 2;
    while !taken.insert(candidate.clone()) {
        candidate = format!("{base}_{counter}");
        counter += 1;
    }
    candidate
}

fn media_type_for(logical_path: &str) -> &'static str {
    match logical_path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "md" | "markdown" => "text/markdown",
        "txt" | "log" => "text/plain",
        "json" | "jsonl" => "application/json",
        "csv" => "text/csv",
        "py" => "text/x-python",
        "rs" => "text/x-rust",
        "toml" | "yaml" | "yml" => "text/plain",
        "png" => "image/png",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn run_after(haystack: &[u8], prefix: &[u8], accept: impl Fn(u8) -> bool, minimum: usize) -> bool {
    haystack
        .windows(prefix.len())
        .enumerate()
        .any(|(index, window)| {
            window == prefix
                && haystack[index + prefix.len()..]
                    .iter()
                    .take_while(|byte| accept(**byte))
                    .count()
                    >= minimum
        })
}

/// Conservative credential detector mirroring the SDK/MCP upload helper.
pub fn looks_like_credential(bytes: &[u8]) -> bool {
    let token = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-';
    let contains = |needle: &[u8]| bytes.windows(needle.len()).any(|window| window == needle);
    (contains(b"-----BEGIN") && contains(b"PRIVATE KEY"))
        || run_after(bytes, b"sk-", token, 20)
        || run_after(
            bytes,
            b"AKIA",
            |b| b.is_ascii_uppercase() || b.is_ascii_digit(),
            16,
        )
        || run_after(bytes, b"ghp_", |b| b.is_ascii_alphanumeric(), 30)
        || [b"xoxb-", b"xoxp-", b"xoxa-"]
            .iter()
            .any(|prefix| run_after(bytes, *prefix, token, 10))
}

fn relative_logical_path(relative: &Path) -> Result<String, ExportError> {
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(
                part.to_str()
                    .ok_or_else(|| invalid("file names must be UTF-8"))?
                    .to_owned(),
            ),
            _ => {
                return Err(invalid(format!(
                    "{} is not a plain relative path",
                    relative.display()
                )))
            }
        }
    }
    Ok(parts.join("/"))
}

/// Read exactly the selected files and describe them as Contribution assets.
pub fn collect_selected_files(
    root: &Path,
    files: &[IndexSelectedFile],
) -> Result<Vec<SelectedAsset>, ExportError> {
    if files.is_empty() || files.len() > MAX_FILES {
        return Err(invalid(format!("select between 1 and {MAX_FILES} files")));
    }
    let root: PathBuf = root
        .canonicalize()
        .map_err(|error| ExportError::Io(format!("export root: {error}")))?;
    if !root.is_dir() {
        return Err(invalid("export root must be a directory"));
    }
    let mut total = 0u64;
    let mut taken_ids = BTreeSet::new();
    let mut taken_paths = BTreeSet::new();
    let mut assets = Vec::with_capacity(files.len());
    for file in files {
        if !ASSET_ROLES.contains(&file.role.as_str()) {
            return Err(invalid(format!("unsupported asset role {:?}", file.role)));
        }
        let relative = Path::new(&file.path);
        let default_logical = relative_logical_path(relative)?;
        let candidate = root.join(relative);
        let metadata = std::fs::symlink_metadata(&candidate)
            .map_err(|error| ExportError::Io(format!("{}: {error}", file.path)))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid(format!(
                "{} must be a regular file, not a symlink",
                file.path
            )));
        }
        let resolved = candidate
            .canonicalize()
            .map_err(|error| ExportError::Io(format!("{}: {error}", file.path)))?;
        if !resolved.starts_with(&root) {
            return Err(invalid(format!("{} escapes the export root", file.path)));
        }
        total += metadata.len();
        if total > MAX_TOTAL_BYTES {
            return Err(invalid("selected files exceed the 64 MiB export bound"));
        }
        let bytes = std::fs::read(&resolved)
            .map_err(|error| ExportError::Io(format!("{}: {error}", file.path)))?;
        if looks_like_credential(&bytes) {
            return Err(invalid(format!(
                "{} looks like it contains a credential; remove it before export",
                file.path
            )));
        }
        let logical_path = file.logical_path.clone().unwrap_or(default_logical);
        validate_logical_path(&logical_path)?;
        if !taken_paths.insert(logical_path.clone()) {
            return Err(invalid(format!("duplicate logical path {logical_path:?}")));
        }
        assets.push(SelectedAsset {
            asset_id: asset_id_for(&logical_path, &mut taken_ids),
            role: file.role.clone(),
            media_type: media_type_for(&logical_path).to_owned(),
            size_bytes: bytes.len() as u64,
            digest_sha256: sha256_hex(&bytes),
            logical_path,
            bytes,
        });
    }
    Ok(assets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected(path: &str, role: &str) -> IndexSelectedFile {
        IndexSelectedFile {
            path: path.into(),
            role: role.into(),
            logical_path: None,
        }
    }

    #[test]
    fn collects_only_listed_files_with_digests() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("report.md"), b"# Findings\n").unwrap();
        std::fs::write(root.path().join("unlisted.txt"), b"not exported").unwrap();
        let assets =
            collect_selected_files(root.path(), &[selected("report.md", "report")]).unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].logical_path, "report.md");
        assert_eq!(assets[0].media_type, "text/markdown");
        assert_eq!(assets[0].digest_sha256, sha256_hex(b"# Findings\n"));
    }

    #[test]
    fn rejects_escapes_symlinks_and_credentials() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("work");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(outer.path().join("secret.txt"), b"x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outer.path().join("secret.txt"), root.join("link.txt")).unwrap();
        std::fs::write(
            root.join("leak.env"),
            [b"KEY=sk-".as_slice(), &[b'a'; 30]].concat(),
        )
        .unwrap();
        for path in ["../secret.txt", "link.txt", "leak.env"] {
            assert!(
                collect_selected_files(&root, &[selected(path, "code")]).is_err(),
                "{path}"
            );
        }
        assert!(collect_selected_files(&root, &[selected("leak.env", "wiki")]).is_err());
    }

    #[test]
    fn logical_paths_follow_backend_rules() {
        for bad in ["/a", "a/", "a/../b", "a//b", "contribution.json", "a\\b"] {
            assert!(validate_logical_path(bad).is_err(), "{bad}");
        }
        assert!(validate_logical_path("reports/final.md").is_ok());
    }
}
