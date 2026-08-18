//! Hosted Craftax GELO recipe. optimizers-beta owns algorithm execution and
//! canonical optimizer events; Containers owns child rollout streams.

use super::events::OptimizerEventDraft;
use super::{
    hosted_client::HostedOptimizerClient,
    ingest,
    models::{
        OptimizerCapabilities, OptimizerCreateRequest, OptimizerEventEnvelope,
        OptimizerExecutionBinding, OptimizerRecipeRunRequest, OptimizerResourceRef,
        OPTIMIZER_EVENT_SCHEMA_VERSION,
    },
    OptimizerService,
};
use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use std::time::Duration;
use tokio::{sync::watch, time::sleep};

pub const HOSTED_GELO_CRAFTAX_RECIPE: &str = "gelo.craftax.hosted.v1";
const DEFAULT_CRAFTAX_CONTAINERS_URL: &str = "http://127.0.0.1:8100";
const STATE_SLICES: &[&str] = &[
    "board",
    "themes",
    "candidates",
    "frontier",
    "data-engine",
    "agents",
];

pub(super) async fn reconcile_persisted(
    service: &OptimizerService,
    run_id: &str,
) -> Result<super::models::OptimizerRunRecord> {
    let run = service.get(run_id.to_string()).await?;
    if run.summary.get("recipeId").and_then(Value::as_str) != Some(HOSTED_GELO_CRAFTAX_RECIPE)
        || matches!(run.status.as_str(), "completed" | "failed" | "cancelled")
    {
        return Ok(run);
    }

    let client = HostedOptimizerClient::from_env()?;
    let existing = service
        .events_after(run_id.to_string(), 0, Some(5_000))
        .await?;
    let mut upstream_cursor = existing
        .iter()
        .filter_map(|event| {
            event
                .raw
                .get("sourceSequenceNumber")
                .and_then(Value::as_u64)
        })
        .max()
        .unwrap_or(0);
    let page = client
        .goex_optimizer_events_after(run_id, upstream_cursor, 5_000)
        .await?;
    ingest::ingest_event_page(service, run_id, "go-ex", &page, &mut upstream_cursor).await?;

    let remote = client.get_run(run_id).await?;
    let remote_status = remote
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("running");
    if let Ok(batch) = client.state_batch(run_id, STATE_SLICES).await {
        append_state_batch(service, run_id, remote_status, batch).await?;
    }
    if matches!(remote_status, "succeeded" | "failed" | "cancelled") {
        append_terminal(
            service,
            run_id,
            if remote_status == "succeeded" {
                "completed"
            } else {
                remote_status
            },
            remote
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string),
        )
        .await?;
    }
    service.get(run_id.to_string()).await
}

pub fn recipe_catalog() -> Value {
    let availability = if HostedOptimizerClient::from_env().is_ok() {
        "available"
    } else {
        "unavailable"
    };
    json!({
        "id": HOSTED_GELO_CRAFTAX_RECIPE,
        "title": "Hosted GELO · Craftax",
        "algorithmId": "go-ex",
        "task": "craftax",
        "availability": availability,
        "limits": {
            "producer": "optimizers-beta",
            "trainSeeds": [101, 102],
            "heldoutSeeds": [501],
            "maxSearchRollouts": 2,
            "proposerRounds": 1,
            "candidatesPerProposer": 1,
            "streamTransport": "sse",
            "streamSlot": "stream",
            "streamRetention": "run"
        },
        "credentialInputs": [],
        "prerequisites": [
            "SYNTH_OPTIMIZERS_BETA_URL (default http://127.0.0.1:8879)",
            "OPTIMIZERS_BETA_SERVICE_TOKEN",
            "SYNTH_CONTAINERS_CRAFTAX_URL (default http://127.0.0.1:8100)",
            "optimizers-beta proposer credentials"
        ]
    })
}

