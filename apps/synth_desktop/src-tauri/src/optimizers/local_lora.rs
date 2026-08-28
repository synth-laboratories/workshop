//! Durable This-Mac LoRA inventory. Bytes stay on disk; Workshop stores the
//! projection the catalog and sidecar inference pin against.

use super::models::{
    SavedLoraCheckpoint, SavedLoraCheckpointQuery, SavedLoraLineage, SavedLoraPatchRequest,
    SavedLoraStorage,
};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct HostedLoraOverlay {
    pub name: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub updated_at: String,
}

#[derive(Clone, Debug)]
pub struct LocalLoraUpsert {
    pub sha256: String,
    pub name: String,
    pub description: String,
    pub base_model: String,
    pub optimizer_algorithm: Option<String>,
    pub checkpoint_kind: String,
    pub step: Option<u64>,
    pub lora_rank: Option<i32>,
    pub adapter_path: PathBuf,
    pub size_bytes: Option<u64>,
    pub run_id: Option<String>,
    pub source_checkpoint_id: Option<String>,
    pub metadata: Value,
}

impl LocalLoraUpsert {
    pub fn from_checkpoint_event(run_id: &str, payload: &Value) -> Option<Self> {
        let path = payload.get("path").and_then(Value::as_str)?;
        let sha = payload.get("sha256").and_then(Value::as_str)?;
        let digest = normalize_digest(sha);
        let step = payload.get("step").and_then(Value::as_u64);
        let name = payload
            .get("checkpoint_id")
            .and_then(Value::as_str)
            .unwrap_or(path)
            .to_string();
        Some(Self {
            sha256: digest.clone(),
            name,
            description: "This Mac MLX adapter".into(),
            base_model: payload
                .get("base_model")
                .and_then(Value::as_str)
                .unwrap_or("Qwen/Qwen3.5-2B")
                .to_string(),
            optimizer_algorithm: payload
                .get("algorithm")
                .and_then(Value::as_str)
                .map(str::to_string),
            checkpoint_kind: "inference".into(),
            step,
            lora_rank: payload
                .get("lora_rank")
                .and_then(Value::as_i64)
                .map(|value| value as i32),
            adapter_path: PathBuf::from(path),
            size_bytes: payload.get("bytes").and_then(Value::as_u64),
            run_id: Some(run_id.to_string()),
            source_checkpoint_id: None,
            metadata: payload.clone(),
        })
    }
}

pub fn normalize_digest(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(hex) = trimmed.strip_prefix("sha256:") {
        format!("sha256:{}", hex.trim())
    } else {
        format!("sha256:{trimmed}")
    }
}

pub fn upsert_local_lora(conn: &Connection, row: &LocalLoraUpsert) -> Result<SavedLoraCheckpoint> {
    let now = iso_now();
    let tags = "[]";
    let metadata = serde_json::to_string(&row.metadata)?;
    let durable = persist_adapter_tree(&row.adapter_path, &row.sha256)?;
    let path = durable.display().to_string();
    conn.execute(
        "INSERT INTO local_lora_checkpoints(
            checkpoint_id, name, description, base_model, optimizer_algorithm,
            checkpoint_kind, step, lora_rank, status, adapter_path, sha256,
            size_bytes, run_id, source_checkpoint_id, tags_json, metadata_json,
            created_at, updated_at, archived_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'ready',?9,?10,?11,?12,?13,?14,?15,?16,?16,NULL)
         ON CONFLICT(checkpoint_id) DO UPDATE SET
            base_model=excluded.base_model,
            optimizer_algorithm=COALESCE(excluded.optimizer_algorithm, local_lora_checkpoints.optimizer_algorithm),
            checkpoint_kind=excluded.checkpoint_kind,
            step=excluded.step,
            lora_rank=excluded.lora_rank,
            status='ready',
            adapter_path=excluded.adapter_path,
            sha256=excluded.sha256,
            size_bytes=excluded.size_bytes,
            run_id=COALESCE(excluded.run_id, local_lora_checkpoints.run_id),
            source_checkpoint_id=COALESCE(excluded.source_checkpoint_id, local_lora_checkpoints.source_checkpoint_id),
            metadata_json=excluded.metadata_json,
            updated_at=excluded.updated_at,
            archived_at=NULL",
        params![
            row.sha256,
            row.name,
            row.description,
            row.base_model,
            row.optimizer_algorithm,
            row.checkpoint_kind,
            row.step.map(|value| value as i64),
            row.lora_rank,
            path,
            row.sha256,
            row.size_bytes.map(|value| value as i64),
            row.run_id,
            row.source_checkpoint_id,
            tags,
            metadata,
            now,
        ],
    )?;
    get_local_lora(conn, &row.sha256)?.ok_or_else(|| anyhow!("local LoRA upsert did not persist"))
}

