//! Product-owned optimizer recipes. This module is the local execution trust
//! boundary: callers select an allowlisted recipe but cannot supply commands,
//! paths, environment variables, or credentials.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::watch, time::sleep};

use super::events::OptimizerEventDraft;
use super::{
    manager::DEFAULT_ALGORITHM_VERSION,
    models::{
        OptimizerCreateRequest, OptimizerExecutionBinding, OptimizerRecipeRunRequest,
        OptimizerResourceRef,
    },
    normalize, OptimizerService,
};

pub(super) async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    start_inner(service, request, true).await
}

async fn start_inner(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
    spawn: bool,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    let session = request
        .session_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("workspace recipes require session_ref"))?;
    let workspace =
        super::workspace_recipe::require_session_workspace(service.database(), session)?;
    let recipe = super::workspace_recipe::find_recipe(&workspace, &request.recipe_id)?;
    match recipe.algorithm {
        super::workspace_recipe::AlgorithmKind::Eval => {
            return super::container_eval::start(service, request).await;
        }
        super::workspace_recipe::AlgorithmKind::Gepa => {}
    }
    let manager = service.manager().clone();
    require_plugin_ready(&manager).await?;
    let ensured =
        super::container_lifecycle::ensure(service.database(), &workspace, &recipe.container)
            .await?;
    let run_id = format!(
        "gepa_{}_{}",
        recipe
            .id
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect::<String>(),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let runs_root = gepa_runs_root()?;
    let run_dir = runs_root.join(&run_id);
    fs::create_dir_all(&run_dir).context("create GEPA run directory")?;
    let config_path = super::workspace_recipe::copy_into_run_dir(&recipe, &run_dir)?;
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(&config_path)?)?;
    let table = config
        .as_table_mut()
        .ok_or_else(|| anyhow!("workspace recipe must be a TOML table"))?;
    table.insert(
        "run".into(),
        toml::Value::Table(
            [
                ("run_id".into(), toml::Value::String(run_id.clone())),
                (
                    "output_dir".into(),
                    toml::Value::String(runs_root.display().to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        ),
    );
    table.insert(
        "container".into(),
        toml::Value::Table(
            [("url".into(), toml::Value::String(ensured.base_url.clone()))]
                .into_iter()
                .collect(),
        ),
    );
    let openai = resolve_provider_workload(&recipe.provider, &run_id, &recipe.id)?;
    super::workspace_recipe::bind_locality_urls(
        table,
        recipe.locality,
        openai.base_url.as_deref(),
        openai.config_base_url.as_deref(),
        openai.inference_url.as_deref(),
    )?;
    fs::write(&config_path, toml::to_string_pretty(&config)?)?;
    if let Some(lease) = openai.lease.as_ref() {
        crate::secrets::lease::bind_lease_into_toml(&config_path, lease)?;
    }
    let create = OptimizerCreateRequest {
        algorithm_id: "gepa".into(),
        algorithm_version: Some(DEFAULT_ALGORITHM_VERSION.into()),
        objective: Some(recipe.title.clone()),
        source: Some("local".into()),
        project_ref: Some(workspace.display().to_string()),
        session_ref: request.session_ref.clone(),
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![OptimizerExecutionBinding {
            kind: "local_process".into(),
            id: run_id.clone(),
            label: Some(recipe.title.clone()),
            status: Some("starting".into()),
            metadata: json!({
                "recipeId": recipe.id,
                "containerId": ensured.container_id,
                "locality": recipe.locality.as_str(),
                "sourceHash": recipe.source_hash,
            }),
        }]),
        input_refs: Some(vec![
            OptimizerResourceRef {
                kind: "container".into(),
                id: ensured.container_id.clone(),
                digest: None,
                role: Some("eval_target".into()),
                title: Some(recipe.container.clone()),
                metadata: json!({ "baseUrl": ensured.base_url }),
            },
            OptimizerResourceRef {
                kind: "recipe".into(),
                id: recipe.id.clone(),
                digest: Some(recipe.source_hash.clone()),
                role: Some("configuration".into()),
                title: Some(recipe.title.clone()),
                metadata: json!({
                    "cwd": run_dir,
                    "locality": recipe.locality.as_str(),
                }),
            },
        ]),
        capabilities: None,
        summary: Some(json!({
            "recipeId": recipe.id,
            "task": recipe.family,
            "source": "workspace",
            "containerId": ensured.container_id,
            "locality": recipe.locality.as_str(),
            "sourceHash": recipe.source_hash,
            "limits": {
                "maxCostUsd": recipe.bounds.max_cost_usd,
                "maxTotalRollouts": recipe.bounds.max_total_rollouts,
            },
            "runDirectory": run_dir,
        })),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    let (run, event) = service.create(create).await?;
    let (run, _) = manager.pin_run(service, &run.id, &recipe.id).await?;
    if !spawn {
        return Ok((run, event));
    }
    append_status_event(service, &run_id, "optimizer.run.queued", "queued").await?;
    let (cancel_tx, cancel_rx) = watch::channel(false);
    service
        .register_local_recipe(run_id.clone(), cancel_tx)
        .await;
    let worker_service = service.clone();
    let worker_manager = manager.clone();
    let work_dir = run_dir.clone();
    tokio::spawn(async move {
        if let Err(error) = run_recipe_worker(
            worker_service.clone(),
            run_id.clone(),
            work_dir.clone(),
            config_path,
            work_dir,
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
    let manager = service.manager().clone();
    require_plugin_ready(&manager).await?;
    let (mut run, event) = start_inner(service, request, false).await?;
    let digest = preparation_digest(&run);
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert("preparationDigest".into(), json!(digest));
    summary.insert("waitingForViewer".into(), json!(true));
    // A missing digest is a refusal, never a skipped pin. Without this, prepare
    // records no `capabilitiesDigest`, start's comparison is a skipped `if let`,
    // and the anti-swap guard is inert in exactly the case it exists to catch:
    // capabilities that were never proven by a live handshake.
    let capabilities_digest = manager
        .advertised_capabilities()
        .get("digest")
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "optimizer capabilities are not proven; the sidecar must complete a capability \
                 handshake before a run is prepared"
            )
        })?;
    summary.insert("capabilitiesDigest".into(), capabilities_digest);
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
        bail!(
            "optimizer run `{run_id}` is not prepared for start (status {})",
            run.status
        );
    }
    let recipe_id = run
        .summary
        .get("recipeId")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("prepared run omitted recipeId"))?
        .to_owned();
    let _ = recipe_id;
    let run_dir = run
        .summary
        .get("runDirectory")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("prepared run omitted runDirectory"))?;
    let cookbook = run_dir.clone();
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

