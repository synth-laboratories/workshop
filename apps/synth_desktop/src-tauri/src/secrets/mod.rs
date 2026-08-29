//! Local-first secrets vault and provider proxy for Workshop.
//!
//! Agents may request, import, and use credentials, but they receive only
//! masked metadata and bounded capabilities. Plaintext crosses only the
//! trusted vault-to-proxy boundary.

mod audit;
mod backend;
pub(crate) mod capability;
mod fingerprint;
mod importer;
pub mod lease;
mod locator;
mod path_gate;
mod providers;
mod proxy;
mod vault;

use anyhow::{anyhow, bail, Result};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::error::AppError;
use crate::storage::Database;
use tauri::{Manager, State};

use audit::SecretAuditEvent;
use backend::{default_backend, SecretBackend, SecretBytes};
use capability::{CapabilityStore, CapabilitySummary, IssuedCapability, ProviderUsePolicy};
use fingerprint::RedactionIndex;
use importer::{AfterImportAction, ImportPreview, PendingImport};
use proxy::{ProviderProxy, ProxyState, WorkloadEnv};
use vault::SecretSummary;

pub use capability::CapabilityLedger;
pub use capability::ProviderUsePolicy as SecretsUsePolicy;
pub(crate) use capability::{ProviderUsageCapability, ProviderUsageReceipt};
#[allow(unused_imports)]
pub use lease::{
    CredentialLease, CredentialReadinessReceipt, CredentialSourceDescriptor, OptimizerRuntimeLease,
    CONTRACT as CREDENTIAL_CONTRACT, CREDENTIAL_MODE_WORKSHOP_PROXY,
};
pub use locator::{
    CredentialBindingSummary, CredentialLocatorKind, CredentialLocatorState,
    CredentialLocatorSummary,
};
pub use path_gate::WorkspaceRootSummary;
pub use proxy::API_KEY_SENTINEL;

static LIVE: OnceLock<Arc<SecretsService>> = OnceLock::new();

pub fn install_live(service: Arc<SecretsService>) {
    let _ = LIVE.set(service);
}

pub fn live() -> Option<Arc<SecretsService>> {
    LIVE.get().cloned()
}


/// Scrub registered secret values out of text that is about to be persisted.
pub fn redact_live(text: &str) -> String {
    match live() {
        Some(secrets) => secrets.redact(text),
        None => text.to_owned(),
    }
}

/// Revoke every capability issued for `run_id`. Best-effort: a missing live
/// vault is a no-op so tests without the broker still compile and run.
pub fn revoke_run_best_effort(run_id: &str) {
    if let Some(secrets) = live() {
        if let Err(error) = secrets.revoke_run(run_id) {
            crate::platform::logging::report(
                "secrets",
                "eprintln",
                format!("synth-desktop: revoke secrets for {run_id}: {error:#}"),
            );
        }
    }
}

/// Revoke a run and seal the public credential-chain receipt. Terminal owners
/// use this path so the durable projection cannot continue to claim an active
/// capability after the authoritative capability ledger revoked it.
pub fn seal_run_best_effort(run_id: &str) {
    if let Some(secrets) = live() {
        if let Err(error) = secrets.seal_run_chain(run_id) {
            crate::platform::logging::report(
                "secrets",
                "eprintln",
                format!("synth-desktop: seal secrets for {run_id}: {error:#}"),
            );
        }
    }
}

/// Drops by revoking the run's provider capabilities.
pub struct RevokeRunOnDrop(pub String);

impl Drop for RevokeRunOnDrop {
    fn drop(&mut self) {
        seal_run_best_effort(&self.0);
    }
}

/// Failure-scoped capability guard for run preparation. Armed at issue time so
/// any error path between issuing a provider capability and handing ownership
/// to a durable run record (or the run worker) revokes it; `disarm` transfers
/// ownership and makes the drop a no-op.
pub struct RevokeRunOnFailure {
    run_id: String,
    armed: bool,
}

impl RevokeRunOnFailure {
    pub fn new(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            armed: true,
        }
    }

    /// Ownership of the capability has been transferred to a durable owner
    /// (a prepared run record, or the spawned run worker's own guard).
    pub fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for RevokeRunOnFailure {
    fn drop(&mut self) {
        if self.armed {
            seal_run_best_effort(&self.run_id);
        }
    }
}

#[derive(Clone)]
pub struct SecretsService {
    pub(crate) db: Arc<Database>,
    pub(crate) backend: Arc<dyn SecretBackend>,
    pub(crate) env_sources: Arc<lease::EnvSourceStore>,
    capabilities: Arc<CapabilityStore>,
    pending_imports: Arc<Mutex<HashMap<String, PendingImport>>>,
    pending_grants: Arc<Mutex<HashMap<String, PendingGrant>>>,
    pending_source_consents: Arc<Mutex<HashSet<String>>>,
    pub(crate) redaction: Arc<RedactionIndex>,
    proxy: Arc<Mutex<Option<Arc<ProviderProxy>>>>,
    pub(crate) readiness: Arc<Mutex<HashMap<String, lease::CredentialReadinessReceipt>>>,
    pub(crate) chains: Arc<Mutex<HashMap<String, serde_json::Value>>>,
}