pub fn import_local_lora_dir(conn: &Connection, path: &Path) -> Result<SavedLoraCheckpoint> {
    let adapter = path.to_path_buf();
    if !adapter.is_dir() {
        bail!("import path must be a directory containing mlx-lora.v1 files");
    }
    for required in ["adapter_config.json", "adapters.safetensors"] {
        if !adapter.join(required).is_file() {
            bail!("not mlx-lora.v1: missing {required}");
        }
    }
    let sha = digest_directory(&adapter)?;
    let size = dir_size(&adapter)?;
    let rank = read_lora_rank(&adapter);
    let name = adapter
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("imported-adapter")
        .to_string();
    upsert_local_lora(
        conn,
        &LocalLoraUpsert {
            sha256: sha,
            name,
            description: "Imported mlx-lora.v1 adapter".into(),
            // An adapter that declares its base is taken at its word. Stamping
            // every import with the local SFT student made a Laguna adapter
            // fail `is_laguna_compatible`, so no imported adapter could reach
            // the Composer picker.
            base_model: read_base_model(&adapter).unwrap_or_else(|| "Qwen/Qwen3.5-2B".into()),
            optimizer_algorithm: None,
            checkpoint_kind: "inference".into(),
            step: None,
            lora_rank: rank,
            adapter_path: adapter,
            size_bytes: Some(size),
            run_id: None,
            source_checkpoint_id: None,
            metadata: json!({"imported": true}),
        },
    )
}

pub fn get_local_lora(
    conn: &Connection,
    checkpoint_id: &str,
) -> Result<Option<SavedLoraCheckpoint>> {
    let id = normalize_digest(checkpoint_id);
    let row = conn
        .query_row(
            "SELECT checkpoint_id, name, description, base_model, optimizer_algorithm,
                    checkpoint_kind, step, lora_rank, status, adapter_path, sha256,
                    size_bytes, run_id, source_checkpoint_id, tags_json, metadata_json,
                    created_at, updated_at, archived_at
             FROM local_lora_checkpoints WHERE checkpoint_id = ?1",
            params![id],
            map_row,
        )
        .optional()?;
    Ok(row)
}

pub fn archive_local_lora(conn: &Connection, checkpoint_id: &str) -> Result<SavedLoraCheckpoint> {
    let id = normalize_digest(checkpoint_id);
    let now = iso_now();
    let changed = conn.execute(
        "UPDATE local_lora_checkpoints SET status='archived', archived_at=?1, updated_at=?1
         WHERE checkpoint_id=?2",
        params![now, id],
    )?;
    if changed == 0 {
        bail!("local LoRA not found");
    }
    get_local_lora(conn, &id)?.ok_or_else(|| anyhow!("local LoRA not found after archive"))
}

pub fn patch_local_lora(
    conn: &Connection,
    checkpoint_id: &str,
    patch: &SavedLoraPatchRequest,
) -> Result<SavedLoraCheckpoint> {
    let id = normalize_digest(checkpoint_id);
    let existing = get_local_lora(conn, &id)?.ok_or_else(|| anyhow!("local LoRA not found"))?;
    let name = patch
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&existing.name);
    let description = patch
        .description
        .as_deref()
        .unwrap_or(&existing.description);
    let tags = patch.tags.clone().unwrap_or(existing.tags);
    let tags_json = serde_json::to_string(&tags)?;
    let now = iso_now();
    conn.execute(
        "UPDATE local_lora_checkpoints SET name=?1, description=?2, tags_json=?3, updated_at=?4
         WHERE checkpoint_id=?5",
        params![name, description, tags_json, now, id],
    )?;
    get_local_lora(conn, &id)?.ok_or_else(|| anyhow!("local LoRA not found after patch"))
}

