//! Privacy-safe product telemetry.
//!
//! Optional funnel events are allowlisted, scanned, and dropped when the
//! operator opts out. Essential reliability events still record. Nothing in
//! this module accepts prompts, traces, filenames, or secret values.

mod dictionary;
mod privacy;

use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::sync::{Arc, OnceLock};
use tauri::State;
use uuid::Uuid;

use crate::error::AppError;
use crate::storage::Database;

pub use dictionary::{spec, EventSpec, COLLECTION_POLICY_VERSION, DICTIONARY_VERSION, EVENTS};

const ENABLED_KEY: &str = "telemetry.product.enabled";
const INSTALL_ID_KEY: &str = "telemetry.install_id";
const CONSENT_KEY: &str = "telemetry.consent_version";
const FIRST_PREFIX: &str = "telemetry.first.";

static LIVE: OnceLock<Arc<ProductTelemetry>> = OnceLock::new();

pub fn install_live(service: Arc<ProductTelemetry>) {
    let _ = LIVE.set(service);
}

pub fn live() -> Option<Arc<ProductTelemetry>> {
    LIVE.get().cloned()
}

/// Best-effort emit used by identity and activation call sites.
pub fn emit(name: &str, properties: Value) {
    if let Some(telemetry) = live() {
        if let Err(error) = telemetry.record(name, properties, false) {
            crate::platform::logging::report("telemetry", "eprintln", format!("synth-desktop: product telemetry dropped {name}: {error}"));
        }
    }
}

/// Record `name` at most once per install.
pub fn mark_once(name: &str, properties: Value) {
    if let Some(telemetry) = live() {
        if let Err(error) = telemetry.record(name, properties, true) {
            crate::platform::logging::report("telemetry", "eprintln", format!("synth-desktop: product telemetry dropped {name}: {error}"));
        }
    }
}

