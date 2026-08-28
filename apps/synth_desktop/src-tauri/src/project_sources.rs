//! One shared authority for project-source roots.
//!
//! A *project source* is a folder Workshop may discover executable
//! declarations in: `workshop.containers.toml`, `workshop.recipe.toml`, and
//! `workshop.recipes/*.toml`. It is a strictly stronger grant than a chat
//! workspace attachment, which only lets a conversation read and write files,
//! so it lives in its own authority with its own approval and its own record.
//!
//! Three rules hold everywhere in this module:
//!
//! 1. **An agent can request, never admit.** [`request`] writes a pending row
//!    and nothing else. Admission requires [`approve`], which requires a path
//!    the person at the keyboard re-selected in the native picker.
//! 2. **Canonical paths only.** Every comparison and every persisted value is
//!    the canonicalized directory, so a symlink cannot make one folder stand
//!    in for another between approval and use.
//! 3. **A record is provenance, never permission.** Declarations are re-read
//!    and re-hashed before every execution; removing a source stops future
//!    discovery immediately while historical receipts stay intact.

use crate::storage::{Database, EventAppend};
use crate::synth_config::{self, ProjectSourceEntry, ProjectSourceSettings};
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    env,
    path::{Path, PathBuf},
    sync::Arc,
};

const CONTAINERS_FILE: &str = "workshop.containers.toml";
const RECIPE_FILE: &str = "workshop.recipe.toml";
const RECIPES_DIR: &str = "workshop.recipes";
const CONTAINER_SOURCE_ROOTS_ENV: &str = "SYNTH_CONTAINER_SOURCE_ROOTS";
const RECIPE_SOURCE_ROOTS_ENV: &str = "SYNTH_RECIPE_SOURCE_ROOTS";

/// One Desktop instance is single-process; serialize the complete decision and
/// config mutation so approve and deny cannot cross between the DB check and
/// the TOML grant. Unlike a persisted intermediate state, this cannot strand a
/// request after a crash.
static REQUEST_RESOLUTION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// What a root is allowed to contribute to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Capability {
    Containers,
    Recipes,
}

impl Capability {
    fn enabled_for(self, entry: &ProjectSourceEntry) -> bool {
        match self {
            Self::Containers => entry.containers,
            Self::Recipes => entry.recipes,
        }
    }
}

/// Where an effective root came from. Reported in diagnostics so "no sources"
/// can be told apart from "sources configured, none of them valid".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RootOrigin {
    Configured,
    Environment,
    Remembered,
    DevelopmentFallback,
}

impl RootOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Environment => "environment",
            Self::Remembered => "remembered",
            Self::DevelopmentFallback => "development_fallback",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedRoot {
    pub path: PathBuf,
    pub origin: RootOrigin,
}

/// What Workshop found beneath one root, without running anything.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceInspection {
    pub path: String,
    /// `valid`, `invalid`, or `missing`.
    pub status: String,
    pub code: Option<String>,
    pub message: Option<String>,
    pub containers: Vec<String>,
    pub recipes: Vec<String>,
}

impl ProjectSourceInspection {
    pub fn is_valid(&self) -> bool {
        self.status == "valid"
    }
}

/// One persisted project source plus its current validation state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceRow {
    pub path: String,
    pub containers: bool,
    pub recipes: bool,
    pub origin: RootOrigin,
    pub inspection: ProjectSourceInspection,
    pub last_scanned_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceCatalog {
    pub config_path: String,
    pub sources: Vec<ProjectSourceRow>,
    /// Roots in effect that are *not* persisted -- environment overrides,
    /// remembered provenance, the development fallback. Shown so the user can
    /// see why discovery behaves as it does without reading the launcher.
    pub implicit_roots: Vec<ProjectSourceRow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceRequest {
    pub id: String,
    pub session_id: Option<String>,
    pub requested_path: String,
    pub canonical_path: String,
    pub reason: String,
    pub containers: bool,
    pub recipes: bool,
    pub attach_to_conversation: bool,
    pub status: String,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceApproval {
    pub request: ProjectSourceRequest,
    pub source: ProjectSourceRow,
    pub catalog: ProjectSourceCatalog,
    /// Present only when the request asked to attach the folder to its
    /// conversation and that attachment succeeded.
    pub scope: Option<crate::workspace_scope::ConversationWorkspaceScope>,
}

// ---------------------------------------------------------------------------
// Root resolution
// ---------------------------------------------------------------------------

/// Split a platform path list, dropping empty segments.
///
/// `env -i FOO=` and a trailing `:` both produce an empty segment.
/// `env::split_paths` yields that as an empty `PathBuf`, which canonicalizes to
/// the *current working directory* -- so an unset-looking variable used to turn
/// into "scan wherever the app happens to be running". An empty setting means
/// "not configured".
fn split_path_list(value: &std::ffi::OsStr) -> Vec<PathBuf> {
    env::split_paths(value)
        .filter(|path| !path.as_os_str().is_empty())
        .collect()
}

fn environment_roots(capability: Capability) -> Vec<PathBuf> {
    let names: &[&str] = match capability {
        Capability::Containers => &[CONTAINER_SOURCE_ROOTS_ENV],
        // A desktop that configured only container roots still declares recipe
        // sources in the same repositories.
        Capability::Recipes => &[RECIPE_SOURCE_ROOTS_ENV, CONTAINER_SOURCE_ROOTS_ENV],
    };
    for name in names {
        let Some(value) = env::var_os(name) else {
            continue;
        };
        let roots = split_path_list(&value);
        if !roots.is_empty() {
            return roots;
        }
    }
    Vec::new()
}

/// A bounded, developer-oriented last resort.
///
/// Kept only for the case where nothing at all is configured, so a fresh
/// developer install still finds its checkouts. It is reported as
/// [`RootOrigin::DevelopmentFallback`] rather than presented as a grant, and a
/// single approved repository takes precedence over it the moment one exists.
fn development_fallback_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("GitHub"));
    }
    if let Some(workshop) = env::var_os("SYNTH_WORKSHOP_ROOT") {
        roots.push(PathBuf::from(workshop));
    }
    roots
}

