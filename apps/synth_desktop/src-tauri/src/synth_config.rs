use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

const DEFAULT_PROFILE: &str = "prod";
const DEFAULT_API_KEY_ENV: &str = "SYNTH_API_KEY";
const DEFAULT_WORKER_KEY_ENV: &str = "SMR_WORKER_API_KEY";
const OPENROUTER_API_KEY_ENV: &str = "OPENROUTER_API_KEY";
const DEFAULT_MODEL: &str = "gpt-5.6-luna";
const DEFAULT_MODEL_EFFORT: &str = "xhigh";
const DEFAULT_MODEL_PROVIDERS: &[&str] = &["chatgpt", "openrouter"];

/// A deliberately narrow, instance-local OpenRouter target declaration.  This
/// is configuration, not a provider adapter: it cannot carry credentials,
/// headers, URLs, prompts, prices, or arbitrary generation-body fields.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenRouterModelConfig {
    pub id: String,
    pub model: String,
    #[serde(default = "default_openrouter_model_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub routing: Option<OpenRouterRoutingConfig>,
    #[serde(default)]
    pub ui: Option<OpenRouterModelUiConfig>,
}

fn default_openrouter_model_enabled() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenRouterRoutingConfig {
    #[serde(default)]
    pub allow_fallbacks: Option<bool>,
    #[serde(default)]
    pub require_parameters: Option<bool>,
    #[serde(default)]
    pub data_collection: Option<OpenRouterDataCollection>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OpenRouterDataCollection {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenRouterModelUiConfig {
    #[serde(default)]
    pub reasoning_control: Option<OpenRouterReasoningControl>,
    #[serde(default)]
    pub default_reasoning: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OpenRouterReasoningControl {
    None,
    Binary,
    Effort,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct OpenRouterModelConfigSet {
    pub models: Vec<OpenRouterModelConfig>,
    /// Errors are scoped to a config path so one invalid entry cannot remove
    /// source-owned targets or valid neighboring entries.
    pub diagnostics: Vec<String>,
}

/// Checked-in backend endpoint defaults for the explicit workshop lane
/// profile names, layered on top of the legacy `prod`/`staging`/`local`
/// fallback `resolve()` already had. `[intern.endpoints].<profile>` in
/// config.toml always wins over these.
const DEFAULT_ENDPOINTS: &[(&str, &str)] = &[
    ("local-slot1", "http://127.0.0.1:41109"),
    ("staging", "https://api-dev.usesynth.ai"),
    ("production", "https://mcp.usesynth.ai"),
];

/// Source-owned Responses gateway routing. These values deliberately cannot
/// be overridden by environment or config: Workshop cloud inference must
/// always traverse the metered gateway and must never fall back to the main
/// backend Responses route. Unknown profiles have no gateway and fail closed.
const DEFAULT_GATEWAYS: &[(&str, &str)] = &[
    ("local-slot1", "http://127.0.0.1:41124"),
    ("local", "http://127.0.0.1:41124"),
    (
        "staging",
        "https://synth-responses-gateway-staging-dev.up.railway.app",
    ),
    (
        "prod",
        "https://synth-responses-gateway-prod-production.up.railway.app",
    ),
    (
        "production",
        "https://synth-responses-gateway-prod-production.up.railway.app",
    ),
];

fn lookup_default(table: &[(&str, &str)], profile: &str) -> Option<String> {
    table
        .iter()
        .find(|(name, _)| *name == profile)
        .map(|(_, url)| (*url).to_owned())
}

fn source_owned_backend_url(profile: &str) -> Option<String> {
    lookup_default(DEFAULT_ENDPOINTS, profile).or_else(|| {
        match profile {
            "prod" => Some("https://api.usesynth.ai"),
            "local" => Some("http://127.0.0.1:8000"),
            _ => None,
        }
        .map(str::to_owned)
    })
}

/// Mutable routing exists only for named debug instances. A data-root alone
/// is not an authority signal: release apps also have one, and must never
/// forward credentials to an endpoint selected by their launch environment.
pub(crate) fn development_routing_enabled() -> bool {
    cfg!(debug_assertions) && crate::instance::name().is_some()
}

fn select_backend_url(
    profile: &str,
    allow_development_override: bool,
    environment_override: Option<String>,
    configured_override: Option<String>,
) -> Result<String> {
    if allow_development_override {
        environment_override
            .filter(|value| !value.trim().is_empty())
            .or(configured_override)
            .or_else(|| source_owned_backend_url(profile))
            .ok_or_else(|| anyhow!("unknown Desktop backend profile `{profile}`"))
    } else {
        source_owned_backend_url(profile).ok_or_else(|| {
            anyhow!(
                "Desktop profile `{profile}` has no source-owned backend route; network access is blocked"
            )
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum MultiAgentVersion {
    None,
    V1,
    V2,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelMultiAgentSetting {
    pub model_id: String,
    pub display_name: String,
    pub preset: MultiAgentVersion,
    pub effective: MultiAgentVersion,
    pub overridden: bool,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelMultiAgentUpdate {
    pub model_id: String,
    pub version: Option<MultiAgentVersion>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceAccessSettings {
    pub allowed_roots: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceAccessUpdate {
    pub allowed_roots: Vec<String>,
}

/// User-owned paid-compute auto-approval, stored only in Workshop config.
///
/// Amounts travel as decimal USD strings (at most six fractional digits). The
/// host converts them to integer USD micros so authorization never sees
/// floating-point money.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PaidComputeAutoApprovalSettings {
    pub enabled: bool,
    pub max_request_usd: String,
    pub max_conversation_usd: String,
    pub providers: Vec<String>,
}

impl PaidComputeAutoApprovalSettings {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            max_request_usd: "0.10".into(),
            max_conversation_usd: "10.00".into(),
            providers: Vec::new(),
        }
    }

    /// Fail closed: malformed amounts or providers never become a policy.
    pub fn policy(&self) -> Result<PaidComputeAutoApprovalPolicy> {
        Ok(PaidComputeAutoApprovalPolicy {
            enabled: self.enabled,
            max_request_usd_micros: parse_usd_micros(&self.max_request_usd)?,
            max_conversation_usd_micros: parse_usd_micros(&self.max_conversation_usd)?,
            providers: normalize_paid_compute_providers(&self.providers)?,
        })
    }
}

/// Integer-micros form sealed onto a session at start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaidComputeAutoApprovalPolicy {
    pub enabled: bool,
    pub max_request_usd_micros: u64,
    pub max_conversation_usd_micros: u64,
    pub providers: Vec<String>,
}

impl PaidComputeAutoApprovalPolicy {
    pub fn disabled() -> Self {
        PaidComputeAutoApprovalSettings::disabled()
            .policy()
            .expect("default paid-compute settings are well-formed")
    }

    pub fn allows_provider(&self, provider: &str) -> bool {
        normalize_provider(provider)
            .ok()
            .is_some_and(|normalized| self.providers.iter().any(|allowed| allowed == &normalized))
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPermissionSettings {
    pub config_path: String,
    pub approval_policy: String,
    pub sandbox_mode: String,
    pub paid_compute: PaidComputeAutoApprovalSettings,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPermissionUpdate {
    pub approval_policy: String,
    pub sandbox_mode: String,
    #[serde(default)]
    pub paid_compute: Option<PaidComputeAutoApprovalSettings>,
}

const MODEL_MULTI_AGENT_PRESETS: &[(&str, &str, MultiAgentVersion)] = &[
    ("gpt-5.6-sol", "GPT-5.6 Sol", MultiAgentVersion::V2),
    ("gpt-5.6-terra", "GPT-5.6 Terra", MultiAgentVersion::V2),
    ("gpt-5.6-luna", "GPT 5.6 Luna", MultiAgentVersion::V1),
    ("laguna-xs-2.1", "Laguna XS 2.1", MultiAgentVersion::None),
    ("laguna-s-2.1", "Laguna S 2.1", MultiAgentVersion::None),
    ("muse-spark-1.2", "Muse Spark 1.2", MultiAgentVersion::None),
];

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DefaultModelSettings {
    pub model: String,
    pub effort: String,
    pub providers: Vec<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackendSettings {
    pub config_path: String,
    pub env_file: String,
    pub profile: String,
    pub backend_url: String,
    pub api_key_env: String,
    pub api_key_configured: bool,
    pub api_key_fingerprint: Option<String>,
    pub api_key_source: Option<String>,
    pub worker_key_configured: bool,
    pub openrouter_api_key_configured: bool,
    pub openrouter_api_key_fingerprint: Option<String>,
    pub openrouter_api_key_source: Option<String>,
    /// `[models.default]` from Workshop's config.toml. Provider order is a
    /// fallback chain, not a request to silently change providers mid-turn.
    pub default_model: DefaultModelSettings,
}

#[derive(Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackendSettingsUpdate {
    pub profile: String,
    pub backend_url: String,
    pub env_file: String,
    pub api_key_env: String,
    /// Write-only. It may arrive from the renderer for manual API-key setup,
    /// but is never returned by any settings command.
    pub api_key: Option<String>,
}

impl std::fmt::Debug for BackendSettingsUpdate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BackendSettingsUpdate")
            .field("profile", &self.profile)
            .field("backend_url", &self.backend_url)
            .field("env_file", &self.env_file)
            .field("api_key_env", &self.api_key_env)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[derive(Clone)]
pub struct ResolvedBackend {
    pub config_path: PathBuf,
    pub env_file: PathBuf,
    pub backend_url: String,
    /// This profile's source-owned Responses gateway — never a copy of
    /// `backend_url`. `None` for an unknown profile so callers fail closed.
    pub responses_gateway_url: Option<String>,
    pub api_key: Option<String>,
    pub worker_key: Option<String>,
}

pub fn get() -> Result<BackendSettings> {
    let resolved = resolve()?;
    let mut document = read_toml(&resolved.config_path)?;
    if ensure_default_model_config(&mut document) {
        write_toml(&resolved.config_path, &document)?;
    }
    let intern = document.get("intern").and_then(toml::Value::as_table);
    let profile = intern
        .and_then(|value| value.get("profile"))
        .and_then(toml::Value::as_str)
        .unwrap_or(DEFAULT_PROFILE)
        .to_owned();
    let api_key_env = intern
        .and_then(|value| value.get("api_key_env"))
        .and_then(toml::Value::as_str)
        .unwrap_or(DEFAULT_API_KEY_ENV)
        .to_owned();
    let (api_key, api_key_source) = resolve_secret(&api_key_env, &resolved.env_file);
    let (openrouter_api_key, openrouter_api_key_source) = resolve_openrouter_secret(&resolved);
    let default = document
        .get("models")
        .and_then(toml::Value::as_table)
        .and_then(|models| models.get("default"))
        .and_then(toml::Value::as_table);
    let default_model = DefaultModelSettings {
        model: default
            .and_then(|value| value.get("model"))
            .and_then(toml::Value::as_str)
            .unwrap_or(DEFAULT_MODEL)
            .to_owned(),
        effort: default
            .and_then(|value| value.get("effort"))
            .and_then(toml::Value::as_str)
            .unwrap_or(DEFAULT_MODEL_EFFORT)
            .to_owned(),
        providers: default
            .and_then(|value| value.get("providers"))
            .and_then(toml::Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .filter(|values: &Vec<String>| !values.is_empty())
            .unwrap_or_else(|| {
                DEFAULT_MODEL_PROVIDERS
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect()
            }),
    };
    Ok(BackendSettings {
        config_path: resolved.config_path.display().to_string(),
        env_file: resolved.env_file.display().to_string(),
        profile,
        backend_url: resolved.backend_url,
        api_key_env,
        api_key_configured: api_key.is_some(),
        api_key_fingerprint: api_key.as_deref().map(secret_fingerprint),
        api_key_source,
        worker_key_configured: resolved.worker_key.is_some(),
        openrouter_api_key_configured: openrouter_api_key.is_some(),
        openrouter_api_key_fingerprint: openrouter_api_key.as_deref().map(secret_fingerprint),
        openrouter_api_key_source,
        default_model,
    })
}

/// Materialize the source default so the effective choice is inspectable and
/// operator-editable in config.toml, while preserving every existing value.
fn ensure_default_model_config(document: &mut toml::Value) -> bool {
    let Some(root) = document.as_table_mut() else {
        return false;
    };
    let models = root
        .entry("models")
        .or_insert_with(|| toml::Value::Table(Default::default()));
    let Some(models) = models.as_table_mut() else {
        return false;
    };
    if models.contains_key("default") {
        return false;
    }
    let mut default = toml::Table::new();
    default.insert("model".into(), toml::Value::String(DEFAULT_MODEL.into()));
    default.insert(
        "effort".into(),
        toml::Value::String(DEFAULT_MODEL_EFFORT.into()),
    );
    default.insert(
        "providers".into(),
        toml::Value::Array(
            DEFAULT_MODEL_PROVIDERS
                .iter()
                .map(|value| toml::Value::String((*value).into()))
                .collect(),
        ),
    );
    models.insert("default".into(), toml::Value::Table(default));
    true
}

pub fn update(request: BackendSettingsUpdate) -> Result<BackendSettings> {
    let profile = request.profile.trim().to_lowercase();
    if profile.is_empty()
        || !profile
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(anyhow!("profile must use letters, numbers, - or _"));
    }
    let backend_url = validate_url(&request.backend_url)?;
    let api_key_env = validate_env_key(&request.api_key_env)?;
    let config_path = config_path();
    let env_file = expand_path(
        &request.env_file,
        config_path.parent().unwrap_or(Path::new(".")),
    )?;
    let mut document = read_toml(&config_path)?;
    let root = document
        .as_table_mut()
        .ok_or_else(|| anyhow!("Synth config root must be a TOML table"))?;
    let intern = root
        .entry("intern")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("[intern] must be a TOML table"))?;
    intern.insert("profile".into(), toml::Value::String(profile.clone()));
    intern.insert(
        "env_file".into(),
        toml::Value::String(path_for_toml(&env_file)),
    );
    intern.insert(
        "api_key_env".into(),
        toml::Value::String(api_key_env.clone()),
    );
    let endpoints = intern
        .entry("endpoints")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("[intern.endpoints] must be a TOML table"))?;
    endpoints.insert(profile, toml::Value::String(backend_url));
    write_toml(&config_path, &document)?;

    if let Some(api_key) = request.api_key.as_deref() {
        store_api_key(api_key)?;
    }

    get()
}

/// Persist a Synth API key obtained by device pairing or the write-only manual
/// setup field into the configured 0600 env file, without returning the key.
pub fn store_api_key(secret: &str) -> Result<()> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(anyhow!("pairing returned an empty API key"));
    }
    if secret.contains(['\r', '\n']) {
        return Err(anyhow!("API key cannot contain a newline"));
    }
    let resolved = resolve()?;
    let document = read_toml(&resolved.config_path)?;
    let api_key_env = document
        .get("intern")
        .and_then(toml::Value::as_table)
        .and_then(|v| v.get("api_key_env"))
        .and_then(toml::Value::as_str)
        .unwrap_or(DEFAULT_API_KEY_ENV)
        .to_owned();
    write_env_secret(&resolved.env_file, &api_key_env, secret)
}

/// Removes the desktop-managed Synth API key from the private env file.
/// A process-environment override cannot be erased by the app and remains
/// visible through the redacted settings snapshot.
pub fn remove_api_key() -> Result<()> {
    let resolved = resolve()?;
    let document = read_toml(&resolved.config_path)?;
    let api_key_env = document
        .get("intern")
        .and_then(toml::Value::as_table)
        .and_then(|value| value.get("api_key_env"))
        .and_then(toml::Value::as_str)
        .unwrap_or(DEFAULT_API_KEY_ENV);
    if env::var(api_key_env)
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Err(anyhow!(
            "the API key comes from the process environment; remove {api_key_env} from the launching environment to sign out"
        ));
    }
    remove_env_secret(&resolved.env_file, api_key_env)
}

pub fn openrouter_api_key() -> Result<Option<String>> {
    let resolved = resolve()?;
    Ok(resolve_openrouter_secret(&resolved).0)
}

pub fn workspace_access_settings() -> Result<WorkspaceAccessSettings> {
    let document = read_toml(&config_path())?;
    let allowed_roots = document
        .get("workspace")
        .and_then(|value| value.get("allowed_roots"))
        .and_then(toml::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(WorkspaceAccessSettings { allowed_roots })
}

pub fn allowed_workspace_roots() -> Result<Vec<String>> {
    Ok(workspace_access_settings()?.allowed_roots)
}

/// Operator-declared live-eval capabilities for a container base URL.
///
/// This is the **only** way a capability claim enters Workshop without the
/// service itself advertising it, and it is deliberately here rather than in
/// registration metadata: `config.toml` is written by Tauri commands (the
/// person at the keyboard) and is unreachable from the loopback IPC the MCP
/// adapters speak, so an agent cannot assert that an incompatible engine
/// supports the prepared-rollout workflow.
///
/// ```toml
/// [[containers.capability_declaration]]
/// base_url = "http://127.0.0.1:8104"
/// protocol = "synth.container.live-eval.v1"
/// operations = { "rollouts.prepare" = true, "reward.get" = true }
/// policy_refs = [{ harness = "react", config = "luna_low" }]
/// ```
pub fn container_capability_declaration(base_url: &str) -> Result<Option<serde_json::Value>> {
    Ok(declaration_for(&read_toml(&config_path())?, base_url))
}

fn declaration_for(document: &toml::Value, base_url: &str) -> Option<serde_json::Value> {
    let wanted = base_url.trim().trim_end_matches('/');
    if wanted.is_empty() {
        return None;
    }
    let entries = document
        .get("containers")
        .and_then(|value| value.get("capability_declaration"))
        .and_then(toml::Value::as_array)?;
    let entry = entries.iter().find(|entry| {
        entry
            .get("base_url")
            .and_then(toml::Value::as_str)
            .map(|declared| declared.trim().trim_end_matches('/'))
            == Some(wanted)
    })?;
    let mut block = serde_json::to_value(entry).ok()?;
    // `base_url` is the key, not part of the capability projection.
    block.as_object_mut()?.remove("base_url");
    Some(block)
}

pub(crate) fn select_default_workspace_path(
    allowed_roots: &[String],
    sandbox_mode: &str,
    launcher_workspace: Option<std::path::PathBuf>,
    home: Option<std::path::PathBuf>,
    isolated_default: std::path::PathBuf,
) -> std::path::PathBuf {
    // Named and packaged instances explicitly provide an isolated launcher
    // workspace. Keep that boundary authoritative even with full-system
    // permissions so workspace-local manifests (containers, recipes, and
    // experiments) resolve where the release runner staged them.
    if let Some(workspace) = launcher_workspace {
        return workspace;
    }
    if let Some(root) = allowed_roots.first() {
        return root.into();
    }
    // `danger-full-access` is a machine-wide access promise. Keep an explicit
    // attached root or launcher workspace authoritative. For ordinary launches
    // without either, starting at home preserves repository discovery.
    if sandbox_mode == "danger-full-access" {
        return home.unwrap_or(isolated_default);
    }
    isolated_default
}

pub fn update_workspace_access(request: WorkspaceAccessUpdate) -> Result<WorkspaceAccessSettings> {
    let allowed_roots = validate_workspace_roots(request.allowed_roots)?;
    let path = config_path();
    let mut document = read_toml(&path)?;
    let root = document
        .as_table_mut()
        .ok_or_else(|| anyhow!("Synth config root must be a TOML table"))?;
    let workspace = root
        .entry("workspace")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("[workspace] must be a TOML table"))?;
    workspace.insert(
        "allowed_roots".into(),
        toml::Value::Array(
            allowed_roots
                .iter()
                .cloned()
                .map(toml::Value::String)
                .collect(),
        ),
    );
    write_toml(&path, &document)?;
    Ok(WorkspaceAccessSettings { allowed_roots })
}

pub fn desktop_permission_settings() -> Result<DesktopPermissionSettings> {
    #[cfg(test)]
    if let Some(settings) = test_machine_permissions::current() {
        return Ok(settings);
    }
    let primary = config_path();
    let canonical = dirs::home_dir()
        .unwrap_or_default()
        .join(".synth-desktop/config.toml");
    desktop_permission_settings_with_fallback(
        &primary,
        (primary != canonical).then_some(canonical.as_path()),
    )
}

/// Machine permission settings are process-global state (env plus the home
/// config file), so unit tests that depend on a specific machine policy must
/// install one here instead of reading whatever the developer's machine says.
/// The guard serializes such tests on one lock and restores the previous
/// override on drop.
#[cfg(test)]
pub(crate) mod test_machine_permissions {
    use super::DesktopPermissionSettings;
    use std::sync::{Mutex, MutexGuard, OnceLock, RwLock};

    static OVERRIDE: RwLock<Option<DesktopPermissionSettings>> = RwLock::new(None);
    static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

    pub(crate) fn current() -> Option<DesktopPermissionSettings> {
        OVERRIDE.read().expect("machine override lock").clone()
    }

    pub(crate) struct Guard {
        _serial: MutexGuard<'static, ()>,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            *OVERRIDE.write().expect("machine override lock") = None;
        }
    }

    pub(crate) fn install(approval_policy: &str, sandbox_mode: &str) -> Guard {
        let serial = SERIAL
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *OVERRIDE.write().expect("machine override lock") = Some(DesktopPermissionSettings {
            config_path: "test://machine-permissions".into(),
            approval_policy: approval_policy.into(),
            sandbox_mode: sandbox_mode.into(),
            paid_compute: super::PaidComputeAutoApprovalSettings::disabled(),
        });
        Guard { _serial: serial }
    }

    pub(crate) fn install_with_paid_compute(
        approval_policy: &str,
        sandbox_mode: &str,
        paid_compute: super::PaidComputeAutoApprovalSettings,
    ) -> Guard {
        let serial = SERIAL
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *OVERRIDE.write().expect("machine override lock") = Some(DesktopPermissionSettings {
            config_path: "test://machine-permissions".into(),
            approval_policy: approval_policy.into(),
            sandbox_mode: sandbox_mode.into(),
            paid_compute,
        });
        Guard { _serial: serial }
    }
}

pub fn update_desktop_permissions(
    request: DesktopPermissionUpdate,
) -> Result<DesktopPermissionSettings> {
    update_desktop_permissions_at(&config_path(), request)
}

fn desktop_permission_settings_at(path: &Path) -> Result<DesktopPermissionSettings> {
    desktop_permission_settings_with_fallback(path, None)
}

fn permission_values(
    document: &toml::Value,
) -> Result<(
    Option<String>,
    Option<String>,
    Option<PaidComputeAutoApprovalSettings>,
)> {
    let permissions = document
        .get("desktop")
        .and_then(|value| value.get("permissions"));
    let approval_policy = permissions
        .and_then(|value| value.get("approval_policy"))
        .or_else(|| document.get("approval_policy"))
        .and_then(toml::Value::as_str)
        .filter(|value| is_approval_policy(value))
        .map(str::to_owned);
    let sandbox_mode = permissions
        .and_then(|value| value.get("sandbox_mode"))
        .or_else(|| document.get("sandbox_mode"))
        .and_then(toml::Value::as_str)
        .filter(|value| is_sandbox_mode(value))
        .map(str::to_owned);
    let paid_compute = match permissions.and_then(|value| value.get("paid_compute")) {
        Some(value) => Some(parse_paid_compute_table(value)?),
        None => None,
    };
    Ok((approval_policy, sandbox_mode, paid_compute))
}

fn desktop_permission_settings_with_fallback(
    path: &Path,
    fallback: Option<&Path>,
) -> Result<DesktopPermissionSettings> {
    let document = read_toml(path)?;
    let (approval_policy, sandbox_mode, paid_compute) = permission_values(&document)?;
    let (fallback_approval, fallback_sandbox, fallback_paid) = match fallback {
        Some(fallback_path) => permission_values(&read_toml(fallback_path)?)?,
        None => (None, None, None),
    };
    Ok(DesktopPermissionSettings {
        config_path: path.display().to_string(),
        approval_policy: approval_policy
            .or(fallback_approval)
            .unwrap_or_else(|| "untrusted".into()),
        sandbox_mode: sandbox_mode
            .or(fallback_sandbox)
            .unwrap_or_else(|| "workspace-write".into()),
        paid_compute: paid_compute
            .or(fallback_paid)
            .unwrap_or_else(PaidComputeAutoApprovalSettings::disabled),
    })
}

fn update_desktop_permissions_at(
    path: &Path,
    request: DesktopPermissionUpdate,
) -> Result<DesktopPermissionSettings> {
    if !is_approval_policy(&request.approval_policy) {
        return Err(anyhow!("unsupported approval policy"));
    }
    if !is_sandbox_mode(&request.sandbox_mode) {
        return Err(anyhow!("unsupported sandbox mode"));
    }
    let mut document = read_toml(path)?;
    let root = document
        .as_table_mut()
        .ok_or_else(|| anyhow!("Synth config root must be a TOML table"))?;
    let desktop = root
        .entry("desktop")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("[desktop] must be a TOML table"))?;
    let permissions = desktop
        .entry("permissions")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("[desktop.permissions] must be a TOML table"))?;
    permissions.insert(
        "approval_policy".into(),
        toml::Value::String(request.approval_policy),
    );
    permissions.insert(
        "sandbox_mode".into(),
        toml::Value::String(request.sandbox_mode),
    );
    if let Some(paid_compute) = request.paid_compute {
        let policy = paid_compute.policy()?;
        let mut table = toml::value::Table::new();
        table.insert("auto_approve".into(), toml::Value::Boolean(policy.enabled));
        table.insert(
            "max_request_usd".into(),
            toml::Value::String(format_usd_micros(policy.max_request_usd_micros)),
        );
        table.insert(
            "max_conversation_usd".into(),
            toml::Value::String(format_usd_micros(policy.max_conversation_usd_micros)),
        );
        table.insert(
            "providers".into(),
            toml::Value::Array(
                policy
                    .providers
                    .into_iter()
                    .map(toml::Value::String)
                    .collect(),
            ),
        );
        permissions.insert("paid_compute".into(), toml::Value::Table(table));
    }
    write_toml(path, &document)?;
    desktop_permission_settings_at(path)
}

fn is_approval_policy(value: &str) -> bool {
    matches!(value, "untrusted" | "on-request" | "never")
}

fn is_sandbox_mode(value: &str) -> bool {
    matches!(
        value,
        "read-only" | "workspace-write" | "danger-full-access"
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PaidComputeToml {
    auto_approve: bool,
    max_request_usd: String,
    max_conversation_usd: String,
    providers: Vec<String>,
}

fn parse_paid_compute_table(value: &toml::Value) -> Result<PaidComputeAutoApprovalSettings> {
    let parsed: PaidComputeToml = value
        .clone()
        .try_into()
        .map_err(|error| anyhow!("malformed [desktop.permissions.paid_compute]: {error}"))?;
    let settings = PaidComputeAutoApprovalSettings {
        enabled: parsed.auto_approve,
        max_request_usd: parsed.max_request_usd,
        max_conversation_usd: parsed.max_conversation_usd,
        providers: parsed.providers,
    };
    settings.policy()?;
    Ok(settings)
}

/// Decimal USD string → integer micros. Rejects sign, exponent, and more than
/// six fractional digits so authorization never sees a float.
pub(crate) fn parse_usd_micros(value: &str) -> Result<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("USD amount must not be empty"));
    }
    if trimmed.starts_with('+') || trimmed.starts_with('-') {
        return Err(anyhow!("USD amount must not be signed: {value}"));
    }
    if trimmed.contains(['e', 'E']) {
        return Err(anyhow!(
            "USD amount must not use exponent notation: {value}"
        ));
    }
    let (whole, frac) = match trimmed.split_once('.') {
        Some((whole, frac)) => (whole, frac),
        None => (trimmed, ""),
    };
    if whole.is_empty() || !whole.chars().all(|c| c.is_ascii_digit()) {
        return Err(anyhow!("USD amount is not a decimal string: {value}"));
    }
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return Err(anyhow!("USD amount is not a decimal string: {value}"));
    }
    if frac.len() > 6 {
        return Err(anyhow!(
            "USD amount may have at most six fractional digits: {value}"
        ));
    }
    let whole_micros = whole
        .parse::<u64>()
        .map_err(|_| anyhow!("USD amount is out of range: {value}"))?
        .checked_mul(1_000_000)
        .ok_or_else(|| anyhow!("USD amount is out of range: {value}"))?;
    let frac_micros = if frac.is_empty() {
        0
    } else {
        let mut padded = frac.to_string();
        while padded.len() < 6 {
            padded.push('0');
        }
        padded
            .parse::<u64>()
            .map_err(|_| anyhow!("USD amount is out of range: {value}"))?
    };
    whole_micros
        .checked_add(frac_micros)
        .ok_or_else(|| anyhow!("USD amount is out of range: {value}"))
}

pub(crate) fn format_usd_micros(micros: u64) -> String {
    let dollars = micros / 1_000_000;
    let rem = micros % 1_000_000;
    if rem == 0 {
        format!("{dollars}.00")
    } else if rem % 10_000 == 0 {
        format!("{dollars}.{:02}", rem / 10_000)
    } else {
        let frac = format!("{rem:06}");
        format!("{dollars}.{}", frac.trim_end_matches('0'))
    }
}

pub(crate) fn normalize_provider(value: &str) -> Result<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(anyhow!("paid-compute provider must not be empty"));
    }
    if !normalized
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase())
        || !normalized
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(anyhow!(
            "paid-compute provider `{value}` is not a normalized identifier"
        ));
    }
    Ok(normalized)
}

