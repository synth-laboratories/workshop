//! Host-mediated `.env` import. The trusted host reads the file; agent-visible
//! results contain names, classifications, and masked suffixes — never values.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::backend::SecretBytes;
use super::fingerprint::{display_suffix, mask_suffix};
use super::providers::{self, classification_label, classify_variable, default_alias};

const MAX_ENV_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MaskedImportCandidate {
    pub variable: String,
    pub provider: Option<String>,
    pub masked: String,
    pub classification: String,
    pub selected: bool,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub request_id: String,
    pub status: String,
    pub source_path: String,
    pub candidates: Vec<MaskedImportCandidate>,
    pub source_remains_readable: bool,
    pub warning: Option<String>,
    /// Masked line-oriented preview of replace/remove. Never contains values.
    pub cleanup_diff: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AfterImportAction {
    Keep,
    ReplaceAliases,
    RemoveEntries,
}

#[derive(Clone)]
pub struct PendingImport {
    pub request_id: String,
    pub source_path: PathBuf,
    pub destination_scope: String,
    pub entries: Vec<PendingEntry>,
}

#[derive(Clone)]
pub struct PendingEntry {
    pub variable: String,
    pub provider: String,
    pub alias: String,
    pub value: SecretBytes,
    pub suffix: String,
}

impl std::fmt::Debug for PendingEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingEntry")
            .field("variable", &self.variable)
            .field("provider", &self.provider)
            .field("alias", &self.alias)
            .field("value", &"<redacted>")
            .field("suffix", &self.suffix)
            .finish()
    }
}

pub fn preview(
    source_path: &str,
    variable_names: &[String],
    destination_scope: &str,
    allowed_roots: &[PathBuf],
) -> Result<(ImportPreview, PendingImport)> {
    let path = canonicalize_import_path(Path::new(source_path), allowed_roots)?;
    let text = read_bounded(&path)?;
    let parsed = parse_dotenv(&text)?;
    let mut entries = Vec::new();
    let mut candidates = Vec::new();
    let wanted: Vec<String> = if variable_names.is_empty() {
        parsed.keys().cloned().collect()
    } else {
        variable_names.to_vec()
    };
    for name in wanted {
        let Some(value) = parsed.get(&name) else {
            continue;
        };
        let provider = classify_variable(&name).unwrap_or("unknown").to_string();
        let bytes = SecretBytes::from_utf8(value);
        let suffix = display_suffix(&bytes);
        candidates.push(MaskedImportCandidate {
            variable: name.clone(),
            provider: Some(provider.clone()),
            masked: mask_suffix(&suffix),
            classification: classification_label(Some(&provider)).into(),
            selected: providers::classify_variable(&name).is_some()
                && providers::classify_variable(&name) != Some("database"),
        });
        entries.push(PendingEntry {
            variable: name.clone(),
            provider: provider.clone(),
            alias: default_alias(&provider),
            value: bytes,
            suffix,
        });
    }
    let request_id = format!("imp_{}", Uuid::new_v4().simple());
    let readable = path.is_file();
    let warning = if readable {
        Some("The original file still contains plaintext values. Agents with workspace read access can still see them until you replace or remove those entries.".into())
    } else {
        None
    };
    let cleanup_diff = Some(masked_cleanup_diff(&candidates));
    Ok((
        ImportPreview {
            request_id: request_id.clone(),
            status: "approval_required".into(),
            source_path: path.display().to_string(),
            candidates,
            source_remains_readable: readable,
            warning,
            cleanup_diff,
        },
        PendingImport {
            request_id,
            source_path: path,
            destination_scope: destination_scope.to_owned(),
            entries,
        },
    ))
}

pub fn canonicalize_import_path(path: &Path, allowed_roots: &[PathBuf]) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("import path is empty");
    }
    let canonical =
        fs::canonicalize(path).with_context(|| format!("canonicalize {}", path.display()))?;
    if !canonical.is_file() {
        bail!("{} is not a regular file", canonical.display());
    }
    if canonical.symlink_metadata()?.file_type().is_symlink() {
        // canonicalize already resolved the final target; refuse if the original
        // path itself was a symlink that escaped the allowed roots.
    }
    if allowed_roots.is_empty() {
        return Ok(canonical);
    }
    let allowed = allowed_roots.iter().any(|root| {
        fs::canonicalize(root)
            .ok()
            .is_some_and(|root| canonical.starts_with(root))
    });
    if !allowed {
        bail!(
            "refusing to import {}: path is outside the allowed workspace roots",
            canonical.display()
        );
    }
    Ok(canonical)
}

fn read_bounded(path: &Path) -> Result<String> {
    let file = fs::File::open(path)?;
    let meta = file.metadata()?;
    if meta.len() > MAX_ENV_BYTES {
        bail!(".env file is larger than {MAX_ENV_BYTES} bytes");
    }
    let mut buf = String::new();
    file.take(MAX_ENV_BYTES + 1)
        .read_to_string(&mut buf)
        .with_context(|| format!("read {}", path.display()))?;
    if buf.len() as u64 > MAX_ENV_BYTES {
        bail!(".env file is larger than {MAX_ENV_BYTES} bytes");
    }
    Ok(buf)
}

