//! SQLite outbox for product telemetry.
//!
//! One table serves both purposes: the local, user-inspectable record and the
//! sync spool. A watermark over rowid marks what has been shipped; retention
//! pruning bounds the spool without a separate queue.

use anyhow::Result;
use chrono::{Duration, Utc};
use rusqlite::params;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

use super::contract::{self, Sensitivity};
use crate::storage::Database;

const INSTALL_ID_KEY: &str = "telemetry.install_id";
const WATERMARK_KEY: &str = "telemetry.sync.watermark";
const LAST_SYNC_KEY: &str = "telemetry.sync.last_at";
const FIRST_PREFIX: &str = "telemetry.first.";

/// One stored event, addressed by rowid for watermarking.
#[derive(Clone, Debug)]
pub struct OutboxEvent {
    pub rowid: i64,
    pub event_id: String,
    pub name: String,
    pub at: String,
    pub properties: Value,
}

#[derive(Clone)]
pub struct TelemetryStore {
    db: Arc<Database>,
}

impl TelemetryStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn insert(&self, name: &str, sensitivity: Sensitivity, properties: &Value) -> Result<String> {
        let event_id = format!("pte_{}", Uuid::new_v4().simple());
        let at = Utc::now().to_rfc3339();
        let payload = serde_json::to_string(properties)?;
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO product_telemetry_events(event_id, name, at, sensitivity, properties_json)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    event_id,
                    name,
                    at,
                    contract::sensitivity_name(sensitivity),
                    payload
                ],
            )?;
            Ok(())
        })?;
        self.prune()?;
        Ok(event_id)
    }

    pub fn recent(&self, limit: i64) -> Result<Vec<(String, String, String, String, Value)>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT event_id, name, at, sensitivity, properties_json
                 FROM product_telemetry_events ORDER BY at DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map([limit.max(1).min(200)], |row| {
                let raw: String = row.get(4)?;
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    serde_json::from_str(&raw).unwrap_or(Value::Null),
                ))
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Sync-eligible events past the watermark, oldest first. Eligibility is
    /// decided by the contract at read time so a contract change never
    /// requires a schema migration.
    pub fn batch_for_sync(&self, limit: usize) -> Result<Vec<OutboxEvent>> {
        let watermark = self.watermark()?;
        let rows: Vec<OutboxEvent> = self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT rowid, event_id, name, at, properties_json
                 FROM product_telemetry_events
                 WHERE rowid > ?1 AND sensitivity = 'optional'
                 ORDER BY rowid ASC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![watermark, limit as i64], |row| {
                let raw: String = row.get(4)?;
                Ok(OutboxEvent {
                    rowid: row.get(0)?,
                    event_id: row.get(1)?,
                    name: row.get(2)?,
                    at: row.get(3)?,
                    properties: serde_json::from_str(&raw).unwrap_or(Value::Null),
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        Ok(rows
            .into_iter()
            .filter(|event| {
                contract::spec(&event.name)
                    .is_some_and(|spec| spec.sync == contract::SyncClass::Eligible)
            })
            .collect())
    }

    pub fn watermark(&self) -> Result<i64> {
        Ok(self
            .read_setting(WATERMARK_KEY)?
            .and_then(|value| value.parse().ok())
            .unwrap_or(0))
    }

    pub fn advance_watermark(&self, rowid: i64) -> Result<()> {
        self.write_setting(WATERMARK_KEY, &rowid.to_string())?;
        self.write_setting(LAST_SYNC_KEY, &Utc::now().to_rfc3339())
    }

    pub fn last_sync_at(&self) -> Result<Option<String>> {
        self.read_setting(LAST_SYNC_KEY)
    }

    pub fn install_id(&self) -> Result<String> {
        if let Some(existing) = self.read_setting(INSTALL_ID_KEY)? {
            return Ok(existing);
        }
        let id = format!("ins_{}", Uuid::new_v4().simple());
        self.write_setting(INSTALL_ID_KEY, &id)?;
        Ok(id)
    }

    pub fn first_marked(&self, name: &str) -> Result<bool> {
        Ok(self
            .read_setting(&format!("{FIRST_PREFIX}{name}"))?
            .is_some())
    }

    pub fn mark_first(&self, name: &str) -> Result<()> {
        self.write_setting(&format!("{FIRST_PREFIX}{name}"), &Utc::now().to_rfc3339())
    }

    pub fn prune(&self) -> Result<()> {
        let optional_floor = (Utc::now()
            - Duration::days(contract::retention_days(Sensitivity::Optional)))
        .to_rfc3339();
        let essential_floor = (Utc::now()
            - Duration::days(contract::retention_days(Sensitivity::Essential)))
        .to_rfc3339();
        self.db.with_conn(|conn| {
            conn.execute(
                "DELETE FROM product_telemetry_events
                 WHERE (sensitivity = 'optional' AND at < ?1)
                    OR (sensitivity = 'essential' AND at < ?2)",
                params![optional_floor, essential_floor],
            )?;
            Ok(())
        })
    }

    pub fn delete_optional(&self) -> Result<()> {
        self.db.with_conn(|conn| {
            conn.execute(
                "DELETE FROM product_telemetry_events WHERE sensitivity = 'optional'",
                [],
            )?;
            Ok(())
        })
    }

    pub fn read_setting(&self, key: &str) -> Result<Option<String>> {
        self.db.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT value_json FROM runtime_settings WHERE key = ?1")?;
            let mut rows = stmt.query([key])?;
            Ok(match rows.next()? {
                Some(row) => {
                    let raw: String = row.get(0)?;
                    let parsed = serde_json::from_str::<String>(&raw).unwrap_or(raw);
                    let trimmed = parsed.trim().trim_matches('"').to_owned();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                }
                None => None,
            })
        })
    }

    pub fn write_setting(&self, key: &str, value: &str) -> Result<()> {
        let payload = serde_json::to_string(value)?;
        let at = Utc::now().to_rfc3339();
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO runtime_settings(key, value_json, updated_at) VALUES(?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
                params![key, payload, at],
            )?;
            Ok(())
        })
    }
}
