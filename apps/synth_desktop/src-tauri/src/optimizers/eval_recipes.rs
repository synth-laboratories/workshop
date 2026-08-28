//! Local `eval` recipes: preflight, launch, ingest, cancel.
//!
//! `eval` is a local algorithm. It is never advertised as hosted, and the
//! Rust side never chooses an image, a seed, a metric, or a selection rule —
//! those are recipe data owned by the trusted Optimizers catalog. Workshop
//! contributes the app-owned runtime, an immutable candidate set, and the
//! projection an agent watches.
//!
//! Configuration lives in TOML under the app's own state root, never in
//! environment variables, so a run's settings can be read back afterwards:
//!
//! ```toml
//! # <state root>/optimizers/eval.toml
//! python = "/opt/synth/runtime/bin/python3"   # optional; app runtime otherwise
//! ```

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::{Duration, Instant},
};

fn eval_cli_path(inherited: Option<&OsStr>) -> Result<OsString> {
    let mut entries = vec![
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
    ];
    if let Some(inherited) = inherited {
        entries.extend(std::env::split_paths(inherited));
    }
    std::env::join_paths(entries).context("compose eval CLI PATH")
}
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::watch,
    time::sleep,
};

use super::eval_runtime::{fault as eval_fault, EvalRuntimeFault};
use super::events::OptimizerEventDraft;
use super::{
    models::{
        OptimizerCapabilities, OptimizerCreateRequest, OptimizerEventEnvelope,
        OptimizerExecutionBinding, OptimizerRecipeRunRequest, OptimizerResourceRef,
        OptimizerRunRecord, OPTIMIZER_EVENT_SCHEMA_VERSION,
    },
    OptimizerService,
};

pub const EVAL_ALGORITHM_ID: &str = "eval";

pub const EVAL_CRAFTAX_SMOKE_RECIPE: &str = "eval.craftax.code-policy.smoke.v1";
pub const EVAL_GAMEBENCH_CONFIRM_RECIPE: &str = "eval.gamebench.craftax-code-policy.confirm.v1";
pub const EVAL_CRAFTAX_MLX_LOCAL_RECIPE: &str = "eval.craftax.mlx-local-policy.smoke.v1";
pub const EVAL_GAMEBENCH_LLM_RECIPE: &str = "eval.gamebench.llm-policy.confirm.v1";
pub const EVAL_MLX_LOCAL_RECIPE: &str = "eval.mlx.local-policy.smoke.v1";


/// The product contract for the report-only Craftax smoke recipes is fixed per
/// staged candidate. Older local runtime catalogs omitted `limits.trials`,
/// even though the worker recipes themselves were fixed-cardinality. Keep the
/// authority here until every supported runtime publishes the field itself;
/// this is a compatibility projection, not an agent-selected limit.
const CRAFTAX_CODE_SMOKE_TRIALS_PER_CANDIDATE: u64 = 10;
const CRAFTAX_LLM_SMOKE_TRIALS_PER_CANDIDATE: u64 = 2;

/// The allowlist the MCP schema publishes. A recipe id outside it never
/// reaches the worker.
pub const EVAL_RECIPE_IDS: [&str; 5] = [
    EVAL_CRAFTAX_SMOKE_RECIPE,
    EVAL_GAMEBENCH_CONFIRM_RECIPE,
    EVAL_CRAFTAX_MLX_LOCAL_RECIPE,
    EVAL_GAMEBENCH_LLM_RECIPE,
    EVAL_MLX_LOCAL_RECIPE,
];

/// How long a cancelled worker gets to stop its containers, release its
/// semaphore leases, and seal evidence before it is killed outright.
const CANCEL_GRACE: Duration = Duration::from_secs(30);
/// Sentinels the worker watches. Pausing stops it dispatching new trials;
/// trials already in flight finish and seal rather than being abandoned.
const PAUSE_SENTINEL: &str = "PAUSE";
const PREFLIGHT_TTL: Duration = Duration::from_secs(20);

pub fn is_eval_recipe(recipe_id: &str) -> bool {
    EVAL_RECIPE_IDS.contains(&recipe_id)
}

pub fn eval_home() -> PathBuf {
    crate::instance::data_root().join("optimizers").join("eval")
}

fn config_path() -> PathBuf {
    crate::instance::state_root()
        .join("optimizers")
        .join("eval.toml")
}

fn selected_runtime_python() -> Option<PathBuf> {
    let optimizers_root = crate::instance::data_root().join("optimizers");
    let selected = fs::read_to_string(optimizers_root.join("selected_version")).ok()?;
    let selected = selected.trim();
    if selected.is_empty()
        || !selected.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
    {
        return None;
    }
    ["python3", "python"]
        .into_iter()
        .map(|executable| {
            optimizers_root
                .join("versions")
                .join(selected)
                .join("runtime/bin")
                .join(executable)
        })
        .find(|path| path.is_file())
}

/// The app owns the Optimizers runtime. There is deliberately no ambient
/// `SYNTH_PYTHON` fallback: an interpreter that happens to be on the operator's
/// PATH is not the one this feature was packaged against.
fn resolve_python() -> Result<PathBuf> {
    resolve_python_checked().map_err(anyhow::Error::from)
}

/// The Eval interpreter, or a structured reason there is none.
///
/// Every branch that used to swallow its error now names it. The lazy
/// provisioning attempt in particular was `let _ = provision_from_disk();`,
/// which meant a runtime that was installed but had a broken import, a stale
/// digest, or a half-extracted interpreter all arrived at the same closing
/// `bail!` -- "the local Optimizers runtime is not installed" -- and sent the
/// operator to reinstall something that was already there.
fn resolve_python_checked() -> std::result::Result<PathBuf, EvalRuntimeFault> {
    // Developer/QA builds stage one reviewed Optimizers checkout. Eval and
    // GEPA must execute that same runtime authority; falling through to the
    // previously selected installed version makes the catalog and worker
    // silently disagree (for example, 2 stale Craftax trials instead of the
    // staged digest-pinned 10-trial contract).
    match super::manager::optimizer_project_root() {
        Ok(Some(project)) => {
            return resolve_developer_python(&project).map_err(|error| {
                EvalRuntimeFault::new(eval_fault::INTERPRETER_MISSING, error.to_string())
            })
        }
        Ok(None) => {}
        Err(error) => {
            return Err(EvalRuntimeFault::new(
                eval_fault::PLUGIN_NOT_INSTALLED,
                error.to_string(),
            ))
        }
    }
    // Keep the desktop-owned pin's reason. The layouts below are compatibility
    // fallbacks; if none of them resolves, this is the honest answer.
    let pinned = match super::eval_runtime::ready_python() {
        Ok(python) => return Ok(python),
        Err(fault) => fault,
    };
    if let Ok(text) = fs::read_to_string(config_path()) {
        if let Some(configured) = text
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("python"))
            .find_map(|line| line.split_once('=').map(|(_, value)| value.trim()))
        {
            let path = PathBuf::from(configured.trim_matches(['"', '\'']));
            if path.is_file() {
                return Ok(path);
            }
            return Err(EvalRuntimeFault::new(
                eval_fault::INTERPRETER_MISSING,
                format!(
                    "{} sets python = {} but that interpreter does not exist",
                    config_path().display(),
                    path.display()
                ),
            ));
        }
    }
    // The plugin installer stores immutable versioned runtimes and records
    // the active selection. Eval must consume the same selected runtime as the
    // sidecar instead of looking only at the obsolete unversioned layout.
    if let Some(python) = selected_runtime_python() {
        return Ok(python);
    }
    let owned = crate::instance::data_root()
        .join("runtime")
        .join("optimizers")
        .join("bin")
        .join("python3");
    if owned.is_file() {
        return Ok(owned);
    }
    // Only now is "not installed" the right thing to say, and only when that
    // is what the pin actually reported.
    if pinned.code == eval_fault::PLUGIN_NOT_INSTALLED {
        return Err(EvalRuntimeFault::new(
            eval_fault::PLUGIN_NOT_INSTALLED,
            format!(
                "the local Optimizers runtime is not installed; install it under {} \
                 or set python = \"…\" in {}",
                owned.display(),
                config_path().display()
            ),
        ));
    }
    Err(pinned)
}

