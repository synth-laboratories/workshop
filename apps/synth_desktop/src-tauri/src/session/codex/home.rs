//! Codex home directory / config.toml / provider binding helpers.
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;

use crate::credential_broker::{self, CredentialBroker};
use crate::synth_config::MultiAgentVersion;

use super::proto::{
    CodexSessionInfo, CodexSessionRecord, CodexSessionStartRequest, Session,
    MIN_AUTO_COMPACT_TOKEN_LIMIT,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderClass {
    LocalLaguna,
    SynthCloud,
    OpenRouter,
    OpenaiCodexOauth,
    Direct,
}

pub fn provider_class(provider_name: Option<&str>) -> ProviderClass {
    match provider_name {
        Some(name) if name.eq_ignore_ascii_case("local-laguna") => ProviderClass::LocalLaguna,
        Some(name) if name.eq_ignore_ascii_case("synth-cloud") => ProviderClass::SynthCloud,
        Some(name) if name.eq_ignore_ascii_case("openrouter") => ProviderClass::OpenRouter,
        Some(name) if name.eq_ignore_ascii_case(crate::codex_oauth::PROVIDER_ID) => {
            ProviderClass::OpenaiCodexOauth
        }
        _ => ProviderClass::Direct,
    }
}

/// Point a session start request at the Synth Cloud provider.
///
/// `gateway_url` must already be the profile's resolved, fail-closed
/// Responses gateway — see `synth_config::require_responses_gateway_url`,
/// which every caller runs before reaching this function. This function
/// itself never falls back to a backend URL.
///
/// Fail-closed when the Synth API key is missing. Always overwrites any
/// renderer-supplied `api_key` / `base_url` / env key — credentials never
/// originate from the renderer.
///
/// The key is only *staged* here; `CodexManager::start` exchanges it for a
/// revocable loopback lease at spawn time, so what the child process, its
/// shell snapshots, and its config end up carrying is never the real key. See
/// `credential_broker`.
pub fn apply_synth_cloud_provider(
    request: &mut CodexSessionStartRequest,
    gateway_url: &str,
    api_key: Option<&str>,
) -> Result<(), String> {
    let key = api_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Synth API key not configured — Settings → Account".to_string())?;
    request.base_url = format!("{}/api/v1", normalize_gateway_origin(gateway_url));
    request.provider_name = Some("synth-cloud".into());
    request.provider_title = Some("Synth Cloud Responses".into());
    stage_brokered_credential(request, key)
}

/// Stage a user credential for native custody.
///
/// The request carries the real key only between preparation and spawn, and
/// only inside this process. Deliberately no lease is minted here: preparation
/// runs on every send, and minting for a session whose live child is about to
/// be reused would invalidate the token that child is still presenting. The
/// exchange happens in `CodexManager::start`, on its spawn path.
pub fn stage_brokered_credential(
    request: &mut CodexSessionStartRequest,
    api_key: &str,
) -> Result<(), String> {
    // Surface a malformed endpoint at preparation time, where the caller can
    // still map it to a typed provider failure for the renderer.
    validated_provider_endpoint(request)?;
    request.api_key = api_key.to_owned();
    request.broker_credential = true;
    Ok(())
}

/// Move a staged credential into native custody at spawn time.
///
/// The request keeps its logical endpoint path but is re-pointed at the
/// loopback proxy, and the staged key it carries becomes a revocable lease
/// token. Every provider whose credential belongs to the user — not to a local
/// loopback service — goes through here before a child process can observe it.
pub fn apply_brokered_credential(
    request: &mut CodexSessionStartRequest,
    broker: &CredentialBroker,
) -> Result<(), String> {
    let endpoint = validated_provider_endpoint(request)?;
    let lease = broker.lease(
        &request.session_id,
        &endpoint.origin().ascii_serialization(),
        &request.api_key,
    );
    request.base_url = format!("{}{}", lease.origin, endpoint.path().trim_end_matches('/'));
    request.api_key = lease.token;
    request.provider_env_key = Some(credential_broker::LEASE_ENV_KEY.into());
    request.broker_credential = false;
    Ok(())
}

pub(crate) fn validated_provider_endpoint(
    request: &CodexSessionStartRequest,
) -> Result<reqwest::Url, String> {
    reqwest::Url::parse(&request.base_url).map_err(|_| {
        format!(
            "{} could not start because its endpoint is invalid: {}. Update it in Settings → Account → Backend API.",
            request
                .provider_title
                .as_deref()
                .or(request.provider_name.as_deref())
                .unwrap_or("Selected provider"),
            safe_endpoint_label(&request.base_url)
        )
    })
}

/// Local services often advertise `0.0.0.0` as their bind address. That is
/// not a usable client destination, so turn it into loopback before Codex
/// validates or connects to the Synth Cloud Responses provider.
pub(crate) fn client_base_url(backend_url: &str) -> String {
    backend_url
        .trim()
        .trim_end_matches('/')
        .replacen("http://0.0.0.0:", "http://127.0.0.1:", 1)
}

/// A checked-in gateway URL may already include the `/api/v1` (or
/// `/api/v1/responses`) suffix `apply_synth_cloud_provider` is about to append.
/// Strip it first so the composed `base_url` never doubles the path.
pub(crate) fn normalize_gateway_origin(gateway_url: &str) -> String {
    let mut origin = client_base_url(gateway_url);
    for suffix in ["/api/v1/responses", "/api/v1"] {
        if let Some(stripped) = origin.strip_suffix(suffix) {
            origin = stripped.to_owned();
            break;
        }
    }
    origin
}

pub(crate) fn ensure_home(home: &Path, request: &CodexSessionStartRequest) -> Result<()> {
    fs::create_dir_all(home.join("sessions"))?;
    fs::write(home.join("AGENTS.md"), crate::context::WORKSHOP_AGENTS)?;
    let container_skill = home.join("skills/use-synth-containers");
    fs::create_dir_all(&container_skill)?;
    fs::write(
        container_skill.join("SKILL.md"),
        include_str!("../../../../skills/use-synth-containers/SKILL.md"),
    )?;
    let visuals_skill = home.join("skills/use-synth-visuals");
    fs::create_dir_all(visuals_skill.join("references"))?;
    fs::write(
        visuals_skill.join("SKILL.md"),
        include_str!("../../../../skills/use-synth-visuals/SKILL.md"),
    )?;
    fs::write(
        visuals_skill.join("references/visual-recipes.md"),
        include_str!("../../../../skills/use-synth-visuals/references/visual-recipes.md"),
    )?;
    let diagrams_skill = home.join("skills/author-synth-diagrams");
    fs::create_dir_all(diagrams_skill.join("references"))?;
    fs::write(
        diagrams_skill.join("SKILL.md"),
        include_str!("../../../../skills/author-synth-diagrams/SKILL.md"),
    )?;
    for (name, body) in [
        (
            "families.md",
            include_str!("../../../../skills/author-synth-diagrams/references/families.md"),
        ),
        (
            "flowchart.md",
            include_str!("../../../../skills/author-synth-diagrams/references/flowchart.md"),
        ),
        (
            "sequence.md",
            include_str!("../../../../skills/author-synth-diagrams/references/sequence.md"),
        ),
        (
            "class.md",
            include_str!("../../../../skills/author-synth-diagrams/references/class.md"),
        ),
        (
            "state.md",
            include_str!("../../../../skills/author-synth-diagrams/references/state.md"),
        ),
        (
            "er.md",
            include_str!("../../../../skills/author-synth-diagrams/references/er.md"),
        ),
        (
            "c4.md",
            include_str!("../../../../skills/author-synth-diagrams/references/c4.md"),
        ),
        (
            "feedback-loop.md",
            include_str!("../../../../skills/author-synth-diagrams/references/feedback-loop.md"),
        ),
        (
            "systems-map.md",
            include_str!("../../../../skills/author-synth-diagrams/references/systems-map.md"),
        ),
        (
            "dynamic-systems.md",
            include_str!("../../../../skills/author-synth-diagrams/references/dynamic-systems.md"),
        ),
    ] {
        fs::write(diagrams_skill.join("references").join(name), body)?;
    }
    let dynamic_explainers_skill = home.join("skills/author-time-dynamic-explainers");
    fs::create_dir_all(dynamic_explainers_skill.join("references"))?;
    fs::create_dir_all(dynamic_explainers_skill.join("agents"))?;
    fs::write(
        dynamic_explainers_skill.join("SKILL.md"),
        include_str!("../../../../skills/author-time-dynamic-explainers/SKILL.md"),
    )?;
    fs::write(
        dynamic_explainers_skill.join("agents/openai.yaml"),
        include_str!("../../../../skills/author-time-dynamic-explainers/agents/openai.yaml"),
    )?;
    for (name, body) in [
        (
            "visual-grammar.md",
            include_str!(
                "../../../../skills/author-time-dynamic-explainers/references/visual-grammar.md"
            ),
        ),
        (
            "motion-grammar.md",
            include_str!(
                "../../../../skills/author-time-dynamic-explainers/references/motion-grammar.md"
            ),
        ),
        (
            "pattern-library.md",
            include_str!(
                "../../../../skills/author-time-dynamic-explainers/references/pattern-library.md"
            ),
        ),
        (
            "review-checklist.md",
            include_str!(
                "../../../../skills/author-time-dynamic-explainers/references/review-checklist.md"
            ),
        ),
        (
            "observed-sources.md",
            include_str!(
                "../../../../skills/author-time-dynamic-explainers/references/observed-sources.md"
            ),
        ),
    ] {
        fs::write(dynamic_explainers_skill.join("references").join(name), body)?;
    }
    let optimizers_skill = home.join("skills/use-synth-optimizers");
    fs::create_dir_all(&optimizers_skill)?;
    fs::write(
        optimizers_skill.join("SKILL.md"),
        include_str!("../../../../skills/use-synth-optimizers/SKILL.md"),
    )?;
    let optimizers_references = optimizers_skill.join("references");
    fs::create_dir_all(&optimizers_references)?;
    fs::write(
        optimizers_references.join("gepa.md"),
        include_str!("../../../../skills/use-synth-optimizers/references/gepa.md"),
    )?;
    let plugins_skill = home.join("skills/use-synth-plugins");
    fs::create_dir_all(&plugins_skill)?;
    fs::write(
        plugins_skill.join("SKILL.md"),
        include_str!("../../../../skills/use-synth-plugins/SKILL.md"),
    )?;
    let session_skill = home.join("skills/use-synth-session");
    fs::create_dir_all(&session_skill)?;
    fs::write(
        session_skill.join("SKILL.md"),
        include_str!("../../../../skills/use-synth-session/SKILL.md"),
    )?;
    // Apply the durable Context settings after bundled materialization. This
    // keeps the existing reference-file setup intact while making disabled
    // skills and edited SKILL.md copies authoritative for new sessions.
    for id in [
        "use-synth-containers",
        "use-synth-visuals",
        "author-synth-diagrams",
        "use-synth-optimizers",
        "run-live-container-evals",
    ] {
        let directory = home.join("skills").join(id);
        if !crate::context::skill_enabled(id) {
            if directory.exists() {
                fs::remove_dir_all(&directory)?;
            }
        } else if let Some(body) = crate::context::skill_override(id) {
            fs::create_dir_all(&directory)?;
            fs::write(directory.join("SKILL.md"), body)?;
        }
    }
    if let Some(body) = crate::context::cookbook_skill(&crate::context::cookbook()) {
        let cookbook_skill = home.join("skills/use-synth-cookbooks");
        fs::create_dir_all(&cookbook_skill)?;
        fs::write(cookbook_skill.join("SKILL.md"), body)?;
    }
    let provider = request.provider_name.as_deref().unwrap_or("custom");
    let title = request
        .provider_title
        .as_deref()
        .unwrap_or("Synth Responses Provider");
    let env_key = request
        .provider_env_key
        .as_deref()
        .unwrap_or("SYNTH_LAGUNA_API_KEY");
    let multi_agent_version = request
        .multi_agent_version
        .unwrap_or(MultiAgentVersion::None);
    let (agents_enabled, multi_agent_v1, multi_agent_v2) = multi_agent_flags(multi_agent_version);
    let mut writable_roots = vec![request.workspace.clone()];
    writable_roots.extend(request.writable_roots.clone());
    writable_roots.sort();
    writable_roots.dedup();
    let workspace_write_config = workspace_write_config(&writable_roots);
    let compaction_config = if supports_provider_compaction(request) {
        // OpenAI and Azure Responses providers are recognized by Codex itself.
        // Leave their compaction configuration untouched so Codex uses the
        // provider-hosted /responses/compact implementation.
        String::new()
    } else {
        format!(
            "model_auto_compact_token_limit = {}\ntool_output_token_limit = 12000\ncompact_prompt = \"{}\"\n",
            auto_compact_token_limit(request),
            toml_string(super::proto::COMPACT_PROMPT)
        )
    };
    // The Synth Hosted Laguna gateway behind `synth-cloud` is itself a
    // stateless native Responses passthrough with no `previous_response_id`
    // session store (see `laguna_daemon.responses_api.backends.remote_responses`).
    // Telling Codex to disable response storage keeps both sides of the wire
    // consistent: Codex sends `store: false` and the full turn history on
    // every request instead of a bare `previous_response_id`, so nothing
    // here ever depends on a server-held session or a `submit_tool_outputs`
    // continuation the gateway cannot serve.
    let disable_response_storage_config = if requires_disabled_response_storage(request) {
        "disable_response_storage = true\n"
    } else {
        ""
    };
    let oauth = provider_class(request.provider_name.as_deref()) == ProviderClass::OpenaiCodexOauth;
    let auth_config = if oauth {
        "cli_auth_credentials_store = \"file\"\n"
    } else {
        ""
    };
    // A ChatGPT subscription uses the Codex auth.json file. Pointing this
    // provider at the local Laguna key makes Codex send that loopback secret as
    // a Bearer token instead of the ChatGPT OAuth access token.
    let env_key_config = if oauth {
        String::new()
    } else {
        format!("env_key = \"{}\"\n", toml_string(env_key))
    };
    let provider_base_url = if oauth {
        request.base_url.trim_end_matches('/').to_owned()
    } else {
        responses_base_url(&request.base_url)
    };
    let config = format!(
        "model = \"{}\"\nmodel_provider = \"{}\"\napproval_policy = \"{}\"\nsandbox_mode = \"{}\"\nservice_tier = \"{}\"\n{}{}{}\n{}[model_providers.{}]\nname = \"{}\"\nbase_url = \"{}\"\n{}wire_api = \"responses\"\nrequires_openai_auth = {}\n# Codex selects provider-hosted compaction for OpenAI/Azure and local summarization otherwise.\n\n[agents]\nenabled = {}\n\n[features]\nmulti_agent = {}\nmulti_agent_v2 = {}\ntool_call_mcp_elicitation = false\nshell_tool = true\nunified_exec = true\n",
        toml_string(&request.model), toml_string(provider), toml_string(request.approval_policy.as_deref().unwrap_or("untrusted")), toml_string(request.sandbox.as_deref().unwrap_or("workspace-write")), toml_string(request.service_tier.as_deref().unwrap_or("default")), auth_config, disable_response_storage_config, compaction_config, workspace_write_config, toml_key(provider), toml_string(title), toml_string(&provider_base_url), env_key_config, oauth, agents_enabled, multi_agent_v1, multi_agent_v2
    );
    fs::write(home.join("config.toml"), config)?;
    let auth = home.join("auth.json");
    if oauth {
        let credential: crate::codex_oauth::Credential = serde_json::from_str(&request.api_key)
            .context("ChatGPT subscription credential was unavailable")?;
        let body = serde_json::to_vec_pretty(&serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": credential.id_token,
                "access_token": credential.access_token,
                // The child receives only the already-refreshed, bounded
                // session credential. Long-lived refresh authority remains in
                // the native store and is never materialized in a shell-enabled
                // Codex home.
                "refresh_token": "synth-desktop-does-not-delegate-refresh",
                "account_id": credential.account_id,
            },
            "last_refresh": chrono::DateTime::from_timestamp_millis(credential.last_refresh_ms)
                .map(|value| value.to_rfc3339()),
        }))?;
        fs::write(&auth, body)?;
        set_private_file(&auth)?;
    } else if !auth.exists()
        || fs::read_to_string(&auth)
            .unwrap_or_default()
            .contains("\"auth_mode\": \"chatgpt\"")
    {
        fs::write(
            auth,
            "{\n  \"OPENAI_API_KEY\": \"synth-desktop-provider\"\n}\n",
        )?;
    }
    // Point Codex at the Rust noun adapters (all forward to CoreRuntime IPC).
    if let Ok(exe) = env::current_exe() {
        let ipc = crate::storage::app_data_root().join("visuals-ipc.json");
        let mut existing = fs::read_to_string(home.join("config.toml")).unwrap_or_default();
        for (server, binary) in [
            ("synth_plugins", "synth-plugins-mcp"),
            ("synth_containers", "synth-containers-mcp"),
            ("synth_visuals", "synth-visuals-mcp"),
            ("synth_optimizers", "synth-optimizers-mcp"),
            ("synth_session", "synth-session-mcp"),
        ] {
            if !crate::context::mcp_group_enabled("bundled") {
                continue;
            }
            if server == "synth_optimizers" && !crate::plugins::optimizers_plugin_enabled() {
                continue;
            }
            let bin = exe
                .parent()
                .map(|dir| dir.join(binary))
                .filter(|path| path.exists())
                .or_else(|| {
                    let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join(format!("target/debug/{binary}"));
                    candidate.exists().then_some(candidate)
                });
            let Some(bin) = bin else { continue };
            let heading = format!("[mcp_servers.{server}]");
            if existing.contains(&heading) {
                continue;
            }
            existing.push_str(&format!(
                "\n{heading}\ncommand = \"{}\"\nargs = []\n{}default_tools_approval_mode = \"approve\"\nenv = {{ {} = \"{}\", SYNTH_SESSION_ID = \"{}\" }}\n",
                toml_string(&bin.display().to_string()), mcp_enabled_tools(server), mcp_ipc_env_key(server), toml_string(&ipc.display().to_string()), toml_string(&request.session_id),
            ));
        }
        fs::write(home.join("config.toml"), existing)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<()> {
    Ok(())
}

/// Remove every materialized ChatGPT credential from Workshop-owned Codex homes.
pub fn scrub_oauth_auth_files() -> Result<()> {
    let homes = codex_root().join("homes");
    let Ok(entries) = fs::read_dir(homes) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let home = entry.path();
        let config = fs::read_to_string(home.join("config.toml")).unwrap_or_default();
        if config.contains(&format!(
            "model_provider = \"{}\"",
            crate::codex_oauth::PROVIDER_ID
        )) {
            match fs::remove_file(home.join("auth.json")) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

pub(crate) fn multi_agent_flags(version: MultiAgentVersion) -> (bool, bool, bool) {
    match version {
        MultiAgentVersion::None => (false, false, false),
        MultiAgentVersion::V1 => (true, true, false),
        MultiAgentVersion::V2 => (true, true, true),
    }
}

pub(crate) fn validate_start(request: &CodexSessionStartRequest) -> Result<()> {
    if request.session_id.trim().is_empty() || request.model.trim().is_empty() {
        return Err(anyhow!("sessionId and model are required"));
    }
    if !Path::new(&request.workspace).is_dir() {
        return Err(anyhow!("workspace must be an existing directory"));
    }
    if let Some(tier) = request.service_tier.as_deref() {
        if !matches!(tier, "default" | "fast") {
            return Err(anyhow!("serviceTier must be default or fast"));
        }
    }
    if !supports_provider_compaction(request) {
        let compact_limit = auto_compact_token_limit(request);
        let max_compact_limit = model_context_window(&request.model) * 9 / 10;
        if !(MIN_AUTO_COMPACT_TOKEN_LIMIT..=max_compact_limit).contains(&compact_limit) {
            return Err(anyhow!(
                "autoCompactTokenLimit must be between {MIN_AUTO_COMPACT_TOKEN_LIMIT} and {max_compact_limit} for {}",
                request.model
            ));
        }
    }
    if !(request.base_url.starts_with("http://127.0.0.1:")
        || request.base_url.starts_with("http://localhost:")
        || request.base_url.starts_with("https://"))
    {
        let provider = request
            .provider_title
            .as_deref()
            .or(request.provider_name.as_deref())
            .unwrap_or("Selected provider");
        return Err(anyhow!(
            "{provider} could not start because its endpoint is invalid: {}. Use an HTTPS endpoint, or a local endpoint such as http://127.0.0.1:<port>. Update it in Settings → Account → Backend API.",
            safe_endpoint_label(&request.base_url),
        ));
    }
    Ok(())
}

pub(crate) fn model_context_window(model: &str) -> u64 {
    if model.to_ascii_lowercase().contains("laguna-xs") {
        262_144
    } else if model.to_ascii_lowercase().contains("muse-spark-1.2") {
        1_048_576
    } else if model.to_ascii_lowercase().contains("laguna-s-2.1")
        || model.to_ascii_lowercase().contains("gpt-5.6-luna")
    {
        1_050_000
    } else {
        262_144
    }
}

pub(crate) fn auto_compact_token_limit(request: &CodexSessionStartRequest) -> u64 {
    let requested = request.auto_compact_token_limit.unwrap_or_else(|| {
        let model = request.model.to_ascii_lowercase();
        if model.contains("laguna-s-2.1")
            || model.contains("gpt-5.6-luna")
            || model.contains("muse-spark-1.2")
        {
            250_000
        } else if model.contains("laguna-xs") {
            150_000
        } else {
            model_context_window(&request.model) * 4 / 5
        }
    });

    // The renderer persists this control globally, while model context
    // windows are model-scoped. A value selected for Luna (250k) must not
    // make a smaller-window Codex model such as Terra fail to start.
    // Keep the lower-bound validation intact so malformed requests remain
    // actionable, but cap normal preferences at this model's safe maximum.
    requested.min(model_context_window(&request.model) * 9 / 10)
}

/// Whether Codex should disable server-side response storage for this
/// session's provider.
///
/// `synth-cloud` is the only provider this applies to today: the Synth
/// Hosted Laguna gateway behind it is a stateless native Responses
/// passthrough (`store: false` is forced upstream, and any
/// `previous_response_id` a client sends is dropped rather than resolved —
/// see `remote_responses.py`'s `_passthrough_body`). Setting
/// `disable_response_storage = true` makes Codex match that contract: it
/// sends `store: false` and full turn history with every request, so it
/// never depends on the gateway resolving a `previous_response_id` or
/// serving a `submit_tool_outputs` continuation against session state the
/// gateway does not keep.
pub(crate) fn requires_disabled_response_storage(request: &CodexSessionStartRequest) -> bool {
    request
        .provider_name
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case("synth-cloud"))
}

pub(crate) fn supports_provider_compaction(request: &CodexSessionStartRequest) -> bool {
    if request.provider_name.as_deref() == Some("openai")
        || request.provider_title.as_deref() == Some("OpenAI")
        || request
            .provider_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("azure"))
        || request
            .provider_title
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("azure"))
    {
        return true;
    }
    let base_url = request.base_url.to_ascii_lowercase();
    [
        "openai.azure.",
        "cognitiveservices.azure.",
        "aoai.azure.",
        "azure-api.",
        "azurefd.",
        "windows.net/openai",
    ]
    .iter()
    .any(|marker| base_url.contains(marker))
}