pub async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(
    super::models::OptimizerRunRecord,
    Option<crate::storage::AppEvent>,
)> {
    if request.recipe_id != HOSTED_GELO_CRAFTAX_RECIPE {
        bail!("unknown hosted GELO recipe: {}", request.recipe_id);
    }
    let client = HostedOptimizerClient::from_env()?;
    let container_url = craftax_containers_url()?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let run_id = format!("gelo_craftax_{}", &suffix[..8]);
    let config = craftax_config(&run_id, &container_url);
    let create = OptimizerCreateRequest {
        algorithm_id: "go-ex".into(),
        algorithm_version: Some("optimizers-beta-hosted-gelo-craftax-v1".into()),
        objective: Some("Craftax themes · hosted GELO search".into()),
        source: Some("hosted".into()),
        project_ref: Some("craftax@containers-streaming-v1".into()),
        session_ref: request.session_ref,
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![
            OptimizerExecutionBinding {
                kind: "optimizers_beta".into(),
                id: client.base_url.clone(),
                label: Some("optimizers-beta hosted GELO".into()),
                status: Some("starting".into()),
                metadata: json!({
                    "recipeId": HOSTED_GELO_CRAFTAX_RECIPE,
                    "algorithmId": "go-ex",
                }),
            },
            OptimizerExecutionBinding {
                kind: "container_service".into(),
                id: container_url.clone(),
                label: Some("Craftax · Containers streaming v1".into()),
                status: Some("ready".into()),
                metadata: json!({
                    "role": "child_evaluations",
                    "slot": "stream",
                    "transport": "sse",
                    "cursor": "sequence",
                    "worldRef": "world:craftax_default@symbolic_survival",
                    "policyRef": {"harness": "react", "config": "luna_med"},
                }),
            },
        ]),
        input_refs: Some(vec![
            OptimizerResourceRef {
                kind: "recipe".into(),
                id: HOSTED_GELO_CRAFTAX_RECIPE.into(),
                digest: None,
                role: Some("configuration".into()),
                title: Some("Bounded hosted Craftax GELO".into()),
                metadata: json!({"producer": "optimizers-beta"}),
            },
            OptimizerResourceRef {
                kind: "environment_ref".into(),
                id: "env:craftax_gold".into(),
                digest: None,
                role: Some("evaluation".into()),
                title: Some("Craftax EnvironmentService".into()),
                metadata: json!({"generation": "containers-streaming-v1"}),
            },
            OptimizerResourceRef {
                kind: "policy_ref".into(),
                id: "luna_med".into(),
                digest: None,
                role: Some("evaluation".into()),
                title: Some("Craftax GELO evaluation policy".into()),
                metadata: json!({"harness": "react", "config": "luna_med"}),
            },
        ]),
        capabilities: Some(OptimizerCapabilities::for_algorithm("go-ex")),
        summary: Some(json!({
            "recipeId": HOSTED_GELO_CRAFTAX_RECIPE,
            "producer": "optimizers-beta",
            "task": "craftax",
            "containersUrl": container_url,
            "streamingContract": "containers-streaming-v1",
        })),
        open_visual: request.open_visual.or(Some(true)),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    // Visual creation is part of the local create transaction and completes
    // before the hosted worker is submitted: visual-ready precedes paid work.
    let (run, event) = service.create(create).await?;
    spawn_worker(service, client, run_id, config).await;
    Ok((run, event))
}

fn craftax_containers_url() -> Result<String> {
    let url = std::env::var("SYNTH_CONTAINERS_CRAFTAX_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_CRAFTAX_CONTAINERS_URL.into());
    let host_port = url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    let address: std::net::SocketAddr = host_port
        .parse()
        .with_context(|| format!("Craftax Containers URL is not a host:port: {url}"))?;
    if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_err() {
        bail!(
            "Craftax Containers service is not listening at {url}; start the craftax_engine façade or set SYNTH_CONTAINERS_CRAFTAX_URL"
        );
    }
    Ok(url)
}

async fn spawn_worker(
    service: &OptimizerService,
    client: HostedOptimizerClient,
    run_id: String,
    config: Value,
) {
    let (cancel_tx, cancel_rx) = watch::channel(false);
    service
        .register_local_recipe(run_id.clone(), cancel_tx)
        .await;
    let worker = service.clone();
    tokio::spawn(async move {
        if let Err(error) =
            run_worker(worker.clone(), client, run_id.clone(), config, cancel_rx).await
        {
            eprintln!("hosted GELO worker {run_id} failed: {error:#}");
            let _ = append_terminal(&worker, &run_id, "failed", Some(error.to_string())).await;
        }
        worker.unregister_local_recipe(&run_id).await;
    });
}

async fn run_worker(
    service: OptimizerService,
    client: HostedOptimizerClient,
    run_id: String,
    config: Value,
    mut cancel: watch::Receiver<bool>,
) -> Result<()> {
    client.submit_json("go-ex", &run_id, config).await?;
    let mut upstream_cursor = 0u64;
    let mut last_state = String::new();
    loop {
        let page = client
            .goex_optimizer_events_after(&run_id, upstream_cursor, 5_000)
            .await?;
        ingest::ingest_event_page(&service, &run_id, "go-ex", &page, &mut upstream_cursor).await?;
        let remote = client.get_run(&run_id).await?;
        let remote_status = remote
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("running");
        if let Ok(batch) = client.state_batch(&run_id, STATE_SLICES).await {
            let encoded = serde_json::to_string(&batch)?;
            if encoded != last_state {
                append_state_batch(&service, &run_id, remote_status, batch).await?;
                last_state = encoded;
            }
        }
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_ok() && *cancel.borrow() {
                    let _ = client.cancel(&run_id).await;
                    append_terminal(&service, &run_id, "cancelled", None).await?;
                    return Ok(());
                }
            }
            _ = sleep(Duration::from_millis(600)) => {}
        }
        if matches!(remote_status, "succeeded" | "failed" | "cancelled") {
            let page = client
                .goex_optimizer_events_after(&run_id, upstream_cursor, 5_000)
                .await?;
            ingest::ingest_event_page(&service, &run_id, "go-ex", &page, &mut upstream_cursor)
                .await?;
            if let Ok(batch) = client.state_batch(&run_id, STATE_SLICES).await {
                append_state_batch(&service, &run_id, remote_status, batch).await?;
            }
            append_terminal(
                &service,
                &run_id,
                if remote_status == "succeeded" {
                    "completed"
                } else {
                    remote_status
                },
                remote
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            )
            .await?;
            return Ok(());
        }
    }
}