#[derive(Clone, Debug)]
struct PendingGrant {
    secret_id: String,
    run_id: String,
    recipe_id: String,
    policy: ProviderUsePolicy,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UseRequestResult {
    pub status: String,
    pub request_id: Option<String>,
    pub capability_id: Option<String>,
    pub proxy_origin: Option<String>,
    pub handle: Option<String>,
    pub summary: Option<CapabilitySummary>,
    #[specta(type = specta_typescript::Unknown)]
    pub provider_routes: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SecretsProxyStatus {
    pub origin: Option<String>,
    pub running: bool,
}

fn capability_provider_routes(
    provider: &str,
    origin: &str,
    handle: &str,
) -> Option<serde_json::Value> {
    if !matches!(provider, "openai" | "openrouter" | "tinker" | "groq") {
        return None;
    }
    let container_origin = proxy::rewrite_origin_for_containers(origin);
    Some(serde_json::json!({
        "openai": proxy::capability_chat_completions_url(&container_origin, handle, provider),
        "openai_base": proxy::capability_base_url(&container_origin, handle, provider),
        "auth": "capability_path",
        "api_key_sentinel": API_KEY_SENTINEL,
        "container_host": proxy::container_proxy_host(),
        "extra_hosts": ["host.docker.internal:host-gateway"],
    }))
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingGrantSummary {
    pub request_id: String,
    pub secret_id: String,
    pub alias: Option<String>,
    pub provider: Option<String>,
    pub run_id: String,
    pub recipe_id: String,
    pub models: Vec<String>,
    pub max_calls: u32,
    pub max_cost_usd: f64,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SecretsInbox {
    pub imports: Vec<ImportPreview>,
    pub grants: Vec<PendingGrantSummary>,
    pub proxy: SecretsProxyStatus,
}

impl SecretsService {
    pub fn new(db: Arc<Database>) -> Self {
        Self::with_backend(db, default_backend())
    }

    pub fn with_backend(db: Arc<Database>, backend: Arc<dyn SecretBackend>) -> Self {
        Self {
            db,
            backend,
            env_sources: Arc::new(lease::EnvSourceStore::new()),
            capabilities: Arc::new(CapabilityStore::new()),
            pending_imports: Arc::new(Mutex::new(HashMap::new())),
            pending_grants: Arc::new(Mutex::new(HashMap::new())),
            pending_source_consents: Arc::new(Mutex::new(HashSet::new())),
            redaction: Arc::new(RedactionIndex::new()),
            proxy: Arc::new(Mutex::new(None)),
            readiness: Arc::new(Mutex::new(HashMap::new())),
            chains: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn start_proxy(&self) -> Result<String> {
        let mut slot = self.proxy.lock().expect("provider proxy");
        if let Some(proxy) = slot.as_ref() {
            return Ok(proxy.origin().to_owned());
        }
        let state = Arc::new(ProxyState {
            db: self.db.clone(),
            backend: self.backend.clone(),
            env_sources: self.env_sources.clone(),
            capabilities: self.capabilities.clone(),
        });
        let proxy = Arc::new(ProviderProxy::start(state)?);
        let origin = proxy.origin().to_owned();
        *slot = Some(proxy);
        Ok(origin)
    }

    pub fn proxy_origin(&self) -> Option<String> {
        self.proxy
            .lock()
            .expect("provider proxy")
            .as_ref()
            .map(|proxy| proxy.origin().to_owned())
    }

    pub fn proxy_socket_path(&self) -> Option<PathBuf> {
        self.proxy
            .lock()
            .expect("provider proxy")
            .as_ref()
            .and_then(|proxy| proxy.socket_path().map(PathBuf::from))
    }

    fn allowed_workspace_paths(&self) -> Vec<PathBuf> {
        crate::synth_config::allowed_workspace_roots()
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .collect()
    }

    pub fn workspace_roots(&self) -> Vec<WorkspaceRootSummary> {
        path_gate::summarize_workspace_roots(&self.allowed_workspace_paths())
    }

    pub fn locators(&self, include_external: bool) -> Result<Vec<CredentialLocatorSummary>> {
        let loaded = self.env_sources.keys();
        self.db
            .with_conn(|conn| locator::list(conn, &loaded, include_external))
    }

    pub fn bindings(&self) -> Result<Vec<CredentialBindingSummary>> {
        let loaded = self.env_sources.keys();
        self.db.with_conn(|conn| locator::bindings(conn, &loaded))
    }

    fn rewrite_locator_export(&self) {
        let result = self
            .locators(true)
            .and_then(|rows| crate::synth_config::rewrite_credential_locator_export(&rows));
        if let Err(error) = result {
            crate::platform::logging::report(
                "secrets",
                "credential_locator_export_failed",
                format!(
                    "credential locator SQLite commit succeeded but TOML export failed: {error:#}"
                ),
            );
        }
    }

    fn validate_locator_identity(provider: &str, variable: &str) -> Result<()> {
        if provider.trim().is_empty() || !path_gate::is_valid_env_variable(variable.trim()) {
            return Err(AppError::invalid_argument(
                "provider and a valid environment variable are required",
            )
            .into());
        }
        Ok(())
    }

    pub fn remember_workspace_locator_pending(
        &self,
        workspace_root_ref: &str,
        relative_path: &str,
        provider: &str,
        variable: &str,
        label: &str,
    ) -> Result<CredentialLocatorSummary> {
        Self::validate_locator_identity(provider, variable)?;
        let gated = path_gate::gate_workspace_file(
            &self.allowed_workspace_paths(),
            workspace_root_ref,
            relative_path,
        )?;
        let id = self.db.transaction(|conn| {
            locator::insert_workspace_pending(conn, &gated, provider, variable, label)
                .map(|record| record.id)
        })?;
        self.rewrite_locator_export();
        self.locators(false)?
            .into_iter()
            .find(|row| row.id == id)
            .ok_or_else(|| anyhow!("credential locator missing after remember"))
    }

    pub fn remember_external_locator(
        &self,
        picker_path: &Path,
        provider: &str,
        variable: &str,
        label: &str,
    ) -> Result<CredentialLocatorSummary> {
        Self::validate_locator_identity(provider, variable)?;
        let id = self.db.transaction(|conn| {
            locator::insert_external_observed(conn, picker_path, provider, variable, label)
                .map(|record| record.id)
        })?;
        self.rewrite_locator_export();
        self.locators(true)?
            .into_iter()
            .find(|row| row.id == id)
            .ok_or_else(|| anyhow!("credential locator missing after remember"))
    }

    pub fn settle_remembered_locator(&self, locator_id: &str) -> Result<()> {
        self.db.transaction(|conn| {
            let record = locator::get(conn, locator_id)?
                .ok_or_else(|| anyhow!("credential locator {locator_id} was not found"))?;
            if record.state == CredentialLocatorState::ApprovalPending {
                locator::transition(conn, locator_id, CredentialLocatorState::Observed)?;
            }
            Ok(())
        })?;
        self.rewrite_locator_export();
        Ok(())
    }

    /// Returns true when this was an already-remembered locator that must be
    /// restored to Observed if the Register card is rejected.
    pub fn prepare_register_approval(&self, locator_id: &str) -> Result<bool> {
        self.db.transaction(|conn| {
            let record = locator::get(conn, locator_id)?
                .ok_or_else(|| anyhow!("credential locator {locator_id} was not found"))?;
            match record.state {
                CredentialLocatorState::ApprovalPending => Ok(false),
                CredentialLocatorState::Observed => {
                    let pending: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM credential_locators
                         WHERE state IN ('proposed','approval_pending')",
                        [],
                        |row| row.get(0),
                    )?;
                    if pending >= locator::MAX_PENDING_LOCATORS {
                        return Err(lease::CredentialError::new(
                            lease::CREDENTIAL_LOCATOR_LIMIT,
                            "locator",
                            false,
                            "credential locator pending-consent limit reached",
                        )
                        .anyhow());
                    }
                    locator::transition(conn, locator_id, CredentialLocatorState::ApprovalPending)?;
                    Ok(true)
                }
                CredentialLocatorState::Missing => Err(lease::CredentialError::new(
                    lease::CREDENTIAL_LOCATOR_NOT_REGULAR_FILE,
                    "locator",
                    false,
                    "missing credential location cannot be registered",
                )
                .anyhow()),
                CredentialLocatorState::WorkspaceAuthorityRevoked => {
                    Err(lease::CredentialError::new(
                        lease::CREDENTIAL_LOCATOR_UNAPPROVED_WORKSPACE,
                        "locator",
                        false,
                        "workspace authority for this credential location was revoked",
                    )
                    .anyhow())
                }
                _ => Err(anyhow!(
                    "credential locator is not available for registration"
                )),
            }
        })
    }

    pub fn deny_pending_locator(&self, locator_id: &str) -> Result<()> {
        self.db
            .transaction(|conn| locator::remove(conn, locator_id).map(|_| ()))?;
        self.rewrite_locator_export();
        Ok(())
    }

    pub fn begin_source_consent(&self, provider: &str, variable: &str) -> Result<()> {
        let key = format!(
            "{}:{}",
            provider.trim().to_ascii_lowercase(),
            variable.trim()
        );
        let mut pending = self
            .pending_source_consents
            .lock()
            .expect("pending source consents");
        if pending.contains(&key) {
            return Err(lease::CredentialError::new(
                lease::CREDENTIAL_SOURCE_CONSENT_PENDING,
                "locator",
                true,
                "a credential source decision is already pending for this provider",
            )
            .anyhow());
        }
        if pending.len() >= locator::MAX_PENDING_LOCATORS as usize {
            return Err(lease::CredentialError::new(
                lease::CREDENTIAL_LOCATOR_LIMIT,
                "locator",
                false,
                "credential source pending-consent limit reached",
            )
            .anyhow());
        }
        pending.insert(key);
        Ok(())
    }

    pub fn end_source_consent(&self, provider: &str, variable: &str) {
        let key = format!(
            "{}:{}",
            provider.trim().to_ascii_lowercase(),
            variable.trim()
        );
        self.pending_source_consents
            .lock()
            .expect("pending source consents")
            .remove(&key);
    }

    pub fn register_locator(&self, locator_id: &str) -> Result<CredentialLocatorSummary> {
        let record = self
            .db
            .with_conn(|conn| locator::get(conn, locator_id))?
            .ok_or_else(|| anyhow!("credential locator {locator_id} was not found"))?;
        match record.state {
            CredentialLocatorState::Observed | CredentialLocatorState::ApprovalPending => {}
            CredentialLocatorState::WorkspaceAuthorityRevoked => {
                return Err(lease::CredentialError::new(
                    lease::CREDENTIAL_LOCATOR_UNAPPROVED_WORKSPACE,
                    "locator",
                    false,
                    "This folder is allowed again. Forget and remember to restore.",
                )
                .anyhow());
            }
            CredentialLocatorState::Missing => {
                return Err(lease::CredentialError::new(
                    lease::CREDENTIAL_LOCATOR_NOT_REGULAR_FILE,
                    "locator",
                    false,
                    "missing credential location cannot be registered",
                )
                .anyhow());
            }
            _ => {
                return Err(anyhow!(
                    "credential locator is not available for registration"
                ))
            }
        }
        let path = match locator::resolve_path(&record, &self.allowed_workspace_paths()) {
            Ok(path) => path,
            Err(error) => {
                let revoked = error
                    .chain()
                    .filter_map(|cause| cause.downcast_ref::<lease::CredentialError>())
                    .any(|failure| failure.code == lease::CREDENTIAL_LOCATOR_UNAPPROVED_WORKSPACE);
                self.db.with_conn(|conn| {
                    locator::set_observation_state(
                        conn,
                        &record.id,
                        if revoked {
                            CredentialLocatorState::WorkspaceAuthorityRevoked
                        } else {
                            CredentialLocatorState::Missing
                        },
                    )
                })?;
                self.rewrite_locator_export();
                return Err(error);
            }
        };
        let value = lease::read_env_file_value(&path, &record.variable).ok_or_else(|| {
            lease::CredentialError::new(
                lease::CREDENTIAL_VALUE_MISSING,
                "locator",
                false,
                format!("{} is missing or empty", record.variable),
            )
            .anyhow()
        })?;
        let bytes = SecretBytes::from_utf8(&value);
        let digest = fingerprint::fingerprint(&bytes);
        let (source_id, backend_ref, displaced) = self.db.transaction(|conn| {
            let displaced = locator::preferred_source(conn, &record.provider, &record.variable)?;
            let source_id = lease::upsert_env_source_descriptor(
                conn,
                &record.provider,
                &record.variable,
                &path,
                Some(&record.id),
                Some(&digest),
                true,
            )?;
            locator::mark_preferred(conn, &source_id, &record.provider, &record.variable)?;
            locator::set_observation_state(conn, &record.id, CredentialLocatorState::Observed)?;
            let backend_ref: String = conn.query_row(
                "SELECT backend_ref FROM secret_refs WHERE id=?1",
                [&source_id],
                |row| row.get(0),
            )?;
            Ok((source_id, backend_ref, displaced))
        })?;

        if let Some((old_source_id, old_locator_id)) = displaced {
            if old_source_id != source_id {
                if let Some(old) = self
                    .db
                    .with_conn(|conn| vault::record(conn, &old_source_id))?
                {
                    self.env_sources.remove(&old.backend_ref);
                }
                self.capabilities.revoke_secret(&old_source_id);
                let _ = self.db.with_conn(|conn| {
                    let old_locator = locator::get(conn, &old_locator_id)?;
                    if old_locator.is_some_and(|old| {
                        old.state == CredentialLocatorState::Observed
                            && old.kind != CredentialLocatorKind::InstanceEnvFile
                    }) {
                        let _ = locator::transition(
                            conn,
                            &old_locator_id,
                            CredentialLocatorState::Superseded,
                        );
                    }
                    Ok(())
                });
            }
        }
        self.env_sources.put(&backend_ref, &bytes);
        self.redaction.register(&bytes);
        self.rewrite_locator_export();
        self.locators(true)?
            .into_iter()
            .find(|row| row.id == locator_id)
            .ok_or_else(|| anyhow!("credential locator missing after registration"))
    }

    pub fn forget_locator(&self, locator_id: &str) -> Result<()> {
        let record = self
            .db
            .with_conn(|conn| locator::get(conn, locator_id))?
            .ok_or_else(|| anyhow!("credential locator {locator_id} was not found"))?;
        let sources = self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT id,backend_ref,preferred FROM secret_refs WHERE locator_id=?1")?;
            let rows = stmt.query_map([locator_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? != 0,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into)
        })?;
        let was_preferred = sources.iter().any(|(_, _, preferred)| *preferred);
        for (source_id, backend_ref, preferred) in &sources {
            if *preferred {
                self.env_sources.remove(backend_ref);
            }
            self.capabilities.revoke_secret(source_id);
        }
        let fallback = self.db.transaction(|conn| {
            locator::remove(conn, locator_id)?;
            if was_preferred {
                if let Some(source_id) =
                    locator::preferred_instance_source(conn, &record.provider, &record.variable)?
                {
                    locator::mark_preferred(conn, &source_id, &record.provider, &record.variable)?;
                    let locator_id = conn.query_row(
                        "SELECT locator_id FROM secret_refs WHERE id=?1",
                        [&source_id],
                        |row| row.get::<_, String>(0),
                    )?;
                    return Ok(Some(locator_id));
                }
            }
            Ok(None)
        })?;
        if let Some(fallback) = fallback {
            let _ = self.load_registered_locator(&fallback, true);
        }
        self.rewrite_locator_export();
        Ok(())
    }

    pub fn remove_locator_source(&self, locator_id: &str) -> Result<()> {
        let record = self
            .db
            .with_conn(|conn| locator::get(conn, locator_id))?
            .ok_or_else(|| anyhow!("credential locator {locator_id} was not found"))?;
        let (sources, fallback) = self.db.transaction(|conn| {
            let sources = {
                let mut stmt = conn.prepare(
                    "SELECT id,backend_ref,preferred FROM secret_refs WHERE locator_id=?1",
                )?;
                let rows = stmt.query_map([locator_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)? != 0,
                    ))
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            let was_preferred = sources.iter().any(|(_, _, preferred)| *preferred);
            conn.execute("DELETE FROM secret_refs WHERE locator_id=?1", [locator_id])?;
            locator::set_observation_state(conn, locator_id, CredentialLocatorState::Observed)?;
            let fallback = if was_preferred {
                if let Some(source_id) =
                    locator::preferred_instance_source(conn, &record.provider, &record.variable)?
                {
                    locator::mark_preferred(conn, &source_id, &record.provider, &record.variable)?;
                    Some(conn.query_row(
                        "SELECT locator_id FROM secret_refs WHERE id=?1",
                        [&source_id],
                        |row| row.get::<_, String>(0),
                    )?)
                } else {
                    None
                }
            } else {
                None
            };
            Ok((sources, fallback))
        })?;
        for (source_id, backend_ref, preferred) in sources {
            if preferred {
                self.env_sources.remove(&backend_ref);
            }
            self.capabilities.revoke_secret(&source_id);
        }
        if let Some(fallback) = fallback {
            let _ = self.load_registered_locator(&fallback, true);
        }
        self.rewrite_locator_export();
        Ok(())
    }

    pub fn source_for_locator(&self, locator_id: &str) -> Result<String> {
        let source = self
            .db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT id,preferred FROM secret_refs WHERE locator_id=?1
                     ORDER BY updated_at DESC LIMIT 1",
                    [locator_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0)),
                )
                .optional()
                .map_err(Into::into)
            })?
            .ok_or_else(|| {
                lease::CredentialError::new(
                    lease::CREDENTIAL_SOURCE_UNCONFIGURED,
                    "locator",
                    false,
                    "credential location has not been registered",
                )
                .anyhow()
            })?;
        if !source.1 {
            return Err(lease::CredentialError::new(
                lease::CREDENTIAL_VALUE_UNLOADED,
                "locator",
                false,
                "credential source is registered but is not the loaded preferred source",
            )
            .anyhow());
        }
        let source_id = source.0;
        let record = self
            .db
            .with_conn(|conn| vault::record(conn, &source_id))?
            .ok_or_else(|| anyhow!("credential source {source_id} was not found"))?;
        if !self.env_sources.contains(&record.backend_ref) {
            return Err(lease::CredentialError::new(
                lease::CREDENTIAL_VALUE_UNLOADED,
                "locator",
                false,
                "credential source is registered but not loaded",
            )
            .anyhow());
        }
        Ok(source_id)
    }

