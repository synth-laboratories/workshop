//! Product-owned optimizer recipes. This module is the local execution trust
//! boundary: callers select an allowlisted recipe but cannot supply commands,
//! paths, environment variables, or credentials.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::json;
use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::watch, time::sleep};

use super::{
    models::{
        OptimizerCreateRequest, OptimizerEventEnvelope, OptimizerExecutionBinding,
        OptimizerRecipeRunRequest, OptimizerResourceRef, OPTIMIZER_EVENT_SCHEMA_VERSION,
    },
    normalize, OptimizerService,
};

pub const BANKING77_GEPA_SMOKE_RECIPE: &str = "gepa.banking77.smoke.v1";
pub const BANKING77_GEPA_LUNA_RECIPE: &str = "gepa.banking77.luna.v1";
pub const BANKING77_GEPA_SOL_RECIPE: &str = "gepa.banking77.sol.v1";
const TRAIN_ROWS: usize = 50;
const HELDOUT_ROWS: usize = 50;
const MAX_GENERATIONS: i64 = 1;
const PROPOSALS_PER_GENERATION: i64 = 1;
const MINIBATCH_SIZE: i64 = 20;
// One generation consumes a 50-example seed evaluation, a 20-example parent
// reference, a 20-example candidate minibatch, and a 50-example candidate
// evaluation. Terminal comparison gives both the seed and a distinct winning
// proposal 50 heldout examples, so heldout needs room for two candidates.
const MAX_TRAIN_ROLLOUTS: i64 = 140;
const MAX_HELDOUT_ROLLOUTS: i64 = 100;
const MAX_TOTAL_ROLLOUTS: i64 = 240;
const MAX_COST_USD: f64 = 2.45;
const PROPOSER_ESTIMATED_COST_USD: f64 = 0.05;
const ROLLOUT_ESTIMATED_COST_USD: f64 = 0.01;
const PROPOSER_TIMEOUT_SECONDS: i64 = 300;
const PROPOSER_MESSAGE_STALL_TIMEOUT_SECONDS: i64 = 120;

#[derive(Clone, Copy)]
enum ProposerProfile {
    LunaMedium,
    SolMedium,
}

impl ProposerProfile {
    fn for_recipe(recipe_id: &str) -> Result<Self> {
        match recipe_id {
            BANKING77_GEPA_SMOKE_RECIPE | BANKING77_GEPA_LUNA_RECIPE => Ok(Self::LunaMedium),
            BANKING77_GEPA_SOL_RECIPE => Ok(Self::SolMedium),
            _ => bail!("unknown Banking77 GEPA recipe: {recipe_id}"),
        }
    }

    fn config_id(self) -> &'static str {
        match self {
            Self::LunaMedium => "luna_med",
            Self::SolMedium => "sol_med",
        }
    }

    fn model(self) -> &'static str {
        match self {
            Self::LunaMedium => "gpt-5.6-luna",
            Self::SolMedium => "gpt-5.6-sol",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::LunaMedium => "Luna medium",
            Self::SolMedium => "Sol medium",
        }
    }
}

pub(super) async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    let recipe_id = request.recipe_id.clone();
    let proposer = ProposerProfile::for_recipe(&recipe_id)?;
    let manager = service.manager().clone();
    require_plugin_ready(&manager).await?;
    let cookbook = banking77_cookbook_root()?;
    let run_id = recipe_run_id(proposer);
    let runs_root = cookbook
        .parent()
        .ok_or_else(|| anyhow!("invalid Banking77 cookbook path"))?
        .join("runs");
    let run_dir = runs_root.join(&run_id);
    fs::create_dir_all(&run_dir).context("create Banking77 GEPA run directory")?;
    let port = reserve_loopback_port()?;
    let uv = resolve_uv()?;
    let codex_home = resolve_codex_home()?;
    let config_path = run_dir.join("workshop.recipe.toml");
    materialize_config(
        &cookbook,
        &runs_root,
        &run_id,
        port,
        &uv,
        &config_path,
        proposer,
        &codex_home,
    )?;

    let create = OptimizerCreateRequest {
        algorithm_id: "gepa".into(),
        algorithm_version: Some("synth-optimizers-0.2.0".into()),
        objective: Some(format!(
            "Banking77 intent prompt · bounded GEPA · {}",
            proposer.title()
        )),
        source: Some("local".into()),
        project_ref: Some("banking77@huggingface-polyai-pinned-by-cookbook".into()),
        session_ref: request.session_ref,
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![OptimizerExecutionBinding {
            kind: "local_process".into(),
            id: run_id.clone(),
            label: Some(format!("Banking77 GEPA · {}", proposer.title())),
            status: Some("starting".into()),
            metadata: json!({
                "recipeId": recipe_id,
                "port": port,
                "proposerPolicyRef": {
                    "harness": "gepa_proposer",
                    "config": proposer.config_id(),
                },
            }),
        }]),
        input_refs: Some(vec![
            OptimizerResourceRef {
                kind: "dataset".into(),
                id: "banking77".into(),
                digest: None,
                role: Some("train_and_heldout".into()),
                title: Some("Banking77 (cookbook-pinned loader)".into()),
                metadata: json!({ "trainRows": TRAIN_ROWS, "heldoutRows": HELDOUT_ROWS }),
            },
            OptimizerResourceRef {
                kind: "recipe".into(),
                id: recipe_id.clone(),
                digest: None,
                role: Some("configuration".into()),
                title: Some(format!("Bounded Banking77 GEPA · {}", proposer.title())),
                metadata: recipe_limits(),
            },
            OptimizerResourceRef {
                kind: "policy_ref".into(),
                id: proposer.config_id().into(),
                digest: None,
                role: Some("proposer".into()),
                title: Some(format!("GEPA proposer · {}", proposer.title())),
                metadata: json!({
                    "harness": "gepa_proposer",
                    "config": proposer.config_id(),
                }),
            },
            OptimizerResourceRef {
                kind: "policy_ref".into(),
                id: "banking77_candidate".into(),
                digest: None,
                role: Some("evaluator".into()),
                title: Some("Banking77 candidate evaluator".into()),
                metadata: json!({
                    "harness": "banking77_eval",
                    "config": "candidate",
                }),
            },
        ]),
        capabilities: None,
        summary: Some(json!({
            "recipeId": recipe_id,
            "task": "banking77",
            "proposerPolicyRef": {
                "harness": "gepa_proposer",
                "config": proposer.config_id(),
            },
            "limits": recipe_limits(),
            "runDirectory": run_dir,
        })),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    let (run, event) = service.create(create).await?;
    let (run, _) = manager.pin_run(service, &run.id, &recipe_id).await?;
    append_status_event(service, &run_id, "optimizer.run.queued", "queued").await?;

    let (cancel_tx, cancel_rx) = watch::channel(false);
    service
        .register_local_recipe(run_id.clone(), cancel_tx)
        .await;
    let worker_service = service.clone();
    let worker_manager = manager.clone();
    tokio::spawn(async move {
        if let Err(error) = run_recipe_worker(
            worker_service.clone(),
            run_id.clone(),
            cookbook,
            config_path,
            run_dir,
            manager,
            cancel_rx,
        )
        .await
        {
            let _ = append_terminal_event(&worker_service, &run_id, true, error.to_string()).await;
        }
        worker_manager.release_gepa_recipe(&run_id).await;
        worker_service.unregister_local_recipe(&run_id).await;
    });
    Ok((run, event))
}

pub(super) async fn prepare(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    let recipe_id = request.recipe_id.clone();
    let _proposer = ProposerProfile::for_recipe(&recipe_id)?;
    let manager = service.manager().clone();
    require_plugin_ready(&manager).await?;
    let (mut run, event, _cookbook, _config_path, _run_dir) =
        materialize_prepared_run(service, request, "waiting_for_viewer").await?;
    let digest = preparation_digest(&run);
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert("preparationDigest".into(), json!(digest));
    summary.insert("waitingForViewer".into(), json!(true));
    if let Some(digest) = manager
        .advertised_capabilities()
        .get("digest")
        .cloned()
    {
        summary.insert("capabilitiesDigest".into(), digest);
    }
    run.summary = serde_json::Value::Object(summary);
    run.status = "waiting_for_viewer".into();
    let run = service.persist_run(run).await?;
    Ok((run, event))
}

