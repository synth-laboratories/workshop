//! Product-owned optimizer recipes. This module is the local execution trust
//! boundary: callers select an allowlisted recipe but cannot supply commands,
//! paths, environment variables, or credentials.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::json;
use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{process::Command, sync::watch, time::sleep};

use super::{
    models::{
        OptimizerCreateRequest, OptimizerEventEnvelope, OptimizerExecutionBinding,
        OptimizerRecipeRunRequest, OptimizerResourceRef, OPTIMIZER_EVENT_SCHEMA_VERSION,
    },
    normalize, OptimizerService,
};

pub const BANKING77_GEPA_SMOKE_RECIPE: &str = "gepa.banking77.smoke.v1";
const TRAIN_ROWS: usize = 4;
const HELDOUT_ROWS: usize = 2;
const MAX_GENERATIONS: i64 = 1;
const PROPOSALS_PER_GENERATION: i64 = 1;
const MINIBATCH_SIZE: i64 = 2;
const MAX_TRAIN_ROLLOUTS: i64 = 4;
const MAX_HELDOUT_ROLLOUTS: i64 = 2;
const MAX_TOTAL_ROLLOUTS: i64 = 8;
const MAX_COST_USD: f64 = 0.25;
const PROPOSER_ESTIMATED_COST_USD: f64 = 0.08;
const ROLLOUT_ESTIMATED_COST_USD: f64 = 0.02;

pub(super) async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    let cookbook = banking77_cookbook_root()?;
    let run_suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("banking77_gepa_smoke_{}", &run_suffix[..8]);
    let runs_root = cookbook
        .parent()
        .ok_or_else(|| anyhow!("invalid Banking77 cookbook path"))?
        .join("runs");
    let run_dir = runs_root.join(&run_id);
    fs::create_dir_all(&run_dir).context("create Banking77 GEPA run directory")?;
    let port = reserve_loopback_port()?;
    let uv = resolve_uv()?;
    let config_path = run_dir.join("workshop.recipe.toml");
    materialize_config(&cookbook, &runs_root, &run_id, port, &uv, &config_path)?;

    let create = OptimizerCreateRequest {
        algorithm_id: "gepa".into(),
        algorithm_version: Some("synth-optimizers-0.2.0".into()),
        objective: Some("Banking77 intent prompt · bounded GEPA smoke".into()),
        source: Some("local".into()),
        project_ref: Some("banking77@huggingface-polyai-pinned-by-cookbook".into()),
        session_ref: request.session_ref,
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![OptimizerExecutionBinding {
            kind: "local_process".into(),
            id: run_id.clone(),
            label: Some("Banking77 GEPA smoke".into()),
            status: Some("starting".into()),
            metadata: json!({ "recipeId": BANKING77_GEPA_SMOKE_RECIPE, "port": port }),
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
                id: BANKING77_GEPA_SMOKE_RECIPE.into(),
                digest: None,
                role: Some("configuration".into()),
                title: Some("Bounded Banking77 GEPA smoke".into()),
                metadata: recipe_limits(),
            },
        ]),
        capabilities: None,
        summary: Some(json!({
            "recipeId": BANKING77_GEPA_SMOKE_RECIPE,
            "task": "banking77",
            "limits": recipe_limits(),
            "runDirectory": run_dir,
        })),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    let (run, event) = service.create(create).await?;

    let (cancel_tx, cancel_rx) = watch::channel(false);
    service
        .register_local_recipe(run_id.clone(), cancel_tx)
        .await;
    let worker_service = service.clone();
    tokio::spawn(async move {
        if let Err(error) = run_recipe_worker(
            worker_service.clone(),
            run_id.clone(),
            cookbook,
            config_path,
            run_dir,
            uv,
            cancel_rx,
        )
        .await
        {
            let _ = append_terminal_event(&worker_service, &run_id, true, error.to_string()).await;
        }
        worker_service.unregister_local_recipe(&run_id).await;
    });
    Ok((run, event))
}

