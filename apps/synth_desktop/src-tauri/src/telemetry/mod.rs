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
            crate::platform::logging::report(
                "telemetry",
                "eprintln",
                format!("synth-desktop: product telemetry dropped {name}: {error}"),
            );
        }
    }
}

/// Record `name` at most once per install.
pub fn mark_once(name: &str, properties: Value) {
    if let Some(telemetry) = live() {
        if let Err(error) = telemetry.record(name, properties, true) {
            crate::platform::logging::report(
                "telemetry",
                "eprintln",
                format!("synth-desktop: product telemetry dropped {name}: {error}"),
            );
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

