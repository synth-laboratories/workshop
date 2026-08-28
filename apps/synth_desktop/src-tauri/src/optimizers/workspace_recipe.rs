//! Desktop-source-declared optimizer recipes and container specs.
//!
//! Workshop does not infer task identity from a chat. Recipes and containers
//! are discovered from bounded desktop source roots, then copied into a
//! run-owned directory. Session references may own the resulting run and its
//! visuals, but they never select project code or configuration.

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use rusqlite::params;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
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
    pub policy_source: Option<String>,
    pub policy_max_calls: Option<u32>,
    pub train_seeds: Vec<i64>,
    pub heldout_seeds: Vec<i64>,
    pub concurrency: usize,
    pub proposer_model: Option<String>,
    pub requires_credential_advertisement: bool,
    pub source_path: PathBuf,
    pub source_hash: String,
    pub source_root: PathBuf,
}

#[derive(Clone, Debug)]
struct RecipeSource {
    root: PathBuf,
    source_hash: String,
    recipes: Vec<WorkspaceRecipe>,
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
    policy_source: Option<String>,
    #[serde(default)]
    policy: Option<PolicyFile>,
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
}

#[derive(Deserialize, Default)]
struct PolicyFile {
    max_calls: Option<u32>,
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

/// List recipe declarations available to this desktop. Sources come from
/// configured roots and persisted desktop provenance, never from the active
/// chat or its isolated workspace.
pub fn catalog(db: &crate::storage::Database) -> Result<Vec<WorkspaceRecipe>> {
    flatten_sources(discover_sources_in_roots(&discovery_roots(db)?)?)
}

/// Resolve one recipe from the desktop catalog and retain its refreshed source
/// provenance before a run copies the declaration into app-owned state.
pub async fn resolve(
    db: &Arc<crate::storage::Database>,
    recipe_id: &str,
) -> Result<WorkspaceRecipe> {
    let recipe_id = recipe_id.trim();
    if recipe_id.is_empty() || recipe_id.len() > 256 {
        bail!("recipe_id is required");
    }
    let sources = discover_sources_in_roots(&discovery_roots(db)?)?;
    persist_sources(db, &sources).await?;
    flatten_sources(sources)?
        .into_iter()
        .find(|recipe| recipe.id == recipe_id)
        .ok_or_else(|| anyhow!("catalog recipe `{recipe_id}` is not declared"))
}

/// Rescan every effective recipe root and re-record its provenance.
///
/// This is what an admission or removal calls so the recipe catalog reflects
/// the change without waiting for the next run to resolve a recipe.
pub async fn refresh_sources(db: &Arc<crate::storage::Database>) -> Result<Vec<WorkspaceRecipe>> {
    let sources = discover_sources_in_roots(&discovery_roots(db)?)?;
    persist_sources(db, &sources).await?;
    flatten_sources(sources)
}

/// Unit-test declarations are stored against the test database, so tests use
/// the same catalog flow without mutating process-wide source-root settings.
#[cfg(test)]
pub(crate) async fn remember_source_for_test(
    db: &Arc<crate::storage::Database>,
    root: &Path,
) -> Result<()> {
    let sources = discover_sources_in_roots(&[root.to_path_buf()])?;
    if sources.is_empty() {
        bail!("test recipe source {} declares no recipes", root.display());
    }
    persist_sources(db, &sources).await
}

/// Recipe roots come from the same shared authority the container catalog
/// uses, so one approved repository is discoverable by both or by neither.
fn discovery_roots(db: &crate::storage::Database) -> Result<Vec<PathBuf>> {
    crate::project_sources::discovery_roots(db, crate::project_sources::Capability::Recipes)
}

fn discover_sources_in_roots(roots: &[PathBuf]) -> Result<Vec<RecipeSource>> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        let Ok(root) = root.canonicalize() else {
            continue;
        };
        if !root.is_dir() {
            continue;
        }
        candidates.push(root.clone());
        let entries = fs::read_dir(&root)
            .with_context(|| format!("read recipe source root {}", root.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("read entry below {}", root.display()))?;
            if entry.file_type()?.is_dir() {
                candidates.push(entry.path());
            }
        }
    }

    let mut sources = Vec::new();
    for candidate in candidates {
        let Ok(root) = candidate.canonicalize() else {
            continue;
        };
        if !seen.insert(root.clone()) {
            continue;
        }
        // One malformed declaration in a broad desktop root must not hide all
        // other valid sources. Fixing it makes it discoverable next refresh.
        let Ok(recipes) = load_recipes(&root) else {
            continue;
        };
        if recipes.is_empty() {
            continue;
        }
        sources.push(RecipeSource {
            source_hash: source_hash(&recipes),
            root,
            recipes,
        });
    }
    sources.sort_by(|left, right| left.root.cmp(&right.root));
    Ok(sources)
}

