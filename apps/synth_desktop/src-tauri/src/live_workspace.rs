//! Host-owned, read-only access to a live Harbor workspace.
//!
//! A visual can ask only for the exact descriptor path persisted in its
//! `live_workspace` binding. The descriptor carries no credential. Docker is
//! addressed through Harbor's opaque container/context reference; provider
//! credentials and arbitrary host paths never cross into the renderer.

use crate::{visuals, AppError, CoreRuntime};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::State;
use tokio::process::Command;

pub const LIVE_WORKSPACE_SCHEMA: &str = "synth.live-workspace.v1";
pub const HARBOR_STREAMS_REVISION: &str = "d4fe6133eb196247cad104d01103e37f67ec488d";
const MAX_DESCRIPTOR_BYTES: u64 = 64 * 1024;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 500;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualWorkspaceRequest {
    pub visual_id: String,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualWorkspacePathRequest {
    pub visual_id: String,
    pub source: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualWorkspaceStatus {
    pub schema_version: String,
    pub state: String,
    pub provider: String,
    pub mode: String,
    pub workspace: String,
    pub capabilities: Vec<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualWorkspaceEntry {
    pub path: String,
    pub name: String,
    pub kind: String,
    #[specta(type = specta_typescript::Number)]
    pub size: u64,
    pub modified_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualWorkspaceListResult {
    pub schema_version: String,
    pub path: String,
    pub entries: Vec<VisualWorkspaceEntry>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VisualWorkspaceReadResult {
    pub schema_version: String,
    pub path: String,
    pub content: Option<String>,
    pub binary: bool,
    pub truncated: bool,
    #[specta(type = specta_typescript::Number)]
    pub byte_size: u64,
}

#[derive(Clone, Debug)]
struct DockerWorkspace {
    context: String,
    container_id: String,
    root: String,
}

fn forbidden_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "password",
        "credential",
        "private",
        "secret",
        "token",
        "api_key",
        "apikey",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn reject_secrets(value: &Value) -> Result<()> {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if forbidden_key(key) {
                    bail!("live workspace descriptor contains forbidden secret field {key:?}");
                }
                reject_secrets(value)?;
            }
        }
        Value::Array(items) => {
            for value in items {
                reject_secrets(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn safe_identifier(value: &str, label: &str) -> Result<String> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('-')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'));
    if !valid {
        bail!("Harbor {label} is invalid");
    }
    Ok(value.to_owned())
}

fn parse_harbor_descriptor(bytes: &[u8]) -> Result<DockerWorkspace> {
    let value: Value = serde_json::from_slice(bytes).context("decode live workspace descriptor")?;
    reject_secrets(&value)?;
    if let Some(schema) = value.get("schemaVersion").and_then(Value::as_str) {
        if schema != LIVE_WORKSPACE_SCHEMA {
            bail!("unsupported live workspace descriptor schema {schema}");
        }
    }
    let provider = value
        .get("provider")
        .and_then(Value::as_str)
        .context("live workspace descriptor omitted provider")?;
    if provider != "docker" {
        bail!("Harbor provider {provider:?} does not expose a read-only Workshop adapter yet");
    }
    let reference = value
        .get("reference")
        .and_then(Value::as_object)
        .context("Harbor stream descriptor omitted reference")?;
    let context = safe_identifier(
        reference
            .get("context")
            .and_then(Value::as_str)
            .unwrap_or("default"),
        "Docker context",
    )?;
    let container_id = safe_identifier(
        reference
            .get("container_id")
            .and_then(Value::as_str)
            .context("Harbor Docker stream descriptor omitted container_id")?,
        "container id",
    )?;
    let root = value
        .get("workspace")
        .and_then(Value::as_str)
        .context("Harbor stream descriptor omitted workspace")?;
    if root == "/" || !root.starts_with('/') || root.len() > 4096 || root.contains('\0') {
        bail!("Harbor workspace root is invalid");
    }
    Ok(DockerWorkspace {
        context,
        container_id,
        root: root.trim_end_matches('/').to_owned() + "/",
    })
}

fn normalized_relative(path: &str) -> Result<String> {
    if path.len() > 4096 || path.contains('\0') || Path::new(path).is_absolute() {
        bail!("workspace path must be a bounded relative path");
    }
    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => parts.push(
                part.to_str()
                    .context("workspace path is not valid UTF-8")?
                    .to_owned(),
            ),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("workspace path escapes its declared root")
            }
        }
    }
    Ok(parts.join("/"))
}

fn remote_path(workspace: &DockerWorkspace, relative: &str) -> Result<String> {
    let relative = normalized_relative(relative)?;
    Ok(if relative.is_empty() {
        workspace.root.trim_end_matches('/').to_owned()
    } else {
        format!("{}{relative}", workspace.root)
    })
}

async fn canonical_remote_path(workspace: &DockerWorkspace, relative: &str) -> Result<String> {
    let target = remote_path(workspace, relative)?;
    let root = workspace.root.trim_end_matches('/');
    let output = docker_output(
        workspace,
        &[
            "sh",
            "-c",
            r#"root=$(readlink -f -- "$1") || exit 10
target=$(readlink -f -- "$2") || exit 11
if [ "$root" = / ]; then
  printf '%s' "$target"
else
  case "$target" in
    "$root"|"$root"/*) printf '%s' "$target" ;;
    *) exit 12 ;;
  esac
fi"#,
            "workshop",
            root,
            &target,
        ],
    )
    .await
    .context("workspace path is unavailable or escapes its declared root")?;
    String::from_utf8(output.stdout).context("Harbor workspace returned a non-UTF-8 canonical path")
}

async fn declared_descriptor(
    state: &Arc<CoreRuntime>,
    visual_id: &str,
    source: &str,
) -> Result<(PathBuf, DockerWorkspace)> {
    let visual = state.visuals().get(visual_id.to_owned()).await?;
    if !visuals::declared_workspace_sources(&visual.bindings)
        .iter()
        .any(|declared| declared == source)
    {
        bail!("live workspace descriptor is not declared on this visual");
    }
    let path = PathBuf::from(source);
    let metadata = tokio::fs::metadata(&path).await.with_context(|| {
        format!(
            "live workspace descriptor is unavailable: {}",
            path.display()
        )
    })?;
    if !metadata.is_file() || metadata.len() > MAX_DESCRIPTOR_BYTES {
        bail!("live workspace descriptor must be a file no larger than 64 KiB");
    }
    let canonical = tokio::fs::canonicalize(&path).await?;
    let bytes = tokio::fs::read(&canonical).await?;
    Ok((canonical, parse_harbor_descriptor(&bytes)?))
}

async fn docker_output(workspace: &DockerWorkspace, args: &[&str]) -> Result<std::process::Output> {
    let context = tokio::time::timeout(
        COMMAND_TIMEOUT,
        Command::new("docker")
            .arg("context")
            .arg("inspect")
            .arg(&workspace.context)
            .arg("--format")
            .arg("{{json .Endpoints.docker.Host}}")
            .output(),
    )
    .await
    .context("Docker context inspection timed out")??;
    if !context.status.success()
        || !String::from_utf8_lossy(&context.stdout)
            .trim_matches(|ch: char| ch.is_ascii_whitespace() || ch == '"')
            .starts_with("unix://")
    {
        bail!("Harbor live workspace requires a local Unix-socket Docker context");
    }
    let mut command = Command::new("docker");
    command
        .kill_on_drop(true)
        .arg("--context")
        .arg(&workspace.context)
        .arg("exec")
        .arg(&workspace.container_id)
        .args(args);
    let output = tokio::time::timeout(COMMAND_TIMEOUT, command.output())
        .await
        .context("Docker workspace request timed out")??;
    if !output.status.success() {
        bail!("Harbor Docker workspace is unavailable");
    }
    Ok(output)
}

#[tauri::command]
#[specta::specta]
pub async fn visual_workspace_status(
    state: State<'_, Arc<CoreRuntime>>,
    request: VisualWorkspaceRequest,
) -> Result<VisualWorkspaceStatus, AppError> {
    let (_, workspace) = declared_descriptor(&state, &request.visual_id, &request.source)
        .await
        .map_err(AppError::from)?;
    let output = docker_output(
        &workspace,
        &[
            "sh",
            "-c",
            "test -d \"$1\" && printf ready",
            "workshop",
            workspace.root.trim_end_matches('/'),
        ],
    )
    .await;
    let (state_name, message, capabilities) = match output {
        Ok(_) => (
            "observing".to_owned(),
            None,
            vec![
                "files.read".into(),
                "files.watch".into(),
                "trajectory.preview".into(),
                "artifacts.read".into(),
            ],
        ),
        Err(error) => (
            "unavailable".to_owned(),
            Some(error.to_string()),
            Vec::new(),
        ),
    };
    Ok(VisualWorkspaceStatus {
        schema_version: LIVE_WORKSPACE_SCHEMA.into(),
        state: state_name,
        provider: "harbor-docker".into(),
        mode: "observe".into(),
        workspace: workspace.root.trim_end_matches('/').to_owned(),
        capabilities,
        message,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn visual_workspace_list(
    state: State<'_, Arc<CoreRuntime>>,
    request: VisualWorkspacePathRequest,
) -> Result<VisualWorkspaceListResult, AppError> {
    let (_, workspace) = declared_descriptor(&state, &request.visual_id, &request.source)
        .await
        .map_err(AppError::from)?;
    let relative = normalized_relative(&request.path).map_err(AppError::from)?;
    let target = canonical_remote_path(&workspace, &relative)
        .await
        .map_err(AppError::from)?;
    let output = docker_output(
        &workspace,
        &[
            "find",
            &target,
            "-mindepth",
            "1",
            "-maxdepth",
            "1",
            "-printf",
            "%y\\t%s\\t%T@\\t%f\\0",
        ],
    )
    .await
    .map_err(AppError::from)?;
    let mut entries = Vec::new();
    let mut truncated = false;
    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|row| !row.is_empty())
    {
        if entries.len() == MAX_ENTRIES {
            truncated = true;
            break;
        }
        let row = String::from_utf8_lossy(raw);
        let fields = row.splitn(4, '\t').collect::<Vec<_>>();
        if fields.len() != 4 || fields[3].contains('/') {
            continue;
        }
        let path = if relative.is_empty() {
            fields[3].to_owned()
        } else {
            format!("{relative}/{}", fields[3])
        };
        entries.push(VisualWorkspaceEntry {
            path,
            name: fields[3].to_owned(),
            kind: match fields[0] {
                "d" => "directory",
                "l" => "symlink",
                _ => "file",
            }
            .into(),
            size: fields[1].parse().unwrap_or(0),
            modified_at: fields[2]
                .split('.')
                .next()
                .and_then(|seconds| seconds.parse::<i64>().ok())
                .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
                .map(|time| time.to_rfc3339()),
        });
    }
    entries.sort_by(|left, right| {
        let left_dir = left.kind == "directory";
        let right_dir = right.kind == "directory";
        right_dir
            .cmp(&left_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(VisualWorkspaceListResult {
        schema_version: LIVE_WORKSPACE_SCHEMA.into(),
        path: relative,
        entries,
        truncated,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn visual_workspace_read(
    state: State<'_, Arc<CoreRuntime>>,
    request: VisualWorkspacePathRequest,
) -> Result<VisualWorkspaceReadResult, AppError> {
    let (_, workspace) = declared_descriptor(&state, &request.visual_id, &request.source)
        .await
        .map_err(AppError::from)?;
    let relative = normalized_relative(&request.path).map_err(AppError::from)?;
    if relative.is_empty() {
        return Err(AppError::from(anyhow::anyhow!("select a file to preview")));
    }
    let target = canonical_remote_path(&workspace, &relative)
        .await
        .map_err(AppError::from)?;
    let size_output = docker_output(&workspace, &["stat", "-Lc", "%s", "--", &target])
        .await
        .map_err(AppError::from)?;
    let byte_size = String::from_utf8_lossy(&size_output.stdout)
        .trim()
        .parse::<u64>()
        .context("Harbor workspace returned an invalid file size")
        .map_err(AppError::from)?;
    if byte_size > MAX_FILE_BYTES {
        return Ok(VisualWorkspaceReadResult {
            schema_version: LIVE_WORKSPACE_SCHEMA.into(),
            path: relative,
            content: None,
            binary: false,
            truncated: true,
            byte_size,
        });
    }
    let output = docker_output(&workspace, &["cat", "--", &target])
        .await
        .map_err(AppError::from)?;
    let content = String::from_utf8(output.stdout);
    Ok(match content {
        Ok(content) => VisualWorkspaceReadResult {
            schema_version: LIVE_WORKSPACE_SCHEMA.into(),
            path: relative,
            content: Some(content),
            binary: false,
            truncated: false,
            byte_size,
        },
        Err(_) => VisualWorkspaceReadResult {
            schema_version: LIVE_WORKSPACE_SCHEMA.into(),
            path: relative,
            content: None,
            binary: true,
            truncated: false,
            byte_size,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harbor_docker_descriptor_is_credential_free_and_bounded() {
        let parsed = parse_harbor_descriptor(
            br#"{"provider":"docker","reference":{"container_id":"abc123","context":"default"},"workspace":"/workspace"}"#,
        )
        .unwrap();
        assert_eq!(parsed.container_id, "abc123");
        assert_eq!(parsed.root, "/workspace/");
        assert!(parse_harbor_descriptor(
            br#"{"provider":"docker","reference":{"container_id":"abc","context":"default","api_key":"nope"},"workspace":"/workspace"}"#,
        )
        .unwrap_err()
        .to_string()
        .contains("forbidden secret"));
        assert!(parse_harbor_descriptor(
            br#"{"provider":"docker","reference":{"container_id":"abc","context":"default"}}"#,
        )
        .unwrap_err()
        .to_string()
        .contains("omitted workspace"));
    }

    #[test]
    fn relative_paths_fail_closed() {
        assert_eq!(normalized_relative("src/main.rs").unwrap(), "src/main.rs");
        assert!(normalized_relative("../etc/passwd").is_err());
        assert!(normalized_relative("/etc/passwd").is_err());
    }
}
