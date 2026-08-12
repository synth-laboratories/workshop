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
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackendSettingsUpdate {
    pub profile: String,
    pub backend_url: String,
    pub env_file: String,
    pub api_key_env: String,
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
    let document = read_toml(&resolved.config_path)?;
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
    })
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

    get()
}

/// Persist a Synth API key obtained by device pairing into the configured
/// 0600 env file, without touching routing config. Renderer never calls this.
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
    let endpoint = env::var("SYNTH_BACKEND_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            intern
                .and_then(|v| v.get("endpoints"))
                .and_then(toml::Value::as_table)
                .and_then(|v| v.get(&profile))
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| lookup_default(DEFAULT_ENDPOINTS, &profile))
        .unwrap_or_else(|| match profile.as_str() {
            "staging" => "https://api-dev.usesynth.ai".into(),
            "local" => "http://127.0.0.1:8000".into(),
            _ => "https://api.usesynth.ai".into(),
        });
    // Named development instances may point their isolated cloud lane at a
    // disposable local Responses backend. Canonical apps retain source-owned
    // routing and deliberately ignore this process override.
    let responses_gateway_url = if env::var_os(crate::instance::DATA_ROOT_ENV).is_some() {
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
    env::var_os("SYNTH_DESKTOP_CONFIG")
        .or_else(|| env::var_os("SYNTH_INTERN_CONFIG"))
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::instance::state_root().join("config.toml"))
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
    fs::read_to_string(path)
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
    if !(value.starts_with("http://") || value.starts_with("https://"))
        || value.contains(char::is_whitespace)
    {
        return Err(anyhow!("backend URL must be an http:// or https:// URL"));
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
    fn backend_settings_update_ignores_legacy_renderer_key_fields() {
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