pub(super) async fn append_state_batch(
    service: &OptimizerService,
    run_id: &str,
    remote_status: &str,
    batch: Value,
) -> Result<()> {
    let mut snapshot = Map::new();
    snapshot.insert(
        "slices".into(),
        batch.get("slices").cloned().unwrap_or_else(|| json!({})),
    );
    snapshot.insert("status".into(), json!(remote_status));
    service
        .append_event_payloads(
            run_id.to_string(),
            vec![OptimizerEventDraft::new("goex.state.batch.updated", "go-ex")
                .level("info")
                .snapshot(snapshot)
                .raw(json!({"source": "optimizers-beta-state-batch"}))],
        )
        .await?;
    Ok(())
}

async fn append_terminal(
    service: &OptimizerService,
    run_id: &str,
    status: &str,
    error: Option<String>,
) -> Result<()> {
    let mut draft = OptimizerEventDraft::new(
        match status {
            "completed" => "optimizer.run.completed",
            "cancelled" => "optimizer.run.cancelled",
            _ => "optimizer.run.failed",
        },
        "go-ex",
    )
    .idempotency_key(format!("terminal:{status}"))
    .level(if status == "failed" { "error" } else { "info" })
    .delta(Map::from_iter([("status".into(), json!(status))]))
    .raw(json!({"source": "optimizers-beta"}));
    if let Some(message) = error {
        draft = draft.error(json!({ "message": message }));
    }
    service
        .append_event_payloads(run_id.to_string(), vec![draft])
        .await?;
    Ok(())
}