pub(super) async fn start_prepared(
    service: &OptimizerService,
    run_id: &str,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    require_plugin_ready(service.manager()).await?;
    let run = service.get(run_id.to_string()).await?;
    if run.status != "waiting_for_viewer" && run.status != "queued" {
        bail!("optimizer run `{run_id}` is not prepared for start (status {})", run.status);
    }
    let recipe_id = run
        .summary
        .get("recipeId")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("prepared run omitted recipeId"))?
        .to_owned();
    let _ = ProposerProfile::for_recipe(&recipe_id)?;
    let cookbook = banking77_cookbook_root()?;
    let run_dir = run
        .summary
        .get("runDirectory")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("prepared run omitted runDirectory"))?;
    let config_path = run_dir.join("workshop.recipe.toml");
    if !config_path.is_file() {
        bail!("prepared run is missing its recipe config");
    }
    let manager = service.manager().clone();
    append_status_event(service, run_id, "optimizer.run.queued", "queued").await?;
    let (cancel_tx, cancel_rx) = watch::channel(false);
    service
        .register_local_recipe(run_id.to_string(), cancel_tx)
        .await;
    let worker_service = service.clone();
    let worker_manager = manager.clone();
    let worker_run_id = run_id.to_string();
    tokio::spawn(async move {
        if let Err(error) = run_recipe_worker(
            worker_service.clone(),
            worker_run_id.clone(),
            cookbook,
            config_path,
            run_dir,
            manager,
            cancel_rx,
        )
        .await
        {
            let _ = append_terminal_event(&worker_service, &worker_run_id, true, error.to_string())
                .await;
        }
        worker_manager.release_gepa_recipe(&worker_run_id).await;
        worker_service.unregister_local_recipe(&worker_run_id).await;
    });
    let started = service.get(run_id.to_string()).await?;
    Ok((started, None))
}

async fn require_plugin_ready(manager: &super::OptimizerManager) -> Result<()> {
    if manager.is_running().await {
        return Ok(());
    }
    let status = manager.status().await;
    let suggested = if status.version.is_none() {
        "install"
    } else {
        "start"
    };
    let phase = if status.version.is_none() {
        "not_installed"
    } else {
        status.phase.as_str()
    };
    Err(crate::plugins::PluginNotReady::new(phase, suggested).into())
}

fn preparation_digest(run: &super::models::OptimizerRunRecord) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(run.id.as_bytes());
    hasher.update(run.algorithm_id.as_bytes());
    if let Some(version) = run.algorithm_version.as_deref() {
        hasher.update(version.as_bytes());
    }
    hasher.update(serde_json::to_vec(&run.summary).unwrap_or_default());
    format!("sha256:{:x}", hasher.finalize())
}

async fn materialize_prepared_run(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
    status: &str,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
    PathBuf,
    PathBuf,
    PathBuf,
)> {
    let recipe_id = request.recipe_id.clone();
    let proposer = ProposerProfile::for_recipe(&recipe_id)?;
    let cookbook = banking77_cookbook_root()?;
    let run_id = recipe_run_id(proposer);
    let runs_root = cookbook
        .parent()
        .ok_or_else(|| anyhow!("invalid Banking77 cookbook path"))?
        .join("runs");
    let run_dir = runs_root.join(&run_id);
    fs::create_dir_all(&run_dir).context("create Banking77 GEPA run directory")?;
    let port = reserve_loopback_port()?;
    let uv = resolve_uv()?;
    let codex_home = resolve_codex_home()?;
    let config_path = run_dir.join("workshop.recipe.toml");
    materialize_config(
        &cookbook,
        &runs_root,
        &run_id,
        port,
        &uv,
        &config_path,
        proposer,
        &codex_home,
    )?;
    let create = OptimizerCreateRequest {
        algorithm_id: "gepa".into(),
        algorithm_version: Some("synth-optimizers-0.2.0".into()),
        objective: Some(format!(
            "Banking77 intent prompt · bounded GEPA · {}",
            proposer.title()
        )),
        source: Some("local".into()),
        project_ref: Some("banking77@huggingface-polyai-pinned-by-cookbook".into()),
        session_ref: request.session_ref,
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![OptimizerExecutionBinding {
            kind: "local_process".into(),
            id: run_id.clone(),
            label: Some(format!("Banking77 GEPA · {}", proposer.title())),
            status: Some(status.into()),
            metadata: json!({
                "recipeId": recipe_id,
                "port": port,
                "proposerPolicyRef": {
                    "harness": "gepa_proposer",
                    "config": proposer.config_id(),
                },
            }),
        }]),
        input_refs: Some(vec![
            OptimizerResourceRef {
                kind: "dataset".into(),
                id: "banking77".into(),
                digest: None,
                role: Some("train_and_heldout".into()),
                title: Some("Banking77 (cookbook-pinned loader)".into()),
                metadata: json!({ "trainRows": TRAIN_ROWS, "heldoutRows": HELDOUT_ROWS }),
            },
            OptimizerResourceRef {
                kind: "recipe".into(),
                id: recipe_id.clone(),
                digest: None,
                role: Some("configuration".into()),
                title: Some(format!("Bounded Banking77 GEPA · {}", proposer.title())),
                metadata: recipe_limits(),
            },
            OptimizerResourceRef {
                kind: "policy_ref".into(),
                id: proposer.config_id().into(),
                digest: None,
                role: Some("proposer".into()),
                title: Some(format!("GEPA proposer · {}", proposer.title())),
                metadata: json!({
                    "harness": "gepa_proposer",
                    "config": proposer.config_id(),
                }),
            },
            OptimizerResourceRef {
                kind: "policy_ref".into(),
                id: "banking77_candidate".into(),
                digest: None,
                role: Some("evaluator".into()),
                title: Some("Banking77 candidate evaluator".into()),
                metadata: json!({
                    "harness": "banking77_eval",
                    "config": "candidate",
                }),
            },
        ]),
        capabilities: None,
        summary: Some(json!({
            "recipeId": recipe_id,
            "task": "banking77",
            "proposerPolicyRef": {
                "harness": "gepa_proposer",
                "config": proposer.config_id(),
            },
            "limits": recipe_limits(),
            "runDirectory": run_dir,
            "proposerModel": proposer.model(),
        })),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    let (run, event) = service.create(create).await?;
    let (run, _) = service
        .manager()
        .pin_run(service, &run.id, &recipe_id)
        .await?;
    Ok((run, event, cookbook, config_path, run_dir))
}

pub fn recipe_catalog() -> Vec<serde_json::Value> {
    let availability = if banking77_cookbook_root().is_ok() {
        "available"
    } else {
        "unavailable"
    };
    [
        (
            BANKING77_GEPA_SMOKE_RECIPE,
            "Banking77 GEPA · bounded smoke",
            "luna_med",
        ),
        (
            BANKING77_GEPA_LUNA_RECIPE,
            "Banking77 GEPA · Luna medium",
            "luna_med",
        ),
        (
            BANKING77_GEPA_SOL_RECIPE,
            "Banking77 GEPA · Sol medium",
            "sol_med",
        ),
    ]
    .into_iter()
    .map(|(id, title, config)| {
        json!({
            "id": id,
            "title": title,
            "algorithmId": "gepa",
            "task": "banking77",
            "availability": availability,
            "limits": recipe_limits(),
            "policyRef": { "harness": "gepa_proposer", "config": config },
            "credentialInputs": [],
        })
    })
    .collect()
}

pub(super) async fn reconcile_persisted(
    service: &OptimizerService,
    run_id: &str,
) -> Result<super::models::OptimizerRunRecord> {
    let run = service.get(run_id.to_string()).await?;
    if !matches!(run.status.as_str(), "completed" | "failed" | "cancelled")
        || run
            .summary
            .get("recipeId")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|recipe_id| ProposerProfile::for_recipe(recipe_id).is_err())
    {
        return Ok(run);
    }
    let Some(run_dir) = run
        .summary
        .get("runDirectory")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
    else {
        return Ok(run);
    };
    let existing = service
        .events_after(run_id.to_string(), 0, Some(2_000))
        .await?;
    let event_path = run_dir.join("events.jsonl");
    if event_path.is_file() {
        let text = fs::read_to_string(&event_path).context("read persisted Banking77 events")?;
        let raw = text
            .lines()
            .enumerate()
            .filter_map(|(index, line)| {
                let mut value: serde_json::Value = serde_json::from_str(line).ok()?;
                if let Some(object) = value.as_object_mut() {
                    object
                        .entry("_seq")
                        .or_insert_with(|| json!(index as u64 + 2));
                }
                Some(value)
            })
            .collect::<Vec<_>>();
        let existing_ids = existing
            .iter()
            .filter_map(|event| event.event_id.as_deref())
            .collect::<std::collections::HashSet<_>>();
        let mut next_sequence = run.cursor_seq;
        let mut missing = normalize::normalize_events(&raw, run_id, "gepa")
            .into_iter()
            .filter(|event| {
                event
                    .event_id
                    .as_deref()
                    .is_none_or(|event_id| !existing_ids.contains(event_id))
            })
            .collect::<Vec<_>>();
        for event in &mut missing {
            next_sequence += 1;
            event.sequence_number = next_sequence;
        }
        if !missing.is_empty() {
            service.append_events(run_id.to_string(), missing).await?;
        }
        let has_artifact_event = existing
            .iter()
            .any(|event| event.event_type == "optimizer.recipe.artifacts");
        if !has_artifact_event && run.status == "completed" {
            append_recipe_artifacts(service, run_id, &run_dir).await?;
        }
    }
    let has_candidate_payloads = existing
        .iter()
        .any(|event| event.event_type == "candidate.artifact.loaded");
    if run.status == "completed" && !has_candidate_payloads {
        append_recipe_candidates(service, run_id, &run_dir).await?;
    }
    let has_proposer_transcripts = existing
        .iter()
        .any(|event| event.event_type == "proposer.transcript.loaded");
    if run.status == "completed" && !has_proposer_transcripts {
        append_proposer_transcripts(service, run_id, &run_dir).await?;
    }
    let has_rich_diagnostic = existing.iter().any(|event| {
        event.event_type == "optimizer.recipe.diagnostic"
            && event
                .error
                .as_ref()
                .and_then(|error| error.get("stderrTail"))
                .and_then(serde_json::Value::as_str)
                .is_some()
    });
    if run.status == "failed" && !has_rich_diagnostic {
        append_diagnostic_event(
            service,
            run_id,
            "The local optimizer recipe failed; inspect the bounded stderr tail below.".into(),
        )
        .await?;
    }
    service.get(run_id.to_string()).await
}