fn resolve_developer_python(project: &Path) -> Result<PathBuf> {
    resolve_developer_python_with_fallback(project, selected_runtime_python())
}

fn resolve_developer_python_with_fallback(
    project: &Path,
    fallback: Option<PathBuf>,
) -> Result<PathBuf> {
    for candidate in [
        project.join(".venv/bin/python"),
        project.join(".venv/Scripts/python.exe"),
    ] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    fallback.ok_or_else(|| {
        anyhow!(
            "developer optimizer project {} has no prepared .venv Python and no selected installed runtime",
            project.display()
        )
    })
}

fn run_cli(python: &Path, args: &[&str]) -> Result<Value> {
    let mut command = std::process::Command::new(python);
    command.arg("-m").arg("synth_optimizers.eval").args(args);
    if let Some(project) = super::manager::optimizer_project_root()? {
        command.env("PYTHONPATH", project.join("src"));
    }
    let output = command
        // Finder launches do not inherit the operator's shell PATH. Docker is
        // commonly installed by OrbStack in /usr/local/bin or Homebrew in
        // /opt/homebrew/bin; the eval producer must see that supported runtime.
        .env("PATH", eval_cli_path(std::env::var_os("PATH").as_deref())?)
        .stdin(Stdio::null())
        .output()
        .context("run the local eval CLI")?;
    decode_cli_output(
        output.status.success(),
        &output.status.to_string(),
        &output.stdout,
        &output.stderr,
    )
}

fn decode_cli_output(success: bool, status: &str, stdout: &[u8], stderr: &[u8]) -> Result<Value> {
    // Parse the complete producer payload. Bounding before parsing turns every
    // valid catalog larger than the diagnostic limit into fabricated malformed
    // JSON (the Craftax catalog is about 12 KiB).
    let stdout_text = String::from_utf8_lossy(stdout);
    let parsed: Value = serde_json::from_str(stdout_text.trim()).map_err(|error| {
        let stdout = bounded_cli_text(stdout);
        let stderr = bounded_cli_text(stderr);
        anyhow!(
            "eval_cli_unparseable_stdout: status={status}; parse={error}; stdout={stdout:?}; stderr={stderr:?}"
        )
    })?;
    let stdout = bounded_cli_text(stdout);
    let stderr = bounded_cli_text(stderr);
    if parsed.get("ready").and_then(Value::as_bool) == Some(false) {
        bail!(
            "eval_cli_not_ready: valid readiness report from {status}; stdout={stdout:?}; stderr={stderr:?}"
        );
    }
    if !success {
        bail!("eval_cli_non_zero_exit: status={status}; stdout={stdout:?}; stderr={stderr:?}");
    }
    Ok(parsed)
}

fn bounded_cli_text(bytes: &[u8]) -> String {
    const MAX_CHARS: usize = 2_000;
    let text = String::from_utf8_lossy(bytes);
    let mut bounded = text.chars().take(MAX_CHARS).collect::<String>();
    if text.chars().count() > MAX_CHARS {
        bounded.push_str("… (truncated)");
    }
    bounded
}

/// One cached preflight report and when it was taken.
type PreflightCache = Mutex<Option<(Instant, Result<Value, String>)>>;

fn preflight_cache() -> &'static PreflightCache {
    static CACHE: std::sync::OnceLock<PreflightCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Runtime + OCI runtime + catalog, as the worker itself sees them. Cached
/// briefly so listing algorithms does not spawn a process per keystroke.
pub fn preflight() -> Result<Value, String> {
    if let Ok(guard) = preflight_cache().lock() {
        if let Some((at, cached)) = guard.as_ref() {
            if at.elapsed() < PREFLIGHT_TTL {
                return cached.clone();
            }
        }
    }
    let home = eval_home();
    let result = (|| -> Result<Value> {
        let python = resolve_python()?;
        fs::create_dir_all(&home).ok();
        let home_arg = home.to_string_lossy().into_owned();
        run_cli(&python, &["doctor", "--home", &home_arg, "--json"])
    })()
    .map_err(|error| error.to_string());
    if let Ok(mut guard) = preflight_cache().lock() {
        *guard = Some((Instant::now(), result.clone()));
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalPinnedTargetPolicy {
    pub supported: bool,
    pub enabled: bool,
    pub source: &'static str,
}

pub(crate) fn local_pinned_target_policy() -> LocalPinnedTargetPolicy {
    if !cfg!(debug_assertions) {
        return LocalPinnedTargetPolicy {
            supported: false,
            enabled: false,
            source: "build_default",
        };
    }
    let path = crate::instance::data_root().join("eval-admission.toml");
    let configured = fs::read_to_string(path)
        .ok()
        .and_then(|text| text.parse::<toml::Value>().ok())
        .and_then(|value| {
            value
                .get("target_admission")
                .and_then(|value| value.get("local_pinned_digest"))
                .and_then(|value| value.get("enabled"))
                .and_then(toml::Value::as_bool)
        });
    if let Some(value) = std::env::var_os("SYNTH_EVAL_ALLOW_LOCAL_PINNED_TARGETS") {
        let enabled = value == "1";
        if configured != Some(enabled) {
            return LocalPinnedTargetPolicy {
                supported: true,
                enabled,
                source: "environment_override",
            };
        }
    }
    LocalPinnedTargetPolicy {
        supported: true,
        enabled: configured.unwrap_or(false),
        source: if configured.is_some() {
            "instance_config"
        } else {
            "build_default"
        },
    }
}

pub(crate) fn execution_capability_projection() -> Value {
    let local = local_pinned_target_policy();
    json!({
        "recipe_evaluation": { "supported": true },
        "target_admission": {
            "registry_digest": { "supported": true, "enabled": true },
            "local_pinned_digest": { "supported": local.supported, "enabled": local.enabled, "source": local.source }
        }
    })
}

pub fn recipe_catalog() -> Vec<Value> {
    let python = match resolve_python_checked() {
        Ok(python) => python,
        Err(fault) => return offline_catalog(fault.code, &fault.message),
    };
    let home = eval_home().to_string_lossy().into_owned();
    match run_cli(&python, &["recipes", "--home", &home, "--json"]) {
        Ok(payload) => payload
            .get("recipes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|recipe| {
                recipe
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(is_eval_recipe)
            })
            .map(normalize_builtin_recipe_contract)
            .collect(),
        // The runtime resolved and ran; a failure here is the CLI's, not a
        // missing install, and must not be reported as one.
        Err(error) => offline_catalog("eval_cli_failed", &error.to_string()),
    }
}

fn normalize_builtin_recipe_contract(mut recipe: Value) -> Value {
    let producer_ready = recipe.get("availability").and_then(Value::as_str) == Some("available");
    mark_unreproducible_target_unavailable(&mut recipe);
    project_eval_recipe_state(&mut recipe, producer_ready);
    let Some((trials, authority)) = (match recipe.get("id").and_then(Value::as_str) {
        Some(EVAL_CRAFTAX_SMOKE_RECIPE) => Some((
            CRAFTAX_CODE_SMOKE_TRIALS_PER_CANDIDATE,
            "workshop.builtin.eval.craftax.code-policy.smoke.v1",
        )),
        Some(EVAL_CRAFTAX_LLM_RECIPE) => Some((
            CRAFTAX_LLM_SMOKE_TRIALS_PER_CANDIDATE,
            "workshop.builtin.eval.craftax.llm-policy.smoke.v1",
        )),
        _ => None,
    }) else {
        return recipe;
    };
    if recipe.pointer("/limits/trials").is_some() {
        return recipe;
    }
    let Some(object) = recipe.as_object_mut() else {
        return recipe;
    };
    let limits = object
        .entry("limits")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(limits) = limits.as_object_mut() {
        limits.insert("trials".into(), json!(trials));
        limits.insert("trialAuthority".into(), json!(authority));
    }
    recipe
}

fn project_eval_recipe_state(recipe: &mut Value, producer_ready: bool) {
    let Some(object) = recipe.as_object_mut() else {
        return;
    };
    object.insert("executionKind".into(), json!("evaluation"));
    object.insert("recipeDiscovered".into(), json!(true));
    object.insert("executionSupported".into(), json!(true));
    object.insert("targetPresent".into(), json!(producer_ready));
    object.insert("targetDigestMatches".into(), json!(producer_ready));
    let available = object.get("availability").and_then(Value::as_str) == Some("available");
    object.insert("targetAdmitted".into(), json!(available));
    if let Some(reason) = object.get("availabilityReason").and_then(Value::as_str) {
        if let Ok(error) = serde_json::from_str::<Value>(reason) {
            object.insert("admissionError".into(), error);
        }
    }
}

/// A valid digest is not enough when it names only a local daemon image. Mark
/// that catalog entry unavailable before it reaches the UI so developers cannot
/// start a run whose identity depends on a mutable local retag.
fn mark_unreproducible_target_unavailable(recipe: &mut Value) {
    let Some(recipe_id) = recipe.get("id").and_then(Value::as_str) else {
        return;
    };
    let Err(error) = require_digest_pinned_target(recipe, recipe_id) else {
        return;
    };
    let Some(object) = recipe.as_object_mut() else {
        return;
    };
    object.insert("availability".into(), json!("unavailable"));
    object.insert("availabilityReason".into(), json!(error.to_string()));
}

/// Resolve the candidate-set id an eval launch will actually score.
/// A training artifact is staged first so paid-compute approval sees the same
/// set the worker will run.
pub(crate) fn resolve_eval_candidate_set(request: &OptimizerRecipeRunRequest) -> Result<String> {
    if request.training_artifact_id.is_some() && request.candidate_set_id.is_some() {
        bail!("eval recipes take either training_artifact_id or candidate_set_id, not both");
    }
    if let Some(artifact_id) = request
        .training_artifact_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let artifact = crate::training_artifacts::get(artifact_id)?;
        let staged = super::eval_candidates::stage_training_artifact(&artifact)?;
        return staged
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("staged candidate set omitted id"));
    }
    request
        .candidate_set_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow!("eval recipes require a staged candidate_set_id or a training_artifact_id")
        })
}