fn flatten_sources(sources: Vec<RecipeSource>) -> Result<Vec<WorkspaceRecipe>> {
    let mut by_id = BTreeMap::new();
    for source in sources {
        for recipe in source.recipes {
            if let Some(previous) = by_id.insert(recipe.id.clone(), recipe.clone()) {
                bail!(
                    "catalog recipe `{}` is declared by both {} and {}",
                    recipe.id,
                    previous.source_path.display(),
                    recipe.source_path.display()
                );
            }
        }
    }
    Ok(by_id.into_values().collect())
}

async fn persist_sources(
    db: &Arc<crate::storage::Database>,
    sources: &[RecipeSource],
) -> Result<()> {
    let rows: Vec<_> = sources
        .iter()
        .map(|source| {
            (
                source.root.to_string_lossy().to_string(),
                source.source_hash.clone(),
            )
        })
        .collect();
    db.clone()
        .run_transaction(move |conn| {
            let now = Utc::now().to_rfc3339();
            for (root, source_hash) in rows {
                conn.execute(
                    "INSERT INTO optimizer_recipe_sources(canonical_path,source_hash,discovered_at,updated_at)
                     VALUES(?1,?2,?3,?3)
                     ON CONFLICT(canonical_path) DO UPDATE SET
                        source_hash=excluded.source_hash,
                        discovered_at=excluded.discovered_at,
                        updated_at=excluded.updated_at",
                    params![root, source_hash, now],
                )?;
            }
            Ok(())
        })
        .await
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
    let mut seen = HashSet::new();
    for recipe in &recipes {
        if !seen.insert(recipe.id.as_str()) {
            bail!("source declares recipe id `{}` more than once", recipe.id);
        }
    }
    for recipe in &mut recipes {
        recipe.source_root = workspace.to_path_buf();
    }
    Ok(recipes)
}

pub fn find_recipe(workspace: &Path, recipe_id: &str) -> Result<WorkspaceRecipe> {
    load_recipes(workspace)?
        .into_iter()
        .find(|recipe| recipe.id == recipe_id)
        .ok_or_else(|| {
            anyhow!(
                "catalog recipe `{recipe_id}` is not declared in {} or {}/",
                RECIPE_FILE,
                RECIPES_DIR
            )
        })
}

pub fn load_container_specs(source_root: &Path) -> Result<Vec<ContainerSpec>> {
    let path = source_root.join(CONTAINERS_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    parse_containers(source_root, &text)
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
        "source": "catalog",
        "sourceRoot": recipe.source_root,
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
        bail!(
            "recipe `{}` bounds.max_cost_usd must be a positive finite number",
            parsed.id
        );
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
    let policy_max_calls = parsed.policy.as_ref().and_then(|policy| policy.max_calls);
    if policy_max_calls == Some(0) {
        bail!("recipe `{}` policy.max_calls must be greater than zero", parsed.id);
    }
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
        policy_source: parsed.policy_source,
        policy_max_calls,
        train_seeds: parsed
            .train_seeds
            .unwrap_or_else(|| vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
        heldout_seeds: parsed.heldout_seeds.unwrap_or_default(),
        concurrency: parsed.concurrency.unwrap_or(1).max(1),
        proposer_model: parsed.proposer_model,
        requires_credential_advertisement: parsed.requires_credential_advertisement,
        source_path: path.to_path_buf(),
        source_hash: content_hash(&text),
        source_root: path.parent().unwrap_or_else(|| Path::new("")).to_path_buf(),
        id: parsed.id,
    })
}

fn parse_containers(source_root: &Path, text: &str) -> Result<Vec<ContainerSpec>> {
    let parsed: ContainersFile = toml::from_str(text).context("parse workshop.containers.toml")?;
    let mut specs = Vec::new();
    for item in parsed.container {
        if item.command.is_empty() && item.url.as_deref().map(str::trim).unwrap_or("").is_empty() {
            bail!("container `{}` must declare command or url", item.id);
        }
        let cwd_rel = item.cwd.unwrap_or_else(|| ".".into());
        let cwd = resolve_source_path(source_root, &cwd_rel)?;
        specs.push(ContainerSpec {
            id: item.id,
            command: item.command,
            cwd,
            url: item.url.filter(|value| !value.trim().is_empty()),
            health: item.health,
            contract: item.contract,
            locality: item.locality,
            family: item.family,
        });
    }
    Ok(specs)
}

