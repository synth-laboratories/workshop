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
    path::{Component, Path, PathBuf},
};

use crate::error::StructuredFailure;

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
    pub policy_source: Option<String>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerDeclarationOrigin {
    pub manifest_path: PathBuf,
    pub source_root: PathBuf,
    pub declaration_id: String,
    pub source_revision: Option<String>,
    pub source_digest: Option<String>,
}

impl ContainerDeclarationOrigin {
    pub fn to_json(&self) -> Value {
        json!({
            "manifestPath": self.manifest_path.display().to_string(),
            "sourceRoot": self.source_root.display().to_string(),
            "declarationId": self.declaration_id,
            "sourceRevision": self.source_revision,
            "sourceDigest": self.source_digest,
        })
    }

    pub fn from_json(value: &Value) -> Option<Self> {
        let manifest_path = value
            .get("manifestPath")
            .or_else(|| value.get("manifest_path"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let source_root = value
            .get("sourceRoot")
            .or_else(|| value.get("source_root"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let declaration_id = value
            .get("declarationId")
            .or_else(|| value.get("declaration_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?
            .to_string();
        Some(Self {
            manifest_path: PathBuf::from(manifest_path),
            source_root: PathBuf::from(source_root),
            declaration_id,
            source_revision: value
                .get("sourceRevision")
                .or_else(|| value.get("source_revision"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            source_digest: value
                .get("sourceDigest")
                .or_else(|| value.get("source_digest"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
        })
    }
}

/// A relative path that has been bound to a declaring repository exactly once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRepositoryPath {
    pub source_root: PathBuf,
    pub absolute_path: PathBuf,
}

#[derive(Clone, Debug)]
pub enum LaunchDeclarationError {
    ManifestUnreadable {
        manifest_path: PathBuf,
        cause: String,
    },
    UnsupportedSchema {
        found: String,
    },
    SourcePathMissing {
        manifest_path: PathBuf,
        source_root: PathBuf,
        declared_path: String,
        resolved_path: PathBuf,
    },
    SourcePathEscapesRoot {
        manifest_path: PathBuf,
        source_root: PathBuf,
        declared_path: String,
        resolved_path: PathBuf,
    },
    AbsoluteManifestPath {
        declared_path: String,
    },
    SourceDigestMismatch {
        declared: String,
        actual: String,
    },
    CheckoutRevisionMismatch {
        declared: String,
        actual: String,
    },
    InvalidEnvironmentName {
        name: String,
    },
    SourceRootNotApproved {
        source_root: PathBuf,
        manifest_path: PathBuf,
    },
}

impl LaunchDeclarationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ManifestUnreadable { .. } => "launch_manifest_unreadable",
            Self::UnsupportedSchema { .. } => "launch_unsupported_schema",
            Self::SourcePathMissing { .. } => "launch_source_path_not_found",
            Self::SourcePathEscapesRoot { .. } => "launch_source_path_escapes_root",
            Self::AbsoluteManifestPath { .. } => "launch_absolute_path_rejected",
            Self::SourceDigestMismatch { .. } => "launch_source_digest_mismatch",
            Self::CheckoutRevisionMismatch { .. } => "launch_checkout_revision_mismatch",
            Self::InvalidEnvironmentName { .. } => "launch_invalid_environment_name",
            Self::SourceRootNotApproved { .. } => "launch_source_root_not_approved",
        }
    }

    pub fn into_structured(self) -> StructuredFailure {
        let code = self.code();
        let details = match &self {
            Self::ManifestUnreadable {
                manifest_path,
                cause,
            } => json!({
                "manifest": manifest_path.display().to_string(),
                "cause": cause,
            }),
            Self::UnsupportedSchema { found } => json!({ "found": found }),
            Self::SourcePathMissing {
                manifest_path,
                source_root,
                declared_path,
                resolved_path,
            }
            | Self::SourcePathEscapesRoot {
                manifest_path,
                source_root,
                declared_path,
                resolved_path,
            } => json!({
                "manifest": manifest_path.display().to_string(),
                "source_root": source_root.display().to_string(),
                "declared_path": declared_path,
                "resolved_path": resolved_path.display().to_string(),
            }),
            Self::AbsoluteManifestPath { declared_path } => json!({
                "declared_path": declared_path,
            }),
            Self::SourceDigestMismatch { declared, actual } => json!({
                "declared": declared,
                "actual": actual,
            }),
            Self::CheckoutRevisionMismatch { declared, actual } => json!({
                "declared": declared,
                "actual": actual,
            }),
            Self::InvalidEnvironmentName { name } => json!({ "name": name }),
            Self::SourceRootNotApproved {
                source_root,
                manifest_path,
            } => json!({
                "source_root": source_root.display().to_string(),
                "manifest": manifest_path.display().to_string(),
            }),
        };
        let message = self.to_string();
        let remediation = match code {
            "launch_source_path_not_found" => {
                "Workshop looks for launch files in the repository that declared this container, not the chat folder."
            }
            "launch_source_path_escapes_root" => {
                "Keep launch inputs inside that repository, then retry."
            }
            "launch_source_root_not_approved" => {
                "Attach the repository that contains workshop.containers.toml to this conversation, then try again."
            }
            "launch_source_digest_mismatch" => {
                "Update the declared digest, or restore the launch files, then retry."
            }
            "launch_checkout_revision_mismatch" => {
                "Check out the declared revision, or update the declaration to this checkout."
            }
            _ => "Fix the launch files, then retry the same operation.",
        };
        StructuredFailure::new(code, message, remediation).with_details(details)
    }

    /// Wrap as `anyhow::Error` via `StructuredFailure`. Do not impl
    /// `From<LaunchDeclarationError>` — that overlaps anyhow's blanket
    /// `From<E: StdError>`.
    pub fn into_anyhow(self) -> anyhow::Error {
        self.into_structured().into()
    }
}

impl std::fmt::Display for LaunchDeclarationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ManifestUnreadable {
                manifest_path,
                cause,
            } => write!(
                formatter,
                "Couldn't read {}: {cause}",
                manifest_path.display()
            ),
            Self::UnsupportedSchema { found } => {
                write!(formatter, "Unsupported launch schema `{found}`.")
            }
            Self::SourcePathMissing {
                declared_path,
                resolved_path,
                ..
            } => write!(
                formatter,
                "Couldn't find `{declared_path}` at {}.",
                resolved_path.display()
            ),
            Self::SourcePathEscapesRoot {
                declared_path,
                source_root,
                ..
            } => write!(
                formatter,
                "`{declared_path}` is outside {}.",
                source_root.display()
            ),
            Self::AbsoluteManifestPath { declared_path } => {
                write!(
                    formatter,
                    "Absolute launch path `{declared_path}` isn't allowed."
                )
            }
            Self::SourceDigestMismatch { declared, actual } => {
                write!(
                    formatter,
                    "Declared launch digest is {declared}; current files are {actual}."
                )
            }
            Self::CheckoutRevisionMismatch { declared, actual } => {
                write!(
                    formatter,
                    "Declaration tracks {declared}; this checkout is {actual}."
                )
            }
            Self::InvalidEnvironmentName { name } => {
                write!(formatter, "Environment name `{name}` isn't allowed.")
            }
            Self::SourceRootNotApproved { source_root, .. } => write!(
                formatter,
                "{} isn't attached to this conversation.",
                source_root.display()
            ),
        }
    }
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
    pub policy_source: Option<String>,
    pub source_revision: Option<String>,
    pub manifest_digest: Option<String>,
    pub origin: ContainerDeclarationOrigin,
    pub launch: ContainerLaunchDeclarationV1,
}

#[derive(Clone, Debug)]
pub struct ContainerLaunchDeclarationV1 {
    pub working_directory: PathBuf,
    pub command: Vec<String>,
    pub readiness_timeout_seconds: u64,
    pub shutdown_grace_seconds: u64,
    pub expected_port: u16,
    pub image_ref: String,
    pub health_target: String,
    pub declared_environment: Vec<String>,
    pub environment: std::collections::BTreeMap<String, String>,
    pub tracked_revision: String,
    pub dirty_digest: Option<String>,
    pub include: Vec<String>,
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
    policy_source: Option<String>,
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
    policy_source: Option<String>,
    #[serde(default)]
    launch: Option<ContainerLaunchFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContainerLaunchFile {
    schema_version: String,
    working_directory: String,
    command: Vec<String>,
    readiness_timeout_seconds: u64,
    shutdown_grace_seconds: u64,
    expected_port: u16,
    image_ref: String,
    health_target: String,
    #[serde(default)]
    declared_environment: Vec<String>,
    #[serde(default)]
    environment: std::collections::BTreeMap<String, String>,
    source: ContainerLaunchSourceFile,
}

#[derive(Deserialize)]
struct ContainerLaunchSourceFile {
    revision_policy: String,
    tracked_revision: String,
    #[serde(default)]
    dirty_digest: Option<String>,
    include: Vec<String>,
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
            bail!(
                "workspace declares recipe id `{}` more than once",
                recipe.id
            );
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
    load_container_specs_from_root(workspace)
}

pub fn load_container_specs_from_root(source_root: &Path) -> Result<Vec<ContainerSpec>> {
    let path = source_root.join(CONTAINERS_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    load_container_specs_from_manifest(&path)
}

pub fn load_container_specs_from_manifest(manifest_path: &Path) -> Result<Vec<ContainerSpec>> {
    let source_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("container manifest {} has no parent", manifest_path.display()))?;
    let text = fs::read_to_string(manifest_path).map_err(|cause| {
        LaunchDeclarationError::ManifestUnreadable {
            manifest_path: manifest_path.to_path_buf(),
            cause: cause.to_string(),
        }
        .into_anyhow()
    })?;
    parse_containers(source_root, manifest_path, &text)
}

pub fn find_container_spec(workspace: &Path, spec_id: &str) -> Result<ContainerSpec> {
    load_container_specs_from_root(workspace)?
        .into_iter()
        .find(|spec| spec.id == spec_id)
        .ok_or_else(|| anyhow!("container spec `{spec_id}` is not declared in {CONTAINERS_FILE}"))
}

const MANIFEST_WALK_SKIP: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    "__pycache__",
];

/// Discover `workshop.containers.toml` files under approved roots.
///
/// Each manifest keeps the directory that contains it as `source_root`. A
/// parent-approved folder may contain nested declaring repositories; those
/// nested manifests are not reinterpreted against the parent.
pub fn discover_container_manifests(search_roots: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut manifests = Vec::new();
    for root in search_roots {
        let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        collect_container_manifests(&canonical, 2, &mut manifests);
    }
    manifests.sort();
    manifests.dedup();
    Ok(manifests)
}

fn collect_container_manifests(root: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let candidate = root.join(CONTAINERS_FILE);
    if candidate.is_file() {
        out.push(
            candidate
                .canonicalize()
                .unwrap_or_else(|_| candidate.clone()),
        );
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || MANIFEST_WALK_SKIP.contains(&name.as_ref()) {
            continue;
        }
        collect_container_manifests(&path, depth.saturating_sub(1), out);
    }
}

pub fn origin_is_under_approved_roots(
    origin: &ContainerDeclarationOrigin,
    search_roots: &[PathBuf],
) -> bool {
    search_roots.iter().any(|root| {
        let root = root.canonicalize().unwrap_or_else(|_| root.clone());
        paths_related(&origin.source_root, &root) || origin.manifest_path.starts_with(&root)
    })
}

fn paths_related(path: &Path, root: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    path == *root || path.starts_with(root)
}

/// Recover declaration provenance from registry metadata.
///
/// Current records store `declarationOrigin`. Older records stored only
/// `sourcePath` as the launch working directory; walk up from that path to
/// the declaring manifest rather than falling back to the chat workspace.
pub fn origin_from_metadata(metadata: &Value, spec_id: &str) -> Option<ContainerDeclarationOrigin> {
    if let Some(origin) = metadata
        .get("declarationOrigin")
        .and_then(ContainerDeclarationOrigin::from_json)
    {
        return Some(origin);
    }
    let source_path = metadata
        .get("sourcePath")
        .or_else(|| metadata.get("source_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    recover_origin_from_legacy_source_path(Path::new(source_path), spec_id)
}

pub fn recover_origin_from_legacy_source_path(
    source_path: &Path,
    declaration_id: &str,
) -> Option<ContainerDeclarationOrigin> {
    let mut dir = if source_path.is_file() {
        source_path.parent()?.to_path_buf()
    } else {
        source_path.to_path_buf()
    };
    for _ in 0..6 {
        let manifest = dir.join(CONTAINERS_FILE);
        if manifest.is_file() {
            let source_root = dir.canonicalize().unwrap_or_else(|_| dir.clone());
            let manifest_path = manifest.canonicalize().unwrap_or_else(|_| manifest.clone());
            return Some(ContainerDeclarationOrigin {
                manifest_path,
                source_root,
                declaration_id: declaration_id.to_string(),
                source_revision: None,
                source_digest: None,
            });
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

pub fn find_container_spec_in_roots(
    search_roots: &[PathBuf],
    spec_id: &str,
) -> Result<ContainerSpec> {
    let mut matches = Vec::new();
    for manifest in discover_container_manifests(search_roots)? {
        for spec in load_container_specs_from_manifest(&manifest)? {
            if spec.id == spec_id {
                matches.push(spec);
            }
        }
    }
    match matches.len() {
        0 => Err(anyhow!(
            "container spec `{spec_id}` is not declared in any approved workshop.containers.toml"
        )),
        1 => Ok(matches.remove(0)),
        _ => {
            if let Some(exact) = matches.iter().find(|spec| {
                search_roots.iter().any(|root| {
                    spec.origin.source_root == *root
                        || spec.origin.source_root
                            == root.canonicalize().unwrap_or_else(|_| root.clone())
                })
            }) {
                return Ok(exact.clone());
            }
            Err(anyhow!(
                "container spec `{spec_id}` is declared in more than one approved repository"
            ))
        }
    }
}

pub fn resolve_container_spec(
    search_roots: &[PathBuf],
    spec_id: &str,
    stored_origin: Option<&ContainerDeclarationOrigin>,
) -> Result<ContainerSpec> {
    if let Some(origin) = stored_origin {
        if !origin_is_under_approved_roots(origin, search_roots) {
            return Err(LaunchDeclarationError::SourceRootNotApproved {
                source_root: origin.source_root.clone(),
                manifest_path: origin.manifest_path.clone(),
            }
            .into_anyhow());
        }
        return find_container_spec(&origin.source_root, spec_id);
    }
    find_container_spec_in_roots(search_roots, spec_id)
}

pub fn session_search_roots(
    db: &crate::storage::Database,
    session_id: &str,
) -> Result<Vec<PathBuf>> {
    crate::workspace_scope::approved_search_roots(db, session_id)
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
    const POLICY_KEYS: &[&str] = &[
        "api",
        "effort",
        "temperature",
        "top_p",
        "top_k",
        "max_calls",
        "max_steps",
        "context_token_budget",
        "compact_at",
        "compact_after_tokens",
        "max_compactions",
        "thinking_budget",
        "answer_max_tokens",
        "timeout_seconds",
        "min_request_interval",
        "sampler_retries",
        "retry_max_wait",
        "min_actions",
        "max_actions",
    ];
    for key in parsed.policy.keys() {
        if !POLICY_KEYS.contains(&key.as_str()) {
            bail!(
                "recipe `{}` policy.{key} is not an admitted policy option",
                parsed.id
            );
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
        policy_source: parsed
            .policy_source
            .filter(|value| !value.trim().is_empty()),
        train_seeds: parsed
            .train_seeds
            .unwrap_or_else(|| vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]),
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

fn parse_containers(
    source_root: &Path,
    manifest_path: &Path,
    text: &str,
) -> Result<Vec<ContainerSpec>> {
    let parsed: ContainersFile = toml::from_str(text).context("parse workshop.containers.toml")?;
    let mut specs = Vec::new();
    let source_root = source_root
        .canonicalize()
        .unwrap_or_else(|_| source_root.to_path_buf());
    let manifest_path = manifest_path
        .canonicalize()
        .unwrap_or_else(|_| manifest_path.to_path_buf());
    for item in parsed.container {
        let launch_file = item.launch.context(format!(
            "launch_declaration_missing: container `{}` must declare [container.launch]",
            item.id
        ))?;
        let origin = ContainerDeclarationOrigin {
            manifest_path: manifest_path.clone(),
            source_root: source_root.clone(),
            declaration_id: item.id.clone(),
            source_revision: None,
            source_digest: None,
        };
        let launch = validate_launch(&origin, item.url.as_deref(), launch_file)?;
        let command = launch.command.clone();
        let cwd = launch.working_directory.clone();
        for provider in &item.credential_providers {
            if provider != "openrouter" {
                bail!(
                    "container `{}` requests unsupported credential provider `{}`",
                    item.id,
                    provider
                );
            }
        }
        for name in launch.environment.keys() {
            let upper = name.to_ascii_uppercase();
            if !name
                .chars()
                .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
                || name.is_empty()
                || upper.contains("KEY")
                || upper.contains("SECRET")
                || upper.contains("TOKEN")
                || upper.contains("PASSWORD")
            {
                bail!(
                    "container `{}` has unsafe environment name `{name}`",
                    item.id
                );
            }
        }
        let origin = ContainerDeclarationOrigin {
            source_revision: Some(launch.tracked_revision.clone()),
            source_digest: launch.dirty_digest.clone(),
            ..origin
        };
        specs.push(ContainerSpec {
            id: item.id,
            command,
            cwd,
            url: item.url.filter(|value| !value.trim().is_empty()),
            health: item.health,
            contract: item.contract,
            locality: item.locality,
            family: item.family,
            credential_providers: item.credential_providers,
            environment: launch.environment.clone(),
            policy_source: item.policy_source.filter(|value| !value.trim().is_empty()),
            source_revision: Some(launch.tracked_revision.clone()),
            manifest_digest: launch.dirty_digest.clone(),
            origin,
            launch,
        });
    }
    Ok(specs)
}

fn validate_launch(
    origin: &ContainerDeclarationOrigin,
    container_url: Option<&str>,
    launch: ContainerLaunchFile,
) -> Result<ContainerLaunchDeclarationV1> {
    if launch.schema_version != "synth.container-launch.v1" {
        return Err(LaunchDeclarationError::UnsupportedSchema {
            found: launch.schema_version,
        }
        .into_anyhow());
    }
    anyhow::ensure!(
        !launch.command.is_empty(),
        "launch_declaration_invalid: command is empty"
    );
    anyhow::ensure!(
        launch.readiness_timeout_seconds > 0,
        "launch_declaration_invalid: readiness timeout must be positive"
    );
    anyhow::ensure!(
        launch.shutdown_grace_seconds > 0,
        "launch_declaration_invalid: shutdown grace must be positive"
    );
    anyhow::ensure!(
        !launch.image_ref.trim().is_empty(),
        "launch_declaration_invalid: image_ref is empty"
    );
    anyhow::ensure!(
        !launch.health_target.trim().is_empty(),
        "launch_declaration_invalid: health_target is empty"
    );
    anyhow::ensure!(
        launch.source.revision_policy == "exact-or-dirty-digest",
        "launch_declaration_invalid: unsupported revision_policy `{}`",
        launch.source.revision_policy
    );
    anyhow::ensure!(
        !launch.source.tracked_revision.trim().is_empty(),
        "launch_declaration_invalid: tracked_revision is empty"
    );
    let expected = container_url
        .and_then(|value| reqwest::Url::parse(value).ok())
        .and_then(|value| value.port_or_known_default());
    anyhow::ensure!(
        expected == Some(launch.expected_port),
        "launch_declaration_invalid: expected_port {} does not match container URL",
        launch.expected_port
    );
    for name in &launch.declared_environment {
        if name.is_empty()
            || !name
                .chars()
                .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        {
            return Err(
                LaunchDeclarationError::InvalidEnvironmentName { name: name.clone() }.into_anyhow(),
            );
        }
        let upper = name.to_ascii_uppercase();
        anyhow::ensure!(
            !upper.contains("KEY")
                && !upper.contains("SECRET")
                && !upper.contains("TOKEN")
                && !upper.contains("PASSWORD"),
            "launch_declaration_invalid: credential-bearing environment name `{name}` is forbidden"
        );
    }
    let declared = launch
        .declared_environment
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let configured = launch
        .environment
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    anyhow::ensure!(
        configured.is_subset(&declared),
        "launch_declaration_invalid: configured environment names must be declared"
    );
    let mut includes = launch.source.include;
    anyhow::ensure!(
        !includes.is_empty(),
        "launch_declaration_invalid: source include list is empty"
    );
    includes.sort();
    includes.dedup();
    for relative in &includes {
        resolve_repository_path(origin, relative).map_err(LaunchDeclarationError::into_anyhow)?;
    }
    let actual_digest = launch_source_manifest_digest(origin, &includes)?;
    if let Some(declared_digest) = launch.source.dirty_digest.as_deref() {
        if declared_digest != actual_digest {
            return Err(LaunchDeclarationError::SourceDigestMismatch {
                declared: declared_digest.to_string(),
                actual: actual_digest,
            }
            .into_anyhow());
        }
    }
    let git_revision = std::process::Command::new("git")
        .arg("-C")
        .arg(&origin.source_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string());
    if let Some(current_revision) = git_revision {
        if current_revision != launch.source.tracked_revision {
            return Err(LaunchDeclarationError::CheckoutRevisionMismatch {
                declared: launch.source.tracked_revision.clone(),
                actual: current_revision,
            }
            .into_anyhow());
        }
        let mut status = std::process::Command::new("git");
        status
            .arg("-C")
            .arg(&origin.source_root)
            .args(["status", "--porcelain", "--"]);
        status.args(&includes);
        let dirty = status
            .output()
            .context("launch_source_mismatch: inspect declared launch inputs")?;
        anyhow::ensure!(
            dirty.status.success(),
            "launch_source_mismatch: git status failed"
        );
        if !dirty.stdout.is_empty() {
            anyhow::ensure!(
                launch.source.dirty_digest.is_some(),
                "launch_source_mismatch: declared launch inputs are dirty but no dirty_digest was provided"
            );
        }
    }
    Ok(ContainerLaunchDeclarationV1 {
        working_directory: resolve_repository_path(origin, &launch.working_directory)
            .map_err(LaunchDeclarationError::into_anyhow)?
            .absolute_path,
        command: launch.command,
        readiness_timeout_seconds: launch.readiness_timeout_seconds,
        shutdown_grace_seconds: launch.shutdown_grace_seconds,
        expected_port: launch.expected_port,
        image_ref: launch.image_ref,
        health_target: launch.health_target,
        declared_environment: launch.declared_environment,
        environment: launch.environment,
        tracked_revision: launch.source.tracked_revision,
        dirty_digest: launch
            .source
            .dirty_digest
            .filter(|value| !value.trim().is_empty()),
        include: includes,
    })
}

fn launch_source_manifest_digest(
    origin: &ContainerDeclarationOrigin,
    includes: &[String],
) -> Result<String> {
    let mut hasher = Sha256::new();
    for relative in includes {
        let path = resolve_repository_path(origin, relative)
            .map_err(LaunchDeclarationError::into_anyhow)?
            .absolute_path;
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(fs::read(&path).with_context(|| {
            format!(
                "launch_source_mismatch: read declared input {}",
                path.display()
            )
        })?);
        hasher.update([0]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

pub fn resolve_repository_path(
    origin: &ContainerDeclarationOrigin,
    relative: &str,
) -> Result<ResolvedRepositoryPath, LaunchDeclarationError> {
    let declared = relative.trim();
    if declared.is_empty() {
        return Err(LaunchDeclarationError::SourcePathMissing {
            manifest_path: origin.manifest_path.clone(),
            source_root: origin.source_root.clone(),
            declared_path: relative.to_string(),
            resolved_path: origin.source_root.clone(),
        });
    }
    let raw = Path::new(declared);
    if raw.is_absolute() {
        return Err(LaunchDeclarationError::AbsoluteManifestPath {
            declared_path: declared.to_string(),
        });
    }
    if raw
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        let resolved = origin.source_root.join(raw);
        return Err(LaunchDeclarationError::SourcePathEscapesRoot {
            manifest_path: origin.manifest_path.clone(),
            source_root: origin.source_root.clone(),
            declared_path: declared.to_string(),
            resolved_path: resolved,
        });
    }
    let source_root = origin
        .source_root
        .canonicalize()
        .unwrap_or_else(|_| origin.source_root.clone());
    let candidate = source_root.join(raw);
    let canonical = match candidate.canonicalize() {
        Ok(path) => path,
        Err(_) => {
            return Err(LaunchDeclarationError::SourcePathMissing {
                manifest_path: origin.manifest_path.clone(),
                source_root: source_root.clone(),
                declared_path: declared.to_string(),
                resolved_path: candidate,
            });
        }
    };
    if !canonical.starts_with(&source_root) {
        return Err(LaunchDeclarationError::SourcePathEscapesRoot {
            manifest_path: origin.manifest_path.clone(),
            source_root,
            declared_path: declared.to_string(),
            resolved_path: canonical,
        });
    }
    Ok(ResolvedRepositoryPath {
        source_root,
        absolute_path: canonical,
    })
}

/// Recipe policy paths stay bound to the session workspace. Container launch
/// paths must go through [`resolve_repository_path`].
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
        let error = find_recipe(&workspace, "eval.too-rich.v1")
            .unwrap_err()
            .to_string();
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
url = "http://127.0.0.1:8098"
health = "/health"
contract = "synth-containers/v1"
locality = "container"
credential_providers = ["openrouter"]
[container.launch]
schema_version = "synth.container-launch.v1"
working_directory = "svc"
command = ["python3", "serve.py"]
readiness_timeout_seconds = 30
shutdown_grace_seconds = 5
expected_port = 8098
image_ref = "fixture"
health_target = "fixture"
declared_environment = ["SYNTH_CRAFTAX_URL"]
environment = { SYNTH_CRAFTAX_URL = "http://127.0.0.1:8098" }
[container.launch.source]
revision_policy = "exact-or-dirty-digest"
tracked_revision = "fixture-revision"
include = ["svc/serve.py"]
"#,
        )
        .unwrap();
        let spec = find_container_spec(&workspace, "classify").unwrap();
        assert!(spec.cwd.starts_with(&workspace.canonicalize().unwrap()));
        assert!(spec.cwd.ends_with("svc"));
        assert_eq!(spec.credential_providers, vec!["openrouter"]);
        assert_eq!(
            spec.environment["SYNTH_CRAFTAX_URL"],
            "http://127.0.0.1:8098"
        );
    }

    #[test]
    fn launch_declaration_rejects_orchestrator_ownership_policy() {
        let (_dir, workspace) = write_workspace();
        fs::create_dir_all(workspace.join("svc")).unwrap();
        fs::write(workspace.join("svc/serve.py"), "print('ok')").unwrap();
        fs::write(
            workspace.join(CONTAINERS_FILE),
            r#"
[[container]]
id = "classify"
url = "http://127.0.0.1:8098"
locality = "container"
[container.launch]
schema_version = "synth.container-launch.v1"
working_directory = "svc"
command = ["python3", "serve.py"]
ownership = "workshop-adoptable"
readiness_timeout_seconds = 30
shutdown_grace_seconds = 5
expected_port = 8098
image_ref = "fixture"
health_target = "fixture"
[container.launch.source]
revision_policy = "exact-or-dirty-digest"
tracked_revision = "fixture-revision"
include = ["svc/serve.py"]
"#,
        )
        .unwrap();

        let error = format!(
            "{:#}",
            find_container_spec(&workspace, "classify").unwrap_err()
        );
        assert!(error.contains("unknown field `ownership`"), "{error}");
    }

    #[test]
    fn legacy_container_command_is_not_launch_authority() {
        let (_dir, workspace) = write_workspace();
        fs::write(
            workspace.join(CONTAINERS_FILE),
            r#"
[[container]]
id = "legacy"
command = ["scripts/start.sh"]
cwd = "."
url = "http://127.0.0.1:8098"
locality = "container"
"#,
        )
        .unwrap();
        let error = find_container_spec(&workspace, "legacy")
            .unwrap_err()
            .to_string();
        assert!(error.contains("launch_declaration_missing"), "{error}");
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

    fn write_container_manifest(root: &Path, include: &str) {
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(root.join(include), "#!/bin/sh\necho ok\n").unwrap();
        fs::write(
            root.join(CONTAINERS_FILE),
            format!(
                r#"
[[container]]
id = "nanohorizon-craftax"
url = "http://127.0.0.1:18091"
health = "/health"
contract = "synth.container.live-eval.v1"
locality = "container"
family = "craftax"
credential_providers = ["openrouter"]
policy_source = "src/challenge/policy.py"
[container.launch]
schema_version = "synth.container-launch.v1"
working_directory = "."
command = ["scripts/up_craftax_container.sh"]
readiness_timeout_seconds = 30
shutdown_grace_seconds = 5
expected_port = 18091
image_ref = "craftax-gamebench-rust"
health_target = "craftax_nanohorizon"
declared_environment = ["WORKSHOP_PROXY_ONLY"]
environment = {{ WORKSHOP_PROXY_ONLY = "1" }}
[container.launch.source]
revision_policy = "exact-or-dirty-digest"
tracked_revision = "fixture-revision"
include = ["{include}"]
"#
            ),
        )
        .unwrap();
        fs::create_dir_all(root.join("src/challenge")).unwrap();
        fs::write(root.join("src/challenge/policy.py"), "print('policy')").unwrap();
    }

    #[test]
    fn discovery_keeps_the_declaring_repository_not_the_session_workspace() {
        let dir = tempdir().unwrap();
        let session = dir.path().join("j-workspace");
        let source = dir.path().join("nanohorizon");
        fs::create_dir_all(&session).unwrap();
        write_container_manifest(&source, "scripts/up_craftax_container.sh");
        let spec = find_container_spec_in_roots(
            &[session.clone(), source.clone()],
            "nanohorizon-craftax",
        )
        .unwrap();
        assert_eq!(spec.origin.source_root, source.canonicalize().unwrap());
        assert!(spec.origin.manifest_path.starts_with(&spec.origin.source_root));
        assert!(spec.cwd.starts_with(&spec.origin.source_root));
        assert!(spec.cwd.starts_with(&source.canonicalize().unwrap()));
        assert!(!spec.cwd.starts_with(&session));
    }

    #[test]
    fn nested_manifest_retains_the_nested_declaring_root() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("approved");
        let nested = parent.join("nanohorizon");
        fs::create_dir_all(&parent).unwrap();
        write_container_manifest(&nested, "scripts/up_craftax_container.sh");
        let spec = find_container_spec_in_roots(&[parent], "nanohorizon-craftax").unwrap();
        assert_eq!(spec.origin.source_root, nested.canonicalize().unwrap());
    }

    #[test]
    fn symlink_escape_from_the_approved_source_root_is_rejected() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("nanohorizon");
        let outside = dir.path().join("outside.sh");
        fs::write(&outside, "echo leak").unwrap();
        write_container_manifest(&source, "scripts/up_craftax_container.sh");
        std::os::unix::fs::symlink(&outside, source.join("scripts/escape.sh")).unwrap();
        let origin = ContainerDeclarationOrigin {
            manifest_path: source.join(CONTAINERS_FILE),
            source_root: source.clone(),
            declaration_id: "nanohorizon-craftax".into(),
            source_revision: None,
            source_digest: None,
        };
        let error = resolve_repository_path(&origin, "scripts/escape.sh").unwrap_err();
        assert_eq!(error.code(), "launch_source_path_escapes_root");
    }

    #[test]
    fn dirty_declared_inputs_validate_against_the_exact_digest() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("nanohorizon");
        write_container_manifest(&source, "scripts/up_craftax_container.sh");
        let origin = ContainerDeclarationOrigin {
            manifest_path: source.join(CONTAINERS_FILE),
            source_root: source.clone(),
            declaration_id: "nanohorizon-craftax".into(),
            source_revision: Some("fixture-revision".into()),
            source_digest: None,
        };
        let actual = launch_source_manifest_digest(
            &origin,
            &["scripts/up_craftax_container.sh".into()],
        )
        .unwrap();
        fs::write(
            source.join(CONTAINERS_FILE),
            format!(
                r#"
[[container]]
id = "nanohorizon-craftax"
url = "http://127.0.0.1:18091"
locality = "container"
[container.launch]
schema_version = "synth.container-launch.v1"
working_directory = "."
command = ["scripts/up_craftax_container.sh"]
readiness_timeout_seconds = 30
shutdown_grace_seconds = 5
expected_port = 18091
image_ref = "craftax-gamebench-rust"
health_target = "craftax_nanohorizon"
[container.launch.source]
revision_policy = "exact-or-dirty-digest"
tracked_revision = "fixture-revision"
dirty_digest = "sha256:deadbeef"
include = ["scripts/up_craftax_container.sh"]
"#
            ),
        )
        .unwrap();
        let error = find_container_spec(&source, "nanohorizon-craftax")
            .unwrap_err()
            .to_string();
        assert!(error.contains("launch_source_digest_mismatch"), "{error}");
        assert!(error.contains("sha256:deadbeef"), "{error}");
        assert!(error.contains(&actual), "{error}");
    }

    #[test]
    fn missing_include_reports_declared_and_resolved_paths() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("nanohorizon");
        write_container_manifest(&source, "scripts/up_craftax_container.sh");
        fs::write(
            source.join(CONTAINERS_FILE),
            r#"
[[container]]
id = "nanohorizon-craftax"
url = "http://127.0.0.1:18091"
locality = "container"
[container.launch]
schema_version = "synth.container-launch.v1"
working_directory = "."
command = ["scripts/up_craftax_container.sh"]
readiness_timeout_seconds = 30
shutdown_grace_seconds = 5
expected_port = 18091
image_ref = "craftax-gamebench-rust"
health_target = "craftax_nanohorizon"
[container.launch.source]
revision_policy = "exact-or-dirty-digest"
tracked_revision = "fixture-revision"
include = ["scripts/missing.sh"]
"#,
        )
        .unwrap();
        let error = find_container_spec(&source, "nanohorizon-craftax").unwrap_err();
        let failure = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<StructuredFailure>())
            .expect("typed launch failure");
        assert_eq!(failure.code, "launch_source_path_not_found");
        assert_eq!(failure.details["declared_path"], "scripts/missing.sh");
        let resolved = failure.details["resolved_path"].as_str().unwrap();
        assert!(
            resolved.ends_with("scripts/missing.sh"),
            "{resolved}"
        );
        let json = failure.to_json();
        assert_eq!(json["code"], "launch_source_path_not_found");
        assert_eq!(json["declared_path"], "scripts/missing.sh");
        assert!(!failure.message.contains("launch_declaration_invalid"));
    }

    #[test]
    fn matching_dirty_digest_validates() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("nanohorizon");
        write_container_manifest(&source, "scripts/up_craftax_container.sh");
        let origin = ContainerDeclarationOrigin {
            manifest_path: source.join(CONTAINERS_FILE),
            source_root: source.clone(),
            declaration_id: "nanohorizon-craftax".into(),
            source_revision: Some("fixture-revision".into()),
            source_digest: None,
        };
        let actual = launch_source_manifest_digest(
            &origin,
            &["scripts/up_craftax_container.sh".into()],
        )
        .unwrap();
        fs::write(
            source.join(CONTAINERS_FILE),
            format!(
                r#"
[[container]]
id = "nanohorizon-craftax"
url = "http://127.0.0.1:18091"
locality = "container"
[container.launch]
schema_version = "synth.container-launch.v1"
working_directory = "."
command = ["scripts/up_craftax_container.sh"]
readiness_timeout_seconds = 30
shutdown_grace_seconds = 5
expected_port = 18091
image_ref = "craftax-gamebench-rust"
health_target = "craftax_nanohorizon"
[container.launch.source]
revision_policy = "exact-or-dirty-digest"
tracked_revision = "fixture-revision"
dirty_digest = "{actual}"
include = ["scripts/up_craftax_container.sh"]
"#
            ),
        )
        .unwrap();
        let spec = find_container_spec(&source, "nanohorizon-craftax").unwrap();
        assert_eq!(spec.origin.source_digest.as_deref(), Some(actual.as_str()));
    }

    #[test]
    fn absolute_launch_path_is_rejected() {
        let origin = ContainerDeclarationOrigin {
            manifest_path: PathBuf::from("/tmp/workshop.containers.toml"),
            source_root: PathBuf::from("/tmp"),
            declaration_id: "classify".into(),
            source_revision: None,
            source_digest: None,
        };
        let error = resolve_repository_path(&origin, "/etc/passwd").unwrap_err();
        assert_eq!(error.code(), "launch_absolute_path_rejected");
        let traversal = resolve_repository_path(&origin, "../outside.sh").unwrap_err();
        assert_eq!(traversal.code(), "launch_source_path_escapes_root");
    }

    #[test]
    fn instance_workspace_is_not_used_as_the_launch_root() {
        let dir = tempdir().unwrap();
        let instance = dir.path().join("workshop-instance");
        let session = dir.path().join("chat");
        let source = dir.path().join("nanohorizon");
        fs::create_dir_all(&instance).unwrap();
        fs::create_dir_all(&session).unwrap();
        write_container_manifest(&source, "scripts/up_craftax_container.sh");
        let spec = find_container_spec_in_roots(
            &[session, instance, source.clone()],
            "nanohorizon-craftax",
        )
        .unwrap();
        assert_eq!(spec.origin.source_root, source.canonicalize().unwrap());
    }
}