/// Convert the eval runtime's per-trial budget into the total product approval
/// bound for this immutable candidate set. The worker continues to enforce the
/// recipe-owned per-trial cap; this aggregate is the maximum exposure the
/// Workshop approval surface must display and retain.
pub(crate) fn paid_compute_bounds(
    recipe: &Value,
    candidate_set_id: Option<&str>,
) -> Result<(f64, u64)> {
    // Admission may receive the projected host catalog rather than the raw
    // runtime catalog. Apply the same built-in compatibility contract here so
    // the Craftax smoke cannot advertise ten trials and then fail moments later
    // as if that bound were absent.
    let normalized_recipe = normalize_builtin_recipe_contract(recipe.clone());
    let candidate_set_id = candidate_set_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("paid eval recipes require a staged candidate_set_id"))?;
    let candidate_set = super::eval_candidates::load(candidate_set_id)?;
    let candidate_count = candidate_set
        .get("candidates")
        .and_then(Value::as_array)
        .map(|candidates| candidates.len() as u64)
        .filter(|count| *count > 0)
        .ok_or_else(|| anyhow!("staged candidate set has no candidates"))?;
    paid_compute_bounds_for_candidate_count(&normalized_recipe, candidate_count)
}

fn paid_compute_bounds_for_candidate_count(
    recipe: &Value,
    candidate_count: u64,
) -> Result<(f64, u64)> {
    // Only a recipe that publishes a model allowlist can reach a paid provider.
    // A code-policy recipe runs the candidate's own code in a network-less
    // container, so demanding a dollar budget from it would block a run that
    // cannot spend a cent — and the trial count is then the bound that means
    // something. A recipe that *does* declare models still fails closed.
    let declares_paid_models = recipe
        .get("models")
        .and_then(Value::as_array)
        .is_some_and(|models| !models.is_empty());
    let per_trial_usd = if declares_paid_models {
        recipe
            .pointer("/budget/max_usd")
            .or_else(|| recipe.pointer("/budget/maxUsd"))
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or_else(|| anyhow!("eval recipe is missing budget.max_usd"))?
    } else {
        0.0
    };
    let trials_per_candidate = recipe
        .pointer("/limits/trials")
        .and_then(Value::as_u64)
        .filter(|count| *count > 0)
        .ok_or_else(|| anyhow!("eval recipe is missing limits.trials"))?;
    let total_trials = trials_per_candidate
        .checked_mul(candidate_count)
        .ok_or_else(|| anyhow!("eval trial count overflow"))?;
    let total_usd = per_trial_usd * total_trials as f64;
    if !total_usd.is_finite() || total_usd < 0.0 {
        bail!("eval paid-compute cap is invalid");
    }
    Ok((total_usd, total_trials))
}

fn secrets_proxy_error(code: &str, message: &str) -> anyhow::Error {
    anyhow!(
        "{}",
        json!({
            "code": code,
            "contract": "workshop.secrets_proxy",
            "retryable": code == "secrets_proxy_unavailable",
            "message": message,
        })
    )
}

/// Recipe-owned capability ceilings. The candidate never chooses the route,
/// model allowlist, or spend bound.
fn policy_from_eval_recipe(
    recipe: &Value,
    candidate_count: u64,
) -> Result<crate::secrets::SecretsUsePolicy> {
    let mut models = recipe
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| {
                    model
                        .get("id")
                        .and_then(Value::as_str)
                        .or_else(|| model.as_str())
                        .map(str::to_owned)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if models.is_empty() {
        return Err(secrets_proxy_error(
            "secrets_proxy_denied",
            "paid eval recipe is missing a model allowlist",
        ));
    }
    for model in models.clone() {
        if let Some(unqualified) = model.strip_prefix("openai/") {
            if !models.iter().any(|candidate| candidate == unqualified) {
                models.push(unqualified.to_string());
            }
        }
    }
    let (max_usd, max_trials) =
        paid_compute_bounds_for_candidate_count(recipe, candidate_count.max(1))?;
    let mut reasoning_efforts = Vec::new();
    if let Some(effort) = recipe
        .pointer("/policy/reasoning_effort")
        .or_else(|| recipe.pointer("/policy/effort"))
        .and_then(Value::as_str)
    {
        reasoning_efforts.push(effort.to_string());
    }
    Ok(super::admission::provider_use_policy_from_bounds(
        vec!["chat.completions.create".into(), "responses.create".into()],
        models,
        reasoning_efforts,
        max_trials.saturating_mul(16).clamp(40, u32::MAX as u64) as u32,
        (max_usd.max(0.01) * 1_000_000.0).round().max(0.0) as u64,
        crate::limits::SECRETS_CAPABILITY_TTL.as_secs(),
        None,
        None,
    ))
}

/// Trusted route binding. `synth-optimizers` must copy `provider_routes.openai`
/// into the trial container as `EVAL_LLM_ROUTE` / `WORKSHOP_OPENAI_ROUTE` and
/// must not let the recipe or candidate replace it with `api.openai.com`.
pub(crate) fn bind_provider_routes_into_manifest(path: &Path, routes: Value) -> Result<()> {
    let mut manifest: Value = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read eval worker manifest {}", path.display()))?,
    )
    .context("parse eval worker manifest")?;
    let Some(object) = manifest.as_object_mut() else {
        return Err(secrets_proxy_error(
            "secrets_proxy_route_unbound",
            "eval worker manifest is not an object",
        ));
    };
    object.insert("credential_mode".into(), json!("workshop_proxy"));
    object.insert(
        "inference_url".into(),
        routes.get("openai_base").cloned().unwrap_or(Value::Null),
    );
    let mut bound_routes = routes.clone();
    if let Some(object) = bound_routes.as_object_mut() {
        object
            .entry("extra_hosts".to_string())
            .or_insert_with(|| json!(["host.docker.internal:host-gateway"]));
        object
            .entry("api_key_sentinel".to_string())
            .or_insert_with(|| json!(crate::secrets::API_KEY_SENTINEL));
    }
    object.insert("provider_routes".into(), bound_routes);
    fs::write(path, serde_json::to_vec_pretty(&manifest)?)
        .context("write eval worker provider_routes")?;
    Ok(())
}

