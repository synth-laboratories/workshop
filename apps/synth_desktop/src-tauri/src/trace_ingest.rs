//! Trace V5 bundle ingestion at the synth-containers/Desktop trust boundary.
//!
//! `synth-trace inspect-input` is the format authority. This module retains the
//! supplied bytes, consumes its versioned inspection result, and publishes only
//! trusted, self-contained bundles into Desktop's trace CAS and catalog.

use crate::data::{TraceBundleInspection, TraceRecord};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::process::Command;
use uuid::Uuid;

const MAX_INSPECTION_BYTES: usize = 16 * 1024 * 1024;
const MAX_PROJECTION_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TraceBundleIngestRequest {
    pub source_path: String,
    pub source_kind: Option<String>,
    pub title: Option<String>,
    pub source_uri: Option<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TraceBundleIngestResult {
    pub compatibility_level: String,
    pub trusted: bool,
    pub duplicate: bool,
    pub input_digest: String,
    pub bundle_digest: Option<String>,
    pub archive_digest: Option<String>,
    pub traces: Vec<TraceRecord>,
    #[specta(type = specta_typescript::Unknown)]
    pub validation: serde_json::Value,
}

pub(crate) struct InspectedInput {
    pub inspection: TraceBundleInspection,
    pub inspection_json: serde_json::Value,
    pub archive_bytes: Option<Vec<u8>>,
    pub raw_file_bytes: Option<Vec<u8>>,
}

pub(crate) struct DerivedTraceProjection {
    pub projection_schema: String,
    pub payload_digest: String,
    pub relative_path: String,
    pub payload: serde_json::Value,
}

/// Inspect one path through the format-owning CLI and ask it to atomically
/// materialize the exact verified deterministic archive it inspected.
pub(crate) async fn inspect_input(
    request: &TraceBundleIngestRequest,
    staging_root: &Path,
) -> Result<InspectedInput> {
    let source = PathBuf::from(&request.source_path);
    let metadata = fs::symlink_metadata(&source)
        .with_context(|| format!("inspect trace input {}", source.display()))?;
    if metadata.file_type().is_symlink() {
        bail!("trace input may not be a symlink");
    }
    if !metadata.is_file() && !metadata.is_dir() {
        bail!("trace input must be a regular file or directory");
    }

    fs::create_dir_all(staging_root)?;
    let staging = staging_root.join(format!("inspect-{}", Uuid::new_v4().simple()));
    fs::create_dir(&staging)?;
    let archive_path = staging.join("bundle.zip");
    let cli = resolve_trace_cli();
    let output = Command::new(cli)
        .arg("inspect-input")
        .arg(&source)
        .arg("--archive-output")
        .arg(&archive_path)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .context("run synth-trace inspect-input; install a compatible synth-containers build or set SYNTH_TRACE_CLI")?;

    let result = (|| -> Result<InspectedInput> {
        if output.stdout.len() > MAX_INSPECTION_BYTES {
            bail!("trace inspection exceeded {MAX_INSPECTION_BYTES} bytes");
        }
        let inspection_json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .with_context(|| {
                let stderr = String::from_utf8_lossy(&output.stderr);
                format!(
                    "decode synth-trace inspection (status {}; stderr: {stderr})",
                    output.status
                )
            })?;
        let inspection: TraceBundleInspection = serde_json::from_value(inspection_json.clone())
            .context("decode synth.trace-inspection.v1")?;
        if inspection.schema_version != "synth.trace-inspection.v1" {
            bail!(
                "unsupported trace inspection schema: {}",
                inspection.schema_version
            );
        }
        let archive_bytes = archive_path
            .exists()
            .then(|| fs::read(&archive_path))
            .transpose()?;
        let raw_file_bytes = metadata.is_file().then(|| fs::read(&source)).transpose()?;
        Ok(InspectedInput {
            inspection,
            inspection_json,
            archive_bytes,
            raw_file_bytes,
        })
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}

/// Resolve the format-authority CLI without assuming a Finder-launched app has
/// inherited the user's interactive shell PATH. Release packaging can place the
/// executable under Resources/bin; local dogfood installs also recognize the
/// stable Synth data location and the conventional sibling development checkout.
pub(crate) fn resolve_trace_cli() -> PathBuf {
    if let Some(explicit) = env::var_os("SYNTH_TRACE_CLI") {
        return PathBuf::from(explicit);
    }
    let mut candidates = Vec::new();
    if let Ok(executable) = env::current_exe() {
        if let Some(contents) = executable.parent().and_then(Path::parent) {
            candidates.push(contents.join("Resources/bin/synth-trace"));
        }
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".synth-desktop/bin/synth-trace"));
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from("synth-trace"))
}

/// Derive a consumer projection from a trusted Trace V5 archive on demand.
///
/// Producers are only responsible for sealing Trace V5. They do not need to
/// know which Desktop visual happens to open that trace. Projection runs in an
/// isolated staging directory so neither the trusted archive nor its semantic
/// identities are mutated.
pub(crate) async fn project_trace_archive(
    archive_path: &Path,
    trace_digest: &str,
    projection_kind: &str,
    staging_root: &Path,
) -> Result<DerivedTraceProjection> {
    let projection_format = match projection_kind {
        "rollout-inspector" => "rollout-inspector",
        other => bail!("unsupported on-demand Trace V5 projection: {other}"),
    };
    let trace_digest = qualified_sha256(trace_digest)?;
    fs::create_dir_all(staging_root)?;
    let staging = staging_root.join(format!("project-{}", Uuid::new_v4().simple()));
    fs::create_dir(&staging)?;
    let bundle_path = staging.join("bundle");
    let cli = resolve_trace_cli();

    let result = async {
        let extracted = Command::new(&cli)
            .arg("extract")
            .arg(archive_path)
            .arg(&bundle_path)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .context("extract trusted Trace V5 archive for projection")?;
        if !extracted.status.success() {
            bail!(
                "Trace V5 extraction failed (status {}): {}",
                extracted.status,
                String::from_utf8_lossy(&extracted.stderr)
            );
        }

        let projected = Command::new(&cli)
            .arg("project")
            .arg(&bundle_path)
            .arg("--format")
            .arg(projection_format)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .context("derive projection from sealed Trace V5")?;
        if !projected.status.success() {
            bail!(
                "Trace V5 projection failed (status {}): {}",
                projected.status,
                String::from_utf8_lossy(&projected.stderr)
            );
        }
        if projected.stdout.len() > MAX_INSPECTION_BYTES {
            bail!("Trace V5 projection receipt exceeded {MAX_INSPECTION_BYTES} bytes");
        }

        let receipts: Vec<serde_json::Value> = serde_json::from_slice(&projected.stdout)
            .context("decode Trace V5 projection receipt")?;
        let receipt = receipts
            .iter()
            .find(|item| {
                item.pointer("/manifest/source_trace_digest")
                    .and_then(serde_json::Value::as_str)
                    == Some(trace_digest.as_str())
            })
            .ok_or_else(|| anyhow!("projection omitted sealed trace {trace_digest}"))?;
        let manifest = receipt
            .get("manifest")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| anyhow!("Trace V5 projection receipt omitted its manifest"))?;
        let projection_schema = manifest
            .get("format")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("Trace V5 projection manifest omitted format"))?
            .to_string();
        let payload_digest = qualified_sha256(
            manifest
                .get("target_digest")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("Trace V5 projection manifest omitted target digest"))?,
        )?;
        let projection_path = PathBuf::from(
            receipt
                .get("path")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("Trace V5 projection receipt omitted output path"))?,
        );
        let canonical_bundle = fs::canonicalize(&bundle_path)?;
        let canonical_projection = fs::canonicalize(&projection_path)?;
        if !canonical_projection.starts_with(&canonical_bundle) {
            bail!("Trace V5 projector returned a path outside its isolated bundle");
        }
        let relative_path = canonical_projection
            .strip_prefix(&canonical_bundle)?
            .to_string_lossy()
            .trim_start_matches('/')
            .to_string();
        let metadata = fs::metadata(&canonical_projection)?;
        if metadata.len() > MAX_PROJECTION_BYTES as u64 {
            bail!("projection payload exceeds {MAX_PROJECTION_BYTES} bytes");
        }
        let document: serde_json::Value = serde_json::from_slice(&fs::read(&canonical_projection)?)
            .context("decode derived Trace V5 projection JSON")?;
        let payload = document.get("payload").cloned().unwrap_or(document);
        if payload
            .get("trace_digest")
            .and_then(serde_json::Value::as_str)
            != Some(trace_digest.as_str())
        {
            bail!("derived projection is not bound to requested trace {trace_digest}");
        }

        Ok(DerivedTraceProjection {
            projection_schema,
            payload_digest,
            relative_path,
            payload,
        })
    }
    .await;

    let _ = fs::remove_dir_all(&staging);
    result
}

pub(crate) fn qualified_sha256(value: &str) -> Result<String> {
    let hex = value.strip_prefix("sha256:").unwrap_or(value);
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(anyhow!("invalid sha256 digest: {value}"));
    }
    Ok(format!("sha256:{}", hex.to_ascii_lowercase()))
}