fn recipe_run_id(proposer: ProposerProfile) -> String {
    let run_suffix = uuid::Uuid::new_v4().simple().to_string();
    format!(
        "banking77_gepa_{}_{}",
        proposer.config_id(),
        &run_suffix[..8]
    )
}

fn recipe_limits() -> serde_json::Value {
    json!({
        "maxGenerations": MAX_GENERATIONS,
        "proposalsPerGeneration": PROPOSALS_PER_GENERATION,
        "minibatchSize": MINIBATCH_SIZE,
        "maxTrainRollouts": MAX_TRAIN_ROLLOUTS,
        "maxHeldoutRollouts": MAX_HELDOUT_ROLLOUTS,
        "maxTotalRollouts": MAX_TOTAL_ROLLOUTS,
        "maxCostUsd": MAX_COST_USD,
        "proposerEstimatedCostUsd": PROPOSER_ESTIMATED_COST_USD,
        "rolloutEstimatedCostUsd": ROLLOUT_ESTIMATED_COST_USD,
    })
}

async fn run_recipe_worker(
    service: OptimizerService,
    run_id: String,
    cookbook: PathBuf,
    config_path: PathBuf,
    run_dir: PathBuf,
    manager: Arc<super::OptimizerManager>,
    mut cancel_rx: watch::Receiver<bool>,
) -> Result<()> {
    append_status_event(&service, &run_id, "optimizer.run.started", "running").await?;
    let openai_api_key = resolve_secret("OPENAI_API_KEY")?;
    let stdout = fs::File::create(run_dir.join("workshop.stdout.log"))?;
    let stderr = fs::File::create(run_dir.join("workshop.stderr.log"))?;
    let mut child = manager
        .spawn_gepa_recipe(
            &run_id,
            &cookbook,
            &config_path,
            stdout,
            stderr,
            &openai_api_key,
        )
        .await?;

    let mut upstream_cursor = 0;
    loop {
        if let Err(error) =
            ingest_available(&service, &manager, &run_id, &mut upstream_cursor).await
        {
            // The producer registers its durable index shortly after spawn.
            // A 404 is retryable only while the child is demonstrably alive;
            // it is not a successful empty event page.
            if !super::OptimizerManager::optimizer_run_not_indexed(&error) {
                return Err(error);
            }
        }
        tokio::select! {
            status = child.wait() => {
                let status = status.context("wait for Banking77 GEPA process")?;
                ingest_available(&service, &manager, &run_id, &mut upstream_cursor).await?;
                if !status.success() {
                    bail!("Banking77 GEPA exited with {status}; see {}", run_dir.join("workshop.stderr.log").display());
                }
                append_recipe_artifacts(&service, &run_id, &run_dir).await?;
                append_recipe_candidates(&service, &run_id, &run_dir).await?;
                append_proposer_transcripts(&service, &run_id, &run_dir).await?;
                append_terminal_event(&service, &run_id, false, "recipe process completed".into()).await?;
                return Ok(());
            }
            changed = cancel_rx.changed() => {
                if changed.is_ok() && *cancel_rx.borrow() {
                    manager.terminate_gepa_recipe(&run_id).await;
                    if child.try_wait()?.is_none() {
                        child.kill().await.context("cancel Banking77 GEPA process")?;
                    }
                    append_status_event(&service, &run_id, "optimizer.run.cancelled", "cancelled").await?;
                    return Ok(());
                }
            }
            _ = sleep(Duration::from_millis(750)) => {
                // A proposer generation seals its app-server artifacts before
                // the optimizer run finishes. Reconcile those artifacts while
                // the run is live so the right-panel Trace V5 viewer does not
                // have to wait for terminal state. Deterministic event ids make
                // repeated polls and reconnects idempotent.
                if let Err(error) = append_proposer_transcripts(&service, &run_id, &run_dir).await {
                    eprintln!("transient proposer transcript reconciliation failure: {error:#}");
                }
            }
        }
    }
}

/// Resolve a recipe secret exclusively inside the Rust host. Finder-launched
/// applications do not inherit shell variables, so consult a small trusted
/// file allowlist after checking the process environment. Values are returned
/// only to the child environment and are never persisted or logged.
fn resolve_secret(name: &str) -> Result<String> {
    if name != "OPENAI_API_KEY" {
        bail!("optimizer recipe requested a non-allowlisted secret name");
    }
    if let Ok(value) = std::env::var(name) {
        if !value.trim().is_empty() {
            return Ok(value);
        }
    }
    let candidates = std::env::var_os("SYNTH_BANKING77_SECRET_ENV_FILE")
        .map(PathBuf::from)
        .into_iter();
    for path in candidates {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        if let Some(value) = dotenv_value(&text, name) {
            return Ok(value);
        }
    }
    bail!(
        "Banking77 GEPA requires {name}; configure it in the Desktop process or a trusted recipe env file"
    )
}

fn dotenv_value(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
        if line.starts_with('#') || line.is_empty() {
            return None;
        }
        let (key, raw) = line.split_once('=')?;
        if key.trim() != name {
            return None;
        }
        let value = raw.trim();
        let value = if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            &value[1..value.len() - 1]
        } else {
            value
        };
        (!value.is_empty()).then(|| value.to_string())
    })
}

async fn ingest_available(
    service: &OptimizerService,
    manager: &super::OptimizerManager,
    run_id: &str,
    upstream_cursor: &mut u64,
) -> Result<()> {
    let page = manager
        .optimizer_events_after(run_id, *upstream_cursor, 500)
        .await?;
    super::ingest::ingest_event_page(service, run_id, "gepa", &page, upstream_cursor).await?;
    Ok(())
}

async fn append_recipe_artifacts(
    service: &OptimizerService,
    run_id: &str,
    run_dir: &Path,
) -> Result<()> {
    let artifacts = [
        ("candidate", "best_candidate.json", "Best candidate"),
        ("manifest", "result_manifest.json", "Result manifest"),
        ("log", "workshop.stdout.log", "Process stdout"),
        ("log", "workshop.stderr.log", "Process stderr"),
    ]
    .into_iter()
    .filter_map(|(kind, name, title)| {
        let path = run_dir.join(name);
        path.is_file().then(|| {
            json!({
                "kind": kind,
                "id": path,
                "path": path,
                "title": title,
            })
        })
    })
    .collect::<Vec<_>>();
    if artifacts.is_empty() {
        return Ok(());
    }
    let run = service.get(run_id.to_string()).await?;
    let event = OptimizerEventEnvelope {
        schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
        event_id: Some(format!("{run_id}:workshop:artifacts")),
        event_type: "optimizer.recipe.artifacts".into(),
        sequence_number: run.cursor_seq + 1,
        occurred_at: chrono::Utc::now().to_rfc3339(),
        optimizer_run_id: run_id.into(),
        algorithm_id: "gepa".into(),
        level: Some("info".into()),
        item: None,
        delta: serde_json::from_value(json!({
            "message": format!("Persisted {} optimizer artifacts", artifacts.len()),
        }))?,
        snapshot: None,
        usage_delta: None,
        artifact_refs: artifacts,
        error: None,
        raw: json!({ "source": "workshop_recipe" }),
    };
    service
        .append_events(run_id.to_string(), vec![event])
        .await?;
    Ok(())
}

