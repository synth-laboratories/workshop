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
/// Matches Workshop B's bounded-approval ceiling. Individual recipes remain
/// responsible for declaring a tighter, evidence-based envelope; the exact
/// amount is still shown for per-run operator approval.
pub const PRODUCT_MAX_COST_USD: f64 = 50.00;
pub const PRODUCT_MAX_TOTAL_ROLLOUTS: i64 = 480;

const RECIPE_FILE: &str = "workshop.recipe.toml";
const RECIPES_DIR: &str = "workshop.recipes";
const CONTAINERS_FILE: &str = "workshop.containers.toml";

/// Shipped annotated eval recipes. Written into a session workspace on first
/// catalog list so a fresh session can run them without copying fixtures.
const BUNDLED_ANNOTATION_EVAL_RECIPES: &[(&str, &str)] = &[
    (
        "eval.craftax.gold.annotated.v1.toml",
        include_str!("../../recipes/annotation_eval/eval.craftax.gold.annotated.v1.toml"),
    ),
    (
        "eval.banking77.annotated.v1.toml",
        include_str!("../../recipes/annotation_eval/eval.banking77.annotated.v1.toml"),
    ),
    (
        "eval.banking77.live_annotated.v1.toml",
        include_str!("../../recipes/annotation_eval/eval.banking77.live_annotated.v1.toml"),
    ),
    (
        "eval.deepswe.annotated.v1.toml",
        include_str!("../../recipes/annotation_eval/eval.deepswe.annotated.v1.toml"),
    ),
    (
        "eval.code_policy.annotated.v1.toml",
        include_str!("../../recipes/annotation_eval/eval.code_policy.annotated.v1.toml"),
    ),
    (
        "eval.healthbench.annotated.v1.toml",
        include_str!("../../recipes/annotation_eval/eval.healthbench.annotated.v1.toml"),
    ),
    (
        "eval.healthbench.live_annotated.v1.toml",
        include_str!("../../recipes/annotation_eval/eval.healthbench.live_annotated.v1.toml"),
    ),
    (
        "eval.craftax.gold.live_annotated.v1.toml",
        include_str!("../../recipes/annotation_eval/eval.craftax.gold.live_annotated.v1.toml"),
    ),
];

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
    /// Optional exact container task identities, positionally aligned with
    /// the corresponding seed pools. Empty means the legacy family/seed
    /// identity is used.
    pub train_task_instance_ids: Vec<String>,
    pub heldout_task_instance_ids: Vec<String>,
    pub minibatch_size: usize,
    pub concurrency: usize,
    pub proposer_model: Option<String>,
    pub requires_credential_advertisement: bool,
    /// How this recipe wants a running rollout's event journal drained and its
    /// native frames retained. Declared, never inferred: a 500-step Craftax
    /// episode and a two-call classifier do not want the same cadence, and the
    /// difference must not be a code edit.
    pub relay: super::eval_relay::RelaySettings,
    /// Optional post-rollout annotation stage; `None` means off (the default).
    pub annotation: Option<super::annotation_stage::AnnotationStageSpec>,
    /// Optional live annotation protocol streamed beside each rollout while
    /// it runs (observe-only, provisional); `None` means off.
    pub live_annotation: Option<super::live_annotation::LiveAnnotationSpec>,
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
    train_task_instance_ids: Option<Vec<String>>,
    #[serde(default)]
    heldout_task_instance_ids: Option<Vec<String>>,
    #[serde(default)]
    minibatch_size: Option<usize>,
    #[serde(default)]
    event_stream: Option<EventStreamFile>,
    #[serde(default)]
    media: Option<MediaFile>,
    #[serde(default)]
    annotation: Option<toml::value::Table>,
    #[serde(default)]
    live_annotation: Option<toml::value::Table>,
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

/// Copy shipped annotated eval recipes into `workshop.recipes/` when missing.
/// Existing files win so an operator override is never overwritten.
pub fn ensure_bundled_annotation_eval_recipes(workspace: &Path) -> Result<usize> {
    let recipes_dir = workspace.join(RECIPES_DIR);
    fs::create_dir_all(&recipes_dir)
        .with_context(|| format!("create {}", recipes_dir.display()))?;
    let mut written = 0usize;
    for (name, contents) in BUNDLED_ANNOTATION_EVAL_RECIPES {
        let dest = recipes_dir.join(name);
        if dest.exists() {
            continue;
        }
        fs::write(&dest, contents).with_context(|| format!("write {}", dest.display()))?;
        written += 1;
    }
    Ok(written)
}