/// The catalog is still worth showing when the runtime is missing: an empty
/// list reads as "this product does not exist", which is the wrong answer.
/// Refuse a target that is not pinned to an immutable digest.
///
/// A recipe's honesty about *which* image it scored against is the whole basis
/// for comparing two runs. A mutable tag can be repointed between them, and a
/// local-checkout fallback is not reproducible on any other machine — both turn
/// a benchmark number into an anecdote. Craftax eval is blocked on publishing a
/// real image precisely so this can be enforced rather than worked around.
///
/// A recipe that declares no image has nothing to pin (the fixture smoke is
/// deterministic and benchmark-free); a recipe that declares one must pin it.
fn require_digest_pinned_target(recipe: &Value, recipe_id: &str) -> Result<()> {
    let allow_local_pinned_target = local_pinned_target_policy().enabled;
    require_digest_pinned_target_with_policy(recipe, recipe_id, allow_local_pinned_target)
}

fn require_digest_pinned_target_with_policy(
    recipe: &Value,
    recipe_id: &str,
    allow_local_pinned_target: bool,
) -> Result<()> {
    let image = recipe
        .get("image")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|image| !image.is_empty());
    let Some(image) = image else {
        return Ok(());
    };
    let refuse = |code: &str, reason: &str| -> anyhow::Error {
        anyhow!(
            "{}",
            json!({
                "code": code,
                "contract": "workflow.immutable_target",
                "owner": recipe_id,
                "retryable": false,
                "requestedRecipeId": recipe_id,
                "substitutionAllowed": false,
                "image": image,
                "evaluation_supported": true,
                "local_pinned_target_supported": cfg!(debug_assertions),
                "local_pinned_target_enabled": allow_local_pinned_target,
                "message": reason,
            })
        )
    };

    // A path or file URL is a local checkout, not a published image.
    if image.starts_with('/')
        || image.starts_with("./")
        || image.starts_with("../")
        || image.starts_with("file://")
        || image.starts_with("oci-archive:")
        || image.starts_with("docker-archive:")
    {
        return Err(refuse(
            "registry_target_required",
            "the eval target resolves to a local checkout; publish the image and pin its digest",
        ));
    }

    // A bare name with no registry host is not a published image; it resolves
    // only against whatever the local daemon happens to hold. This is the form
    // actually in play: the catalog names `craftax-eval-target`, and an
    // operator pin supplied the digest of a local `craftax-eval-target:refresh`
    // build that was never pushed. The recipe then reported `available` and
    // would have produced a benchmark number no other machine could reproduce.
    // A registry host is what makes a digest resolvable by someone else.
    let repository = image.split('@').next().unwrap_or(image);
    let host = repository.split('/').next().unwrap_or("");
    let has_registry_host = repository.contains('/')
        && (host.contains('.') || host.contains(':') || host == "localhost");
    if !has_registry_host && !allow_local_pinned_target {
        return Err(refuse("local_pinned_target_disabled",
            "Evaluation is supported, but this app process does not admit registry-less local images.",
        ));
    }

    let digest = recipe
        .get("imageDigest")
        .or_else(|| recipe.get("targetManifestDigest"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|digest| !digest.is_empty());
    // The reference itself may carry the digest; either source is acceptable,
    // but one of them must exist.
    let inline = image.split_once("@sha256:").map(|(_, hex)| hex);
    let declared = digest.and_then(|digest| digest.strip_prefix("sha256:"));
    let Some(hex) = inline.or(declared) else {
        return Err(refuse(
            "target_digest_missing",
            "the eval target is a mutable tag; publish the image and record its sha256 digest",
        ));
    };
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(refuse(
            "target_digest_mismatch",
            "the eval target digest is not a sha256 manifest digest",
        ));
    }
    // Both present and disagreeing is worse than either missing: the run would
    // record one identity and execute another.
    if let (Some(inline), Some(declared)) = (inline, declared) {
        if inline != declared {
            return Err(refuse(
                "target_digest_mismatch",
                "the eval target reference and its recorded digest disagree",
            ));
        }
    }
    Ok(())
}

/// The catalog when Eval cannot run, carrying *why*.
///
/// `availabilityCode` is the machine-readable half: `plugin_not_installed`,
/// `eval_runtime_not_provisioned`, `eval_runtime_interpreter_missing`,
/// `eval_runtime_import_failed`, `eval_runtime_digest_mismatch`, or
/// `eval_cli_failed`. A recipe that is merely unpinned is marked unavailable
/// by `mark_unreproducible_target_unavailable` with `target_not_digest_pinned`
/// and never reaches this function -- that distinction is the point.
fn offline_catalog(code: &str, reason: &str) -> Vec<Value> {
    EVAL_RECIPE_IDS
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "algorithmId": EVAL_ALGORITHM_ID,
                "availability": "unavailable",
                "availabilityCode": code,
                "availabilityReason": reason,
                "title": id,
                "executionKind": "evaluation",
                "recipeDiscovered": false,
                "executionSupported": true,
                "targetAdmitted": false,
            })
        })
        .collect()
}