    pub fn redact(&self, text: &str) -> String {
        self.redaction.redact(text)
    }

    pub fn list(&self, provider: Option<&str>, scope: Option<&str>) -> Result<Vec<SecretSummary>> {
        self.db.with_conn(|conn| vault::list(conn, provider, scope))
    }

    pub fn create(
        &self,
        alias: &str,
        provider: &str,
        scope: &str,
        value: &str,
        actor: &str,
    ) -> Result<SecretSummary> {
        if value.trim().is_empty() {
            bail!("credential value is empty");
        }
        let bytes = SecretBytes::from_utf8(value.trim());
        let summary = self.db.transaction(|conn| {
            vault::create(
                conn,
                self.backend.as_ref(),
                alias,
                provider,
                scope,
                &bytes,
                actor,
            )
        })?;
        self.redaction.register(&bytes);
        Ok(summary)
    }

    pub fn replace(&self, id: &str, value: &str, actor: &str) -> Result<SecretSummary> {
        if value.trim().is_empty() {
            bail!("credential value is empty");
        }
        let bytes = SecretBytes::from_utf8(value.trim());
        let summary = self
            .db
            .transaction(|conn| vault::replace(conn, self.backend.as_ref(), id, &bytes, actor))?;
        self.redaction.register(&bytes);
        Ok(summary)
    }