pub fn load_recipes(workspace: &Path) -> Result<Vec<WorkspaceRecipe>> {
    let mut recipes = Vec::new();
    for path in recipe_paths(workspace)? {
        recipes.push(parse_recipe(&path)?);
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

fn recipe_paths(workspace: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let canonical_root = workspace.canonicalize().context("recipe source is unavailable")?;
    let contained = |path: &Path| -> Result<PathBuf> {
        let canonical = path.canonicalize().context("recipe declaration is unavailable")?;
        if !canonical.starts_with(&canonical_root) {
            bail!("recipe_source_root_not_approved: {} escapes its source", path.display());
        }
        Ok(canonical)
    };
    let root_file = workspace.join(RECIPE_FILE);
    if root_file.exists() || root_file.is_symlink() {
        paths.push(contained(&root_file)?);
    }
    let recipes_dir = workspace.join(RECIPES_DIR);
    if recipes_dir.exists() || recipes_dir.is_symlink() {
        contained(&recipes_dir)?;
    }
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
        for entry in entries { paths.push(contained(&entry)?); }
    }
    Ok(paths)
}

fn declared_recipe_id(path: &Path) -> Result<Option<String>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let value: toml::Value = toml::from_str(&text)
        .with_context(|| format!("parse workspace recipe identity {}", path.display()))?;
    Ok(value
        .get("id")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned))
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

/// Resolve recipes only from executable project sources with recipe capability.
/// Conversation file attachments do not grant execution authority.
pub fn find_session_recipe(
    _db: &crate::storage::Database,
    _session_id: &str,
    recipe_id: &str,
) -> Result<(PathBuf, WorkspaceRecipe)> {
    let roots = crate::project_sources::discovery_roots(crate::project_sources::Capability::Recipes)?;
    let mut matches = Vec::new();
    for root in roots {
        for path in recipe_paths(&root)? {
            match parse_recipe(&path) {
                Ok(recipe) if recipe.id == recipe_id => matches.push((root.clone(), recipe)),
                Ok(_) => {}
                Err(error) => match declared_recipe_id(&path) {
                    Ok(Some(id)) if id != recipe_id => {}
                    _ => return Err(error),
                },
            }
        }
    }
    match matches.len() {
        0 => Err(anyhow!(
            "workspace recipe `{recipe_id}` is not declared in any approved recipe source"
        )),
        1 => Ok(matches.remove(0)),
        _ => Err(anyhow!(
            "workspace recipe `{recipe_id}` is declared in more than one approved recipe source"
        )),
    }
}

/// Catalog recipes from the same approved roots used by execution. Duplicate
/// ids are retained here so start can reject the ambiguity instead of the
/// catalog silently choosing one source.
pub fn load_session_recipes(
    _db: &crate::storage::Database,
    _session_id: &str,
) -> Result<Vec<WorkspaceRecipe>> {
    let mut recipes = Vec::new();
    for root in crate::project_sources::discovery_roots(crate::project_sources::Capability::Recipes)? {
        for path in recipe_paths(&root)? {
            if let Ok(recipe) = parse_recipe(&path) {
                recipes.push(recipe);
            }
        }
    }
    Ok(recipes)
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
    let source_root = manifest_path.parent().ok_or_else(|| {
        anyhow!(
            "container manifest {} has no parent",
            manifest_path.display()
        )
    })?;
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
        let Ok(canonical) = root.canonicalize() else {
            continue;
        };
        collect_container_manifests(&canonical, &canonical, 2, &mut manifests);
    }
    manifests.sort();
    manifests.dedup();
    Ok(manifests)
}

fn collect_container_manifests(root: &Path, approved: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(canonical_root) = root.canonicalize() else {
        return;
    };
    if !canonical_root.starts_with(approved) {
        return;
    }
    let candidate = root.join(CONTAINERS_FILE);
    if candidate.is_file() {
        if let Ok(canonical) = candidate.canonicalize() {
            if canonical.starts_with(approved) {
                out.push(canonical);
            }
        }
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
        collect_container_manifests(&path, approved, depth.saturating_sub(1), out);
    }
}

