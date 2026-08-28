//! Rust-owned model catalog for the Desktop picker and session identity.
//!
//! The renderer never parses config.toml or guesses an OpenRouter model's
//! capabilities.  It receives this projection, which is available from the
//! public metadata cache immediately and can be refreshed separately.

use crate::synth_config::{
    self, OpenRouterModelConfig, OpenRouterModelUiConfig, OpenRouterReasoningControl,
};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use futures_util::StreamExt;
use reqwest::header::{ETAG, IF_NONE_MATCH};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf, time::Duration};

const OPENROUTER_MODELS_URL: &str = "https://openrouter.ai/api/v1/models";
const CACHE_FILE: &str = "openrouter-model-metadata-v1.json";
const CACHE_VERSION: u8 = 1;
const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ModelCatalogSource {
    Builtin,
    UserConfig,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelCatalogAvailability {
    Ready,
    CredentialRequired,
    Unverified,
    Unavailable,
    Expired,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelCatalogReasoningControl {
    None,
    Binary,
    Effort,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogCapabilities {
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub tools: bool,
    pub reasoning_control: ModelCatalogReasoningControl,
    pub default_reasoning: Option<String>,
    /// JavaScript-safe numeric values for the generated Tauri contract.
    /// OpenRouter model limits are far below `Number.MAX_SAFE_INTEGER`.
    pub max_context_tokens: Option<f64>,
    pub max_completion_tokens: Option<f64>,
}

impl Default for ModelCatalogCapabilities {
    fn default() -> Self {
        Self {
            input_modalities: vec!["text".into()],
            output_modalities: vec!["text".into()],
            tools: false,
            reasoning_control: ModelCatalogReasoningControl::None,
            default_reasoning: None,
            max_context_tokens: None,
            max_completion_tokens: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogEntry {
    pub target_id: String,
    pub provider: String,
    pub model_id: String,
    pub display_name: String,
    pub source: ModelCatalogSource,
    pub enabled: bool,
    pub availability: ModelCatalogAvailability,
    pub capabilities: ModelCatalogCapabilities,
    pub metadata_observed_at: Option<String>,
    pub metadata_source: Option<String>,
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogDiagnostic {
    pub location: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub entries: Vec<ModelCatalogEntry>,
    pub diagnostics: Vec<ModelCatalogDiagnostic>,
    pub generated_at: String,
}

#[derive(Clone, Debug)]
struct Candidate {
    target_id: String,
    model_id: String,
    display_name: Option<String>,
    source: ModelCatalogSource,
    enabled: bool,
    routing_configured: bool,
    ui: Option<OpenRouterModelUiConfig>,
    builtin_capabilities: Option<ModelCatalogCapabilities>,
}

/// Read config and the last public metadata snapshot only.  This function does
/// no network I/O and is safe to call on the application startup path.
pub fn catalog() -> Result<ModelCatalog> {
    let config = synth_config::openrouter_model_configs()?;
    let cache = read_cache();
    Ok(project_catalog(config, cache.as_ref(), Vec::new()))
}

/// Refresh OpenRouter's public model list with a short, bounded request.  A
/// refresh failure is returned as an inspectable catalog diagnostic instead of
/// making the picker disappear or blocking startup.
pub async fn refresh() -> Result<ModelCatalog> {
    let config = synth_config::openrouter_model_configs()?;
    let cache = read_cache();
    let candidates = candidates(&config.models);
    match fetch_metadata(cache.as_ref(), &candidates).await {
        Ok(Some(next)) => {
            if let Err(error) = write_cache(&next) {
                return Ok(project_catalog(
                    config,
                    Some(&next),
                    vec![format!(
                        "OpenRouter metadata was fetched but could not be cached: {error}"
                    )],
                ));
            }
            Ok(project_catalog(config, Some(&next), Vec::new()))
        }
        Ok(None) => Ok(project_catalog(config, cache.as_ref(), Vec::new())),
        Err(error) => Ok(project_catalog(
            config,
            cache.as_ref(),
            vec![format!("OpenRouter metadata refresh failed: {error}")],
        )),
    }
}

fn project_catalog(
    config: synth_config::OpenRouterModelConfigSet,
    cache: Option<&MetadataCache>,
    refresh_diagnostics: Vec<String>,
) -> ModelCatalog {
    let credential_configured = synth_config::openrouter_api_key().ok().flatten().is_some();
    let mut diagnostics = config
        .diagnostics
        .into_iter()
        .map(|message| ModelCatalogDiagnostic {
            location: config_location(&message),
            message,
        })
        .collect::<Vec<_>>();
    diagnostics.extend(
        refresh_diagnostics
            .into_iter()
            .map(|message| ModelCatalogDiagnostic {
                location: "OpenRouter metadata".into(),
                message,
            }),
    );

    let entries = candidates(&config.models)
        .into_iter()
        .map(|candidate| {
            let metadata = cache.and_then(|cache| cache.models.get(&candidate.model_id));
            let unavailable_at = cache
                .and_then(|cache| cache.missing_models.get(&candidate.model_id))
                .cloned();
            let expired = metadata
                .and_then(|metadata| metadata.expires_at.as_deref())
                .and_then(parse_timestamp)
                .is_some_and(|expiration| expiration <= Utc::now());
            let availability = if candidate.routing_configured {
                // The current Responses adapter has no admitted OpenRouter
                // routing envelope. Do not silently drop a user policy.
                ModelCatalogAvailability::Unavailable
            } else if !candidate.enabled {
                ModelCatalogAvailability::Unavailable
            } else if expired {
                ModelCatalogAvailability::Expired
            } else if unavailable_at.is_some() {
                ModelCatalogAvailability::Unavailable
            } else if metadata.is_some() || candidate.builtin_capabilities.is_some() {
                if credential_configured {
                    ModelCatalogAvailability::Ready
                } else {
                    ModelCatalogAvailability::CredentialRequired
                }
            } else if credential_configured {
                ModelCatalogAvailability::Unverified
            } else {
                ModelCatalogAvailability::CredentialRequired
            };

            let mut capabilities = metadata
                .map(|metadata| metadata.capabilities.clone())
                .or_else(|| candidate.builtin_capabilities.clone())
                .unwrap_or_default();
            narrow_reasoning_capability(&mut capabilities, candidate.ui.as_ref());
            let diagnostic = if candidate.routing_configured {
                Some("This routing policy is not admitted by the current Responses adapter, so new turns are blocked rather than sent without it.".into())
            } else if !candidate.enabled {
                Some("Disabled in config.toml; existing sessions retain their pinned target.".into())
            } else if expired {
                Some("OpenRouter metadata reports this model as expired; new turns are blocked.".into())
            } else if let Some(observed_at) = unavailable_at {
                Some(format!(
                    "OpenRouter did not list this slug when metadata was checked {observed_at}; new turns are blocked."
                ))
            } else if metadata.is_none() && candidate.builtin_capabilities.is_none() {
                Some("OpenRouter metadata has not been verified. Only text input is admitted until refresh succeeds.".into())
            } else if !credential_configured {
                Some("OpenRouter API key required for new turns.".into())
            } else {
                None
            };
            ModelCatalogEntry {
                target_id: candidate.target_id,
                provider: "openrouter".into(),
                model_id: candidate.model_id.clone(),
                display_name: candidate
                    .display_name
                    .or_else(|| metadata.map(|metadata| metadata.display_name.clone()))
                    .unwrap_or_else(|| candidate.model_id.clone()),
                source: candidate.source,
                enabled: candidate.enabled,
                availability,
                capabilities,
                metadata_observed_at: metadata
                    .map(|metadata| metadata.observed_at.clone()),
                metadata_source: metadata
                    .map(|metadata| metadata.source.clone())
                    .or_else(|| candidate.builtin_capabilities.as_ref().map(|_| "builtin".into())),
                diagnostic,
            }
        })
        .collect();
    ModelCatalog {
        entries,
        diagnostics,
        generated_at: Utc::now().to_rfc3339(),
    }
}

fn config_location(message: &str) -> String {
    message
        .split(": [[models.openrouter]]")
        .next()
        .unwrap_or("config.toml")
        .to_owned()
}

fn candidates(configured: &[OpenRouterModelConfig]) -> Vec<Candidate> {
    let mut entries = builtin_candidates();
    entries.extend(configured.iter().map(|entry| Candidate {
        target_id: format!("openrouter:{}", entry.id),
        model_id: entry.model.clone(),
        display_name: entry.display_name.clone(),
        source: ModelCatalogSource::UserConfig,
        enabled: entry.enabled,
        routing_configured: entry.routing.as_ref().is_some_and(|routing| {
            routing.allow_fallbacks.is_some()
                || routing.require_parameters.is_some()
                || routing.data_collection.is_some()
        }),
        ui: entry.ui.clone(),
        builtin_capabilities: None,
    }));
    entries
}

fn builtin_candidates() -> Vec<Candidate> {
    vec![
        builtin(
            "openrouter-luna",
            "openai/gpt-5.6-luna",
            "GPT 5.6 Luna",
            ModelCatalogCapabilities {
                input_modalities: vec!["text".into(), "image".into()],
                output_modalities: vec!["text".into()],
                tools: true,
                reasoning_control: ModelCatalogReasoningControl::Effort,
                default_reasoning: Some("xhigh".into()),
                max_context_tokens: Some(272_000.0),
                max_completion_tokens: None,
            },
        ),
        builtin(
            "openrouter-laguna-s",
            "poolside/laguna-s-2.1",
            "Laguna S 2.1",
            ModelCatalogCapabilities {
                input_modalities: vec!["text".into()],
                output_modalities: vec!["text".into()],
                tools: true,
                reasoning_control: ModelCatalogReasoningControl::Binary,
                default_reasoning: Some("max".into()),
                max_context_tokens: Some(262_144.0),
                max_completion_tokens: None,
            },
        ),
        builtin(
            "openrouter-muse-spark",
            "meta/muse-spark-1.2",
            "Muse Spark 1.2",
            ModelCatalogCapabilities {
                input_modalities: vec!["text".into(), "image".into()],
                output_modalities: vec!["text".into()],
                tools: true,
                reasoning_control: ModelCatalogReasoningControl::Effort,
                default_reasoning: Some("medium".into()),
                max_context_tokens: Some(1_048_576.0),
                max_completion_tokens: None,
            },
        ),
        builtin(
            "openrouter-gemini-flash",
            "google/gemini-3.7-flash",
            "Gemini 3.7 Flash",
            ModelCatalogCapabilities {
                input_modalities: vec!["text".into(), "image".into()],
                output_modalities: vec!["text".into()],
                tools: true,
                reasoning_control: ModelCatalogReasoningControl::Effort,
                default_reasoning: Some("medium".into()),
                max_context_tokens: Some(1_048_576.0),
                max_completion_tokens: None,
            },
        ),
    ]
}

fn builtin(
    target_id: &str,
    model_id: &str,
    display_name: &str,
    capabilities: ModelCatalogCapabilities,
) -> Candidate {
    Candidate {
        target_id: target_id.into(),
        model_id: model_id.into(),
        display_name: Some(display_name.into()),
        source: ModelCatalogSource::Builtin,
        enabled: true,
        routing_configured: false,
        ui: None,
        builtin_capabilities: Some(capabilities),
    }
}

fn narrow_reasoning_capability(
    capabilities: &mut ModelCatalogCapabilities,
    ui: Option<&OpenRouterModelUiConfig>,
) {
    let Some(ui) = ui else { return };
    if let Some(control) = ui.reasoning_control {
        capabilities.reasoning_control = match (control, capabilities.reasoning_control) {
            (OpenRouterReasoningControl::None, _) => ModelCatalogReasoningControl::None,
            (OpenRouterReasoningControl::Binary, ModelCatalogReasoningControl::None) => {
                ModelCatalogReasoningControl::None
            }
            (OpenRouterReasoningControl::Binary, _) => ModelCatalogReasoningControl::Binary,
            (OpenRouterReasoningControl::Effort, ModelCatalogReasoningControl::Effort) => {
                ModelCatalogReasoningControl::Effort
            }
            (OpenRouterReasoningControl::Effort, _) => ModelCatalogReasoningControl::None,
        };
    }
    capabilities.default_reasoning =
        if capabilities.reasoning_control == ModelCatalogReasoningControl::None {
            None
        } else {
            ui.default_reasoning
                .clone()
                .or_else(|| capabilities.default_reasoning.clone())
        };
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct MetadataCache {
    version: u8,
    observed_at: Option<String>,
    etag: Option<String>,
    models: HashMap<String, ObservedModel>,
    missing_models: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ObservedModel {
    display_name: String,
    capabilities: ModelCatalogCapabilities,
    observed_at: String,
    source: String,
    expires_at: Option<String>,
}

impl<'de> Deserialize<'de> for ModelCatalogCapabilities {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            input_modalities: Vec<String>,
            output_modalities: Vec<String>,
            tools: bool,
            reasoning_control: ModelCatalogReasoningControl,
            default_reasoning: Option<String>,
            max_context_tokens: Option<f64>,
            max_completion_tokens: Option<f64>,
        }
        let wire = Wire::deserialize(deserializer)?;
        Ok(Self {
            input_modalities: wire.input_modalities,
            output_modalities: wire.output_modalities,
            tools: wire.tools,
            reasoning_control: wire.reasoning_control,
            default_reasoning: wire.default_reasoning,
            max_context_tokens: wire.max_context_tokens,
            max_completion_tokens: wire.max_completion_tokens,
        })
    }
}

fn cache_path() -> PathBuf {
    crate::instance::state_root().join(CACHE_FILE)
}

fn read_cache() -> Option<MetadataCache> {
    let raw = fs::read(cache_path()).ok()?;
    let cache = serde_json::from_slice::<MetadataCache>(&raw).ok()?;
    (cache.version == CACHE_VERSION).then_some(cache)
}

fn write_cache(cache: &MetadataCache) -> Result<()> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let contents = serde_json::to_vec(cache)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, contents)?;
    fs::rename(&temporary, &path)
        .with_context(|| format!("replace metadata cache {}", path.display()))?;
    Ok(())
}

async fn fetch_metadata(
    cache: Option<&MetadataCache>,
    candidates: &[Candidate],
) -> Result<Option<MetadataCache>> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(7))
        .build()?;
    let mut request = client.get(OPENROUTER_MODELS_URL);
    if let Some(etag) = cache.and_then(|cache| cache.etag.as_deref()) {
        request = request.header(IF_NONE_MATCH, etag);
    }
    let response = request.send().await?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(anyhow!("OpenRouter returned {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_METADATA_BYTES as u64)
    {
        return Err(anyhow!(
            "OpenRouter metadata response exceeded the {} MiB limit",
            MAX_METADATA_BYTES / 1024 / 1024
        ));
    }
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len().saturating_add(chunk.len()) > MAX_METADATA_BYTES {
            return Err(anyhow!(
                "OpenRouter metadata response exceeded the {} MiB limit",
                MAX_METADATA_BYTES / 1024 / 1024
            ));
        }
        body.extend_from_slice(&chunk);
    }
    let response: OpenRouterModelsResponse =
        serde_json::from_slice(&body).context("invalid OpenRouter public models response")?;
    let observed_at = Utc::now().to_rfc3339();
    let api_models = response
        .data
        .into_iter()
        .map(|model| (model.id.clone(), model))
        .collect::<HashMap<_, _>>();
    let mut models = HashMap::new();
    let mut missing_models = HashMap::new();
    for candidate in candidates {
        // Presets are provider-owned aliases and may not appear in the public
        // list. Keep them unverified until the provider resolves them.
        if candidate.model_id.starts_with("@preset/") {
            continue;
        }
        match api_models.get(&candidate.model_id) {
            Some(model) => {
                models.insert(
                    candidate.model_id.clone(),
                    ObservedModel::from_api(model, &observed_at),
                );
            }
            None => {
                missing_models.insert(candidate.model_id.clone(), observed_at.clone());
            }
        }
    }
    Ok(Some(MetadataCache {
        version: CACHE_VERSION,
        observed_at: Some(observed_at),
        etag,
        models,
        missing_models,
    }))
}

#[derive(Debug, Deserialize)]
struct OpenRouterModelsResponse {
    #[serde(default)]
    data: Vec<OpenRouterApiModel>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterApiModel {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    top_provider: Option<OpenRouterTopProvider>,
    #[serde(default)]
    architecture: Option<OpenRouterArchitecture>,
    #[serde(default)]
    supported_parameters: Vec<String>,
    #[serde(default, rename = "expiration_date", alias = "expires_at")]
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterTopProvider {
    #[serde(default)]
    max_completion_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterArchitecture {
    #[serde(default)]
    input_modalities: Vec<String>,
    #[serde(default)]
    output_modalities: Vec<String>,
}

impl ObservedModel {
    fn from_api(model: &OpenRouterApiModel, observed_at: &str) -> Self {
        let parameters = model
            .supported_parameters
            .iter()
            .map(|parameter| parameter.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let reasoning_control = if parameters
            .iter()
            .any(|parameter| parameter == "reasoning_effort")
        {
            ModelCatalogReasoningControl::Effort
        } else if parameters
            .iter()
            .any(|parameter| matches!(parameter.as_str(), "reasoning" | "include_reasoning"))
        {
            ModelCatalogReasoningControl::Binary
        } else {
            ModelCatalogReasoningControl::None
        };
        let architecture = model.architecture.as_ref();
        let mut input_modalities = architecture
            .map(|architecture| architecture.input_modalities.clone())
            .unwrap_or_default();
        if input_modalities.is_empty() {
            input_modalities.push("text".into());
        }
        let mut output_modalities = architecture
            .map(|architecture| architecture.output_modalities.clone())
            .unwrap_or_default();
        if output_modalities.is_empty() {
            output_modalities.push("text".into());
        }
        Self {
            display_name: model.name.clone().unwrap_or_else(|| model.id.clone()),
            capabilities: ModelCatalogCapabilities {
                input_modalities,
                output_modalities,
                tools: parameters
                    .iter()
                    .any(|parameter| parameter == "tools" || parameter == "tool_choice"),
                reasoning_control,
                default_reasoning: None,
                max_context_tokens: model.context_length.map(|value| value as f64),
                max_completion_tokens: model
                    .top_provider
                    .as_ref()
                    .and_then(|provider| provider.max_completion_tokens)
                    .map(|value| value as f64),
            },
            observed_at: observed_at.into(),
            source: "openrouter.models.v1".into(),
            expires_at: model.expires_at.clone(),
        }
    }
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)
                .map(|timestamp| timestamp.and_utc())
        })
}