pub fn resolve_source_path(source_root: &Path, relative: &str) -> Result<PathBuf> {
    let source_root = source_root
        .canonicalize()
        .unwrap_or_else(|_| source_root.to_path_buf());
    let candidate = if Path::new(relative).is_absolute() {
        PathBuf::from(relative)
    } else {
        source_root.join(relative)
    };
    let canonical = candidate
        .canonicalize()
        .with_context(|| format!("path `{}` does not exist", candidate.display()))?;
    if !canonical.starts_with(&source_root) {
        bail!(
            "path {} escapes the source root {}",
            canonical.display(),
            source_root.display()
        );
    }
    Ok(canonical)
}

fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn source_hash(recipes: &[WorkspaceRecipe]) -> String {
    let mut entries: Vec<_> = recipes
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe.source_hash.as_str()))
        .collect();
    entries.sort_unstable();
    let mut hasher = Sha256::new();
    for (id, hash) in entries {
        hasher.update(id.as_bytes());
        hasher.update([0]);
        hasher.update(hash.as_bytes());
        hasher.update([0]);
    }
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
            let base = host_base_url
                .ok_or_else(|| anyhow!("locality=host requires the host provider proxy URL"))?;
            (base.to_string(), None)
        }
        PolicyLocality::Container => {
            let base = container_base_url.ok_or_else(|| {
                anyhow!(
                    "locality=container requires container_openai_base_url; refusing host loopback"
                )
            })?;
            refuse_loopback(base)?;
            let inference = container_inference_url
                .ok_or_else(|| anyhow!("locality=container requires container_openai_route"))?;
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
        policy.insert("inference_url".into(), toml::Value::String(inference_url));
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
        let proposer_base = host_base_url
            .ok_or_else(|| anyhow!("proposer requires the host provider proxy URL"))?;
        proposer.insert("provider".into(), toml::Value::String(provider.clone()));
        proposer.insert(
            "base_url".into(),
            toml::Value::String(proposer_base.to_string()),
        );
        proposer.insert(
            "api_key_env".into(),
            toml::Value::String(
                match provider.as_str() {
                    "openrouter" => "OPENROUTER_API_KEY",
                    "anthropic" => "ANTHROPIC_API_KEY",
                    _ => "OPENAI_API_KEY",
                }
                .into(),
            ),
        );
    }
    Ok(())
}

pub fn refuse_loopback(url: &str) -> Result<()> {
    let lowered = url.to_ascii_lowercase();
    if lowered.contains("127.0.0.1") || lowered.contains("localhost") || lowered.contains("[::1]") {
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
[policy]
max_calls = 10
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
        assert_eq!(recipe.policy_max_calls, Some(10));
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
        let error = find_recipe(&workspace, "eval.too-rich.v1")
            .unwrap_err()
            .to_string();
        assert!(error.contains("exceeds product cap"), "{error}");
    }

    #[test]
    fn container_command_must_stay_inside_source_root() {
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
"#,
        )
        .unwrap();
        let spec = load_container_specs(&workspace)
            .unwrap()
            .into_iter()
            .find(|spec| spec.id == "classify")
            .unwrap();
        assert!(spec.cwd.starts_with(&workspace.canonicalize().unwrap()));
        assert!(spec.cwd.ends_with("svc"));
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

    #[test]
    fn desktop_source_catalog_discovers_recipes_without_a_session_workspace() {
        let (_dir, sources_root) = write_workspace();
        let source = sources_root.join("craftax-source");
        fs::create_dir_all(source.join(RECIPES_DIR)).unwrap();
        fs::write(
            source.join(RECIPES_DIR).join("craftax.toml"),
            r#"
id = "eval.craftax.catalog-smoke.v1"
algorithm = "eval"
container = "craftax_react"
model = "gpt-4.1-nano"
locality = "host"
[bounds]
max_cost_usd = 0.10
max_total_rollouts = 1
"#,
        )
        .unwrap();

        let recipes = flatten_sources(discover_sources_in_roots(&[sources_root]).unwrap()).unwrap();
        assert_eq!(recipes.len(), 1);
        assert_eq!(recipes[0].id, "eval.craftax.catalog-smoke.v1");
        assert_eq!(recipes[0].source_root, source.canonicalize().unwrap());
        assert_eq!(catalog_entry(&recipes[0])["source"], json!("catalog"));
    }
}