fn normalize_paid_compute_providers(providers: &[String]) -> Result<Vec<String>> {
    let mut normalized = Vec::new();
    for provider in providers {
        let value = normalize_provider(provider)?;
        if normalized.iter().any(|existing| existing == &value) {
            return Err(anyhow!("duplicate paid-compute provider `{value}`"));
        }
        normalized.push(value);
    }
    Ok(normalized)
}

pub fn model_multi_agent_settings() -> Result<Vec<ModelMultiAgentSetting>> {
    let document = read_toml(&config_path())?;
    let overrides = model_multi_agent_table(&document);
    Ok(MODEL_MULTI_AGENT_PRESETS
        .iter()
        .map(|(model_id, display_name, preset)| {
            let override_value = overrides.and_then(|table| {
                table
                    .get(*model_id)
                    .and_then(toml::Value::as_str)
                    .and_then(parse_multi_agent_version)
            });
            ModelMultiAgentSetting {
                model_id: (*model_id).to_owned(),
                display_name: (*display_name).to_owned(),
                preset: *preset,
                effective: override_value.unwrap_or(*preset),
                overridden: override_value.is_some(),
            }
        })
        .collect())
}

pub fn update_model_multi_agent(
    request: ModelMultiAgentUpdate,
) -> Result<Vec<ModelMultiAgentSetting>> {
    let model_id = canonical_model_id(&request.model_id);
    if model_id.is_empty() {
        return Err(anyhow!("modelId is required"));
    }
    let path = config_path();
    let mut document = read_toml(&path)?;
    let root = document
        .as_table_mut()
        .ok_or_else(|| anyhow!("Synth config root must be a TOML table"))?;
    let models = root
        .entry("models")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("[models] must be a TOML table"))?;
    let multi_agent = models
        .entry("multi_agent")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("[models.multi_agent] must be a TOML table"))?;
    match request.version {
        Some(version) => {
            multi_agent.insert(
                model_id,
                toml::Value::String(multi_agent_version_name(version).to_owned()),
            );
        }
        None => {
            multi_agent.remove(&model_id);
        }
    }
    write_toml(&path, &document)?;
    model_multi_agent_settings()
}