    pub fn delete(&self, id: &str, actor: &str) -> Result<()> {
        self.db
            .transaction(|conn| vault::delete(conn, self.backend.as_ref(), id, actor))
    }

    pub fn test(&self, id: &str, actor: &str) -> Result<SecretSummary> {
        let secret = self.db.with_conn(|conn| {
            vault::resolve_for_proxy(
                conn,
                self.backend.as_ref(),
                Some(self.env_sources.as_ref()),
                id,
            )
        })?;
        let valid = !secret.as_bytes().is_empty();
        let status = if valid { "valid" } else { "invalid" };
        drop(secret);
        self.db
            .with_conn(|conn| vault::mark_status(conn, id, status, actor))?;
        self.db
            .with_conn(|conn| vault::get(conn, id))?
            .ok_or_else(|| anyhow!("secret {id} missing after test"))
    }

    pub fn request_use(
        &self,
        secret_id: &str,
        run_id: &str,
        recipe_id: &str,
        policy: ProviderUsePolicy,
        actor: &str,
    ) -> Result<UseRequestResult> {
        if let Some(live) = self.capabilities.find_active(secret_id, run_id) {
            ensure_capability_covers(&live, &policy)?;
            return self.live_result(live);
        }
        let always = self
            .db
            .with_conn(|conn| vault::has_recipe_grant(conn, secret_id, recipe_id))?;
        if always {
            return self.grant_use(secret_id, run_id, recipe_id, policy, actor, false);
        }
        {
            let pending = self.pending_grants.lock().expect("pending grants");
            if let Some((request_id, pending_grant)) = pending
                .iter()
                .find(|(_, grant)| grant.secret_id == secret_id && grant.run_id == run_id)
            {
                ensure_policy_covers(&pending_grant.policy, &policy, None)?;
                return Ok(UseRequestResult {
                    status: "approval_required".into(),
                    request_id: Some(request_id.clone()),
                    capability_id: None,
                    proxy_origin: self.proxy_origin(),
                    handle: None,
                    summary: None,
                    provider_routes: None,
                });
            }
        }
        let request_id = format!("grant_{}", uuid::Uuid::new_v4().simple());
        self.pending_grants.lock().expect("pending grants").insert(
            request_id.clone(),
            PendingGrant {
                secret_id: secret_id.into(),
                run_id: run_id.into(),
                recipe_id: recipe_id.into(),
                policy,
            },
        );
        Ok(UseRequestResult {
            status: "approval_required".into(),
            request_id: Some(request_id),
            capability_id: None,
            proxy_origin: self.proxy_origin(),
            handle: None,
            summary: None,
            provider_routes: None,
        })
    }

