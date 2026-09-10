//! User-configured executable and workspace boundaries. MCP cannot register an
//! arbitrary executable or expand these roots. Secrets remain in local .env files.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Backend {
    pub id: String,
    pub command: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    pub workspace: PathBuf,
    pub env_file: Option<PathBuf>,
    pub max_sessions: u32,
    pub max_turn_seconds: u32,
}

impl Backend {
    pub fn validate(&mut self) -> Result<()> {
        anyhow::ensure!(
            !self.id.is_empty()
                && self.id.len() <= 64
                && self
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
            "invalid backend id"
        );
        anyhow::ensure!(
            self.command.is_absolute() && self.workspace.is_absolute(),
            "backend executable and workspace must be absolute"
        );
        self.command = self
            .command
            .canonicalize()
            .context("backend executable not found")?;
        self.workspace = self
            .workspace
            .canonicalize()
            .context("backend workspace not found")?;
        anyhow::ensure!(
            self.command.is_file() && self.workspace.is_dir(),
            "invalid executable or workspace"
        );
        anyhow::ensure!(
            (1..=16).contains(&self.max_sessions) && (1..=3600).contains(&self.max_turn_seconds),
            "backend limits are outside supported bounds"
        );
        anyhow::ensure!(
            self.args.len() <= 64 && self.args.iter().all(|arg| arg.len() <= 8192),
            "backend arguments are too large"
        );
        if let Some(path) = &mut self.env_file {
            anyhow::ensure!(path.is_absolute(), "env file must be absolute");
            *path = path.canonicalize()?;
            anyhow::ensure!(
                path.starts_with(&self.workspace) && path.is_file(),
                "env file must belong to the configured workspace"
            );
        }
        Ok(())
    }
}

pub fn read(root: &Path) -> Result<Vec<Backend>> {
    let path = root.join("agent-backends.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let metadata = fs::symlink_metadata(&path)?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= 1024 * 1024,
        "invalid backend registry file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        anyhow::ensure!(
            metadata.uid() == unsafe { libc::geteuid() }
                && metadata.permissions().mode() & 0o077 == 0,
            "backend registry must be private and owned by the current user"
        );
    }
    let mut backends: Vec<Backend> = serde_json::from_slice(&fs::read(path)?)?;
    let mut ids = std::collections::HashSet::new();
    for backend in &mut backends {
        backend.validate()?;
        anyhow::ensure!(ids.insert(backend.id.clone()), "duplicate backend id");
    }
    Ok(backends)
}