pub fn origin_is_under_approved_roots(
    origin: &ContainerDeclarationOrigin,
    search_roots: &[PathBuf],
) -> bool {
    let (Ok(source), Ok(manifest)) = (
        origin.source_root.canonicalize(),
        origin.manifest_path.canonicalize(),
    ) else {
        return false;
    };
    search_roots.iter().any(|root| {
        let Ok(root) = root.canonicalize() else {
            return false;
        };
        source.starts_with(&root) && manifest.starts_with(&source) && manifest.is_file()
    })
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
    _db: &crate::storage::Database,
    _session_id: &str,
) -> Result<Vec<PathBuf>> {
    crate::project_sources::discovery_roots(crate::project_sources::Capability::Containers)
}

pub fn catalog_entry(recipe: &WorkspaceRecipe) -> Value {
    let credential_inputs: Vec<&str> = match recipe.provider.to_ascii_lowercase().as_str() {
        "none" => Vec::new(),
        "openrouter" => vec!["OPENROUTER_API_KEY"],
        "anthropic" => vec!["ANTHROPIC_API_KEY"],
        _ => vec!["OPENAI_API_KEY"],
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
        "provider": recipe.provider,
        "sourceHash": recipe.source_hash,
        "limits": {
            "maxCostUsd": recipe.bounds.max_cost_usd,
            "maxTotalRollouts": recipe.bounds.max_total_rollouts,
            "maximumModelCallsPerRollout": recipe.policy
                .get("max_calls")
                .and_then(Value::as_i64),
            "maximumStepsPerRollout": recipe.policy
                .get("max_steps")
                .and_then(Value::as_i64),
            "maxTrainRollouts": recipe.bounds.max_train_rollouts,
            "maxHeldoutRollouts": recipe.bounds.max_heldout_rollouts,
            "maxGenerations": recipe.bounds.max_generations,
        },
        "policyRef": {
            "harness": recipe.harness,
            "config": recipe.policy_config,
        },
        "taskInstanceIds": {
            "train": recipe.train_task_instance_ids,
            "heldout": recipe.heldout_task_instance_ids,
        },
        "credentialInputs": credential_inputs,
        "expectedVisual": match recipe.algorithm {
            AlgorithmKind::Gepa => "optimizer.gepa.v1",
            AlgorithmKind::Eval => "experiment.overview.v1",
        },
        "liveAnnotation": recipe.live_annotation.as_ref().map(|spec| spec.summary_json()),
        "annotation": recipe.annotation.as_ref().map(|stage| json!({
            "label": stage.label,
            "annotatorCount": stage.annotators.len(),
            "annotators": stage.annotators.iter().map(|item| item.annotator_id.clone()).collect::<Vec<_>>(),
        })),
    })
}

pub fn copy_into_run_dir(recipe: &WorkspaceRecipe, run_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(run_dir).context("create run-owned recipe directory")?;
    let destination = run_dir.join(RECIPE_FILE);
    let source = fs::read_to_string(&recipe.source_path)
        .with_context(|| format!("read {}", recipe.source_path.display()))?;
    let mut document: toml::Value = toml::from_str(&source)
        .with_context(|| format!("parse {}", recipe.source_path.display()))?;
    let root = document
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("workspace recipe root must be a TOML table"))?;
    let policy = root
        .entry("policy")
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("workspace recipe policy must be a TOML table"))?;
    // The workspace contract owns provider/model at the recipe root, while
    // the installed Optimizers GEPA contract consumes them from [policy].
    // Compile those authoritative values into the run-owned snapshot instead
    // of asking authors to maintain two potentially contradictory copies.
    policy.insert(
        "provider".into(),
        toml::Value::String(recipe.provider.clone()),
    );
    policy.insert("model".into(), toml::Value::String(recipe.model.clone()));
    if matches!(recipe.algorithm, AlgorithmKind::Gepa) {
        compile_gepa_task_contract(root, recipe)?;
    }
    let normalized = toml::to_string_pretty(&document)
        .context("encode normalized run-owned workspace recipe")?;
    fs::write(&destination, normalized)
        .with_context(|| format!("write {}", destination.display()))?;
    Ok(destination)
}