pub fn recipe_catalog() -> serde_json::Value {
    json!({
        "id": BANKING77_GEPA_SMOKE_RECIPE,
        "title": "Banking77 GEPA smoke",
        "algorithmId": "gepa",
        "task": "banking77",
        "availability": if banking77_cookbook_root().is_ok() { "available" } else { "unavailable" },
        "limits": recipe_limits(),
        "credentialInputs": [],
    })
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
            != Some(BANKING77_GEPA_SMOKE_RECIPE)
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
    uv: PathBuf,
    mut cancel_rx: watch::Receiver<bool>,
) -> Result<()> {
    append_status_event(&service, &run_id, "optimizer.run.started", "running").await?;
    let openai_api_key = resolve_secret("OPENAI_API_KEY")?;
    let stdout = fs::File::create(run_dir.join("workshop.stdout.log"))?;
    let stderr = fs::File::create(run_dir.join("workshop.stderr.log"))?;
    let mut child = Command::new(&uv)
        .current_dir(&cookbook)
        .args([
            "run",
            "--no-project",
            "--with",
            "synth-optimizers==0.2.0",
            "synth-optimizers",
            "gepa",
            "run",
            "--config",
        ])
        .arg(&config_path)
        .env("OPENAI_API_KEY", openai_api_key)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true)
        .spawn()
        .context("launch allowlisted Banking77 GEPA recipe")?;

    let event_path = run_dir.join("events.jsonl");
    loop {
        ingest_available(&service, &run_id, &event_path).await?;
        tokio::select! {
            status = child.wait() => {
                let status = status.context("wait for Banking77 GEPA process")?;
                ingest_available(&service, &run_id, &event_path).await?;
                if !status.success() {
                    bail!("Banking77 GEPA exited with {status}; see {}", run_dir.join("workshop.stderr.log").display());
                }
                append_recipe_artifacts(&service, &run_id, &run_dir).await?;
                append_recipe_candidates(&service, &run_id, &run_dir).await?;
                append_terminal_event(&service, &run_id, false, "recipe process completed".into()).await?;
                return Ok(());
            }
            changed = cancel_rx.changed() => {
                if changed.is_ok() && *cancel_rx.borrow() {
                    child.kill().await.context("cancel Banking77 GEPA process")?;
                    append_status_event(&service, &run_id, "optimizer.run.cancelled", "cancelled").await?;
                    return Ok(());
                }
            }
            _ = sleep(Duration::from_millis(750)) => {}
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
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("SYNTH_BANKING77_SECRET_ENV_FILE") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join("Documents/GitHub/synth-ai/.env"));
        candidates.push(home.join("Documents/GitHub/backend/.env.local"));
    }
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

async fn ingest_available(service: &OptimizerService, run_id: &str, path: &Path) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let text = fs::read_to_string(path).context("read Banking77 GEPA events")?;
    let raw = text
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let mut value: serde_json::Value = serde_json::from_str(line).ok()?;
            if let Some(object) = value.as_object_mut() {
                // Native GEPA events are `{type, ts, fields}` records without a
                // sequence. Reserve sequence 1 for the Desktop start event and
                // deterministically number native lines so repeated tails dedupe.
                object
                    .entry("_seq")
                    .or_insert_with(|| json!(index as u64 + 2));
            }
            Some(value)
        })
        .collect::<Vec<_>>();
    let cursor = service.get(run_id.to_string()).await?.cursor_seq;
    let events = normalize::normalize_events(&raw, run_id, "gepa")
        .into_iter()
        .filter(|event| event.sequence_number > cursor)
        .collect::<Vec<_>>();
    if !events.is_empty() {
        service.append_events(run_id.to_string(), events).await?;
    }
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
        .or_else(|| {
            dirs::home_dir().map(|home| {
                home.join("Documents/GitHub/synth-cookbooks-public/cookbooks/optimizers/gepa/banking77_container")
            })
        })
        .ok_or_else(|| anyhow!("cannot resolve home directory"))?;
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

fn materialize_config(
    cookbook: &Path,
    runs_root: &Path,
    run_id: &str,
    port: u16,
    uv: &Path,
    destination: &Path,
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
                "BANKING77_TRAIN_SAMPLE=4",
                "BANKING77_TEST_SAMPLE=2",
                "BANKING77_POLICY_CONCURRENCY=4",
                "BANKING77_POLICY_TIMEOUT_SECONDS=20",
                "BANKING77_ROLLOUT_TIMEOUT_SECONDS=25",
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

    let cache = table_mut(&mut config, "cache")?;
    cache.insert("mode".into(), toml::Value::String("off".into()));
    cache.insert("path".into(), toml::Value::String(String::new()));
    cache.insert("namespace".into(), toml::Value::String(run_id.into()));

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
    use tempfile::tempdir;

    #[test]
    fn materialized_recipe_enforces_tiny_bounds_and_no_secret_values() {
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
        let output = dir.path().join("recipe.toml");
        materialize_config(
            dir.path(),
            &runs,
            "test_run",
            23456,
            Path::new("/usr/bin/true"),
            &output,
        )
        .unwrap();
        let text = fs::read_to_string(output).unwrap();
        let config: toml::Value = toml::from_str(&text).unwrap();
        validate_limits(&config).unwrap();
        assert!(text.contains("max_total_rollouts = 8"));
        assert!(text.contains("max_cost_usd = 0.25"));
        assert!(text.contains("proposer_estimated_cost_usd = 0.08"));
        assert!(text.contains("rollout_estimated_cost_usd = 0.02"));
        assert!(!text.contains("OPENAI_API_KEY="));
        assert!(!text.contains("secret"));
    }

    #[test]
    fn catalog_discloses_no_credential_inputs() {
        assert_eq!(recipe_catalog()["credentialInputs"], json!([]));
        assert_eq!(recipe_catalog()["limits"]["maxCostUsd"], json!(0.25));
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
}