pub async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    let recipe_id = request.recipe_id.clone();
    if !is_eval_recipe(&recipe_id) {
        bail!("unknown eval recipe: {recipe_id}");
    }
    if request.training_artifact_id.is_some() && request.candidate_set_id.is_some() {
        bail!("eval recipes take either training_artifact_id or candidate_set_id, not both");
    }
    let mut training_artifact = None;
    let candidate_set_id = if let Some(artifact_id) = request.training_artifact_id.clone() {
        let artifact = crate::training_artifacts::get(&artifact_id)?;
        let staged = super::eval_candidates::stage_training_artifact(&artifact)?;
        let staged_id = staged
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("staged candidate set omitted id"))?
            .to_string();
        training_artifact = Some(artifact);
        staged_id
    } else {
        request.candidate_set_id.clone().ok_or_else(|| {
            anyhow!("eval recipes require a staged candidate_set_id or a training_artifact_id")
        })?
    };
    let candidate_set_path = super::eval_candidates::manifest_path(&candidate_set_id)?;
    let candidate_set = super::eval_candidates::load(&candidate_set_id)?;
    let python = resolve_python()?;
    let report = preflight().map_err(|error| anyhow!(error))?;
    let recipe = recipe_catalog()
        .into_iter()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some(recipe_id.as_str()))
        .ok_or_else(|| anyhow!("eval recipe {recipe_id} is not in the local catalog"))?;
    if recipe.get("availability").and_then(Value::as_str) != Some("available") {
        if let Some(error) = recipe.get("admissionError") {
            bail!("{}", error);
        }
        let reason = recipe
            .get("availabilityReason")
            .and_then(Value::as_str)
            .unwrap_or("target image is not pinned");
        bail!(
            "{}",
            json!({
                "code": "recipe_unavailable",
                "contract": "workflow.exact_recipe",
                "owner": recipe_id,
                "retryable": true,
                "requestedRecipeId": recipe_id,
                "substitutionAllowed": false,
                "message": reason,
            })
        );
    }
    require_digest_pinned_target(&recipe, &recipe_id)?;
    if report
        .get("containerRuntimeAvailable")
        .and_then(Value::as_bool)
        != Some(true)
    {
        bail!(
            "no OCI runtime is available for eval trials; install {} or change it in the eval home's runtime.toml",
            report
                .get("containerRuntime")
                .and_then(Value::as_str)
                .unwrap_or("docker")
        );
    }

    let home = eval_home();
    let mut mlx_inference_url = None;
    if recipe_id == EVAL_MLX_LOCAL_RECIPE || recipe_id == EVAL_CRAFTAX_MLX_LOCAL_RECIPE {
        let client = super::mlx_runtime::MlxLoopback::ensure().await?;
        mlx_inference_url = Some(client.base_url.clone());
    }
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("opt_eval_{}", &suffix[..12]);
    // Local MLX speaks the OpenAI wire format, whose clients require a
    // non-empty API key even though this instance-owned loopback service does
    // not use a provider credential. Mint a per-run sentinel and pass it only
    // to the worker process; never persist it in the manifest or run record.
    let local_mlx_token = local_mlx_worker_token(&recipe_id);
    let workers = home.join("workers");
    fs::create_dir_all(&workers).context("create eval worker directory")?;
    let manifest_path = workers.join(format!("{run_id}.json"));
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&json!({
            "schema_version": "eval.worker-manifest.v1",
            "run_id": run_id,
            "recipe_id": recipe_id,
            "home": home,
            "candidate_set_path": candidate_set_path,
            "session_ref": request.session_ref.clone(),
            "mlx_inference_url": mlx_inference_url,
            "plan_override": request.plan_override.clone(),
        }))?,
    )
    .context("write eval worker manifest")?;

    let run_dir = home.join("runs").join(&run_id);
    let paid_provider = paid_provider_for_recipe(&recipe).map(str::to_string);
    let limits = recipe.get("limits").cloned().unwrap_or_else(|| json!({}));
    let candidates = candidate_set
        .get("candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let summary = json!({
        "recipeId": recipe_id,
        "task": recipe.get("task"),
        "candidateSetId": candidate_set_id,
        "candidateCount": candidates.len(),
        "baselineId": candidate_set.get("baseline_id"),
        "limits": limits,
        "model": if recipe_id == EVAL_MLX_LOCAL_RECIPE || recipe_id == EVAL_CRAFTAX_MLX_LOCAL_RECIPE {
            Some(super::mlx_runtime::TRAINING_MODEL_ID)
        } else {
            None
        },
        "image": recipe.get("image"),
        "imageDigest": recipe.get("imageDigest"),
        "targetManifestDigest": recipe.get("targetManifestDigest"),
        "runDirectory": run_dir,
        "globalMaxConcurrentTrials": report.get("maxConcurrentTrials"),
        "trainingArtifactId": training_artifact.as_ref().map(|item| item.id.clone()),
        "baseModelId": training_artifact.as_ref().map(|item| item.base_model_id.clone()),
        "producingRunId": training_artifact.as_ref().map(|item| item.producing_run_id.clone()),
        "configDigest": training_artifact.as_ref().and_then(|item| item.config_digest.clone()),
    });

    let create = OptimizerCreateRequest {
        algorithm_id: EVAL_ALGORITHM_ID.into(),
        algorithm_version: Some("1".into()),
        objective: Some(format!(
            "{} · score {} staged policies",
            recipe
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(&recipe_id),
            candidates.len()
        )),
        source: Some("local".into()),
        project_ref: recipe
            .get("task")
            .and_then(Value::as_str)
            .map(|task| format!("{task}@eval")),
        session_ref: request.session_ref.clone(),
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![OptimizerExecutionBinding {
            kind: "local_process".into(),
            id: run_id.clone(),
            label: Some("Local eval worker".into()),
            status: Some("starting".into()),
            metadata: json!({"recipeId": recipe_id, "runDirectory": run_dir}),
        }]),
        input_refs: Some({
            let mut refs = vec![
                OptimizerResourceRef {
                    kind: "candidate_set".into(),
                    id: candidate_set_id.clone(),
                    digest: None,
                    role: Some("candidates".into()),
                    title: Some("Staged policy candidates".into()),
                    metadata: candidate_set.clone(),
                },
                OptimizerResourceRef {
                    kind: "recipe".into(),
                    id: recipe_id.clone(),
                    digest: recipe
                        .get("targetManifestDigest")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    role: Some("configuration".into()),
                    title: recipe
                        .get("title")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    metadata: recipe.clone(),
                },
            ];
            if let Some(artifact) = &training_artifact {
                refs.push(OptimizerResourceRef {
                    kind: "training_artifact".into(),
                    id: artifact.id.clone(),
                    digest: artifact.digest.clone(),
                    role: Some("evaluated_adapter".into()),
                    title: Some("Training artifact".into()),
                    metadata: json!({
                        "baseModelId": artifact.base_model_id,
                        "producingRunId": artifact.producing_run_id,
                        "producingAlgorithm": artifact.producing_algorithm,
                        "configDigest": artifact.config_digest,
                        "datasetDigest": artifact.dataset_digest,
                    }),
                });
            }
            refs
        }),
        capabilities: Some(OptimizerCapabilities::for_algorithm(EVAL_ALGORITHM_ID)),
        summary: Some(summary),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    let (run, event) = service.create(create).await?;
    let (cancel_tx, cancel_rx) = watch::channel(None);
    service
        .register_local_recipe(run_id.clone(), cancel_tx)
        .await;
    let worker = service.clone();
    let worker_recipe_id = recipe_id.clone();
    let worker_recipe = recipe.clone();
    let worker_candidate_count = candidates.len() as u64;
    tokio::spawn(async move {
        if let Err(error) = run_worker(
            worker.clone(),
            run_id.clone(),
            python,
            manifest_path,
            run_dir,
            paid_provider,
            worker_recipe_id,
            worker_recipe,
            worker_candidate_count,
            local_mlx_token,
            cancel_rx,
        )
        .await
        {
            let _ = append_terminal(&worker, &run_id, "failed", error.to_string()).await;
        }
        worker.unregister_local_recipe(&run_id).await;
    });
    Ok((run, event))
}

/// Select the Workshop provider proxy only for credentials it actually owns.
/// Local MLX recipes deliberately carry their own app-owned token and route;
/// treating every HTTP route as OpenAI makes those runs demand an unrelated
/// provider key before the first trial can be dispatched.
fn paid_provider_for_recipe(recipe: &Value) -> Option<&'static str> {
    let secret = recipe
        .get("models")?
        .as_array()?
        .first()?
        .get("secret")?
        .as_str()?;
    match secret {
        "OPENROUTER_API_KEY" => Some("openrouter"),
        "OPENAI_API_KEY" => Some("openai"),
        _ => None,
    }
}

fn local_mlx_worker_token(recipe_id: &str) -> Option<String> {
    matches!(
        recipe_id,
        EVAL_MLX_LOCAL_RECIPE | EVAL_CRAFTAX_MLX_LOCAL_RECIPE
    )
    .then(|| format!("synth-local-{}", uuid::Uuid::new_v4().simple()))
}