pub fn resolve_model_multi_agent(model: &str) -> Result<MultiAgentVersion> {
    let model_id = canonical_model_id(model);
    let document = read_toml(&config_path())?;
    if let Some(version) = model_multi_agent_table(&document)
        .and_then(|table| table.get(&model_id))
        .and_then(toml::Value::as_str)
        .and_then(parse_multi_agent_version)
    {
        return Ok(version);
    }
    Ok(MODEL_MULTI_AGENT_PRESETS
        .iter()
        .find(|(id, _, _)| *id == model_id)
        .map(|(_, _, version)| *version)
        .unwrap_or(MultiAgentVersion::None))
}

fn canonical_model_id(model: &str) -> String {
    let lower = model.trim().to_ascii_lowercase().replace('_', "-");
    for (model_id, _, _) in MODEL_MULTI_AGENT_PRESETS {
        if lower.contains(model_id) {
            return (*model_id).to_owned();
        }
    }
    lower.rsplit('/').next().unwrap_or(&lower).to_owned()
}

fn model_multi_agent_table(document: &toml::Value) -> Option<&toml::Table> {
    document.get("models")?.get("multi_agent")?.as_table()
}

fn parse_multi_agent_version(value: &str) -> Option<MultiAgentVersion> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" | "disabled" => Some(MultiAgentVersion::None),
        "v1" => Some(MultiAgentVersion::V1),
        "v2" => Some(MultiAgentVersion::V2),
        _ => None,
    }
}