async fn append_recipe_candidates(
    service: &OptimizerService,
    run_id: &str,
    run_dir: &Path,
) -> Result<()> {
    let registry_path = run_dir.join("candidate_registry.json");
    if !registry_path.is_file() {
        return Ok(());
    }
    let registry: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&registry_path)
            .with_context(|| format!("read candidate registry {}", registry_path.display()))?,
    )?;
    let Some(candidates) = registry.as_array() else {
        return Ok(());
    };
    let run = service.get(run_id.to_string()).await?;
    let mut events = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let Some(candidate_id) = candidate
            .get("candidate_id")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let values = candidate
            .get("payload")
            .or_else(|| candidate.pointer("/lever_bundle/values"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let status = candidate
            .get("status")
            .cloned()
            .unwrap_or_else(|| json!("evaluated"));
        let sequence_number = run.cursor_seq + events.len() as u64 + 1;
        let mut delta = serde_json::Map::new();
        for key in ["train_reward", "heldout_reward", "minibatch_reward"] {
            if let Some(value) = candidate.get(key) {
                delta.insert(key.into(), value.clone());
            }
        }
        if let Some(parent_id) = candidate.get("parent_id") {
            delta.insert("parentId".into(), parent_id.clone());
        }
        events.push(OptimizerEventEnvelope {
            schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: Some(format!("{run_id}:candidate-artifact:{candidate_id}")),
            event_type: "candidate.artifact.loaded".into(),
            sequence_number,
            occurred_at: chrono::Utc::now().to_rfc3339(),
            optimizer_run_id: run_id.into(),
            algorithm_id: "gepa".into(),
            level: Some("info".into()),
            item: Some(json!({
                "kind": "candidate",
                "id": candidate_id,
                "status": status,
                "raw": {
                    "values": values,
                    "sourceArtifact": "candidate_registry.json"
                }
            })),
            delta,
            snapshot: None,
            usage_delta: None,
            artifact_refs: vec![json!({
                "kind": "candidate_registry",
                "id": registry_path,
                "path": registry_path,
                "title": "Candidate registry"
            })],
            error: None,
            raw: json!({ "source": "candidate_registry.json", "index": index }),
        });
    }
    if !events.is_empty() {
        service.append_events(run_id.to_string(), events).await?;
    }
    Ok(())
}

fn truncated_text(value: &serde_json::Value, max_chars: usize) -> serde_json::Value {
    match value.as_str() {
        Some(text) if text.chars().count() > max_chars => {
            let cut: String = text.chars().take(max_chars).collect();
            json!({ "text": cut, "truncated": true, "total_chars": text.chars().count() })
        }
        Some(text) => json!({ "text": text, "truncated": false }),
        None => json!({ "text": serde_json::Value::Null, "truncated": false }),
    }
}

fn string_list(
    value: Option<&serde_json::Value>,
    max_items: usize,
    max_chars: usize,
) -> Vec<serde_json::Value> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(max_items)
                .map(|item| truncated_text(item, max_chars))
                .collect()
        })
        .unwrap_or_default()
}

fn bounded_trace_text(value: &serde_json::Value, max_chars: usize) -> String {
    let raw = value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| serde_json::to_string_pretty(value).unwrap_or_default());
    if raw.chars().count() <= max_chars {
        return raw;
    }
    let head = raw.chars().take(max_chars).collect::<String>();
    format!(
        "{head}\n… truncated in projection ({} chars; sealed artifact retains the complete value)",
        raw.chars().count()
    )
}