/// Every root discovery should consult, in documented precedence order.
///
/// 1. Roots persisted in `[desktop.project_sources]`.
/// 2. Non-empty explicit environment roots (developer/CI overrides).
/// 3. The development fallback -- **only** when 1-2 produced nothing.
///
/// 1-3 merge rather than shadow each other: an environment override in a CI
/// run must not silently drop the repository a user approved on the same
/// machine. Deduplication is by canonical path, so a symlinked duplicate of an
/// already-listed root collapses into it.
pub(crate) fn resolve_roots(_db: &Database, capability: Capability) -> Result<Vec<ResolvedRoot>> {
    let mut resolved: Vec<ResolvedRoot> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut push = |path: PathBuf, origin: RootOrigin, resolved: &mut Vec<ResolvedRoot>| {
        // An unreadable or missing root is not an error here; it is simply not
        // a place to look. `inspect` reports it when the user asks about it.
        let Ok(canonical) = path.canonicalize() else {
            return;
        };
        if !canonical.is_dir() || !seen.insert(canonical.clone()) {
            return;
        }
        resolved.push(ResolvedRoot {
            path: canonical,
            origin,
        });
    };

    for entry in synth_config::project_source_roots() {
        if capability.enabled_for(&entry) {
            push(
                PathBuf::from(&entry.path),
                RootOrigin::Configured,
                &mut resolved,
            );
        }
    }
    for path in environment_roots(capability) {
        push(path, RootOrigin::Environment, &mut resolved);
    }
    if resolved.is_empty() {
        for path in development_fallback_roots() {
            push(path, RootOrigin::DevelopmentFallback, &mut resolved);
        }
    }
    Ok(resolved)
}

/// The catalogs' view of [`resolve_roots`]: just the paths.
pub(crate) fn discovery_roots(db: &Database, capability: Capability) -> Result<Vec<PathBuf>> {
    Ok(resolve_roots(db, capability)?
        .into_iter()
        .map(|root| root.path)
        .collect())
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Roots too broad to be a project source.
///
/// Approving one of these would turn "this repository" into "every repository,
/// plus everything else under it", and every direct child would become a place
/// Workshop may discover executable declarations.
fn reject_broad_root(path: &Path) -> Result<()> {
    if path.parent().is_none() {
        bail!("the filesystem root cannot be a project source");
    }
    if let Some(home) = dirs::home_dir().and_then(|home| home.canonicalize().ok()) {
        if path == home {
            bail!(
                "the home folder is too broad to be a project source; choose the repository itself"
            );
        }
    }
    const TOO_BROAD: &[&str] = &[
        "/",
        "/Users",
        "/home",
        "/Applications",
        "/Library",
        "/System",
        "/Volumes",
        "/private",
        "/tmp",
        "/var",
        "/etc",
        "/usr",
        "/opt",
        "/bin",
        "/sbin",
    ];
    let display = path.to_string_lossy();
    if TOO_BROAD.iter().any(|broad| display == *broad) {
        bail!("{display} is too broad to be a project source; choose the repository itself");
    }
    Ok(())
}

/// Canonicalize and validate a candidate project source directory.
///
/// Canonicalization is the symlink defence: a path that escapes through a
/// symlink resolves to where it actually points, and that resolved path is
/// what gets compared against the user's picker selection and persisted.
pub fn canonical_project_root(raw: &str) -> Result<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("a project source path is required");
    }
    let path = Path::new(raw);
    if !path.is_absolute() {
        bail!("project source paths must be absolute");
    }
    let canonical = path
        .canonicalize()
        .with_context(|| format!("project folder does not exist: {raw}"))?;
    if !canonical.is_dir() {
        bail!("a project source must be a directory");
    }
    reject_broad_root(&canonical)?;
    Ok(canonical)
}

/// Read the declarations under `root` without executing anything.
///
/// Parsing is the point: a manifest that cannot be parsed is reported as
/// `invalid` with a code, rather than skipped the way broad discovery skips it.
/// "No project source" and "your manifest has a typo" are different problems
/// and used to produce the same empty list.
pub fn inspect(root: &Path) -> ProjectSourceInspection {
    let path = root.to_string_lossy().into_owned();
    let invalid = |code: &str, message: String| ProjectSourceInspection {
        path: path.clone(),
        status: "invalid".into(),
        code: Some(code.into()),
        message: Some(message),
        containers: Vec::new(),
        recipes: Vec::new(),
    };
    if !root.is_dir() {
        return ProjectSourceInspection {
            path: path.clone(),
            status: "missing".into(),
            code: Some("source_path_missing".into()),
            message: Some(format!("{path} is not a readable directory")),
            containers: Vec::new(),
            recipes: Vec::new(),
        };
    }

    let declares_containers = root.join(CONTAINERS_FILE).is_file();
    let declares_recipes = root.join(RECIPE_FILE).is_file() || root.join(RECIPES_DIR).is_dir();
    if !declares_containers && !declares_recipes {
        return ProjectSourceInspection {
            path: path.clone(),
            status: "invalid".into(),
            code: Some("no_declaration".into()),
            message: Some(format!(
                "{path} declares no {CONTAINERS_FILE}, {RECIPE_FILE}, or {RECIPES_DIR}/"
            )),
            containers: Vec::new(),
            recipes: Vec::new(),
        };
    }

    let containers = match crate::optimizers::workspace_recipe::load_container_specs(root) {
        Ok(specs) => specs.into_iter().map(|spec| spec.id).collect::<Vec<_>>(),
        Err(error) => {
            return invalid(
                "container_manifest_invalid",
                format!("{CONTAINERS_FILE} could not be parsed: {error}"),
            )
        }
    };
    let recipes = match crate::optimizers::workspace_recipe::load_recipes(root) {
        Ok(recipes) => recipes
            .into_iter()
            .map(|recipe| recipe.id)
            .collect::<Vec<_>>(),
        Err(error) => {
            return invalid(
                "recipe_manifest_invalid",
                format!("a recipe declaration could not be parsed: {error}"),
            )
        }
    };
    if containers.is_empty() && recipes.is_empty() {
        return invalid(
            "no_declaration",
            format!("{path} declares no container specs and no recipes"),
        );
    }
    ProjectSourceInspection {
        path,
        status: "valid".into(),
        code: None,
        message: None,
        containers,
        recipes,
    }
}

