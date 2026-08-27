//! One credential contract for Workshop, Optimizers, and containers.
//!
//! `.env` is the authority. Workshop is the only process that reads it.
//! Workloads receive a short-lived `CredentialLease` that points at the
//! provider proxy and never contains the real key.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use super::backend::SecretBytes;
use super::capability::ProviderUsePolicy;
use super::fingerprint;
use super::proxy::{self, WorkloadEnv, API_KEY_SENTINEL};
use super::vault;
use super::SecretsService;

pub const LEASE_SCHEMA: &str = "workshop.credential-lease.v1";
pub const SOURCE_SCHEMA: &str = "workshop.credential-source.v1";
pub const RECEIPT_SCHEMA: &str = "workshop.credential-readiness-receipt.v1";
pub const CHAIN_SCHEMA: &str = "workshop.credential-chain.v1";
pub const RUNTIME_LEASE_SCHEMA: &str = "workshop.optimizer-runtime-lease.v1";
pub const CONTRACT: &str = "workshop.credential.v1";
pub const CREDENTIAL_MODE_WORKSHOP_PROXY: &str = "workshop_proxy";
pub const SOURCE_KIND_CONFIGURED_ENV: &str = "configured_env_file";
pub const BACKEND_CONFIGURED_ENV: &str = "configured_env_file";

pub const CREDENTIAL_SOURCE_UNCONFIGURED: &str = "credential_source_unconfigured";
pub const CREDENTIAL_VALUE_MISSING: &str = "credential_value_missing";
pub const CREDENTIAL_VALUE_UNLOADED: &str = "credential_value_unloaded";
pub const PROXY_NOT_RUNNING: &str = "proxy_not_running";
pub const PROXY_ROUTE_UNBOUND: &str = "proxy_route_unbound";
pub const PROXY_CONTAINER_UNREACHABLE: &str = "proxy_container_unreachable";
pub const CAPABILITY_DENIED: &str = "capability_denied";
pub const CAPABILITY_EXPIRED: &str = "capability_expired";
pub const PROVIDER_AUTH_REJECTED: &str = "provider_auth_rejected";
pub const PROVIDER_RATE_LIMITED: &str = "provider_rate_limited";
pub const PROVIDER_UNAVAILABLE: &str = "provider_unavailable";
pub const OPTIMIZER_RUNTIME_STALE: &str = "optimizer_runtime_stale";
pub const OPTIMIZER_RUNTIME_UNHEALTHY: &str = "optimizer_runtime_unhealthy";
pub const MANAGED_BYOK_REJECTED: &str = "managed_byok_rejected";

const CANONICAL_PROVIDER_VARS: &[(&str, &str)] = &[
    ("openai", "OPENAI_API_KEY"),
    ("openrouter", "OPENROUTER_API_KEY"),
    ("anthropic", "ANTHROPIC_API_KEY"),
    ("tinker", "TINKER_API_KEY"),
    ("groq", "GROQ_API_KEY"),
];

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialError {
    pub code: String,
    pub contract: String,
    pub layer: String,
    pub retryable: bool,
    pub message: String,
}

impl CredentialError {
    pub fn new(code: &str, layer: &str, retryable: bool, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            contract: CONTRACT.into(),
            layer: layer.into(),
            retryable,
            message: message.into(),
        }
    }

    pub fn anyhow(self) -> anyhow::Error {
        anyhow!("{}", serde_json::to_string(&self).unwrap_or(self.message))
    }
}

impl std::fmt::Display for CredentialError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}",
            serde_json::to_string(self).unwrap_or_else(|_| self.message.clone())
        )
    }
}

pub fn classify_upstream_status(status: u16) -> &'static str {
    match status {
        401 | 403 => PROVIDER_AUTH_REJECTED,
        429 => PROVIDER_RATE_LIMITED,
        500..=599 => PROVIDER_UNAVAILABLE,
        _ => "upstream_status",
    }
}

pub fn env_backend_ref(provider: &str, variable: &str) -> String {
    format!(
        "envsrc:{}:{}",
        provider.trim().to_ascii_lowercase(),
        variable.trim()
    )
}