    pub fn grant_use(
        &self,
        secret_id: &str,
        run_id: &str,
        recipe_id: &str,
        policy: ProviderUsePolicy,
        actor: &str,
        remember_recipe: bool,
    ) -> Result<UseRequestResult> {
        if let Some(live) = self.capabilities.find_active(secret_id, run_id) {
            ensure_capability_covers(&live, &policy)?;
            return self.live_result(live);
        }
        let _ = self.start_proxy();
        let record = self
            .db
            .with_conn(|conn| vault::record(conn, secret_id))?
            .ok_or_else(|| anyhow!("secret {secret_id} was not found"))?;
        let issued = self.db.transaction(|conn| {
            if remember_recipe {
                vault::grant_recipe(conn, secret_id, recipe_id)?;
            }
            capability::issue(
                conn,
                &self.capabilities,
                secret_id,
                run_id,
                recipe_id,
                &record.provider,
                &policy,
                actor,
            )
        })?;
        Ok(self.issued_result(issued, &record.display_suffix))
    }

    pub fn grant_pending(
        &self,
        request_id: &str,
        actor: &str,
        remember_recipe: bool,
    ) -> Result<UseRequestResult> {
        let pending = self
            .pending_grants
            .lock()
            .expect("pending grants")
            .remove(request_id)
            .ok_or_else(|| anyhow!("grant request {request_id} is not pending"))?;
        self.grant_use(
            &pending.secret_id,
            &pending.run_id,
            &pending.recipe_id,
            pending.policy,
            actor,
            remember_recipe,
        )
    }

    pub fn deny_pending(&self, request_id: &str, actor: &str) -> Result<()> {
        let pending = self
            .pending_grants
            .lock()
            .expect("pending grants")
            .remove(request_id)
            .ok_or_else(|| anyhow!("grant request {request_id} is not pending"))?;
        self.db.with_conn(|conn| {
            let mut event = SecretAuditEvent::new("user", actor, "capability.deny", "denied");
            event.secret_id = Some(pending.secret_id);
            event.detail = Some(format!(
                "run={} recipe={}",
                pending.run_id, pending.recipe_id
            ));
            audit::append(conn, &event)
        })
    }

    fn issued_result(&self, mut issued: IssuedCapability, suffix: &str) -> UseRequestResult {
        issued.proxy_origin = self.proxy_origin().unwrap_or_default();
        issued.summary.display_suffix = Some(fingerprint::mask_suffix(suffix));
        let provider_routes = capability_provider_routes(
            &issued.summary.provider,
            &issued.proxy_origin,
            &issued.handle,
        );
        UseRequestResult {
            status: "granted".into(),
            request_id: None,
            capability_id: Some(issued.id),
            proxy_origin: Some(issued.proxy_origin),
            handle: Some(issued.handle),
            summary: Some(issued.summary),
            provider_routes,
        }
    }

    fn live_result(&self, live: capability::LiveCapability) -> Result<UseRequestResult> {
        let suffix = self
            .db
            .with_conn(|conn| vault::record(conn, &live.secret_id))?
            .map(|record| fingerprint::mask_suffix(&record.display_suffix));
        let origin = self.proxy_origin();
        let provider_routes = origin
            .as_deref()
            .and_then(|origin| capability_provider_routes(&live.provider, origin, &live.handle));
        Ok(UseRequestResult {
            status: "granted".into(),
            request_id: None,
            capability_id: Some(live.id.clone()),
            proxy_origin: origin,
            handle: Some(live.handle.clone()),
            summary: Some(capability::summary_from_live(&live, suffix)),
            provider_routes,
        })
    }

    pub fn inbox(&self) -> Result<SecretsInbox> {
        let imports = self
            .pending_imports
            .lock()
            .expect("pending imports")
            .values()
            .map(|pending| ImportPreview {
                request_id: pending.request_id.clone(),
                status: "approval_required".into(),
                source_path: pending.source_path.display().to_string(),
                candidates: pending
                    .entries
                    .iter()
                    .map(|entry| importer::MaskedImportCandidate {
                        variable: entry.variable.clone(),
                        provider: Some(entry.provider.clone()),
                        masked: fingerprint::mask_suffix(&entry.suffix),
                        classification: providers::classification_label(Some(&entry.provider))
                            .into(),
                        selected: true,
                    })
                    .collect(),
                source_remains_readable: pending.source_path.is_file(),
                warning: Some(
                    "Waiting for you to approve in Settings. The original file still contains plaintext until you replace or remove those entries.".into(),
                ),
                cleanup_diff: Some(importer::masked_cleanup_diff_from_entries(
                    &pending.entries,
                )),
            })
            .collect();
        let grants = {
            let pending = self.pending_grants.lock().expect("pending grants");
            let mut out = Vec::new();
            for (request_id, grant) in pending.iter() {
                let record = self
                    .db
                    .with_conn(|conn| vault::record(conn, &grant.secret_id))?;
                out.push(PendingGrantSummary {
                    request_id: request_id.clone(),
                    secret_id: grant.secret_id.clone(),
                    alias: record.as_ref().map(|record| record.alias.clone()),
                    provider: record.as_ref().map(|record| record.provider.clone()),
                    run_id: grant.run_id.clone(),
                    recipe_id: grant.recipe_id.clone(),
                    models: grant.policy.models.clone(),
                    max_calls: grant.policy.max_calls,
                    max_cost_usd: grant.policy.max_cost_usd,
                });
            }
            out
        };
        let origin = self.proxy_origin();
        Ok(SecretsInbox {
            imports,
            grants,
            proxy: SecretsProxyStatus {
                running: origin.is_some(),
                origin,
            },
        })
    }

