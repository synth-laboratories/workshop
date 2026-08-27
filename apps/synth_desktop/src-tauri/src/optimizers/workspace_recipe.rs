//! Workspace-declared optimizer recipes and container specs.
//!
//! Workshop does not ship task identity. A session workspace may declare
//! `workshop.recipe.toml`, `workshop.recipes/*.toml`, and
//! `workshop.containers.toml`. The host validates, clamps bounds to product
//! caps, copies the recipe into a run-owned directory, and executes that copy.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Product ceiling. Workspace `[bounds]` may be stricter, never looser.
pub const PRODUCT_MAX_COST_USD: f64 = 2.45;
pub const PRODUCT_MAX_TOTAL_ROLLOUTS: i64 = 240;

const RECIPE_FILE: &str = "workshop.recipe.toml";
const RECIPES_DIR: &str = "workshop.recipes";
const CONTAINERS_FILE: &str = "workshop.containers.toml";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlgorithmKind {
    Gepa,
    Eval,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PolicyLocality {
    Host,
    Container,
}

impl PolicyLocality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Container => "container",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RecipeBounds {
    pub max_cost_usd: f64,
    pub max_total_rollouts: i64,
    pub max_train_rollouts: Option<i64>,
    pub max_heldout_rollouts: Option<i64>,
    pub max_generations: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct WorkspaceRecipe {
    pub id: String,
    pub algorithm: AlgorithmKind,
    pub title: String,
    pub container: String,
    pub provider: String,
    pub model: String,
    pub locality: PolicyLocality,
    pub credential_mode: String,
    pub bounds: RecipeBounds,
    pub family: String,
    pub harness: String,
    pub policy_config: String,
    pub policy: serde_json::Map<String, Value>,
    pub train_seeds: Vec<i64>,
    pub heldout_seeds: Vec<i64>,
    pub concurrency: usize,
    pub proposer_model: Option<String>,
    pub requires_credential_advertisement: bool,
    /// How this recipe wants a running rollout's event journal drained and its
    /// native frames retained. Declared, never inferred: a 500-step Craftax
    /// episode and a two-call classifier do not want the same cadence, and the
    /// difference must not be a code edit.
    pub relay: super::eval_relay::RelaySettings,
    pub source_path: PathBuf,
    pub source_hash: String,
}

#[derive(Clone, Debug)]
pub struct ContainerSpec {
    pub id: String,
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub url: Option<String>,
    pub health: String,
    pub contract: String,
    pub locality: PolicyLocality,
    pub family: Option<String>,
    pub credential_providers: Vec<String>,
    pub environment: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct RecipeFile {
    id: String,
    algorithm: AlgorithmKind,
    #[serde(default)]
    title: Option<String>,
    container: String,
    #[serde(default = "default_provider")]
    provider: String,
    model: String,
    locality: PolicyLocality,
    #[serde(default = "default_credential_mode")]
    credential_mode: String,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    harness: Option<String>,
    #[serde(default)]
    policy_config: Option<String>,
    #[serde(default)]
    policy: toml::value::Table,
    #[serde(default)]
    proposer_model: Option<String>,
    #[serde(default)]
    requires_credential_advertisement: bool,
    #[serde(default)]
    concurrency: Option<usize>,
    #[serde(default)]
    bounds: Option<BoundsFile>,
    #[serde(default)]
    train_seeds: Option<Vec<i64>>,
    #[serde(default)]
    heldout_seeds: Option<Vec<i64>>,
    #[serde(default)]
    event_stream: Option<EventStreamFile>,
    #[serde(default)]
    media: Option<MediaFile>,
}

#[derive(Deserialize, Default)]
struct EventStreamFile {
    poll_interval_ms: Option<u64>,
    page_limit: Option<u32>,
    max_events_per_rollout: Option<usize>,
}

#[derive(Deserialize, Default)]
struct MediaFile {
    frame_retention: Option<String>,
    max_frame_bytes: Option<u64>,
    max_frames_per_rollout: Option<usize>,
    max_total_frame_bytes: Option<u64>,
}

#[derive(Deserialize, Default)]
struct BoundsFile {
    max_cost_usd: Option<f64>,
    max_total_rollouts: Option<i64>,
    max_train_rollouts: Option<i64>,
    max_heldout_rollouts: Option<i64>,
    max_generations: Option<i64>,
}

#[derive(Deserialize)]
struct ContainersFile {
    #[serde(default)]
    container: Vec<ContainerFile>,
}

#[derive(Deserialize)]
struct ContainerFile {
    id: String,
    #[serde(default)]
    command: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default = "default_health")]
    health: String,
    #[serde(default = "default_contract")]
    contract: String,
    locality: PolicyLocality,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    credential_providers: Vec<String>,
    #[serde(default)]
    environment: std::collections::BTreeMap<String, String>,
}

fn default_provider() -> String {
    "openai".into()
}
fn default_credential_mode() -> String {
    "proxy".into()
}
fn default_health() -> String {
    "/health".into()
}
fn default_contract() -> String {
    "synth-containers/v1".into()
}

pub fn session_workspace(
    db: &crate::storage::Database,
    session_id: &str,
) -> Result<Option<PathBuf>> {
    let id = session_id.to_string();
    let workspace: Option<String> = db.with_conn(|conn| {
        use rusqlite::OptionalExtension;
        conn.query_row(
            "SELECT workspace FROM conversation_workspace_scopes WHERE session_id=?1",
            [&id],
            |row| row.get(0),
        )
        .optional()
        .map_err(anyhow::Error::from)
    })?;
    match workspace {
        Some(path) if !path.trim().is_empty() => {
            Ok(Some(crate::workspace_scope::canonical_directory(&path)?))
        }
        _ => Ok(None),
    }
}

pub fn require_session_workspace(
    db: &crate::storage::Database,
    session_id: &str,
) -> Result<PathBuf> {
    session_workspace(db, session_id)?.ok_or_else(|| {
        anyhow!("session `{session_id}` has no workspace; declare workshop.recipe.toml there")
    })
}

pub fn load_recipes(workspace: &Path) -> Result<Vec<WorkspaceRecipe>> {
    let mut recipes = Vec::new();
    let root_file = workspace.join(RECIPE_FILE);
    if root_file.is_file() {
        recipes.push(parse_recipe(&root_file)?);
    }
    let recipes_dir = workspace.join(RECIPES_DIR);
    if recipes_dir.is_dir() {
        let mut entries: Vec<PathBuf> = fs::read_dir(&recipes_dir)
            .with_context(|| format!("read {}", recipes_dir.display()))?
            .filter_map(|entry| entry.ok().map(|item| item.path()))
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
            })
            .collect();
        entries.sort();
        for path in entries {
            recipes.push(parse_recipe(&path)?);
        }
    }
    let mut seen = std::collections::HashSet::new();
    for recipe in &recipes {
        if !seen.insert(recipe.id.as_str()) {
            bail!("workspace declares recipe id `{}` more than once", recipe.id);
        }
    }
    Ok(recipes)
}