fn multi_agent_version_name(value: MultiAgentVersion) -> &'static str {
    match value {
        MultiAgentVersion::None => "none",
        MultiAgentVersion::V1 => "v1",
        MultiAgentVersion::V2 => "v2",
    }
}

pub fn resolve() -> Result<ResolvedBackend> {
    let config_path = config_path();
    let document = read_toml(&config_path)?;
    let intern = document.get("intern").and_then(toml::Value::as_table);
    let profile = env::var("SYNTH_INTERN_PROFILE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            intern
                .and_then(|v| v.get("profile"))
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| DEFAULT_PROFILE.into());
    let allow_development_override = development_routing_enabled();
    let configured_endpoint = intern
        .and_then(|v| v.get("endpoints"))
        .and_then(toml::Value::as_table)
        .and_then(|v| v.get(&profile))
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let endpoint = select_backend_url(
        &profile,
        allow_development_override,
        env::var("SYNTH_BACKEND_URL").ok(),
        configured_endpoint,
    )?;
    // Named development instances may point their isolated cloud lane at a
    // disposable local Responses backend. Canonical apps retain source-owned
    // routing and deliberately ignore this process override.
    let responses_gateway_url = if allow_development_override {
        env::var("SYNTH_RESPONSES_GATEWAY_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| lookup_default(DEFAULT_GATEWAYS, &profile))
    } else {
        lookup_default(DEFAULT_GATEWAYS, &profile)
    };
    let env_file_raw = intern
        .and_then(|v| v.get("env_file"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            crate::instance::state_root()
                .join(".env")
                .to_string_lossy()
                .into_owned()
        });
    let env_file = expand_path(
        &env_file_raw,
        config_path.parent().unwrap_or(Path::new(".")),
    )?;
    let api_key_env = intern
        .and_then(|v| v.get("api_key_env"))
        .and_then(toml::Value::as_str)
        .unwrap_or(DEFAULT_API_KEY_ENV);
    let worker_key_env = intern
        .and_then(|v| v.get("worker_key_env"))
        .and_then(toml::Value::as_str)
        .unwrap_or(DEFAULT_WORKER_KEY_ENV);
    Ok(ResolvedBackend {
        config_path,
        env_file: env_file.clone(),
        backend_url: validate_url(&endpoint)?,
        responses_gateway_url,
        api_key: resolve_secret(api_key_env, &env_file).0,
        worker_key: resolve_secret("SYNTH_EVAL_EXEC_WORKER_API_KEY", &env_file)
            .0
            .or_else(|| resolve_secret(worker_key_env, &env_file).0),
    })
}

/// The endpoint Codex's native Responses wire traffic should use for the
/// `synth-cloud` provider, or `None` when this profile has no gateway
/// configured — callers must fail closed in that case, never fall back to
/// `resolved.backend_url`.
///
/// The Synth Hosted Laguna gateway is a native `/v1/responses` passthrough
/// with no server-side session store: Codex already sends full history and
/// `store: false` on every turn (see `codex::apply_synth_cloud_provider`), so
/// nothing here needs to add continuity — this function only decides *which
/// host* receives that stateless traffic.
///
/// Account, billing, and usage calls (`account_cloud`, `tariffs`, the
/// account-snapshot fetch, …) always read `ResolvedBackend::backend_url`
/// directly and never call this function — signing in, plan/usage display,
/// and checkout stay pinned to the main backend no matter what this returns.
///
/// Routing is source-owned: `resolve()` selects a checked-in gateway for the
/// active profile. There is no environment/config override and no backend
/// fallback. See `require_responses_gateway_url` for the fail-closed helper
/// every `synth-cloud` call site uses.
pub fn responses_gateway_url(resolved: &ResolvedBackend) -> Option<String> {
    resolved.responses_gateway_url.clone()
}

/// `responses_gateway_url`, but fails closed with a message safe to show
/// the user instead of returning `Option`. Every `synth-cloud` call site —
/// Codex's own session start (`lib.rs`, `eval_driver.rs::prepare_start`) and
/// the eval driver's policy path (`eval_driver.rs::resolve_policy_target`) —
/// goes through this rather than inlining the same fallback decision.
pub fn require_responses_gateway_url(resolved: &ResolvedBackend) -> Result<String, String> {
    responses_gateway_url(resolved).ok_or_else(|| {
        "This Desktop profile has no checked-in Synth Responses gateway. Cloud inference is blocked to prevent an unmetered backend fallback.".to_string()
    })
}

fn config_path() -> PathBuf {
    // A bundled instance descriptor owns the complete runtime identity,
    // including its state/config root. Ignore login-session environment left
    // behind by another Workshop instance; otherwise a Finder/LaunchServices
    // launch can silently load another instance's config and credential file.
    if crate::instance::identity()
        .ok()
        .and_then(|identity| identity.descriptor)
        .is_some()
    {
        return crate::instance::state_root().join("config.toml");
    }
    env::var_os("SYNTH_DESKTOP_CONFIG")
        .or_else(|| env::var_os("SYNTH_INTERN_CONFIG"))
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::instance::state_root().join("config.toml"))
}