pub fn overlay_hosted_lora(
    conn: &Connection,
    checkpoint_id: &str,
    patch: &SavedLoraPatchRequest,
    mut base: SavedLoraCheckpoint,
) -> Result<SavedLoraCheckpoint> {
    upsert_hosted_overlay(conn, checkpoint_id, patch)?;
    let overlay = get_hosted_overlay(conn, checkpoint_id)?
        .ok_or_else(|| anyhow!("hosted LoRA overlay missing after upsert"))?;
    apply_hosted_overlay(&mut base, &overlay);
    Ok(base)
}

pub fn upsert_hosted_overlay(
    conn: &Connection,
    checkpoint_id: &str,
    patch: &SavedLoraPatchRequest,
) -> Result<()> {
    let name = patch
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let description = patch.description.as_deref();
    let tags_json = patch.tags.as_ref().map(serde_json::to_string).transpose()?;
    let now = iso_now();
    conn.execute(
        "INSERT INTO hosted_lora_overlays(checkpoint_id, name, description, tags_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(checkpoint_id) DO UPDATE SET
            name=COALESCE(excluded.name, hosted_lora_overlays.name),
            description=COALESCE(excluded.description, hosted_lora_overlays.description),
            tags_json=COALESCE(excluded.tags_json, hosted_lora_overlays.tags_json),
            updated_at=excluded.updated_at",
        params![checkpoint_id, name, description, tags_json, now],
    )?;
    Ok(())
}