pub fn find_recipe(workspace: &Path, recipe_id: &str) -> Result<WorkspaceRecipe> {
    load_recipes(workspace)?
        .into_iter()
        .find(|recipe| recipe.id == recipe_id)
        .ok_or_else(|| {
            anyhow!(
                "workspace recipe `{recipe_id}` is not declared in {} or {}/",
                RECIPE_FILE,
                RECIPES_DIR
            )
        })
}

pub fn load_container_specs(workspace: &Path) -> Result<Vec<ContainerSpec>> {
    let path = workspace.join(CONTAINERS_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))?;
    parse_containers(workspace, &text)
}

pub fn find_container_spec(workspace: &Path, spec_id: &str) -> Result<ContainerSpec> {
    load_container_specs(workspace)?
        .into_iter()
        .find(|spec| spec.id == spec_id)
        .ok_or_else(|| {
            anyhow!("container spec `{spec_id}` is not declared in {CONTAINERS_FILE}")
        })
}

pub fn catalog_entry(recipe: &WorkspaceRecipe) -> Value {
    let credential_input = match recipe.provider.to_ascii_lowercase().as_str() {
        "openrouter" => "OPENROUTER_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        _ => "OPENAI_API_KEY",
    };
    json!({
        "id": recipe.id,
        "title": recipe.title,
        "algorithmId": match recipe.algorithm {
            AlgorithmKind::Gepa => "gepa",
            AlgorithmKind::Eval => "eval",
        },
        "task": recipe.family,
        "source": "workspace",
        "availability": "available",
        "semantics": match recipe.algorithm {
            AlgorithmKind::Gepa => "gepa_optimization",
            AlgorithmKind::Eval => "baseline_eval",
        },
        "locality": recipe.locality.as_str(),
        "container": recipe.container,
        "sourceHash": recipe.source_hash,
        "limits": {
            "maxCostUsd": recipe.bounds.max_cost_usd,
            "maxTotalRollouts": recipe.bounds.max_total_rollouts,
            "maxTrainRollouts": recipe.bounds.max_train_rollouts,
            "maxHeldoutRollouts": recipe.bounds.max_heldout_rollouts,
            "maxGenerations": recipe.bounds.max_generations,
        },
        "policyRef": {
            "harness": recipe.harness,
            "config": recipe.policy_config,
        },
        "credentialInputs": [credential_input],
        "expectedVisual": match recipe.algorithm {
            AlgorithmKind::Gepa => "optimizer.gepa.v1",
            AlgorithmKind::Eval => "experiment.overview.v1",
        },
    })
}