pub fn canonical_variable(provider: &str) -> Option<&'static str> {
    let provider = provider.trim().to_ascii_lowercase();
    CANONICAL_PROVIDER_VARS
        .iter()
        .find(|(name, _)| *name == provider)
        .map(|(_, variable)| *variable)
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSourceDescriptor {
    pub schema_version: String,
    pub provider: String,
    pub source_kind: String,
    pub variable: String,
    pub configured: bool,
    pub loaded: bool,
    pub validated: bool,
    pub fingerprint: Option<String>,
    pub env_file: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialLease {
    pub schema_version: String,
    pub provider: String,
    pub credential_mode: String,
    pub capability_id: String,
    pub capability_handle: String,
    pub run_id: String,
    pub recipe_id: String,
    pub host_base_url: String,
    pub container_base_url: String,
    pub inference_url: String,
    pub api_key_sentinel: String,
    pub api_key_env: String,
    pub operations: Vec<String>,
    pub models: Vec<String>,
    pub max_calls: u32,
    pub max_cost_usd: f64,
    pub expires_at: Option<String>,
}

impl CredentialLease {
    pub fn compile_host_env(&self) -> Vec<(String, String)> {
        let mut env = vec![
            ("OPENAI_API_KEY".into(), self.api_key_sentinel.clone()),
            ("OPENAI_BASE_URL".into(), self.host_base_url.clone()),
            (
                "WORKSHOP_OPENAI_BASE_URL".into(),
                self.container_base_url.clone(),
            ),
            (
                "WORKSHOP_OPENAI_ROUTE".into(),
                format!(
                    "{}/chat/completions",
                    self.container_base_url.trim_end_matches('/')
                ),
            ),
            ("WORKSHOP_CAPABILITY".into(), self.capability_handle.clone()),
            ("WORKSHOP_RUN_ID".into(), self.run_id.clone()),
            (
                "WORKSHOP_CREDENTIAL_MODE".into(),
                self.credential_mode.clone(),
            ),
            ("WORKSHOP_INFERENCE_URL".into(), self.inference_url.clone()),
        ];
        if self.api_key_env != "OPENAI_API_KEY" {
            env.push((self.api_key_env.clone(), self.api_key_sentinel.clone()));
        }
        env
    }

    pub fn compile_container_env(&self) -> Vec<(String, String)> {
        let mut env = vec![
            ("OPENAI_API_KEY".into(), self.api_key_sentinel.clone()),
            ("OPENAI_BASE_URL".into(), self.container_base_url.clone()),
            (
                "WORKSHOP_OPENAI_BASE_URL".into(),
                self.container_base_url.clone(),
            ),
            (
                "WORKSHOP_OPENAI_ROUTE".into(),
                format!(
                    "{}/chat/completions",
                    self.container_base_url.trim_end_matches('/')
                ),
            ),
            (
                "EVAL_LLM_ROUTE".into(),
                format!(
                    "{}/chat/completions",
                    self.container_base_url.trim_end_matches('/')
                ),
            ),
            (
                "WORKSHOP_CREDENTIAL_MODE".into(),
                self.credential_mode.clone(),
            ),
        ];
        if self.api_key_env != "OPENAI_API_KEY" {
            env.push((self.api_key_env.clone(), self.api_key_sentinel.clone()));
        }
        env
    }

    pub fn compile_policy_toml(&self) -> serde_json::Value {
        serde_json::json!({
            "provider": self.provider,
            "credential_mode": self.credential_mode,
            "inference_url": self.inference_url,
            "api_key_env": self.api_key_env,
        })
    }

    pub fn compile_eval_manifest_patch(&self) -> serde_json::Value {
        serde_json::json!({
            "credential_mode": self.credential_mode,
            "inference_url": self.inference_url,
            "credential_lease": self,
            "provider_routes": self.compile_provider_routes(),
        })
    }

    pub fn compile_provider_routes(&self) -> serde_json::Value {
        let completions = format!(
            "{}/chat/completions",
            self.container_base_url.trim_end_matches('/')
        );
        serde_json::json!({
            "openai": completions,
            "openai_base": self.container_base_url,
            "auth": "capability_path",
            "api_key_sentinel": self.api_key_sentinel,
            "container_host": proxy::container_proxy_host(),
            "extra_hosts": ["host.docker.internal:host-gateway"],
        })
    }

    pub fn to_workload_env(
        &self,
        capability_file: Option<String>,
        proxy_socket: Option<String>,
    ) -> WorkloadEnv {
        WorkloadEnv {
            openai_base_url: Some(self.host_base_url.clone()),
            anthropic_base_url: None,
            openai_api_key: self.api_key_sentinel.clone(),
            capability_handle: self.capability_handle.clone(),
            capability_file,
            workshop_run_id: self.run_id.clone(),
            capability_id: self.capability_id.clone(),
            openai_route: Some(format!(
                "{}/chat/completions",
                self.host_base_url.trim_end_matches('/')
            )),
            container_openai_base_url: Some(self.container_base_url.clone()),
            container_openai_route: Some(format!(
                "{}/chat/completions",
                self.container_base_url.trim_end_matches('/')
            )),
            proxy_socket,
        }
    }

    pub fn digest(&self) -> String {
        let body = serde_json::to_vec(self).unwrap_or_default();
        format!("sha256:{:x}", Sha256::digest(&body))
    }

    pub fn assert_managed_proxy(&self) -> Result<()> {
        if self.credential_mode != CREDENTIAL_MODE_WORKSHOP_PROXY {
            return Err(CredentialError::new(
                MANAGED_BYOK_REJECTED,
                "admission",
                false,
                format!(
                    "managed recipe {} cannot use credential_mode={}",
                    self.recipe_id, self.credential_mode
                ),
            )
            .anyhow());
        }
        if self.api_key_sentinel != API_KEY_SENTINEL {
            return Err(CredentialError::new(
                CAPABILITY_DENIED,
                "lease",
                false,
                "lease sentinel is not the public workshop-proxy value",
            )
            .anyhow());
        }
        if looks_like_provider_origin(&self.inference_url)
            || looks_like_provider_origin(&self.container_base_url)
            || looks_like_provider_origin(&self.host_base_url)
        {
            return Err(CredentialError::new(
                PROXY_ROUTE_UNBOUND,
                "route",
                false,
                "lease inference_url must be the Workshop proxy, not the upstream provider",
            )
            .anyhow());
        }
        if looks_like_loopback(&self.container_base_url) || looks_like_loopback(&self.inference_url)
        {
            return Err(CredentialError::new(
                PROXY_CONTAINER_UNREACHABLE,
                "route",
                false,
                "container inference_url still points at loopback",
            )
            .anyhow());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialReadinessReceipt {
    pub schema_version: String,
    pub provider: String,
    pub source: String,
    pub route: String,
    pub proxy_reachable: bool,
    #[serde(default)]
    pub container_reachable: bool,
    pub credential_resolved: bool,
    pub provider_authenticated: Option<bool>,
    pub capability_policy_verified: bool,
    pub lease_digest: Option<String>,
    pub source_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerRuntimeLease {
    pub schema_version: String,
    pub pid: u32,
    pub process_start_identity: String,
    #[serde(default)]
    pub process_group_id: Option<u32>,
    pub service_url: String,
    pub database_digest: String,
    pub instance_id: String,
    #[serde(default)]
    pub boot_epoch: String,
    pub version: String,
    #[serde(default)]
    pub digest: String,
    #[serde(default)]
    pub runtime_epoch: String,
    pub started_at: String,
}

/// Live `.env` values keyed by deterministic `envsrc:{provider}:{variable}`.
/// Never written to SQLite. Reconstructed from config on every start.
#[derive(Default)]
pub struct EnvSourceStore {
    values: Mutex<HashMap<String, Vec<u8>>>,
}

impl EnvSourceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&self, backend_ref: &str, value: &SecretBytes) {
        self.values
            .lock()
            .expect("env source store")
            .insert(backend_ref.to_owned(), value.0.clone());
    }

    pub fn get(&self, backend_ref: &str) -> Option<SecretBytes> {
        self.values
            .lock()
            .expect("env source store")
            .get(backend_ref)
            .cloned()
            .map(SecretBytes)
    }

    pub fn contains(&self, backend_ref: &str) -> bool {
        self.values
            .lock()
            .expect("env source store")
            .contains_key(backend_ref)
    }

    pub fn clear(&self) {
        self.values.lock().expect("env source store").clear();
    }
}

pub fn provider_variable_from_config(document: &toml::Value, provider: &str) -> Result<String> {
    let provider = provider.trim().to_ascii_lowercase();
    let mapped = document
        .get("credentials")
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("providers"))
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get(provider.as_str()))
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("variable"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if let Some(variable) = mapped {
        return Ok(variable);
    }
    canonical_variable(&provider)
        .map(str::to_owned)
        .ok_or_else(|| {
            CredentialError::new(
                CREDENTIAL_SOURCE_UNCONFIGURED,
                "config",
                false,
                format!("no credentials.providers.{provider} variable mapping in config.toml"),
            )
            .anyhow()
        })
}

pub fn read_env_file_value(path: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .find_map(|line| {
            let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
            if line.starts_with('#') || line.is_empty() {
                return None;
            }
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then(|| value.trim().trim_matches(['\'', '"']).to_owned())
        })
        .filter(|value| !value.is_empty())
}

pub fn upsert_env_source_descriptor(
    conn: &rusqlite::Connection,
    provider: &str,
    variable: &str,
    env_file: &Path,
    fingerprint: Option<&str>,
    loaded: bool,
) -> Result<String> {
    let id = format!(
        "envsrc_{}",
        hex_short(&format!("{provider}:{variable}:{}", env_file.display()))
    );
    let backend_ref = env_backend_ref(provider, variable);
    let now = chrono::Utc::now().to_rfc3339();
    let alias = format!("config.toml {variable}");
    let status = if loaded { "valid" } else { "invalid" };
    let digest = fingerprint.unwrap_or("sha256:unloaded");
    conn.execute(
        "INSERT INTO secret_refs(
            id, alias, provider, scope, backend, backend_ref, fingerprint,
            display_suffix, status, created_at, updated_at, last_validated_at
        ) VALUES (?1,?2,?3,'project/config/env',?4,?5,?6,'',?7,?8,?8,?9)
         ON CONFLICT(backend_ref) DO UPDATE SET
            alias=excluded.alias,
            fingerprint=excluded.fingerprint,
            status=excluded.status,
            updated_at=excluded.updated_at,
            last_validated_at=excluded.last_validated_at",
        rusqlite::params![
            id,
            alias,
            provider.trim().to_ascii_lowercase(),
            BACKEND_CONFIGURED_ENV,
            backend_ref,
            digest,
            status,
            now,
            loaded.then_some(now.clone()),
        ],
    )?;
    let stored: String = conn.query_row(
        "SELECT id FROM secret_refs WHERE backend_ref=?1",
        [&backend_ref],
        |row| row.get(0),
    )?;
    let _ = env_file;
    let _ = variable;
    Ok(stored)
}

fn hex_short(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn looks_like_loopback(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("127.0.0.1") || lower.contains("localhost") || lower.contains("[::1]")
}

fn looks_like_provider_origin(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("api.openai.com")
        || lower.contains("openrouter.ai")
        || lower.contains("api.anthropic.com")
        || lower.contains("api.groq.com")
}

impl SecretsService {
    pub fn configured_source_store(&self) -> &EnvSourceStore {
        self.env_sources.as_ref()
    }

    pub fn load_configured_env_sources(&self) -> Result<Vec<CredentialSourceDescriptor>> {
        self.env_sources.clear();
        let resolved = crate::synth_config::resolve().ok();
        let env_file = resolved
            .as_ref()
            .map(|backend| backend.env_file.clone())
            .unwrap_or_else(|| crate::instance::state_root().join(".env"));
        let document = resolved
            .as_ref()
            .and_then(|backend| std::fs::read_to_string(&backend.config_path).ok())
            .and_then(|text| text.parse::<toml::Value>().ok())
            .unwrap_or(toml::Value::Table(toml::map::Map::new()));
        let mut descriptors = Vec::new();
        for (provider, default_var) in CANONICAL_PROVIDER_VARS {
            let variable = provider_variable_from_config(&document, provider)
                .unwrap_or_else(|_| (*default_var).to_owned());
            descriptors.push(self.load_one_env_source(provider, &variable, &env_file)?);
        }
        Ok(descriptors)
    }

    pub fn load_one_env_source(
        &self,
        provider: &str,
        variable: &str,
        env_file: &Path,
    ) -> Result<CredentialSourceDescriptor> {
        let backend_ref = env_backend_ref(provider, variable);
        let value = read_env_file_value(env_file, variable);
        let loaded = value.is_some();
        let fingerprint = value
            .as_ref()
            .map(|secret| fingerprint::fingerprint(&SecretBytes::from_utf8(secret)));
        if let Some(secret) = value.as_ref() {
            let bytes = SecretBytes::from_utf8(secret);
            self.env_sources.put(&backend_ref, &bytes);
            self.redaction.register(&bytes);
        }
        let descriptor = CredentialSourceDescriptor {
            schema_version: SOURCE_SCHEMA.into(),
            provider: provider.to_ascii_lowercase(),
            source_kind: SOURCE_KIND_CONFIGURED_ENV.into(),
            variable: variable.into(),
            configured: true,
            loaded,
            validated: loaded,
            fingerprint: fingerprint.clone(),
            env_file: env_file.display().to_string(),
        };
        self.db.with_conn(|conn| {
            upsert_env_source_descriptor(
                conn,
                provider,
                variable,
                env_file,
                fingerprint.as_deref(),
                loaded,
            )
        })?;
        Ok(descriptor)
    }

    #[cfg(test)]
    pub fn load_test_env_source(
        &self,
        provider: &str,
        variable: &str,
        value: &str,
        env_file: &Path,
    ) -> Result<CredentialSourceDescriptor> {
        std::fs::write(env_file, format!("{variable}={value}\n"))?;
        self.load_one_env_source(provider, variable, env_file)
    }

    pub fn configured_source(&self, provider: &str) -> Result<CredentialSourceDescriptor> {
        let variable = self.configured_variable(provider)?;
        let env_file = crate::synth_config::resolve()
            .map(|backend| backend.env_file)
            .unwrap_or_else(|_| crate::instance::state_root().join(".env"));
        let backend_ref = env_backend_ref(provider, &variable);
        let loaded = self.env_sources.contains(&backend_ref);
        let fingerprint = self
            .env_sources
            .get(&backend_ref)
            .map(|bytes| fingerprint::fingerprint(&bytes));
        Ok(CredentialSourceDescriptor {
            schema_version: SOURCE_SCHEMA.into(),
            provider: provider.to_ascii_lowercase(),
            source_kind: SOURCE_KIND_CONFIGURED_ENV.into(),
            variable,
            configured: true,
            loaded,
            validated: loaded,
            fingerprint,
            env_file: env_file.display().to_string(),
        })
    }

    fn configured_variable(&self, provider: &str) -> Result<String> {
        let document = crate::synth_config::resolve()
            .ok()
            .and_then(|backend| std::fs::read_to_string(&backend.config_path).ok())
            .and_then(|text| text.parse::<toml::Value>().ok())
            .unwrap_or(toml::Value::Table(toml::map::Map::new()));
        provider_variable_from_config(&document, provider)
    }

    pub fn issue_lease(
        &self,
        provider: &str,
        run_id: &str,
        recipe_id: &str,
        policy: ProviderUsePolicy,
        actor: &str,
    ) -> Result<CredentialLease> {
        reject_managed_byok(recipe_id, None)?;
        let variable = self.configured_variable(provider)?;
        let backend_ref = env_backend_ref(provider, &variable);
        if !self.env_sources.contains(&backend_ref) {
            let env_file = crate::synth_config::resolve()
                .map(|backend| backend.env_file.display().to_string())
                .unwrap_or_else(|_| "<unresolved env_file>".into());
            let descriptor_exists = self
                .db
                .with_conn(|conn| vault::find_configured_source(conn, provider))
                .ok()
                .flatten()
                .is_some();
            let code = if descriptor_exists {
                CREDENTIAL_VALUE_UNLOADED
            } else {
                CREDENTIAL_VALUE_MISSING
            };
            return Err(CredentialError::new(
                code,
                "source",
                false,
                format!("{variable} is missing from the config-declared env file {env_file}"),
            )
            .anyhow());
        }
        let origin = self.start_proxy().map_err(|error| {
            CredentialError::new(
                PROXY_NOT_RUNNING,
                "proxy",
                true,
                format!("Workshop provider proxy did not start: {error}"),
            )
            .anyhow()
        })?;
        let secret_id = self
            .db
            .with_conn(|conn| vault::find_configured_source(conn, provider))?
            .map(|record| record.id)
            .ok_or_else(|| {
                CredentialError::new(
                    CREDENTIAL_VALUE_UNLOADED,
                    "source",
                    false,
                    format!("configured {provider} descriptor is not loaded in this process"),
                )
                .anyhow()
            })?;
        let granted = self
            .grant_use(&secret_id, run_id, recipe_id, policy.clone(), actor, false)
            .map_err(|error| {
                CredentialError::new(
                    CAPABILITY_DENIED,
                    "capability",
                    false,
                    format!("could not issue a run-scoped {provider} capability: {error}"),
                )
                .anyhow()
            })?;
        let handle = granted.handle.clone().ok_or_else(|| {
            CredentialError::new(
                CAPABILITY_DENIED,
                "capability",
                false,
                "capability handle missing",
            )
            .anyhow()
        })?;
        let host_base_url = proxy::capability_base_url(&origin, &handle, provider);
        let container_origin = proxy::rewrite_origin_for_containers(&origin);
        let container_base_url = proxy::capability_base_url(&container_origin, &handle, provider);
        let lease = CredentialLease {
            schema_version: LEASE_SCHEMA.into(),
            provider: provider.to_ascii_lowercase(),
            credential_mode: CREDENTIAL_MODE_WORKSHOP_PROXY.into(),
            capability_id: granted.capability_id.unwrap_or_default(),
            capability_handle: handle,
            run_id: run_id.into(),
            recipe_id: recipe_id.into(),
            host_base_url,
            container_base_url: container_base_url.clone(),
            inference_url: container_base_url,
            api_key_sentinel: API_KEY_SENTINEL.into(),
            api_key_env: variable,
            operations: policy.operations.clone(),
            models: policy.models.clone(),
            max_calls: policy.max_calls,
            max_cost_usd: policy.max_cost_usd,
            expires_at: granted
                .summary
                .as_ref()
                .map(|summary| summary.expires_at.clone()),
        };
        lease.assert_managed_proxy()?;
        if !run_id.starts_with("preflight_") {
            let source = self.configured_source(provider).ok();
            self.record_run_chain(&lease, source.as_ref())?;
        }
        Ok(lease)
    }

    pub fn preflight_openai_route(
        &self,
        recipe_id: &str,
        policy: ProviderUsePolicy,
    ) -> Result<CredentialReadinessReceipt> {
        reject_managed_byok(recipe_id, None)?;
        let source = self.configured_source("openai")?;
        if !source.configured {
            return Err(CredentialError::new(
                CREDENTIAL_SOURCE_UNCONFIGURED,
                "config",
                false,
                "config.toml does not map openai to an env variable",
            )
            .anyhow());
        }
        if !source.loaded {
            return Err(CredentialError::new(
                CREDENTIAL_VALUE_MISSING,
                "source",
                false,
                format!(
                    "{} is absent from the config-declared env file {}",
                    source.variable, source.env_file
                ),
            )
            .anyhow());
        }
        let preflight_run = format!("preflight_{recipe_id}");
        let lease = match self.issue_lease("openai", &preflight_run, recipe_id, policy, "admission")
        {
            Ok(lease) => lease,
            Err(error) => {
                let _ = self.revoke_run(&preflight_run);
                return Err(error);
            }
        };
        let host_ok = probe_capability_self(&lease.host_base_url, &lease.capability_handle);
        let backend_ref = env_backend_ref("openai", &lease.api_key_env);
        let credential_resolved = self.env_sources.contains(&backend_ref);
        let container_ok = match probe_container_route(&lease) {
            Ok(ok) => ok,
            Err(error) => {
                let _ = self.revoke_run(&preflight_run);
                return Err(error);
            }
        };
        let receipt = CredentialReadinessReceipt {
            schema_version: RECEIPT_SCHEMA.into(),
            provider: "openai".into(),
            source: "config_env".into(),
            route: "container_proxy".into(),
            proxy_reachable: host_ok,
            container_reachable: container_ok,
            credential_resolved,
            provider_authenticated: None,
            capability_policy_verified: host_ok && credential_resolved && container_ok,
            lease_digest: Some(lease.digest()),
            source_fingerprint: source.fingerprint.clone(),
        };
        let _ = self.revoke_run(&preflight_run);
        if !receipt.proxy_reachable {
            return Err(CredentialError::new(
                PROXY_NOT_RUNNING,
                "proxy",
                true,
                "Workshop proxy did not answer the host readiness probe",
            )
            .anyhow());
        }
        if !receipt.credential_resolved {
            return Err(CredentialError::new(
                CREDENTIAL_VALUE_UNLOADED,
                "source",
                false,
                "proxy cannot resolve the in-memory configured credential",
            )
            .anyhow());
        }
        if !receipt.container_reachable {
            return Err(CredentialError::new(
                PROXY_CONTAINER_UNREACHABLE,
                "route",
                false,
                "container inference_url is not reachable from a real container network",
            )
            .anyhow());
        }
        self.remember_readiness(recipe_id, receipt.clone());
        Ok(receipt)
    }

    pub fn remember_readiness(&self, recipe_id: &str, receipt: CredentialReadinessReceipt) {
        self.readiness
            .lock()
            .expect("credential readiness")
            .insert(recipe_id.to_owned(), receipt);
    }

    pub fn take_readiness(&self, recipe_id: &str) -> Option<CredentialReadinessReceipt> {
        self.readiness
            .lock()
            .expect("credential readiness")
            .remove(recipe_id)
    }

    pub fn chain_for_run(&self, run_id: &str) -> Option<Value> {
        self.chains
            .lock()
            .expect("credential chains")
            .get(run_id)
            .cloned()
    }

    fn record_run_chain(
        &self,
        lease: &CredentialLease,
        source: Option<&CredentialSourceDescriptor>,
    ) -> Result<()> {
        let readiness = self.take_readiness(&lease.recipe_id);
        let chain = json!({
            "schemaVersion": CHAIN_SCHEMA,
            "provider": lease.provider,
            "sourceKind": source.map(|item| item.source_kind.clone()).unwrap_or_else(|| SOURCE_KIND_CONFIGURED_ENV.to_string()),
            "sourceFingerprint": source.and_then(|item| item.fingerprint.clone()),
            "readiness": readiness,
            "leaseDigest": lease.digest(),
            "capabilityId": lease.capability_id,
            "credentialMode": lease.credential_mode,
            "inferenceUrlDigest": format!("sha256:{:x}", Sha256::digest(lease.inference_url.as_bytes())),
            "workloadManifestDigest": format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&lease.compile_eval_manifest_patch()).unwrap_or_default())),
            "routeKind": "container_proxy",
            "issuedAt": chrono::Utc::now().to_rfc3339(),
            "revokedAt": Value::Null,
            "capabilityRevoked": false,
        });
        self.chains
            .lock()
            .expect("credential chains")
            .insert(lease.run_id.clone(), chain);
        Ok(())
    }

    pub fn seal_run_chain(&self, run_id: &str) -> Result<Option<Value>> {
        let _ = self.revoke_run(run_id);
        let mut chains = self.chains.lock().expect("credential chains");
        let Some(mut chain) = chains.get(run_id).cloned() else {
            return Ok(None);
        };
        if let Some(object) = chain.as_object_mut() {
            object.insert("revokedAt".into(), json!(chrono::Utc::now().to_rfc3339()));
            object.insert("capabilityRevoked".into(), json!(true));
        }
        chains.insert(run_id.to_string(), chain.clone());
        Ok(Some(chain))
    }
}