fn project_trace_v5_items(source: &str) -> Vec<serde_json::Value> {
    let mut items = Vec::new();
    for line in source.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(envelope) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if envelope.get("method").and_then(serde_json::Value::as_str) != Some("item/completed") {
            continue;
        }
        let item = envelope
            .get("params")
            .and_then(|params| params.get("item"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let item_type = item
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let occurred_at = envelope
            .get("emittedAtMs")
            .and_then(serde_json::Value::as_i64)
            .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
            .map(|value| value.to_rfc3339());
        let id = item
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("trace-item-{}", items.len() + 1));
        let sequence = items.len() + 1;
        let projected = match item_type {
            "userMessage" => {
                let body = item
                    .get("content")
                    .and_then(serde_json::Value::as_array)
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|part| part.get("text").and_then(serde_json::Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                Some(json!({ "id": id, "sequence": sequence, "family": "input", "kind": "message.input", "title": "GEPA proposer request", "occurredAt": occurred_at, "body": bounded_trace_text(&json!(body), 20_000) }))
            }
            "agentMessage" => item.get("text").and_then(serde_json::Value::as_str).map(|body| {
                let final_answer = item.get("phase").and_then(serde_json::Value::as_str) == Some("final_answer");
                json!({ "id": id, "sequence": sequence, "family": if final_answer { "output" } else { "thinking" }, "kind": if final_answer { "message.output" } else { "reasoning.summary" }, "title": if final_answer { "Proposer response" } else { "Reasoning summary" }, "occurredAt": occurred_at, "body": bounded_trace_text(&json!(body), 20_000) })
            }),
            "commandExecution" => {
                let exit_code = item.get("exitCode").and_then(serde_json::Value::as_i64);
                Some(json!({ "id": id, "sequence": sequence, "family": "tool", "kind": "tool.shell", "title": "Run shell command", "occurredAt": occurred_at, "body": bounded_trace_text(item.get("command").unwrap_or(&serde_json::Value::Null), 20_000), "detail": bounded_trace_text(item.get("aggregatedOutput").unwrap_or(&serde_json::Value::Null), 20_000), "status": if exit_code == Some(0) { "completed".into() } else { format!("exit {}", exit_code.map_or_else(|| "?".into(), |value| value.to_string())) } }))
            }
            "fileChange" => {
                let changes = item
                    .get("changes")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let title = changes
                    .iter()
                    .filter_map(|change| change.get("path").and_then(serde_json::Value::as_str))
                    .collect::<Vec<_>>()
                    .join(", ");
                let detail = changes
                    .iter()
                    .map(|change| format!("{} {}\n{}", change.pointer("/kind/type").and_then(serde_json::Value::as_str).unwrap_or("change"), change.get("path").and_then(serde_json::Value::as_str).unwrap_or_default(), change.get("diff").and_then(serde_json::Value::as_str).unwrap_or_default()))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                Some(json!({ "id": id, "sequence": sequence, "family": "artifact", "kind": "tool.file_change", "title": if title.is_empty() { "Workspace file change" } else { &title }, "occurredAt": occurred_at, "detail": bounded_trace_text(&json!(detail), 20_000) }))
            }
            _ => None,
        };
        if let Some(projected) = projected {
            items.push(projected);
        }
    }
    items
}

/// Backfill `proposer.transcript.loaded` events from the proposer workspace
/// artifacts of a completed run, so the trace viewer can show the reflection
/// narrative (critique, evidence, rationale, proposals) without reading the
/// filesystem. Live producers stream the same content as `proposer.delta`
/// chunks; this is the durable-reopen path.
async fn append_proposer_transcripts(
    service: &OptimizerService,
    run_id: &str,
    run_dir: &Path,
) -> Result<()> {
    let workspaces_dir = run_dir.join("proposer_workspaces");
    if !workspaces_dir.is_dir() {
        return Ok(());
    }
    let mut generation_dirs: Vec<_> = fs::read_dir(&workspaces_dir)
        .with_context(|| format!("read {}", workspaces_dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("generation_"))
        })
        .collect();
    generation_dirs.sort();
    let run = service.get(run_id.to_string()).await?;
    let mut events = Vec::new();
    for dir in generation_dirs {
        let generation: u64 = dir
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("generation_"))
            .and_then(|digits| digits.parse().ok())
            .unwrap_or(0);
        let response_path = dir.join(".agent_artifacts").join("opencode_response.json");
        if !response_path.is_file() {
            continue;
        }
        let response: serde_json::Value = match fs::read_to_string(&response_path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
        {
            Some(value) => value,
            None => continue,
        };
        let manifest = response
            .get("manifest")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let evidence = manifest
            .get("evidence")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let proposals = response
            .get("proposals")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .take(8)
                    .map(|proposal| {
                        json!({
                            "proposal_type": proposal.get("proposal_type"),
                            "parent_candidate_ids": proposal.get("parent_candidate_ids"),
                            "rationale": truncated_text(
                                proposal.get("rationale").unwrap_or(&serde_json::Value::Null),
                                4_000
                            ),
                            "proposed_payload": truncated_text(
                                proposal.get("proposed_payload").unwrap_or(&serde_json::Value::Null),
                                6_000
                            ),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let transcript_event_id = format!("{run_id}:proposer-transcript:{generation}");
        if !service
            .has_event_id(run_id.to_string(), transcript_event_id.clone())
            .await?
        {
            let sequence_number = run.cursor_seq + events.len() as u64 + 1;
            events.push(OptimizerEventEnvelope {
                schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
                event_id: Some(transcript_event_id),
                event_type: "proposer.transcript.loaded".into(),
                sequence_number,
                occurred_at: chrono::Utc::now().to_rfc3339(),
                optimizer_run_id: run_id.into(),
                algorithm_id: "gepa".into(),
                level: Some("info".into()),
                item: None,
                delta: serde_json::from_value(json!({
                    "generation": generation,
                    "message": "Proposer transcript reconciled from workspace artifacts",
                    "critique": truncated_text(
                        manifest.get("critique").unwrap_or(&serde_json::Value::Null),
                        4_000
                    ),
                    "rationale": truncated_text(
                        manifest.get("rationale").unwrap_or(&serde_json::Value::Null),
                        4_000
                    ),
                    "failure_patterns": string_list(evidence.get("failure_patterns"), 12, 1_000),
                    "winning_patterns": string_list(evidence.get("winning_patterns"), 12, 1_000),
                    "candidate_comparison": truncated_text(
                        evidence.get("candidate_comparison").unwrap_or(&serde_json::Value::Null),
                        2_000
                    ),
                    "proposals": proposals,
                    "usage": response.get("usage"),
                }))?,
                snapshot: None,
                usage_delta: None,
                artifact_refs: vec![json!({
                    "kind": "proposer_transcript",
                    "id": response_path,
                    "path": response_path,
                    "title": format!("Proposer transcript · generation {generation}")
                })],
                error: None,
                raw: json!({ "source": "opencode_response.json", "generation": generation }),
            });
        }
        let trace_path = dir
            .join(".agent_artifacts")
            .join("opencode_sse_events.jsonl");
        let trace_event_id = format!("{run_id}:proposer-trace-v5:{generation}");
        if trace_path.is_file()
            && !service
                .has_event_id(run_id.to_string(), trace_event_id.clone())
                .await?
        {
            let items = fs::read_to_string(&trace_path)
                .ok()
                .map(|source| project_trace_v5_items(&source))
                .unwrap_or_default();
            if !items.is_empty() {
                let sequence_number = run.cursor_seq + events.len() as u64 + 1;
                events.push(OptimizerEventEnvelope {
                    schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
                    event_id: Some(trace_event_id),
                    event_type: "proposer.trace_v5.loaded".into(),
                    sequence_number,
                    occurred_at: chrono::Utc::now().to_rfc3339(),
                    optimizer_run_id: run_id.into(),
                    algorithm_id: "gepa".into(),
                    level: Some("info".into()),
                    item: None,
                    delta: serde_json::from_value(json!({
                        "generation": generation,
                        "schema_version": "synth.trace-projection.rollout-inspector.v1",
                        "message": "Sealed proposer Trace V5 reconciled from app-server artifacts",
                        "items": items,
                    }))?,
                    snapshot: None,
                    usage_delta: None,
                    artifact_refs: vec![json!({
                        "kind": "trace_v5",
                        "id": trace_path,
                        "path": trace_path,
                        "title": format!("Proposer Trace V5 · generation {generation}")
                    })],
                    error: None,
                    raw: json!({ "source": "opencode_sse_events.jsonl", "generation": generation }),
                });
            }
        }
    }
    if !events.is_empty() {
        service.append_events(run_id.to_string(), events).await?;
    }
    Ok(())
}

async fn append_status_event(
    service: &OptimizerService,
    run_id: &str,
    event_type: &str,
    status: &str,
) -> Result<()> {
    let run = service.get(run_id.to_string()).await?;
    let event = OptimizerEventEnvelope {
        schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
        event_id: Some(format!("{run_id}:workshop:{}", run.cursor_seq + 1)),
        event_type: event_type.into(),
        sequence_number: run.cursor_seq + 1,
        occurred_at: chrono::Utc::now().to_rfc3339(),
        optimizer_run_id: run_id.into(),
        algorithm_id: "gepa".into(),
        level: None,
        item: None,
        delta: serde_json::from_value(json!({ "status": status }))?,
        snapshot: None,
        usage_delta: None,
        artifact_refs: vec![],
        error: None,
        raw: json!({ "source": "workshop_recipe" }),
    };
    service
        .append_events(run_id.to_string(), vec![event])
        .await?;
    Ok(())
}

async fn append_terminal_event(
    service: &OptimizerService,
    run_id: &str,
    failed: bool,
    detail: String,
) -> Result<()> {
    let run = service.get(run_id.to_string()).await?;
    if matches!(run.status.as_str(), "completed" | "failed" | "cancelled") {
        return Ok(());
    }
    let status = if failed { "failed" } else { "completed" };
    append_status_event(
        service,
        run_id,
        if failed {
            "optimizer.run.failed"
        } else {
            "optimizer.run.completed"
        },
        status,
    )
    .await?;
    if failed {
        append_diagnostic_event(service, run_id, detail).await?;
    }
    Ok(())
}

async fn append_diagnostic_event(
    service: &OptimizerService,
    run_id: &str,
    detail: String,
) -> Result<()> {
    // Preserve a bounded diagnostic in the run summary via an error event.
    let run = service.get(run_id.to_string()).await?;
    let run_directory = run
        .summary
        .get("runDirectory")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let stderr_path = run_directory
        .as_ref()
        .map(|directory| directory.join("workshop.stderr.log"));
    let stderr_tail = stderr_path
        .as_ref()
        .and_then(|path| bounded_log_tail(path, 4_000).ok())
        .filter(|text| !text.trim().is_empty());
    let display_message = stderr_tail.as_deref().unwrap_or(&detail);
    let mut event = OptimizerEventEnvelope {
        schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
        event_id: Some(format!("{run_id}:workshop:{}", run.cursor_seq + 1)),
        event_type: "optimizer.recipe.diagnostic".into(),
        sequence_number: run.cursor_seq + 1,
        occurred_at: chrono::Utc::now().to_rfc3339(),
        optimizer_run_id: run_id.into(),
        algorithm_id: "gepa".into(),
        level: Some("error".into()),
        item: None,
        delta: Default::default(),
        snapshot: None,
        usage_delta: None,
        artifact_refs: vec![],
        error: Some(json!({
            "message": display_message.chars().take(1_000).collect::<String>(),
            "stderrTail": stderr_tail,
            "logPath": stderr_path,
        })),
        raw: json!({}),
    };
    event.delta.insert("status".into(), json!("failed"));
    service
        .append_events(run_id.to_string(), vec![event])
        .await?;
    Ok(())
}

fn bounded_log_tail(path: &Path, max_chars: usize) -> Result<String> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("read optimizer diagnostic log {}", path.display()))?;
    let start = text
        .char_indices()
        .rev()
        .nth(max_chars.saturating_sub(1))
        .map(|(index, _)| index)
        .unwrap_or(0);
    Ok(text[start..].to_string())
}

fn banking77_cookbook_root() -> Result<PathBuf> {
    let path = std::env::var_os("SYNTH_BANKING77_GEPA_COOKBOOK_ROOT")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("SYNTH_BANKING77_GEPA_COOKBOOK_ROOT is not configured"))?;
    let path = path.canonicalize().unwrap_or(path);
    if !path.join("gepa.toml").is_file() || !path.join("synth_service_app.py").is_file() {
        bail!(
            "Banking77 GEPA cookbook is unavailable; set SYNTH_BANKING77_GEPA_COOKBOOK_ROOT in the Desktop process"
        );
    }
    Ok(path)
}

fn reserve_loopback_port() -> Result<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

fn resolve_uv() -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("SYNTH_OPTIMIZER_UV_PATH") {
        candidates.push(PathBuf::from(path));
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/uv"),
        PathBuf::from("/usr/local/bin/uv"),
    ]);
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin/uv"));
        candidates.push(home.join(".cargo/bin/uv"));
    }
    for path in candidates {
        if path.is_file() {
            return path
                .canonicalize()
                .with_context(|| format!("canonicalize trusted uv path {}", path.display()));
        }
    }
    bail!(
        "Banking77 GEPA requires uv; install it or set SYNTH_OPTIMIZER_UV_PATH in the Desktop process"
    )
}