async fn run_worker(
    service: OptimizerService,
    run_id: String,
    python: PathBuf,
    manifest_path: PathBuf,
    run_dir: PathBuf,
    paid_provider: Option<String>,
    recipe_id: String,
    recipe: Value,
    candidate_count: u64,
    local_mlx_token: Option<String>,
    mut cancel: super::CancelObserver,
) -> Result<()> {
    let _ownership = service.hold_run_ownership(&run_id)?;
    append_status(&service, &run_id, "optimizer.run.started", "running").await?;
    fs::create_dir_all(&run_dir).context("create eval run directory")?;
    let stdout_path = run_dir.join("worker.stdout.log");
    let stderr_path = run_dir.join("worker.stderr.log");
    let stderr = fs::File::create(&stderr_path)?;
    let mut command = Command::new(&python);
    command
        .arg("-m")
        .arg("synth_optimizers.eval")
        .arg("worker")
        .arg("--manifest")
        .arg(&manifest_path)
        // Admission and execution must resolve the same runtime. Finder does
        // not inherit the operator's shell PATH, so without this the catalog
        // can truthfully report Docker ready and the worker can still fail
        // immediately with `docker is not on PATH`.
        .env("PATH", eval_cli_path(std::env::var_os("PATH").as_deref())?);
    // Catalog discovery and execution must import the same reviewed source.
    // A packaged CUA snapshot intentionally has no project .venv, so the
    // selected immutable interpreter needs this overlay just as run_cli does;
    // otherwise admission can publish a ten-lane recipe while the worker
    // silently executes an older installed two-lane catalog.
    if let Some(project) = super::manager::optimizer_project_root()? {
        command.env("PYTHONPATH", project.join("src"));
    }
    if let Some(token) = local_mlx_token {
        command.env("SYNTH_MLX_RL_TOKEN", token);
    }
    if let Some(provider) = paid_provider.as_deref() {
        let secrets = crate::secrets::live().ok_or_else(|| {
            secrets_proxy_error(
                "secrets_proxy_unavailable",
                "paid eval workers require the Workshop secrets proxy",
            )
        })?;
        let policy = policy_from_eval_recipe(&recipe, candidate_count)?;
        let env = secrets
            .workload_env(provider, &run_id, &recipe_id, policy, "eval")
            .map_err(|error| secrets_proxy_error("secrets_proxy_denied", &error.to_string()))?;
        let routes = env.provider_routes().map_err(|error| {
            secrets_proxy_error("secrets_proxy_route_unbound", &error.to_string())
        })?;
        bind_provider_routes_into_manifest(&manifest_path, routes)?;
        let _ = service.persist_credential_chain(&run_id).await;
        for (key, value) in env.as_pairs() {
            command.env(key, value);
        }
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true)
        .spawn()
        .context("launch the local eval worker")?;

    // The worker's stdout is the event stream. Read it as it arrives and
    // mirror each line immediately; the same bytes are also appended to the
    // durable log so a Desktop restart can reconcile a run it no longer owns.
    let pipe = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("eval worker stdout was not piped"))?;
    let mut ingest = LiveIngest::open(&service, &run_id).await?;
    let mut log = fs::File::create(&stdout_path)?;
    let mut lines = BufReader::new(pipe).lines();
    let mut streaming = true;

    let mut cancelled_at: Option<Instant> = None;
    // Same observer policy as the GEPA poll loop: a gateway miss from the
    // evidence plane must not kill a live worker on the first tick, but a
    // sustained outage ends the run under a named code.
    let mut event_endpoint_outage_started: Option<Instant> = None;
    let outage_wait = crate::limits::OPTIMIZER_RUN_INDEX_WAIT;
    loop {
        tokio::select! {
            line = lines.next_line(), if streaming => {
                match line {
                    Ok(Some(line)) => {
                        use std::io::Write;
                        let _ = writeln!(log, "{line}");
                        let _ = log.flush();
                        match ingest.push(&line).await {
                            Ok(()) => event_endpoint_outage_started = None,
                            Err(error)
                                if super::manager::observer_error_is_transient_gateway(&error) =>
                            {
                                let started = event_endpoint_outage_started
                                    .get_or_insert_with(Instant::now);
                                if started.elapsed() >= outage_wait {
                                    let waited = outage_wait.as_secs_f32();
                                    let _ = child.kill().await;
                                    bail!(
                                        "event_endpoint_outage: the eval observer stayed \
                                         unavailable for {waited}s while the worker for {run_id} \
                                         was live (last error: {error})"
                                    );
                                }
                                crate::platform::logging::report("optimizers", "eprintln", format!("eval event ingest failed: {error}"));
                            }
                            Err(error) => {
                                // Durable-log write misses stay on the log;
                                // the producer is still running. Restart
                                // reconcile rereads worker.stdout.log.
                                crate::platform::logging::report("optimizers", "eprintln", format!("eval event ingest failed: {error}"));
                            }
                        }
                    }
                    // Stdout closed: the worker is finishing. Stop selecting on
                    // it or the branch spins on a closed pipe.
                    Ok(None) | Err(_) => streaming = false,
                }
            }
            status = child.wait() => {
                let status = status.context("wait for the eval worker")?;
                // Drain anything buffered between the last read and exit.
                while let Ok(Some(line)) = lines.next_line().await {
                    use std::io::Write;
                    let _ = writeln!(log, "{line}");
                    let _ = ingest.push(&line).await;
                }
                let _ = std::io::Write::flush(&mut log);
                if !status.success() {
                    bail!(
                        "eval worker exited with {status}; see {}",
                        stderr_path.display()
                    );
                }
                // A clean worker exit is not a successful campaign. The status
                // comes from what the durable log proves, not from the child's
                // return code.
                let (status, detail) = settled_status(&service, &run_id).await?;
                append_terminal(&service, &run_id, status, detail).await?;
                return Ok(());
            }
            changed = cancel.changed() => {
                if changed.is_ok() && cancel.borrow().is_some() && cancelled_at.is_none() {
                    // Ask first: the worker still has containers to stop, leases
                    // to release, and evidence to seal.
                    fs::write(run_dir.join("CANCEL"), chrono::Utc::now().to_rfc3339()).ok();
                    cancelled_at = Some(Instant::now());
                }
            }
            _ = sleep(Duration::from_millis(250)) => {}
        }
        if let Some(at) = cancelled_at {
            if at.elapsed() > CANCEL_GRACE {
                child.kill().await.context("kill the eval worker")?;
                ingest_stdout(&service, &run_id, &stdout_path).await?;
                append_terminal(
                    &service,
                    &run_id,
                    "cancelled",
                    "eval worker did not seal evidence within the cancellation grace period".into(),
                )
                .await?;
                return Ok(());
            }
        }
    }
}

/// Pausing a matrix stops the worker dispatching new trials. In-flight trials
/// finish and seal — a paused run is a run holding position, not a run that
/// threw away the containers it had already started.
pub fn set_paused(run_id: &str, paused: bool) -> Result<()> {
    let sentinel = eval_home().join("runs").join(run_id).join(PAUSE_SENTINEL);
    if paused {
        let dir = sentinel
            .parent()
            .ok_or_else(|| anyhow!("invalid eval run directory"))?;
        fs::create_dir_all(dir).context("create eval run directory")?;
        fs::write(&sentinel, chrono::Utc::now().to_rfc3339()).context("write pause sentinel")?;
    } else {
        let _ = fs::remove_file(&sentinel);
    }
    Ok(())
}

/// Appends worker events as they arrive on the pipe.
///
/// The worker's stdout *is* the event stream, so it is read line by line and
/// mirrored immediately rather than polled off a log file. The cursor is held
/// here instead of re-read per event: a live run appends, it does not re-scan
/// what it has already written.
struct LiveIngest {
    service: OptimizerService,
    run_id: String,
    sequence: u64,
}

impl LiveIngest {
    async fn open(service: &OptimizerService, run_id: &str) -> Result<Self> {
        let sequence = service.get(run_id.to_string()).await?.cursor_seq;
        Ok(Self {
            service: service.clone(),
            run_id: run_id.to_string(),
            sequence,
        })
    }

    async fn push(&mut self, line: &str) -> Result<()> {
        let line = crate::secrets::redact_live(line);
        let Ok(raw) = serde_json::from_str::<Value>(&line) else {
            return Ok(());
        };
        if raw.get("schema_version").and_then(Value::as_str) != Some("eval.worker-event.v1") {
            return Ok(());
        }
        let worker_seq = raw.get("seq").and_then(Value::as_u64).unwrap_or(0);
        let Some(envelope) = envelope_for(&self.run_id, self.sequence + 1, worker_seq, raw) else {
            return Ok(());
        };
        self.sequence += 1;
        self.service
            .append_events(self.run_id.clone(), vec![envelope])
            .await?;
        Ok(())
    }
}

/// Build one `optimizer_event.v1` from one worker line. Shared by the live pipe
/// and the file-based reconcile path so both produce identical envelopes.
fn envelope_for(
    run_id: &str,
    sequence: u64,
    worker_seq: u64,
    raw: Value,
) -> Option<OptimizerEventEnvelope> {
    let (event_type, item, delta, snapshot, usage, level) = canonicalize(&raw)?;
    Some(OptimizerEventEnvelope {
        schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
        event_id: Some(format!("{run_id}:eval:{worker_seq}")),
        event_type: event_type.into(),
        sequence_number: sequence,
        occurred_at: raw
            .get("occurred_at")
            .and_then(Value::as_str)
            .unwrap_or(&chrono::Utc::now().to_rfc3339())
            .to_string(),
        optimizer_run_id: run_id.into(),
        algorithm_id: EVAL_ALGORITHM_ID.into(),
        level: Some(level.into()),
        item,
        delta,
        snapshot,
        usage_delta: usage,
        artifact_refs: artifact_refs(&raw),
        error: raw.get("error").cloned().filter(|value| !value.is_null()),
        raw,
    })
}

