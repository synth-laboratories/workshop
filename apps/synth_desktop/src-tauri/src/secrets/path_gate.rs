//! Workspace-relative credential path authority.
//!
//! Agent-facing callers name an opaque root reference and a relative path.
//! Canonical paths never cross that boundary.

use anyhow::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

use super::lease::{
    CredentialError, CREDENTIAL_LOCATOR_BROAD_DISCOVERY, CREDENTIAL_LOCATOR_NOT_REGULAR_FILE,
    CREDENTIAL_LOCATOR_UNAPPROVED_WORKSPACE, CREDENTIAL_PATH_ESCAPE,
};

pub const MAX_RELATIVE_PATH_BYTES: usize = 256;

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRootSummary {
    pub workspace_root_ref: String,
    pub display_name: String,
}

#[derive(Clone, Debug)]
pub struct GatedWorkspacePath {
    pub workspace_root_ref: String,
    pub root_canonical: PathBuf,
    pub relative_path: String,
    pub file_canonical: PathBuf,
}

pub fn workspace_root_ref(canonical_root: &Path) -> String {
    let digest = Sha256::digest(canonical_root.as_os_str().to_string_lossy().as_bytes());
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("wsroot_{hex}")
}

pub fn canonical_workspace_roots(roots: &[PathBuf]) -> Vec<(String, PathBuf)> {
    roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .filter(|root| root.is_dir())
        .map(|root| (workspace_root_ref(&root), root))
        .collect()
}

pub fn summarize_workspace_roots(roots: &[PathBuf]) -> Vec<WorkspaceRootSummary> {
    let canonical = canonical_workspace_roots(roots);
    let mut names = canonical
        .iter()
        .map(|(_, root)| {
            root.file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("workspace")
                .to_owned()
        })
        .collect::<Vec<_>>();
    for index in 0..names.len() {
        if names
            .iter()
            .enumerate()
            .any(|(other, name)| other != index && name == &names[index])
        {
            let parent = canonical[index]
                .1
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .unwrap_or("workspace");
            names[index] = format!("{parent}/{}", names[index]);
        }
    }
    for index in 0..names.len() {
        if names
            .iter()
            .enumerate()
            .any(|(other, name)| other != index && name == &names[index])
        {
            names[index].push_str(&format!(" · {}", &canonical[index].0[7..11]));
        }
    }
    canonical
        .into_iter()
        .zip(names)
        .map(
            |((workspace_root_ref, _), display_name)| WorkspaceRootSummary {
                workspace_root_ref,
                display_name,
            },
        )
        .collect()
}

pub fn gate_workspace_file(
    roots: &[PathBuf],
    requested_root_ref: &str,
    relative_path: &str,
) -> Result<GatedWorkspacePath> {
    if roots.is_empty() {
        return Err(CredentialError::new(
            CREDENTIAL_LOCATOR_UNAPPROVED_WORKSPACE,
            "path_gate",
            false,
            "no workspace roots are approved",
        )
        .anyhow());
    }
    if relative_path.as_bytes().len() > MAX_RELATIVE_PATH_BYTES {
        return Err(CredentialError::new(
            CREDENTIAL_PATH_ESCAPE,
            "path_gate",
            false,
            "credential relative path is longer than 256 bytes",
        )
        .anyhow());
    }
    if relative_path
        .chars()
        .any(|character| matches!(character, '*' | '?' | '[' | ']'))
    {
        return Err(CredentialError::new(
            CREDENTIAL_LOCATOR_BROAD_DISCOVERY,
            "path_gate",
            false,
            "credential locations must name one file, not a search pattern",
        )
        .anyhow());
    }
    let relative = Path::new(relative_path);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(CredentialError::new(
            CREDENTIAL_PATH_ESCAPE,
            "path_gate",
            false,
            "credential path must stay relative to its approved workspace",
        )
        .anyhow());
    }
    let Some((workspace_root_ref, root_canonical)) = canonical_workspace_roots(roots)
        .into_iter()
        .find(|(reference, _)| reference == requested_root_ref)
    else {
        return Err(CredentialError::new(
            CREDENTIAL_LOCATOR_UNAPPROVED_WORKSPACE,
            "path_gate",
            false,
            "workspace root is not currently approved",
        )
        .anyhow());
    };

    let mut current = root_canonical.clone();
    for component in relative.components() {
        match component {
            Component::CurDir => continue,
            Component::Normal(name) => current.push(name),
            _ => unreachable!("unsafe components were rejected above"),
        }
        let metadata = std::fs::symlink_metadata(&current).map_err(|_| {
            CredentialError::new(
                CREDENTIAL_LOCATOR_NOT_REGULAR_FILE,
                "path_gate",
                false,
                "credential locator does not identify a regular file",
            )
            .anyhow()
        })?;
        if metadata.file_type().is_symlink() {
            let target = std::fs::canonicalize(&current).map_err(|_| {
                CredentialError::new(
                    CREDENTIAL_PATH_ESCAPE,
                    "path_gate",
                    false,
                    "credential path contains an unresolved symlink",
                )
                .anyhow()
            })?;
            if !target.starts_with(&root_canonical) {
                return Err(CredentialError::new(
                    CREDENTIAL_PATH_ESCAPE,
                    "path_gate",
                    false,
                    "credential path symlink leaves the approved workspace",
                )
                .anyhow());
            }
            current = target;
        }
    }
    let file_canonical = std::fs::canonicalize(&current).map_err(|_| {
        CredentialError::new(
            CREDENTIAL_LOCATOR_NOT_REGULAR_FILE,
            "path_gate",
            false,
            "credential locator does not identify a regular file",
        )
        .anyhow()
    })?;
    if !file_canonical.starts_with(&root_canonical) || !file_canonical.is_file() {
        let code = if file_canonical.starts_with(&root_canonical) {
            CREDENTIAL_LOCATOR_NOT_REGULAR_FILE
        } else {
            CREDENTIAL_PATH_ESCAPE
        };
        return Err(CredentialError::new(
            code,
            "path_gate",
            false,
            "credential locator does not identify an allowed regular file",
        )
        .anyhow());
    }
    Ok(GatedWorkspacePath {
        workspace_root_ref,
        root_canonical,
        relative_path: relative.to_string_lossy().into_owned(),
        file_canonical,
    })
}

pub fn is_valid_env_variable(variable: &str) -> bool {
    let mut bytes = variable.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