/// Produces an endpoint label that is useful in UI errors without exposing a
/// query-string token or credentials embedded in the URL authority.
pub(crate) fn safe_endpoint_label(endpoint: &str) -> String {
    let without_query = endpoint.trim().split(['?', '#']).next().unwrap_or_default();
    let redacted = match without_query.split_once("://") {
        Some((scheme, remainder)) => match remainder.split_once('@') {
            Some((_, host_and_path)) => format!("{scheme}://[credentials]@{host_and_path}"),
            None => without_query.to_owned(),
        },
        None => without_query.to_owned(),
    };
    const MAX_CHARS: usize = 160;
    if redacted.chars().count() <= MAX_CHARS {
        redacted
    } else {
        format!(
            "{}…",
            redacted
                .chars()
                .take(MAX_CHARS.saturating_sub(1))
                .collect::<String>()
        )
    }
}

pub(crate) fn validate_reasoning_effort(value: &str) -> Result<&str> {
    match value {
        "none" | "low" | "medium" | "high" | "xhigh" | "max" => Ok(value),
        _ => Err(anyhow!("unsupported reasoning effort: {value}")),
    }
}

pub(crate) fn nested_id(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            payload
                .get(key.trim_end_matches("Id"))
                .and_then(|nested| nested.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

pub(crate) async fn session_info(id: &str, session: &Session) -> CodexSessionInfo {
    CodexSessionInfo {
        session_id: id.into(),
        thread_id: session.thread_id.clone(),
        turn_id: session.turn_id.read().await.clone(),
    }
}

pub fn codex_root() -> PathBuf {
    env::var_os("SYNTH_CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::instance::state_root().join("codex"))
}

pub fn oauth_auth_path(session_id: &str) -> PathBuf {
    codex_root()
        .join("homes")
        .join(safe_component(session_id))
        .join("auth.json")
}

pub(crate) async fn persist_records(
    records: &Arc<RwLock<HashMap<String, CodexSessionRecord>>>,
    state_path: &Path,
) -> Result<()> {
    if let Some(parent) = state_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let data = serde_json::to_vec_pretty(&*records.read().await)?;
    let temporary = state_path.with_extension("json.tmp");
    tokio::fs::write(&temporary, data).await?;
    tokio::fs::rename(temporary, state_path).await?;
    Ok(())
}
pub(crate) fn safe_component(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}
pub(crate) fn toml_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
pub(crate) fn toml_key(value: &str) -> String {
    format!("\"{}\"", toml_string(value))
}

pub(crate) fn workspace_write_config(allowed_roots: &[String]) -> String {
    if allowed_roots.is_empty() {
        return String::new();
    }
    let roots = allowed_roots
        .iter()
        .map(|root| format!("\"{}\"", toml_string(root)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[sandbox_workspace_write]\nwritable_roots = [{roots}]\n\n")
}

pub(crate) fn mcp_enabled_tools(server: &str) -> &'static str {
    match server {
        // Codex sees one compact namespace member. The adapter keeps legacy
        // tools callable for other MCP clients, while visual_manage routes the
        // same operations after the visual skill is loaded.
        "synth_visuals" => "enabled_tools = [\"visual_manage\"]\n",
        "synth_optimizers" => "enabled_tools = [\"optimizer_manage\"]\n",
        "synth_plugins" => "enabled_tools = [\"plugin_manage\"]\n",
        "synth_session" => "enabled_tools = [\"session_present\"]\n",
        _ => "",
    }
}

pub(crate) fn mcp_ipc_env_key(server: &str) -> &'static str {
    match server {
        "synth_visuals" => "SYNTH_VISUALS_IPC_FILE",
        _ => "SYNTH_DESKTOP_IPC_FILE",
    }
}

/// Codex appends `/responses` to the provider base URL. Laguna and standard
/// OpenAI-compatible providers expose that endpoint below `/v1`.
pub(crate) fn responses_base_url(value: &str) -> String {
    let trimmed = value.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_owned()
    } else {
        format!("{trimmed}/v1")
    }
}

pub(crate) fn automatic_thread_title(prompt: &str) -> Option<String> {
    let mut value = prompt
        .lines()
        .find(|line| !line.trim().is_empty())?
        .trim()
        .trim_start_matches(|c: char| matches!(c, '-' | '*' | '#' | '>' | ' '))
        .to_owned();
    for prefix in [
        "please ",
        "can you ",
        "could you ",
        "would you ",
        "i want you to ",
        "i need you to ",
    ] {
        if value.to_ascii_lowercase().starts_with(prefix) {
            value = value[prefix.len()..].trim_start().to_owned();
            break;
        }
    }
    let words = value.split_whitespace().collect::<Vec<_>>();
    let skip_skill_preamble = words
        .first()
        .is_some_and(|word| word.eq_ignore_ascii_case("use"))
        && words.get(1).is_some_and(|word| word.starts_with('$'));
    value = words
        .into_iter()
        .enumerate()
        .filter(|(index, word)| {
            !word.starts_with('$')
                && !(skip_skill_preamble && *index == 0)
                && !(skip_skill_preamble && *index == 2 && word.eq_ignore_ascii_case("to"))
        })
        .map(|(_, word)| word)
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(index) = value.find(['\n', '.', '?', '!']) {
        value.truncate(index);
    }
    value = value
        .trim_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | ';' | '-' | '—'))
        .to_owned();
    if value.is_empty() {
        return None;
    }
    const MAX_CHARS: usize = 56;
    if value.chars().count() > MAX_CHARS {
        let mut shortened = String::new();
        for word in value.split_whitespace() {
            let next_len = shortened.chars().count()
                + usize::from(!shortened.is_empty())
                + word.chars().count();
            if next_len > MAX_CHARS {
                break;
            }
            if !shortened.is_empty() {
                shortened.push(' ');
            }
            shortened.push_str(word);
        }
        value = shortened;
    }
    let mut chars = value.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().collect::<String>() + chars.as_str())
}