/// Reconcile path: re-read the durable log for a run whose worker process is
/// gone (a Desktop restart), deduplicating against what already mirrored.
async fn ingest_stdout(service: &OptimizerService, run_id: &str, path: &Path) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let existing = service
        .events_after(run_id.to_string(), 0, Some(2_000))
        .await?;
    let seen = existing
        .iter()
        .filter_map(|event| event.event_id.as_deref())
        .collect::<std::collections::HashSet<_>>();
    let mut sequence = service.get(run_id.to_string()).await?.cursor_seq;
    let mut events = Vec::new();
    for line in fs::read_to_string(path)?.lines() {
        let Ok(raw) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if raw.get("schema_version").and_then(Value::as_str) != Some("eval.worker-event.v1") {
            continue;
        }
        let worker_seq = raw.get("seq").and_then(Value::as_u64).unwrap_or(0);
        if seen.contains(format!("{run_id}:eval:{worker_seq}").as_str()) {
            continue;
        }
        if let Some(envelope) = envelope_for(run_id, sequence + 1, worker_seq, raw) {
            sequence += 1;
            events.push(envelope);
        }
    }
    if !events.is_empty() {
        service.append_events(run_id.to_string(), events).await?;
    }
    Ok(())
}

/// Reopen a locally-owned eval after the Desktop process disappeared.  The
/// worker's stdout log is durable authority: a terminal line in that log must
/// win over the stale `running` projection that happened to be persisted just
/// before restart.  This intentionally does not launch a worker or infer a
/// result from an exit code; it only mirrors already-durable worker evidence.
pub(super) async fn reconcile_persisted(
    service: &OptimizerService,
    run_id: &str,
) -> Result<OptimizerRunRecord> {
    let run = service.get(run_id.to_string()).await?;
    if run.algorithm_id != EVAL_ALGORITHM_ID || super::service::is_terminal_status(&run.status) {
        return Ok(run);
    }
    let Some(run_dir) = run
        .summary
        .get("runDirectory")
        .and_then(Value::as_str)
        .map(PathBuf::from)
    else {
        return Ok(run);
    };
    ingest_stdout(service, run_id, &run_dir.join("worker.stdout.log")).await?;
    service.get(run_id.to_string()).await
}

