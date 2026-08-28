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

#[cfg(test)]
pub(crate) fn install_test_live_openai() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let dir = tempfile::tempdir().expect("test secrets dir");
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);
        let storage = crate::storage::Storage::open(&path).expect("test secrets storage");
        let service = Arc::new(SecretsService::with_backend(
            storage.database().clone(),
            Arc::new(backend::MemoryBackend::new()),
        ));
        let env_file = path.join(".env");
        std::fs::write(&env_file, "OPENAI_API_KEY=sk-test-healthbench-not-real\n")
            .expect("seed test env");
        service
            .load_one_env_source("openai", "OPENAI_API_KEY", &env_file)
            .expect("load configured openai env source");
        let _ = service.start_proxy();
        install_live(service);
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use serde_json::json;

    fn service() -> (tempfile::TempDir, SecretsService) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let service = SecretsService::with_backend(
            storage.database().clone(),
            Arc::new(backend::MemoryBackend::new()),
        );
        (dir, service)
    }

    fn capability_source(service: &SecretsService, path: &Path) -> String {
        service
            .db
            .transaction(|conn| {
                lease::upsert_env_source_descriptor(
                    conn,
                    "openrouter",
                    "OPENROUTER_API_KEY",
                    path,
                    None,
                    None,
                    false,
                )
            })
            .unwrap()
    }

    fn issue_test_capability(
        service: &SecretsService,
        source_id: &str,
        run_id: &str,
        max_calls: u32,
    ) -> capability::IssuedCapability {
        let policy = ProviderUsePolicy {
            max_calls,
            ..ProviderUsePolicy::default()
        };
        service
            .db
            .transaction(|conn| {
                capability::issue(
                    conn,
                    &service.capabilities,
                    source_id,
                    run_id,
                    "nanohorizon.craftax.eval.v1",
                    "openrouter",
                    &policy,
                    "test",
                )
            })
            .unwrap()
    }

    fn debit_test_capability(
        service: &SecretsService,
        issued: &capability::IssuedCapability,
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: Option<f64>,
    ) -> capability::LiveCapability {
        service.capabilities.reserve_call(&issued.handle).unwrap();
        let live = service
            .capabilities
            .debit_usage(
                &issued.handle,
                &capability::MeasuredUsage {
                    calls: 1,
                    input_tokens,
                    output_tokens,
                    cost_usd,
                },
            )
            .unwrap();
        service
            .db
            .with_conn(|conn| capability::persist_usage(conn, &live))
            .unwrap();
        live
    }

    #[test]
    fn provider_usage_receipt_includes_active_exhausted_and_revoked_rows() {
        let (dir, service) = service();
        let source_id = capability_source(&service, &dir.path().join("not-read.env"));
        let run_id = "run_all_capability_statuses";

        let active = issue_test_capability(&service, &source_id, run_id, 5);
        let active_live = debit_test_capability(&service, &active, 10, 1, Some(0.001234));
        assert_eq!(active_live.status, "active");

        let exhausted = issue_test_capability(&service, &source_id, run_id, 1);
        let exhausted_live = debit_test_capability(&service, &exhausted, 20, 2, Some(0.002));
        assert_eq!(exhausted_live.status, "exhausted");

        let revoked = issue_test_capability(&service, &source_id, run_id, 5);
        debit_test_capability(&service, &revoked, 30, 3, Some(0.003));
        service
            .revoke_capability(&revoked.id, "test")
            .expect("revoke active capability");

        let receipt = service
            .provider_usage_receipt(run_id)
            .unwrap()
            .expect("provider usage receipt");
        assert_eq!(receipt.calls, 3);
        assert_eq!(receipt.input_tokens, 60);
        assert_eq!(receipt.output_tokens, 6);
        assert_eq!(receipt.cost_usd, Some(0.006234));
        let statuses = receipt
            .capabilities
            .iter()
            .map(|capability| capability.status.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(statuses, HashSet::from(["active", "exhausted", "revoked"]));
        assert!(receipt.digest.starts_with("sha256:"));
        assert_eq!(receipt.digest.len(), 71);
        assert_eq!(
            receipt,
            service.provider_usage_receipt(run_id).unwrap().unwrap(),
            "the canonical receipt is stable across reads"
        );
        let encoded = serde_json::to_string(&receipt).unwrap();
        assert!(!encoded.contains(&source_id));
        assert!(!encoded.contains("wcap_"));
    }

    #[test]
    fn provider_usage_receipt_poisoned_cost_stays_unknown() {
        let (dir, service) = service();
        let source_id = capability_source(&service, &dir.path().join("not-read.env"));
        let run_id = "run_unknown_capability_cost";
        let known = issue_test_capability(&service, &source_id, run_id, 5);
        let unknown = issue_test_capability(&service, &source_id, run_id, 5);
        debit_test_capability(&service, &known, 40, 4, Some(0.004));
        debit_test_capability(&service, &unknown, 50, 5, None);

        let receipt = service
            .provider_usage_receipt(run_id)
            .unwrap()
            .expect("provider usage receipt");
        assert_eq!(receipt.calls, 2);
        assert_eq!(receipt.input_tokens, 90);
        assert_eq!(receipt.output_tokens, 9);
        assert_eq!(receipt.cost_usd, None);
        assert_eq!(receipt.capabilities.len(), 2);
    }

    fn locator_service(label: &str) -> (crate::instance::IsolatedDataRoot, SecretsService) {
        let root = crate::instance::IsolatedDataRoot::new(label);
        let storage = Storage::open(root.path.join("storage")).unwrap();
        let service = SecretsService::with_backend(
            storage.database().clone(),
            Arc::new(backend::MemoryBackend::new()),
        );
        (root, service)
    }

    #[test]
    fn remembered_locator_does_not_enter_ram_until_registered() {
        let (root, service) = locator_service("locator-remember-register");
        let env_file = root.path.join("provider.env");
        std::fs::write(&env_file, "OPENAI_API_KEY=sk-locator-not-real\n").unwrap();

        let locator = service
            .remember_external_locator(&env_file, "openai", "OPENAI_API_KEY", "Project OpenAI")
            .unwrap();
        assert!(service.env_sources.keys().is_empty());
        assert!(locator.source_id.is_none());

        assert!(service.prepare_register_approval(&locator.id).unwrap());
        service.settle_remembered_locator(&locator.id).unwrap();
        assert!(service.env_sources.keys().is_empty());
        assert!(service
            .locators(true)
            .unwrap()
            .into_iter()
            .find(|row| row.id == locator.id)
            .unwrap()
            .source_id
            .is_none());

        assert!(service.prepare_register_approval(&locator.id).unwrap());
        let registered = service.register_locator(&locator.id).unwrap();
        assert!(registered.loaded);
        assert!(registered.preferred);
        assert_eq!(
            service.env_sources.keys(),
            vec![lease::env_backend_ref("openai", "OPENAI_API_KEY")]
        );
    }

    #[test]
    fn empty_locator_value_writes_no_source_license_or_ram_value() {
        let (root, service) = locator_service("locator-empty-value");
        let env_file = root.path.join("empty.env");
        std::fs::write(&env_file, "OPENAI_API_KEY=\n").unwrap();
        let locator = service
            .remember_external_locator(&env_file, "openai", "OPENAI_API_KEY", "Empty OpenAI")
            .unwrap();

        let error = service.register_locator(&locator.id).unwrap_err();
        assert!(error.to_string().contains(lease::CREDENTIAL_VALUE_MISSING));
        assert!(service.env_sources.keys().is_empty());
        assert!(service
            .locators(true)
            .unwrap()
            .into_iter()
            .find(|row| row.id == locator.id)
            .unwrap()
            .source_id
            .is_none());
    }

    #[test]
    fn source_consent_is_serialized_per_pair_and_globally_capped() {
        let (_dir, service) = service();
        service.begin_source_consent("openai", "KEY_ONE").unwrap();
        let duplicate = service
            .begin_source_consent("openai", "KEY_ONE")
            .unwrap_err();
        assert!(duplicate
            .to_string()
            .contains(lease::CREDENTIAL_SOURCE_CONSENT_PENDING));
        service.begin_source_consent("openai", "KEY_TWO").unwrap();
        service.begin_source_consent("openai", "KEY_THREE").unwrap();
        service.begin_source_consent("openai", "KEY_FOUR").unwrap();
        let over_limit = service
            .begin_source_consent("openai", "KEY_FIVE")
            .unwrap_err();
        assert!(over_limit
            .to_string()
            .contains(lease::CREDENTIAL_LOCATOR_LIMIT));
    }

    #[test]
    fn removing_a_source_license_keeps_the_remembered_location() {
        let (root, service) = locator_service("locator-source-remove");
        let env_file = root.path.join("provider.env");
        std::fs::write(&env_file, "OPENAI_API_KEY=sk-remove-not-real\n").unwrap();
        let locator = service
            .remember_external_locator(&env_file, "openai", "OPENAI_API_KEY", "Project OpenAI")
            .unwrap();
        service.register_locator(&locator.id).unwrap();

        service.remove_locator_source(&locator.id).unwrap();
        let remembered = service
            .locators(true)
            .unwrap()
            .into_iter()
            .find(|row| row.id == locator.id)
            .unwrap();
        assert_eq!(remembered.state, CredentialLocatorState::Observed);
        assert!(!remembered.registered);
        assert!(!remembered.loaded);
        assert!(service.env_sources.keys().is_empty());
    }

    #[test]
    fn changed_fingerprint_unloads_registered_locator() {
        let (root, service) = locator_service("locator-fingerprint-change");
        let env_file = root.path.join("changing.env");
        std::fs::write(&env_file, "OPENAI_API_KEY=sk-first-not-real\n").unwrap();
        let locator = service
            .remember_external_locator(&env_file, "openai", "OPENAI_API_KEY", "Changing OpenAI")
            .unwrap();
        service.register_locator(&locator.id).unwrap();
        std::fs::write(&env_file, "OPENAI_API_KEY=sk-second-not-real\n").unwrap();

        let error = service
            .load_registered_locator(&locator.id, false)
            .unwrap_err();
        assert!(error.to_string().contains(lease::CREDENTIAL_VALUE_UNLOADED));
        assert!(service.env_sources.keys().is_empty());
        let row = service
            .locators(true)
            .unwrap()
            .into_iter()
            .find(|row| row.id == locator.id)
            .unwrap();
        assert!(!row.loaded);
        assert_eq!(row.source_state.as_deref(), Some("fingerprint_changed"));
    }

    #[test]
    fn switching_sources_revokes_old_caps_and_forget_restores_instance() {
        let (root, service) = locator_service("locator-switch-fallback");
        let instance_file = root.path.join("instance.env");
        std::fs::write(&instance_file, "OPENAI_API_KEY=sk-instance-not-real\n").unwrap();
        let instance_bytes = SecretBytes::from_utf8("sk-instance-not-real");
        let instance_digest = fingerprint::fingerprint(&instance_bytes);
        let (instance_locator_id, instance_source_id) = service
            .db
            .transaction(|conn| {
                let instance = locator::upsert_instance(
                    conn,
                    &instance_file,
                    "openai",
                    "OPENAI_API_KEY",
                    "Workshop instance OpenAI",
                )?;
                let source_id = lease::upsert_env_source_descriptor(
                    conn,
                    "openai",
                    "OPENAI_API_KEY",
                    &instance_file,
                    Some(&instance.id),
                    Some(&instance_digest),
                    true,
                )?;
                locator::mark_preferred(conn, &source_id, "openai", "OPENAI_API_KEY")?;
                Ok((instance.id, source_id))
            })
            .unwrap();
        service
            .load_registered_locator(&instance_locator_id, true)
            .unwrap();

        let first_file = root.path.join("first.env");
        std::fs::write(&first_file, "OPENAI_API_KEY=sk-first-external-not-real\n").unwrap();
        let first = service
            .remember_external_locator(&first_file, "openai", "OPENAI_API_KEY", "First external")
            .unwrap();
        let first = service.register_locator(&first.id).unwrap();
        let first_source_id = first.source_id.clone().unwrap();
        let issued = service
            .db
            .with_conn(|conn| {
                capability::issue(
                    conn,
                    service.capabilities.as_ref(),
                    &first_source_id,
                    "run-switch",
                    "recipe-switch",
                    "openai",
                    &ProviderUsePolicy::default(),
                    "test",
                )
            })
            .unwrap();
        assert!(service
            .capabilities
            .find_active(&first_source_id, "run-switch")
            .is_some());

        let second_file = root.path.join("second.env");
        std::fs::write(&second_file, "OPENAI_API_KEY=sk-second-external-not-real\n").unwrap();
        let second = service
            .remember_external_locator(&second_file, "openai", "OPENAI_API_KEY", "Second external")
            .unwrap();
        service.register_locator(&second.id).unwrap();
        assert!(service
            .capabilities
            .find_active(&first_source_id, "run-switch")
            .is_none());
        assert!(service.capabilities.reserve_call(&issued.handle).is_err());

        service.forget_locator(&second.id).unwrap();
        assert_eq!(
            service.source_for_locator(&instance_locator_id).unwrap(),
            instance_source_id
        );
        assert!(
            service
                .locators(true)
                .unwrap()
                .into_iter()
                .find(|row| row.id == instance_locator_id)
                .unwrap()
                .loaded
        );
    }

    #[test]
    fn create_list_never_returns_plaintext() {
        let (_dir, service) = service();
        let canary = "sk-proj-SUPERSECRET7F2A";
        let created = service
            .create(
                "Personal OpenAI",
                "openai",
                "personal/development/providers",
                canary,
                "test",
            )
            .unwrap();
        assert_eq!(created.alias, "Personal OpenAI");
        assert!(created.display_suffix.as_deref().unwrap().contains("7F2A"));
        let encoded = serde_json::to_string(&created).unwrap();
        assert!(!encoded.contains(canary));
        assert!(!encoded.contains("SUPERSECRET"));
        let listed = service.list(Some("openai"), None).unwrap();
        assert_eq!(listed.len(), 1);
        let dump = serde_json::to_string(&listed).unwrap();
        assert!(!dump.contains(canary));
    }

    #[test]
    fn sqlite_metadata_has_no_secret_value() {
        let (dir, service) = service();
        let canary = "sk-live-canary-value-9XYZ";
        service
            .create(
                "Personal OpenAI",
                "openai",
                "personal/development/providers",
                canary,
                "test",
            )
            .unwrap();
        let db = std::fs::read(dir.path().join("synth.sqlite3")).unwrap();
        assert!(!String::from_utf8_lossy(&db).contains(canary));
        assert!(!db
            .windows(canary.len())
            .any(|window| window == canary.as_bytes()));
    }

    #[test]
    fn repeated_proxy_reads_do_not_reenter_the_os_store() {
        use super::backend::{CachedBackend, MemoryBackend};
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Counting(MemoryBackend, AtomicUsize);
        impl SecretBackend for Counting {
            fn create(&self, id: &str, value: &SecretBytes) -> Result<String> {
                self.0.create(id, value)
            }
            fn replace(&self, backend_ref: &str, value: &SecretBytes) -> Result<()> {
                self.0.replace(backend_ref, value)
            }
            fn delete(&self, backend_ref: &str) -> Result<()> {
                self.0.delete(backend_ref)
            }
            fn resolve(&self, backend_ref: &str) -> Result<SecretBytes> {
                self.1.fetch_add(1, Ordering::SeqCst);
                self.0.resolve(backend_ref)
            }
            fn status(&self, backend_ref: &str) -> Result<backend::BackendStatus> {
                panic!("status must not probe Keychain: {backend_ref}");
            }
            fn backend_name(&self) -> &'static str {
                "counting"
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let counting = Arc::new(Counting(MemoryBackend::new(), AtomicUsize::new(0)));
        let service = SecretsService::with_backend(
            storage.database().clone(),
            CachedBackend::wrap(counting.clone()),
        );
        let created = service
            .create(
                "Personal OpenAI",
                "openai",
                "personal/development/providers",
                "sk-repeat-cache",
                "test",
            )
            .unwrap();
        service.test(&created.id, "test").unwrap();
        service.test(&created.id, "test").unwrap();
        assert_eq!(
            counting.1.load(Ordering::SeqCst),
            0,
            "create already cached the value; validate must not hit Keychain again"
        );
    }

    #[test]
    fn replace_and_delete_are_audited_separately() {
        let (_dir, service) = service();
        let created = service
            .create(
                "Personal OpenAI",
                "openai",
                "personal/development/providers",
                "sk-one-AAA1",
                "test",
            )
            .unwrap();
        service.replace(&created.id, "sk-two-BBB2", "test").unwrap();
        service.delete(&created.id, "test").unwrap();
        let events = service.audit(20).unwrap();
        let actions: Vec<_> = events.iter().map(|event| event.action.as_str()).collect();
        assert!(actions.contains(&"secret.create"));
        assert!(actions.contains(&"secret.replace"));
        assert!(actions.contains(&"secret.delete"));
        let dump = serde_json::to_string(&events).unwrap();
        assert!(!dump.contains("sk-one"));
        assert!(!dump.contains("sk-two"));
    }

    #[test]
    fn agent_use_returns_capability_not_secret() {
        let (_dir, service) = service();
        service.start_proxy().unwrap();
        let created = service
            .create(
                "Personal OpenAI",
                "openai",
                "personal/development/providers",
                "sk-use-CCCC",
                "test",
            )
            .unwrap();
        let granted = service
            .grant_use(
                &created.id,
                "eval_01",
                "eval.craftax.llm-policy.smoke.v1",
                ProviderUsePolicy::default(),
                "test",
                false,
            )
            .unwrap();
        assert_eq!(granted.status, "granted");
        assert!(granted.handle.as_deref().unwrap().starts_with("wcap_"));
        let dump = serde_json::to_string(&granted).unwrap();
        assert!(!dump.contains("sk-use-CCCC"));
    }

    #[test]
    fn agent_request_use_returns_approval_id_not_secret() {
        let (_dir, service) = service();
        let created = service
            .create(
                "Personal OpenAI",
                "openai",
                "personal/development/providers",
                "sk-ask-GGGG",
                "test",
            )
            .unwrap();
        let pending = service
            .request_use(
                &created.id,
                "eval_03",
                "eval.craftax.llm-policy.smoke.v1",
                ProviderUsePolicy::default(),
                "agent",
            )
            .unwrap();
        assert_eq!(pending.status, "approval_required");
        let request_id = pending.request_id.clone().unwrap();
        assert!(request_id.starts_with("grant_"));
        assert!(pending.handle.is_none());
        let dump = serde_json::to_string(&pending).unwrap();
        assert!(!dump.contains("sk-ask-GGGG"));
        let granted = service
            .grant_pending(&request_id, "settings", false)
            .unwrap();
        assert_eq!(granted.status, "granted");
        assert!(granted.handle.as_deref().unwrap().starts_with("wcap_"));
        let again = service
            .request_use(
                &created.id,
                "eval_03",
                "eval.craftax.llm-policy.smoke.v1",
                ProviderUsePolicy::default(),
                "agent",
            )
            .unwrap();
        assert_eq!(again.status, "granted");
        assert_eq!(again.handle, granted.handle);
        let inbox = service.inbox().unwrap();
        assert!(inbox.grants.is_empty());
    }

    #[test]
    fn active_capability_reuse_rejects_an_underscoped_grant() {
        let (_dir, service) = service();
        let created = service
            .create(
                "Project OpenRouter",
                "openrouter",
                "project/config/env",
                "sk-not-real-underscope",
                "test",
            )
            .unwrap();
        let narrow = ProviderUsePolicy {
            max_calls: 40,
            max_cost_usd: 0.60,
            ..ProviderUsePolicy::default()
        };
        service
            .grant_use(&created.id, "run-envelope", "recipe", narrow, "test", false)
            .unwrap();
        let required = ProviderUsePolicy {
            max_calls: 50,
            max_cost_usd: 2.45,
            models: vec!["z-ai/glm-5.3-flash".into()],
            ..ProviderUsePolicy::default()
        };
        let error = service
            .request_use(&created.id, "run-envelope", "recipe", required, "agent")
            .unwrap_err()
            .to_string();
        assert!(error.contains("capability_underscoped"), "{error}");
        assert!(error.contains("\"maxCalls\":40"), "{error}");
        assert!(error.contains("\"maxCalls\":50"), "{error}");
        assert!(error.contains("2.45"), "{error}");
    }

    #[test]
    fn revoking_a_run_capability_preserves_the_file_backed_source() {
        let (root, service) = locator_service("locator-capability-revoke");
        let env_file = root.path.join("provider.env");
        std::fs::write(&env_file, "OPENROUTER_API_KEY=sk-revoke-not-real\n").unwrap();
        let locator = service
            .remember_external_locator(
                &env_file,
                "openrouter",
                "OPENROUTER_API_KEY",
                "Project OpenRouter",
            )
            .unwrap();
        service.register_locator(&locator.id).unwrap();
        let source_id = service.source_for_locator(&locator.id).unwrap();
        let granted = service
            .grant_use(
                &source_id,
                "run-revoke-only",
                "recipe",
                ProviderUsePolicy::default(),
                "test",
                false,
            )
            .unwrap();
        let revoked_handle = granted.handle.clone().unwrap();
        service
            .revoke_capability(granted.capability_id.as_deref().unwrap(), "test")
            .unwrap();

        let denied = service
            .capabilities
            .reserve_call(&revoked_handle)
            .unwrap_err()
            .to_string();
        assert!(denied.contains("revoked"), "{denied}");
        assert!(service
            .capabilities
            .find_active(&source_id, "run-revoke-only")
            .is_none());
        let source = service
            .locators(true)
            .unwrap()
            .into_iter()
            .find(|row| row.id == locator.id)
            .unwrap();
        assert!(
            source.registered,
            "capability revoke must not unregister source"
        );
        assert!(
            source.loaded,
            "capability revoke must not unload source material"
        );

        let fresh = service
            .grant_use(
                &source_id,
                "run-after-revoke",
                "recipe",
                ProviderUsePolicy::default(),
                "test",
                false,
            )
            .unwrap();
        assert_eq!(fresh.status, "granted");
    }

    #[tokio::test]
    async fn proxy_rejects_unknown_paths_and_missing_capability() {
        let (_dir, service) = service();
        let origin = service.start_proxy().unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let client = crate::http::http_client();
        let forbidden = client
            .post(format!("{origin}/v1/providers/openai/anything"))
            .bearer_auth("wcap_nope")
            .json(&json!({"model":"gpt-4"}))
            .send()
            .await
            .unwrap();
        assert_eq!(forbidden.status(), 404);
        let unauthorized = client
            .post(format!("{origin}/v1/providers/openai/chat/completions"))
            .json(&json!({"model":"gpt-4"}))
            .send()
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), 401);
        let sentinel = client
            .post(format!("{origin}/v1/providers/openai/chat/completions"))
            .bearer_auth(API_KEY_SENTINEL)
            .json(&json!({"model":"gpt-4"}))
            .send()
            .await
            .unwrap();
        assert_eq!(sentinel.status(), 401);
        let created = service
            .create(
                "Personal OpenAI",
                "openai",
                "personal/development/providers",
                "sk-path-auth",
                "test",
            )
            .unwrap();
        let granted = service
            .grant_use(
                &created.id,
                "eval_path",
                "recipe",
                ProviderUsePolicy::default(),
                "test",
                false,
            )
            .unwrap();
        let handle = granted.handle.unwrap();
        let via_path = client
            .post(format!(
                "{origin}/cap/{handle}/v1/providers/openai/chat/completions"
            ))
            .bearer_auth(API_KEY_SENTINEL)
            .json(&json!({"model": "gpt-5.6-luna"}))
            .send()
            .await
            .unwrap();
        let via_status = via_path.status();
        let body = via_path.text().await.unwrap_or_default();
        assert_ne!(via_status, 404, "{body}");
        assert!(
            !body.contains("a Workshop run capability is required")
                && !body.contains("capability is not valid"),
            "path capability should authenticate: {body}"
        );
        assert!(
            !body.contains("sk-path-auth"),
            "upstream errors must not echo the stored credential: {body}"
        );
        let connect = client
            .request(
                reqwest::Method::CONNECT,
                format!("{origin}/v1/providers/openai/chat/completions"),
            )
            .send()
            .await;
        if let Ok(response) = connect {
            assert_eq!(response.status(), 403);
        }
    }

    #[tokio::test]
    async fn proxy_rejects_disallowed_model_and_revoked_capability() {
        let (_dir, service) = service();
        let origin = service.start_proxy().unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let created = service
            .create(
                "Personal OpenAI",
                "openai",
                "personal/development/providers",
                "sk-proxy-DDDD",
                "test",
            )
            .unwrap();
        let mut policy = ProviderUsePolicy::default();
        policy.models = vec!["gpt-5.6-luna".into()];
        policy.max_calls = 2;
        let granted = service
            .grant_use(
                &created.id,
                "eval_02",
                "eval.craftax.llm-policy.smoke.v1",
                policy,
                "test",
                false,
            )
            .unwrap();
        let handle = granted.handle.clone().unwrap();
        let client = crate::http::http_client();
        let denied = client
            .post(format!("{origin}/v1/providers/openai/chat/completions"))
            .bearer_auth(&handle)
            .json(&json!({"model":"gpt-4o"}))
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), 403);
        service
            .revoke_capability(&granted.capability_id.unwrap(), "test")
            .unwrap();
        let replay = client
            .post(format!("{origin}/v1/providers/openai/chat/completions"))
            .bearer_auth(&handle)
            .json(&json!({"model":"gpt-5.6-luna"}))
            .send()
            .await
            .unwrap();
        assert_eq!(replay.status(), 401);
    }

    #[test]
    fn env_import_returns_masked_candidates_and_stores_without_agent_plaintext() {
        let (dir, service) = service();
        let env_path = dir.path().join("workspace").join(".env");
        std::fs::create_dir_all(env_path.parent().unwrap()).unwrap();
        std::fs::write(
            &env_path,
            "OPENAI_API_KEY=sk-import-EEEE7F2A\nDATABASE_URL=postgres://hidden\n",
        )
        .unwrap();
        let preview = service
            .request_env_import(
                env_path.to_str().unwrap(),
                &["OPENAI_API_KEY".into()],
                "personal/development/providers",
                &[dir.path().to_path_buf()],
            )
            .unwrap();
        let encoded = serde_json::to_string(&preview).unwrap();
        assert!(encoded.contains("OPENAI_API_KEY"));
        assert!(encoded.contains("7F2A"));
        assert!(!encoded.contains("sk-import-EEEE7F2A"));
        assert!(!encoded.contains("postgres://hidden"));
        let pending_inbox = serde_json::to_string(&service.inbox().unwrap()).unwrap();
        assert!(pending_inbox.contains("OPENAI_API_KEY"));
        assert!(!pending_inbox.contains("sk-import-EEEE7F2A"));
        let imported = service
            .commit_env_import(
                &preview.request_id,
                &["OPENAI_API_KEY".into()],
                AfterImportAction::Keep,
                "test",
                false,
            )
            .unwrap();
        assert_eq!(imported.len(), 1);
        assert!(!serde_json::to_string(&imported)
            .unwrap()
            .contains("sk-import"));
        assert!(preview.source_remains_readable);
        assert!(preview.warning.unwrap().contains("plaintext"));
        let inbox = service.inbox().unwrap();
        assert_eq!(
            inbox.imports.len(),
            0,
            "commit should clear the pending import"
        );
        let listed = service.list(Some("openai"), None).unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[test]
    fn env_import_rejects_symlink_escape() {
        let (dir, service) = service();
        let allowed = dir.path().join("workspace");
        std::fs::create_dir_all(&allowed).unwrap();
        let outside = dir.path().join("outside.env");
        std::fs::write(&outside, "OPENAI_API_KEY=sk-escape\n").unwrap();
        let link = allowed.join(".env");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            let error = service
                .request_env_import(
                    link.to_str().unwrap(),
                    &[],
                    "personal/development/providers",
                    &[allowed],
                )
                .unwrap_err();
            assert!(
                error.to_string().contains("outside") || error.to_string().contains("refusing"),
                "{error}"
            );
        }
    }

    #[test]
    fn redaction_scrubs_registered_values() {
        let (_dir, service) = service();
        service
            .create(
                "Personal OpenAI",
                "openai",
                "personal/development/providers",
                "sk-log-FFFF1111",
                "test",
            )
            .unwrap();
        let redacted = service.redact("the key is sk-log-FFFF1111 in this trace");
        assert!(!redacted.contains("sk-log-FFFF1111"));
        assert!(redacted.contains("<redacted-by-workshop>"));
    }

    #[test]
    fn no_read_value_surface_on_public_types() {
        let source = include_str!("mod.rs");
        for forbidden in [
            "secrets_get",
            "secrets_reveal",
            "secrets_export",
            "read_value",
            "readValue",
        ] {
            assert!(
                !source.contains(&format!("pub fn {forbidden}")),
                "public read-value API {forbidden} must not exist"
            );
        }
    }

    #[test]
    fn workload_env_is_sentinel_not_provider_key() {
        let (dir, service) = service();
        let env_file = dir.path().join(".env");
        service
            .load_test_env_source("openai", "OPENAI_API_KEY", "sk-proj-NEVERINENV", &env_file)
            .unwrap();
        let env = service
            .workload_env(
                "openai",
                "eval_cap",
                "eval.craftax.llm-policy.smoke.v1",
                ProviderUsePolicy::default(),
                "test",
            )
            .unwrap();
        let pairs = env.as_pairs();
        let dump = format!("{pairs:?}");
        assert!(!dump.contains("sk-proj-NEVERINENV"));
        assert_eq!(
            pairs
                .iter()
                .find(|(key, _)| key == "OPENAI_API_KEY")
                .map(|(_, value)| value.as_str()),
            Some(API_KEY_SENTINEL)
        );
        let base = pairs
            .iter()
            .find(|(key, _)| key == "OPENAI_BASE_URL")
            .map(|(_, value)| value.as_str())
            .unwrap();
        assert!(base.contains("/cap/wcap_"));
        assert!(base.contains("/v1/providers/openai"));
        assert!(base.contains("127.0.0.1"), "host worker uses loopback");
        let route = pairs
            .iter()
            .find(|(key, _)| key == "WORKSHOP_OPENAI_ROUTE")
            .map(|(_, value)| value.as_str())
            .unwrap();
        assert!(route.contains("host.docker.internal"));
        assert!(route.ends_with("/v1/providers/openai/chat/completions"));
        assert!(!route.contains("127.0.0.1"));
        assert!(!route.contains("api.openai.com"));
        let routes = env.provider_routes().unwrap();
        assert_eq!(routes["openai"], route);
        assert_eq!(routes["api_key_sentinel"], API_KEY_SENTINEL);
        assert!(env.capability_file.is_some());
        let file = std::fs::read_to_string(env.capability_file.as_ref().unwrap()).unwrap();
        assert!(file.starts_with("wcap_"));
        assert!(!file.contains("sk-proj"));
        #[cfg(unix)]
        {
            let socket = pairs
                .iter()
                .find(|(key, _)| key == "WORKSHOP_PROXY_SOCKET")
                .map(|(_, value)| value.as_str());
            assert!(
                socket.is_some_and(|path| path.contains("workshop-proxy-") && path.ends_with(".sock")),
                "unix socket should be advertised: {pairs:?}"
            );
        }
    }

    #[test]
    fn docker_bridge_peers_are_allowed_lan_is_not() {
        use std::net::IpAddr;
        fn ip(value: &str) -> IpAddr {
            value.parse().unwrap()
        }
        assert!(proxy::allowed_proxy_peer(ip("127.0.0.1")));
        assert!(proxy::allowed_proxy_peer(ip("172.17.0.5")));
        assert!(proxy::allowed_proxy_peer(ip("172.31.255.1")));
        assert!(proxy::allowed_proxy_peer(ip("192.168.65.2")));
        assert!(proxy::allowed_proxy_peer(ip("10.88.0.9")));
        assert!(!proxy::allowed_proxy_peer(ip("192.168.1.50")));
        assert!(!proxy::allowed_proxy_peer(ip("10.0.0.5")));
        assert!(!proxy::allowed_proxy_peer(ip("8.8.8.8")));
    }

    #[test]
    fn container_origin_still_rewrites_loopback_for_docker_desktop() {
        let rewritten = proxy::rewrite_origin_for_containers("http://127.0.0.1:18451");
        assert_eq!(rewritten, "http://host.docker.internal:18451");
        assert!(!rewritten.contains("api.openai.com"));
    }

    #[test]
    fn destructive_import_requires_confirmation() {
        let (dir, service) = service();
        let env_path = dir.path().join("workspace").join(".env");
        std::fs::create_dir_all(env_path.parent().unwrap()).unwrap();
        std::fs::write(&env_path, "OPENAI_API_KEY=sk-import-NEEDCONFIRM\n").unwrap();
        let preview = service
            .request_env_import(
                env_path.to_str().unwrap(),
                &["OPENAI_API_KEY".into()],
                "personal/development/providers",
                &[dir.path().to_path_buf()],
            )
            .unwrap();
        let error = service
            .commit_env_import(
                &preview.request_id,
                &["OPENAI_API_KEY".into()],
                AfterImportAction::RemoveEntries,
                "test",
                false,
            )
            .unwrap_err();
        assert!(error.to_string().contains("confirmation"), "{error}");
        assert!(env_path.is_file());
        let preview = service
            .request_env_import(
                env_path.to_str().unwrap(),
                &["OPENAI_API_KEY".into()],
                "personal/development/providers",
                &[dir.path().to_path_buf()],
            )
            .unwrap();
        service
            .commit_env_import(
                &preview.request_id,
                &["OPENAI_API_KEY".into()],
                AfterImportAction::RemoveEntries,
                "test",
                true,
            )
            .unwrap();
        let remaining = std::fs::read_to_string(&env_path).unwrap();
        assert!(!remaining.contains("sk-import-NEEDCONFIRM"));
        assert!(!remaining.contains("OPENAI_API_KEY"));
    }

    #[test]
    fn dotenv_parser_covers_comments_duplicates_and_multiline() {
        let parsed = importer::parse_dotenv(
            "# comment\nOPENAI_API_KEY=first\nOPENAI_API_KEY=second\nNOTE=\"hello\nworld\"\nexport ANTHROPIC_API_KEY='sk-ant'\n",
        )
        .unwrap();
        assert_eq!(
            parsed.get("OPENAI_API_KEY").map(String::as_str),
            Some("second")
        );
        assert_eq!(
            parsed.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some("sk-ant")
        );
        assert_eq!(parsed.get("NOTE").map(String::as_str), Some("hello\nworld"));
        assert!(importer::is_sensitive_env_path(std::path::Path::new(
            "/tmp/.env"
        )));
        assert!(importer::is_sensitive_env_path(std::path::Path::new(
            "secrets.toml"
        )));
        let malformed = importer::parse_dotenv("OPENAI_API_KEY=\"unterminated\n");
        assert!(
            malformed.unwrap_err().to_string().contains("unterminated"),
            "malformed quoted values must fail closed"
        );
    }

    #[test]
    fn capability_store_fails_closed_at_call_ceiling() {
        let store = capability::CapabilityStore::new();
        let live = capability::LiveCapability {
            id: "cap_1".into(),
            handle: "wcap_ceiling".into(),
            secret_id: "sec_1".into(),
            run_id: "run_1".into(),
            recipe_id: "recipe".into(),
            provider: "openai".into(),
            operations: vec!["chat.completions.create".into()],
            models: Vec::new(),
            reasoning_efforts: Vec::new(),
            max_calls: 2,
            used_calls: 0,
            max_input_tokens: 10_000,
            used_input_tokens: 0,
            max_output_tokens: 10_000,
            used_output_tokens: 0,
            max_cost_usd_micros: 1_000_000,
            used_cost_usd_micros: Some(0),
            status: "granted".into(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 60_000,
        };
        store.insert(live.clone());
        store.reserve_call("wcap_ceiling").unwrap();
        store.reserve_call("wcap_ceiling").unwrap();
        let exhausted = store.reserve_call("wcap_ceiling").unwrap_err();
        assert!(
            exhausted.to_string().contains("ceiling")
                || exhausted.to_string().contains("exhausted")
        );
        let mut replay = live;
        replay.handle = "wcap_replay".into();
        replay.used_calls = 0;
        replay.status = "granted".into();
        store.insert(replay);
        store.revoke_run("run_1");
        let replayed = store.reserve_call("wcap_replay").unwrap_err();
        assert!(replayed.to_string().contains("revoked"));
    }

    #[test]
    fn capability_path_strips_handle_for_sdk_base_url() {
        let (handle, path) =
            proxy::split_capability_path("/cap/wcap_abc12345/v1/providers/openai/chat/completions");
        assert_eq!(handle.as_deref(), Some("wcap_abc12345"));
        assert_eq!(path, "/v1/providers/openai/chat/completions");
        let (none, original) =
            proxy::split_capability_path("/v1/providers/openai/chat/completions");
        assert!(none.is_none());
        assert_eq!(original, "/v1/providers/openai/chat/completions");
    }

    #[test]
    fn container_origin_rewrites_loopback_for_docker() {
        let rewritten = proxy::rewrite_origin_for_containers("http://127.0.0.1:18451");
        assert_eq!(rewritten, "http://host.docker.internal:18451");
        let route = proxy::capability_chat_completions_url(&rewritten, "wcap_abc12345", "openai");
        assert_eq!(
            route,
            "http://host.docker.internal:18451/cap/wcap_abc12345/v1/providers/openai/chat/completions"
        );
        assert!(!route.contains("api.openai.com"));
    }

    #[test]
    fn vault_rows_are_not_a_fallback_for_code_workflows() {
        let (_dir, service) = service();
        service
            .create(
                "Personal OpenAI",
                "openai",
                "personal/development/providers",
                "sk-vault-must-not-authorize",
                "test",
            )
            .unwrap();
        let error = service
            .workload_env(
                "openai",
                "run_vault",
                "gepa.craftax.smoke.v1",
                ProviderUsePolicy::default(),
                "test",
            )
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("credential_value_missing")
                || error.contains("credential_value_unloaded"),
            "vault secret must not authorize a code workflow: {error}"
        );
        assert!(!error.contains("sk-vault"));
    }

    #[test]
    fn stale_memory_descriptor_is_not_a_live_connection() {
        let (dir, service) = service();
        let env_file = dir.path().join(".env");
        service
            .load_test_env_source("openai", "OPENAI_API_KEY", "sk-live-then-gone", &env_file)
            .unwrap();
        service.env_sources.clear();
        let error = service
            .issue_lease(
                "openai",
                "run_stale",
                "eval.craftax.llm-policy.smoke.v1",
                ProviderUsePolicy::default(),
                "test",
            )
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("credential_value_unloaded")
                || error.contains("credential_value_missing"),
            "unloaded descriptor must not look valid: {error}"
        );
    }

    #[test]
    fn managed_recipe_cannot_select_byok() {
        let error = lease::reject_managed_byok("gepa.craftax.smoke.v1", Some("byok"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("managed_byok_rejected"));
    }

    #[test]
    fn lease_compiler_never_emits_the_real_key() {
        let (dir, service) = service();
        let env_file = dir.path().join(".env");
        service
            .load_test_env_source(
                "openai",
                "OPENAI_API_KEY",
                "sk-real-must-not-leak",
                &env_file,
            )
            .unwrap();
        let lease = service
            .issue_lease(
                "openai",
                "run_compile",
                "gepa.craftax.smoke.v1",
                ProviderUsePolicy::default(),
                "test",
            )
            .unwrap();
        lease.assert_managed_proxy().unwrap();
        assert_eq!(lease.credential_mode, lease::CREDENTIAL_MODE_WORKSHOP_PROXY);
        assert_eq!(lease.api_key_sentinel, API_KEY_SENTINEL);
        let dump = format!(
            "{:?}{:?}{:?}",
            lease.compile_host_env(),
            lease.compile_container_env(),
            lease.compile_eval_manifest_patch()
        );
        assert!(!dump.contains("sk-real-must-not-leak"));
        assert!(dump.contains(API_KEY_SENTINEL));
        assert!(lease.inference_url.contains("host.docker.internal"));
        assert!(!lease.inference_url.contains("api.openai.com"));
    }

    #[test]
    fn default_backend_never_opens_keychain() {
        assert_eq!(backend::default_backend().backend_name(), "memory");
    }

    #[test]
    fn classify_upstream_status_is_typed() {
        assert_eq!(
            lease::classify_upstream_status(401),
            lease::PROVIDER_AUTH_REJECTED
        );
        assert_eq!(
            lease::classify_upstream_status(429),
            lease::PROVIDER_RATE_LIMITED
        );
        assert_eq!(
            lease::classify_upstream_status(503),
            lease::PROVIDER_UNAVAILABLE
        );
    }

    #[test]
    fn restart_does_not_restore_in_memory_leases() {
        let (dir, service) = service();
        let env_file = dir.path().join(".env");
        service
            .load_test_env_source("openai", "OPENAI_API_KEY", "sk-restart", &env_file)
            .unwrap();
        let lease = service
            .issue_lease(
                "openai",
                "run_restart",
                "eval.craftax.llm-policy.smoke.v1",
                ProviderUsePolicy::default(),
                "test",
            )
            .unwrap();
        let restarted = SecretsService::with_backend(
            service.db.clone(),
            Arc::new(backend::MemoryBackend::new()),
        );
        assert!(restarted
            .env_sources
            .get(&lease::env_backend_ref("openai", "OPENAI_API_KEY"))
            .is_none());
        let error = restarted
            .issue_lease(
                "openai",
                "run_restart_2",
                "eval.craftax.llm-policy.smoke.v1",
                ProviderUsePolicy::default(),
                "test",
            )
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("credential_value_unloaded")
                || error.contains("credential_value_missing"),
            "restarted process must not reuse the old in-memory lease: {error}"
        );
        let _ = lease;
    }

    #[test]
    fn preflight_probes_host_and_container_route() {
        let (dir, service) = service();
        let env_file = dir.path().join(".env");
        service
            .load_test_env_source("openai", "OPENAI_API_KEY", "sk-preflight", &env_file)
            .unwrap();
        let receipt = service
            .preflight_openai_route(
                "eval.craftax.llm-policy.smoke.v1",
                ProviderUsePolicy::default(),
            )
            .unwrap();
        assert!(receipt.proxy_reachable);
        assert!(receipt.container_reachable);
        assert!(receipt.capability_policy_verified);
        assert!(receipt.lease_digest.is_some());
        assert!(!format!("{receipt:?}").contains("sk-preflight"));
    }

    #[test]
    fn issue_lease_records_a_receipt_chain_without_the_key() {
        let (dir, service) = service();
        let env_file = dir.path().join(".env");
        service
            .load_test_env_source("openai", "OPENAI_API_KEY", "sk-chain-secret", &env_file)
            .unwrap();
        let lease = service
            .issue_lease(
                "openai",
                "run_chain",
                "eval.craftax.llm-policy.smoke.v1",
                ProviderUsePolicy::default(),
                "test",
            )
            .unwrap();
        let chain = service.chain_for_run("run_chain").expect("chain recorded");
        let dump = chain.to_string();
        assert!(!dump.contains("sk-chain-secret"));
        assert!(!dump.contains(&lease.capability_handle));
        assert_eq!(chain["leaseDigest"], json!(lease.digest()));
        assert_eq!(chain["capabilityStatus"], json!("granted"));
        assert_eq!(chain["capabilityRevoked"], json!(false));
        service
            .capabilities
            .reserve_call(&lease.capability_handle)
            .unwrap();
        service
            .capabilities
            .debit_usage(
                &lease.capability_handle,
                &capability::MeasuredUsage {
                    calls: 1,
                    input_tokens: 123,
                    output_tokens: 45,
                    cost_usd: Some(0.012345),
                },
            )
            .unwrap();
        let sealed = service
            .seal_run_chain("run_chain")
            .unwrap()
            .expect("sealed chain");
        assert_eq!(sealed["capabilityRevoked"], json!(true));
        assert_eq!(sealed["providerUsage"]["requestAttempts"], json!(1));
        assert_eq!(sealed["providerUsage"]["inputTokens"], json!(123));
        assert_eq!(sealed["providerUsage"]["outputTokens"], json!(45));
        assert_eq!(sealed["providerUsage"]["totalTokens"], json!(168));
        assert_eq!(sealed["providerUsage"]["costUsd"], json!(0.012345));
        assert_eq!(
            sealed["providerUsage"]["basis"],
            json!("workshop_provider_proxy_reserved_requests")
        );
        assert_eq!(sealed["providerUsage"]["requestCountComplete"], json!(true));
        assert_eq!(sealed["capabilityStatus"], json!("revoked"));
        assert!(sealed
            .get("revokedAt")
            .and_then(|value| value.as_str())
            .is_some());
    }

    #[test]
    fn exhausted_capability_remains_authoritative_when_run_chain_is_sealed() {
        let (dir, service) = service();
        let source_id = service
            .db
            .transaction(|conn| {
                lease::upsert_env_source_descriptor(
                    conn,
                    "openai",
                    "OPENAI_API_KEY",
                    &dir.path().join("not-loaded.env"),
                    None,
                    None,
                    false,
                )
            })
            .unwrap();
        let policy = ProviderUsePolicy {
            max_calls: 1,
            ..ProviderUsePolicy::default()
        };
        let issued = service
            .db
            .transaction(|conn| {
                capability::issue(
                    conn,
                    &service.capabilities,
                    &source_id,
                    "run_exhausted_chain",
                    "eval.craftax.llm-policy.smoke.v1",
                    "openai",
                    &policy,
                    "test",
                )
            })
            .unwrap();
        let reserved = service
            .capabilities
            .reserve_call(&issued.handle)
            .expect("reserve the only allowed call");
        let exhausted = service
            .capabilities
            .debit_usage(
                &issued.handle,
                &capability::MeasuredUsage {
                    calls: 1,
                    input_tokens: 0,
                    output_tokens: 0,
                    cost_usd: None,
                },
            )
            .unwrap();
        assert_eq!(reserved.used_calls, 1);
        assert_eq!(exhausted.status, "exhausted");
        service
            .db
            .with_conn(|conn| capability::persist_usage(conn, &exhausted))
            .unwrap();
        let mut stale_revoked_copy = exhausted.clone();
        stale_revoked_copy.status = capability::CapabilityStatus::Revoked.as_str().into();
        service
            .db
            .with_conn(|conn| capability::persist_usage(conn, &stale_revoked_copy))
            .unwrap();
        service.chains.lock().unwrap().insert(
            "run_exhausted_chain".into(),
            json!({
                "schemaVersion": lease::CHAIN_SCHEMA,
                "capabilityId": issued.id,
                "capabilityStatus": "granted",
                "revokedAt": null,
                "capabilityRevoked": false,
            }),
        );

        let sealed = service
            .seal_run_chain("run_exhausted_chain")
            .unwrap()
            .expect("sealed chain");
        let lifecycle = service
            .durable_capability_lifecycle(&issued.id)
            .unwrap()
            .expect("durable lifecycle");

        assert_eq!(lifecycle.status, capability::CapabilityStatus::Exhausted);
        assert_eq!(lifecycle.revoked_at, None);
        assert_eq!(sealed["capabilityStatus"], json!("exhausted"));
        assert_eq!(sealed["capabilityRevoked"], json!(false));
        assert_eq!(sealed["revokedAt"], json!(null));
        assert_eq!(
            service.capabilities.lookup(&issued.handle).unwrap().status,
            "exhausted",
            "run sealing must not copy revoked over an exhausted live capability"
        );
    }

    /// Failure-scoped preparation guard: an armed drop revokes every
    /// capability issued for the failed run; a disarmed drop leaves the
    /// prepared run's capability owned. Uses the process-global live service
    /// (whichever test installed it first) with unique run ids, since the
    /// guard revokes through `live()` exactly as production failure paths do.
    #[test]
    fn a_failure_guard_revokes_on_drop_unless_ownership_was_transferred() {
        let (dir, service) = service();
        let env_file = dir.path().join(".env");
        service
            .load_test_env_source("openai", "OPENAI_API_KEY", "sk-guard-secret", &env_file)
            .unwrap();
        install_live(Arc::new(service));
        let service = live().expect("live secrets service");
        if service
            .list(Some("openai"), None)
            .map(|secrets| secrets.is_empty())
            .unwrap_or(true)
        {
            let _ = service.load_test_env_source(
                "openai",
                "OPENAI_API_KEY",
                "sk-guard-secret",
                &env_file,
            );
        }
        let active_for = |run: &str| {
            service
                .active_capabilities()
                .unwrap()
                .into_iter()
                .filter(|capability| capability.run_id == run)
                .count()
        };

        service
            .issue_lease(
                "openai",
                "run_guard_armed",
                "gepa.banking77.local.l.v1",
                ProviderUsePolicy::default(),
                "test",
            )
            .unwrap();
        assert_eq!(active_for("run_guard_armed"), 1);
        drop(RevokeRunOnFailure::new("run_guard_armed"));
        assert_eq!(
            active_for("run_guard_armed"),
            0,
            "a failed pre-start path must leave no active capability"
        );
        let chain = service
            .chain_for_run("run_guard_armed")
            .expect("failed run keeps a sealed public chain");
        assert_eq!(chain["capabilityRevoked"], json!(true));
        assert!(chain["revokedAt"].as_str().is_some());

        service
            .issue_lease(
                "openai",
                "run_guard_disarmed",
                "gepa.banking77.local.l.v1",
                ProviderUsePolicy::default(),
                "test",
            )
            .unwrap();
        RevokeRunOnFailure::new("run_guard_disarmed").disarm();
        assert_eq!(
            active_for("run_guard_disarmed"),
            1,
            "a successful prepare keeps exactly one owned capability"
        );
        service.revoke_run("run_guard_disarmed").unwrap();
    }
}