pub fn copy_into_run_dir(recipe: &WorkspaceRecipe, run_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(run_dir).context("create run-owned recipe directory")?;
    let destination = run_dir.join(RECIPE_FILE);
    fs::copy(&recipe.source_path, &destination).with_context(|| {
        format!(
            "copy {} into {}",
            recipe.source_path.display(),
            destination.display()
        )
    })?;
    Ok(destination)
}

fn parse_recipe(path: &Path) -> Result<WorkspaceRecipe> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let parsed: RecipeFile = toml::from_str(&text)
        .with_context(|| format!("parse workspace recipe {}", path.display()))?;
    if parsed.id.trim().is_empty() {
        bail!("{} is missing id", path.display());
    }
    if parsed.container.trim().is_empty() {
        bail!("recipe `{}` must name a container spec id", parsed.id);
    }
    if parsed.credential_mode != "proxy" {
        bail!(
            "recipe `{}` credential_mode must be `proxy`; got {}",
            parsed.id,
            parsed.credential_mode
        );
    }
    let bounds_src = parsed.bounds.unwrap_or_default();
    let max_cost_usd = bounds_src.max_cost_usd.unwrap_or(PRODUCT_MAX_COST_USD);
    let max_total_rollouts = bounds_src
        .max_total_rollouts
        .unwrap_or(PRODUCT_MAX_TOTAL_ROLLOUTS);
    if !(max_cost_usd.is_finite() && max_cost_usd > 0.0) {
        bail!("recipe `{}` bounds.max_cost_usd must be a positive finite number", parsed.id);
    }
    if max_cost_usd > PRODUCT_MAX_COST_USD {
        bail!(
            "recipe `{}` bounds.max_cost_usd {max_cost_usd} exceeds product cap {PRODUCT_MAX_COST_USD}",
            parsed.id
        );
    }
    if max_total_rollouts <= 0 || max_total_rollouts > PRODUCT_MAX_TOTAL_ROLLOUTS {
        bail!(
            "recipe `{}` bounds.max_total_rollouts must be 1..={PRODUCT_MAX_TOTAL_ROLLOUTS}",
            parsed.id
        );
    }
    let family = parsed
        .family
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| parsed.container.clone());
    const POLICY_KEYS: &[&str] = &[
        "api", "effort", "temperature", "top_p", "top_k", "max_calls",
        "max_steps", "context_token_budget", "compact_at", "compact_after_tokens",
        "max_compactions", "thinking_budget", "answer_max_tokens", "timeout_seconds",
        "min_request_interval", "sampler_retries", "retry_max_wait", "min_actions",
        "max_actions",
    ];
    for key in parsed.policy.keys() {
        if !POLICY_KEYS.contains(&key.as_str()) {
            bail!("recipe `{}` policy.{key} is not an admitted policy option", parsed.id);
        }
    }
    let policy = serde_json::to_value(&parsed.policy)
        .context("encode workspace policy options")?
        .as_object()
        .cloned()
        .unwrap_or_default();
    let relay = parse_relay_settings(&parsed.id, parsed.event_stream, parsed.media)?;
    Ok(WorkspaceRecipe {
        title: parsed
            .title
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| parsed.id.clone()),
        algorithm: parsed.algorithm,
        container: parsed.container,
        provider: parsed.provider,
        model: parsed.model,
        locality: parsed.locality,
        credential_mode: parsed.credential_mode,
        bounds: RecipeBounds {
            max_cost_usd,
            max_total_rollouts,
            max_train_rollouts: bounds_src.max_train_rollouts,
            max_heldout_rollouts: bounds_src.max_heldout_rollouts,
            max_generations: bounds_src.max_generations,
        },
        family,
        harness: parsed.harness.unwrap_or_else(|| "desktop_eval".into()),
        policy_config: parsed.policy_config.unwrap_or_else(|| "default".into()),
        policy,
        train_seeds: parsed.train_seeds.unwrap_or_else(|| vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
        heldout_seeds: parsed.heldout_seeds.unwrap_or_default(),
        concurrency: parsed.concurrency.unwrap_or(1).max(1),
        proposer_model: parsed.proposer_model,
        requires_credential_advertisement: parsed.requires_credential_advertisement,
        relay,
        source_path: path.to_path_buf(),
        source_hash: content_hash(&text),
        id: parsed.id,
    })
}