pub(super) async fn require_plugin_ready(manager: &super::OptimizerManager) -> Result<()> {
    // A disabled plugin refuses work even when its sidecar is still up.
    // `disable` only clears the registry flag, so the process keeps running;
    // without this check the only thing enforcing "disabled" was that the MCP
    // server stopped being registered at session start — which a session that
    // was already open never sees.
    if !crate::plugins::optimizers_plugin_enabled() {
        return Err(crate::plugins::PluginNotReady::new("disabled", "enable").into());
    }
    if manager.is_running().await {
        return Ok(());
    }
    let status = manager.status().await;
    if status.version.is_none() {
        return Err(crate::plugins::PluginNotReady::new("not_installed", "install").into());
    }
    // Installed/enabled is sufficient authority for an idempotent warm start.
    // `OptimizerManager::start` probes first and returns only after the
    // authenticated proxy is healthy, so a normal recipe attempt no longer
    // leaks a stopped sidecar as `plugin_not_ready` to the agent.
    manager
        .start()
        .await
        .context("start the installed Optimizers plugin for this workflow")?;
    Ok(())
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

pub fn recipe_catalog() -> Vec<serde_json::Value> {
    Vec::new()
}

pub(super) async fn start_container_eval(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    super::container_eval::start(service, request).await
}

pub(super) async fn reconcile_persisted(
    service: &OptimizerService,
    run_id: &str,
) -> Result<super::models::OptimizerRunRecord> {
    let run = service.get(run_id.to_string()).await?;
    if !super::service::is_terminal_status(&run.status)
        || run
            .summary
            .get("recipeId")
            .and_then(serde_json::Value::as_str)
            .is_none()
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

fn run_index_wait() -> Duration {
    if cfg!(test) {
        if let Ok(millis) = std::env::var("SYNTH_OPTIMIZER_TEST_INDEX_WAIT_MS") {
            if let Ok(millis) = millis.parse::<u64>() {
                return Duration::from_millis(millis);
            }
        }
    }
    crate::limits::OPTIMIZER_RUN_INDEX_WAIT
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
    let _revoke = crate::secrets::RevokeRunOnDrop(run_id.clone());
    let _ownership = service.hold_run_ownership(&run_id)?;
    append_status_event(&service, &run_id, "optimizer.run.started", "running").await?;
    let provider = fs::read_to_string(&config_path)
        .context("read run-owned recipe provider")?
        .parse::<toml::Value>()?
        .get("provider")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("run-owned recipe is missing provider"))?
        .to_string();
    let openai = resolve_provider_workload(&provider, &run_id, recipe_id_for_lease(&config_path))?;
    if let Some(lease) = openai.lease.as_ref() {
        crate::secrets::lease::bind_lease_into_toml(&config_path, lease)?;
        let _ = service.persist_credential_chain(&run_id).await;
    }
    let stdout = fs::File::create(run_dir.join("workshop.stdout.log"))?;
    let stderr = fs::File::create(run_dir.join("workshop.stderr.log"))?;
    let extra_env = openai
        .lease
        .as_ref()
        .map(|lease| lease.compile_host_env())
        .unwrap_or_default();
    let mut child = manager
        .spawn_gepa_recipe(
            &run_id,
            &cookbook,
            &config_path,
            stdout,
            stderr,
            &openai.api_key,
            openai.base_url.as_deref(),
            &extra_env,
        )
        .await?;

    let mut upstream_cursor = 0;
    // A run that never becomes visible is bounded, not waited out. Retrying an
    // unindexed run until the child exits on its own turns a contract failure
    // into a full-budget one: the rollouts are still paid for, and their events
    // can never be ingested. `run_not_found` cannot distinguish "not registered
    // yet" from "this service will never see this run" (a mismatched database,
    // or a service that does not serve the events route at all), so the wait is
    // capped and the child is killed rather than allowed to spend to term.
    let mut indexed = false;
    let index_wait = run_index_wait();
    let index_deadline = tokio::time::Instant::now() + index_wait;
    loop {
        match ingest_available(&service, &manager, &run_id, &mut upstream_cursor).await {
            Ok(()) => indexed = true,
            Err(error) => {
                if super::OptimizerManager::optimizer_event_endpoint_temporarily_unavailable(&error)
                {
                    // The producer is independently supervised and paid work
                    // may still be progressing. Keep the bounded index wait
                    // below, but do not turn one observer gateway miss into a
                    // terminal optimizer failure.
                } else
                // The producer registers its durable index shortly after spawn.
                // A 404 is retryable only while the child is demonstrably alive;
                // it is not a successful empty event page.
                if !super::OptimizerManager::optimizer_run_not_indexed(&error) {
                    return Err(error);
                }
                if !indexed && tokio::time::Instant::now() >= index_deadline {
                    manager.terminate_gepa_recipe(&run_id).await;
                    if child.try_wait()?.is_none() {
                        let _ = child.kill().await;
                    }
                    let waited = index_wait.as_secs_f32();
                    append_terminal_event(
                        &service,
                        &run_id,
                        true,
                        format!(
                            "run_never_indexed: the optimizer service never reported run {run_id} \
                             after {waited}s of a live recipe process; the child was terminated \
                             before spending further"
                        ),
                    )
                    .await?;
                    bail!(
                        "run_never_indexed: optimizer run {run_id} was never visible to the \
                         polled service after {waited}s; terminated the recipe process. The \
                         service and the recipe child are not sharing a run index — verify the \
                         sidecar serves /runs/{{id}}/optimizer-events and that the child writes \
                         the database the service was started with"
                    );
                }
            }
        }
        tokio::select! {
            status = child.wait() => {
                let status = status.context("wait for product-owned GEPA process")?;
                let final_ingest =
                    ingest_available(&service, &manager, &run_id, &mut upstream_cursor).await;
                if !status.success() {
                    let ingest_detail = final_ingest
                        .as_ref()
                        .err()
                        .map(|error| format!("; final event ingestion also failed: {error:#}"))
                        .unwrap_or_default();
                    bail!(
                        "GEPA recipe {}{ingest_detail}; stdout={} stderr={}",
                        describe_exit_status(&status),
                        run_dir.join("workshop.stdout.log").display(),
                        run_dir.join("workshop.stderr.log").display()
                    );
                }
                final_ingest.context("ingest final GEPA event page after successful child exit")?;
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
                        child.kill().await.context("cancel product-owned GEPA process")?;
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

/// Resolve OpenAI access for a recipe through the local vault and provider
/// proxy. Paid workers never receive a provider key and never fall back to a
/// process variable or dotenv file.
fn resolve_provider_workload(
    provider: &str,
    run_id: &str,
    recipe_id: &str,
) -> Result<OpenAiWorkload> {
    #[cfg(test)]
    {
        if std::env::var("SYNTH_OPTIMIZER_TEST_CHILD_SLEEP_SECS").is_ok()
            || std::env::var("SYNTH_OPTIMIZER_TEST_SUPPRESS_SPOOL").is_ok()
        {
            return Ok(OpenAiWorkload {
                api_key: crate::secrets::API_KEY_SENTINEL.to_owned(),
                base_url: None,
                config_base_url: None,
                inference_url: None,
                lease: None,
            });
        }
    }
    let secrets = crate::secrets::live().ok_or_else(|| {
        crate::secrets::lease::CredentialError::new(
            crate::secrets::lease::PROXY_NOT_RUNNING,
            "proxy",
            true,
            "Workshop secrets proxy is not running",
        )
        .anyhow()
    })?;
    let lease = secrets
        .issue_lease(
            provider,
            run_id,
            recipe_id,
            crate::secrets::SecretsUsePolicy::default(),
            "optimizer",
        )
        .map_err(|error| anyhow!("{error}"))?;
    Ok(OpenAiWorkload {
        api_key: crate::secrets::API_KEY_SENTINEL.to_owned(),
        base_url: Some(lease.host_base_url.clone()),
        config_base_url: Some(lease.container_base_url.clone()),
        inference_url: Some(lease.inference_url.clone()),
        lease: Some(lease),
    })
}

fn recipe_id_for_lease(config_path: &Path) -> &str {
    config_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("gepa")
}

#[derive(Clone, Debug)]
pub(super) struct OpenAiWorkload {
    pub api_key: String,
    pub base_url: Option<String>,
    pub config_base_url: Option<String>,
    pub inference_url: Option<String>,
    pub lease: Option<crate::secrets::CredentialLease>,
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
    let mut draft = OptimizerEventDraft::new("optimizer.recipe.artifacts", "gepa")
        .idempotency_key("workshop:artifacts")
        .level("info")
        .delta(serde_json::from_value(json!({
            "message": format!("Persisted {} optimizer artifacts", artifacts.len()),
        }))?)
        .raw(json!({ "source": "workshop_recipe" }));
    draft.artifact_refs = artifacts;
    service
        .append_event_payloads(run_id.to_string(), vec![draft])
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
        let mut delta = serde_json::Map::new();
        for key in ["train_reward", "heldout_reward", "minibatch_reward"] {
            if let Some(value) = candidate.get(key) {
                delta.insert(key.into(), value.clone());
            }
        }
        if let Some(parent_id) = candidate.get("parent_id") {
            delta.insert("parentId".into(), parent_id.clone());
        }
        // Content-addressed identity: the same candidate reconciled twice is the
        // same event, so a repeated reconcile re-offers it rather than minting a
        // second sequence for it.
        let mut draft = OptimizerEventDraft::new("candidate.artifact.loaded", "gepa")
            .idempotency_key(format!("candidate-artifact:{candidate_id}"))
            .level("info")
            .item(json!({
                "kind": "candidate",
                "id": candidate_id,
                "status": status,
                "raw": {
                    "values": values,
                    "sourceArtifact": "candidate_registry.json"
                }
            }))
            .delta(delta)
            .raw(json!({ "source": "candidate_registry.json", "index": index }));
        draft.artifact_refs = vec![json!({
            "kind": "candidate_registry",
            "id": registry_path,
            "path": registry_path,
            "title": "Candidate registry"
        })];
        events.push(draft);
    }
    if !events.is_empty() {
        service
            .append_event_payloads(run_id.to_string(), events)
            .await?;
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
            events.push(
                OptimizerEventDraft::new("proposer.transcript.loaded", "gepa")
                    .idempotency_key(format!("proposer-transcript:{generation}"))
                    .level("info")
                    .delta(serde_json::from_value(json!({
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
                    }))?)
                    .raw(json!({ "source": "opencode_response.json", "generation": generation }))
                    .artifact_refs(vec![json!({
                        "kind": "proposer_transcript",
                        "id": response_path,
                        "path": response_path,
                        "title": format!("Proposer transcript · generation {generation}")
                    })]),
            );
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
                events.push(
                    OptimizerEventDraft::new("proposer.trace_v5.loaded", "gepa")
                        .idempotency_key(format!("proposer-trace-v5:{generation}"))
                        .level("info")
                        .delta(serde_json::from_value(json!({
                            "generation": generation,
                            "schema_version": "synth.trace-projection.rollout-inspector.v1",
                            "message": "Sealed proposer Trace V5 reconciled from app-server artifacts",
                            "items": items,
                        }))?)
                        .raw(json!({ "source": "opencode_sse_events.jsonl", "generation": generation }))
                        .artifact_refs(vec![json!({
                            "kind": "trace_v5",
                            "id": trace_path,
                            "path": trace_path,
                            "title": format!("Proposer Trace V5 · generation {generation}")
                        })]),
                );
            }
        }
    }
    if !events.is_empty() {
        service
            .append_event_payloads(run_id.to_string(), events)
            .await?;
    }
    Ok(())
}

async fn append_status_event(
    service: &OptimizerService,
    run_id: &str,
    event_type: &str,
    status: &str,
) -> Result<()> {
    service
        .append_event_payloads(
            run_id.to_string(),
            vec![OptimizerEventDraft::new(event_type, "gepa")
                // One lifecycle transition per run: a retried append re-offers
                // the same event instead of minting a second one.
                .idempotency_key(format!("workshop:lifecycle:{event_type}"))
                .delta(serde_json::from_value(json!({ "status": status }))?)
                .raw(json!({ "source": "workshop_recipe" }))],
        )
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
    let stdout_path = run_directory
        .as_ref()
        .map(|directory| directory.join("workshop.stdout.log"));
    let stderr_path = run_directory
        .as_ref()
        .map(|directory| directory.join("workshop.stderr.log"));
    let stdout_tail = stdout_path
        .as_ref()
        .and_then(|path| bounded_log_tail(path, 4_000).ok())
        .filter(|text| !text.trim().is_empty());
    let stderr_tail = stderr_path
        .as_ref()
        .and_then(|path| bounded_log_tail(path, 4_000).ok())
        .filter(|text| !text.trim().is_empty());
    // `detail` is the supervisor's causal error (exit status, signal, ingest
    // failure, etc.). Log output is supporting evidence and must never replace
    // it: stderr may contain only a warning, which previously hid the actual
    // failure and produced a polished-looking lie in the run UI.
    let display_message = detail.trim();
    let mut draft = OptimizerEventDraft::new("optimizer.recipe.diagnostic", "gepa")
        .level("error")
        .error(json!({
            "message": display_message.chars().take(1_000).collect::<String>(),
            "supervisorDetail": detail.chars().take(4_000).collect::<String>(),
            "stdoutTail": stdout_tail,
            "stderrTail": stderr_tail,
            "logPath": stderr_path,
            "stdoutLogPath": stdout_path,
            "stderrLogPath": stderr_path,
        }));
    draft.delta.insert("status".into(), json!("failed"));
    service
        .append_event_payloads(run_id.to_string(), vec![draft])
        .await?;

    // The run evidence above stays authoritative. This makes the same failure
    // findable from the other side — by optimizer_run_id, alongside whatever
    // container, stream, or visual failed with it.
    if let Some(diagnostics) = service.diagnostics() {
        let mut input = crate::diagnostics::DiagnosticInput::new(
            crate::diagnostics::Severity::Error,
            "optimizers",
            "optimizer.worker.failed",
            crate::diagnostics::codes::OPTIMIZER_WORKER_FAILED,
            display_message.chars().take(500).collect::<String>(),
        );
        input.correlation.optimizer_run_id = Some(run_id.to_owned());
        input.details.insert("algorithm".into(), json!("gepa"));
        if let Some(path) = stderr_path.as_ref() {
            // The pointer, not the contents: a log tail is evidence to open,
            // not payload to index.
            input
                .details
                .insert("log_path".into(), json!(path.display().to_string()));
        }
        diagnostics.emit(input);
    }
    Ok(())
}

fn describe_exit_status(status: &std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("terminated by signal {signal}");
        }
    }
    match status.code() {
        Some(code) => format!("exited with code {code}"),
        None => format!("exited with unknown status {status}"),
    }
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
///
/// Deliberately *not* beside the cookbook. Packaged builds ship the cookbooks
/// inside `Synth Desktop.app/Contents/Resources`, so deriving the runs root
/// from the cookbook put every run directory — configs, proposer transcripts,
/// `best_candidate.json`, the result manifest `get_result` reads — inside the
/// application bundle. A rebuild or an update deletes them (observed: a 19:47
/// rebuild erased four completed runs' evidence), and a signed, quarantined
/// install cannot write there at all. Evidence lives in the instance's own
/// writable data root, which survives both.
fn gepa_runs_root() -> Result<PathBuf> {
    let root = crate::instance::data_root()
        .join("optimizers")
        .join("gepa")
        .join("runs");
    fs::create_dir_all(&root).context("create the optimizer runs root")?;
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn product_gepa_catalog_is_empty() {
        assert!(
            recipe_catalog().is_empty(),
            "Workshop must not ship task GEPA/eval recipes; the workspace declares them"
        );
    }
}

#[cfg(test)]
mod runs_root_tests {
    use super::*;

    #[test]
    fn gepa_run_evidence_lives_outside_the_application_bundle() {
        let temp = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("SYNTH_DESKTOP_DATA_ROOT");
        std::env::set_var("SYNTH_DESKTOP_DATA_ROOT", temp.path());
        let root = gepa_runs_root().unwrap();
        match previous {
            Some(value) => std::env::set_var("SYNTH_DESKTOP_DATA_ROOT", value),
            None => std::env::remove_var("SYNTH_DESKTOP_DATA_ROOT"),
        }
        assert!(root.starts_with(temp.path()), "{}", root.display());
        assert!(root.is_dir(), "the runs root is created eagerly");
        assert!(
            !root
                .components()
                .any(|component| component.as_os_str().to_string_lossy().ends_with(".app")),
            "{} must not sit inside an application bundle",
            root.display()
        );
    }
}