    pub fn deny_import(&self, request_id: &str, actor: &str) -> Result<()> {
        let removed = self
            .pending_imports
            .lock()
            .expect("pending imports")
            .remove(request_id)
            .ok_or_else(|| anyhow!("import request {request_id} is not pending"))?;
        self.db.with_conn(|conn| {
            let mut event = SecretAuditEvent::new("user", actor, "secret.import.deny", "denied");
            event.detail = Some(removed.source_path.display().to_string());
            audit::append(conn, &event)
        })?;
        Ok(())
    }

    pub fn deny_use(&self, secret_id: &str, actor: &str) -> Result<UseRequestResult> {
        self.pending_grants
            .lock()
            .expect("pending grants")
            .retain(|_, grant| grant.secret_id != secret_id);
        self.db.with_conn(|conn| {
            let mut event = SecretAuditEvent::new("user", actor, "capability.deny", "denied");
            event.secret_id = Some(secret_id.into());
            audit::append(conn, &event)
        })?;
        Ok(UseRequestResult {
            status: "denied".into(),
            request_id: None,
            capability_id: None,
            proxy_origin: self.proxy_origin(),
            handle: None,
            summary: None,
            provider_routes: None,
        })
    }

    /// The trusted provider accounting for one run.
    ///
    /// Read while the run is live and again at terminal. It is host-owned
    /// evidence, so it answers even when the evaluator has reported nothing —
    /// which is the whole reason a run must never render "cost unavailable"
    /// over a ledger that already holds a billed figure.
    pub fn run_ledger(&self, run_id: &str) -> capability::CapabilityLedger {
        capability::CapabilityLedger::from_capabilities(&self.capabilities.list_for_run(run_id))
    }

    pub fn active_capabilities(&self) -> Result<Vec<CapabilitySummary>> {
        let live = self.capabilities.list_active();
        let mut out = Vec::new();
        for item in live {
            let suffix = self
                .db
                .with_conn(|conn| vault::record(conn, &item.secret_id))?
                .map(|record| fingerprint::mask_suffix(&record.display_suffix));
            out.push(capability::summary_from_live(&item, suffix));
        }
        Ok(out)
    }

    pub fn revoke_capability(&self, capability_id: &str, actor: &str) -> Result<()> {
        self.db
            .transaction(|conn| capability::revoke(conn, &self.capabilities, capability_id, actor))
    }

    pub fn capability_run_id(&self, capability_id: &str) -> Result<Option<String>> {
        self.db
            .with_conn(|conn| capability::run_id_for_capability(conn, capability_id))
    }

    /// Durable, non-secret provider usage for internal optimizer settlement.
    /// Unlike the settings list, this includes exhausted and revoked rows.
    pub(crate) fn provider_usage_receipt(
        &self,
        run_id: &str,
    ) -> Result<Option<ProviderUsageReceipt>> {
        self.db
            .with_conn(|conn| capability::provider_usage_receipt(conn, run_id))
    }

    /// Revoke every live capability for `run_id`, returning the revoked
    /// capability ids so the caller can journal them.
    pub fn revoke_run(&self, run_id: &str) -> Result<Vec<String>> {
        self.db
            .transaction(|conn| capability::revoke_run(conn, &self.capabilities, run_id))
    }

    /// Aggregate the provider proxy's run-level meter. Request attempts are
    /// reserved before forwarding, so this count includes failed provider
    /// requests and is more authoritative than agent-message heuristics.
    pub fn provider_usage_for_run(&self, run_id: &str) -> Option<serde_json::Value> {
        let capabilities = self.capabilities.list_for_run(run_id);
        if capabilities.is_empty() {
            return None;
        }
        let request_attempts = capabilities
            .iter()
            .map(|live| u64::from(live.used_calls))
            .sum::<u64>();
        let input_tokens = capabilities
            .iter()
            .map(|live| live.used_input_tokens)
            .sum::<u64>();
        let output_tokens = capabilities
            .iter()
            .map(|live| live.used_output_tokens)
            .sum::<u64>();
        // Cost is Option-typed on purpose: a reported $0.00 must never be
        // confused with an absent charge. The aggregate is only a dollar
        // figure when every capability's meter is known.
        let cost_known = capabilities
            .iter()
            .all(|live| live.used_cost_usd_micros.is_some());
        let cost_usd_micros = capabilities
            .iter()
            .filter_map(|live| live.used_cost_usd_micros)
            .sum::<u64>();
        let mut providers = capabilities
            .iter()
            .map(|live| live.provider.clone())
            .collect::<Vec<_>>();
        providers.sort();
        providers.dedup();
        Some(serde_json::json!({
            "requestAttempts": request_attempts,
            "inputTokens": input_tokens,
            "outputTokens": output_tokens,
            "totalTokens": input_tokens.saturating_add(output_tokens),
            "costUsd": if cost_known {
                serde_json::json!(cost_usd_micros as f64 / 1_000_000.0)
            } else {
                serde_json::Value::Null
            },
            "capabilityCount": capabilities.len(),
            "providers": providers,
            "basis": "workshop_provider_proxy_reserved_requests",
            "requestCountComplete": true,
        }))
    }

    pub fn audit(&self, limit: i64) -> Result<Vec<SecretAuditEvent>> {
        self.db.with_conn(|conn| audit::list(conn, limit))
    }

    pub fn request_env_import(
        &self,
        source_path: &str,
        variable_names: &[String],
        destination_scope: &str,
        allowed_roots: &[PathBuf],
    ) -> Result<ImportPreview> {
        let (preview, pending) = importer::preview(
            source_path,
            variable_names,
            destination_scope,
            allowed_roots,
        )?;
        self.pending_imports
            .lock()
            .expect("pending imports")
            .insert(preview.request_id.clone(), pending);
        Ok(preview)
    }