pub fn reject_managed_byok(recipe_id: &str, credential_mode: Option<&str>) -> Result<()> {
    let mode = credential_mode
        .map(str::trim)
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    if mode == "byok" {
        return Err(CredentialError::new(
            MANAGED_BYOK_REJECTED,
            "admission",
            false,
            format!("managed recipe {recipe_id} cannot select credential_mode=byok"),
        )
        .anyhow());
    }
    Ok(())
}

pub fn bind_lease_into_toml(path: &Path, lease: &CredentialLease) -> Result<()> {
    lease.assert_managed_proxy()?;
    let text = std::fs::read_to_string(path)
        .with_context_path(path, "read optimizer config for credential lease")?;
    let mut document: toml::Value = text
        .parse()
        .with_context_path(path, "parse optimizer config for credential lease")?;
    let policy = document
        .as_table_mut()
        .and_then(|table| table.get_mut("policy"))
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| {
            CredentialError::new(
                PROXY_ROUTE_UNBOUND,
                "route",
                false,
                format!("{} is missing [policy]", path.display()),
            )
            .anyhow()
        })?;
    policy.insert(
        "credential_mode".into(),
        // The lease contract calls this route `workshop_proxy`; the pinned
        // optimizer TOML schema calls the same managed route `proxy`.
        toml::Value::String("proxy".into()),
    );
    policy.insert(
        "inference_url".into(),
        toml::Value::String(lease.inference_url.clone()),
    );
    policy.insert(
        "api_key_env".into(),
        toml::Value::String(lease.api_key_env.clone()),
    );
    policy.remove("base_url");
    std::fs::write(path, toml::to_string_pretty(&document)?)
        .with_context_path(path, "write optimizer credential lease")?;
    Ok(())
}