pub fn parse_dotenv(text: &str) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    let mut pending_key: Option<String> = None;
    let mut pending_value = String::new();
    let mut quote: Option<char> = None;
    for raw in text.lines() {
        if let Some(key) = pending_key.clone() {
            pending_value.push('\n');
            pending_value.push_str(raw);
            if let Some(mark) = quote {
                if raw.ends_with(mark) && !raw.ends_with(&format!("\\{mark}")) {
                    pending_value.pop();
                    out.insert(key, unescape(&pending_value, mark));
                    pending_key = None;
                    pending_value.clear();
                    quote = None;
                }
            }
            continue;
        }
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, rest)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let rest = rest.trim();
        if (rest.starts_with('"') && !rest.ends_with('"'))
            || (rest.starts_with('\'') && !rest.ends_with('\''))
        {
            quote = rest.chars().next();
            pending_key = Some(key.into());
            pending_value = rest[1..].to_owned();
            continue;
        }
        let value = if rest.len() >= 2
            && ((rest.starts_with('"') && rest.ends_with('"'))
                || (rest.starts_with('\'') && rest.ends_with('\'')))
        {
            unescape(&rest[1..rest.len() - 1], rest.chars().next().unwrap())
        } else {
            rest.split('#').next().unwrap_or(rest).trim().to_owned()
        };
        out.insert(key.to_owned(), value);
    }
    if pending_key.is_some() {
        bail!("unterminated quoted value in .env");
    }
    Ok(out)
}

pub fn masked_cleanup_diff(candidates: &[MaskedImportCandidate]) -> String {
    let mut lines = vec![
        "Replace with aliases:".to_owned(),
        "  each selected variable becomes a non-secret Workshop alias".to_owned(),
        "Remove entries:".to_owned(),
        "  each selected variable is deleted from the file".to_owned(),
        String::new(),
        "Selected:".to_owned(),
    ];
    for candidate in candidates.iter().filter(|candidate| candidate.selected) {
        lines.push(format!(
            "  {}  {}  →  alias or removed",
            candidate.variable, candidate.masked
        ));
    }
    lines.join("\n")
}

pub fn masked_cleanup_diff_from_entries(entries: &[PendingEntry]) -> String {
    let candidates: Vec<MaskedImportCandidate> = entries
        .iter()
        .map(|entry| MaskedImportCandidate {
            variable: entry.variable.clone(),
            provider: Some(entry.provider.clone()),
            masked: mask_suffix(&entry.suffix),
            classification: classification_label(Some(&entry.provider)).into(),
            selected: true,
        })
        .collect();
    masked_cleanup_diff(&candidates)
}

pub fn is_sensitive_env_path(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    name == ".env"
        || name.starts_with(".env.")
        || name.ends_with(".env")
        || name.eq_ignore_ascii_case("secrets.toml")
}

fn unescape(value: &str, quote: char) -> String {
    if quote == '\'' {
        return value.to_owned();
    }
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(character);
        }
    }
    out
}

pub fn apply_after_action(
    path: &Path,
    selected: &[String],
    aliases: &HashMap<String, String>,
    action: AfterImportAction,
) -> Result<Option<String>> {
    match action {
        AfterImportAction::Keep => Ok(None),
        AfterImportAction::ReplaceAliases => {
            let original = fs::read_to_string(path)?;
            let next = rewrite_env(&original, selected, Some(aliases))?;
            fs::write(path, &next)?;
            Ok(Some(next))
        }
        AfterImportAction::RemoveEntries => {
            let original = fs::read_to_string(path)?;
            let next = rewrite_env(&original, selected, None)?;
            fs::write(path, &next)?;
            Ok(Some(next))
        }
    }
}

fn rewrite_env(
    text: &str,
    selected: &[String],
    aliases: Option<&HashMap<String, String>>,
) -> Result<String> {
    let mut out = String::new();
    for line in text.lines() {
        let trimmed = line.trim().strip_prefix("export ").unwrap_or(line.trim());
        let key = trimmed.split_once('=').map(|(key, _)| key.trim());
        if let Some(key) = key {
            if selected.iter().any(|name| name == key) {
                if let Some(aliases) = aliases {
                    if let Some(alias) = aliases.get(key) {
                        out.push_str(&format!("{key}=workshop://secret/{alias}\n"));
                    }
                }
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

pub fn agent_visible_preview(preview: &ImportPreview) -> ImportPreview {
    preview.clone()
}

pub fn reject_if_agent_asked_for_value(name: &str) -> Result<()> {
    let lower = name.to_ascii_lowercase();
    if lower.contains("get")
        || lower.contains("reveal")
        || lower.contains("export")
        || lower.contains("readvalue")
        || lower.contains("read_value")
    {
        return Err(anyhow!(
            "agents cannot read secret values; only masked metadata is available"
        ));
    }
    Ok(())
}