fn resolve_codex_home() -> Result<PathBuf> {
    let path = std::env::var_os("SYNTH_OPTIMIZER_CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
        .ok_or_else(|| anyhow!("cannot resolve Codex home"))?;
    let path = path.canonicalize().unwrap_or(path);
    if !path.join("auth.json").is_file() {
        bail!(
            "Banking77 GEPA Luna/Sol requires ChatGPT auth; sign in to Codex or set SYNTH_OPTIMIZER_CODEX_HOME"
        );
    }
    Ok(path)
}

fn materialize_config(
    cookbook: &Path,
    runs_root: &Path,
    run_id: &str,
    port: u16,
    uv: &Path,
    destination: &Path,
    proposer_profile: ProposerProfile,
    codex_home: &Path,
) -> Result<()> {
    let source = fs::read_to_string(cookbook.join("gepa.toml"))?;
    let mut config: toml::Value = toml::from_str(&source)?;
    let run = table_mut(&mut config, "run")?;
    run.insert("run_id".into(), toml::Value::String(run_id.into()));
    run.insert(
        "output_dir".into(),
        toml::Value::String(runs_root.display().to_string()),
    );

    let container = table_mut(&mut config, "container")?;
    // The first Banking77 launch may need to create the isolated uv environment
    // and load the Hugging Face dataset before it can bind its health endpoint.
    // Keep the runtime's default 30s deadline for warm services, but give this
    // explicitly managed cold-start path enough room to become ready.
    container.insert("startup_timeout_seconds".into(), 120.into());
    let container_stream_root = destination
        .parent()
        .ok_or_else(|| anyhow!("generated recipe destination has no run directory"))?
        .join("container-streams");
    container.insert(
        "url".into(),
        toml::Value::String(format!("http://127.0.0.1:{port}")),
    );
    container.insert(
        "cwd".into(),
        toml::Value::String(
            cookbook
                .parent()
                .ok_or_else(|| anyhow!("invalid cookbook path"))?
                .display()
                .to_string(),
        ),
    );
    container.insert(
        "command".into(),
        toml::Value::Array(
            vec![
                "/usr/bin/env",
                "BANKING77_TRAIN_SAMPLE=50",
                "BANKING77_TEST_SAMPLE=50",
                "BANKING77_POLICY_CONCURRENCY=4",
                "BANKING77_POLICY_TIMEOUT_SECONDS=20",
                "BANKING77_ROLLOUT_TIMEOUT_SECONDS=25",
                &format!("BANKING77_STREAM_ROOT={}", container_stream_root.display()),
                "HF_HUB_DISABLE_PROGRESS_BARS=1",
                &uv.display().to_string(),
                "run",
                "--project",
                "banking77_container",
                "python",
                "banking77_container/synth_service_app.py",
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
            ]
            .into_iter()
            .map(|v| toml::Value::String(v.into()))
            .collect(),
        ),
    );

    let dataset = table_mut(&mut config, "dataset")?;
    dataset.insert("train_seeds".into(), integer_array(0, TRAIN_ROWS));
    dataset.insert("heldout_seeds".into(), integer_array(100, HELDOUT_ROWS));

    let taskset = [
        ("train_split".into(), toml::Value::String("train".into())),
        ("heldout_split".into(), toml::Value::String("test".into())),
        (
            "train_ids".into(),
            string_array((0..TRAIN_ROWS).map(|seed| format!("train:{seed}"))),
        ),
        (
            "heldout_ids".into(),
            string_array((100..100 + HELDOUT_ROWS).map(|seed| format!("test:{seed}"))),
        ),
    ]
    .into_iter()
    .collect();
    config
        .as_table_mut()
        .ok_or_else(|| anyhow!("Banking77 base config must be a TOML table"))?
        .insert("taskset".into(), toml::Value::Table(taskset));

    let gepa = table_mut(&mut config, "gepa")?;
    gepa.insert("max_generations".into(), MAX_GENERATIONS.into());
    gepa.insert(
        "proposals_per_generation".into(),
        PROPOSALS_PER_GENERATION.into(),
    );
    gepa.insert("minibatch_size".into(), MINIBATCH_SIZE.into());
    gepa.insert("max_train_rollouts".into(), MAX_TRAIN_ROLLOUTS.into());
    gepa.insert("max_heldout_rollouts".into(), MAX_HELDOUT_ROLLOUTS.into());
    gepa.insert("max_total_rollouts".into(), MAX_TOTAL_ROLLOUTS.into());
    gepa.insert("max_cost_usd".into(), MAX_COST_USD.into());
    gepa.insert(
        "proposer_estimated_cost_usd".into(),
        PROPOSER_ESTIMATED_COST_USD.into(),
    );
    gepa.insert(
        "rollout_estimated_cost_usd".into(),
        ROLLOUT_ESTIMATED_COST_USD.into(),
    );
    let task_pools = [
        (
            "pareto".into(),
            string_array((0..TRAIN_ROWS).map(|seed| format!("train:{seed}"))),
        ),
        (
            "minibatch".into(),
            string_array((0..MINIBATCH_SIZE as usize).map(|seed| format!("train:{seed}"))),
        ),
        (
            "reflection".into(),
            string_array((0..TRAIN_ROWS).map(|seed| format!("train:{seed}"))),
        ),
        (
            "heldout".into(),
            string_array((100..100 + HELDOUT_ROWS).map(|seed| format!("test:{seed}"))),
        ),
    ]
    .into_iter()
    .collect();
    gepa.insert("task_pools".into(), toml::Value::Table(task_pools));

    let cache = table_mut(&mut config, "cache")?;
    cache.insert("mode".into(), toml::Value::String("off".into()));
    cache.insert("path".into(), toml::Value::String(String::new()));
    cache.insert("namespace".into(), toml::Value::String(run_id.into()));

    let proposer = table_mut(&mut config, "proposer")?;
    proposer.insert(
        "model".into(),
        toml::Value::String(proposer_profile.model().into()),
    );
    proposer.insert(
        "reasoning_effort".into(),
        toml::Value::String("medium".into()),
    );
    proposer.insert(
        "timeout_seconds".into(),
        toml::Value::Integer(PROPOSER_TIMEOUT_SECONDS),
    );
    proposer.insert(
        "message_stall_timeout_seconds".into(),
        toml::Value::Integer(PROPOSER_MESSAGE_STALL_TIMEOUT_SECONDS),
    );
    proposer.insert("auth_mode".into(), toml::Value::String("chatgpt".into()));
    proposer.insert("copy_host_auth".into(), toml::Value::Boolean(true));
    proposer.remove("api_key_env");
    proposer.insert(
        "codex_home".into(),
        toml::Value::String(codex_home.display().to_string()),
    );

    validate_limits(&config)?;
    fs::write(destination, toml::to_string_pretty(&config)?)?;
    Ok(())
}

fn table_mut<'a>(config: &'a mut toml::Value, key: &str) -> Result<&'a mut toml::value::Table> {
    config
        .get_mut(key)
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| anyhow!("Banking77 base config missing [{key}]"))
}

fn integer_array(start: usize, count: usize) -> toml::Value {
    toml::Value::Array(
        (start..start + count)
            .map(|value| toml::Value::Integer(value as i64))
            .collect(),
    )
}

fn string_array(values: impl IntoIterator<Item = String>) -> toml::Value {
    toml::Value::Array(values.into_iter().map(toml::Value::String).collect())
}