    pub fn commit_env_import(
        &self,
        request_id: &str,
        selected: &[String],
        after: AfterImportAction,
        actor: &str,
        confirm: bool,
    ) -> Result<Vec<SecretSummary>> {
        if !matches!(after, AfterImportAction::Keep) && !confirm {
            bail!(
                "replacing or removing .env entries requires a separate confirmation in Settings"
            );
        }
        let pending = self
            .pending_imports
            .lock()
            .expect("pending imports")
            .remove(request_id)
            .ok_or_else(|| anyhow!("import request {request_id} is not pending"))?;
        let mut imported = Vec::new();
        let mut aliases = HashMap::new();
        for entry in &pending.entries {
            if !selected.is_empty() && !selected.iter().any(|name| name == &entry.variable) {
                continue;
            }
            let backend_ref = lease::env_backend_ref(&entry.provider, &entry.variable);
            self.env_sources.put(&backend_ref, &entry.value);
            self.redaction.register(&entry.value);
            let fingerprint = fingerprint::fingerprint(&entry.value);
            let id = self.db.with_conn(|conn| {
                lease::upsert_env_source_descriptor(
                    conn,
                    &entry.provider,
                    &entry.variable,
                    &pending.source_path,
                    None,
                    Some(&fingerprint),
                    true,
                )
            })?;
            let summary =
                self.db
                    .with_conn(|conn| vault::get(conn, &id))?
                    .unwrap_or(SecretSummary {
                        id: id.clone(),
                        alias: entry.alias.clone(),
                        provider: entry.provider.clone(),
                        scope: "project/config/env".into(),
                        status: "valid".into(),
                        backend: lease::BACKEND_CONFIGURED_ENV.into(),
                        display_suffix: Some(fingerprint::mask_suffix(&entry.suffix)),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        last_validated_at: Some(chrono::Utc::now().to_rfc3339()),
                        allowed_recipes: Vec::new(),
                    });
            let _ = actor;
            aliases.insert(entry.variable.clone(), summary.id.clone());
            imported.push(summary);
        }
        if !matches!(after, AfterImportAction::Keep) {
            importer::apply_after_action(&pending.source_path, selected, &aliases, after)?;
        }
        Ok(imported)
    }

    pub fn workload_env(
        &self,
        provider: &str,
        run_id: &str,
        recipe_id: &str,
        policy: ProviderUsePolicy,
        actor: &str,
    ) -> Result<WorkloadEnv> {
        let lease = self.issue_lease(provider, run_id, recipe_id, policy, actor)?;
        let mut env = lease.to_workload_env(
            None,
            self.proxy_socket_path()
                .map(|path| path.display().to_string()),
        );
        let cap_dir = self
            .db
            .path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("capabilities")
            .join(run_id);
        let _ = env.write_capability_file(&cap_dir);
        if matches!(provider, "openai" | "openrouter" | "tinker" | "groq") {
            let _ = env.provider_routes()?;
        }
        Ok(env)
    }
}

fn ensure_capability_covers(
    live: &capability::LiveCapability,
    requested: &ProviderUsePolicy,
) -> Result<()> {
    if live.covers(requested) {
        return Ok(());
    }
    ensure_policy_covers(
        &ProviderUsePolicy {
            operations: live.operations.clone(),
            models: live.models.clone(),
            reasoning_efforts: live.reasoning_efforts.clone(),
            max_calls: live.max_calls,
            max_input_tokens: live.max_input_tokens,
            max_output_tokens: live.max_output_tokens,
            max_cost_usd: live.max_cost_usd_micros as f64 / 1_000_000.0,
            lifetime_seconds: 0,
        },
        requested,
        Some(&live.id),
    )
}

fn ensure_policy_covers(
    granted: &ProviderUsePolicy,
    requested: &ProviderUsePolicy,
    capability_id: Option<&str>,
) -> Result<()> {
    let list_covers = |available: &[String], needed: &[String]| {
        available.is_empty()
            || needed.iter().all(|item| {
                available
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(item))
            })
    };
    if list_covers(&granted.operations, &requested.operations)
        && list_covers(&granted.models, &requested.models)
        && list_covers(&granted.reasoning_efforts, &requested.reasoning_efforts)
        && granted.max_calls >= requested.max_calls
        && granted.max_input_tokens >= requested.max_input_tokens
        && granted.max_output_tokens >= requested.max_output_tokens
        && granted.max_cost_usd + f64::EPSILON >= requested.max_cost_usd
    {
        return Ok(());
    }
    bail!(
        "{}",
        json!({
            "code": "capability_underscoped",
            "contract": "workshop.secrets_proxy",
            "retryable": false,
            "message": "an existing run capability does not cover the admitted execution envelope",
            "capabilityId": capability_id,
            "granted": {
                "operations": granted.operations,
                "models": granted.models,
                "reasoningEfforts": granted.reasoning_efforts,
                "maxCalls": granted.max_calls,
                "maxInputTokens": granted.max_input_tokens,
                "maxOutputTokens": granted.max_output_tokens,
                "maxCostUsd": granted.max_cost_usd,
            },
            "required": {
                "operations": requested.operations,
                "models": requested.models,
                "reasoningEfforts": requested.reasoning_efforts,
                "maxCalls": requested.max_calls,
                "maxInputTokens": requested.max_input_tokens,
                "maxOutputTokens": requested.max_output_tokens,
                "maxCostUsd": requested.max_cost_usd,
            },
        })
    )
}

fn service(state: &State<'_, Arc<SecretsService>>) -> Result<Arc<SecretsService>, AppError> {
    Ok(state.inner().clone())
}

#[tauri::command]
#[specta::specta]
pub fn secrets_workspace_roots_list(
    state: State<'_, Arc<SecretsService>>,
) -> Result<Vec<WorkspaceRootSummary>, AppError> {
    Ok(service(&state)?.workspace_roots())
}