trait WithPathContext<T> {
    fn with_context_path(self, path: &Path, what: &str) -> Result<T>;
}

impl<T, E: Into<anyhow::Error>> WithPathContext<T> for std::result::Result<T, E> {
    fn with_context_path(self, path: &Path, what: &str) -> Result<T> {
        self.map_err(|error| {
            let error = error.into();
            anyhow!("{} {}: {error}", what, path.display())
        })
    }
}

fn probe_capability_self(host_base_url: &str, handle: &str) -> bool {
    probe_http_get(host_base_url, handle)
}

fn probe_http_get(base_url: &str, handle: &str) -> bool {
    let Some(rest) = base_url.strip_prefix("http://") else {
        return false;
    };
    let Some((hostport, _)) = rest.split_once('/') else {
        return false;
    };
    let (host, port): (&str, u16) = match hostport.split_once(':') {
        Some((host, port)) => (host, port.parse().unwrap_or(80)),
        None => (hostport, 80),
    };
    let path = format!("/cap/{handle}/v1/capabilities/self");
    let Ok(addr) = format!("{host}:{port}").parse::<SocketAddr>() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAuthorization: Bearer {handle}\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut buf = String::new();
    let _ = stream.read_to_string(&mut buf);
    buf.contains("HTTP/1.1 200") || buf.contains("HTTP/1.0 200")
}