/// Read the declared relay/media policy, or fall back to the shipped defaults.
///
/// Every bound is validated where it is read rather than where it is used, so a
/// recipe that asks for a 4 GiB frame is refused when it is loaded instead of
/// discovered halfway through a campaign.
fn parse_relay_settings(
    recipe_id: &str,
    event_stream: Option<EventStreamFile>,
    media: Option<MediaFile>,
) -> Result<super::eval_relay::RelaySettings> {
    let mut settings = super::eval_relay::RelaySettings::default();
    if let Some(declared) = event_stream {
        if let Some(ms) = declared.poll_interval_ms {
            if ms == 0 {
                bail!("recipe `{recipe_id}` event_stream.poll_interval_ms must be positive");
            }
            settings.event_stream.poll_interval = std::time::Duration::from_millis(ms);
        }
        if let Some(limit) = declared.page_limit {
            if !(1..=10_000).contains(&limit) {
                bail!("recipe `{recipe_id}` event_stream.page_limit must be 1..=10000");
            }
            settings.event_stream.page_limit = limit;
        }
        if let Some(cap) = declared.max_events_per_rollout {
            if cap == 0 {
                bail!("recipe `{recipe_id}` event_stream.max_events_per_rollout must be positive");
            }
            settings.event_stream.max_events_per_rollout = cap;
        }
    }
    if let Some(declared) = media {
        if let Some(retention) = declared.frame_retention.as_deref() {
            settings.media.frame_retention = super::eval_relay::FrameRetention::parse(retention)
                .with_context(|| format!("recipe `{recipe_id}`"))?;
        }
        if let Some(bytes) = declared.max_frame_bytes {
            if bytes == 0 || bytes > PRODUCT_MAX_FRAME_BYTES {
                bail!(
                    "recipe `{recipe_id}` media.max_frame_bytes must be 1..={PRODUCT_MAX_FRAME_BYTES}"
                );
            }
            settings.media.max_frame_bytes = bytes;
        }
        if let Some(count) = declared.max_frames_per_rollout {
            settings.media.max_frames_per_rollout = count;
        }
        if let Some(bytes) = declared.max_total_frame_bytes {
            if bytes > PRODUCT_MAX_TOTAL_FRAME_BYTES {
                bail!(
                    "recipe `{recipe_id}` media.max_total_frame_bytes exceeds product cap {PRODUCT_MAX_TOTAL_FRAME_BYTES}"
                );
            }
            settings.media.max_total_frame_bytes = bytes;
        }
    }
    settings.event_stream = settings.event_stream.normalized();
    Ok(settings)
}