pub fn get_hosted_overlay(
    conn: &Connection,
    checkpoint_id: &str,
) -> Result<Option<HostedLoraOverlay>> {
    conn.query_row(
        "SELECT name, description, tags_json, updated_at FROM hosted_lora_overlays WHERE checkpoint_id=?1",
        params![checkpoint_id],
        |row| {
            let tags_json: Option<String> = row.get(2)?;
            Ok(HostedLoraOverlay {
                name: row.get(0)?,
                description: row.get(1)?,
                tags: tags_json.and_then(|raw| serde_json::from_str(&raw).ok()),
                updated_at: row.get(3)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn clear_hosted_overlay(conn: &Connection, checkpoint_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM hosted_lora_overlays WHERE checkpoint_id=?1",
        params![checkpoint_id],
    )?;
    Ok(())
}

pub fn list_hosted_overlays(conn: &Connection) -> Result<HashMap<String, HostedLoraOverlay>> {
    let mut stmt = conn.prepare(
        "SELECT checkpoint_id, name, description, tags_json, updated_at FROM hosted_lora_overlays",
    )?;
    let rows = stmt.query_map([], |row| {
        let tags_json: Option<String> = row.get(3)?;
        Ok((
            row.get::<_, String>(0)?,
            HostedLoraOverlay {
                name: row.get(1)?,
                description: row.get(2)?,
                tags: tags_json.and_then(|raw| serde_json::from_str(&raw).ok()),
                updated_at: row.get(4)?,
            },
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (id, overlay) = row?;
        out.insert(id, overlay);
    }
    Ok(out)
}

pub fn apply_hosted_overlay(checkpoint: &mut SavedLoraCheckpoint, overlay: &HostedLoraOverlay) {
    if let Some(name) = overlay
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        checkpoint.name = name.to_string();
    }
    if let Some(description) = overlay.description.as_ref() {
        checkpoint.description = description.clone();
    }
    if let Some(tags) = overlay.tags.as_ref() {
        checkpoint.tags = tags.clone();
    }
    checkpoint.updated_at = Some(overlay.updated_at.clone());
    match checkpoint.metadata.as_object_mut() {
        Some(metadata) => {
            metadata.insert("localOverlay".into(), json!(true));
        }
        None => {
            checkpoint.metadata = json!({ "localOverlay": true });
        }
    }
}

pub fn is_laguna_compatible(checkpoint: &SavedLoraCheckpoint) -> bool {
    checkpoint.placement == "this_mac"
        && checkpoint.checkpoint_kind == "inference"
        && checkpoint.status == "ready"
        && {
            let base = checkpoint.base_model.to_ascii_lowercase();
            base.contains("laguna") || base.contains("poolside")
        }
}

pub fn durable_lora_root() -> PathBuf {
    if let Ok(raw) = std::env::var("SYNTH_LOCAL_LORA_ROOT") {
        let path = PathBuf::from(raw.trim());
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    crate::instance::state_root().join("loras")
}

pub fn persist_adapter_tree(src: &Path, digest: &str) -> Result<PathBuf> {
    let hex = digest.trim().trim_start_matches("sha256:");
    let dest = durable_lora_root().join(hex);
    if src == dest {
        return Ok(dest);
    }
    if dest.is_dir() {
        return Ok(dest);
    }
    fs::create_dir_all(&dest).with_context(|| format!("create {}", dest.display()))?;
    copy_adapter_files(src, &dest)?;
    Ok(dest)
}

fn copy_adapter_files(src: &Path, dest: &Path) -> Result<()> {
    for entry in fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry?;
        let from = entry.path();
        if from.is_file() {
            fs::copy(&from, dest.join(entry.file_name()))
                .with_context(|| format!("copy {}", from.display()))?;
        }
    }
    Ok(())
}

pub fn zip_adapter_dir(src: &Path) -> Result<(PathBuf, String)> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    let dest = durable_lora_root().join(format!(
        "{}.zip",
        src.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("adapter")
    ));
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(&dest)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for entry in fs::read_dir(src)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow!("adapter file name is not utf-8"))?;
        zip.start_file(name, options)?;
        zip.write_all(&fs::read(&path)?)?;
    }
    zip.finish()?;
    let mut hasher = Sha256::new();
    let mut bytes = fs::File::open(&dest)?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = bytes.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok((dest, format!("{:x}", hasher.finalize())))
}

pub fn search_local_loras(
    conn: &Connection,
    query: &SavedLoraCheckpointQuery,
) -> Result<Vec<SavedLoraCheckpoint>> {
    let mut sql = String::from(
        "SELECT checkpoint_id, name, description, base_model, optimizer_algorithm,
                checkpoint_kind, step, lora_rank, status, adapter_path, sha256,
                size_bytes, run_id, source_checkpoint_id, tags_json, metadata_json,
                created_at, updated_at, archived_at
         FROM local_lora_checkpoints WHERE 1=1",
    );
    let mut binds: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let status = query.status.clone().unwrap_or_else(|| "ready".into());
    if status != "all" {
        sql.push_str(" AND status = ?");
        binds.push(Box::new(status));
    }
    if let Some(algorithm) = query
        .optimizer_algorithm
        .as_deref()
        .filter(|value| *value != "all")
    {
        sql.push_str(" AND optimizer_algorithm = ?");
        binds.push(Box::new(algorithm.to_string()));
    }
    if let Some(kind) = query
        .checkpoint_kind
        .as_deref()
        .filter(|value| *value != "all")
    {
        sql.push_str(" AND checkpoint_kind = ?");
        binds.push(Box::new(kind.to_string()));
    }
    if let Some(run_id) = query.run_id.as_deref() {
        sql.push_str(" AND run_id = ?");
        binds.push(Box::new(run_id.to_string()));
    }
    if let Some(search) = query
        .search
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        sql.push_str(" AND (name LIKE ? OR checkpoint_id LIKE ? OR base_model LIKE ? OR adapter_path LIKE ?)");
        let pattern = format!("%{}%", search.trim());
        binds.push(Box::new(pattern.clone()));
        binds.push(Box::new(pattern.clone()));
        binds.push(Box::new(pattern.clone()));
        binds.push(Box::new(pattern));
    }
    sql.push_str(" ORDER BY updated_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(params_refs.as_slice(), map_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SavedLoraCheckpoint> {
    let checkpoint_id: String = row.get(0)?;
    let name: String = row.get(1)?;
    let description: String = row.get(2)?;
    let base_model: String = row.get(3)?;
    let optimizer_algorithm: Option<String> = row.get(4)?;
    let checkpoint_kind: String = row.get(5)?;
    let step: Option<i64> = row.get(6)?;
    let lora_rank: Option<i32> = row.get(7)?;
    let status: String = row.get(8)?;
    let adapter_path: String = row.get(9)?;
    let sha256: String = row.get(10)?;
    let size_bytes: Option<i64> = row.get(11)?;
    let run_id: Option<String> = row.get(12)?;
    let source_checkpoint_id: Option<String> = row.get(13)?;
    let tags_json: String = row.get(14)?;
    let metadata_json: String = row.get(15)?;
    let created_at: String = row.get(16)?;
    let updated_at: String = row.get(17)?;
    let archived_at: Option<String> = row.get(18)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let metadata: Value = serde_json::from_str(&metadata_json).unwrap_or_else(|_| json!({}));
    let ready = status == "ready" && checkpoint_kind == "inference";
    Ok(SavedLoraCheckpoint {
        schema_version: "saved_lora_checkpoint.v1".into(),
        checkpoint_id,
        org_id: "local".into(),
        owner_user_id: None,
        visibility: "private".into(),
        name,
        description,
        provider: "mlx".into(),
        checkpoint_kind,
        provider_checkpoint_reference: Some(sha256.clone()),
        run_id: run_id.clone(),
        attempt_id: None,
        source_checkpoint_id: source_checkpoint_id.clone(),
        optimizer_algorithm: optimizer_algorithm.clone(),
        base_model,
        lora_rank,
        step: step.map(|value| value as u64),
        status,
        storage: SavedLoraStorage {
            backend: "mlx-store".into(),
            bucket: "this-mac".into(),
            key: adapter_path,
            version: None,
            etag: None,
            sha256: Some(sha256),
            size_bytes: size_bytes.map(|value| value as u64),
            content_type: "application/x-mlx-lora".into(),
        },
        lineage: SavedLoraLineage {
            optimizer_algorithm,
            run_id,
            attempt_id: None,
            source_checkpoint_id,
            provider_checkpoint_reference: None,
        },
        placement: "this_mac".into(),
        inference_chat_completions: ready,
        inference_responses: ready,
        tags,
        metadata,
        created_at: Some(created_at),
        updated_at: Some(updated_at),
        archived_at,
    })
}

/// The one definition of an adapter tree's identity.
///
/// The publisher reproduces this framing exactly; a second implementation
/// would mean a downloaded adapter never matches the id it was published
/// under, so this is a contract rather than an implementation detail.
pub fn digest_directory(root: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut files: Vec<PathBuf> = fs::read_dir(root)
        .with_context(|| format!("read {}", root.display()))?
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    for file in files {
        let relative = file
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .as_bytes();
        let content = fs::read(&file)?;
        hasher.update((relative.len() as u64).to_be_bytes());
        hasher.update(relative);
        hasher.update((content.len() as u64).to_be_bytes());
        hasher.update(&content);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn dir_size(root: &Path) -> Result<u64> {
    let mut total = 0;
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_file() {
            total += fs::metadata(&path)?.len();
        }
    }
    Ok(total)
}

fn read_base_model(root: &Path) -> Option<String> {
    let raw = fs::read_to_string(root.join("adapter_config.json")).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    for pointer in [
        "/base_model",
        "/base_model_name_or_path",
        "/synth_test_fixture/base_model",
    ] {
        if let Some(name) = value.pointer(pointer).and_then(Value::as_str) {
            let name = name.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn read_lora_rank(root: &Path) -> Option<i32> {
    let raw = fs::read_to_string(root.join("adapter_config.json")).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    value
        .pointer("/lora_parameters/rank")
        .and_then(Value::as_i64)
        .or_else(|| value.get("r").and_then(Value::as_i64))
        .map(|value| value as i32)
}

fn iso_now() -> String {
    Utc::now().to_rfc3339()
}