// ---------------------------------------------------------------------------
// Persisted catalog
// ---------------------------------------------------------------------------

fn last_scanned_at(db: &Database, path: &str) -> Option<String> {
    db.with_conn(|conn| {
        let container = conn
            .query_row(
                "SELECT updated_at FROM container_sources WHERE canonical_path=?1",
                [path],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let recipe = conn
            .query_row(
                "SELECT updated_at FROM optimizer_recipe_sources WHERE canonical_path=?1",
                [path],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(container.max(recipe))
    })
    .ok()
    .flatten()
}

fn row_for(db: &Database, entry: &ProjectSourceEntry, origin: RootOrigin) -> ProjectSourceRow {
    ProjectSourceRow {
        inspection: inspect(Path::new(&entry.path)),
        last_scanned_at: last_scanned_at(db, &entry.path),
        path: entry.path.clone(),
        containers: entry.containers,
        recipes: entry.recipes,
        origin,
    }
}

/// Everything Settings needs to show and manage project sources.
pub fn catalog(db: &Database) -> Result<ProjectSourceCatalog> {
    let settings = synth_config::project_source_settings()?;
    let persisted: HashSet<String> = settings
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    let sources = settings
        .entries
        .iter()
        .map(|entry| row_for(db, entry, RootOrigin::Configured))
        .collect();

    let mut implicit: Vec<ProjectSourceRow> = Vec::new();
    for capability in [Capability::Containers, Capability::Recipes] {
        for root in resolve_roots(db, capability)? {
            if root.origin == RootOrigin::Configured {
                continue;
            }
            let path = root.path.to_string_lossy().into_owned();
            if persisted.contains(&path) {
                continue;
            }
            if let Some(existing) = implicit.iter_mut().find(|row| row.path == path) {
                existing.containers |= capability == Capability::Containers;
                existing.recipes |= capability == Capability::Recipes;
                continue;
            }
            implicit.push(ProjectSourceRow {
                inspection: inspect(&root.path),
                last_scanned_at: last_scanned_at(db, &path),
                path,
                containers: capability == Capability::Containers,
                recipes: capability == Capability::Recipes,
                origin: root.origin,
            });
        }
    }
    Ok(ProjectSourceCatalog {
        config_path: settings.config_path,
        sources,
        implicit_roots: implicit,
    })
}

/// Persist one root, merging into any entry that already names it.
///
/// Re-approving an already-admitted source is idempotent: it widens the grant
/// if the new request asked for more, and changes nothing if it did not.
fn persist_root(
    canonical: &Path,
    containers: bool,
    recipes: bool,
) -> Result<ProjectSourceSettings> {
    synth_config::merge_project_source(ProjectSourceEntry {
        path: canonical.to_string_lossy().into_owned(),
        containers,
        recipes,
    })
}

/// Refresh both catalogs and re-persist their provenance rows.
///
/// Called after every admission and removal so the agent's next `discover`
/// reflects the change without a restart.
pub async fn refresh(db: &Arc<Database>) -> Result<()> {
    crate::optimizers::container_catalog::discover(db).await?;
    // Recipe discovery stays session/workspace-scoped on this trunk; container
    // sources are the only desktop-level provenance refreshed here.
    Ok(())
}

/// Stop discovering a persisted project source.
///
/// Future execution is prevented immediately: the config entry goes away, and
/// the remembered-provenance rows that would otherwise re-admit the path are
/// dropped in the same operation. Historical run receipts and sealed artifacts
/// are untouched -- they record what happened, and removing a source does not
/// make that untrue.
pub async fn remove(db: &Arc<Database>, raw: &str) -> Result<ProjectSourceCatalog> {
    let path = raw.trim().to_owned();
    if path.is_empty() {
        bail!("a project source path is required");
    }
    // Deliberately not canonicalized: a source whose folder has since been
    // deleted or moved must still be removable from Settings.
    synth_config::forget_project_source(&path)?;
    let forget = path.clone();
    db.clone()
        .run_transaction(move |conn| {
            conn.execute(
                "DELETE FROM container_sources WHERE canonical_path=?1",
                [&forget],
            )?;
            conn.execute(
                "DELETE FROM optimizer_recipe_sources WHERE canonical_path=?1",
                [&forget],
            )?;
            crate::storage::append_event(
                conn,
                EventAppend::system(
                    "project_source.removed",
                    serde_json::json!({ "path": forget }),
                ),
            )?;
            Ok(())
        })
        .await?;
    refresh(db).await?;
    catalog(db)
}

/// Add a project source the user chose directly in Settings.
///
/// Same validation as the agent-request path: an operator picking a folder and
/// an agent asking for one converge on the same checks.
pub async fn add_from_picker(
    db: &Arc<Database>,
    raw: &str,
    containers: bool,
    recipes: bool,
) -> Result<ProjectSourceCatalog> {
    if !containers && !recipes {
        bail!("a project source must enable containers, recipes, or both");
    }
    let canonical = canonical_project_root(raw)?;
    let inspection = inspect(&canonical);
    if !inspection.is_valid() {
        bail!(
            "{}",
            inspection
                .message
                .unwrap_or_else(|| "the folder declares no Workshop project files".into())
        );
    }
    persist_root(&canonical, containers, recipes)?;
    announce(db, &canonical, &inspection).await?;
    refresh(db).await?;
    catalog(db)
}

async fn announce(
    db: &Arc<Database>,
    canonical: &Path,
    inspection: &ProjectSourceInspection,
) -> Result<()> {
    let payload = serde_json::json!({
        "path": canonical.to_string_lossy(),
        "containers": inspection.containers,
        "recipes": inspection.recipes,
    });
    db.clone()
        .run_transaction(move |conn| {
            crate::storage::append_event(
                conn,
                EventAppend::system("project_source.approved", payload),
            )?;
            Ok(())
        })
        .await
}

// ---------------------------------------------------------------------------
// Agent requests
// ---------------------------------------------------------------------------

fn request_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectSourceRequest> {
    Ok(ProjectSourceRequest {
        id: row.get(0)?,
        session_id: row.get(1)?,
        requested_path: row.get(2)?,
        canonical_path: row.get(3)?,
        reason: row.get(4)?,
        containers: row.get::<_, i64>(5)? != 0,
        recipes: row.get::<_, i64>(6)? != 0,
        attach_to_conversation: row.get::<_, i64>(7)? != 0,
        status: row.get(8)?,
        created_at: row.get(9)?,
        resolved_at: row.get(10)?,
    })
}

const REQUEST_COLUMNS: &str = "id,session_id,requested_path,canonical_path,reason,containers,\
     recipes,attach_to_conversation,status,created_at,resolved_at";

fn load_request(conn: &rusqlite::Connection, id: &str) -> Result<Option<ProjectSourceRequest>> {
    Ok(conn
        .query_row(
            &format!("SELECT {REQUEST_COLUMNS} FROM project_source_requests WHERE id=?1"),
            [id],
            request_row,
        )
        .optional()?)
}

/// Record an agent's request to admit a folder as a project source.
///
/// This creates a pending row and nothing else. No configuration is written,
/// no catalog is refreshed, and nothing becomes discoverable or executable.
/// The path is canonicalized and inspected now only so the approval card can
/// tell the user what they would be admitting -- not to grant anything.
pub async fn request(
    db: &Arc<Database>,
    session_id: Option<&str>,
    raw_path: &str,
    reason: &str,
    containers: bool,
    recipes: bool,
    attach_to_conversation: bool,
) -> Result<ProjectSourceRequest> {
    if !containers && !recipes {
        bail!("a project source request must ask for containers, recipes, or both");
    }
    let reason = reason.trim();
    if reason.is_empty() {
        bail!("a project source request must include a reason");
    }
    if reason.len() > 2048 {
        bail!("a project source request reason must be under 2048 characters");
    }
    let canonical = canonical_project_root(raw_path)?;
    let id = uuid::Uuid::new_v4().to_string();
    let session_id = session_id.map(str::to_owned);
    let requested = raw_path.trim().to_owned();
    let canonical_path = canonical.to_string_lossy().into_owned();
    let reason = reason.to_owned();
    db.clone()
        .run_transaction(move |conn| {
            // One live request per folder per conversation. A retrying agent
            // should surface the request the user has not answered yet rather
            // than stack a second identical card on top of it.
            if let Some(existing) = conn
                .query_row(
                    &format!(
                        "SELECT {REQUEST_COLUMNS} FROM project_source_requests
                         WHERE canonical_path=?1 AND status='pending'
                           AND (session_id IS ?2 OR (session_id IS NULL AND ?2 IS NULL))
                         ORDER BY created_at DESC LIMIT 1"
                    ),
                    params![canonical_path, session_id],
                    request_row,
                )
                .optional()?
            {
                return Ok(existing);
            }
            conn.execute(
                "INSERT INTO project_source_requests(id,session_id,requested_path,canonical_path,
                    reason,containers,recipes,attach_to_conversation,status,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'pending',datetime('now'))",
                params![
                    id,
                    session_id,
                    requested,
                    canonical_path,
                    reason,
                    containers as i64,
                    recipes as i64,
                    attach_to_conversation as i64
                ],
            )?;
            load_request(conn, &id)?
                .ok_or_else(|| anyhow!("project source request was not created"))
        })
        .await
}

pub async fn list_requests(
    db: &Arc<Database>,
    session_id: Option<&str>,
) -> Result<Vec<ProjectSourceRequest>> {
    let session_id = session_id.map(str::to_owned);
    db.run(move |conn| {
        let (sql, bind): (String, Vec<Box<dyn rusqlite::ToSql>>) = match &session_id {
            Some(id) => (
                format!(
                    "SELECT {REQUEST_COLUMNS} FROM project_source_requests
                     WHERE session_id=?1 ORDER BY created_at DESC"
                ),
                vec![Box::new(id.clone())],
            ),
            None => (
                format!(
                    "SELECT {REQUEST_COLUMNS} FROM project_source_requests ORDER BY created_at DESC"
                ),
                Vec::new(),
            ),
        };
        let mut statement = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = bind.iter().map(|value| value.as_ref()).collect();
        let rows = statement
            .query_map(params.as_slice(), request_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .await
}

pub async fn deny(db: &Arc<Database>, request_id: &str) -> Result<ProjectSourceRequest> {
    let _resolution = REQUEST_RESOLUTION.lock().await;
    let id = request_id.to_owned();
    db.clone()
        .run_transaction(move |conn| {
            let changed = conn.execute(
                "UPDATE project_source_requests SET status='denied',resolved_at=datetime('now')
                 WHERE id=?1 AND status='pending'",
                [&id],
            )?;
            if changed == 0 {
                return Err(anyhow!("pending project source request was not found"));
            }
            load_request(conn, &id)?.ok_or_else(|| anyhow!("project source request disappeared"))
        })
        .await
}

/// Admit a requested folder after the user re-selected it in the native picker.
///
/// `confirmed_path` comes from the picker, not from the agent. It must
/// canonicalize to exactly the requested canonical path: an agent that names
/// one folder cannot have an approval land on a different one, and approving
/// a repository never approves its parent or its siblings.
///
/// Everything after that check happens as one admission: validate, persist,
/// optionally attach, refresh both catalogs, and emit the durable event the
/// requesting agent watches for.
pub async fn approve(
    db: &Arc<Database>,
    request_id: &str,
    confirmed_path: &str,
) -> Result<ProjectSourceApproval> {
    let _resolution = REQUEST_RESOLUTION.lock().await;
    let confirmed = canonical_project_root(confirmed_path)?;
    let id = request_id.to_owned();
    let confirmed_display = confirmed.to_string_lossy().into_owned();

    // Resolve and check the request before touching configuration.
    let request = db
        .clone()
        .run_transaction(move |conn| {
            let request = load_request(conn, &id)?
                .ok_or_else(|| anyhow!("project source request was not found"))?;
            if request.status != "pending" {
                return Err(anyhow!("project source request is no longer pending"));
            }
            if request.canonical_path != confirmed_display {
                return Err(anyhow!(
                    "the selected folder does not match the requested folder"
                ));
            }
            Ok(request)
        })
        .await?;

    // Validate the declarations before persistence, so a typo in a manifest
    // surfaces as an error on the approval card instead of as a recorded
    // permission that never resolves to anything.
    let inspection = inspect(&confirmed);
    if !inspection.is_valid() {
        bail!(
            "{}",
            inspection
                .message
                .clone()
                .unwrap_or_else(|| "the folder declares no Workshop project files".into())
        );
    }

    let previous = synth_config::project_source_roots()
        .into_iter()
        .find(|entry| entry.path == confirmed.to_string_lossy());
    if let Err(error) = persist_root(&confirmed, request.containers, request.recipes) {
        return Err(error);
    }

    let resolved = {
        let id = request.id.clone();
        db.clone()
            .run_transaction(move |conn| {
                let changed = conn.execute(
                    "UPDATE project_source_requests SET status='approved',resolved_at=datetime('now')
                     WHERE id=?1 AND status='pending'",
                    [&id],
                )?;
                if changed != 1 {
                    return Err(anyhow!("project source request approval was not committed"));
                }
                load_request(conn, &id)?
                    .ok_or_else(|| anyhow!("project source request disappeared"))
            })
            .await
    };
    let resolved = match resolved {
        Ok(resolved) => resolved,
        Err(error) => {
            // Restore the exact authority that preceded this attempt. This is
            // essential when re-approval was widening an existing grant.
            synth_config::forget_project_source(&confirmed.to_string_lossy())?;
            if let Some(previous) = previous {
                synth_config::merge_project_source(previous)?;
            }
            return Err(error);
        }
    };

    // Attaching is a convenience the request may ask for; it is a separate,
    // weaker grant and its failure must not undo the admission.
    let scope = match (
        request.attach_to_conversation,
        request.session_id.as_deref(),
    ) {
        (true, Some(session_id)) => crate::workspace_scope::attach(
            db,
            session_id,
            &confirmed.to_string_lossy(),
            crate::workspace_scope::WorkspaceAccessMode::ReadWrite,
            crate::workspace_scope::AttachmentSource::AgentRequest,
        )
        .await
        .map_err(|error| {
            eprintln!("synth-desktop: project source attach reported {error}");
            error
        })
        .ok(),
        _ => None,
    };

    announce(db, &confirmed, &inspection).await?;
    refresh(db).await?;
    let catalog = catalog(db)?;
    let source = catalog
        .sources
        .iter()
        .find(|row| row.path == confirmed.to_string_lossy())
        .cloned()
        .unwrap_or_else(|| ProjectSourceRow {
            path: confirmed.to_string_lossy().into_owned(),
            containers: request.containers,
            recipes: request.recipes,
            origin: RootOrigin::Configured,
            inspection,
            last_scanned_at: None,
        });
    Ok(ProjectSourceApproval {
        request: resolved,
        source,
        catalog,
        scope,
    })
}

// ---------------------------------------------------------------------------
// Discovery readiness
// ---------------------------------------------------------------------------

/// Why discovery returned nothing, in terms an agent can act on.
///
/// An empty `sources` array used to be the only signal, and it conflated "no
/// folder has ever been admitted" with "the one you admitted has a broken
/// manifest". The first is fixed by asking the user; the second is fixed by
/// editing a file, and asking the user again will never help.
pub fn readiness(db: &Database, capability: Capability, discovered: usize) -> serde_json::Value {
    if discovered > 0 {
        return serde_json::json!({ "status": "ready" });
    }
    let roots = resolve_roots(db, capability).unwrap_or_default();
    if roots.is_empty() {
        return serde_json::json!({
            "status": "blocked",
            "code": "no_project_sources",
            "retryable": true,
            "message": "No project source folders are configured. Ask the user to approve the exact repository folder that declares the container or recipe.",
            "nextAction": { "operation": "project_source_request" },
        });
    }
    let diagnostics: Vec<serde_json::Value> = roots
        .iter()
        .map(|root| inspect(&root.path))
        .filter(|inspection| !inspection.is_valid())
        .map(|inspection| {
            serde_json::json!({
                "path": inspection.path,
                "status": inspection.status,
                "code": inspection.code,
                "message": inspection.message,
            })
        })
        .collect();
    serde_json::json!({
        "status": "blocked",
        "code": if diagnostics.is_empty() { "no_declarations_found" } else { "project_sources_invalid" },
        "retryable": true,
        "message": if diagnostics.is_empty() {
            "Project source folders are configured but declare nothing Workshop can run.".to_string()
        } else {
            format!("{} configured project source(s) could not be read.", diagnostics.len())
        },
        "sourceDiagnostics": diagnostics,
        "nextAction": { "operation": "project_source_request" },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth_config::ProjectSourceUpdate;

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    fn containers_manifest() -> &'static str {
        r#"
[[container]]
id = "demo-service"
url = "http://127.0.0.1:9999"
locality = "container"
family = "demo"
[container.launch]
schema_version = "synth.container-launch.v1"
working_directory = "."
command = ["true"]
readiness_timeout_seconds = 30
shutdown_grace_seconds = 5
expected_port = 9999
image_ref = "fixture"
health_target = "fixture"
[container.launch.source]
revision_policy = "exact-or-dirty-digest"
tracked_revision = "fixture-revision"
include = ["workshop.containers.toml"]
"#
    }

    const ISOLATED_VARS: &[&str] = &[
        crate::instance::DATA_ROOT_ENV,
        "SYNTH_DESKTOP_CONFIG",
        "SYNTH_INTERN_CONFIG",
        CONTAINER_SOURCE_ROOTS_ENV,
        RECIPE_SOURCE_ROOTS_ENV,
        "SYNTH_WORKSHOP_ROOT",
        "HOME",
    ];

    struct Isolated {
        root: tempfile::TempDir,
        previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl Drop for Isolated {
        fn drop(&mut self) {
            for (name, value) in self.previous.drain(..) {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    impl Isolated {
        /// A private instance root, an empty `HOME` (so the development
        /// fallback has nothing to find), and no source-root environment.
        fn new() -> Self {
            let guard = crate::instance::environment_lock();
            let root = tempfile::tempdir().unwrap();
            let previous = ISOLATED_VARS
                .iter()
                .map(|name| (*name, std::env::var_os(name)))
                .collect();
            for name in ISOLATED_VARS {
                std::env::remove_var(name);
            }
            std::env::set_var(crate::instance::DATA_ROOT_ENV, root.path());
            std::env::set_var("HOME", root.path());
            Self {
                root,
                previous,
                _guard: guard,
            }
        }

        fn config(&self) -> PathBuf {
            self.root.path().join("config.toml")
        }

        /// A repository that declares one container spec.
        fn repository(&self, name: &str) -> PathBuf {
            let path = self.root.path().join(name);
            std::fs::create_dir_all(&path).unwrap();
            write(&path.join(CONTAINERS_FILE), containers_manifest());
            path.canonicalize().unwrap()
        }

        fn storage(&self) -> crate::storage::Storage {
            crate::storage::Storage::open(&self.root.path().join("state")).unwrap()
        }
    }

    fn approved_paths() -> Vec<String> {
        synth_config::project_source_settings()
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.path)
            .collect()
    }

    #[test]
    fn an_empty_path_list_is_not_a_configured_root() {
        assert!(split_path_list(std::ffi::OsStr::new("")).is_empty());
        assert!(split_path_list(std::ffi::OsStr::new(":")).is_empty());
        assert_eq!(
            split_path_list(std::ffi::OsStr::new("/a::/b")),
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    #[test]
    fn broad_and_malformed_roots_are_refused() {
        assert!(canonical_project_root("relative/path").is_err());
        assert!(canonical_project_root("").is_err());
        assert!(canonical_project_root("/definitely/not/here").is_err());
        assert!(canonical_project_root("/").is_err());
        let home = dirs::home_dir().unwrap();
        assert!(canonical_project_root(&home.to_string_lossy()).is_err());
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("file");
        write(&file, "x");
        assert!(canonical_project_root(&file.to_string_lossy()).is_err());
        assert!(canonical_project_root(&temp.path().to_string_lossy()).is_ok());
    }

    #[test]
    fn inspection_names_the_declarations_it_found() {
        let temp = tempfile::tempdir().unwrap();
        write(&temp.path().join(CONTAINERS_FILE), containers_manifest());
        let inspection = inspect(temp.path());
        assert_eq!(inspection.status, "valid", "{inspection:?}");
        assert_eq!(inspection.containers, vec!["demo-service".to_owned()]);
    }

    #[test]
    fn a_folder_without_declarations_is_invalid_not_empty() {
        let temp = tempfile::tempdir().unwrap();
        let inspection = inspect(temp.path());
        assert_eq!(inspection.status, "invalid");
        assert_eq!(inspection.code.as_deref(), Some("no_declaration"));
    }

    #[test]
    fn an_unparseable_manifest_reports_a_diagnostic_rather_than_vanishing() {
        let temp = tempfile::tempdir().unwrap();
        write(&temp.path().join(CONTAINERS_FILE), "this is not = [toml");
        let inspection = inspect(temp.path());
        assert_eq!(inspection.status, "invalid");
        assert_eq!(
            inspection.code.as_deref(),
            Some("container_manifest_invalid")
        );
        assert!(inspection
            .message
            .unwrap_or_default()
            .contains(CONTAINERS_FILE));
    }

    /// A declaration that points outside its own repository is a manifest
    /// error with a message, not a source that silently declares nothing.
    #[test]
    fn a_cwd_that_escapes_the_source_root_is_a_reported_manifest_error() {
        let temp = tempfile::tempdir().unwrap();
        write(
            &temp.path().join(CONTAINERS_FILE),
            r#"
[[container]]
id = "escaping"
url = "http://127.0.0.1:9999"
locality = "container"
[container.launch]
schema_version = "synth.container-launch.v1"
working_directory = ".."
command = ["true"]
readiness_timeout_seconds = 30
shutdown_grace_seconds = 5
expected_port = 9999
image_ref = "fixture"
health_target = "fixture"
[container.launch.source]
revision_policy = "exact-or-dirty-digest"
tracked_revision = "fixture-revision"
include = ["workshop.containers.toml"]
"#,
        );
        let inspection = inspect(temp.path());
        assert_eq!(inspection.status, "invalid");
        assert_eq!(
            inspection.code.as_deref(),
            Some("container_manifest_invalid")
        );
        assert!(
            inspection.message.unwrap_or_default().contains("outside"),
            "the diagnostic must say why the manifest was refused"
        );
    }

    #[test]
    fn a_missing_directory_is_reported_as_missing() {
        let inspection = inspect(Path::new("/definitely/not/here"));
        assert_eq!(inspection.status, "missing");
        assert_eq!(inspection.code.as_deref(), Some("source_path_missing"));
    }

    #[test]
    fn an_empty_environment_override_does_not_disable_configured_roots() {
        let isolated = Isolated::new();
        let repository = isolated.repository("nanohorizon");
        // Exactly the launcher's old behaviour: the variable is present and
        // empty. It used to mean "configured with zero roots".
        std::env::set_var(CONTAINER_SOURCE_ROOTS_ENV, "");
        synth_config::update_project_sources(ProjectSourceUpdate {
            entries: vec![ProjectSourceEntry {
                path: repository.to_string_lossy().into_owned(),
                containers: true,
                recipes: true,
            }],
        })
        .unwrap();
        let storage = isolated.storage();
        let roots = resolve_roots(storage.database(), Capability::Containers).unwrap();
        assert_eq!(
            roots,
            vec![ResolvedRoot {
                path: repository,
                origin: RootOrigin::Configured
            }]
        );
    }

    #[test]
    fn an_empty_environment_variable_falls_through_to_the_development_fallback() {
        let isolated = Isolated::new();
        let checkouts = isolated.root.path().join("GitHub");
        std::fs::create_dir_all(&checkouts).unwrap();
        std::env::set_var(CONTAINER_SOURCE_ROOTS_ENV, ":");
        let storage = isolated.storage();
        let roots = resolve_roots(storage.database(), Capability::Containers).unwrap();
        assert_eq!(
            roots,
            vec![ResolvedRoot {
                path: checkouts.canonicalize().unwrap(),
                origin: RootOrigin::DevelopmentFallback
            }]
        );
    }

    #[test]
    fn configured_and_environment_roots_merge_and_deduplicate() {
        let isolated = Isolated::new();
        let approved = isolated.repository("approved");
        let override_root = isolated.repository("from-ci");
        synth_config::update_project_sources(ProjectSourceUpdate {
            entries: vec![ProjectSourceEntry {
                path: approved.to_string_lossy().into_owned(),
                containers: true,
                recipes: true,
            }],
        })
        .unwrap();
        // The same approved root twice, plus a CI override. A symlink to an
        // already-listed root must not become a second root.
        let link = isolated.root.path().join("approved-link");
        std::os::unix::fs::symlink(&approved, &link).unwrap();
        std::env::set_var(
            CONTAINER_SOURCE_ROOTS_ENV,
            format!(
                "{}:{}:{}",
                override_root.display(),
                approved.display(),
                link.display()
            ),
        );
        let storage = isolated.storage();
        let roots = resolve_roots(storage.database(), Capability::Containers).unwrap();
        assert_eq!(
            roots,
            vec![
                ResolvedRoot {
                    path: approved,
                    origin: RootOrigin::Configured
                },
                ResolvedRoot {
                    path: override_root,
                    origin: RootOrigin::Environment
                }
            ],
            "an environment override must not drop the approved source"
        );
    }

    #[tokio::test]
    async fn discovery_provenance_does_not_outlive_temporary_authority() {
        let isolated = Isolated::new();
        let repository = isolated.repository("temporary");
        let storage = isolated.storage();
        std::env::set_var(CONTAINER_SOURCE_ROOTS_ENV, &repository);
        assert_eq!(
            crate::optimizers::container_catalog::discover(storage.database())
                .await
                .unwrap()
                .len(),
            1
        );

        std::env::remove_var(CONTAINER_SOURCE_ROOTS_ENV);
        assert!(
            resolve_roots(storage.database(), Capability::Containers)
                .unwrap()
                .is_empty(),
            "catalog provenance must not become execution authority"
        );
    }

    #[test]
    fn a_capability_flag_bounds_which_catalog_sees_the_root() {
        let isolated = Isolated::new();
        let repository = isolated.repository("containers-only");
        synth_config::update_project_sources(ProjectSourceUpdate {
            entries: vec![ProjectSourceEntry {
                path: repository.to_string_lossy().into_owned(),
                containers: true,
                recipes: false,
            }],
        })
        .unwrap();
        let storage = isolated.storage();
        assert_eq!(
            discovery_roots(storage.database(), Capability::Containers).unwrap(),
            vec![repository]
        );
        assert!(
            discovery_roots(storage.database(), Capability::Recipes)
                .unwrap()
                .is_empty(),
            "a containers-only grant must not admit the folder to the recipe catalog"
        );
    }

    #[tokio::test]
    async fn an_agent_request_creates_no_authority() {
        let isolated = Isolated::new();
        let repository = isolated.repository("nanohorizon");
        let storage = isolated.storage();
        let recorded = request(
            storage.database(),
            None,
            &repository.to_string_lossy(),
            "Discover the declared container.",
            true,
            true,
            false,
        )
        .await
        .unwrap();
        assert_eq!(recorded.status, "pending");
        assert_eq!(recorded.canonical_path, repository.to_string_lossy());
        assert!(
            approved_paths().is_empty(),
            "a request must not write configuration"
        );
        assert!(
            discovery_roots(storage.database(), Capability::Containers)
                .unwrap()
                .is_empty(),
            "a request must not make the folder discoverable"
        );
    }

    #[tokio::test]
    async fn a_repeated_request_reuses_the_pending_card() {
        let isolated = Isolated::new();
        let repository = isolated.repository("nanohorizon");
        let storage = isolated.storage();
        let path = repository.to_string_lossy().into_owned();
        let first = request(storage.database(), None, &path, "why", true, true, false)
            .await
            .unwrap();
        let second = request(
            storage.database(),
            None,
            &path,
            "why again",
            true,
            true,
            false,
        )
        .await
        .unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(
            second.reason, "why",
            "the answered-yet card is authoritative"
        );
    }

    #[tokio::test]
    async fn approval_requires_the_picker_to_confirm_the_same_folder() {
        let isolated = Isolated::new();
        let repository = isolated.repository("nanohorizon");
        let sibling = isolated.repository("other");
        let storage = isolated.storage();
        let recorded = request(
            storage.database(),
            None,
            &repository.to_string_lossy(),
            "why",
            true,
            true,
            false,
        )
        .await
        .unwrap();

        // A sibling repository is a real, valid folder -- it is refused purely
        // because it is not the one that was asked for.
        let error = approve(storage.database(), &recorded.id, &sibling.to_string_lossy())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("does not match"), "{error}");

        // Selecting the enclosing folder must never widen the grant. Here it
        // is refused as too broad before the match check even runs.
        let parent = isolated.root.path().canonicalize().unwrap();
        assert!(
            approve(storage.database(), &recorded.id, &parent.to_string_lossy())
                .await
                .is_err(),
            "approving the parent of the requested folder must fail"
        );

        assert!(
            approved_paths().is_empty(),
            "a mismatched selection must admit nothing"
        );
        // The request is still pending, so the user can answer it correctly.
        assert_eq!(
            list_requests(storage.database(), None).await.unwrap()[0].status,
            "pending"
        );
    }

    #[tokio::test]
    async fn approval_persists_the_canonical_path_and_makes_the_source_discoverable() {
        let isolated = Isolated::new();
        let repository = isolated.repository("nanohorizon");
        let storage = isolated.storage();
        let recorded = request(
            storage.database(),
            None,
            &repository.to_string_lossy(),
            "Discover the declared container.",
            true,
            true,
            false,
        )
        .await
        .unwrap();
        let approval = approve(
            storage.database(),
            &recorded.id,
            &repository.to_string_lossy(),
        )
        .await
        .unwrap();

        assert_eq!(approval.request.status, "approved");
        assert_eq!(approval.source.inspection.containers, vec!["demo-service"]);
        assert_eq!(approved_paths(), vec![repository.to_string_lossy()]);
        assert!(
            std::fs::read_to_string(isolated.config())
                .unwrap()
                .contains(&repository.to_string_lossy().into_owned()),
            "the canonical path belongs in config.toml, not only in the database"
        );
        let sources = crate::optimizers::container_catalog::discover(storage.database())
            .await
            .unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].specs[0].id, "demo-service");

        // Replaying the same approval must not create a second entry or fail.
        let replay = approve(
            storage.database(),
            &recorded.id,
            &repository.to_string_lossy(),
        )
        .await;
        assert!(replay.is_err(), "a resolved request is no longer pending");
        assert_eq!(approved_paths().len(), 1);
    }

    /// Two approvals resolved at the same moment must both be recorded.
    ///
    /// The hazard is a read-modify-write that reads outside the lock: both
    /// threads see one entry, and the loser writes a list that never contained
    /// the winner's grant.
    #[test]
    fn concurrent_admissions_cannot_clobber_each_other() {
        let isolated = Isolated::new();
        let first = isolated.repository("first");
        let second = isolated.repository("second");
        std::thread::scope(|scope| {
            for repository in [&first, &second] {
                scope.spawn(move || {
                    persist_root(repository, true, true).unwrap();
                });
            }
        });
        let mut recorded = approved_paths();
        recorded.sort();
        let mut expected = vec![
            first.to_string_lossy().into_owned(),
            second.to_string_lossy().into_owned(),
        ];
        expected.sort();
        assert_eq!(recorded, expected);
    }

    #[tokio::test]
    async fn denial_changes_no_configuration() {
        let isolated = Isolated::new();
        let repository = isolated.repository("nanohorizon");
        let storage = isolated.storage();
        let recorded = request(
            storage.database(),
            None,
            &repository.to_string_lossy(),
            "why",
            true,
            true,
            false,
        )
        .await
        .unwrap();
        let denied = deny(storage.database(), &recorded.id).await.unwrap();
        assert_eq!(denied.status, "denied");
        assert!(approved_paths().is_empty());
        assert!(deny(storage.database(), &recorded.id).await.is_err());
    }

    #[tokio::test]
    async fn denial_cannot_override_a_completed_approval() {
        let isolated = Isolated::new();
        let repository = isolated.repository("nanohorizon");
        let storage = isolated.storage();
        let recorded = request(
            storage.database(),
            None,
            &repository.to_string_lossy(),
            "why",
            true,
            true,
            false,
        )
        .await
        .unwrap();
        approve(
            storage.database(),
            &recorded.id,
            &repository.to_string_lossy(),
        )
        .await
        .unwrap();
        assert!(deny(storage.database(), &recorded.id).await.is_err());
        assert_eq!(approved_paths().len(), 1);
    }

    #[tokio::test]
    async fn a_removed_source_stops_being_discoverable_immediately() {
        let isolated = Isolated::new();
        let repository = isolated.repository("nanohorizon");
        let storage = isolated.storage();
        add_from_picker(
            storage.database(),
            &repository.to_string_lossy(),
            true,
            true,
        )
        .await
        .unwrap();
        assert_eq!(
            crate::optimizers::container_catalog::discover(storage.database())
                .await
                .unwrap()
                .len(),
            1
        );

        let after = remove(storage.database(), &repository.to_string_lossy())
            .await
            .unwrap();
        assert!(after.sources.is_empty());
        assert!(
            crate::optimizers::container_catalog::discover(storage.database())
                .await
                .unwrap()
                .is_empty(),
            "removal must also drop the remembered row that would re-admit it"
        );
    }

    #[tokio::test]
    async fn a_folder_without_declarations_is_refused_before_persistence() {
        let isolated = Isolated::new();
        let empty = isolated.root.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let storage = isolated.storage();
        let error = add_from_picker(storage.database(), &empty.to_string_lossy(), true, true)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("declares no"), "{error}");
        assert!(approved_paths().is_empty());
    }

    #[tokio::test]
    async fn discovery_readiness_tells_a_missing_source_from_a_broken_manifest() {
        let isolated = Isolated::new();
        let storage = isolated.storage();
        let blocked = readiness(storage.database(), Capability::Containers, 0);
        assert_eq!(blocked["code"], "no_project_sources");
        assert_eq!(blocked["nextAction"]["operation"], "project_source_request");

        let broken = isolated.root.path().join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        write(&broken.join(CONTAINERS_FILE), "not = [valid");
        synth_config::update_project_sources(ProjectSourceUpdate {
            entries: vec![ProjectSourceEntry {
                path: broken
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                containers: true,
                recipes: true,
            }],
        })
        .unwrap();
        let invalid = readiness(storage.database(), Capability::Containers, 0);
        assert_eq!(invalid["code"], "project_sources_invalid");
        assert_eq!(
            invalid["sourceDiagnostics"][0]["code"],
            "container_manifest_invalid"
        );
        assert_eq!(
            readiness(storage.database(), Capability::Containers, 2)["status"],
            "ready"
        );
    }
}