/// Parse only `[[models.openrouter]]` from the existing TOML document.  The
/// rest of config.toml remains independently owned, so a malformed custom
/// target never blocks built-ins or unrelated settings from loading.
pub(crate) fn openrouter_model_configs() -> Result<OpenRouterModelConfigSet> {
    let path = config_path();
    let document = read_toml(&path)?;
    let Some(models) = document.get("models").and_then(toml::Value::as_table) else {
        return Ok(OpenRouterModelConfigSet::default());
    };
    let Some(entries) = models.get("openrouter") else {
        return Ok(OpenRouterModelConfigSet::default());
    };
    let Some(entries) = entries.as_array() else {
        return Ok(OpenRouterModelConfigSet {
            models: Vec::new(),
            diagnostics: vec![format!(
                "{}: models.openrouter must be an array of tables",
                path.display()
            )],
        });
    };

    let mut result = OpenRouterModelConfigSet::default();
    let mut seen_ids = std::collections::HashSet::new();
    for (index, raw) in entries.iter().enumerate() {
        let location = format!(
            "{}: [[models.openrouter]] entry {}",
            path.display(),
            index + 1
        );
        let entry = match raw.clone().try_into::<OpenRouterModelConfig>() {
            Ok(entry) => entry,
            Err(error) => {
                result.diagnostics.push(format!("{location}: {error}"));
                continue;
            }
        };
        if let Err(error) = validate_openrouter_model_config(&entry) {
            result.diagnostics.push(format!("{location}: {error}"));
            continue;
        }
        if !seen_ids.insert(entry.id.clone()) {
            result.diagnostics.push(format!(
                "{location}: duplicate OpenRouter target id `{}`",
                entry.id
            ));
            continue;
        }
        result.models.push(entry);
    }
    Ok(result)
}

fn validate_openrouter_model_config(entry: &OpenRouterModelConfig) -> Result<()> {
    if entry.id.is_empty()
        || !entry.id.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '_' | '-')
        })
    {
        return Err(anyhow!(
            "id must use lowercase ASCII letters, digits, `_`, or `-`"
        ));
    }
    if !is_openrouter_model_slug(&entry.model) {
        return Err(anyhow!(
            "model must be an exact OpenRouter slug such as `vendor/model` or `@preset/name`"
        ));
    }
    if entry
        .display_name
        .as_deref()
        .is_some_and(|value| value.trim().is_empty() || value.chars().count() > 120)
    {
        return Err(anyhow!(
            "display_name must be 1–120 visible characters when set"
        ));
    }
    if let Some(ui) = &entry.ui {
        if ui.default_reasoning.as_deref().is_some_and(|value| {
            !matches!(value, "none" | "low" | "medium" | "high" | "xhigh" | "max")
        }) {
            return Err(anyhow!(
                "ui.default_reasoning must be one of none, low, medium, high, xhigh, max"
            ));
        }
    }
    Ok(())
}

fn is_openrouter_model_slug(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.len() > 240 || value.contains(char::is_whitespace) {
        return false;
    }
    if let Some(name) = value.strip_prefix("@preset/") {
        return !name.is_empty()
            && name.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '/' | '-' | '_' | '.')
            });
    }
    let body = value;
    let mut pieces = body.split('/');
    let first = pieces.next().unwrap_or_default();
    let second = pieces.next().unwrap_or_default();
    // Ordinary model slugs are exactly vendor/model.
    if first.is_empty() || second.is_empty() || pieces.next().is_some() {
        return false;
    }
    value.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '@' | '/' | '-' | '_' | '.')
    })
}

fn read_toml(path: &Path) -> Result<toml::Value> {
    match fs::read_to_string(path) {
        Ok(raw) => raw
            .parse::<toml::Value>()
            .with_context(|| format!("Invalid TOML in {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(toml::Value::Table(Default::default()))
        }
        Err(error) => Err(error).with_context(|| format!("Read {}", path.display())),
    }
}

fn write_toml(path: &Path, document: &toml::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml::to_string_pretty(document)?)?;
    Ok(())
}

/// Rewrite the non-authoritative, human-readable credential locator export.
/// SQLite remains the only input to boot and runtime lookup.
pub(crate) fn rewrite_credential_locator_export(
    locators: &[crate::secrets::CredentialLocatorSummary],
) -> Result<()> {
    let path = config_path();
    let mut document = read_toml(&path)?;
    let root = document
        .as_table_mut()
        .ok_or_else(|| anyhow!("Synth config root must be a TOML table"))?;
    let desktop = root
        .entry("desktop")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("[desktop] must be a TOML table"))?;
    let entries = locators
        .iter()
        .map(|locator| {
            let mut entry = toml::map::Map::new();
            entry.insert("id".into(), toml::Value::String(locator.id.clone()));
            entry.insert(
                "kind".into(),
                toml::Value::String(locator.kind.as_str().into()),
            );
            if let Some(reference) = locator.workspace_root_ref.as_ref() {
                entry.insert(
                    "workspace_root_ref".into(),
                    toml::Value::String(reference.clone()),
                );
            }
            if let Some(relative) = locator.relative_path.as_ref() {
                entry.insert(
                    "relative_path".into(),
                    toml::Value::String(relative.clone()),
                );
            }
            if matches!(
                locator.kind,
                crate::secrets::CredentialLocatorKind::ExternalEnvFile
            ) && locator.display_path.starts_with("~/")
            {
                entry.insert(
                    "external_path".into(),
                    toml::Value::String(locator.display_path.clone()),
                );
            }
            entry.insert("format".into(), toml::Value::String(locator.format.clone()));
            entry.insert(
                "provider".into(),
                toml::Value::String(locator.provider.clone()),
            );
            entry.insert(
                "variable".into(),
                toml::Value::String(locator.variable.clone()),
            );
            entry.insert("label".into(), toml::Value::String(locator.label.clone()));
            entry.insert(
                "state".into(),
                toml::Value::String(locator.state.as_str().into()),
            );
            toml::Value::Table(entry)
        })
        .collect::<Vec<_>>();
    desktop.insert("credential_locators".into(), toml::Value::Array(entries));
    write_toml(&path, &document)
}

fn resolve_secret(key: &str, env_file: &Path) -> (Option<String>, Option<String>) {
    if let Ok(value) = env::var(key) {
        if !value.trim().is_empty() {
            return (Some(value), Some("process environment".into()));
        }
    }
    let value = read_env_value(env_file, key);
    let source = value.as_ref().map(|_| env_file.display().to_string());
    (value, source)
}

/// A canonical desktop can keep its active Synth credentials in a profile
/// env-file while retaining the OpenRouter key in the original shared private
/// env-file. Honor that established migration path without making named
/// development instances read another instance's credentials.
fn resolve_openrouter_secret(resolved: &ResolvedBackend) -> (Option<String>, Option<String>) {
    let configured = resolve_secret(OPENROUTER_API_KEY_ENV, &resolved.env_file);
    if configured.0.is_some() {
        return configured;
    }

    // Named development instances set a private data root and are deliberately
    // credential-isolated. Only a canonical installed app may inherit the
    // historic shared OpenRouter location.
    if env::var_os(crate::instance::DATA_ROOT_ENV).is_some() {
        return configured;
    }
    let canonical_root = crate::instance::state_root();
    let legacy_env = canonical_root.join(".env");
    if resolved.env_file == legacy_env {
        return configured;
    }
    resolve_openrouter_secret_from_paths(&resolved.env_file, Some(&legacy_env))
}

fn resolve_openrouter_secret_from_paths(
    env_file: &Path,
    legacy_env: Option<&Path>,
) -> (Option<String>, Option<String>) {
    let configured = resolve_secret(OPENROUTER_API_KEY_ENV, env_file);
    if configured.0.is_some() {
        return configured;
    }
    let Some(legacy_env) = legacy_env else {
        return configured;
    };
    let value = read_env_value(legacy_env, OPENROUTER_API_KEY_ENV);
    let source = value.as_ref().map(|_| legacy_env.display().to_string());
    (value, source)
}

fn read_env_value(path: &Path, key: &str) -> Option<String> {
    let path = path.to_path_buf();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(fs::read_to_string(path));
    });
    // Credential files can live in privacy-controlled or cloud-backed
    // folders. macOS may suspend an open indefinitely while waiting for an
    // unavailable provider or consent UI. Secret discovery is optional at
    // startup, so fail closed instead of freezing the entire application.
    receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .ok()?
        .ok()?
        .lines()
        .find_map(|line| {
            let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then(|| value.trim().trim_matches(['\'', '"']).to_owned())
        })
        .filter(|value| !value.is_empty())
}