/// Ceiling on one retained frame. A single environment render that needs more
/// than 32 MiB is a producer defect, not a recipe choice.
const PRODUCT_MAX_FRAME_BYTES: u64 = 32 * 1024 * 1024;

/// Ceiling on one rollout's total retained frame bytes. Deduplication happens
/// physically in the content store, so this bounds what one episode can add.
const PRODUCT_MAX_TOTAL_FRAME_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn parse_containers(workspace: &Path, text: &str) -> Result<Vec<ContainerSpec>> {
    let parsed: ContainersFile =
        toml::from_str(text).context("parse workshop.containers.toml")?;
    let mut specs = Vec::new();
    for item in parsed.container {
        if item.command.is_empty() && item.url.as_deref().map(str::trim).unwrap_or("").is_empty() {
            bail!(
                "container `{}` must declare command or url",
                item.id
            );
        }
        let cwd_rel = item.cwd.unwrap_or_else(|| ".".into());
        let cwd = resolve_workspace_path(workspace, &cwd_rel)?;
        for provider in &item.credential_providers {
            if provider != "openrouter" {
                bail!(
                    "container `{}` requests unsupported credential provider `{}`",
                    item.id,
                    provider
                );
            }
        }
        for name in item.environment.keys() {
            let upper = name.to_ascii_uppercase();
            if !name.chars().all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
                || name.is_empty()
                || upper.contains("KEY")
                || upper.contains("SECRET")
                || upper.contains("TOKEN")
                || upper.contains("PASSWORD")
            {
                bail!("container `{}` has unsafe environment name `{name}`", item.id);
            }
        }
        specs.push(ContainerSpec {
            id: item.id,
            command: item.command,
            cwd,
            url: item.url.filter(|value| !value.trim().is_empty()),
            health: item.health,
            contract: item.contract,
            locality: item.locality,
            family: item.family,
            credential_providers: item.credential_providers,
            environment: item.environment,
        });
    }
    Ok(specs)
}

pub fn resolve_workspace_path(workspace: &Path, relative: &str) -> Result<PathBuf> {
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let candidate = if Path::new(relative).is_absolute() {
        PathBuf::from(relative)
    } else {
        workspace.join(relative)
    };
    let canonical = candidate
        .canonicalize()
        .with_context(|| format!("path `{}` does not exist", candidate.display()))?;
    if !canonical.starts_with(&workspace) {
        bail!(
            "path {} escapes the workspace {}",
            canonical.display(),
            workspace.display()
        );
    }
    Ok(canonical)
}

fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