fn capability_self_url(base_url: &str, handle: &str) -> Option<String> {
    let rest = base_url.strip_prefix("http://")?;
    let (hostport, _) = rest.split_once('/')?;
    Some(format!(
        "http://{hostport}/cap/{handle}/v1/capabilities/self"
    ))
}

fn probe_container_route(lease: &CredentialLease) -> Result<bool> {
    lease.assert_managed_proxy()?;
    if cfg!(test) {
        let rewritten = lease
            .container_base_url
            .replace("host.docker.internal", "127.0.0.1");
        return Ok(probe_capability_self(&rewritten, &lease.capability_handle));
    }
    probe_container_via_docker(&lease.container_base_url, &lease.capability_handle)
}

fn probe_container_via_docker(container_base_url: &str, handle: &str) -> Result<bool> {
    let url = capability_self_url(container_base_url, handle).ok_or_else(|| {
        CredentialError::new(
            PROXY_CONTAINER_UNREACHABLE,
            "route",
            false,
            "container inference_url is not an HTTP Workshop proxy origin",
        )
        .anyhow()
    })?;
    let docker_ok = Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !docker_ok {
        return Err(CredentialError::new(
            PROXY_CONTAINER_UNREACHABLE,
            "route",
            true,
            "docker is required to prove the container proxy route before a paid run",
        )
        .anyhow());
    }
    for image in [
        "busybox:1.36",
        "busybox:latest",
        "alpine:3.20",
        "alpine:latest",
    ] {
        if docker_wget(image, handle, &url, true) {
            return Ok(true);
        }
    }
    if docker_wget("busybox:1.36", handle, &url, false) {
        return Ok(true);
    }
    Ok(false)
}