fn write_env_secret(path: &Path, key: &str, secret: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut lines: Vec<String> = fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| {
            line.trim()
                .strip_prefix("export ")
                .unwrap_or(line.trim())
                .split_once('=')
                .map(|(name, _)| name.trim() != key)
                .unwrap_or(true)
        })
        .map(str::to_owned)
        .collect();
    lines.push(format!("{key}={secret}"));
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    writeln!(file, "{}", lines.join("\n"))?;
    #[cfg(unix)]
    fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(())
}

fn remove_env_secret(path: &Path, key: &str) -> Result<()> {
    let Ok(contents) = fs::read_to_string(path) else {
        return Ok(());
    };
    let lines: Vec<&str> = contents
        .lines()
        .filter(|line| {
            line.trim()
                .strip_prefix("export ")
                .unwrap_or(line.trim())
                .split_once('=')
                .map(|(name, _)| name.trim() != key)
                .unwrap_or(true)
        })
        .collect();
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = fs::OpenOptions::new();
    options.write(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    if !lines.is_empty() {
        writeln!(file, "{}", lines.join("\n"))?;
    }
    #[cfg(unix)]
    fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(())
}

fn validate_url(value: &str) -> Result<String> {
    let value = value.trim().trim_end_matches('/');
    let parsed = reqwest::Url::parse(value)
        .map_err(|_| anyhow!("backend URL must be a valid http:// or https:// URL"))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(anyhow!("backend URL must not contain credentials"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("backend URL must include a host"))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if parsed.scheme() != "https" && !(parsed.scheme() == "http" && loopback) {
        return Err(anyhow!(
            "backend URL must use https (http is allowed only for loopback development)"
        ));
    }
    Ok(value.to_owned())
}

fn validate_env_key(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty()
        || !value
            .chars()
            .enumerate()
            .all(|(i, c)| c == '_' || c.is_ascii_alphanumeric() && (i > 0 || !c.is_ascii_digit()))
    {
        return Err(anyhow!("API key environment name is invalid"));
    }
    Ok(value.to_owned())
}

fn expand_path(value: &str, relative_to: &Path) -> Result<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("env file path is required"));
    }
    let path = if trimmed == "~" {
        dirs::home_dir().unwrap_or_default()
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        dirs::home_dir().unwrap_or_default().join(rest)
    } else {
        PathBuf::from(trimmed)
    };
    Ok(if path.is_absolute() {
        path
    } else {
        relative_to.join(path)
    })
}

fn validate_workspace_roots(values: Vec<String>) -> Result<Vec<String>> {
    let mut validated = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let path = if trimmed == "~" {
            dirs::home_dir().ok_or_else(|| anyhow!("cannot resolve home directory"))?
        } else if let Some(rest) = trimmed.strip_prefix("~/") {
            dirs::home_dir()
                .ok_or_else(|| anyhow!("cannot resolve home directory"))?
                .join(rest)
        } else {
            PathBuf::from(trimmed)
        };
        if !path.is_absolute() {
            return Err(anyhow!(
                "workspace root must be an absolute path: {trimmed}"
            ));
        }
        let canonical = fs::canonicalize(&path)
            .with_context(|| format!("workspace root does not exist: {}", path.display()))?;
        if !canonical.is_dir() {
            return Err(anyhow!(
                "workspace root must be a directory: {}",
                canonical.display()
            ));
        }
        let canonical = canonical.display().to_string();
        if !validated.contains(&canonical) {
            validated.push(canonical);
        }
    }
    Ok(validated)
}

