//! Bounded NDJSON sink used only before SQLite is available.

use anyhow::{Context, Result};
use chrono::Utc;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use super::record::LogRecord;
use crate::platform::failure::redaction::{redact_text, redact_value};

pub const EMERGENCY_FILE: &str = "emergency.ndjson";
const MAX_BYTES: u64 = 8 * 1024 * 1024;

pub fn path_for(data_root: &Path) -> PathBuf {
    data_root.join("logs").join(EMERGENCY_FILE)
}

pub fn write_record(data_root: &Path, record: &LogRecord) -> Result<()> {
    let dir = data_root.join("logs");
    fs::create_dir_all(&dir).context("create emergency log directory")?;
    let path = dir.join(EMERGENCY_FILE);
    if path.exists() {
        if fs::metadata(&path)?.len() >= MAX_BYTES {
            return Ok(());
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .context("open emergency ndjson")?;
    let line = serde_json::json!({
        "logId": record.log_id,
        "level": record.level.as_str(),
        "component": record.component,
        "event": record.event,
        "message": redact_text(&record.message),
        "operationId": record.operation_id.as_ref().map(|id| id.as_str()),
        "failureId": record.failure_id.as_ref().map(|id| id.as_str()),
        "fields": redact_value(record.fields.clone()),
        "at": record.at.to_rfc3339(),
        "mode": "emergency_file",
    });
    writeln!(file, "{line}").context("write emergency ndjson")?;
    Ok(())
}

pub fn write_line(data_root: &Path, component: &str, event: &str, message: &str) -> Result<()> {
    write_record(
        data_root,
        &LogRecord::new(super::record::LogLevel::Error, component, event, message),
    )
}

pub fn exists(data_root: &Path) -> bool {
    path_for(data_root).exists()
}

pub fn load_lines(data_root: &Path) -> Result<Vec<serde_json::Value>> {
    let path = path_for(data_root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).context("read emergency ndjson")?;
    Ok(text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

pub fn rotate(data_root: &Path) -> Result<Option<PathBuf>> {
    let path = path_for(data_root);
    if !path.exists() {
        return Ok(None);
    }
    let rotated = data_root.join("logs").join(format!(
        "emergency.imported.{}.ndjson",
        Utc::now().format("%Y%m%dT%H%M%SZ")
    ));
    fs::rename(&path, &rotated).context("rotate emergency ndjson")?;
    Ok(Some(rotated))
}