/// Bind policy URLs from locality. Container locality cannot yield loopback.
pub fn bind_locality_urls(
    config: &mut toml::value::Table,
    locality: PolicyLocality,
    host_base_url: Option<&str>,
    container_base_url: Option<&str>,
    container_inference_url: Option<&str>,
) -> Result<()> {
    let (base_url, inference_url) = match locality {
        PolicyLocality::Host => {
            let base = host_base_url.ok_or_else(|| {
                anyhow!("locality=host requires the host provider proxy URL")
            })?;
            (base.to_string(), None)
        }
        PolicyLocality::Container => {
            let base = container_base_url.ok_or_else(|| {
                anyhow!("locality=container requires container_openai_base_url; refusing host loopback")
            })?;
            refuse_loopback(base)?;
            let inference = container_inference_url.ok_or_else(|| {
                anyhow!("locality=container requires container_openai_route")
            })?;
            refuse_loopback(inference)?;
            (base.to_string(), Some(inference.to_string()))
        }
    };
    let policy = config
        .entry("policy".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("recipe [policy] must be a table"))?;
    policy.insert("base_url".into(), toml::Value::String(base_url));
    if let Some(inference_url) = inference_url {
        policy.insert(
            "inference_url".into(),
            toml::Value::String(inference_url),
        );
    }
    policy.insert(
        "credential_mode".into(),
        toml::Value::String("proxy".into()),
    );
    let canonical_provider = policy
        .get("provider")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| anyhow!("recipe [policy].provider is required"))?
        .to_string();
    let _ = policy;

    // A local proposer is another consumer of the same recipe-owned
    // provider capability. Bind it from the canonical policy/provider route
    // instead of allowing an independently defaulted `openai` lane to bypass
    // Workshop and send the proxy sentinel to a public origin.
    if let Some(proposer) = config
        .get_mut("proposer")
        .and_then(toml::Value::as_table_mut)
        .filter(|table| {
            matches!(
                table.get("backend").and_then(toml::Value::as_str),
                Some("codex_app_server" | "chat_completions" | "deepseek_chat")
            )
        })
    {
        let provider = canonical_provider;
        let proposer_base = host_base_url.ok_or_else(|| {
            anyhow!("proposer requires the host provider proxy URL")
        })?;
        proposer.insert("provider".into(), toml::Value::String(provider.clone()));
        proposer.insert(
            "base_url".into(),
            toml::Value::String(proposer_base.to_string()),
        );
        proposer.insert(
            "api_key_env".into(),
            toml::Value::String(match provider.as_str() {
                "openrouter" => "OPENROUTER_API_KEY",
                "anthropic" => "ANTHROPIC_API_KEY",
                _ => "OPENAI_API_KEY",
            }.into()),
        );
    }
    Ok(())
}