fn validate_limits(config: &toml::Value) -> Result<()> {
    let gepa = config
        .get("gepa")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| anyhow!("generated recipe missing [gepa]"))?;
    let integer = |key: &str| {
        gepa.get(key)
            .and_then(toml::Value::as_integer)
            .ok_or_else(|| anyhow!("generated recipe missing gepa.{key}"))
    };
    if integer("max_generations")? > MAX_GENERATIONS
        || integer("proposals_per_generation")? > PROPOSALS_PER_GENERATION
        || integer("max_train_rollouts")? > MAX_TRAIN_ROLLOUTS
        || integer("max_heldout_rollouts")? > MAX_HELDOUT_ROLLOUTS
        || integer("max_total_rollouts")? > MAX_TOTAL_ROLLOUTS
    {
        bail!("generated Banking77 GEPA recipe exceeds hard rollout bounds");
    }
    let cost = gepa
        .get("max_cost_usd")
        .and_then(|value| {
            value
                .as_float()
                .or_else(|| value.as_integer().map(|v| v as f64))
        })
        .ok_or_else(|| anyhow!("generated recipe missing gepa.max_cost_usd"))?;
    if !(0.0..=MAX_COST_USD).contains(&cost) {
        bail!("generated Banking77 GEPA recipe exceeds hard cost bound");
    }
    let proposer_cost = gepa
        .get("proposer_estimated_cost_usd")
        .and_then(|value| {
            value
                .as_float()
                .or_else(|| value.as_integer().map(|v| v as f64))
        })
        .ok_or_else(|| anyhow!("generated recipe missing gepa.proposer_estimated_cost_usd"))?;
    if proposer_cost <= 0.0 || proposer_cost > MAX_COST_USD {
        bail!("generated Banking77 GEPA recipe has an invalid proposer cost estimate");
    }
    let rollout_cost = gepa
        .get("rollout_estimated_cost_usd")
        .and_then(|value| {
            value
                .as_float()
                .or_else(|| value.as_integer().map(|v| v as f64))
        })
        .ok_or_else(|| anyhow!("generated recipe missing gepa.rollout_estimated_cost_usd"))?;
    if rollout_cost <= 0.0 || rollout_cost > MAX_COST_USD {
        bail!("generated Banking77 GEPA recipe has an invalid rollout cost estimate");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{ContentStore, EventJournal, Storage};
    use crate::visuals::VisualRegistry;
    use tempfile::tempdir;

    #[test]
    fn sealed_app_server_events_project_to_trace_v5_without_invented_reasoning() {
        let source = [
            json!({"method":"item/started","params":{"item":{"id":"ignored","type":"commandExecution"}}}),
            json!({"method":"item/completed","emittedAtMs":1_786_639_200_000i64,"params":{"item":{"id":"input","type":"userMessage","content":[{"type":"input_text","text":"Improve this prompt"}]}}}),
            json!({"method":"item/completed","params":{"item":{"id":"thought","type":"agentMessage","phase":"commentary","text":"I will inspect the failures."}}}),
            json!({"method":"item/completed","params":{"item":{"id":"tool","type":"commandExecution","command":"python analyze.py","aggregatedOutput":"three clusters","exitCode":0}}}),
            json!({"method":"item/completed","params":{"item":{"id":"file","type":"fileChange","changes":[{"path":"proposal/manifest.json","kind":{"type":"create"},"diff":"+candidate"}]}}}),
            json!({"method":"item/completed","params":{"item":{"id":"final","type":"agentMessage","phase":"final_answer","text":"Created three candidates."}}}),
            json!({"method":"item/completed","params":{"item":{"id":"hidden","type":"reasoning","summary":[]}}}),
        ]
        .into_iter()
        .map(|value| serde_json::to_string(&value).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
        let items = project_trace_v5_items(&source);
        assert_eq!(items.len(), 5);
        assert_eq!(items[0]["family"], "input");
        assert_eq!(items[1]["family"], "thinking");
        assert_eq!(items[2]["family"], "tool");
        assert_eq!(items[2]["detail"], "three clusters");
        assert_eq!(items[3]["kind"], "tool.file_change");
        assert_eq!(items[4]["family"], "output");
        assert!(items.iter().all(|item| item["id"] != "hidden"));
    }

    #[test]
    fn materialized_recipe_enforces_pinned_bounds_and_no_secret_values() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("gepa.toml"),
            r#"[run]
run_id = "base"
output_dir = "runs"
[container]
url = "http://127.0.0.1:8765"
command = []
cwd = "."
[dataset]
train_seeds = [0]
heldout_seeds = [1]
[proposer]
model = "gpt-5.4-nano"
auth_mode = "api_key"
api_key_env = "OPENAI_API_KEY"
[gepa]
max_generations = 99
proposals_per_generation = 99
minibatch_size = 99
max_total_rollouts = 9999
max_cost_usd = 99.0
[cache]
mode = "readwrite"
path = "secret"
namespace = "base"
"#,
        )
        .unwrap();
        fs::write(dir.path().join("synth_service_app.py"), "").unwrap();
        let runs = dir.path().join("runs");
        fs::create_dir_all(&runs).unwrap();
        let codex_home = dir.path().join("codex-home");
        fs::create_dir_all(&codex_home).unwrap();
        fs::write(codex_home.join("auth.json"), "{}").unwrap();
        let output = dir.path().join("recipe.toml");
        materialize_config(
            dir.path(),
            &runs,
            "test_run",
            23456,
            Path::new("/usr/bin/true"),
            &output,
            ProposerProfile::LunaMedium,
            &codex_home,
        )
        .unwrap();
        let text = fs::read_to_string(output).unwrap();
        let config: toml::Value = toml::from_str(&text).unwrap();
        validate_limits(&config).unwrap();
        assert_eq!(
            config["taskset"]["train_ids"].as_array().unwrap().len(),
            TRAIN_ROWS
        );
        assert_eq!(
            config["gepa"]["task_pools"]["minibatch"]
                .as_array()
                .unwrap()
                .len(),
            MINIBATCH_SIZE as usize
        );
        assert_eq!(
            config["taskset"]["heldout_ids"].as_array().unwrap().len(),
            HELDOUT_ROWS
        );
        assert!(text.contains("minibatch_size = 20"));
        assert!(text.contains("max_total_rollouts = 240"));
        assert!(text.contains("max_train_rollouts = 140"));
        assert!(text.contains("max_heldout_rollouts = 100"));
        assert!(text.contains("max_cost_usd = 2.45"));
        assert!(text.contains("proposer_estimated_cost_usd = 0.05"));
        assert!(text.contains("rollout_estimated_cost_usd = 0.01"));
        assert!(text.contains("BANKING77_TRAIN_SAMPLE=50"));
        assert!(text.contains("BANKING77_TEST_SAMPLE=50"));
        assert!(text.contains("startup_timeout_seconds = 120"));
        assert!(!text.contains("OPENAI_API_KEY="));
        assert!(!text.contains("secret"));
        assert!(text.contains("model = \"gpt-5.6-luna\""));
        assert!(text.contains("auth_mode = \"chatgpt\""));
        assert!(text.contains("timeout_seconds = 300"));
        assert!(text.contains("message_stall_timeout_seconds = 120"));
        assert!(!text.contains("api_key_env = \"OPENAI_API_KEY\""));
        assert!(text.contains("train_ids = ["));
        assert!(text.contains("\"train:0\""));
        assert!(text.contains("[gepa.task_pools]"));
        assert!(text.contains("\"test:100\""));
    }

    #[test]
    fn catalog_discloses_no_credential_inputs() {
        let catalog = recipe_catalog();
        assert_eq!(catalog.len(), 3);
        assert!(catalog
            .iter()
            .all(|recipe| recipe["credentialInputs"] == json!([])));
        assert!(catalog
            .iter()
            .all(|recipe| recipe["limits"]["maxCostUsd"] == json!(2.45)));
        assert_ne!(catalog[0]["policyRef"], catalog[1]["policyRef"]);
    }

    #[test]
    fn banking77_gepa_spawn_is_owned_by_optimizer_manager() {
        let source = include_str!("recipes.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("recipes.rs production source");
        assert!(!production.contains("manager.ensure_ready().await"));
        assert!(production.contains("require_plugin_ready"));
        assert!(production.contains("spawn_gepa_recipe"));
        assert!(production.contains("manager.pin_run"));
        let ready_at = production.find("require_plugin_ready").unwrap();
        let spawn_at = production.find("spawn_gepa_recipe").unwrap();
        assert!(
            ready_at < spawn_at,
            "GEPA recipes must require a ready plugin before manager spawn"
        );
        assert!(
            !production.contains("Command::new"),
            "recipes.rs must not launch a raw process; OptimizerManager owns spawn"
        );
        assert!(!production.contains("tokio::process::Command"));
        assert!(!production.contains("std::process::Command"));
        assert!(!production.contains("uv run --with synth-optimizers"));
        assert!(!production.contains("\"gepa\", \"run\""));
    }

    #[test]
    fn two_banking77_recipes_allocate_distinct_run_ids() {
        let luna = recipe_run_id(ProposerProfile::LunaMedium);
        let sol = recipe_run_id(ProposerProfile::SolMedium);
        assert!(luna.starts_with("banking77_gepa_luna_med_"));
        assert!(sol.starts_with("banking77_gepa_sol_med_"));
        assert_ne!(luna, sol);
    }

    #[tokio::test]
    async fn two_banking77_recipes_pin_distinct_spools_through_the_manager() {
        use super::super::models::OptimizerCreateRequest;
        use super::super::OptimizerManager;
        use std::sync::Arc;

        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path().join("core")).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        let content = ContentStore::new(storage.content_root());
        let visuals = VisualRegistry::new(storage.database().clone(), journal.clone(), content);
        let (events_tx, _) = tokio::sync::broadcast::channel(16);
        let manager = Arc::new(OptimizerManager::with_home(dir.path().join("manager")));
        manager.install(None).unwrap();
        manager.start().await.unwrap();
        let service = OptimizerService::new_with_manager(
            storage.database().clone(),
            journal,
            visuals,
            events_tx,
            manager.clone(),
        );

        let mut runs = Vec::new();
        for (recipe_id, proposer) in [
            (BANKING77_GEPA_LUNA_RECIPE, ProposerProfile::LunaMedium),
            (BANKING77_GEPA_SOL_RECIPE, ProposerProfile::SolMedium),
        ] {
            let run_id = recipe_run_id(proposer);
            let (run, _) = service
                .create(OptimizerCreateRequest {
                    algorithm_id: "gepa".into(),
                    algorithm_version: Some("synth-optimizers-0.2.0".into()),
                    objective: Some(format!("Banking77 GEPA · {}", proposer.title())),
                    source: Some("local".into()),
                    project_ref: None,
                    session_ref: None,
                    id: Some(run_id),
                    execution_bindings: None,
                    input_refs: None,
                    capabilities: None,
                    summary: Some(json!({
                        "recipeId": recipe_id,
                        "proposerPolicyRef": {
                            "harness": "gepa_proposer",
                            "config": proposer.config_id(),
                        },
                    })),
                    open_visual: Some(true),
                    seed_fixture: None,
                    cloud_config: None,
                    local_path: None,
                })
                .await
                .unwrap();
            let (pinned, pin) = manager.pin_run(&service, &run.id, recipe_id).await.unwrap();
            runs.push((pinned, pin));
        }
        assert_ne!(runs[0].0.id, runs[1].0.id);
        assert_ne!(runs[0].1.spool_path, runs[1].1.spool_path);
        assert_eq!(runs[0].1.sidecar_version, runs[1].1.sidecar_version);
        assert_eq!(runs[0].1.algorithm_version, runs[1].1.algorithm_version);
        assert_eq!(runs[0].1.recipe_version, BANKING77_GEPA_LUNA_RECIPE);
        assert_eq!(runs[1].1.recipe_version, BANKING77_GEPA_SOL_RECIPE);
        assert_eq!(
            runs[0].0.summary.pointer("/proposerPolicyRef/config"),
            Some(&json!("luna_med"))
        );
        assert_eq!(
            runs[1].0.summary.pointer("/proposerPolicyRef/config"),
            Some(&json!("sol_med"))
        );
        for (run, pin) in &runs {
            assert_eq!(
                run.summary
                    .get("sidecarVersion")
                    .and_then(serde_json::Value::as_str),
                Some(super::super::manager::DEFAULT_SIDECAR_VERSION)
            );
            assert!(std::path::Path::new(&pin.spool_path)
                .join("identity.json")
                .is_file());
            assert!(!run.visual_refs.is_empty());
        }
        let stopped = manager.stop().await.unwrap();
        assert_ne!(stopped.phase, "ready");
        assert_eq!(
            service.get(runs[0].0.id.clone()).await.unwrap().id,
            runs[0].0.id
        );
        assert_eq!(
            service.get(runs[1].0.id.clone()).await.unwrap().id,
            runs[1].0.id
        );
    }

    #[test]
    fn parses_only_the_requested_dotenv_key() {
        let text = "OTHER=do-not-return\nexport OPENAI_API_KEY='test-key'\n";
        assert_eq!(
            dotenv_value(text, "OPENAI_API_KEY").as_deref(),
            Some("test-key")
        );
        assert_eq!(dotenv_value(text, "MISSING"), None);
    }

    #[test]
    fn resolves_an_absolute_uv_path_for_finder_launches() {
        if Path::new("/opt/homebrew/bin/uv").is_file() {
            let path = resolve_uv().unwrap();
            assert!(path.is_absolute());
            assert!(path.is_file());
        }
    }

    /// Manual A3 receipt. This is ignored in normal CI because it performs two
    /// paid Banking77 runs through the same Desktop service and manager. Run it
    /// with SYNTH_OPTIMIZER_PROJECT_ROOT pointed at the reviewed G1 checkout.
    #[tokio::test]
    #[ignore = "paid A3 acceptance; requires ChatGPT auth and OPENAI_API_KEY"]
    async fn paid_dual_banking77_luna_sol_receipt() {
        assert_eq!(
            std::env::var("SYNTH_OPTIMIZER_LIVE_SIDECAR").as_deref(),
            Ok("1"),
            "paid acceptance must exercise the real G1 sidecar, not the unit-test health stub"
        );
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path().join("core")).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        let content = ContentStore::new(storage.content_root());
        let visuals = VisualRegistry::new(storage.database().clone(), journal.clone(), content);
        let receipt_visuals = visuals.clone();
        let (events_tx, _) = tokio::sync::broadcast::channel(64);
        let manager = Arc::new(super::super::OptimizerManager::with_home(
            dir.path().join("manager"),
        ));
        let service = OptimizerService::new_with_manager(
            storage.database().clone(),
            journal,
            visuals,
            events_tx,
            manager.clone(),
        );

        let (luna, sol) = tokio::join!(
            service.start_recipe(OptimizerRecipeRunRequest {
                recipe_id: BANKING77_GEPA_LUNA_RECIPE.into(),
                session_ref: Some("a3-luna-vs-sol".into()),
                open_visual: Some(true),
                base_model: None,
                dataset_shard: None,
            }),
            service.start_recipe(OptimizerRecipeRunRequest {
                recipe_id: BANKING77_GEPA_SOL_RECIPE.into(),
                session_ref: Some("a3-luna-vs-sol".into()),
                open_visual: Some(true),
                base_model: None,
                dataset_shard: None,
            })
        );
        let luna = luna.unwrap().0;
        let sol = sol.unwrap().0;
        assert_ne!(luna.id, sol.id);
        assert!(!luna.visual_refs.is_empty() && !sol.visual_refs.is_empty());
        assert_eq!(
            luna.summary.pointer("/proposerPolicyRef/config"),
            Some(&json!("luna_med"))
        );
        assert_eq!(
            sol.summary.pointer("/proposerPolicyRef/config"),
            Some(&json!("sol_med"))
        );
        let visual_ready = async |run: &super::super::models::OptimizerRunRecord| {
            let visual_id = run
                .visual_refs
                .iter()
                .find(|reference| reference.kind == "visual")
                .map(|reference| reference.id.clone())
                .expect("paid receipt omitted primary visual");
            let visual = receipt_visuals.get(visual_id).await.unwrap();
            json!({"visualId": visual.id, "readyAt": visual.created_at})
        };
        let luna_visual_ready = visual_ready(&luna).await;
        let sol_visual_ready = visual_ready(&sol).await;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(1_800);
        let mut luna_partial = false;
        let mut sol_partial = false;
        let mut luna_advanced_while_sol_selected = false;
        let mut sol_advanced_while_luna_selected = false;
        let mut prior_luna_cursor = luna.cursor_seq;
        let mut prior_sol_cursor = sol.cursor_seq;
        let mut select_luna = true;
        let (luna, sol) = loop {
            let luna = service.get(luna.id.clone()).await.unwrap();
            let sol = service.get(sol.id.clone()).await.unwrap();
            let terminal = |status: &str| matches!(status, "completed" | "failed" | "cancelled");
            luna_partial |= !terminal(&luna.status) && luna.cursor_seq > 2;
            sol_partial |= !terminal(&sol.status) && sol.cursor_seq > 2;
            if select_luna && sol.cursor_seq > prior_sol_cursor {
                sol_advanced_while_luna_selected = true;
            }
            if !select_luna && luna.cursor_seq > prior_luna_cursor {
                luna_advanced_while_sol_selected = true;
            }
            prior_luna_cursor = luna.cursor_seq;
            prior_sol_cursor = sol.cursor_seq;
            select_luna = !select_luna;
            if terminal(&luna.status) && terminal(&sol.status) {
                break (luna, sol);
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out: luna={} sol={}",
                luna.status,
                sol.status
            );
            sleep(Duration::from_secs(2)).await;
        };

        assert_eq!(luna.status, "completed", "luna error: {:?}", luna.error);
        assert_eq!(sol.status, "completed", "sol error: {:?}", sol.error);
        assert!(
            luna_partial && sol_partial,
            "both visuals must receive partial live events"
        );
        assert!(
            luna_advanced_while_sol_selected && sol_advanced_while_luna_selected,
            "visual selection must not stall the unselected run"
        );
        assert!(luna.cursor_seq > 2 && sol.cursor_seq > 2);
        assert_ne!(
            luna.summary.get("spoolPath"),
            sol.summary.get("spoolPath"),
            "each optimizer run must own its spool"
        );
        for (label, run, expected_model) in [
            ("luna", &luna, "gpt-5.6-luna"),
            ("sol", &sol, "gpt-5.6-sol"),
        ] {
            let page = manager
                .optimizer_events_after(&run.id, 0, 2_000)
                .await
                .unwrap();
            let replay = manager
                .optimizer_events_after(&run.id, 0, 2_000)
                .await
                .unwrap();
            assert_eq!(page, replay, "{label} cursor replay was not idempotent");
            let source_events = page["events"].as_array().unwrap();
            assert!(!source_events.is_empty(), "{label} live endpoint was empty");
            for (index, event) in source_events.iter().enumerate() {
                assert_eq!(event["schema_version"], "optimizer_event.v1");
                assert_eq!(event["algorithm_id"], "gepa");
                assert_eq!(event["slot"], "optimizer_run");
                assert_eq!(event["run_id"], run.id);
                assert_eq!(event["sequence_number"], json!((index + 1) as u64));
            }
            let proposer_started = source_events
                .iter()
                .find(|event| event.pointer("/delta/trigger") == Some(&json!("proposer_started")))
                .unwrap_or_else(|| panic!("{label} omitted proposer_started"));
            let ready = if label == "luna" {
                &luna_visual_ready
            } else {
                &sol_visual_ready
            };
            assert!(
                ready["readyAt"].as_str().unwrap()
                    < proposer_started["created_at"].as_str().unwrap(),
                "{label} proposer started before its visual was ready"
            );
            let child_refs = source_events
                .iter()
                .filter(|event| event["type"] == "optimizer.child_rollout.attached")
                .map(|event| event.pointer("/delta/child_resource_ref").unwrap())
                .collect::<Vec<_>>();
            assert!(!child_refs.is_empty(), "{label} omitted child rollout refs");
            for child in child_refs {
                assert_eq!(child["schema"], "synth.resource-ref.v1");
                assert_eq!(child["kind"], "container_rollout");
                assert!(child["id"].as_str().is_some_and(|value| !value.is_empty()));
                assert!(child["attributes"]["stream_id"]
                    .as_str()
                    .is_some_and(|value| value.starts_with("stream:")));
                assert!(child["attributes"]["reward_url"]
                    .as_str()
                    .is_some_and(|value| value.starts_with("/reward?rollout_id=")));
            }
            let pivot = source_events.len() as u64 / 2;
            let suffix = manager
                .optimizer_events_after(&run.id, pivot, 2_000)
                .await
                .unwrap();
            assert!(suffix["events"]
                .as_array()
                .unwrap()
                .iter()
                .all(|event| event["sequence_number"].as_u64().unwrap() > pivot));
            let run_dir = PathBuf::from(
                run.summary
                    .get("runDirectory")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_else(|| panic!("{label} receipt omitted runDirectory")),
            );
            let manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(run_dir.join("result_manifest.json")).unwrap())
                    .unwrap();
            assert!(
                manifest
                    .pointer("/usage/proposer_calls")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0)
                    >= 1,
                "{label} completed without invoking its proposer"
            );
            let generated: toml::Value =
                toml::from_str(&fs::read_to_string(run_dir.join("workshop.recipe.toml")).unwrap())
                    .unwrap();
            assert_eq!(
                generated
                    .get("proposer")
                    .and_then(|value| value.get("model"))
                    .and_then(toml::Value::as_str),
                Some(expected_model),
                "{label} proposer receipt used the wrong model"
            );
        }
        manager.stop().await.unwrap();
    }
}
