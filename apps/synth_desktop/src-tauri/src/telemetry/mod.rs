//! Privacy-safe product telemetry.
//!
//! Modular pipeline: feature code emits against the embedded contract
//! ([`contract`]), a pure gate ([`policy`]) decides recording, the SQLite
//! outbox ([`store`]) is both the local record and the sync spool, and a
//! background flusher ([`flush`]) ships consented, sync-eligible events to
//! the profile-routed backend ([`sink_http`]). Consent ([`consent`]) is
//! honest three-state bookkeeping — unset is not a choice — and essential
//! events never leave the device regardless of it.

pub mod consent;
pub mod contract;
pub mod flush;
mod policy;
mod privacy;
pub mod sink_http;
pub mod store;

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::sync::{Arc, OnceLock};
use tauri::State;

use crate::error::AppError;
use crate::storage::Database;

use consent::{ConsentChoice, ConsentState};
use store::TelemetryStore;

const ENABLED_KEY: &str = "telemetry.product.enabled";

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
    store: TelemetryStore,
}

#[derive(Clone, Debug, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryPolicy {
    pub dictionary_version: String,
    pub collection_policy_version: String,
    pub optional_enabled: bool,
    pub consent: ConsentState,
    /// The consent ask is due: never answered, or answered under an older
    /// collection policy.
    pub needs_ask: bool,
    /// Sync-eligible events may currently leave the device.
    pub sync_allowed: bool,
    pub last_sync_at: Option<String>,
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
        Self {
            store: TelemetryStore::new(db),
        }
    }

    pub fn store(&self) -> &TelemetryStore {
        &self.store
    }

    pub fn policy(&self) -> Result<TelemetryPolicy> {
        let consent = consent::state(&self.store)?;
        Ok(TelemetryPolicy {
            dictionary_version: contract::dictionary_version().into(),
            collection_policy_version: contract::collection_policy_version().into(),
            optional_enabled: self.optional_enabled()?,
            needs_ask: consent::needs_ask(&consent),
            sync_allowed: consent::sync_allowed(&consent),
            last_sync_at: self.store.last_sync_at()?,
            consent,
        })
    }

    /// One decision covers recording and egress: granted turns optional
    /// analytics on and makes sync-eligible events shippable; declined turns
    /// optional analytics off and deletes what was queued.
    pub fn set_consent(&self, choice: ConsentChoice) -> Result<TelemetryPolicy> {
        consent::record_choice(&self.store, choice)?;
        let enabled = choice == ConsentChoice::Granted;
        self.store
            .write_setting(ENABLED_KEY, if enabled { "true" } else { "false" })?;
        if !enabled {
            self.store.delete_optional()?;
        }
        self.policy()
    }

    /// Legacy settings toggle: an explicit user action, so it records the
    /// same consent choice the first-run ask would.
    pub fn set_optional_enabled(&self, enabled: bool) -> Result<TelemetryPolicy> {
        self.set_consent(if enabled {
            ConsentChoice::Granted
        } else {
            ConsentChoice::Declined
        })
    }

    /// Sign-out and account deletion: drop optional analytics. Keep the
    /// install id and essential recovery events until retention expires.
    pub fn on_sign_out(&self) -> Result<()> {
        self.store.delete_optional()?;
        Ok(())
    }

    pub fn record(&self, name: &str, properties: Value, once: bool) -> Result<Option<String>> {
        let spec = contract::spec(name).ok_or_else(|| anyhow!("unknown telemetry event {name}"))?;
        if once && self.store.first_marked(name)? {
            return Ok(None);
        }
        let decision = policy::gate(spec, self.optional_enabled()?, self.envelope()?, properties)?;
        let policy::Decision::Record { properties } = decision else {
            return Ok(None);
        };
        let event_id = self
            .store
            .insert(&spec.name, spec.sensitivity, &properties)?;
        if once {
            self.store.mark_first(name)?;
        }
        Ok(Some(event_id))
    }

    pub fn recent(&self, limit: i64) -> Result<Vec<TelemetryEventRecord>> {
        Ok(self
            .store
            .recent(limit)?
            .into_iter()
            .map(
                |(event_id, name, at, sensitivity, properties)| TelemetryEventRecord {
                    event_id,
                    name,
                    at,
                    sensitivity,
                    properties,
                },
            )
            .collect())
    }

    fn optional_enabled(&self) -> Result<bool> {
        Ok(self
            .store
            .read_setting(ENABLED_KEY)?
            .map(|value| value != "false")
            .unwrap_or(true))
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
        map.insert("install_id".into(), json!(self.store.install_id()?));
        map.insert(
            "collection_policy_version".into(),
            json!(contract::collection_policy_version()),
        );
        Ok(map)
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

/// The first-run consent answer. `granted` enables optional analytics and
/// sync; `declined` disables optional analytics and deletes queued events.
#[tauri::command]
#[specta::specta]
pub fn product_telemetry_set_consent(
    state: State<'_, Arc<ProductTelemetry>>,
    granted: bool,
) -> Result<TelemetryPolicy, AppError> {
    state
        .set_consent(if granted {
            ConsentChoice::Granted
        } else {
            ConsentChoice::Declined
        })
        .map_err(AppError::from)
}

/// Transparency view: the most recent locally stored events, exactly as they
/// would sync. Display-safe by construction — the gate refused anything else.
#[tauri::command]
#[specta::specta]
pub fn product_telemetry_recent(
    state: State<'_, Arc<ProductTelemetry>>,
    limit: crate::contract::specta::OpaqueInteger<i64>,
) -> Result<Vec<TelemetryEventRecord>, AppError> {
    state.recent(limit.0).map_err(AppError::from)
}

/// Manual flush for Settings and QA. Reports the number of events shipped;
/// without current consent it ships nothing and reports zero.
#[tauri::command]
#[specta::specta]
pub async fn product_telemetry_flush_now(
    flusher: State<'_, Arc<flush::Flusher>>,
) -> Result<u32, AppError> {
    match flusher.flush_once().await.map_err(AppError::from)? {
        flush::FlushOutcome::Sent { events } => Ok(events as u32),
        _ => Ok(0),
    }
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
            assert!(contract::spec(name).is_some(), "{name}");
        }
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
            contract::collection_policy_version()
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

    #[test]
    fn policy_reports_unset_consent_honestly_and_choice_flips_it() {
        let (_dir, telemetry) = open();
        let before = telemetry.policy().unwrap();
        assert_eq!(before.consent, ConsentState::Unset);
        assert!(before.needs_ask);
        assert!(!before.sync_allowed);
        // Recording stays on by default, local-only until consent.
        assert!(before.optional_enabled);

        let granted = telemetry.set_consent(ConsentChoice::Granted).unwrap();
        assert!(!granted.needs_ask);
        assert!(granted.sync_allowed);
        assert!(granted.optional_enabled);

        let declined = telemetry.set_consent(ConsentChoice::Declined).unwrap();
        assert!(!declined.needs_ask);
        assert!(!declined.sync_allowed);
        assert!(!declined.optional_enabled);
    }
}