fn docker_wget(image: &str, handle: &str, url: &str, pull_never: bool) -> bool {
    let mut command = Command::new("docker");
    command
        .arg("run")
        .arg("--rm")
        .arg("--add-host")
        .arg("host.docker.internal:host-gateway");
    if pull_never {
        command.arg("--pull").arg("never");
    }
    command
        .arg(image)
        .arg("wget")
        .arg("-q")
        .arg("-O")
        .arg("-")
        .arg("-T")
        .arg("5")
        .arg("--header")
        .arg(format!("Authorization: Bearer {handle}"))
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub fn write_runtime_lease(path: &Path, lease: &OptimizerRuntimeLease) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(lease)?)?;
    Ok(())
}

pub fn read_runtime_lease(path: &Path) -> Result<Option<OptimizerRuntimeLease>> {
    if !path.is_file() {
        return Ok(None);
    }
    let parsed = serde_json::from_slice::<OptimizerRuntimeLease>(&std::fs::read(path)?)?;
    if parsed.schema_version != RUNTIME_LEASE_SCHEMA {
        return Ok(None);
    }
    Ok(Some(parsed))
}

pub fn process_start_identity(pid: u32) -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "lstart="])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if !text.is_empty() {
                return format!("ps-lstart:{text}");
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            return format!(
                "proc-stat:{}",
                stat.split_whitespace().nth(21).unwrap_or("0")
            );
        }
    }
    format!("pid-only:{pid}")
}

pub fn database_digest(path: &Path) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return "sha256:missing".into();
    };
    format!("sha256:{:x}", Sha256::digest(&bytes))
}

/// Resolve a configured-env credential without using a persisted memory pointer.
pub fn resolve_configured_env(store: &EnvSourceStore, backend_ref: &str) -> Result<SecretBytes> {
    store.get(backend_ref).ok_or_else(|| {
        CredentialError::new(
            CREDENTIAL_VALUE_UNLOADED,
            "source",
            false,
            "configured env credential is not loaded in this Workshop process",
        )
        .anyhow()
    })
}