/// Compile Workshop's compact, seed-oriented GEPA declaration into the
/// installed optimizer's explicit task-set contract. Authors declare stable
/// seeds once; the run-owned snapshot carries the concrete ids and pools that
/// the sidecar validates before any provider call.
fn compile_gepa_task_contract(
    root: &mut toml::value::Table,
    recipe: &WorkspaceRecipe,
) -> Result<()> {
    let train_ids = recipe
        .train_seeds
        .iter()
        .map(|seed| toml::Value::String(format!("train:{seed}")))
        .collect::<Vec<_>>();
    let heldout_ids = recipe
        .heldout_seeds
        .iter()
        .map(|seed| toml::Value::String(format!("test:{seed}")))
        .collect::<Vec<_>>();
    anyhow::ensure!(
        !train_ids.is_empty() && !heldout_ids.is_empty(),
        "GEPA recipe `{}` requires non-empty train_seeds and heldout_seeds",
        recipe.id
    );

    root.insert(
        "taskset".into(),
        toml::Value::Table(
            [
                ("train_split".into(), toml::Value::String("train".into())),
                ("heldout_split".into(), toml::Value::String("test".into())),
                ("train_ids".into(), toml::Value::Array(train_ids.clone())),
                (
                    "heldout_ids".into(),
                    toml::Value::Array(heldout_ids.clone()),
                ),
            ]
            .into_iter()
            .collect(),
        ),
    );

    let candidate_field = root
        .get("candidate_field")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("system_prompt")
        .to_string();
    root.insert(
        "candidate".into(),
        toml::Value::Table(
            [(
                "target_modules".into(),
                toml::Value::Array(vec![toml::Value::String(candidate_field.clone())]),
            )]
            .into_iter()
            .collect(),
        ),
    );
    root.entry("seed_candidate").or_insert_with(|| {
        toml::Value::Table(
            [(
                candidate_field,
                toml::Value::String(
                    "Classify the complete user request into exactly one canonical label. Return only that label, preserving its exact spelling and punctuation.".into(),
                ),
            )]
            .into_iter()
            .collect(),
        )
    });

    let policy = root
        .get_mut("policy")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| anyhow!("GEPA recipe policy must be a table"))?;
    let api_family = policy
        .get("api")
        .and_then(toml::Value::as_str)
        .unwrap_or("chat_completions")
        .to_string();
    policy.insert("api_family".into(), toml::Value::String(api_family));
    policy.insert(
        "api_key_env".into(),
        toml::Value::String("OPENAI_API_KEY".into()),
    );
    policy.insert(
        "proxy_mode".into(),
        toml::Value::String("proxy_only".into()),
    );

    // The installed runtime otherwise defaults an omitted proposer to the
    // Codex app-server with a public OpenAI origin.  A Workshop proxy sentinel
    // is not a public API key, so that fallback both violates the admitted
    // route and fails with a misleading 401. Materialize the workspace-owned
    // proposer explicitly; bind_locality_urls attaches its bounded host proxy
    // URL later, alongside the policy's container-visible route.
    let declared_proposer = recipe
        .proposer_model
        .as_deref()
        .unwrap_or(recipe.model.as_str());
    let proposer_model = declared_proposer
        .strip_prefix(&format!("{}/", recipe.provider))
        .unwrap_or(declared_proposer)
        .to_string();
    let declared_reasoning_effort = policy
        .get("effort")
        .and_then(toml::Value::as_str)
        .unwrap_or("medium");
    // The chat-completions proposer runtime currently supports this bounded
    // effort vocabulary. Preserve the selected model while mapping newer
    // Codex effort tiers to the strongest compatible proposer setting.
    let reasoning_effort = match declared_reasoning_effort {
        "xhigh" | "max" | "ultra" => "high",
        "none" | "low" | "medium" | "high" => declared_reasoning_effort,
        _ => "medium",
    }
    .to_string();
    root.insert(
        "proposer".into(),
        toml::Value::Table(
            [
                (
                    "backend".into(),
                    toml::Value::String("chat_completions".into()),
                ),
                (
                    "provider".into(),
                    toml::Value::String(recipe.provider.clone()),
                ),
                (
                    "api_family".into(),
                    toml::Value::String("chat_completions".into()),
                ),
                ("model".into(), toml::Value::String(proposer_model)),
                (
                    "reasoning_effort".into(),
                    toml::Value::String(reasoning_effort),
                ),
                (
                    "allow_unverified_model".into(),
                    toml::Value::Boolean(recipe.provider == "openrouter"),
                ),
                ("auth_mode".into(), toml::Value::String("api_key".into())),
                (
                    "api_key_env".into(),
                    toml::Value::String("OPENAI_API_KEY".into()),
                ),
                ("timeout_seconds".into(), toml::Value::Integer(300)),
                (
                    "message_stall_timeout_seconds".into(),
                    toml::Value::Integer(120),
                ),
            ]
            .into_iter()
            .collect(),
        ),
    );

    let minibatch = train_ids
        .iter()
        .take(recipe.minibatch_size)
        .cloned()
        .collect::<Vec<_>>();
    let mut gepa = toml::value::Table::new();
    gepa.insert(
        "max_total_rollouts".into(),
        toml::Value::Integer(recipe.bounds.max_total_rollouts),
    );
    gepa.insert(
        "max_cost_usd".into(),
        toml::Value::Float(recipe.bounds.max_cost_usd),
    );
    gepa.insert(
        "max_generations".into(),
        toml::Value::Integer(recipe.bounds.max_generations.unwrap_or(1)),
    );
    gepa.insert(
        "minibatch_size".into(),
        toml::Value::Integer(recipe.minibatch_size as i64),
    );
    gepa.insert(
        "task_pools".into(),
        toml::Value::Table(
            [
                ("pareto".into(), toml::Value::Array(train_ids.clone())),
                ("minibatch".into(), toml::Value::Array(minibatch)),
                ("reflection".into(), toml::Value::Array(train_ids)),
                ("heldout".into(), toml::Value::Array(heldout_ids)),
            ]
            .into_iter()
            .collect(),
        ),
    );
    root.insert("gepa".into(), toml::Value::Table(gepa));
    Ok(())
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
        // The inner sandbox of a nested agentic CLI. Registering a policy
        // config replaces the container's seeded configuration, so a recipe
        // that cannot name this key silently drops the seed's sandbox — which
        // is how the 2026-09-03 DeepSWE sample ran every task under a
        // namespace sandbox its container could not create.
        "sandbox",
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
    let annotation = parsed
        .annotation
        .as_ref()
        .map(|table| super::annotation_stage::AnnotationStageSpec::parse(&parsed.id, table))
        .transpose()?
        .flatten();
    let live_annotation = parsed
        .live_annotation
        .as_ref()
        .map(|table| super::live_annotation::LiveAnnotationSpec::parse(&parsed.id, table))
        .transpose()?
        .flatten();
    let train_seeds = parsed
        .train_seeds
        .unwrap_or_else(|| vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let normalize_task_ids = |pool: &str,
                              ids: Option<Vec<String>>,
                              seeds: &[i64]|
     -> Result<Vec<String>> {
        let ids = ids
            .unwrap_or_default()
            .into_iter()
            .map(|value| value.trim().to_string())
            .collect::<Vec<_>>();
        if ids.iter().any(String::is_empty) {
            bail!(
                "recipe `{}` {pool}_task_instance_ids contains an empty task id",
                parsed.id
            );
        }
        if !ids.is_empty() && ids.len() != seeds.len() {
            bail!(
                "recipe `{}` {pool}_task_instance_ids must contain exactly {} entries to match {pool}_seeds; got {}",
                parsed.id,
                seeds.len(),
                ids.len()
            );
        }
        let unique = ids.iter().collect::<std::collections::BTreeSet<_>>();
        if unique.len() != ids.len() {
            bail!(
                "recipe `{}` {pool}_task_instance_ids contains duplicates",
                parsed.id
            );
        }
        Ok(ids)
    };
    let heldout_seeds = parsed.heldout_seeds.unwrap_or_default();
    let train_task_instance_ids =
        normalize_task_ids("train", parsed.train_task_instance_ids, &train_seeds)?;
    let heldout_task_instance_ids =
        normalize_task_ids("heldout", parsed.heldout_task_instance_ids, &heldout_seeds)?;
    let minibatch_size = parsed.minibatch_size.unwrap_or(1);
    if minibatch_size == 0 || minibatch_size > train_seeds.len() {
        bail!(
            "recipe `{}` minibatch_size must be 1..={} for its declared train seeds",
            parsed.id,
            train_seeds.len()
        );
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
        policy,
        policy_source: parsed
            .policy_source
            .filter(|value| !value.trim().is_empty()),
        train_seeds,
        heldout_seeds,
        train_task_instance_ids,
        heldout_task_instance_ids,
        minibatch_size,
        concurrency: parsed.concurrency.unwrap_or(1).max(1),
        proposer_model: parsed.proposer_model,
        requires_credential_advertisement: parsed.requires_credential_advertisement,
        relay,
        annotation,
        live_annotation,
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
            if !name
                .chars()
                .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
                || name.is_empty()
                || credential_bearing_environment_name(name)
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

fn credential_bearing_environment_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    if upper == "SYNTH_ANNOTATION_USD_PER_MILLION_TOKENS" {
        return false;
    }
    upper.contains("KEY")
        || upper.contains("SECRET")
        || upper.contains("TOKEN")
        || upper.contains("PASSWORD")
}

fn validate_launch(
    origin: &ContainerDeclarationOrigin,
    container_url: Option<&str>,
    mut launch: ContainerLaunchFile,
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
    let canonical_url = container_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("launch_declaration_invalid: container URL is required"))?;
    for (name, authoritative) in [
        ("PORT", launch.expected_port.to_string()),
        (
            "CRAFTAX_URL",
            canonical_url.trim_end_matches('/').to_string(),
        ),
    ] {
        if !launch.declared_environment.iter().any(|item| item == name) {
            continue;
        }
        if let Some(configured) = launch.environment.get(name) {
            anyhow::ensure!(
                configured == &authoritative,
                "launch_declaration_invalid: {name}={configured} contradicts the authoritative declaration value {authoritative}"
            );
        } else {
            launch.environment.insert(name.to_string(), authoritative);
        }
    }
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
        anyhow::ensure!(
            !credential_bearing_environment_name(name),
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
    let mut attested_revision = launch.source.tracked_revision.clone();
    if let Some(current_revision) = git_revision {
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
        let included_sources_are_dirty = !dirty.stdout.is_empty();
        if included_sources_are_dirty {
            if current_revision != launch.source.tracked_revision {
                return Err(LaunchDeclarationError::CheckoutRevisionMismatch {
                    declared: launch.source.tracked_revision.clone(),
                    actual: current_revision,
                }
                .into_anyhow());
            }
            anyhow::ensure!(
                launch.source.dirty_digest.is_some(),
                "launch_source_mismatch: declared launch inputs are dirty but no dirty_digest was provided"
            );
        } else if current_revision != launch.source.tracked_revision {
            // A manifest cannot name the commit that contains itself. Permit a
            // clean declaration to name an older tracked base only when that
            // base is in the current history and every admitted input is
            // byte-equivalent across the intervening commits. The current HEAD
            // is the revision Workshop attests downstream.
            let ancestor = std::process::Command::new("git")
                .arg("-C")
                .arg(&origin.source_root)
                .args([
                    "merge-base",
                    "--is-ancestor",
                    launch.source.tracked_revision.as_str(),
                    current_revision.as_str(),
                ])
                .status()
                .context("launch_source_mismatch: validate declared launch base revision")?;
            if !ancestor.success() {
                return Err(LaunchDeclarationError::CheckoutRevisionMismatch {
                    declared: launch.source.tracked_revision.clone(),
                    actual: current_revision,
                }
                .into_anyhow());
            }
            let mut unchanged = std::process::Command::new("git");
            unchanged
                .arg("-C")
                .arg(&origin.source_root)
                .args([
                    "diff",
                    "--quiet",
                    launch.source.tracked_revision.as_str(),
                    current_revision.as_str(),
                    "--",
                ])
                .args(&includes);
            let unchanged = unchanged
                .status()
                .context("launch_source_mismatch: compare declared launch inputs to HEAD")?;
            if !unchanged.success() {
                return Err(LaunchDeclarationError::CheckoutRevisionMismatch {
                    declared: launch.source.tracked_revision.clone(),
                    actual: current_revision,
                }
                .into_anyhow());
            }
        }
        attested_revision = current_revision;
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
        tracked_revision: attested_revision,
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