fn craftax_config(run_id: &str, container_url: &str) -> Value {
    let proposer = |role: &str, schema: &str| {
        json!({
            "model": "gpt-5.4-mini",
            "provider": "openai",
            "role": role,
            "output_schema": schema,
            "backend": "codex_app_server",
            "auth_mode": "auto",
            "runtime_substrate": "local",
            "approval_policy": "never",
            "sandbox_mode": "workspace-write",
            "reasoning_effort": "low",
            "timeout_seconds": 240
        })
    };
    let mut go_ex = json!({
        "max_rollouts": 4,
        "proposer_rounds": 1,
        "fresh_rollouts_per_round": 3,
        "segment_steps": 50,
        "resume_segment_steps": 25,
        "candidates_per_proposer": 1,
        "heldout_measurement_rollouts": 1,
        "holdout_consolidate_k": 1,
        "bootstrap_train_rollout_count": 1,
        "target_new_candidate_count": 1,
        "max_initial_rollouts_per_candidate": 2,
        "min_non_baseline_candidate_fresh_rollouts": 2,
        "max_llm_turns": 12,
        "max_actions_per_turn": 1,
        "submission_mode": "sync",
        "execute_live_proposers": true,
        "request_timeout_seconds": 240.0,
        "container_connect_timeout_seconds": 30.0,
        "allow_resume_fallback_to_fresh": false
    })
    .as_object()
    .cloned()
    .expect("GELO limits are an object");
    go_ex.extend(
        json!({
            "full_rollout_lane_enabled": false,
            "full_rollout_budget_per_round": 1,
            "full_rollout_initial_budget": 1,
            "full_rollout_cadence": 1,
            "preserve_search_measurement_split": true,
            "fresh_rollouts_per_parent": 1,
            "resume_rollouts_per_parent": 0,
            "full_rollout_concurrency": 2,
            "theme_rollout_concurrency": 2,
            "closeout_heldout_concurrency": 1,
            "closeout_heldout_candidate_parallelism": 1,
            "heldout_measurement_concurrency": 1,
            "heldout_measurement_candidate_parallelism": 1,
            "full_rollout_checkpoint_cadence": "none",
            "data_miner_min_new_checkpoints": 0,
            "data_miner_rollouts_per_job": 0
        })
        .as_object()
        .cloned()
        .expect("GELO rollout policy is an object"),
    );
    go_ex.extend(
        json!({
            "theme_finalize_min_checkpoints": 1,
            "theme_partials_per_candidate": 1,
            "theme_saturation_threshold": 0.9,
            "theme_saturation_min_rollouts": 1,
            "max_tentative_themes": 2,
            "theme_proposal_round_budget": 1,
            "theme_aux_budget_per_theme": 0,
            "promotion_min_seeds": 1,
            "promotion_margin": 0.01,
            "consolidation_budget_per_round": 0,
            "data_miner_authority": false,
            "all_candidate_holdout_seed_count": 1,
            "auto_aux_hill_climb_calls_per_round": 0,
            "resume_rollouts_per_round": 0,
            "terminator_default": "agent"
        })
        .as_object()
        .cloned()
        .expect("GELO proposal policy is an object"),
    );
    let go_ex = Value::Object(go_ex);
    let proposers = json!({
        "core_proposer": proposer("core_proposer", "goex.core_proposer.output.v1"),
        "aux_hill_climb_proposer": proposer("aux_hill_climb_proposer", "goex.aux_hill_climb.output.v1"),
        "aux_data_miner_proposer": proposer("aux_data_miner_proposer", "goex.aux_data_miner.v1"),
        "aux_consolidate_proposer": proposer("aux_consolidate_proposer", "goex.aux_consolidate.output.v1"),
        "aux_consolidate_hill_climb_proposer": proposer("aux_consolidate_hill_climb_proposer", "goex.aux_consolidate_hill_climb.output.v1"),
        "theme_verifier_agent": proposer("theme_verifier_agent", "goex_theme_verifier_result.v1"),
        "terminator_agent": proposer("terminator_agent", "goex_terminator_decision.v1")
    });
    json!({
        "run": {"run_id": run_id, "output_dir": ".out/workshop-hosted-gelo"},
        "container": {"url": container_url, "startup_timeout_seconds": 30},
        "taskset": {
            "train_seeds": [101, 102],
            "heldout_seeds": [501],
            "profile": "craftax_singleplayer_rust",
            "backend": "synth_containers",
            "reward_mode": "progress",
            "env_config": {"gamebench_task": "craftax-singleplayer", "substrate": "rust", "max_steps": 80},
            "context": {
                "task_family": "craftax-singleplayer",
                "required_candidate_kind": "prompt",
                "prompt_only_candidate_authoring": true,
                "forbid_code_policy_candidates": true,
                "containers_environment_ref": "env:craftax_gold",
                "containers_world_ref": "world:craftax_default@symbolic_survival",
                "containers_policy_harness": "react",
                "containers_policy_model": "gpt-5.6-luna",
                "containers_policy_effort": "medium"
            }
        },
        "policy": {
            "enabled": false, "model": "gpt-5.4-mini", "provider": "openai", "max_tokens": 64,
            "config": {"use_lm": false, "temperature": 0.0, "max_steps": 6}
        },
        "go_ex": go_ex,
        "seed_candidate": {
            "react_system_prompt": "Play Craftax using valid actions. Prioritize survival, wood, stone, tools, and measurable achievement progress."
        },
        "proposers": proposers,
        "cache": {"mode": "off"},
        "disk_budget": {"enabled": true, "soft_limit_gb": 2.0, "hard_limit_gb": 4.0}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_uses_hosted_beta_and_new_containers_stream_contract() {
        let config = craftax_config("gelo_craftax_test", "http://127.0.0.1:8100");
        assert_eq!(config["taskset"]["backend"], "synth_containers");
        assert_eq!(
            config["taskset"]["context"]["containers_policy_harness"],
            "react"
        );
        assert_eq!(config["go_ex"]["max_rollouts"], 4);
        assert_eq!(config["go_ex"]["max_initial_rollouts_per_candidate"], 2);
        assert_eq!(
            config["go_ex"]["min_non_baseline_candidate_fresh_rollouts"],
            2
        );
        assert_eq!(config["go_ex"]["resume_rollouts_per_parent"], 0);
    }
}