#[derive(Clone)]
pub struct ProductTelemetry {
    db: Arc<Database>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryPolicy {
    pub dictionary_version: String,
    pub collection_policy_version: String,
    pub optional_enabled: bool,
    pub consent_version: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryEventRecord {
    pub event_id: String,
    pub name: String,
    pub at: String,
    pub sensitivity: String,
    #[specta(type = specta_typescript::Unknown)]
    pub properties: Value,
}

impl ProductTelemetry {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn policy(&self) -> Result<TelemetryPolicy> {
        Ok(TelemetryPolicy {
            dictionary_version: DICTIONARY_VERSION.into(),
            collection_policy_version: COLLECTION_POLICY_VERSION.into(),
            optional_enabled: self.optional_enabled()?,
            consent_version: self
                .read_setting(CONSENT_KEY)?
                .unwrap_or_else(|| COLLECTION_POLICY_VERSION.into()),
        })
    }

    pub fn set_optional_enabled(&self, enabled: bool) -> Result<TelemetryPolicy> {
        self.write_setting(ENABLED_KEY, if enabled { "true" } else { "false" })?;
        self.write_setting(CONSENT_KEY, COLLECTION_POLICY_VERSION)?;
        if !enabled {
            self.delete_optional()?;
        }
        self.policy()
    }

    /// Sign-out and account deletion: drop optional analytics. Keep the
    /// install id and essential recovery events until retention expires.
    pub fn on_sign_out(&self) -> Result<()> {
        self.delete_optional()?;
        Ok(())
    }

    pub fn record(&self, name: &str, properties: Value, once: bool) -> Result<Option<String>> {
        let spec =
            dictionary::spec(name).ok_or_else(|| anyhow!("unknown telemetry event {name}"))?;
        if spec.sensitivity == dictionary::Sensitivity::Optional && !self.optional_enabled()? {
            return Ok(None);
        }
        if once
            && self
                .read_setting(&format!("{FIRST_PREFIX}{name}"))?
                .is_some()
        {
            return Ok(None);
        }
        if let Some(field) = privacy::forbidden_field(&properties) {
            return Err(anyhow!(
                "telemetry payload refused: forbidden field {field}"
            ));
        }
        let properties = self.filter_properties(spec, properties)?;
        let event_id = format!("pte_{}", Uuid::new_v4().simple());
        let at = Utc::now().to_rfc3339();
        let payload = serde_json::to_string(&properties)?;
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO product_telemetry_events(event_id, name, at, sensitivity, properties_json)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    event_id,
                    spec.name,
                    at,
                    sensitivity_name(spec.sensitivity),
                    payload
                ],
            )?;
            Ok(())
        })?;
        if once {
            self.write_setting(&format!("{FIRST_PREFIX}{name}"), &at)?;
        }
        self.prune()?;
        Ok(Some(event_id))
    }

    pub fn recent(&self, limit: i64) -> Result<Vec<TelemetryEventRecord>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT event_id, name, at, sensitivity, properties_json
                 FROM product_telemetry_events ORDER BY at DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map([limit.max(1).min(200)], |row| {
                let raw: String = row.get(4)?;
                Ok(TelemetryEventRecord {
                    event_id: row.get(0)?,
                    name: row.get(1)?,
                    at: row.get(2)?,
                    sensitivity: row.get(3)?,
                    properties: serde_json::from_str(&raw).unwrap_or(Value::Null),
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    fn optional_enabled(&self) -> Result<bool> {
        Ok(self
            .read_setting(ENABLED_KEY)?
            .map(|value| value != "false")
            .unwrap_or(true))
    }

    fn install_id(&self) -> Result<String> {
        if let Some(existing) = self.read_setting(INSTALL_ID_KEY)? {
            return Ok(existing);
        }
        let id = format!("ins_{}", Uuid::new_v4().simple());
        self.write_setting(INSTALL_ID_KEY, &id)?;
        Ok(id)
    }

    fn envelope(&self) -> Result<Map<String, Value>> {
        let mut map = Map::new();
        map.insert("schema_version".into(), json!(1));
        map.insert("app_version".into(), json!(env!("CARGO_PKG_VERSION")));
        map.insert(
            "release_channel".into(),
            json!(std::env::var("SYNTH_RELEASE_CHANNEL").unwrap_or_else(|_| "dev".into())),
        );
        map.insert("platform".into(), json!(std::env::consts::OS));
        map.insert("architecture".into(), json!(std::env::consts::ARCH));
        map.insert("install_id".into(), json!(self.install_id()?));
        map.insert(
            "collection_policy_version".into(),
            json!(COLLECTION_POLICY_VERSION),
        );
        Ok(map)
    }

    fn filter_properties(&self, spec: &EventSpec, properties: Value) -> Result<Value> {
        let Value::Object(incoming) = properties else {
            return Err(anyhow!("telemetry properties must be an object"));
        };
        let mut out = self.envelope()?;
        for (key, value) in incoming {
            if !dictionary::allowed_property(spec, &key) {
                return Err(anyhow!(
                    "telemetry payload refused: property {key} is not allowlisted"
                ));
            }
            if matches!(value, Value::Object(_) | Value::Array(_)) {
                return Err(anyhow!(
                    "telemetry payload refused: nested values are not allowed"
                ));
            }
            validate_property(&key, &value)?;
            out.insert(key, value);
        }
        Ok(Value::Object(out))
    }

    fn prune(&self) -> Result<()> {
        let optional_floor =
            (Utc::now() - Duration::days(dictionary::RETENTION_DAYS_OPTIONAL)).to_rfc3339();
        let essential_floor =
            (Utc::now() - Duration::days(dictionary::RETENTION_DAYS_ESSENTIAL)).to_rfc3339();
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

    fn delete_optional(&self) -> Result<()> {
        self.db.with_conn(|conn| {
            conn.execute(
                "DELETE FROM product_telemetry_events WHERE sensitivity = 'optional'",
                [],
            )?;
            Ok(())
        })
    }

    fn read_setting(&self, key: &str) -> Result<Option<String>> {
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

    fn write_setting(&self, key: &str, value: &str) -> Result<()> {
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

fn validate_property(key: &str, value: &Value) -> Result<()> {
    match key {
        "outcome" => match value.as_str() {
            Some("success" | "failure" | "cancelled") => Ok(()),
            _ => Err(anyhow!(
                "telemetry payload refused: outcome must be success, failure, or cancelled"
            )),
        },
        "duration_ms" if value.as_u64().is_none() => Err(anyhow!(
            "telemetry payload refused: duration_ms must be a non-negative integer"
        )),
        "workflow_family" | "error_class" if value.as_str().is_none() => Err(anyhow!(
            "telemetry payload refused: property {key} must be a string"
        )),
        _ => Ok(()),
    }
}

fn sensitivity_name(value: dictionary::Sensitivity) -> &'static str {
    match value {
        dictionary::Sensitivity::Optional => "optional",
        dictionary::Sensitivity::Essential => "essential",
    }
}

#[tauri::command]
#[specta::specta]
pub fn product_telemetry_get_policy(
    state: State<'_, Arc<ProductTelemetry>>,
) -> Result<TelemetryPolicy, AppError> {
    state.policy().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn product_telemetry_set_opt_out(
    state: State<'_, Arc<ProductTelemetry>>,
    opt_out: bool,
) -> Result<TelemetryPolicy, AppError> {
    state.set_optional_enabled(!opt_out).map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    fn open() -> (tempfile::TempDir, ProductTelemetry) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        (dir, ProductTelemetry::new(storage.database().clone()))
    }

    #[test]
    fn dictionary_covers_the_v06_funnel() {
        for name in [
            "download_initiated",
            "download_served",
            "app_first_launch",
            "signup_completed",
            "signin_completed",
            "signout_completed",
            "workflow_started",
            "workflow_terminal",
            "local_activation_completed",
            "hosted_activation_completed",
            "artifact_created",
            "recipe_saved",
            "report_created",
            "report_published",
            "recovery_attempted",
            "recovery_succeeded",
            "first_workspace_opened",
            "first_run_succeeded",
            "first_experiment_visual",
            "first_report_shared",
            "hosted_job_started",
            "hosted_job_completed",
            "hosted_job_failed",
        ] {
            assert!(spec(name).is_some(), "{name}");
        }
        assert!(EVENTS.iter().all(|event| !event.purpose.is_empty()));
        assert!(EVENTS.iter().all(|event| !event.owner.is_empty()));
    }

    #[test]
    fn envelope_uses_the_shared_v1_wire_contract() {
        let (_dir, telemetry) = open();
        telemetry
            .record(
                "local_activation_completed",
                json!({"outcome": "success"}),
                false,
            )
            .unwrap();
        let event = telemetry.recent(1).unwrap().pop().unwrap();
        assert_eq!(event.properties["schema_version"], 1);
        assert_eq!(
            event.properties["collection_policy_version"],
            COLLECTION_POLICY_VERSION
        );
        assert!(event.properties["install_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("ins_")));
    }

    #[test]
    fn optional_events_stop_after_opt_out_and_essential_continue() {
        let (_dir, telemetry) = open();
        telemetry
            .record("app_first_launch", json!({"platform": "macos"}), true)
            .unwrap();
        telemetry.set_optional_enabled(false).unwrap();
        assert!(telemetry
            .record("signin_completed", json!({"outcome": "success"}), false)
            .unwrap()
            .is_none());
        assert!(telemetry
            .record(
                "recovery_attempted",
                json!({"error_class": "offline"}),
                false
            )
            .unwrap()
            .is_some());
        let names: Vec<_> = telemetry
            .recent(20)
            .unwrap()
            .into_iter()
            .map(|event| event.name)
            .collect();
        assert!(!names.contains(&"app_first_launch".to_string()));
        assert!(names.contains(&"recovery_attempted".to_string()));
    }

    #[test]
    fn unknown_events_and_secret_fields_are_refused() {
        let (_dir, telemetry) = open();
        assert!(telemetry
            .record("not_a_real_event", json!({}), false)
            .unwrap_err()
            .to_string()
            .contains("unknown"));
        assert!(telemetry
            .record("signin_completed", json!({"prompt": "do research"}), false)
            .unwrap_err()
            .to_string()
            .contains("forbidden"));
        assert!(telemetry
            .record("signin_completed", json!({"filename": "notes.md"}), false)
            .unwrap_err()
            .to_string()
            .contains("forbidden"));
        assert!(telemetry
            .record(
                "signin_completed",
                json!({"outcome": "success", "api_key": "sk-secret"}),
                false
            )
            .unwrap_err()
            .to_string()
            .contains("forbidden"));
        assert!(telemetry.recent(10).unwrap().is_empty());
    }

    #[test]
    fn event_values_are_closed_over_the_shared_contract() {
        let (_dir, telemetry) = open();
        assert!(telemetry
            .record("workflow_terminal", json!({"outcome": "ok"}), false)
            .unwrap_err()
            .to_string()
            .contains("outcome"));
        assert!(telemetry
            .record("workflow_terminal", json!({"duration_ms": -1}), false)
            .unwrap_err()
            .to_string()
            .contains("duration_ms"));
        assert!(telemetry
            .record("workflow_terminal", json!({"outcome": "success"}), false)
            .unwrap()
            .is_some());
    }

    #[test]
    fn stored_events_never_contain_prompts_or_keys() {
        let (_dir, telemetry) = open();
        telemetry
            .record(
                "hosted_job_failed",
                json!({"workflow_family": "eval", "outcome": "failure", "error_class": "quota"}),
                false,
            )
            .unwrap();
        let dump = serde_json::to_string(&telemetry.recent(5).unwrap()).unwrap();
        assert!(dump.contains("hosted_job_failed"));
        assert!(!dump.contains("sk-"));
        assert!(!dump.contains("prompt"));
        assert!(!dump.contains("/Users/"));
    }

    #[test]
    fn mark_once_does_not_duplicate_first_launch() {
        let (_dir, telemetry) = open();
        assert!(telemetry
            .record("app_first_launch", json!({}), true)
            .unwrap()
            .is_some());
        assert!(telemetry
            .record("app_first_launch", json!({}), true)
            .unwrap()
            .is_none());
        assert_eq!(telemetry.recent(10).unwrap().len(), 1);
    }

    #[test]
    fn sign_out_deletes_optional_events_and_keeps_essential() {
        let (_dir, telemetry) = open();
        telemetry
            .record("signin_completed", json!({"outcome": "success"}), false)
            .unwrap();
        telemetry
            .record("recovery_attempted", json!({"error_class": "auth"}), false)
            .unwrap();
        telemetry.on_sign_out().unwrap();
        let names: Vec<_> = telemetry
            .recent(20)
            .unwrap()
            .into_iter()
            .map(|event| event.name)
            .collect();
        assert!(!names.contains(&"signin_completed".to_string()));
        assert!(names.contains(&"recovery_attempted".to_string()));
    }
}