pub fn refuse_loopback(url: &str) -> Result<()> {
    let lowered = url.to_ascii_lowercase();
    if lowered.contains("127.0.0.1")
        || lowered.contains("localhost")
        || lowered.contains("[::1]")
    {
        bail!("locality=container cannot bind a loopback URL: {url}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_workspace() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        (dir, workspace)
    }

    #[test]
    fn parses_eval_recipe_and_clamps_nothing_under_cap() {
        let (_dir, workspace) = write_workspace();
        fs::write(
            workspace.join(RECIPE_FILE),
            r#"
id = "eval.classify.baseline.v1"
algorithm = "eval"
container = "classify"
provider = "openai"
model = "gpt-4.1-nano"
locality = "container"
family = "classify"
harness = "desktop_eval"
policy_config = "classify_default"
train_seeds = [0, 1]
[bounds]
max_cost_usd = 0.50
max_total_rollouts = 10
"#,
        )
        .unwrap();
        let recipe = find_recipe(&workspace, "eval.classify.baseline.v1").unwrap();
        assert_eq!(recipe.algorithm, AlgorithmKind::Eval);
        assert_eq!(recipe.locality, PolicyLocality::Container);
        assert_eq!(recipe.bounds.max_cost_usd, 0.50);
        assert_eq!(recipe.bounds.max_total_rollouts, 10);
        assert_eq!(recipe.train_seeds, vec![0, 1]);
    }

    #[test]
    fn catalog_uses_the_declared_provider_credential() {
        let (_dir, workspace) = write_workspace();
        fs::write(
            workspace.join(RECIPE_FILE),
            r#"
id = "gepa.openrouter.smoke.v1"
algorithm = "gepa"
container = "fixture"
provider = "openrouter"
model = "openai/gpt-5.6-luna"
locality = "container"
[bounds]
max_cost_usd = 0.10
max_total_rollouts = 1
"#,
        )
        .unwrap();
        let recipe = find_recipe(&workspace, "gepa.openrouter.smoke.v1").unwrap();
        assert_eq!(
            catalog_entry(&recipe)["credentialInputs"],
            json!(["OPENROUTER_API_KEY"])
        );
    }

    #[test]
    fn rejects_bounds_above_product_cap() {
        let (_dir, workspace) = write_workspace();
        fs::write(
            workspace.join(RECIPE_FILE),
            r#"
id = "eval.too-rich.v1"
algorithm = "eval"
container = "x"
model = "gpt-4.1-nano"
locality = "host"
[bounds]
max_cost_usd = 99.0
max_total_rollouts = 10
"#,
        )
        .unwrap();
        let error = find_recipe(&workspace, "eval.too-rich.v1").unwrap_err().to_string();
        assert!(error.contains("exceeds product cap"), "{error}");
    }

    #[test]
    fn container_command_must_stay_inside_workspace() {
        let (_dir, workspace) = write_workspace();
        fs::create_dir_all(workspace.join("svc")).unwrap();
        fs::write(workspace.join("svc/serve.py"), "print('ok')").unwrap();
        fs::write(
            workspace.join(CONTAINERS_FILE),
            r#"
[[container]]
id = "classify"
command = ["python3", "svc/serve.py"]
cwd = "svc"
health = "/health"
contract = "synth-containers/v1"
locality = "container"
credential_providers = ["openrouter"]
environment = { SYNTH_CRAFTAX_URL = "http://127.0.0.1:8098" }
"#,
        )
        .unwrap();
        let spec = find_container_spec(&workspace, "classify").unwrap();
        assert!(spec.cwd.starts_with(&workspace.canonicalize().unwrap()));
        assert!(spec.cwd.ends_with("svc"));
        assert_eq!(spec.credential_providers, vec!["openrouter"]);
        assert_eq!(spec.environment["SYNTH_CRAFTAX_URL"], "http://127.0.0.1:8098");
    }

    #[test]
    fn container_locality_refuses_loopback_bind() {
        refuse_loopback("http://127.0.0.1:9/providers/openai").unwrap_err();
        refuse_loopback("http://host.docker.internal:9/providers/openai").unwrap();
        let mut table = toml::map::Map::new();
        table.insert(
            "policy".into(),
            toml::Value::Table(toml::map::Map::from_iter([(
                "provider".into(),
                toml::Value::String("openai".into()),
            )])),
        );
        bind_locality_urls(
            &mut table,
            PolicyLocality::Container,
            Some("http://127.0.0.1:9/providers/openai"),
            Some("http://host.docker.internal:9/providers/openai"),
            Some("http://host.docker.internal:9/providers/openai/chat/completions"),
        )
        .unwrap();
        assert_eq!(
            table["policy"]["base_url"].as_str().unwrap(),
            "http://host.docker.internal:9/providers/openai"
        );
        bind_locality_urls(
            &mut table,
            PolicyLocality::Container,
            Some("http://127.0.0.1:9/providers/openai"),
            Some("http://127.0.0.1:9/providers/openai"),
            Some("http://127.0.0.1:9/providers/openai/chat/completions"),
        )
        .unwrap_err();
    }

    #[test]
    fn empty_workspace_has_no_recipes() {
        let (_dir, workspace) = write_workspace();
        assert!(load_recipes(&workspace).unwrap().is_empty());
    }
}