fn path_for_toml(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = path.strip_prefix(home) {
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

fn secret_fingerprint(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    format!("sha256:{:x}", digest)[..15].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configurable_openrouter_model_accepts_minimal_ox_alpha_entry() {
        let entry: OpenRouterModelConfig = toml::from_str(
            r#"
id = "ox-alpha"
model = "stealth/ox-alpha"
enabled = true
"#,
        )
        .unwrap();
        validate_openrouter_model_config(&entry).unwrap();
        assert_eq!(entry.id, "ox-alpha");
        assert_eq!(entry.model, "stealth/ox-alpha");
        assert!(entry.enabled);
    }

    #[test]
    fn configurable_openrouter_model_rejects_unknown_and_unsafe_fields() {
        let unknown = toml::from_str::<OpenRouterModelConfig>(
            "id = \"ox-alpha\"\nmodel = \"stealth/ox-alpha\"\napi_key = \"never\"\n",
        );
        assert!(unknown.is_err());

        let invalid: OpenRouterModelConfig =
            toml::from_str("id = \"Ox Alpha\"\nmodel = \"https://example.invalid/model\"\n")
                .unwrap();
        assert!(validate_openrouter_model_config(&invalid).is_err());
    }

    #[test]
    fn materializes_luna_xhigh_default_without_overwriting_operator_choice() {
        let mut document = toml::Value::Table(Default::default());
        assert!(ensure_default_model_config(&mut document));
        assert_eq!(
            document["models"]["default"]["model"].as_str(),
            Some("gpt-5.6-luna")
        );
        assert_eq!(
            document["models"]["default"]["effort"].as_str(),
            Some("xhigh")
        );
        assert_eq!(
            document["models"]["default"]["providers"][0].as_str(),
            Some("chatgpt")
        );

        document["models"]["default"]["effort"] = toml::Value::String("high".into());
        assert!(!ensure_default_model_config(&mut document));
        assert_eq!(
            document["models"]["default"]["effort"].as_str(),
            Some("high")
        );
    }

    #[test]
    fn backend_settings_update_redacts_write_only_renderer_key_field() {
        let request: BackendSettingsUpdate = serde_json::from_value(serde_json::json!({
            "profile": "local",
            "backendUrl": "http://127.0.0.1:8000",
            "envFile": "/tmp/.env",
            "apiKeyEnv": "SYNTH_API_KEY",
            "apiKey": "renderer-secret",
            "openrouterApiKey": "renderer-openrouter-secret"
        }))
        .unwrap();
        let debug = format!("{request:?}");
        assert!(!debug.contains("renderer-secret"));
        assert!(!debug.contains("renderer-openrouter-secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn container_capability_declaration_matches_one_operator_owned_base_url() {
        let document = r#"
[[containers.capability_declaration]]
base_url = "http://127.0.0.1:8104/"
protocol = "synth.container.live-eval.v1"
operations = { "rollouts.prepare" = true, "trace_v5.capture" = false }
policy_refs = [{ harness = "react", config = "luna_low" }]

[[containers.capability_declaration]]
base_url = "http://127.0.0.1:9000"
protocol = "synth.container.live-eval.v1"
operations = { "rollouts.prepare" = false }
"#
        .parse::<toml::Value>()
        .unwrap();

        let declared = declaration_for(&document, "http://127.0.0.1:8104").unwrap();
        assert_eq!(declared["protocol"], "synth.container.live-eval.v1");
        assert_eq!(declared["operations"]["rollouts.prepare"], true);
        assert_eq!(declared["operations"]["trace_v5.capture"], false);
        assert_eq!(declared["policy_refs"][0]["config"], "luna_low");
        // The key is not part of the projection.
        assert!(declared.get("base_url").is_none());

        // Trailing-slash difference still matches; a different port does not.
        assert!(declaration_for(&document, "http://127.0.0.1:8104/").is_some());
        assert!(declaration_for(&document, "http://127.0.0.1:8105").is_none());
        assert!(declaration_for(&document, "").is_none());
        assert!(declaration_for(
            &toml::Value::Table(Default::default()),
            "http://127.0.0.1:8104"
        )
        .is_none());
    }

    #[test]
    fn reads_quoted_env_values() {
        let path = env::temp_dir().join(format!("synth-env-{}", uuid::Uuid::new_v4()));
        fs::write(&path, "SYNTH_API_KEY='secret'\n").unwrap();
        assert_eq!(
            read_env_value(&path, "SYNTH_API_KEY").as_deref(),
            Some("secret")
        );
        let _ = fs::remove_file(path);
    }
    #[cfg(unix)]
    #[test]
    fn writes_private_openrouter_secret_without_exposing_it() {
        use std::os::unix::fs::PermissionsExt;
        let path = env::temp_dir().join(format!("synth-env-{}", uuid::Uuid::new_v4()));
        fs::write(&path, "OPENROUTER_API_KEY=old\nKEEP=yes\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        write_env_secret(&path, OPENROUTER_API_KEY_ENV, "sk-or-secret").unwrap();
        assert_eq!(
            read_env_value(&path, OPENROUTER_API_KEY_ENV).as_deref(),
            Some("sk-or-secret")
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(fs::read_to_string(&path).unwrap().contains("KEEP=yes"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn openrouter_uses_the_canonical_legacy_env_only_after_the_active_env_misses() {
        let root = env::temp_dir().join(format!("synth-openrouter-{}", uuid::Uuid::new_v4()));
        let active = root.join("active.env");
        let legacy = root.join("legacy.env");
        fs::create_dir_all(&root).unwrap();
        fs::write(&active, "SYNTH_API_KEY=synth-only\n").unwrap();
        fs::write(&legacy, "OPENROUTER_API_KEY=legacy-openrouter-key\n").unwrap();
        let (value, source) = resolve_openrouter_secret_from_paths(&active, Some(&legacy));
        assert_eq!(value.as_deref(), Some("legacy-openrouter-key"));
        assert_eq!(source.as_deref(), Some(legacy.to_string_lossy().as_ref()));

        fs::write(&active, "OPENROUTER_API_KEY=active-openrouter-key\n").unwrap();
        let (value, source) = resolve_openrouter_secret_from_paths(&active, Some(&legacy));
        assert_eq!(value.as_deref(), Some("active-openrouter-key"));
        assert_eq!(source.as_deref(), Some(active.to_string_lossy().as_ref()));
        let _ = fs::remove_dir_all(root);
    }
    #[cfg(unix)]
    #[test]
    fn removes_only_the_requested_secret_and_keeps_file_private() {
        use std::os::unix::fs::PermissionsExt;
        let path = env::temp_dir().join(format!("synth-env-{}", uuid::Uuid::new_v4()));
        fs::write(&path, "SYNTH_API_KEY=remove-me\nKEEP=yes\n").unwrap();
        remove_env_secret(&path, DEFAULT_API_KEY_ENV).unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        assert!(!contents.contains("remove-me"));
        assert!(contents.contains("KEEP=yes"));
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = fs::remove_file(path);
    }
    #[test]
    fn validates_backend_urls_and_env_keys() {
        assert!(validate_url("http://127.0.0.1:8000/").is_ok());
        assert!(validate_url("file:///tmp/key").is_err());
        assert!(validate_env_key("SYNTH_API_KEY").is_ok());
        assert!(validate_env_key("1KEY").is_err());
    }

    #[test]
    fn canonicalizes_and_deduplicates_workspace_roots() {
        let root = env::temp_dir().join(format!("synth-workspace-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("nested")).unwrap();
        let roots = validate_workspace_roots(vec![
            root.display().to_string(),
            root.join("nested/..").display().to_string(),
        ])
        .unwrap();
        assert_eq!(
            roots,
            vec![fs::canonicalize(&root).unwrap().display().to_string()]
        );
        assert!(validate_workspace_roots(vec!["relative/path".into()]).is_err());
        assert!(
            validate_workspace_roots(vec![root.join("missing").display().to_string()]).is_err()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn launcher_workspace_wins_under_full_system_access() {
        let selected = select_default_workspace_path(
            &[],
            "danger-full-access",
            Some(PathBuf::from("/isolated/instance/workspace")),
            Some(PathBuf::from("/Users/example")),
            PathBuf::from("/isolated/default"),
        );
        assert_eq!(selected, PathBuf::from("/isolated/instance/workspace"));
    }

    #[test]
    fn named_instance_workspace_wins_over_machine_allowed_roots() {
        let selected = select_default_workspace_path(
            &["/Users/example/Documents/GitHub/containers".into()],
            "danger-full-access",
            Some(PathBuf::from("/isolated/instance/workspace")),
            Some(PathBuf::from("/Users/example")),
            PathBuf::from("/isolated/default"),
        );
        assert_eq!(selected, PathBuf::from("/isolated/instance/workspace"));
    }

    #[test]
    fn desktop_permissions_are_machine_config_and_preserve_other_settings() {
        let root = env::temp_dir().join(format!("synth-permissions-{}", uuid::Uuid::new_v4()));
        let path = root.join("config.toml");
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, "[intern]\nprofile = \"staging\"\n").unwrap();
        let defaults = desktop_permission_settings_at(&path).unwrap();
        assert_eq!(defaults.approval_policy, "untrusted");
        assert_eq!(defaults.sandbox_mode, "workspace-write");
        assert_eq!(defaults.paid_compute.max_conversation_usd, "10.00");
        assert_eq!(
            defaults.paid_compute,
            PaidComputeAutoApprovalSettings::disabled()
        );
        let stored = update_desktop_permissions_at(
            &path,
            DesktopPermissionUpdate {
                approval_policy: "never".into(),
                sandbox_mode: "danger-full-access".into(),
                paid_compute: None,
            },
        )
        .unwrap();
        assert_eq!(stored.approval_policy, "never");
        assert_eq!(stored.sandbox_mode, "danger-full-access");
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("[desktop.permissions]"));
        assert!(contents.contains("approval_policy = \"never\""));
        assert!(contents.contains("sandbox_mode = \"danger-full-access\""));
        assert!(contents.contains("profile = \"staging\""));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn isolated_desktop_inherits_canonical_root_permission_defaults() {
        let root =
            env::temp_dir().join(format!("synth-permission-inherit-{}", uuid::Uuid::new_v4()));
        let isolated = root.join("instance/config.toml");
        let canonical = root.join("canonical/config.toml");
        fs::create_dir_all(isolated.parent().unwrap()).unwrap();
        fs::create_dir_all(canonical.parent().unwrap()).unwrap();
        fs::write(&isolated, "[intern]\nprofile = \"local\"\n").unwrap();
        fs::write(
            &canonical,
            "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\n",
        )
        .unwrap();

        let inherited =
            desktop_permission_settings_with_fallback(&isolated, Some(&canonical)).unwrap();
        assert_eq!(inherited.approval_policy, "never");
        assert_eq!(inherited.sandbox_mode, "danger-full-access");

        fs::write(
            &isolated,
            "[desktop.permissions]\napproval_policy = \"on-request\"\n",
        )
        .unwrap();
        let overridden =
            desktop_permission_settings_with_fallback(&isolated, Some(&canonical)).unwrap();
        assert_eq!(overridden.approval_policy, "on-request");
        assert_eq!(overridden.sandbox_mode, "danger-full-access");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn desktop_permissions_reject_unknown_values() {
        let root = env::temp_dir().join(format!("synth-permissions-{}", uuid::Uuid::new_v4()));
        let path = root.join("config.toml");
        let result = update_desktop_permissions_at(
            &path,
            DesktopPermissionUpdate {
                approval_policy: "YOLO".into(),
                sandbox_mode: "danger-full-access".into(),
                paid_compute: None,
            },
        );
        assert!(result.is_err());
        assert!(!path.exists());
    }

    #[test]
    fn parse_usd_micros_rejects_sign_exponent_and_over_precision() {
        assert_eq!(parse_usd_micros("0.10").unwrap(), 100_000);
        assert_eq!(parse_usd_micros("1").unwrap(), 1_000_000);
        assert_eq!(parse_usd_micros("0.000001").unwrap(), 1);
        assert_eq!(parse_usd_micros("0.018").unwrap(), 18_000);
        assert_eq!(format_usd_micros(100_000), "0.10");
        assert_eq!(format_usd_micros(250_000), "0.25");
        assert_eq!(format_usd_micros(60_000), "0.06");
        assert_eq!(format_usd_micros(18_000), "0.018");
        assert_eq!(format_usd_micros(1_000_000), "1.00");
        assert!(parse_usd_micros("-0.10").is_err());
        assert!(parse_usd_micros("+0.10").is_err());
        assert!(parse_usd_micros("1e-1").is_err());
        assert!(parse_usd_micros("0.1234567").is_err());
        assert!(parse_usd_micros("").is_err());
        assert!(parse_usd_micros("abc").is_err());
    }

    #[test]
    fn paid_compute_settings_fail_closed_on_malformed_toml() {
        let root = env::temp_dir().join(format!("synth-paid-compute-{}", uuid::Uuid::new_v4()));
        let path = root.join("config.toml");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            &path,
            "[desktop.permissions.paid_compute]\nauto_approve = true\nmax_request_usd = \"0.10\"\n",
        )
        .unwrap();
        let error = desktop_permission_settings_at(&path)
            .unwrap_err()
            .to_string();
        assert!(error.contains("paid_compute"), "{error}");

        fs::write(
            &path,
            "[desktop.permissions.paid_compute]\nauto_approve = true\nmax_request_usd = \"0.1234567\"\nmax_conversation_usd = \"1.00\"\nproviders = [\"openrouter\"]\n",
        )
        .unwrap();
        assert!(desktop_permission_settings_at(&path).is_err());

        fs::write(
            &path,
            "[desktop.permissions.paid_compute]\nauto_approve = true\nmax_request_usd = \"-0.10\"\nmax_conversation_usd = \"1.00\"\nproviders = [\"openrouter\"]\n",
        )
        .unwrap();
        assert!(desktop_permission_settings_at(&path).is_err());

        fs::write(
            &path,
            "[desktop.permissions.paid_compute]\nauto_approve = true\nmax_request_usd = \"0.10\"\nmax_conversation_usd = \"1.00\"\nproviders = [\"OpenRouter\"]\nunknown = true\n",
        )
        .unwrap();
        assert!(desktop_permission_settings_at(&path).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn paid_compute_settings_round_trip_and_normalize_providers() {
        let root = env::temp_dir().join(format!("synth-paid-compute-{}", uuid::Uuid::new_v4()));
        let path = root.join("config.toml");
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, "[intern]\nprofile = \"staging\"\n").unwrap();
        let stored = update_desktop_permissions_at(
            &path,
            DesktopPermissionUpdate {
                approval_policy: "untrusted".into(),
                sandbox_mode: "workspace-write".into(),
                paid_compute: Some(PaidComputeAutoApprovalSettings {
                    enabled: true,
                    max_request_usd: "0.10".into(),
                    max_conversation_usd: "1.00".into(),
                    providers: vec!["OpenRouter".into()],
                }),
            },
        )
        .unwrap();
        assert!(stored.paid_compute.enabled);
        assert_eq!(stored.paid_compute.max_request_usd, "0.10");
        assert_eq!(stored.paid_compute.max_conversation_usd, "1.00");
        assert_eq!(stored.paid_compute.providers, vec!["openrouter"]);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("[desktop.permissions.paid_compute]"));
        assert!(contents.contains("auto_approve = true"));
        assert!(contents.contains("profile = \"staging\""));
        let policy = stored.paid_compute.policy().unwrap();
        assert_eq!(policy.max_request_usd_micros, 100_000);
        assert_eq!(policy.max_conversation_usd_micros, 1_000_000);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn isolated_desktop_inherits_canonical_paid_compute_policy() {
        let root = env::temp_dir().join(format!("synth-paid-inherit-{}", uuid::Uuid::new_v4()));
        let isolated = root.join("instance/config.toml");
        let canonical = root.join("canonical/config.toml");
        fs::create_dir_all(isolated.parent().unwrap()).unwrap();
        fs::create_dir_all(canonical.parent().unwrap()).unwrap();
        fs::write(&isolated, "[intern]\nprofile = \"local\"\n").unwrap();
        fs::write(
            &canonical,
            "[desktop.permissions.paid_compute]\nauto_approve = true\nmax_request_usd = \"0.10\"\nmax_conversation_usd = \"0.25\"\nproviders = [\"openrouter\"]\n",
        )
        .unwrap();
        let inherited =
            desktop_permission_settings_with_fallback(&isolated, Some(&canonical)).unwrap();
        assert!(inherited.paid_compute.enabled);
        assert_eq!(inherited.paid_compute.max_conversation_usd, "0.25");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_presets_are_provider_independent() {
        assert_eq!(canonical_model_id("openai/gpt-5.6-terra"), "gpt-5.6-terra");
        assert_eq!(canonical_model_id("custom/GPT-5.6-LUNA"), "gpt-5.6-luna");
        assert_eq!(
            canonical_model_id("poolside/Laguna-XS-2.1-NVFP4-mlx"),
            "laguna-xs-2.1"
        );
    }

    #[test]
    fn responses_gateway_url_is_source_owned_and_fails_closed() {
        let without_gateway = ResolvedBackend {
            config_path: PathBuf::from("/tmp/config.toml"),
            env_file: PathBuf::from("/tmp/.env"),
            backend_url: "https://api.usesynth.ai".into(),
            responses_gateway_url: None,
            api_key: None,
            worker_key: None,
        };
        let with_checked_in_gateway = ResolvedBackend {
            responses_gateway_url: Some(
                "https://synth-responses-gateway-staging-dev.up.railway.app".into(),
            ),
            ..without_gateway.clone()
        };

        // No known profile gateway: fail closed. Never use backend_url,
        // which account/billing calls continue to read directly.
        assert_eq!(responses_gateway_url(&without_gateway), None);
        assert!(require_responses_gateway_url(&without_gateway).is_err());
        assert_eq!(
            responses_gateway_url(&with_checked_in_gateway).as_deref(),
            Some("https://synth-responses-gateway-staging-dev.up.railway.app")
        );
        assert_eq!(without_gateway.backend_url, "https://api.usesynth.ai");
    }

    #[test]
    fn require_responses_gateway_url_reports_an_actionable_error() {
        let resolved = ResolvedBackend {
            config_path: PathBuf::from("/tmp/config.toml"),
            env_file: PathBuf::from("/tmp/.env"),
            backend_url: "https://mcp.usesynth.ai".into(),
            responses_gateway_url: None,
            api_key: None,
            worker_key: None,
        };
        let error = require_responses_gateway_url(&resolved).unwrap_err();
        assert!(error.to_lowercase().contains("gateway"));
        assert!(error.contains("no checked-in"));
        assert!(error.contains("unmetered backend fallback"));

        let configured = ResolvedBackend {
            responses_gateway_url: Some("http://127.0.0.1:41124".into()),
            ..resolved
        };
        assert_eq!(
            require_responses_gateway_url(&configured).unwrap(),
            "http://127.0.0.1:41124"
        );
    }

    #[test]
    fn default_gateways_and_endpoints_cover_the_explicit_lane_profiles() {
        assert_eq!(
            lookup_default(DEFAULT_GATEWAYS, "local-slot1").as_deref(),
            Some("http://127.0.0.1:41124")
        );
        assert_eq!(
            lookup_default(DEFAULT_GATEWAYS, "local").as_deref(),
            Some("http://127.0.0.1:41124")
        );
        assert_eq!(
            lookup_default(DEFAULT_GATEWAYS, "staging").as_deref(),
            Some("https://synth-responses-gateway-staging-dev.up.railway.app")
        );
        assert_eq!(
            lookup_default(DEFAULT_GATEWAYS, "production").as_deref(),
            Some("https://synth-responses-gateway-prod-production.up.railway.app")
        );
        assert_eq!(
            lookup_default(DEFAULT_GATEWAYS, "prod").as_deref(),
            Some("https://synth-responses-gateway-prod-production.up.railway.app")
        );

        assert_eq!(
            lookup_default(DEFAULT_ENDPOINTS, "local-slot1").as_deref(),
            Some("http://127.0.0.1:41109")
        );
        assert_eq!(
            lookup_default(DEFAULT_ENDPOINTS, "staging").as_deref(),
            Some("https://api-dev.usesynth.ai")
        );
        assert_eq!(
            lookup_default(DEFAULT_ENDPOINTS, "production").as_deref(),
            Some("https://mcp.usesynth.ai")
        );
    }

    #[test]
    fn canonical_routing_ignores_environment_and_configured_endpoints() {
        let selected = select_backend_url(
            "prod",
            false,
            Some("https://environment.attacker.invalid".into()),
            Some("https://config.attacker.invalid".into()),
        )
        .unwrap();
        assert_eq!(selected, "https://api.usesynth.ai");
        assert!(select_backend_url(
            "attacker-profile",
            false,
            Some("https://environment.attacker.invalid".into()),
            None,
        )
        .is_err());
    }

    #[test]
    fn backend_urls_require_https_except_for_loopback_development() {
        assert_eq!(
            validate_url("http://127.0.0.1:41109/").unwrap(),
            "http://127.0.0.1:41109"
        );
        assert!(validate_url("http://api.usesynth.ai").is_err());
        assert!(validate_url("https://user:secret@api.usesynth.ai").is_err());
        assert!(validate_url("file:///tmp/backend").is_err());
        assert_eq!(
            validate_url("https://api.usesynth.ai/").unwrap(),
            "https://api.usesynth.ai"
        );
    }

    /// `SYNTH_DESKTOP_CONFIG` is process-global and Rust runs tests in
    /// parallel by default, so both profile fixtures live in one `#[test]`
    /// to avoid racing another thread's config path.
    #[test]
    fn resolve_uses_only_checked_in_gateway_routing() {
        let root = env::temp_dir().join(format!("synth-resolve-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        env::remove_var("SYNTH_INTERN_PROFILE");
        env::remove_var("SYNTH_BACKEND_URL");
        env::remove_var("SYNTH_INTERN_CONFIG");
        // Legacy TOML gateway overrides are ignored; the checked-in staging
        // route is authoritative.
        let staging_config = root.join("staging.toml");
        fs::write(
            &staging_config,
            "[intern]\nprofile = \"staging\"\n\n[intern.gateways]\nstaging = \"https://custom-staging-gateway.example\"\n",
        )
        .unwrap();
        env::set_var("SYNTH_DESKTOP_CONFIG", &staging_config);
        let resolved = resolve().unwrap();
        assert_eq!(resolved.backend_url, "https://api-dev.usesynth.ai");
        assert_eq!(
            resolved.responses_gateway_url.as_deref(),
            Some("https://synth-responses-gateway-staging-dev.up.railway.app")
        );

        // A profile with no override falls back to the checked-in default
        // for both the backend endpoint and its dedicated gateway.
        let local_slot1_config = root.join("local-slot1.toml");
        fs::write(&local_slot1_config, "[intern]\nprofile = \"local-slot1\"\n").unwrap();
        env::set_var("SYNTH_DESKTOP_CONFIG", &local_slot1_config);
        let resolved = resolve().unwrap();
        assert_eq!(resolved.backend_url, "http://127.0.0.1:41109");
        assert_eq!(
            resolved.responses_gateway_url.as_deref(),
            Some("http://127.0.0.1:41124")
        );

        // The conventional local profile uses the same checked-in local
        // gateway and needs no per-machine routing variable.
        let local_config = root.join("local.toml");
        fs::write(&local_config, "[intern]\nprofile = \"local\"\n").unwrap();
        env::set_var("SYNTH_DESKTOP_CONFIG", &local_config);
        let resolved = resolve().unwrap();
        assert_eq!(resolved.backend_url, "http://127.0.0.1:8000");
        assert_eq!(
            resolved.responses_gateway_url.as_deref(),
            Some("http://127.0.0.1:41124")
        );

        // Legacy "prod" maps to the checked-in production gateway so the
        // default profile cannot fall through to the backend Responses route.
        let prod_config = root.join("prod.toml");
        fs::write(&prod_config, "[intern]\nprofile = \"prod\"\n").unwrap();
        env::set_var("SYNTH_DESKTOP_CONFIG", &prod_config);
        let resolved = resolve().unwrap();
        assert_eq!(resolved.backend_url, "https://api.usesynth.ai");
        assert_eq!(
            resolved.responses_gateway_url.as_deref(),
            Some("https://synth-responses-gateway-prod-production.up.railway.app")
        );

        env::remove_var("SYNTH_DESKTOP_CONFIG");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_explicit_multi_agent_versions() {
        assert_eq!(
            parse_multi_agent_version("disabled"),
            Some(MultiAgentVersion::None)
        );
        assert_eq!(parse_multi_agent_version("v1"), Some(MultiAgentVersion::V1));
        assert_eq!(parse_multi_agent_version("V2"), Some(MultiAgentVersion::V2));
    }
}