/// Trace and verifier files are first-class evidence, so they reach the run's
/// artifact slice rather than living only inside the trial directory.
fn artifact_refs(raw: &Value) -> Vec<Value> {
    raw.get("trial")
        .and_then(|trial| trial.get("artifacts"))
        .and_then(Value::as_array)
        .map(|artifacts| {
            artifacts
                .iter()
                .filter(|artifact| artifact.get("declared").and_then(Value::as_bool) == Some(true))
                .map(|artifact| {
                    json!({
                        "kind": artifact.get("role").cloned().unwrap_or(json!("artifact")),
                        "id": artifact.get("path"),
                        "path": artifact.get("path"),
                        "digest": artifact.get("digest"),
                        "bytes": artifact.get("bytes"),
                        "title": format!(
                            "{} · {}",
                            artifact.get("role").and_then(Value::as_str).unwrap_or("artifact"),
                            raw.get("trial_id").and_then(Value::as_str).unwrap_or("trial")
                        )
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

type Canonical = (
    &'static str,
    Option<Value>,
    Map<String, Value>,
    Option<Map<String, Value>>,
    Option<Map<String, Value>>,
    &'static str,
);

fn canonicalize(raw: &Value) -> Option<Canonical> {
    let kind = raw.get("event")?.as_str()?;
    let mut delta = Map::new();
    let mut snapshot: Option<Map<String, Value>> = None;
    let mut item: Option<Value> = None;
    let mut usage: Option<Map<String, Value>> = None;
    let mut level = "info";

    match kind {
        "eval.run.planned" => {
            delta.insert(
                "message".into(),
                json!(format!(
                    "Planned {} trials across {} candidates",
                    raw.get("planned_trials")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    raw.get("candidates")
                        .and_then(Value::as_array)
                        .map(Vec::len)
                        .unwrap_or(0)
                )),
            );
            let mut snap = Map::new();
            for key in [
                "candidates",
                "candidate_set_id",
                "parallelism",
                "global_capacity",
                "planned_trials",
                "manifest_digest",
                "recipe_id",
            ] {
                if let Some(value) = raw.get(key) {
                    snap.insert(key.into(), value.clone());
                }
            }
            snapshot = Some(snap);
        }
        "eval.seed_ledger.sealed" => {
            let ledger = raw.get("ledger").cloned().unwrap_or(json!({}));
            delta.insert("message".into(), json!("Sealed the seed ledger"));
            snapshot = Some(
                [("seedLedger".to_string(), ledger)]
                    .into_iter()
                    .collect::<Map<_, _>>(),
            );
        }
        "eval.trial.queued" | "eval.trial.started" => {
            for key in ["trial_id", "candidate_id", "seed", "scenario", "stage"] {
                if let Some(value) = raw.get(key) {
                    delta.insert(key.into(), value.clone());
                }
            }
            delta.insert(
                "status".into(),
                json!(if kind.ends_with("started") {
                    "running"
                } else {
                    "queued"
                }),
            );
            // A queued trial must not look like a queued *run*.
            delta.remove("status");
        }
        "eval.trial.event" => {
            delta.insert(
                "message".into(),
                raw.get("container_event")
                    .and_then(|event| event.get("event"))
                    .cloned()
                    .unwrap_or(json!("container event")),
            );
            delta.insert("trial_id".into(), raw.get("trial_id").cloned()?);
            delta.insert(
                "container_event".into(),
                raw.get("container_event").cloned().unwrap_or(json!({})),
            );
            level = "debug";
        }
        "eval.trial.terminal" => {
            let trial = raw.get("trial")?;
            item = Some(json!({
                "kind": "trial",
                "id": trial.get("trial_id"),
                "status": trial.get("status"),
                "benchmarkStatus": trial.get("benchmark_status"),
                "valid": trial.get("valid"),
                "candidateId": trial.get("key").and_then(|key| key.get("candidate_id")),
                "stage": trial.get("key").and_then(|key| key.get("stage")),
                "seed": trial.get("key").and_then(|key| key.get("seed")),
                "scenario": trial.get("key").and_then(|key| key.get("scenario")),
                "metrics": trial.get("metrics"),
                "gates": trial.get("gates"),
                "missingGates": trial.get("missing_gates"),
                "missingArtifacts": trial.get("missing_artifacts"),
                "artifacts": trial.get("artifacts"),
                "evidenceDir": trial.get("evidence_dir"),
                "raw": trial
            }));
            delta.insert(
                "message".into(),
                json!(format!(
                    "Trial {} {}",
                    trial.get("trial_id").and_then(Value::as_str).unwrap_or(""),
                    trial.get("status").and_then(Value::as_str).unwrap_or("")
                )),
            );
            if trial.get("status").and_then(Value::as_str) != Some("evaluated") {
                level = "warn";
            }
            let mut totals = Map::new();
            if let Some(value) = trial.get("usage").and_then(|usage| usage.get("rollouts")) {
                totals.insert("rollouts".into(), value.clone());
            }
            if let Some(value) = trial
                .get("usage")
                .and_then(|usage| usage.get("cost_usd"))
                .filter(|value| !value.is_null())
            {
                totals.insert("cost_usd".into(), value.clone());
            }
            if let Some(value) = trial
                .get("usage")
                .and_then(|usage| usage.get("wall_time_ms"))
            {
                totals.insert("wall_time_ms".into(), value.clone());
            }
            if !totals.is_empty() {
                usage = Some(totals);
            }
        }
        "eval.trial.evidence_incomplete" => {
            level = "warn";
            delta.insert(
                "message".into(),
                json!(format!(
                    "Trial {} did not write every promised artifact",
                    raw.get("trial_id").and_then(Value::as_str).unwrap_or("")
                )),
            );
            delta.insert(
                "missingArtifacts".into(),
                raw.get("missing_artifacts").cloned().unwrap_or(json!([])),
            );
        }
        "eval.candidate.scored" => {
            let card = raw.get("scorecard")?;
            item = Some(json!({
                "kind": "candidate",
                "id": card.get("candidate_id"),
                "label": card.get("label"),
                "stage": card.get("stage"),
                "isBaseline": card.get("is_baseline"),
                "trials": card.get("trials"),
                "metrics": card.get("metrics"),
                "gateFailures": card.get("gate_failures"),
                "pairedLift": card.get("paired_lift"),
                "pairedTrials": card.get("paired_trials"),
                "eliminatedAt": card.get("eliminated_at"),
                "eliminationReason": card.get("elimination_reason"),
                "costUsd": card.get("cost_usd"),
                // How much of the scored episodes the candidate's own policy
                // chose. A budget-exhausted policy is replaced by a fallback
                // for the rest of the episode, so a mean read without this can
                // be mostly the fallback's score wearing the candidate's name.
                "policyStepFraction": card.get("policy_step_fraction"),
                "budgetExhaustedTrials": card
                    .get("trials")
                    .and_then(|trials| trials.get("budget_exhausted")),
                "raw": card
            }));
            delta.insert(
                "candidate_id".into(),
                card.get("candidate_id").cloned().unwrap_or(Value::Null),
            );
        }
        "eval.candidate.eliminated" => {
            level = "warn";
            delta.insert(
                "candidate_id".into(),
                raw.get("candidate_id").cloned().unwrap_or(Value::Null),
            );
            delta.insert(
                "message".into(),
                json!(format!(
                    "{} eliminated: {}",
                    raw.get("label").and_then(Value::as_str).unwrap_or(""),
                    raw.get("reason").and_then(Value::as_str).unwrap_or("")
                )),
            );
        }
        "eval.selection.completed" => {
            let selection = raw.get("selection")?;
            snapshot = Some(
                [("selection".to_string(), selection.clone())]
                    .into_iter()
                    .collect::<Map<_, _>>(),
            );
            delta.insert(
                "message".into(),
                json!(selection
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("selection completed")),
            );
        }
        "eval.run.terminal" => {
            let status = raw
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("completed");
            // Orchestration status is not the promotion decision; both travel,
            // and neither is allowed to stand in for the other.
            delta.insert("status".into(), json!(status));
            delta.insert(
                "selectionStatus".into(),
                raw.get("selection_status").cloned().unwrap_or(Value::Null),
            );
            delta.insert(
                "winnerId".into(),
                raw.get("winner_id").cloned().unwrap_or(Value::Null),
            );
            if let Some(dir) = raw.get("evidence_dir") {
                delta.insert("evidenceDir".into(), dir.clone());
            }
            return Some((
                match status {
                    "failed" => "optimizer.run.failed",
                    "cancelled" => "optimizer.run.cancelled",
                    _ => "optimizer.run.completed",
                },
                None,
                delta,
                None,
                None,
                if status == "failed" { "error" } else { "info" },
            ));
        }
        _ => return None,
    }
    Some((
        match kind {
            "eval.run.planned" => "eval.run.planned",
            "eval.seed_ledger.sealed" => "eval.seed_ledger.sealed",
            "eval.trial.queued" => "eval.trial.queued",
            "eval.trial.started" => "eval.trial.started",
            "eval.trial.event" => "eval.trial.event",
            "eval.trial.terminal" => "eval.trial.terminal",
            "eval.trial.evidence_incomplete" => "eval.trial.evidence_incomplete",
            "eval.candidate.scored" => "eval.candidate.scored",
            "eval.candidate.eliminated" => "eval.candidate.eliminated",
            "eval.selection.completed" => "eval.selection.completed",
            _ => return None,
        },
        item,
        delta,
        snapshot,
        usage,
        level,
    ))
}

fn map_of(key: &str, value: Value) -> Map<String, Value> {
    [(key.to_string(), value)].into_iter().collect()
}

async fn append_status(
    service: &OptimizerService,
    run_id: &str,
    event_type: &str,
    status: &str,
) -> Result<()> {
    service
        .append_event_payloads(
            run_id.to_string(),
            vec![OptimizerEventDraft::new(event_type, EVAL_ALGORITHM_ID)
                .idempotency_key(format!("host:lifecycle:{event_type}"))
                .delta(map_of("status", json!(status)))
                .raw(json!({"source": "eval_recipe"}))],
        )
        .await?;
    Ok(())
}

/// Decide how a campaign that ran to completion actually ended.
///
/// A worker that exits 0 has finished its job; it has not necessarily produced
/// a result. The packaged Craftax smoke exited cleanly having recorded
/// "4 of 4 trials did not produce valid evidence" and still settled `completed`,
/// so the selection verdict and the run status disagreed about the same run.
///
/// Some trials failing is normal in a screening matrix and stays `completed`.
/// *No* valid evidence at all, or a producer selection that names its own
/// evidence invalid, is not a success.
async fn settled_status(
    service: &OptimizerService,
    run_id: &str,
) -> Result<(&'static str, String)> {
    let run = service.get(run_id.to_string()).await?;
    let events = service
        .events_after(run_id.to_string(), 0, Some(2_000))
        .await?;
    let verdict = events.iter().rev().find_map(|event| {
        let selection = event.snapshot.as_ref()?.get("selection")?;
        let status = selection
            .get("status")
            .or_else(|| selection.get("selection_status"))
            .and_then(Value::as_str)?;
        Some((
            status.to_string(),
            selection
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        ))
    });
    if let Some((status, reason)) = &verdict {
        if matches!(status.as_str(), "invalid_evidence" | "failed" | "aborted") {
            let reason = if reason.is_empty() {
                format!("the campaign selection settled {status}")
            } else {
                reason.clone()
            };
            return Ok(("failed", reason));
        }
    }
    let counts = super::terminal::work_counts(&run, &events);
    if let (Some(planned), Some(succeeded)) = (counts.planned, counts.succeeded) {
        if planned > 0 && succeeded == 0 {
            return Ok((
                "failed",
                format!("none of {planned} planned trials produced valid evidence"),
            ));
        }
    }
    Ok(("completed", "eval worker finished".into()))
}

async fn append_terminal(
    service: &OptimizerService,
    run_id: &str,
    status: &str,
    detail: String,
) -> Result<()> {
    let run = service.get(run_id.to_string()).await?;
    // Settled is settled when a manifest exists, not merely when the status
    // string looks terminal: a status rewritten without an event must not let
    // the run skip its own terminal event.
    if service
        .terminal_manifest(run_id.to_string())
        .await?
        .is_some()
    {
        return Ok(());
    }
    let error = if status == "failed" {
        let stderr = run
            .summary
            .get("runDirectory")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .map(|dir| dir.join("worker.stderr.log"));
        let tail = stderr.as_ref().and_then(|path| tail_text(path));
        Some(json!({
            "message": tail.as_deref().unwrap_or(&detail),
            "supervisorDetail": detail,
            "stderrTail": tail,
            "logPath": stderr
        }))
    } else {
        None
    };
    let cause = match status {
        "failed" => super::kernel::SettleCause::Failed {
            detail: detail.clone(),
        },
        "cancelled" => super::kernel::SettleCause::Cancelled {
            request: std::sync::Arc::new(super::kernel::CancellationRequest::new(
                super::kernel::CancellationCause::ContainerRequested,
                "eval:worker",
                format!("run:{run_id}"),
            )),
        },
        _ => super::kernel::SettleCause::Completed,
    };
    service.settle_run(run_id.to_string(), cause, error).await?;
    Ok(())
}

fn tail_text(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    if text.trim().is_empty() {
        return None;
    }
    Some(
        text.chars()
            .rev()
            .take(4000)
            .collect::<String>()
            .chars()
            .rev()
            .collect(),
    )
}