#[tauri::command]
#[specta::specta]
pub fn secrets_bindings_list(
    state: State<'_, Arc<SecretsService>>,
) -> Result<Vec<CredentialBindingSummary>, AppError> {
    service(&state)?.bindings().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_locators_list(
    state: State<'_, Arc<SecretsService>>,
) -> Result<Vec<CredentialLocatorSummary>, AppError> {
    service(&state)?.locators(true).map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_locator_remember_external(
    state: State<'_, Arc<SecretsService>>,
    picker_path: String,
    provider: String,
    variable: String,
    label: Option<String>,
) -> Result<CredentialLocatorSummary, AppError> {
    service(&state)?
        .remember_external_locator(
            Path::new(&picker_path),
            &provider,
            &variable,
            label.as_deref().unwrap_or(&provider),
        )
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_locator_register(
    state: State<'_, Arc<SecretsService>>,
    locator_id: String,
) -> Result<CredentialLocatorSummary, AppError> {
    service(&state)?
        .register_locator(&locator_id)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn secrets_locator_forget(
    app: tauri::AppHandle,
    state: State<'_, Arc<SecretsService>>,
    locator_id: String,
) -> Result<(), AppError> {
    let codex = app.state::<Arc<crate::codex::CodexManager>>();
    codex
        .approvals
        .expire_credential_locator(&app, &locator_id, "credential_locator_forgotten")
        .await
        .map_err(AppError::from)?;
    service(&state)?
        .forget_locator(&locator_id)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_list(
    state: State<'_, Arc<SecretsService>>,
    provider: Option<String>,
    scope: Option<String>,
) -> Result<Vec<SecretSummary>, AppError> {
    service(&state)?
        .list(provider.as_deref(), scope.as_deref())
        .map_err(AppError::from)
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SecretCreateRequest {
    pub alias: String,
    pub provider: String,
    pub scope: Option<String>,
    pub value: String,
}

#[tauri::command]
#[specta::specta]
pub fn secrets_create(
    state: State<'_, Arc<SecretsService>>,
    request: SecretCreateRequest,
) -> Result<SecretSummary, AppError> {
    let created = service(&state)?.create(
        &request.alias,
        &request.provider,
        request
            .scope
            .as_deref()
            .unwrap_or("personal/development/providers"),
        &request.value,
        "settings",
    )?;
    Ok(created)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_replace(
    state: State<'_, Arc<SecretsService>>,
    secret_id: String,
    value: String,
) -> Result<SecretSummary, AppError> {
    service(&state)?
        .replace(&secret_id, &value, "settings")
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_delete(
    state: State<'_, Arc<SecretsService>>,
    secret_id: String,
) -> Result<(), AppError> {
    service(&state)?
        .delete(&secret_id, "settings")
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_test(
    state: State<'_, Arc<SecretsService>>,
    secret_id: String,
) -> Result<SecretSummary, AppError> {
    service(&state)?
        .test(&secret_id, "settings")
        .map_err(AppError::from)
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SecretUseRequest {
    pub secret_id: String,
    pub run_id: String,
    pub recipe_id: String,
    pub requested_policy: Option<ProviderUsePolicy>,
}

#[tauri::command]
#[specta::specta]
pub fn secrets_request_use(
    state: State<'_, Arc<SecretsService>>,
    request: SecretUseRequest,
) -> Result<UseRequestResult, AppError> {
    service(&state)?
        .request_use(
            &request.secret_id,
            &request.run_id,
            &request.recipe_id,
            request.requested_policy.unwrap_or_default(),
            "agent",
        )
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_grant_use(
    state: State<'_, Arc<SecretsService>>,
    secret_id: String,
    run_id: String,
    recipe_id: String,
    remember_recipe: bool,
    requested_policy: Option<ProviderUsePolicy>,
    request_id: Option<String>,
) -> Result<UseRequestResult, AppError> {
    let secrets = service(&state)?;
    if let Some(request_id) = request_id {
        return secrets
            .grant_pending(&request_id, "settings", remember_recipe)
            .map_err(AppError::from);
    }
    secrets
        .grant_use(
            &secret_id,
            &run_id,
            &recipe_id,
            requested_policy.unwrap_or_default(),
            "settings",
            remember_recipe,
        )
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_deny_use(
    state: State<'_, Arc<SecretsService>>,
    secret_id: String,
) -> Result<UseRequestResult, AppError> {
    service(&state)?
        .deny_use(&secret_id, "settings")
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_capabilities_list(
    state: State<'_, Arc<SecretsService>>,
) -> Result<Vec<CapabilitySummary>, AppError> {
    service(&state)?
        .active_capabilities()
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn secrets_revoke_capability(
    state: State<'_, Arc<SecretsService>>,
    core: State<'_, Arc<crate::core_runtime::CoreRuntime>>,
    capability_id: String,
) -> Result<(), AppError> {
    let secrets = service(&state)?;
    let run_id = secrets
        .capability_run_id(&capability_id)
        .map_err(AppError::from)?;
    secrets
        .revoke_capability(&capability_id, "settings")
        .map_err(AppError::from)?;
    if let Some(run_id) = run_id {
        let request = crate::optimizers::kernel::CancellationRequest::new(
            crate::optimizers::kernel::CancellationCause::CredentialRevoked,
            "user:settings",
            format!("run:{run_id}"),
        );
        // The reverse edge is best-effort only for already-terminal runs: a
        // manual capability revocation must otherwise become the typed cause
        // of cancellation instead of a cascade of anonymous 401s.
        match core.optimizers().cancel(run_id, request).await {
            Ok(_) => {}
            Err(error) if error.to_string().contains("sealed") => {}
            Err(error) => return Err(AppError::from(error)),
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvImportRequest {
    pub source_path: String,
    pub variable_names: Option<Vec<String>>,
    pub destination_scope: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn secrets_request_env_import(
    state: State<'_, Arc<SecretsService>>,
    request: EnvImportRequest,
) -> Result<ImportPreview, AppError> {
    let roots = allowed_import_roots();
    service(&state)?
        .request_env_import(
            &request.source_path,
            request.variable_names.as_deref().unwrap_or(&[]),
            request
                .destination_scope
                .as_deref()
                .unwrap_or("personal/development/providers"),
            &roots,
        )
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_commit_env_import(
    state: State<'_, Arc<SecretsService>>,
    request_id: String,
    selected: Vec<String>,
    after: AfterImportAction,
    confirm: Option<bool>,
) -> Result<Vec<SecretSummary>, AppError> {
    service(&state)?
        .commit_env_import(
            &request_id,
            &selected,
            after,
            "settings",
            confirm.unwrap_or(false),
        )
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_audit_list(
    state: State<'_, Arc<SecretsService>>,
    limit: Option<u32>,
) -> Result<Vec<SecretAuditEvent>, AppError> {
    service(&state)?
        .audit(i64::from(limit.unwrap_or(100)))
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_proxy_status(
    state: State<'_, Arc<SecretsService>>,
) -> Result<SecretsProxyStatus, AppError> {
    let origin = service(&state)?.proxy_origin();
    Ok(SecretsProxyStatus {
        running: origin.is_some(),
        origin,
    })
}

#[tauri::command]
#[specta::specta]
pub fn secrets_pending(state: State<'_, Arc<SecretsService>>) -> Result<SecretsInbox, AppError> {
    service(&state)?.inbox().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn secrets_deny_env_import(
    state: State<'_, Arc<SecretsService>>,
    request_id: String,
) -> Result<(), AppError> {
    service(&state)?
        .deny_import(&request_id, "settings")
        .map_err(AppError::from)
}

fn allowed_import_roots() -> Vec<PathBuf> {
    crate::secrets::import_roots()
}

pub(crate) fn import_roots() -> Vec<PathBuf> {
    vec![crate::instance::data_root()]
}

