//! Workshop-owned baseline container evals. These are not GEPA campaigns:
//! maxGenerations=0, no candidate generation, no uplift claim. They talk to a
//! registered container over HTTP, retain rewards/evidence, and mint a chat-owned
//! `experiment.overview.v1` visual. They do not require the Optimizers sidecar.

use super::{
    eval_relay::{self, RelayContext, RelaySettings},
    events::OptimizerEventDraft,
    models::{
        OptimizerCapabilities, OptimizerCreateRequest, OptimizerExecutionBinding,
        OptimizerRecipeRunRequest, OptimizerResourceRef, OptimizerRunRecord,
    },
    service::ChatVisualPublication,
    workspace_recipe::{self, WorkspaceRecipe},
    OptimizerManager, OptimizerService,
};
use crate::container_stream::{
    authoritative_poll_telemetry, declared_poll_url, declared_stream_descriptor,
    refuse_auto_transport, resolve_declared_url, wait_for_stream_subscribed, StreamDiagnostics,
    SUBSCRIBE_READY_TIMEOUT,
};
use crate::visuals::{VisualStatus, VisualUpdateRequest, VISUAL_BINDINGS_SCHEMA_VERSION};
use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::task::JoinSet;
use uuid::Uuid;

const EXPERIMENT_TEMPLATE: &str = "experiment.overview.v1";
const EXPERIMENT_SCHEMA: &str = "synth.experiment.overview.v1";
/// The per-seed drill-down. The overview stays the run-level surface; this is
/// what a seed row opens, and it is bound to the same run rather than to a
/// snapshot, so it keeps filling in while the campaign runs.
const WORKBENCH_TEMPLATE: &str = "craftax.trace_workbench.v1";
const EVAL_ALGORITHM_ID: &str = "eval";
const COST_CEILING_USD: f64 = 0.50;
const POLL_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(80);
const DEFAULT_BLOCKING_EVAL_HTTP_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// A failure of the evidence lane — durable events, projections, the terminal
/// manifest, or the chat-owned visual — as opposed to a failure of the compute.
///
/// The distinction is the point of failing closed. A rollout that errored is a
/// result; a rollout that succeeded and could not be recorded is a run with no
/// evidence, and calling that `completed` is what let a 10/10 Banking77 campaign
/// render as "0 trials" over an empty Outputs shelf.
#[derive(Debug)]
struct EvidenceLaneFailure {
    stage: &'static str,
    detail: String,
}

impl std::fmt::Display for EvidenceLaneFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} evidence failed: {}", self.stage, self.detail)
    }
}

impl std::error::Error for EvidenceLaneFailure {}

/// Run one evidence step, tagging its failure so the worker's terminal handler
/// settles the run as degraded rather than as a compute failure.
async fn evidence<T>(
    stage: &'static str,
    step: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    step.await.map_err(|error| {
        anyhow::Error::new(EvidenceLaneFailure {
            stage,
            detail: format!("{error:#}"),
        })
    })
}

#[derive(Clone)]
struct EvalSpec {
    recipe_id: String,
    family: String,
    title: String,
    question: String,
    world_ref: String,
    evaluation_plan_ref: String,
    harness: String,
    policy_config: String,
    policy: serde_json::Map<String, Value>,
    policy_code: Option<String>,
    provider: String,
    model: String,
    concurrency: usize,
    train: Vec<i64>,
    heldout: Vec<i64>,
    cost_ceiling_usd: f64,
    requires_credential_advertisement: bool,
    relay: RelaySettings,
}

impl EvalSpec {
    fn requires_credential_advertisement(&self) -> bool {
        self.requires_credential_advertisement
    }

    fn blocking_http_timeout(&self) -> Duration {
        let Some(per_call) = self
            .policy
            .get("timeout_seconds")
            .and_then(Value::as_f64)
        else {
            return DEFAULT_BLOCKING_EVAL_HTTP_TIMEOUT;
        };
        let calls = self
            .policy
            .get("max_calls")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1) as f64;
        Duration::from_secs_f64((per_call * calls + 60.0).clamp(60.0, 86_400.0))
    }

    fn from_workspace(recipe: &WorkspaceRecipe, workspace: &std::path::Path) -> Result<Self> {
        let policy_code = recipe
            .policy_source
            .as_deref()
            .map(|relative| {
                let path = workspace_recipe::resolve_workspace_path(workspace, relative)?;
                std::fs::read_to_string(&path)
                    .with_context(|| format!("read policy source {}", path.display()))
            })
            .transpose()?;
        Ok(Self {
            recipe_id: recipe.id.clone(),
            family: recipe.family.clone(),
            title: recipe.title.clone(),
            question: format!("Score the advertised {} policy on the declared eval pool.", recipe.family),
            world_ref: format!("world:{}@eval", recipe.family),
            evaluation_plan_ref: format!("{}_eval.v1", recipe.family),
            harness: recipe.harness.clone(),
            policy_config: recipe.policy_config.clone(),
            policy: recipe.policy.clone(),
            policy_code,
            provider: recipe.provider.clone(),
            model: recipe.model.clone(),
            concurrency: recipe.concurrency,
            train: recipe.train_seeds.clone(),
            heldout: recipe.heldout_seeds.clone(),
            cost_ceiling_usd: recipe.bounds.max_cost_usd,
            requires_credential_advertisement: recipe.requires_credential_advertisement,
            relay: recipe.relay,
        })
    }

    #[cfg(test)]
    fn classify_fixture() -> Self {
        Self {
            recipe_id: "eval.classify.baseline.v1".into(),
            family: "banking77".into(),
            title: "Classify baseline eval".into(),
            question: "Score the advertised classify policy.".into(),
            world_ref: "world:banking77@train".into(),
            evaluation_plan_ref: "banking77_eval.v1".into(),
            harness: "desktop_eval".into(),
            policy_config: "banking77_gpt_4_1_nano".into(),
            policy: serde_json::Map::new(),
            policy_code: None,
            provider: "openrouter".into(),
            model: "openai/gpt-4.1-nano".into(),
            concurrency: 10,
            train: (0..10).collect(),
            heldout: Vec::new(),
            cost_ceiling_usd: 0.50,
            requires_credential_advertisement: false,
            relay: RelaySettings::default(),
        }
    }

    #[cfg(test)]
    fn healthbench_fixture() -> Self {
        Self {
            recipe_id: "eval.healthbench.smoke.v1".into(),
            family: "healthbench".into(),
            title: "HealthBench smoke".into(),
            question: "Score the physician-rubric pool.".into(),
            world_ref: "world:healthbench@eval".into(),
            evaluation_plan_ref: "healthbench_eval.v1".into(),
            harness: "chat_completion".into(),
            policy_config: "openai_gpt41_mini".into(),
            policy: serde_json::Map::new(),
            policy_code: None,
            provider: "openai".into(),
            model: "gpt-4.1-mini-2025-04-14".into(),
            concurrency: 2,
            train: vec![0, 1],
            heldout: vec![100, 101],
            cost_ceiling_usd: 0.50,
            requires_credential_advertisement: true,
            relay: RelaySettings::default(),
        }
    }

    fn examples(&self) -> Vec<EvalExample> {
        self.train
            .iter()
            .map(|seed| EvalExample {
                pool: "train",
                seed: *seed,
            })
            .chain(self.heldout.iter().map(|seed| EvalExample {
                pool: "heldout",
                seed: *seed,
            }))
            .collect()
    }

    fn policy_config_body(&self, openai_base_url: Option<&str>) -> Option<Value> {
        if self.policy_config.is_empty() {
            return None;
        }
        let base_url = openai_base_url?;
        let mut config = self.policy.clone();
        config.extend(serde_json::Map::from_iter([
            ("provider".into(), json!(self.provider)),
            ("model".into(), json!(self.model)),
            ("base_url".into(), json!(base_url)),
            ("api_key_env".into(), json!("OPENAI_API_KEY")),
        ]));
        let config = Value::Object(config);
        Some(json!({
            "config_id": self.policy_config,
            "harness": self.harness,
            "config": config,
        }))
    }
}

#[derive(Clone, Copy)]
struct EvalExample {
    pool: &'static str,
    seed: i64,
}

#[derive(Clone)]
struct ReadyContainer {
    id: String,
    base_url: String,
    protocol: String,
    image_digest: Option<String>,
}

pub(super) async fn start(
    service: &OptimizerService,
    request: OptimizerRecipeRunRequest,
) -> Result<(OptimizerRunRecord, Option<crate::storage::AppEvent>)> {
    let session = request
        .session_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("workspace eval recipes require session_ref"))?;
    let workspace = workspace_recipe::require_session_workspace(service.database(), session)?;
    let recipe = workspace_recipe::find_recipe(&workspace, &request.recipe_id)?;
    if recipe.algorithm != workspace_recipe::AlgorithmKind::Eval {
        bail!("recipe `{}` is not an eval recipe", recipe.id);
    }
    let spec = EvalSpec::from_workspace(&recipe, &workspace)?;
    let container =
        find_ready_container(service, &spec.family, request.container_id.as_deref()).await?;
    preflight_container_credentials(&container, &spec).await?;
    let examples = spec.examples();
    let suffix = Uuid::new_v4().simple().to_string();
    let run_id = format!("opt_eval_{}_{}", spec.family, &suffix[..12]);
    let summary = json!({
        "recipeId": spec.recipe_id,
        "task": spec.family,
        "semantics": "baseline_eval",
        "containerProtocol": container.protocol,
        "containerId": container.id,
        "containerBaseUrl": container.base_url,
        "containerImageDigest": container.image_digest,
        "expectedVisual": EXPERIMENT_TEMPLATE,
        "policyRef": { "harness": spec.harness, "config": spec.policy_config },
        "taskPools": { "train": spec.train.len(), "heldout": spec.heldout.len() },
        "concurrency": spec.concurrency,
        "costCeilingUsd": COST_CEILING_USD,
        "records": [],
        "progress": { "completed": 0, "total": examples.len(), "failed": 0 },
    });
    let create = OptimizerCreateRequest {
        algorithm_id: EVAL_ALGORITHM_ID.into(),
        algorithm_version: Some("1".into()),
        objective: Some(spec.title.clone()),
        source: Some("local".into()),
        project_ref: Some(format!("{}@eval", spec.family)),
        session_ref: request.session_ref.clone(),
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![OptimizerExecutionBinding {
            kind: "container_http".into(),
            id: container.id.clone(),
            label: Some(format!("{} container", spec.family)),
            status: Some("starting".into()),
            metadata: json!({
                "recipeId": spec.recipe_id,
                "baseUrl": container.base_url,
            }),
        }]),
        input_refs: Some(vec![
            OptimizerResourceRef {
                kind: "recipe".into(),
                id: spec.recipe_id.clone(),
                digest: None,
                role: Some("configuration".into()),
                title: Some(spec.title.clone()),
                metadata: json!({ "semantics": "baseline_eval" }),
            },
            OptimizerResourceRef {
                kind: "container".into(),
                id: container.id.clone(),
                digest: None,
                role: Some("runtime".into()),
                title: Some(format!("Registered {} container", spec.family)),
                metadata: json!({ "baseUrl": container.base_url }),
            },
        ]),
        capabilities: Some(OptimizerCapabilities::for_algorithm(EVAL_ALGORITHM_ID)),
        summary: Some(summary),
        open_visual: Some(false),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    let (run, event) = service.create(create).await?;
    let visual_id = mint_experiment_visual(service, &run, &spec, examples.len()).await?;
    // Minted with the run, not when the first seed finishes: a workstation that
    // appears only after there is something to see cannot show a rollout
    // starting, which is the thing it exists to show.
    let workbench_id = mint_workbench_visual(service, &run, &spec).await?;
    let (cancel_tx, cancel_rx) = watch::channel(false);
    service
        .register_local_recipe(run.id.clone(), cancel_tx)
        .await;
    let worker = service.clone();
    let worker_run_id = run.id.clone();
    let planned_trials = examples.len();
    let worker_visual_id = visual_id.clone();
    let worker_workbench_id = workbench_id.clone();
    let worker_spec = spec.clone();
    tokio::spawn(async move {
        if let Err(error) = run_eval_worker(
            worker.clone(),
            worker_run_id.clone(),
            &worker_spec,
            container,
            examples,
            worker_visual_id.clone(),
            worker_workbench_id.clone(),
            cancel_rx,
        )
        .await
        {
            // A worker can fail before its first progress projection (for
            // example when Workshop refuses to mint a secrets proxy).  Its
            // durable run is terminal in that case, so its chat-owned visual
            // must not survive as a false live/running artifact.
            let _ = project_worker_failure_visual(
                &worker,
                &worker_run_id,
                &worker_spec,
                &worker_visual_id,
                &worker_workbench_id,
                planned_trials,
            )
            .await;
            settle_worker_failure(&worker, &worker_run_id, error).await;
        }
        worker.unregister_local_recipe(&worker_run_id).await;
    });
    Ok((service.get(run.id).await?, event))
}

async fn project_worker_failure_visual(
    service: &OptimizerService,
    run_id: &str,
    spec: &EvalSpec,
    visual_id: &str,
    workbench_id: &str,
    planned_trials: usize,
) -> Result<()> {
    let run = service.get(run_id.to_string()).await?;
    let records = run
        .summary
        .get("records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total = run
        .summary
        .pointer("/progress/total")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(planned_trials);
    let mean = mean_for_pool(&records, "train").or_else(|| mean_reward(&records));
    let (updated, event) = service
        .visuals()
        .update(
            visual_id.to_string(),
            VisualUpdateRequest {
                title: None,
                bindings: Some(experiment_bindings(
                    spec,
                    "failed",
                    records.len(),
                    total,
                    &records,
                    mean,
                    workbench_id,
                )),
                status: Some(VisualStatus::Failed),
                renderer_kind: None,
                message_id: None,
                run_id: None,
                trace_id: None,
                content: None,
                metadata: None,
                bump_revision: Some(true),
            },
        )
        .await?;
    service.publish_visual_event(event)?;
    debug_assert_eq!(updated.status, VisualStatus::Failed);
    Ok(())
}

/// Refuse admission when a lane the container declares has no credential.
///
/// A packaged HealthBench eval reached 4/4 terminal traces with every single
/// rollout failing `openai_api_key_missing`. Orchestration was flawless and the
/// answer was worthless: the run spent its wall-clock, produced four failure
/// records, and only then reported that the policy could not authenticate. A
/// missing credential is knowable before the first rollout, so it is refused
/// before the first rollout.
///
/// The check follows whatever the container advertises rather than a family
/// allowlist — HealthBench declares `policy` and `scorer`, and any family that
/// adopts the same `credential_present` contract is covered the day it does.
/// A container that declares no roles is not failed here; there is nothing to
/// fail it on, and inventing a refusal from absent metadata would block every
/// family that has not implemented the contract yet.
async fn preflight_container_credentials(container: &ReadyContainer, spec: &EvalSpec) -> Result<()> {
    let client = crate::http::http_client_with_timeout(Duration::from_secs(15));
    let info = client
        .get(format!("{}/info", container.base_url))
        .send()
        .await
        .with_context(|| format!("{} credential preflight GET /info", spec.family))?;
    if !info.status().is_success() {
        bail!(
            "{} credential preflight returned {}",
            spec.family,
            info.status()
        );
    }
    let info = info
        .json::<Value>()
        .await
        .with_context(|| format!("decode {} credential preflight", spec.family))?;
    let roles = info
        .pointer("/metadata/model_roles")
        .and_then(Value::as_object);
    let Some(roles) = roles else {
        // A family whose rollouts cannot run without a provider credential is
        // not admitted by a container that cannot say whether it has one.
        // Treating silence as readiness is what produced 4/4 terminal traces
        // with every rollout failing `openai_api_key_missing`: the pinned
        // Containers dev build (ac43172) emits no `model_roles` at all, so a
        // check that passes on absence passes on exactly the build in use.
        //
        // Families that need no credential are unaffected — there is nothing
        // for them to advertise.
        if spec.requires_credential_advertisement() {
            bail!(
                "{}",
                json!({
                    "code": "credential_readiness_unavailable",
                    "contract": format!("{}.credentials", spec.family),
                    "owner": spec.family,
                    "retryable": true,
                    "message": format!(
                        "the registered {} container does not advertise metadata.model_roles, so its credential readiness cannot be verified before dispatch; register a container that reports credential readiness by lane",
                        spec.family
                    ),
                })
            );
        }
        return Ok(());
    };
    // Sorted so the refusal names the same lane every time for a container
    // missing more than one credential.
    for (lane, role) in roles.iter().collect::<std::collections::BTreeMap<_, _>>() {
        let Some(role) = role.as_object() else {
            continue;
        };
        // Only a lane that declares the field is making a claim about it.
        let Some(present) = role.get("credential_present").and_then(Value::as_bool) else {
            continue;
        };
        if present {
            continue;
        }
        let credential = role
            .get("api_key_env")
            .and_then(Value::as_str)
            .unwrap_or("provider credential");
        // `scorer` is the container's word for the lane the product calls the
        // grader; the owner is named in the product's vocabulary so the message
        // points at something the reader can go configure.
        let owner = format!(
            "{}.{}",
            spec.family,
            if lane == "scorer" { "grader" } else { lane }
        );
        bail!(
            "{}",
            json!({
                "code": "credential_missing",
                "contract": format!("{}.credentials", spec.family),
                "owner": owner,
                "retryable": true,
                "message": format!("{owner} requires {credential}; configure it before starting the eval"),
            })
        );
    }
    Ok(())
}

/// Settle a worker that did not reach its own terminal event.
///
/// A compute failure is a failed run and says why. An evidence failure is not:
/// the rollouts may all have succeeded and been paid for, so the run settles as
/// `degraded` with a named, retryable diagnostic, and the records it did gather
/// stay in the summary. Either way the run reaches a terminal state — a worker
/// that dies silently leaves a card spinning forever.
async fn settle_worker_failure(service: &OptimizerService, run_id: &str, error: anyhow::Error) {
    if let Some(failure) = error.downcast_ref::<EvidenceLaneFailure>() {
        let stage = failure.stage;
        let detail = failure.detail.clone();
        if service
            .settle_evidence_degraded(run_id.to_string(), stage, detail)
            .await
            .is_ok()
        {
            return;
        }
        // The degraded settlement itself failed. Fall through: a terminal event
        // the user can see beats a run stuck at `running` with no explanation.
    }
    let _ = append_terminal(service, run_id, true, format!("{error:#}")).await;
}

/// The Trace V5 workstation for this run's seeds.
///
/// Bound as `optimizer_run` rather than as inline data: the workstation folds
/// the relayed event log itself, so it stays live without the worker having to
/// re-project a trajectory into bindings on every append.
async fn mint_workbench_visual(
    service: &OptimizerService,
    run: &OptimizerRunRecord,
    spec: &EvalSpec,
) -> Result<String> {
    let (visual_id, _event) = service
        .publish_chat_owned_visual(ChatVisualPublication {
            run_id: run.id.clone(),
            session_ref: run.session_ref.clone(),
            template_id: WORKBENCH_TEMPLATE.into(),
            title: format!("{} · trace workstation", spec.title),
            bindings: json!({
                "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
                "inputs": [{
                    "input": "optimizer_run",
                    "kind": "optimizer_run",
                    "source": run.id,
                }]
            }),
            metadata: json!({
                "optimizerRunId": run.id,
                "recipeId": spec.recipe_id,
                "semantics": "baseline_eval_trace",
            }),
            status: VisualStatus::Live,
            role: "trace_workbench".into(),
        })
        .await?;
    Ok(visual_id)
}

async fn mint_experiment_visual(
    service: &OptimizerService,
    run: &OptimizerRunRecord,
    spec: &EvalSpec,
    total: usize,
) -> Result<String> {
    // One publication, not five calls: mint-or-reuse, bind to the run, publish
    // the durable show, select it for the owning chat, and shelve it in that
    // chat's Outputs. Repeating it returns the same visual.
    let (visual_id, _event) = service
        .publish_chat_owned_visual(ChatVisualPublication {
            run_id: run.id.clone(),
            session_ref: run.session_ref.clone(),
            template_id: EXPERIMENT_TEMPLATE.into(),
            title: spec.title.clone(),
            // The overview is minted before the workstation exists, so the
            // first projection carries no drill-down target. `persist_progress`
            // supplies it from the next update onward; a seed row with no
            // target simply has no chip, rather than one that goes nowhere.
            bindings: experiment_bindings(spec, "running", 0, total, &[], None, ""),
            metadata: json!({
                "optimizerRunId": run.id,
                "recipeId": spec.recipe_id,
                "semantics": "baseline_eval",
            }),
            status: VisualStatus::Live,
            role: "primary".into(),
        })
        .await?;
    Ok(visual_id)
}

#[allow(clippy::too_many_arguments)]
fn experiment_bindings(
    spec: &EvalSpec,
    status: &str,
    completed: usize,
    total: usize,
    records: &[Value],
    mean_reward: Option<f64>,
    workbench_id: &str,
) -> Value {
    let train_mean = mean_for_pool(records, "train");
    let heldout_mean = mean_for_pool(records, "heldout");
    let rollouts = records.iter().map(|record| seed_row(record, workbench_id)).collect::<Vec<_>>();
    json!({
        "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
        "inputs": [{
            "input": "experiment",
            "kind": "inline",
            "schema": EXPERIMENT_SCHEMA,
            "data": {
                "schemaVersion": EXPERIMENT_SCHEMA,
                "experimentId": spec.recipe_id,
                "title": spec.title,
                "question": spec.question,
                "status": status,
                "progress": {
                    "phase": if status == "completed" { "done" } else { "scoring" },
                    "completed": completed,
                    "total": total,
                },
                "metrics": [
                    {"label": "Train mean", "value": train_mean, "detail": if train_mean.is_none() { "omitted until the split is complete" } else { "mean of present rewards" }},
                    {"label": "Heldout mean", "value": heldout_mean, "detail": if spec.heldout.is_empty() { "this recipe has no heldout pool" } else if heldout_mean.is_none() { "omitted until the split is complete" } else { "mean of present rewards" }},
                    {"label": "Overall mean", "value": mean_reward, "detail": "missing rewards stay missing"}
                ],
                "results": {
                    "rollouts": rollouts,
                },
                // Each seed opens the workstation, which is bound to the run
                // rather than to this snapshot — so a seed row is a way in, not
                // a second copy of the evidence that can disagree with it.
                "traces": { "prominence": "detail", "items": rollouts },
                "arms": [{
                    "id": "baseline",
                    "label": spec.policy_config,
                    "baseline": true,
                    "status": status,
                    "score": mean_reward
                }],
                "records": records,
                "limitations": [
                    "Baseline-only. No candidate generation and no uplift claim."
                ]
            }
        }]
    })
}

/// One seed's row in the overview, and its way into the workstation.
fn seed_row(record: &Value, workbench_id: &str) -> Value {
    let seed = record.get("seed").cloned().unwrap_or(Value::Null);
    let relay = record.get("relay");
    let frames = relay
        .and_then(|relay| relay.get("framesRetained"))
        .and_then(Value::as_u64);
    let declared = relay
        .and_then(|relay| relay.get("framesDeclared"))
        .and_then(Value::as_u64);
    // The frame count is stated as retained-of-declared. "0 native frames" was
    // once printed for a rollout that rendered thirteen, and a single number
    // cannot tell those two situations apart.
    let summary = match (frames, declared) {
        (Some(frames), Some(declared)) if declared > 0 => {
            format!("{frames}/{declared} native frames retained")
        }
        _ => "no relay receipt was recorded for this seed".to_string(),
    };
    json!({
        "id": record.get("rolloutId").cloned().unwrap_or(Value::Null),
        "label": match seed { Value::Null => "seed".to_string(), ref value => format!("Seed {value}") },
        "seed": seed,
        "reward": record.get("reward").cloned().unwrap_or(Value::Null),
        "status": record.get("status").cloned().unwrap_or(Value::Null),
        "stopReason": record.get("rewardStatus").cloned().unwrap_or(Value::Null),
        "achievements": record
            .get("checkpointAchievements")
            .and_then(Value::as_array)
            .map(Vec::len),
        "summary": summary,
        "visualId": workbench_id,
    })
}

fn mean_for_pool(records: &[Value], pool: &str) -> Option<f64> {
    let values = records
        .iter()
        .filter(|row| row.get("pool").and_then(Value::as_str) == Some(pool))
        .filter_map(|row| row.get("reward").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

async fn run_eval_worker(
    service: OptimizerService,
    run_id: String,
    spec: &EvalSpec,
    container: ReadyContainer,
    examples: Vec<EvalExample>,
    visual_id: String,
    workbench_id: String,
    cancel: watch::Receiver<bool>,
) -> Result<()> {
    let _revoke = crate::secrets::RevokeRunOnDrop(run_id.clone());
    let _ownership = service.hold_run_ownership(&run_id)?;
    evidence(
        "run_started",
        append_status(&service, &run_id, "optimizer.run.started", "running"),
    )
    .await?;
    // The plan is written before the first rollout, so a campaign that dies
    // mid-flight still has a denominator and the card can say 3 / 10 rather
    // than inventing a total or reporting nothing.
    evidence(
        "run_plan",
        append_eval_plan(&service, &run_id, spec, &examples),
    )
    .await?;
    // These recipes intentionally use the container's blocking rollout mode.
    // HealthBench can make one policy call plus many rubric-grader calls, so
    // the generic UI HTTP timeout is not an honest bound for this endpoint.
    let client = crate::http::http_client_with_timeout(spec.blocking_http_timeout());
    let info = match client
        .get(format!("{}/info", container.base_url))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            response.json::<Value>().await.unwrap_or(json!({}))
        }
        _ => json!({}),
    };
    let policy_pin =
        register_policy_pin(&client, &container.base_url, spec, &info, &run_id).await?;
    persist_policy_pin(&service, &run_id, &policy_pin).await?;
    // Frame bodies get their own client: it refuses redirects, so a container
    // event's `url` cannot steer Workshop's fetch off the container's origin.
    let media_client = eval_relay::frame_media_client()?;
    // Frame URLs are only ever resolved against this, never against whatever a
    // payload happens to contain.
    let media_origin = crate::visuals_ipc::validated_loopback_rollout_base(&container.base_url)?;
    let scale_leases = info
        .get("scale_leases")
        .or_else(|| info.pointer("/metadata/scale_leases"))
        .and_then(Value::as_u64)
        .unwrap_or(spec.concurrency as u64)
        .max(1) as usize;
    let permits = spec.concurrency.min(scale_leases).max(1);
    let total = examples.len();
    let mut remaining = examples.into_iter();
    let mut tasks: JoinSet<(EvalExample, Result<Value>)> = JoinSet::new();
    let mut records = Vec::new();
    let mut halt = None::<String>;

    loop {
        if *cancel.borrow() {
            bail!("container eval cancelled");
        }
        while tasks.len() < permits && halt.is_none() {
            if over_cost_ceiling(&records) {
                halt = Some(format!(
                    "cost ceiling ${COST_CEILING_USD:.2} reached; remaining rollouts were not dispatched"
                ));
                break;
            }
            let Some(example) = remaining.next() else {
                break;
            };
            let client = client.clone();
            let media_client = media_client.clone();
            let base = media_origin.clone();
            let pin = policy_pin.clone();
            let spec = spec.clone();
            let service = service.clone();
            let run_id = run_id.clone();
            let container_id = container.id.clone();
            let mut trial_cancel = cancel.clone();
            tasks.spawn(async move {
                let trial = TrialContext {
                    service: &service,
                    run_id: &run_id,
                    client: &client,
                    media_client: &media_client,
                    base: &base,
                    container_id: &container_id,
                    spec: &spec,
                    policy_pin: &pin,
                };
                let result = run_one_example(&trial, example, &mut trial_cancel).await;
                (example, result)
            });
        }
        let Some(joined) = tasks.join_next().await else {
            break;
        };
        let record = match joined {
            Ok((_example, Ok(record))) => record,
            Ok((example, Err(error))) => {
                failed_record(example, spec, &policy_pin, format!("{error:#}"))
            }
            Err(error) => json!({
                "status": "failed",
                "error": error.to_string(),
                "policyRef": policy_pin,
            }),
        };
        evidence(
            "trial_terminal",
            append_eval_terminal(&service, &run_id, spec, &record),
        )
        .await?;
        records.push(record);
        evidence(
            "progress_projection",
            persist_progress(
                &service,
                &run_id,
                spec,
                &visual_id,
                &workbench_id,
                &records,
                total,
                "running",
            ),
        )
        .await?;
    }

    for example in remaining {
        records.push(failed_record(
            example,
            spec,
            &policy_pin,
            halt.clone()
                .unwrap_or_else(|| "required rollout was not dispatched".into()),
        ));
    }

    let failed = records
        .iter()
        .filter(|row| !is_successful_eval_record(row))
        .count();
    let budget_exceeded = over_cost_ceiling(&records);
    let status = if failed == 0 && records.len() == total && !budget_exceeded {
        "completed"
    } else {
        "failed"
    };
    evidence(
        "progress_projection",
        persist_progress(
            &service,
            &run_id,
            spec,
            &visual_id,
            &workbench_id,
            &records,
            total,
            status,
        ),
    )
    .await?;
    evidence(
        "selection",
        append_eval_selection(&service, &run_id, status, mean_reward(&records)),
    )
    .await?;
    let mut detail = String::new();
    if status == "failed" {
        let mut parts = Vec::new();
        if failed != 0 || records.len() != total {
            parts.push(format!("{failed} of {total} required rollouts failed"));
        }
        if budget_exceeded {
            parts.push(format!("cost exceeded ${COST_CEILING_USD:.2}"));
        }
        if let Some(halt) = halt {
            parts.push(halt);
        }
        detail = parts.join("; ");
    }
    evidence(
        "run_terminal",
        append_terminal(&service, &run_id, status == "failed", detail),
    )
    .await?;
    Ok(())
}

/// The campaign plan and one queued event per planned trial, appended as one
/// batch. The service allocates the sequence numbers: this worker never reads
/// the run's cursor, so it cannot compute numbers a concurrent writer has
/// already taken.
async fn append_eval_plan(
    service: &OptimizerService,
    run_id: &str,
    spec: &EvalSpec,
    examples: &[EvalExample],
) -> Result<()> {
    let mut drafts = vec![
        OptimizerEventDraft::new("eval.run.planned", EVAL_ALGORITHM_ID)
            .idempotency_key("eval:plan")
            .snapshot(Map::from_iter([
                ("parallelism".into(), json!(spec.concurrency)),
                ("global_capacity".into(), json!(spec.concurrency)),
                ("planned_trials".into(), json!(examples.len())),
                (
                    "candidates".into(),
                    json!([{"id": spec.policy_config, "label": spec.policy_config}]),
                ),
            ]))
            .raw(json!({ "source": "container_eval" })),
    ];
    for example in examples {
        let trial_id = format!("trial:{}:{}", spec.family, example.seed);
        drafts.push(
            OptimizerEventDraft::new("eval.trial.queued", EVAL_ALGORITHM_ID)
                .idempotency_key(format!("eval:queued:{trial_id}"))
                .delta(Map::from_iter([
                    ("trial_id".into(), json!(trial_id)),
                    ("candidate_id".into(), json!(spec.policy_config)),
                    ("seed".into(), json!(example.seed)),
                    ("scenario".into(), json!(spec.family)),
                    ("stage".into(), json!("screen")),
                ]))
                .raw(json!({ "source": "container_eval" })),
        );
    }
    service
        .append_event_payloads(run_id.to_string(), drafts)
        .await?;
    Ok(())
}

async fn append_eval_terminal(
    service: &OptimizerService,
    run_id: &str,
    spec: &EvalSpec,
    record: &Value,
) -> Result<()> {
    let seed = record.get("seed").cloned().unwrap_or(Value::Null);
    let id = format!("trial:{}:{}", spec.family, seed);
    let valid = is_successful_eval_record(record);
    let mut draft = OptimizerEventDraft::new("eval.trial.terminal", EVAL_ALGORITHM_ID)
        // One settlement per trial. A retried append of the same trial is the
        // same fact, not a second completion.
        .idempotency_key(format!("eval:terminal:{id}"))
        .item(json!({
            "kind": "trial",
            "id": id,
            "status": if valid { "evaluated" } else { "failed" },
            "valid": valid,
            "candidateId": spec.policy_config,
            "stage": "screen",
            "seed": seed,
            "scenario": spec.family,
            "metrics": { "reward": record.get("reward").cloned().unwrap_or(Value::Null) },
            "raw": record,
        }))
        .raw(json!({ "source": "container_eval" }));
    if !valid {
        draft = draft.level("warn");
    }
    service
        .append_event_payloads(run_id.to_string(), vec![draft])
        .await?;
    Ok(())
}

async fn append_eval_selection(
    service: &OptimizerService,
    run_id: &str,
    status: &str,
    mean: Option<f64>,
) -> Result<()> {
    let run = service.get(run_id.to_string()).await?;
    let selection = json!({
        "status": if status == "completed" { "inconclusive" } else { "failed" },
        "winnerId": null,
        "baselineId": run.summary.pointer("/policyRef/config").cloned().unwrap_or(Value::Null),
        "primaryMetric": "mean_reward",
        "lift": null,
        "score": mean,
        "reason": "baseline-only evaluation; no promotion decision",
    });
    service
        .append_event_payloads(
            run_id.to_string(),
            vec![
                OptimizerEventDraft::new("eval.selection.completed", EVAL_ALGORITHM_ID)
                    .idempotency_key("eval:selection")
                    .snapshot(Map::from_iter([("selection".into(), selection)]))
                    .raw(json!({ "source": "container_eval" })),
            ],
        )
        .await?;
    Ok(())
}

fn secrets_proxy_error(code: &str, message: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{}",
        json!({
            "code": code,
            "contract": "workshop.secrets_proxy",
            "retryable": code == "secrets_proxy_unavailable",
            "message": message,
        })
    )
}

fn container_proxy_policy(spec: &EvalSpec) -> crate::secrets::SecretsUsePolicy {
    let mut policy = crate::secrets::SecretsUsePolicy::default();
    policy.operations = vec!["chat.completions.create".into()];
    if !spec.model.is_empty() {
        policy.models = vec![spec.model.clone()];
    }
    policy.max_cost_usd = spec.cost_ceiling_usd;
    let trials = spec.examples().len() as u64;
    let calls_per_trial = spec
        .policy
        .get("max_calls")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1);
    let total_calls = trials.saturating_mul(calls_per_trial).max(1);
    policy.max_calls = total_calls.min(u32::MAX as u64) as u32;
    if let Some(context_tokens) = spec
        .policy
        .get("context_token_budget")
        .and_then(Value::as_u64)
    {
        policy.max_input_tokens = total_calls.saturating_mul(context_tokens);
    }
    let answer_tokens = spec
        .policy
        .get("answer_max_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let thinking_tokens = spec
        .policy
        .get("thinking_budget")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if answer_tokens > 0 || thinking_tokens > 0 {
        policy.max_output_tokens = total_calls
            .saturating_mul(answer_tokens.saturating_add(thinking_tokens));
    }
    policy
}

fn container_openai_proxy_base(run_id: &str, spec: &EvalSpec) -> Result<String> {
    let secrets = crate::secrets::live().ok_or_else(|| {
        secrets_proxy_error(
            "secrets_proxy_unavailable",
            "this recipe requires the Workshop secrets proxy",
        )
    })?;
    let env = secrets
        .workload_env(
            spec.provider.as_str(),
            run_id,
            spec.recipe_id.as_str(),
            container_proxy_policy(spec),
            "eval",
        )
        .map_err(|error| secrets_proxy_error("secrets_proxy_denied", &error.to_string()))?;
    let base = env.container_openai_base_url.clone().ok_or_else(|| {
        secrets_proxy_error(
            "secrets_proxy_route_unbound",
            "Workshop did not bind a container-reachable provider proxy base URL",
        )
    })?;
    if base.contains("api.openai.com")
        || base.contains("127.0.0.1")
        || base.contains("localhost")
        || !base.contains("/cap/wcap_")
    {
        return Err(secrets_proxy_error(
            "secrets_proxy_unreachable",
            "policy base_url must be the container-reachable Workshop proxy",
        ));
    }
    Ok(base)
}

async fn register_policy_pin(
    client: &reqwest::Client,
    base: &str,
    spec: &EvalSpec,
    container_info: &Value,
    run_id: &str,
) -> Result<Value> {
    let pin = json!({ "harness": spec.harness, "config": spec.policy_config });
    let advertised = container_info
        .pointer("/capabilities/policy_refs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|entry| {
            entry.get("harness").and_then(Value::as_str) == Some(spec.harness.as_str())
                && entry.get("config").and_then(Value::as_str) == Some(spec.policy_config.as_str())
        });
    if !spec.requires_credential_advertisement() && advertised {
        return Ok(json!({
            "harness": spec.harness,
            "config": spec.policy_config,
            "configId": spec.policy_config,
            "immutable": true,
            "authority": "container_advertisement",
        }));
    }
    // Credential advertisement and policy registration are independent.
    // A container-backed policy may intentionally hold no provider secret of
    // its own while still requiring Workshop to register a scoped proxy route.
    // Only an already-advertised immutable config may skip registration.
    let openai_base = Some(container_openai_proxy_base(run_id, spec)?);
    if let Some(code) = spec.policy_code.as_deref() {
        let response = client
            .put(format!("{base}/policy"))
            .json(&json!({ "code": code, "harness": spec.harness }))
            .send()
            .await
            .context("PUT /policy")?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            bail!("policy source registration failed: {status} {text}");
        }
    }
    let Some(body) = spec.policy_config_body(openai_base.as_deref()) else {
        if spec.requires_credential_advertisement() {
            return Err(secrets_proxy_error(
                "secrets_proxy_route_unbound",
                "this recipe requires a Workshop proxy base_url; refusing a public provider origin",
            ));
        }
        return Ok(pin);
    };
    let response = client
        .post(format!("{base}/policy-configs"))
        .json(&body)
        .send()
        .await
        .context("POST /policy-configs")?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        bail!("policy config registration failed: {status} {text}");
    }
    let registered = response
        .json::<Value>()
        .await
        .context("decode /policy-configs")?;
    let returned_id = registered
        .get("config_id")
        .or_else(|| registered.get("configId"))
        .and_then(Value::as_str);
    if returned_id != Some(spec.policy_config.as_str()) {
        bail!(
            "policy config identity mismatch: wanted {}, got {returned_id:?}",
            spec.policy_config
        );
    }
    if spec.policy_code.is_some() {
        let response = client
            .post(format!("{base}/policy/restart"))
            .send()
            .await
            .context("POST /policy/restart")?;
        if !response.status().is_success() {
            bail!("policy restart failed: {}", response.status());
        }
    }
    Ok(json!({
        "harness": spec.harness,
        "config": spec.policy_config,
        "configId": spec.policy_config,
        "immutable": true,
        "model": body.pointer("/config/model").cloned().unwrap_or(Value::Null),
        "provider": body.pointer("/config/provider").cloned().unwrap_or(Value::Null),
    }))
}

async fn persist_policy_pin(
    service: &OptimizerService,
    run_id: &str,
    policy_pin: &Value,
) -> Result<()> {
    let policy_pin = policy_pin.clone();
    service
        .patch_run(run_id.to_string(), move |run| {
            let mut summary = run.summary.as_object().cloned().unwrap_or_default();
            summary.insert("policyPin".into(), policy_pin.clone());
            summary.insert("policyRef".into(), policy_pin);
            run.summary = Value::Object(summary);
            Ok(())
        })
        .await?;
    Ok(())
}

fn failed_record(example: EvalExample, spec: &EvalSpec, policy_pin: &Value, error: String) -> Value {
    json!({
        "pool": example.pool,
        "seed": example.seed,
        "taskInstanceId": format!("seed:{}", example.seed),
        "status": "failed",
        "error": error,
        "policyRef": policy_pin,
        "worldRef": spec.world_ref,
    })
}

fn is_successful_eval_record(row: &Value) -> bool {
    row.get("status").and_then(Value::as_str) == Some("completed")
}

fn over_cost_ceiling(records: &[Value]) -> bool {
    recorded_cost_usd(records).is_some_and(|cost| cost >= COST_CEILING_USD)
}

fn recorded_cost_usd(records: &[Value]) -> Option<f64> {
    let mut total = 0.0;
    let mut saw = false;
    for record in records {
        let usage = record.get("usage").unwrap_or(&Value::Null);
        for lane in [usage.get("policy"), usage.get("grader"), Some(usage)] {
            if let Some(cost) = lane.and_then(lane_cost_usd) {
                total += cost;
                saw = true;
            }
        }
    }
    saw.then_some(total)
}

fn lane_cost_usd(blob: &Value) -> Option<f64> {
    if blob.get("policy").is_some() || blob.get("grader").is_some() {
        return None;
    }
    blob.get("cost_usd")
        .or_else(|| blob.get("costUsd"))
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
}

#[allow(clippy::too_many_arguments)]
async fn persist_progress(
    service: &OptimizerService,
    run_id: &str,
    spec: &EvalSpec,
    visual_id: &str,
    workbench_id: &str,
    records: &[Value],
    total: usize,
    status: &str,
) -> Result<()> {
    let completed = records.len();
    let mean = mean_for_pool(records, "train").or_else(|| mean_reward(records));
    let usage = usage_from_records(records);
    let failed_count = records
        .iter()
        .filter(|row| !is_successful_eval_record(row))
        .count();
    let records_value = json!(records);
    let status_value = status.to_string();
    // Patched under the durable record rather than written from a snapshot: a
    // worker that read the run before its own `started` event must not restore
    // that reading over the events it has since appended.
    service
        .patch_run(run_id.to_string(), move |run| {
            let mut summary = run.summary.as_object().cloned().unwrap_or_default();
            summary.insert("records".into(), records_value);
            summary.insert(
                "progress".into(),
                json!({
                    "completed": completed,
                    "total": total,
                    "failed": failed_count
                }),
            );
            if let Some(mean) = mean {
                summary.insert("meanReward".into(), json!(mean));
            }
            summary.insert("evalStatus".into(), json!(status_value));
            summary.insert("costCeilingUsd".into(), json!(COST_CEILING_USD));
            if let Some(cost) = usage.cost_usd {
                summary.insert("costUsd".into(), json!(cost));
            }
            summary.insert(
                "usageLanes".into(),
                json!({
                    "policy": usage.extra.get("policyUsage").cloned().unwrap_or(Value::Null),
                    "grader": usage.extra.get("graderUsage").cloned().unwrap_or(Value::Null),
                }),
            );
            run.summary = Value::Object(summary);
            run.usage = usage;
            Ok(())
        })
        .await?;

    let visual_status = if status == "failed" {
        VisualStatus::Failed
    } else if status == "completed" {
        VisualStatus::Saved
    } else {
        VisualStatus::Live
    };
    let visual_update = service
        .visuals()
        .update(
            visual_id.to_string(),
            VisualUpdateRequest {
                title: None,
                bindings: Some(experiment_bindings(
                    spec,
                    status,
                    completed,
                    total,
                    records,
                    mean,
                    workbench_id,
                )),
                status: Some(visual_status),
                renderer_kind: None,
                message_id: None,
                run_id: None,
                trace_id: None,
                content: None,
                metadata: None,
                bump_revision: Some(true),
            },
        )
        .await;
    match visual_update {
        Ok((_, event)) => service.publish_visual_event(event)?,
        Err(error) => {
            let message = error.to_string();
            service
                .patch_run(run_id.to_string(), move |run| {
                    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
                    summary.insert("visualProjectionError".into(), json!(message));
                    run.summary = Value::Object(summary);
                    Ok(())
                })
                .await?;
            if status != "running" {
                bail!("experiment visual projection failed: {error}");
            }
        }
    }
    Ok(())
}

fn usage_from_records(records: &[Value]) -> super::models::OptimizerUsageSummary {
    let mut usage = super::models::OptimizerUsageSummary::default();
    usage.rollouts = records.len() as u64;
    let mut policy = LaneUsage::default();
    let mut grader = LaneUsage::default();
    let mut cost_sum = 0.0;
    let mut cost_complete = true;
    let mut saw_cost = false;
    for record in records {
        let blob = record.get("usage").cloned().unwrap_or(Value::Null);
        if blob.get("policy").is_some() || blob.get("grader").is_some() {
            add_lane(&mut policy, blob.get("policy"));
            add_lane(&mut grader, blob.get("grader"));
        } else {
            add_lane(&mut policy, Some(&blob));
        }
    }
    usage.prompt_tokens = policy.prompt_tokens + grader.prompt_tokens;
    usage.completion_tokens = policy.completion_tokens + grader.completion_tokens;
    for lane in [&policy, &grader] {
        match lane.cost_usd {
            Some(cost) => {
                cost_sum += cost;
                saw_cost = true;
            }
            None if lane.saw_tokens => cost_complete = false,
            None => {}
        }
    }
    usage.cost_usd = if saw_cost && cost_complete {
        Some(cost_sum)
    } else if saw_cost && !cost_complete {
        None
    } else {
        None
    };
    if saw_cost {
        usage
            .extra
            .insert("costTelemetryComplete".into(), json!(cost_complete));
    }
    usage
        .extra
        .insert("costCeilingUsd".into(), json!(COST_CEILING_USD));
    usage.extra.insert("policyUsage".into(), policy.to_json());
    usage.extra.insert("graderUsage".into(), grader.to_json());
    usage
}

#[derive(Default)]
struct LaneUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    cost_usd: Option<f64>,
    saw_tokens: bool,
}

impl LaneUsage {
    fn to_json(&self) -> Value {
        json!({
            "promptTokens": self.prompt_tokens,
            "completionTokens": self.completion_tokens,
            "costUsd": self.cost_usd,
        })
    }
}

fn add_lane(lane: &mut LaneUsage, blob: Option<&Value>) {
    let Some(blob) = blob.filter(|value| value.is_object()) else {
        return;
    };
    if let Some(tokens) = u64_field(blob, &["prompt_tokens", "promptTokens"]) {
        lane.prompt_tokens += tokens;
        lane.saw_tokens = true;
    }
    if let Some(tokens) = u64_field(blob, &["completion_tokens", "completionTokens"]) {
        lane.completion_tokens += tokens;
        lane.saw_tokens = true;
    }
    if let Some(cost) = lane_cost_usd(blob) {
        lane.cost_usd = Some(lane.cost_usd.unwrap_or(0.0) + cost);
    }
}

fn u64_field(blob: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| blob.get(*key).and_then(Value::as_u64))
}

fn mean_reward(records: &[Value]) -> Option<f64> {
    let values = records
        .iter()
        .filter_map(|row| row.get("reward").and_then(Value::as_f64))
        .collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

/// Everything one trial needs that is not the trial itself.
///
/// Carried as a struct rather than eight positional arguments because the
/// worker now hands the rollout its own identity (run, trial, visual clock) as
/// well as its transport: the relay files events against a run, and a relay
/// that could not name its run would have nowhere to put them.
struct TrialContext<'a> {
    service: &'a OptimizerService,
    run_id: &'a str,
    client: &'a reqwest::Client,
    media_client: &'a reqwest::Client,
    base: &'a str,
    container_id: &'a str,
    spec: &'a EvalSpec,
    policy_pin: &'a Value,
}

/// Run one seed, relaying the container's event journal as it happens.
///
/// Order matters and is the whole point:
///
/// 1. `prepare`, then wait for the declared subscribe ACK (C1-08, unchanged).
/// 2. `eval.trial.started`, so a trial exists in the log before its events do.
/// 3. Start the blocking `POST /rollouts` as a *concurrent future*.
/// 4. Drain the declared poll URL beside it, relaying every semantic event and
///    fetching every native PNG into the content store.
/// 5. When both the request has settled and the journal has closed, fetch the
///    reward, import the sealed Trace V5 bundle, and settle the trial.
///
/// Step 3 keeps `submission_mode: "sync"`. Containers admits an async rollout
/// without running it until the separate completion route is called, so the
/// asynchronous spelling would buy a live pane and lose the rollout.
async fn run_one_example(
    ctx: &TrialContext<'_>,
    example: EvalExample,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Value> {
    let spec = ctx.spec;
    let telemetry = {
        let mut telemetry = authoritative_poll_telemetry();
        if let Some(object) = telemetry.as_object_mut() {
            object.insert("retention".into(), json!("run"));
            // Frames are asked for when the recipe retains them. The eval lane
            // used to pin `frame.enabled: false` and then report "0 native
            // frames", which was true and entirely self-inflicted.
            object.insert("frame".into(), spec.relay.telemetry_frame());
        }
        refuse_auto_transport(&telemetry)?;
        telemetry
    };
    let rollout_id = format!(
        "roll_{}_{}_{}_{}",
        spec.family,
        example.pool,
        example.seed,
        &Uuid::new_v4().simple().to_string()[..8]
    );
    let trial_id = format!("trial:{}:{}", spec.family, example.seed);
    let prepare = ctx
        .client
        .post(format!("{}/rollouts/prepare", ctx.base))
        .json(&json!({ "rollout_id": rollout_id, "telemetry": telemetry }))
        .send()
        .await
        .context("POST /rollouts/prepare")?;
    if !prepare.status().is_success() {
        bail!(
            "prepare failed for {} seed {}: {}",
            example.pool,
            example.seed,
            prepare.status()
        );
    }
    let prepared = prepare.json::<Value>().await?;
    let stream =
        declared_stream_descriptor(&prepared)?.context("prepare omitted stream descriptor")?;
    let poll_url = resolve_declared_url(ctx.base, &declared_poll_url(&stream)?)?;
    wait_for_stream_subscribed(
        ctx.client,
        &poll_url,
        SUBSCRIBE_READY_TIMEOUT,
        &StreamDiagnostics::none().with_rollout(&rollout_id),
    )
    .await?;

    // The trial exists in the durable log before the first container event
    // does. Without this, a relayed event would be the first thing a reader
    // ever heard about the trial, and the pane would have to invent the row.
    eval_relay::append_trial_started(
        ctx.service,
        ctx.run_id,
        &trial_id,
        &rollout_id,
        example.seed,
        example.pool,
        &spec.family,
        &spec.policy_config,
    )
    .await?;

    let start_body = json!({
        "rollout_id": rollout_id,
        "submission_mode": "sync",
        "slot": "stream",
        "telemetry": telemetry,
        "task_instance_id": format!("seed:{}", example.seed),
        "world_ref": spec.world_ref,
        "evaluation_plan_ref": spec.evaluation_plan_ref,
        "policy_ref": { "harness": spec.harness, "config": spec.policy_config },
    });
    let relay_ctx = RelayContext {
        service: ctx.service,
        run_id: ctx.run_id,
        trial_id: &trial_id,
        rollout_id: &rollout_id,
        seed: example.seed,
        pool: example.pool,
        scenario: &spec.family,
        base: ctx.base,
        poll_url: &poll_url,
        client: ctx.client,
        media_client: ctx.media_client,
        settings: spec.relay,
    };
    let rollout = start_rollout(ctx.client, ctx.base, &start_body, example);
    let (started, relay) = eval_relay::relay_while(&relay_ctx, rollout, cancel).await;
    // Bounds that were hit are recorded before the trial settles, so a
    // degraded frame budget is visible even if the terminal settlement fails.
    eval_relay::append_degradations(ctx.service, ctx.run_id, &trial_id, &relay).await?;
    let mut state = started?;

    if !rollout_terminal(&state) {
        state = poll_until_terminal(ctx.client, ctx.base, &rollout_id).await?;
    }
    let reward = fetch_reward(
        ctx.client,
        ctx.base,
        &rollout_id,
        spec.evaluation_plan_ref.as_str(),
    )
    .await;
    let usage = state.get("usage").cloned().unwrap_or(Value::Null);
    let retained = fetch_retained_rollout_state(ctx.client, &poll_url)
        .await
        .unwrap_or_default();
    // Import the sealed bundle now, while the container is still running. A
    // replay that only works until the container stops is not a replay, and
    // Workshop already owns the import — the eval worker simply never called it.
    let sealed = import_sealed_trace(ctx, &rollout_id, &trial_id).await;
    Ok(json!({
        "rolloutId": rollout_id,
        "trialId": trial_id,
        "pool": example.pool,
        "seed": example.seed,
        "taskInstanceId": format!("seed:{}", example.seed),
        "status": state.get("status").cloned().unwrap_or(json!("completed")),
        "terminated": rollout_terminal(&state),
        "reward": reward.get("reward").cloned().unwrap_or(Value::Null),
        "rewardStatus": reward.get("status").cloned().unwrap_or(json!("absent")),
        "usage": usage,
        "policyRef": ctx.policy_pin,
        "worldRef": spec.world_ref,
        "trace": state.get("trace").cloned().unwrap_or(Value::Null),
        "checkpointObservation": retained.get("observation").cloned().unwrap_or(Value::Null),
        "checkpointAchievements": retained.get("achievements").cloned().unwrap_or(Value::Null),
        "relay": relay.to_json(),
        "sealedTrace": sealed,
        "evidence": {
            "containerId": ctx.container_id,
            "eventsUrl": poll_url,
            "rewardUrl": format!("{}/rollouts/{rollout_id}/reward", ctx.base),
        }
    }))
}

/// The blocking rollout request, unchanged in behaviour and error semantics.
/// Split out only so it can be polled as a future beside the relay.
async fn start_rollout(
    client: &reqwest::Client,
    base: &str,
    body: &Value,
    example: EvalExample,
) -> Result<Value> {
    let started = client
        .post(format!("{base}/rollouts"))
        .json(body)
        .send()
        .await
        .context("POST /rollouts")?;
    if !started.status().is_success() {
        let status = started.status();
        let text = started.text().await.unwrap_or_default();
        bail!(
            "POST /rollouts failed for {} seed {}: {status} {text}",
            example.pool,
            example.seed
        );
    }
    Ok(started.json::<Value>().await?)
}

/// Pull the sealed Trace V5 bundle into Workshop for this rollout.
///
/// Best-effort and reported either way. A trial whose bundle could not be
/// imported is still a scored trial; what it loses is offline replay, and the
/// record says so rather than leaving the workbench to discover it.
async fn import_sealed_trace(ctx: &TrialContext<'_>, rollout_id: &str, trial_id: &str) -> Value {
    match ctx
        .service
        .import_container_trace(ctx.container_id, rollout_id, ctx.run_id, trial_id)
        .await
    {
        Ok(result) => result,
        Err(error) => json!({
            "imported": false,
            "containerId": ctx.container_id,
            "rolloutId": rollout_id,
            "error": format!("{error:#}"),
        }),
    }
}

async fn fetch_retained_rollout_state(
    client: &reqwest::Client,
    poll_url: &str,
) -> Result<serde_json::Map<String, Value>> {
    let response = client
        .get(poll_url)
        .send()
        .await
        .context("GET retained rollout events")?;
    if !response.status().is_success() {
        bail!("retained rollout events returned {}", response.status());
    }
    let page = response.json::<Value>().await?;
    Ok(retained_rollout_state(&page))
}

fn retained_rollout_state(page: &Value) -> serde_json::Map<String, Value> {
    let events = page
        .get("events")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for event in events.iter().rev() {
        if event.get("kind").and_then(Value::as_str) != Some("observation") {
            continue;
        }
        let payload = event.get("payload").unwrap_or(&Value::Null);
        let readout = payload
            .get("readout")
            .or_else(|| payload.pointer("/payload/readout"));
        let Some(readout) = readout else {
            continue;
        };
        let observation = readout
            .get("observation_text")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty());
        if let Some(observation) = observation {
            let mut retained = serde_json::Map::new();
            retained.insert("observation".into(), json!(observation));
            if let Some(achievements) = readout.get("achievements") {
                retained.insert("achievements".into(), achievements.clone());
            }
            return retained;
        }
    }
    serde_json::Map::new()
}

fn rollout_terminal(state: &Value) -> bool {
    state.get("terminated").and_then(Value::as_bool) == Some(true)
        || matches!(
            state.get("status").and_then(Value::as_str),
            Some("completed" | "failed" | "cancelled" | "truncated")
        )
}

async fn poll_until_terminal(
    client: &reqwest::Client,
    base: &str,
    rollout_id: &str,
) -> Result<Value> {
    let deadline = Instant::now() + POLL_TIMEOUT;
    let outage_wait = crate::limits::OPTIMIZER_RUN_INDEX_WAIT;
    let mut event_endpoint_outage_started: Option<Instant> = None;
    loop {
        let response = client
            .get(format!("{base}/rollouts/{rollout_id}"))
            .send()
            .await
            .context("GET /rollouts/{id}");
        match response {
            Ok(response) if response.status().is_success() => {
                event_endpoint_outage_started = None;
                let state = response.json::<Value>().await?;
                if rollout_terminal(&state) {
                    return Ok(state);
                }
            }
            Ok(response)
                if OptimizerManager::observer_http_status_is_transient(
                    response.status().as_u16(),
                ) =>
            {
                let started = event_endpoint_outage_started.get_or_insert_with(Instant::now);
                if started.elapsed() >= outage_wait {
                    bail!(
                        "event_endpoint_outage: GET /rollouts/{rollout_id} stayed unavailable \
                         for {:.1}s (last status {})",
                        outage_wait.as_secs_f32(),
                        response.status()
                    );
                }
            }
            Ok(_) => {
                // Non-gateway, non-success: the rollout is not terminal yet
                // (or the container is still admitting). Keep polling until
                // POLL_TIMEOUT, as before.
            }
            Err(error) if super::manager::observer_error_is_transient_gateway(&error) => {
                let started = event_endpoint_outage_started.get_or_insert_with(Instant::now);
                if started.elapsed() >= outage_wait {
                    bail!(
                        "event_endpoint_outage: GET /rollouts/{rollout_id} stayed unavailable \
                         for {:.1}s (last error: {error})",
                        outage_wait.as_secs_f32()
                    );
                }
            }
            Err(error) => return Err(error),
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for terminal state of {rollout_id}");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn fetch_reward(
    client: &reqwest::Client,
    base: &str,
    rollout_id: &str,
    plan_ref: &str,
) -> Value {
    if let Ok(response) = client
        .get(format!("{base}/rollouts/{rollout_id}/reward"))
        .send()
        .await
    {
        if response.status().is_success() {
            if let Ok(body) = response.json::<Value>().await {
                if body.get("reward").and_then(Value::as_f64).is_some()
                    || body.get("status").and_then(Value::as_str) == Some("scored")
                {
                    return body;
                }
            }
        }
    }
    if let Ok(response) = client
        .post(format!("{base}/reward"))
        .json(&json!({
            "rollout_id": rollout_id,
            "mode": "terminal",
            "evaluation_plan_ref": plan_ref,
        }))
        .send()
        .await
    {
        if let Ok(body) = response.json::<Value>().await {
            return body;
        }
    }
    json!({ "status": "absent", "reward": null, "rollout_id": rollout_id })
}

async fn find_ready_container(
    service: &OptimizerService,
    family: &str,
    requested_id: Option<&str>,
) -> Result<ReadyContainer> {
    let family = family.to_string();
    let rows = service
        .database()
        .clone()
        .run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, status, base_url, task_family, metadata_json
                 FROM containers
                 ORDER BY updated_at DESC, id",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await?;
    let mut seen = Vec::new();
    let mut ready_containers = Vec::new();
    for (id, status, base_url, task_family, metadata_json) in rows {
        let metadata: Value = serde_json::from_str(&metadata_json).unwrap_or(json!({}));
        let matches = container_matches_family(task_family.as_deref(), &metadata, &family);
        let ready = matches!(
            status.to_ascii_lowercase().as_str(),
            "ready" | "healthy" | "running" | "live"
        );
        if matches {
            seen.push(format!("{id} ({status})"));
        }
        if matches && ready {
            if let Some(base_url) = base_url.filter(|value| !value.trim().is_empty()) {
                if let Some(protocol) = advertised_eval_protocol(&metadata) {
                    ready_containers.push(ReadyContainer {
                        id: id.clone(),
                        base_url: base_url.trim_end_matches('/').to_string(),
                        protocol,
                        image_digest: container_image_digest(&metadata),
                    });
                }
                seen.push(format!(
                    "{id} ({status}, protocol={})",
                    metadata
                        .pointer("/capabilities/protocol")
                        .and_then(Value::as_str)
                        .unwrap_or("null")
                ));
            }
        }
    }
    if let Some(requested_id) = requested_id {
        return ready_containers
            .into_iter()
            .find(|container| container.id == requested_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "requested {family} container `{requested_id}` is not a ready live-eval container: {}",
                    seen.join(", ")
                )
            });
    }
    if ready_containers.len() == 1 {
        return Ok(ready_containers.remove(0));
    }
    if ready_containers.len() > 1 {
        bail!(
            "ambiguous registered {family} live-eval containers: {}. Pass the explicit containerId selected in Data; refusing to substitute a container.",
            ready_containers
                .iter()
                .map(|container| container.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if seen.is_empty() {
        bail!(
            "no registered {family} container. Register a healthy {family} live-eval container before starting this baseline eval."
        );
    }
    bail!(
        "registered {family} containers are not ready live-eval containers: {}. Probe until status is ready/healthy and the container advertises {}.",
        seen.join(", "),
        crate::container_capabilities::LIVE_EVAL_PROTOCOL
    )
}

fn advertised_eval_protocol(metadata: &Value) -> Option<String> {
    if let Some(protocol) = metadata
        .pointer("/capabilities/protocol")
        .and_then(Value::as_str)
        .filter(|protocol| *protocol == crate::container_capabilities::LIVE_EVAL_PROTOCOL)
    {
        return Some(protocol.to_string());
    }
    for pointer in [
        "/optimizer_contracts/gepa/version",
        "/metadata/optimizer_contracts/gepa/version",
    ] {
        if let Some(version) = metadata.pointer(pointer).and_then(Value::as_str) {
            if version == crate::container_capabilities::GEPA_V2_CONTRACT {
                return Some(version.to_string());
            }
        }
    }
    None
}

fn container_image_digest(metadata: &Value) -> Option<String> {
    for pointer in ["/imageDigest", "/image_digest", "/digest", "/image/digest"] {
        if let Some(digest) = metadata.pointer(pointer).and_then(Value::as_str) {
            let digest = digest.trim();
            if digest.starts_with("sha256:") && digest.len() == 71 {
                return Some(digest.to_string());
            }
        }
    }
    None
}

fn container_matches_family(task_family: Option<&str>, metadata: &Value, family: &str) -> bool {
    let mut candidates = Vec::new();
    if let Some(value) = task_family {
        candidates.push(value.to_ascii_lowercase());
    }
    for key in ["runtime_family", "env_family", "task_family", "target_id"] {
        if let Some(value) = metadata.get(key).and_then(Value::as_str) {
            candidates.push(value.to_ascii_lowercase());
        }
    }
    candidates
        .iter()
        .any(|value| value == family || value.contains(family))
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
                // A lifecycle transition happens once. Keying it means a retry
                // after a transport failure re-offers the same event instead of
                // minting a second `started` at a new sequence.
                .idempotency_key(format!("eval:lifecycle:{event_type}"))
                .delta(Map::from_iter([("status".into(), json!(status))]))
                .raw(json!({ "source": "container_eval" }))],
        )
        .await?;
    Ok(())
}

async fn append_terminal(
    service: &OptimizerService,
    run_id: &str,
    failed: bool,
    detail: String,
) -> Result<()> {
    // Settled is settled *when there is a manifest*. Testing `run.status` alone
    // was how a run whose status had been rewritten without an event skipped its
    // own terminal event and left the log ending at `optimizer.run.started`.
    if service
        .terminal_manifest(run_id.to_string())
        .await?
        .is_some()
    {
        return Ok(());
    }
    let _ = service.seal_credential_chain(run_id).await;
    append_status(
        service,
        run_id,
        if failed {
            "optimizer.run.failed"
        } else {
            "optimizer.run.completed"
        },
        if failed { "failed" } else { "completed" },
    )
    .await?;
    if failed && !detail.is_empty() {
        service
            .append_event_payloads(
                run_id.to_string(),
                vec![
                    OptimizerEventDraft::new("optimizer.run.error", EVAL_ALGORITHM_ID)
                        .level("error")
                        .error(json!({ "message": detail }))
                        .raw(json!({ "source": "container_eval" })),
                ],
            )
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const CLASSIFY_EVAL: &str = "eval.banking77.baseline.v1";
    const HEALTH_EVAL: &str = "eval.healthbench.smoke.v1";
    use crate::ipc::{serve_json, JsonHttpRequest, JsonHttpResponse};
    use crate::optimizers::models::OptimizerRecipeRunRequest;
    use crate::optimizers::service::tests::service;
    use crate::visuals::VisualQuery;
    use hyper::StatusCode;
    use rusqlite::params;
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    #[derive(Clone)]
    struct MockEvalOptions {
        family: &'static str,
        rewards: BTreeMap<(String, i64), f64>,
        policy_status: u16,
        policy_config_id: String,
        fail_seeds: BTreeSet<i64>,
        extra_cost_usd: Option<f64>,
        policy_credential_present: bool,
        grader_credential_present: bool,
        /// False models the pinned Containers dev build (ac43172), which emits
        /// no `metadata.model_roles` at all.
        advertises_model_roles: bool,
    }

    async fn spawn_eval_mock(
        family: &'static str,
        rewards: BTreeMap<(String, i64), f64>,
    ) -> (String, tokio::task::JoinHandle<()>, Arc<AtomicUsize>) {
        spawn_eval_mock_opts(MockEvalOptions {
            family,
            rewards,
            policy_status: 200,
            policy_config_id: if family == "banking77" {
                "banking77_gpt_4_1_nano".into()
            } else {
                "openai_gpt41_mini".into()
            },
            fail_seeds: BTreeSet::new(),
            extra_cost_usd: None,
            policy_credential_present: true,
            grader_credential_present: true,
            advertises_model_roles: true,
        })
        .await
    }

    async fn spawn_eval_mock_opts(
        opts: MockEvalOptions,
    ) -> (String, tokio::task::JoinHandle<()>, Arc<AtomicUsize>) {
        if opts.family == "healthbench" {
            crate::secrets::install_test_live_openai();
        }
        let starts = Arc::new(AtomicUsize::new(0));
        let starts_h = starts.clone();
        let prepared = Arc::new(Mutex::new(BTreeMap::<String, bool>::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _ = serve_json(listener, move |request: JsonHttpRequest| {
                let starts = starts_h.clone();
                let prepared = prepared.clone();
                let opts = opts.clone();
                async move {
                    let path = request.path.split('?').next().unwrap_or(&request.path);
                    let family = opts.family;
                    match (request.method.as_str(), path) {
                        ("GET", "/info") => JsonHttpResponse::ok(json!({
                            "runtime_family": family,
                            "scale_leases": 10,
                            "world_ref": if family == "banking77" { "world:banking77@train" } else { "world:healthbench@eval" },
                            "capabilities": if family == "banking77" { json!({
                                "policy_refs": [{
                                    "harness": "desktop_eval",
                                    "config": "banking77_gpt_4_1_nano",
                                    "model": "openai/gpt-4.1-nano"
                                }]
                            }) } else { json!({}) },
                            "metadata": if family == "healthbench" && opts.advertises_model_roles { json!({
                                "model_roles": {
                                    "policy": {
                                        "api_key_env": "OPENAI_API_KEY",
                                        "credential_present": opts.policy_credential_present
                                    },
                                    "scorer": {
                                        "api_key_env": "OPENAI_API_KEY",
                                        "credential_present": opts.grader_credential_present
                                    }
                                }
                            }) } else { json!({}) },
                        })),
                        ("POST", path) if path == "/policy-configs" || path.starts_with("/policy-configs/") => {
                            if family == "healthbench" {
                                let url = request
                                    .body
                                    .pointer("/config/base_url")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                if url.contains("api.openai.com")
                                    || url.contains("127.0.0.1")
                                    || url.contains("localhost")
                                    || !url.contains("/cap/wcap_")
                                    || request.body.pointer("/config/api_key").is_some()
                                {
                                    return JsonHttpResponse::error(
                                        StatusCode::BAD_REQUEST,
                                        "healthbench must use the workshop proxy, not api.openai.com",
                                    );
                                }
                            }
                            if opts.policy_status == 200 {
                                JsonHttpResponse::ok(json!({"config_id": opts.policy_config_id}))
                            } else {
                                JsonHttpResponse::error(
                                    StatusCode::from_u16(opts.policy_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                                    "policy config refused",
                                )
                            }
                        }
                        ("POST", "/rollouts/prepare") => {
                            let rollout_id = request
                                .body
                                .get("rollout_id")
                                .and_then(Value::as_str)
                                .unwrap_or("roll_unknown")
                                .to_string();
                            prepared.lock().unwrap().insert(rollout_id.clone(), true);
                            JsonHttpResponse::ok(json!({
                                "rollout_id": rollout_id,
                                "stream": {
                                    "id": format!("stream:{rollout_id}"),
                                    "transports": { "poll": { "url": format!("/rollouts/{rollout_id}/events") } }
                                }
                            }))
                        }
                        ("GET", path) if path.ends_with("/events") => JsonHttpResponse::ok(json!({
                            "events": [{ "kind": "stream.subscribed", "ready": true }]
                        })),
                        ("POST", "/rollouts") => {
                            if family == "banking77"
                                && request.body.pointer("/policy_ref/harness") != Some(&json!("desktop_eval"))
                            {
                                return JsonHttpResponse::error(
                                    StatusCode::UNPROCESSABLE_ENTITY,
                                    "prepared Desktop evals must use the advertised policy_ref",
                                );
                            }
                            if family == "banking77"
                                && request.body.pointer("/policy_ref/config") != Some(&json!("banking77_gpt_4_1_nano"))
                            {
                                return JsonHttpResponse::error(
                                    StatusCode::UNPROCESSABLE_ENTITY,
                                    "prepared Desktop evals must use the advertised policy_ref",
                                );
                            }
                            starts.fetch_add(1, Ordering::SeqCst);
                            let rollout_id = request
                                .body
                                .get("rollout_id")
                                .and_then(Value::as_str)
                                .unwrap_or("roll_unknown")
                                .to_string();
                            let seed = request
                                .body
                                .get("task_instance_id")
                                .and_then(Value::as_str)
                                .and_then(|value| value.rsplit(':').next())
                                .and_then(|value| value.parse::<i64>().ok())
                                .unwrap_or(0);
                            if opts.fail_seeds.contains(&seed) {
                                return JsonHttpResponse::error(
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("forced failure for seed {seed}"),
                                );
                            }
                            let pool = if rollout_id.contains("heldout") {
                                "heldout"
                            } else {
                                "train"
                            };
                            let policy_cost = opts.extra_cost_usd.unwrap_or(0.001);
                            let grader_cost = if opts.extra_cost_usd.is_some() { 0.0 } else { 0.002 };
                            JsonHttpResponse::ok(json!({
                                "rollout_id": rollout_id,
                                "status": "completed",
                                "terminated": true,
                                "usage": if family == "healthbench" {
                                    json!({
                                        "policy": {"prompt_tokens": 11, "completion_tokens": 4, "cost_usd": policy_cost},
                                        "grader": {"prompt_tokens": 40, "completion_tokens": 12, "cost_usd": grader_cost}
                                    })
                                } else if let Some(cost) = opts.extra_cost_usd {
                                    json!({"prompt_tokens": 8, "completion_tokens": 1, "cost_usd": cost})
                                } else {
                                    json!({"prompt_tokens": 8, "completion_tokens": 1})
                                },
                                "trace": { "url": format!("/rollouts/{rollout_id}/trace"), "closed": true },
                                "seed": seed,
                                "pool": pool,
                            }))
                        }
                        ("GET", path) if path.starts_with("/rollouts/") && path.ends_with("/reward") => {
                            let rollout_id = path
                                .trim_start_matches("/rollouts/")
                                .trim_end_matches("/reward")
                                .to_string();
                            let seed = rollout_id
                                .split('_')
                                .rev()
                                .nth(1)
                                .and_then(|value| value.parse::<i64>().ok())
                                .unwrap_or(0);
                            let pool = if rollout_id.contains("heldout") {
                                "heldout".into()
                            } else {
                                "train".into()
                            };
                            let reward = opts.rewards.get(&(pool, seed)).copied().unwrap_or(1.0);
                            JsonHttpResponse::ok(json!({
                                "status": "scored",
                                "reward": reward,
                                "rollout_id": rollout_id
                            }))
                        }
                        ("GET", path) if path.starts_with("/rollouts/") => {
                            JsonHttpResponse::ok(json!({
                                "status": "completed",
                                "terminated": true
                            }))
                        }
                        _ => JsonHttpResponse::error(
                            StatusCode::NOT_FOUND,
                            format!("unexpected {} {path}", request.method),
                        ),
                    }
                }
            })
            .await;
        });
        (format!("http://{addr}"), task, starts)
    }

    async fn insert_container(
        service: &OptimizerService,
        family: &str,
        base_url: &str,
        status: &str,
    ) {
        insert_named_container(
            service,
            &format!("ctr_{family}_test"),
            family,
            base_url,
            status,
            "2026-08-17",
        )
        .await;
    }

    async fn insert_named_container(
        service: &OptimizerService,
        id: &str,
        family: &str,
        base_url: &str,
        status: &str,
        updated_at: &str,
    ) {
        let id = id.to_string();
        let family = family.to_string();
        let base_url = base_url.to_string();
        let status = status.to_string();
        let updated_at = updated_at.to_string();
        service
            .database()
            .clone()
            .run_transaction(move |conn| {
                conn.execute(
                    "INSERT INTO containers(id,name,location,status,base_url,task_family,health_json,metadata_json,created_at,updated_at)
                     VALUES(?1,?2,'local',?3,?4,?5,'{\"ok\":true}',?6,'2026-08-17',?7)",
                    params![
                        id,
                        format!("{family} test"),
                        status,
                        base_url,
                        family,
                        json!({
                            "runtime_family": family,
                            "capabilities": {
                                "protocol": crate::container_capabilities::GEPA_V2_CONTRACT,
                                "operations": {
                                    "rollouts.prepare": true,
                                    "rollouts.start": true,
                                    "rollouts.events": true
                                }
                            },
                            "imageDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        })
                        .to_string(),
                        updated_at
                    ],
                )?;
                Ok(())
            })
            .await
            .unwrap();
    }

    async fn wait_terminal(service: &OptimizerService, run_id: &str) -> OptimizerRunRecord {
        for _ in 0..200 {
            let run = service.get(run_id.to_string()).await.unwrap();
            if matches!(
                run.status.as_str(),
                "completed" | "failed" | "cancelled" | "degraded"
            ) {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("run {run_id} did not reach a terminal record");
    }

    /// Wait for the sealed manifest, which is the settlement the UI reads.
    /// A terminal `status` without one is exactly the half-settled state this
    /// lane exists to remove.
    async fn wait_manifest(service: &OptimizerService, run_id: &str) -> Value {
        for _ in 0..200 {
            if let Some(manifest) = service.terminal_manifest(run_id.to_string()).await.unwrap() {
                return manifest;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("run {run_id} never sealed a terminal manifest");
    }

    async fn declare_eval_recipes(svc: &OptimizerService, session: &str) {
        let workspace = tempfile::Builder::new()
            .prefix("ws-eval-")
            .tempdir()
            .unwrap()
            .into_path();
        std::fs::create_dir_all(workspace.join("workshop.recipes")).unwrap();
        std::fs::write(
            workspace.join("workshop.recipes/classify.toml"),
            r#"
id = "eval.banking77.baseline.v1"
algorithm = "eval"
title = "Classify baseline eval"
container = "classify"
provider = "openai"
model = "gpt-4.1-nano"
locality = "host"
family = "banking77"
harness = "desktop_eval"
policy_config = "banking77_gpt_4_1_nano"
concurrency = 10
train_seeds = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
[bounds]
max_cost_usd = 0.50
max_total_rollouts = 10
"#,
        )
        .unwrap();
        std::fs::write(
            workspace.join("workshop.recipes/healthbench.toml"),
            r#"
id = "eval.healthbench.smoke.v1"
algorithm = "eval"
title = "HealthBench smoke"
container = "healthbench"
provider = "openai"
model = "gpt-4.1-mini"
locality = "container"
family = "healthbench"
harness = "chat_completion"
policy_config = "openai_gpt41_mini"
concurrency = 2
train_seeds = [0, 1]
heldout_seeds = [100, 101]
requires_credential_advertisement = true
[bounds]
max_cost_usd = 0.50
max_total_rollouts = 4
"#,
        )
        .unwrap();
        let db = svc.database().clone();
        let session_id = session.to_string();
        let workspace_path = workspace.to_string_lossy().into_owned();
        db.run_transaction(move |conn| {
            conn.execute(
                "INSERT OR IGNORE INTO sessions(id,title,target_json,status,metadata_json,created_at,updated_at) VALUES(?1,?1,'{}','ready',?2,datetime('now'),datetime('now'))",
                rusqlite::params![session_id, serde_json::json!({"workspace": workspace_path}).to_string()],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        crate::workspace_scope::provision(
            svc.database(),
            session,
            workspace.to_str().unwrap(),
        )
        .await
        .unwrap();
    }

    async fn start_banking77(
        svc: &OptimizerService,
        session: &str,
    ) -> (OptimizerRunRecord, tokio::task::JoinHandle<()>) {
        let rewards = (0..10).map(|seed| (("train".into(), seed), 1.0)).collect();
        let (base, task, _) = spawn_eval_mock("banking77", rewards).await;
        insert_container(svc, "banking77", &base, "ready").await;
        declare_eval_recipes(svc, session).await;
        let (run, _) = svc
            .start_recipe(OptimizerRecipeRunRequest {
                recipe_id: CLASSIFY_EVAL.into(),
                session_ref: Some(session.into()),
                open_visual: Some(true),
                base_model: None,
                dataset_shard: None,
                candidate_set_id: None,
                container_id: None,
                training_artifact_id: None,
                plan_override: None,
                search: None,
            })
            .await
            .unwrap();
        (run, task)
    }

    /// The reproduction, as a regression test.
    ///
    /// A ten-rollout campaign that succeeds must leave a complete, contiguous,
    /// durable history — plan, ten queued, ten settlements, a selection, and a
    /// terminal event — not a lone `optimizer.run.started`.
    #[tokio::test]
    async fn a_ten_rollout_eval_retains_its_whole_event_plan() {
        let (svc, _dir, _) = service().await;
        let (run, task) = start_banking77(&svc, "sess_evidence").await;
        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "completed");
        assert_eq!(
            finished.summary["containerProtocol"],
            json!(crate::container_capabilities::GEPA_V2_CONTRACT)
        );
        assert_eq!(
            finished.summary["containerImageDigest"],
            json!("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );

        let events = svc
            .events_after(run.id.clone(), 0, Some(500))
            .await
            .unwrap();
        let sequences: Vec<u64> = events.iter().map(|event| event.sequence_number).collect();
        assert_eq!(
            sequences,
            (1..=sequences.len() as u64).collect::<Vec<_>>(),
            "the durable log must be contiguous from 1"
        );
        let count = |kind: &str| {
            events
                .iter()
                .filter(|event| event.event_type == kind)
                .count()
        };
        assert_eq!(count("optimizer.run.started"), 1);
        assert_eq!(count("eval.run.planned"), 1);
        assert_eq!(count("eval.trial.queued"), 10);
        assert_eq!(count("eval.trial.terminal"), 10);
        assert_eq!(count("eval.selection.completed"), 1);
        assert_eq!(count("optimizer.run.completed"), 1);
        assert_eq!(finished.cursor_seq, events.len() as u64);

        let manifest = wait_manifest(&svc, &run.id).await;
        assert_eq!(manifest["terminalStatus"], json!("completed"));
        assert_eq!(manifest["work"]["planned"], json!(10));
        assert_eq!(manifest["work"]["succeeded"], json!(10));
        assert_eq!(manifest["work"]["failed"], json!(0));
        assert_eq!(manifest["work"]["skipped"], json!(0));
        assert_eq!(manifest["terminalCursor"], json!(finished.cursor_seq));
        task.abort();
    }

    /// A fast campaign can finish before admission finishes patching the run.
    /// Writing that pre-start snapshot back must not erase the evidence the
    /// worker produced in between.
    #[tokio::test]
    async fn a_late_admission_writeback_cannot_erase_a_finished_campaign() {
        let (svc, _dir, _) = service().await;
        let (pre_start, task) = start_banking77(&svc, "sess_late_admission").await;
        let finished = wait_terminal(&svc, &pre_start.id).await;
        let cursor = finished.cursor_seq;

        // Admission's own writeback, arriving after the whole run.
        svc.attach_paid_compute_approval(pre_start.id.clone(), "approval-late", Some(1), Some(10))
            .await
            .unwrap();
        svc.persist_run(pre_start.clone()).await.unwrap();

        let after = svc.get(pre_start.id.clone()).await.unwrap();
        assert_eq!(after.cursor_seq, cursor, "cursor must not rewind");
        assert_eq!(after.status, "completed");
        assert!(
            after.summary.get("visualId").is_some(),
            "the published visual must survive a stale writeback"
        );
        assert_eq!(
            svc.events_after(pre_start.id, 0, Some(500))
                .await
                .unwrap()
                .len() as u64,
            cursor
        );
        task.abort();
    }

    /// Restart recovery: a new service over the same instance reads identical
    /// terminal evidence — counts, usage, selection, artifacts, and result.
    #[tokio::test]
    async fn restarting_returns_identical_terminal_evidence() {
        let (svc, dir, _) = service().await;
        let (run, task) = start_banking77(&svc, "sess_restart").await;
        wait_terminal(&svc, &run.id).await;
        let before_manifest = wait_manifest(&svc, &run.id).await;
        let before_result = svc.get_result(run.id.clone()).await.unwrap();
        task.abort();
        drop(svc);

        let restarted = super::super::service::tests::reopen(&dir).await;
        let after_manifest = restarted
            .terminal_manifest(run.id.clone())
            .await
            .unwrap()
            .expect("the manifest survives a restart");
        assert_eq!(after_manifest, before_manifest);
        let after_result = restarted.get_result(run.id.clone()).await.unwrap();
        assert_eq!(after_result["trials"], before_result["trials"]);
        assert_eq!(after_result["usage"], before_result["usage"]);
        assert_eq!(after_result["metrics"], before_result["metrics"]);
        assert_eq!(after_result["finalCursor"], before_result["finalCursor"]);
        let restored = restarted.get(run.id).await.unwrap();
        assert_eq!(
            restored.visual_refs.len(),
            2,
            "the chat-owned artifacts are still bound after a restart"
        );
    }

    #[tokio::test]
    async fn worker_failure_marks_the_published_experiment_visual_terminal() {
        let (svc, _dir, _) = service().await;
        let (run, task) = start_banking77(&svc, "sess_worker_failure_visual").await;
        let finished = wait_terminal(&svc, &run.id).await;
        let visual_id = finished.summary["visualId"].as_str().unwrap();
        let spec = EvalSpec::classify_fixture();

        project_worker_failure_visual(&svc, &run.id, &spec, visual_id, "vis_workbench", 10)
            .await
            .unwrap();

        let visual = svc.visuals().get(visual_id.to_string()).await.unwrap();
        assert_eq!(visual.status, VisualStatus::Failed);
        assert_eq!(
            visual.bindings.pointer("/inputs/0/data/status"),
            Some(&json!("failed"))
        );
        task.abort();
    }

    /// `get_result` for an eval is typed, and is answerable without a candidate.
    #[tokio::test]
    async fn a_finished_eval_returns_a_typed_eval_result() {
        let (svc, _dir, _) = service().await;
        let (run, task) = start_banking77(&svc, "sess_typed_result").await;
        wait_terminal(&svc, &run.id).await;
        wait_manifest(&svc, &run.id).await;
        let result = svc.get_result(run.id.clone()).await.unwrap();
        assert_eq!(result["resultKind"], json!("eval_run_result.v1"));
        assert_eq!(result["trials"]["succeeded"], json!(10));
        assert_eq!(result["metrics"]["meanReward"], json!(1.0));
        assert_eq!(
            result["metrics"]["selection"]["status"],
            json!("inconclusive")
        );
        assert!(
            result["evidenceRefs"]["visualId"].is_string(),
            "an eval result names the artifact that carries its evidence"
        );
        task.abort();
    }

    /// Publication is idempotent and chat-scoped: repeating it returns the same
    /// visual, and a second conversation cannot take the first one's pane.
    #[tokio::test]
    async fn publishing_the_chat_owned_visual_is_idempotent_and_scoped() {
        let (svc, _dir, _) = service().await;
        let (run, task) = start_banking77(&svc, "sess_owner").await;
        let finished = wait_terminal(&svc, &run.id).await;
        let visual_id = finished.summary["visualId"].as_str().unwrap().to_string();

        let spec = EvalSpec::classify_fixture();
        let again = mint_experiment_visual(&svc, &finished, &spec, 10)
            .await
            .unwrap();
        assert_eq!(
            again, visual_id,
            "republication must not mint a second visual"
        );
        let refreshed = svc.get(run.id.clone()).await.unwrap();
        // The overview and the trace workstation, one of each.
        assert_eq!(refreshed.visual_refs.len(), 2);
        assert_eq!(
            refreshed
                .visual_refs
                .iter()
                .filter(|reference| reference.role.as_deref() == Some("primary"))
                .count(),
            1
        );
        // `visualId` still names the overview: a run's primary pane is not
        // reassigned by publishing a drill-down beside it.
        assert_eq!(refreshed.summary["visualId"], json!(visual_id));
        assert_eq!(
            refreshed.summary["visualIds"]["trace_workbench"]
                .as_str()
                .is_some(),
            true,
            "the workstation is addressable by role"
        );
        assert_eq!(
            svc.visuals()
                .list(VisualQuery::default())
                .await
                .unwrap()
                .iter()
                .filter(|visual| visual.template_id == EXPERIMENT_TEMPLATE)
                .count(),
            1
        );

        // Ownership is the run's session, not whoever asks.
        assert_eq!(
            svc.visuals()
                .selected_for_session("sess_owner".into())
                .await
                .unwrap()
                .as_deref(),
            Some(visual_id.as_str())
        );
        assert_eq!(
            svc.visuals()
                .selected_for_session("sess_other".into())
                .await
                .unwrap(),
            None,
            "another chat must not have had this run's pane pushed into it"
        );
        task.abort();
    }
    /// A visual projection that fails at settlement makes the run degraded, not
    /// completed — and the rollouts it already paid for stay on the record.
    #[tokio::test]
    async fn a_failed_visual_projection_settles_degraded_not_completed() {
        let (svc, _dir, _) = service().await;
        let spec = EvalSpec::classify_fixture();
        let (run, _) = svc
            .create(crate::optimizers::models::OptimizerCreateRequest {
                algorithm_id: EVAL_ALGORITHM_ID.into(),
                algorithm_version: Some("1".into()),
                objective: Some(spec.title.clone()),
                source: Some("local".into()),
                project_ref: None,
                session_ref: Some("sess_degraded".into()),
                id: Some("opt_eval_banking77_degraded".into()),
                execution_bindings: None,
                input_refs: None,
                capabilities: Some(OptimizerCapabilities::for_algorithm(EVAL_ALGORITHM_ID)),
                summary: Some(json!({ "recipeId": spec.recipe_id })),
                open_visual: Some(false),
                seed_fixture: None,
                cloud_config: None,
                local_path: None,
            })
            .await
            .unwrap();
        append_status(&svc, &run.id, "optimizer.run.started", "running")
            .await
            .unwrap();
        let records =
            vec![json!({ "seed": 0, "pool": "train", "reward": 1.0, "status": "completed" })];

        // The visual the campaign was supposed to keep current is gone.
        let failure = evidence(
            "progress_projection",
            persist_progress(&svc, &run.id, &spec, "vis_missing", "vis_workbench", &records, 1, "completed"),
        )
        .await
        .unwrap_err();
        settle_worker_failure(&svc, &run.id, failure).await;

        let settled = svc.get(run.id.clone()).await.unwrap();
        assert_eq!(settled.status, "degraded");
        assert_eq!(
            settled.summary["records"].as_array().map(Vec::len),
            Some(1),
            "the successful rollout stays on the record"
        );
        let manifest = svc.terminal_manifest(run.id).await.unwrap().unwrap();
        assert_eq!(manifest["terminalStatus"], json!("failed_evidence"));
        assert_eq!(manifest["degradation"]["retryable"], json!(true));
    }
    #[tokio::test]
    async fn healthbench_admission_blocks_before_dispatch_when_policy_credential_is_missing() {
        let (base, task, starts) = spawn_eval_mock_opts(MockEvalOptions {
            family: "healthbench",
            rewards: BTreeMap::new(),
            policy_status: 200,
            policy_config_id: "openai_gpt41_mini".into(),
            fail_seeds: BTreeSet::new(),
            extra_cost_usd: None,
            policy_credential_present: false,
            grader_credential_present: true,
            advertises_model_roles: true,
        })
        .await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "healthbench", &base, "ready").await;
        let error = svc
            .start_recipe(recipe(&svc, HEALTH_EVAL, "sess_missing_policy").await)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("credential_missing"), "{error}");
        assert!(error.contains("healthbench.policy"), "{error}");
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        task.abort();
    }

    /// The integration gap between this lane and the pinned runtime.
    ///
    /// Workshop's preflight reads `metadata.model_roles`, which Containers only
    /// began advertising in `e141545` — a commit that is not on `origin/dev`,
    /// so the pinned build (`ac43172`, 0.4.1.dev20260817) emits nothing. A
    /// preflight that treats silence as readiness therefore passes on exactly
    /// the build in use, and the run settles 4/4 terminal traces with every
    /// rollout failing `openai_api_key_missing`. Silence is refused instead.
    #[tokio::test]
    async fn healthbench_admission_refuses_a_container_that_cannot_report_credential_readiness() {
        let (base, task, starts) = spawn_eval_mock_opts(MockEvalOptions {
            family: "healthbench",
            rewards: BTreeMap::new(),
            policy_status: 200,
            policy_config_id: "openai_gpt41_mini".into(),
            fail_seeds: BTreeSet::new(),
            extra_cost_usd: None,
            policy_credential_present: true,
            grader_credential_present: true,
            advertises_model_roles: false,
        })
        .await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "healthbench", &base, "ready").await;
        let error = svc
            .start_recipe(recipe(&svc, HEALTH_EVAL, "sess_no_roles").await)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("credential_readiness_unavailable"),
            "{error}"
        );
        assert!(error.contains("model_roles"), "{error}");
        assert_eq!(
            starts.load(Ordering::SeqCst),
            0,
            "not one rollout may be dispatched"
        );
        task.abort();
    }

    /// Banking77 needs no advertised credential lane: its policy is served by
    /// the container against a pinned, advertised config. The refusal above
    /// must not spread to it.
    #[tokio::test]
    async fn banking77_admission_does_not_require_advertised_credential_lanes() {
        let mut rewards = BTreeMap::new();
        for seed in 0..10 {
            rewards.insert(("train".to_string(), seed), 1.0);
        }
        let (base, task, _) = spawn_eval_mock_opts(MockEvalOptions {
            family: "banking77",
            rewards,
            policy_status: 200,
            policy_config_id: "banking77_gpt_4_1_nano".into(),
            fail_seeds: BTreeSet::new(),
            extra_cost_usd: None,
            policy_credential_present: true,
            grader_credential_present: true,
            advertises_model_roles: false,
        })
        .await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "banking77", &base, "ready").await;
        let started = svc
            .start_recipe(recipe(&svc, CLASSIFY_EVAL, "sess_b77_no_roles").await)
            .await;
        assert!(
            started.is_ok(),
            "banking77 must still admit: {:?}",
            started.err()
        );
        task.abort();
    }

    #[tokio::test]
    async fn healthbench_admission_blocks_before_dispatch_when_grader_credential_is_missing() {
        let (base, task, starts) = spawn_eval_mock_opts(MockEvalOptions {
            family: "healthbench",
            rewards: BTreeMap::new(),
            policy_status: 200,
            policy_config_id: "openai_gpt41_mini".into(),
            fail_seeds: BTreeSet::new(),
            extra_cost_usd: None,
            policy_credential_present: true,
            grader_credential_present: false,
            advertises_model_roles: true,
        })
        .await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "healthbench", &base, "ready").await;
        let error = svc
            .start_recipe(recipe(&svc, HEALTH_EVAL, "sess_missing_grader").await)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("credential_missing"), "{error}");
        assert!(error.contains("healthbench.grader"), "{error}");
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        task.abort();
    }
    #[tokio::test]
    async fn container_eval_refuses_a_ready_pool_that_does_not_advertise_gepa_v2() {
        let (svc, _dir, _) = service().await;
        let (base, task, starts) = spawn_eval_mock("healthbench", BTreeMap::new()).await;
        let family = "healthbench".to_string();
        let base_url = base.clone();
        svc.database()
            .clone()
            .run_transaction(move |conn| {
                conn.execute(
                    "INSERT INTO containers(id,name,location,status,base_url,task_family,health_json,metadata_json,created_at,updated_at)
                     VALUES('ctr_healthbench_null_protocol','healthbench null','local','ready',?1,?2,'{\"ok\":true}',?3,'2026-08-17','2026-08-17')",
                    params![
                        base_url,
                        family,
                        json!({
                            "runtime_family": "healthbench",
                            "capabilities": {
                                "protocol": Value::Null,
                                "operations": "unknown"
                            }
                        })
                        .to_string()
                    ],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let error = svc
            .start_recipe(recipe(&svc, HEALTH_EVAL, "sess_null_protocol").await)
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap();
        assert!(
            error.contains("GEPA v2")
                && error.contains(crate::container_capabilities::GEPA_V2_CONTRACT),
            "expected a GEPA v2 admission refusal, got {error}"
        );
        assert!(
            error.contains("protocol=null"),
            "expected the advertised protocol to be named, got {error}"
        );
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        task.abort();
    }

    async fn recipe(
        svc: &OptimizerService,
        id: &str,
        session: &str,
    ) -> OptimizerRecipeRunRequest {
        recipe_on(svc, id, session, None).await
    }

    async fn recipe_on(
        svc: &OptimizerService,
        id: &str,
        session: &str,
        container_id: Option<&str>,
    ) -> OptimizerRecipeRunRequest {
        declare_eval_recipes(svc, session).await;
        OptimizerRecipeRunRequest {
            recipe_id: id.into(),
            session_ref: Some(session.into()),
            open_visual: Some(true),
            base_model: None,
            dataset_shard: None,
            candidate_set_id: None,
            container_id: container_id.map(str::to_string),
            training_artifact_id: None,
            plan_override: None,
            search: None,
        }
    }

    #[tokio::test]
    async fn two_ready_banking77_pools_without_container_id_fail_ambiguous_before_dispatch() {
        let (isolated_base, isolated_task, isolated_starts) =
            spawn_eval_mock("banking77", BTreeMap::new()).await;
        let (stale_base, stale_task, stale_starts) =
            spawn_eval_mock("banking77", BTreeMap::new()).await;
        let (svc, _dir, _) = service().await;
        insert_named_container(
            &svc,
            "ctr_banking77_isolated",
            "banking77",
            &isolated_base,
            "ready",
            "2026-08-17T00:00:00Z",
        )
        .await;
        insert_named_container(
            &svc,
            "ctr_banking77_stale",
            "banking77",
            &stale_base,
            "ready",
            "2026-08-19T00:00:00Z",
        )
        .await;
        let error = svc
            .start_recipe(recipe(&svc, CLASSIFY_EVAL, "sess_ambiguous").await)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("ambiguous registered banking77 GEPA v2 pools"),
            "expected an ambiguous-pool refusal, got {error}"
        );
        assert!(
            error.contains("ctr_banking77_isolated") && error.contains("ctr_banking77_stale"),
            "expected both ready ids in the refusal, got {error}"
        );
        assert_eq!(isolated_starts.load(Ordering::SeqCst), 0);
        assert_eq!(stale_starts.load(Ordering::SeqCst), 0);
        isolated_task.abort();
        stale_task.abort();
    }

    #[tokio::test]
    async fn explicit_container_id_binds_the_isolated_ready_banking77_pool() {
        let rewards = (0..10).map(|seed| (("train".into(), seed), 1.0)).collect();
        let (isolated_base, isolated_task, isolated_starts) =
            spawn_eval_mock("banking77", rewards).await;
        let (stale_base, stale_task, stale_starts) =
            spawn_eval_mock("banking77", BTreeMap::new()).await;
        let (svc, _dir, _) = service().await;
        insert_named_container(
            &svc,
            "ctr_banking77_isolated",
            "banking77",
            &isolated_base,
            "ready",
            "2026-08-17T00:00:00Z",
        )
        .await;
        insert_named_container(
            &svc,
            "ctr_banking77_stale",
            "banking77",
            &stale_base,
            "ready",
            "2026-08-19T00:00:00Z",
        )
        .await;
        let (run, _) = svc
            .start_recipe(recipe_on(
                &svc,
                CLASSIFY_EVAL,
                "sess_explicit_isolated",
                Some("ctr_banking77_isolated"),
            ).await)
            .await
            .unwrap();
        assert_eq!(run.summary["containerId"], json!("ctr_banking77_isolated"));
        assert_eq!(
            run.summary["containerBaseUrl"],
            json!(isolated_base.trim_end_matches('/'))
        );
        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "completed");
        assert_eq!(isolated_starts.load(Ordering::SeqCst), 10);
        assert_eq!(stale_starts.load(Ordering::SeqCst), 0);
        isolated_task.abort();
        stale_task.abort();
    }

    #[tokio::test]
    async fn requested_container_id_that_is_not_ready_or_wrong_family_fails_closed() {
        let (ready_base, ready_task, ready_starts) =
            spawn_eval_mock("banking77", BTreeMap::new()).await;
        let (health_base, health_task, health_starts) =
            spawn_eval_mock("healthbench", BTreeMap::new()).await;
        let (svc, _dir, _) = service().await;
        insert_named_container(
            &svc,
            "ctr_banking77_ready",
            "banking77",
            &ready_base,
            "ready",
            "2026-08-17T00:00:00Z",
        )
        .await;
        insert_named_container(
            &svc,
            "ctr_banking77_offline",
            "banking77",
            &ready_base,
            "offline",
            "2026-08-18T00:00:00Z",
        )
        .await;
        insert_named_container(
            &svc,
            "ctr_healthbench_ready",
            "healthbench",
            &health_base,
            "ready",
            "2026-08-19T00:00:00Z",
        )
        .await;

        let not_ready = svc
            .start_recipe(recipe_on(
                &svc,
                CLASSIFY_EVAL,
                "sess_not_ready",
                Some("ctr_banking77_offline"),
            ).await)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            not_ready.contains(
                "requested banking77 container `ctr_banking77_offline` is not a ready GEPA v2 pool"
            ),
            "expected a fail-closed missing-ready-id error, got {not_ready}"
        );
        assert!(
            !not_ready.contains("ambiguous"),
            "must not fall back to the other ready pool, got {not_ready}"
        );

        let wrong_family = svc
            .start_recipe(recipe_on(
                &svc,
                CLASSIFY_EVAL,
                "sess_wrong_family",
                Some("ctr_healthbench_ready"),
            ).await)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            wrong_family.contains(
                "requested banking77 container `ctr_healthbench_ready` is not a ready GEPA v2 pool"
            ),
            "expected a fail-closed wrong-family error, got {wrong_family}"
        );
        assert!(
            !wrong_family.contains("ambiguous"),
            "must not fall back to the banking77 pool, got {wrong_family}"
        );
        assert_eq!(ready_starts.load(Ordering::SeqCst), 0);
        assert_eq!(health_starts.load(Ordering::SeqCst), 0);
        ready_task.abort();
        health_task.abort();
    }

    #[tokio::test]
    async fn single_ready_banking77_pool_still_starts_when_container_id_is_omitted() {
        let rewards = (0..10).map(|seed| (("train".into(), seed), 1.0)).collect();
        let (base, task, starts) = spawn_eval_mock("banking77", rewards).await;
        let (svc, _dir, _) = service().await;
        insert_named_container(
            &svc,
            "ctr_banking77_only",
            "banking77",
            &base,
            "ready",
            "2026-08-17T00:00:00Z",
        )
        .await;
        let (run, _) = svc
            .start_recipe(recipe(&svc, CLASSIFY_EVAL, "sess_single_pool").await)
            .await
            .unwrap();
        assert_eq!(run.summary["containerId"], json!("ctr_banking77_only"));
        assert_eq!(
            run.summary["containerBaseUrl"],
            json!(base.trim_end_matches('/'))
        );
        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "completed");
        assert_eq!(starts.load(Ordering::SeqCst), 10);
        task.abort();
    }

    #[tokio::test]
    async fn newer_updated_at_does_not_break_ambiguous_refusal_when_container_id_is_omitted() {
        let (older_base, older_task, older_starts) =
            spawn_eval_mock("banking77", BTreeMap::new()).await;
        let (newer_base, newer_task, newer_starts) =
            spawn_eval_mock("banking77", BTreeMap::new()).await;
        let (svc, _dir, _) = service().await;
        insert_named_container(
            &svc,
            "ctr_banking77_older",
            "banking77",
            &older_base,
            "ready",
            "2026-08-10T00:00:00Z",
        )
        .await;
        insert_named_container(
            &svc,
            "ctr_banking77_stale_newer",
            "banking77",
            &newer_base,
            "ready",
            "2026-08-20T12:00:00Z",
        )
        .await;
        let error = svc
            .start_recipe(recipe(&svc, CLASSIFY_EVAL, "sess_stale_newer").await)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("ambiguous registered banking77 GEPA v2 pools"),
            "a newer updated_at must not restore first-match-wins, got {error}"
        );
        assert!(
            error.contains("ctr_banking77_older") && error.contains("ctr_banking77_stale_newer"),
            "expected both ready ids in the refusal, got {error}"
        );
        assert_eq!(older_starts.load(Ordering::SeqCst), 0);
        assert_eq!(newer_starts.load(Ordering::SeqCst), 0);
        older_task.abort();
        newer_task.abort();
    }

    #[tokio::test]
    async fn healthbench_fails_closed_when_policy_config_registration_is_rejected() {
        let (base, task, starts) = spawn_eval_mock_opts(MockEvalOptions {
            family: "healthbench",
            rewards: BTreeMap::new(),
            policy_status: 503,
            policy_config_id: "openai_gpt41_mini".into(),
            fail_seeds: BTreeSet::new(),
            extra_cost_usd: None,
            policy_credential_present: true,
            grader_credential_present: true,
            advertises_model_roles: true,
        })
        .await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "healthbench", &base, "ready").await;
        let (run, _) = svc
            .start_recipe(recipe(&svc, HEALTH_EVAL, "sess_policy_reject").await)
            .await
            .unwrap();
        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "failed");
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        task.abort();
    }

    #[tokio::test]
    async fn healthbench_fails_closed_when_policy_config_identity_mismatches() {
        let (base, task, starts) = spawn_eval_mock_opts(MockEvalOptions {
            family: "healthbench",
            rewards: BTreeMap::new(),
            policy_status: 200,
            policy_config_id: "groq_llama31_8b".into(),
            fail_seeds: BTreeSet::new(),
            extra_cost_usd: None,
            policy_credential_present: true,
            grader_credential_present: true,
            advertises_model_roles: true,
        })
        .await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "healthbench", &base, "ready").await;
        let (run, _) = svc
            .start_recipe(recipe(
                &svc,
                HEALTH_EVAL,
                "sess_policy_mismatch",
            ).await)
            .await
            .unwrap();
        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "failed");
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        task.abort();
    }

    #[tokio::test]
    async fn banking77_partial_failure_is_failed_not_completed() {
        let rewards = (0..10).map(|seed| (("train".into(), seed), 1.0)).collect();
        let (base, task, starts) = spawn_eval_mock_opts(MockEvalOptions {
            family: "banking77",
            rewards,
            policy_status: 200,
            policy_config_id: "banking77_gpt_4_1_nano".into(),
            fail_seeds: (1..10).collect(),
            extra_cost_usd: None,
            policy_credential_present: true,
            grader_credential_present: true,
            advertises_model_roles: true,
        })
        .await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "banking77", &base, "ready").await;
        let (run, _) = svc
            .start_recipe(recipe(&svc, CLASSIFY_EVAL, "sess_partial").await)
            .await
            .unwrap();
        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "failed");
        assert_eq!(starts.load(Ordering::SeqCst), 10);
        assert_eq!(finished.summary["progress"]["failed"], json!(9));
        assert_ne!(finished.summary["evalStatus"], json!("completed"));
        task.abort();
    }

    #[tokio::test]
    async fn healthbench_stops_dispatch_once_cost_ceiling_is_reached() {
        let mut rewards = BTreeMap::new();
        rewards.insert(("train".into(), 0), 0.8);
        rewards.insert(("train".into(), 1), 0.6);
        rewards.insert(("heldout".into(), 100), 0.5);
        rewards.insert(("heldout".into(), 101), 0.4);
        let (base, task, starts) = spawn_eval_mock_opts(MockEvalOptions {
            family: "healthbench",
            rewards,
            policy_status: 200,
            policy_config_id: "openai_gpt41_mini".into(),
            fail_seeds: BTreeSet::new(),
            extra_cost_usd: Some(0.50),
            policy_credential_present: true,
            grader_credential_present: true,
            advertises_model_roles: true,
        })
        .await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "healthbench", &base, "ready").await;
        let (run, _) = svc
            .start_recipe(recipe(&svc, HEALTH_EVAL, "sess_budget").await)
            .await
            .unwrap();
        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "failed");
        assert!(starts.load(Ordering::SeqCst) <= 2);
        assert_eq!(
            finished
                .summary
                .get("records")
                .and_then(Value::as_array)
                .map(|rows| rows.len()),
            Some(4)
        );
        task.abort();
    }

    #[test]
    fn healthbench_policy_config_requires_workshop_proxy_base() {
        let spec = EvalSpec::healthbench_fixture();
        assert!(spec.policy_config_body(None).is_none());
        let body = spec
            .policy_config_body(Some(
                "http://host.docker.internal:9/cap/wcap_abc12345/v1/providers/openai",
            ))
            .unwrap();
        let dump = body.to_string();
        assert_eq!(
            body["config"]["base_url"],
            "http://host.docker.internal:9/cap/wcap_abc12345/v1/providers/openai"
        );
        assert!(dump.contains("/cap/wcap_abc12345/"));
        assert!(!dump.contains("api.openai.com"));
        assert!(!dump.contains("127.0.0.1"));
        assert!(body.pointer("/config/api_key").is_none());
    }

    #[test]
    fn banking77_policy_config_stays_on_workshop_proxy() {
        let spec = EvalSpec::classify_fixture();
        assert!(spec.policy_config_body(None).is_none());
        let proxy = "http://host.docker.internal:9/cap/wcap_banking77/v1/providers/openrouter";
        let body = spec.policy_config_body(Some(proxy)).unwrap();
        assert_eq!(body["config"]["base_url"], proxy);
        assert_eq!(body["config"]["api_key_env"], "OPENAI_API_KEY");
        assert!(!body.to_string().contains("openrouter.ai"));
    }

    #[test]
    fn retained_rollout_state_uses_the_latest_observation() {
        let page = json!({
            "events": [
                {"kind": "observation", "payload": {"readout": {
                    "observation_text": "opening", "achievements": []
                }}},
                {"kind": "observation", "payload": {"readout": {
                    "observation_text": "final map", "achievements": ["collect_wood"]
                }}}
            ]
        });
        let retained = retained_rollout_state(&page);
        assert_eq!(retained["observation"], json!("final map"));
        assert_eq!(retained["achievements"], json!(["collect_wood"]));
    }

    // ── Craftax relay fixture ────────────────────────────────────────────
    //
    // The regression this fixture pins is the inspected rollout
    // `roll_craftax_train_780003_4e27ac3b`: 10 policy calls, 13 logical native
    // frames, 12 applied actions, 2 achievements, total reward +2.00. Workshop
    // reported it as "0 native frames" because it never read the journal while
    // the rollout was open and never asked for frames in the first place.
    //
    // The important property of the mock is that its blocking `POST /rollouts`
    // *stays open* while the journal fills. A relay that only worked against a
    // container which had already finished would pass a test and change nothing.

    const CRAFTAX_EVAL: &str = "eval.craftax.baseline.v1";
    const CRAFTAX_POLICY_CALLS: usize = 10;
    const CRAFTAX_FRAMES: usize = 13;
    const CRAFTAX_APPLIED_ACTIONS: usize = 12;
    const CRAFTAX_ACHIEVEMENTS: usize = 2;
    const CRAFTAX_TOTAL_REWARD: f64 = 2.0;

    /// A deterministic 8×8 PNG whose pixels depend on `tint`.
    ///
    /// Two frames with the same tint are byte-identical, which is what makes
    /// the deduplication assertion meaningful: the content store must collapse
    /// them physically while the timeline keeps both as separate steps.
    fn fixture_png(tint: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 8, 8);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&vec![tint; 8 * 8 * 4]).unwrap();
        }
        bytes
    }

    #[derive(Clone, Copy, Default)]
    struct CraftaxMockOptions {
        /// Every frame is the same image. Logical frame count must not change.
        identical_frames: bool,
        /// Serve a body that is a PNG header followed by garbage.
        corrupt_frame_step: Option<i64>,
        /// Serve a 302 to somewhere else instead of the frame.
        redirect_frame_step: Option<i64>,
        /// Serve a frame far over `media.max_frame_bytes`.
        oversize_frame_step: Option<i64>,
        /// Skip this producer sequence, so the journal has a real gap.
        skip_sequence: Option<u64>,
        /// Hold the blocking POST open this long after the journal closes.
        trailing_hold_ms: u64,
    }

    /// The producer envelope shape, exactly as `RolloutEventLog` writes it.
    fn journal_event(sequence: u64, kind: &str, payload: Value) -> Value {
        json!({
            "schema": "synth.rollout.stream-event.v1",
            "kind": kind,
            "ts": format!("2026-08-26T00:00:{:02}.000000Z", sequence % 60),
            "control": false,
            "sequence": sequence,
            "event_id": sequence.to_string(),
            // 16 hex characters, like the real producer. Deliberately not a
            // SHA-256: nothing downstream is allowed to treat it as one.
            "digest": format!("{:016x}", sequence.wrapping_mul(0x9e37_79b9_7f4a_7c15)),
            "payload": payload,
        })
    }

    /// The whole Craftax episode as a producer journal.
    fn craftax_journal(opts: CraftaxMockOptions) -> Vec<Value> {
        let mut events = Vec::new();
        let sequence = std::cell::Cell::new(0u64);
        let push = |events: &mut Vec<Value>, kind: &str, payload: Value| {
            sequence.set(sequence.get() + 1);
            events.push(journal_event(sequence.get(), kind, payload));
        };
        let emit_step = |events: &mut Vec<Value>, push: &dyn Fn(&mut Vec<Value>, &str, Value), step: i64| {
            push(
                events,
                "observation",
                json!({
                    "step": step,
                    "seed": 780_003,
                    "grid": "....\n.@..\n....",
                    "readout": {
                        "observation_text": format!("step {step}\nlocal_map:\n....\n.@..\n....\n\ninventory: wood 1"),
                        "local_map": ["....", ".@..", "...."],
                        "achievements": [],
                    }
                }),
            );
            push(
                events,
                "frame",
                json!({
                    "step": step,
                    "format": "png",
                    "digest": format!("{:016x}", step),
                    "url": format!("__ROLLOUT__/frames/{step}.png"),
                }),
            );
            push(
                events,
                "resource_delta",
                json!({ "step": step, "resource": "wood", "before": step, "after": step + 1 }),
            );
        };
        push(&mut events, "env.episode.opened", json!({"seed": 780_003, "max_steps": 64}));
        emit_step(&mut events, &push, 0);
        push(&mut events, "policy.session.opened", json!({"harness": "craftax_react"}));
        let mut applied = 0;
        for call in 0..CRAFTAX_POLICY_CALLS {
            push(&mut events, "span.policy.opened", json!({"call": call}));
            push(
                &mut events,
                "span.policy.data",
                json!({
                    "call": call,
                    "messages": [
                        {"role": "system", "content": "You are playing Craftax."},
                        {"role": "user", "content": format!("observation for call {call}")}
                    ],
                    "assistant": {
                        "reasoning_content": format!("thinking about call {call}"),
                        "content": format!("I will move for call {call}"),
                        "tool_calls": [{
                            "id": format!("call_{call}"),
                            "function": {
                                "name": "craftax_act",
                                "arguments": json!({"actions": ["move_right"]}).to_string()
                            }
                        }]
                    },
                    "usage": {"prompt_tokens": 100 + call, "completion_tokens": 20 + call}
                }),
            );
            push(&mut events, "span.policy.plan", json!({"actions": ["move_right"], "length": 1}));
            push(&mut events, "span.policy.closed", json!({"length": 1}));
            // Two of the ten calls propose an action the environment refuses.
            // The viewer must keep showing the call and must never report the
            // proposal as applied.
            if call == 3 || call == 7 {
                push(
                    &mut events,
                    "action_rejected",
                    json!({"step": applied, "action": "move_right", "reason": "blocked"}),
                );
                continue;
            }
            // A committed batch can apply more than one action, so call count
            // and step count are deliberately different numbers here: a viewer
            // that assumes one call is one frame gets this trace wrong.
            let batch = if call < 5 { 2 } else { 1 };
            for _ in 0..batch {
                let step = applied + 1;
                push(&mut events, "action_applied", json!({"step": step, "action": "move_right"}));
                push(
                    &mut events,
                    "reward_signal",
                    json!({"step": step, "value": 0.0, "authority": "environment"}),
                );
                if step == 3 || step == 9 {
                    push(
                        &mut events,
                        "achievement_unlocked",
                        json!({
                            "step": step,
                            "achievement": if step == 3 { "collect_wood" } else { "place_table" }
                        }),
                    );
                    push(&mut events, "reward_delta", json!({"step": step, "delta": 1.0}));
                }
                push(
                    &mut events,
                    "state_transition",
                    json!({"step": step, "field": "health", "before": 9, "after": 9}),
                );
                emit_step(&mut events, &push, step);
                applied += 1;
            }
        }
        assert_eq!(applied as usize, CRAFTAX_APPLIED_ACTIONS);
        push(&mut events, "policy.session.closed", json!({"calls": CRAFTAX_POLICY_CALLS}));
        push(&mut events, "env.episode.closed", json!({"status": "completed", "steps": applied}));
        push(&mut events, "status", json!({"status": "completed", "steps": applied}));
        push(&mut events, "capture.closed", json!({"high_water": sequence.get()}));
        if let Some(gap) = opts.skip_sequence {
            events.retain(|event| event.get("sequence").and_then(Value::as_u64) != Some(gap));
        }
        events
    }

    struct CraftaxMock {
        base: String,
        task: tokio::task::JoinHandle<()>,
    }

    /// A Craftax-shaped container whose blocking rollout stays open while its
    /// journal becomes pollable page by page.
    async fn spawn_craftax_mock(opts: CraftaxMockOptions) -> CraftaxMock {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let journals: Arc<Mutex<BTreeMap<String, (Vec<Value>, bool)>>> =
            Arc::new(Mutex::new(BTreeMap::new()));
        let script = Arc::new(craftax_journal(opts));
        let task = tokio::spawn(async move {
            let _ = serve_json(listener, move |request: JsonHttpRequest| {
                let journals = journals.clone();
                let script = script.clone();
                async move {
                    let path = request.path.split('?').next().unwrap_or(&request.path).to_string();
                    let query = request.path.split_once('?').map(|(_, q)| q.to_string()).unwrap_or_default();
                    match (request.method.as_str(), path.as_str()) {
                        ("GET", "/info") => JsonHttpResponse::ok(json!({
                            "runtime_family": "craftax",
                            "scale_leases": 4,
                            "world_ref": "world:craftax@eval",
                            "capabilities": {
                                "policy_refs": [{
                                    "harness": "craftax_react",
                                    "config": "craftax_luna_low",
                                    "model": "openai/gpt-5.6-luna"
                                }]
                            }
                        })),
                        ("POST", "/rollouts/prepare") => {
                            let rollout_id = request.body.get("rollout_id").and_then(Value::as_str)
                                .unwrap_or("roll_unknown").to_string();
                            journals.lock().unwrap().insert(rollout_id.clone(), (Vec::new(), false));
                            JsonHttpResponse::ok(json!({
                                "rollout_id": rollout_id,
                                "stream": {
                                    "id": format!("stream:{rollout_id}"),
                                    "transports": {"poll": {"url": format!("/rollouts/{rollout_id}/events")}}
                                }
                            }))
                        }
                        ("GET", path) if path.ends_with("/events") => {
                            let rollout_id = path.trim_start_matches("/rollouts/")
                                .trim_end_matches("/events").to_string();
                            let after = query.split('&')
                                .find_map(|pair| pair.strip_prefix("after="))
                                .and_then(|value| value.parse::<u64>().ok())
                                .unwrap_or(0);
                            let limit = query.split('&')
                                .find_map(|pair| pair.strip_prefix("limit="))
                                .and_then(|value| value.parse::<usize>().ok())
                                .unwrap_or(1000);
                            let guard = journals.lock().unwrap();
                            let Some((events, closed)) = guard.get(&rollout_id) else {
                                return JsonHttpResponse::error(StatusCode::NOT_FOUND, "unknown_rollout");
                            };
                            let available: Vec<Value> = events.iter()
                                .filter(|event| event.get("sequence").and_then(Value::as_u64)
                                    .is_some_and(|sequence| sequence > after))
                                .cloned()
                                .collect();
                            let page: Vec<Value> = available.iter().take(limit).cloned().collect();
                            let high_water = events.last()
                                .and_then(|event| event.get("sequence").and_then(Value::as_u64))
                                .unwrap_or(0);
                            let next = page.last()
                                .and_then(|event| event.get("sequence").and_then(Value::as_u64))
                                .unwrap_or(after);
                            let mut rows = vec![json!({
                                "schema": "synth.rollout.stream-event.v1",
                                "kind": "stream.subscribed",
                                "control": true,
                                "event_id": "stream.subscribed",
                                "ts": "2026-08-26T00:00:00.000000Z",
                                "digest": "0000000000000000",
                                "ready": true,
                                "payload": {"ready": true}
                            })];
                            rows.extend(page.clone());
                            JsonHttpResponse::ok(json!({
                                "rollout_id": rollout_id,
                                "cursor": {
                                    "kind": "sequence",
                                    "after": after,
                                    "high_water": high_water,
                                    "closed": *closed,
                                    "next": next,
                                    "has_more": available.len() > page.len(),
                                },
                                "events": rows,
                            }))
                        }
                        ("GET", path) if path.contains("/frames/") => {
                            let step: i64 = path.rsplit('/').next()
                                .and_then(|name| name.strip_suffix(".png"))
                                .and_then(|value| value.parse().ok())
                                .unwrap_or(-1);
                            if opts.redirect_frame_step == Some(step) {
                                let mut response = JsonHttpResponse::with_status(StatusCode::FOUND, json!({}));
                                response.extra_headers.push(("location", "http://example.com/evil.png".into()));
                                return response;
                            }
                            let body = if opts.corrupt_frame_step == Some(step) {
                                let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
                                bytes.extend_from_slice(&[0u8; 64]);
                                bytes
                            } else if opts.oversize_frame_step == Some(step) {
                                fixture_png(0).repeat(4096)
                            } else if opts.identical_frames {
                                fixture_png(7)
                            } else {
                                fixture_png((step % 251) as u8)
                            };
                            JsonHttpResponse {
                                status: StatusCode::OK,
                                body: json!({}),
                                extra_headers: Vec::new(),
                                raw_body: Some(body),
                                content_type: Some("image/png".into()),
                            }
                        }
                        ("POST", "/rollouts") => {
                            let rollout_id = request.body.get("rollout_id").and_then(Value::as_str)
                                .unwrap_or("roll_unknown").to_string();
                            // The rollout runs here: events become pollable one
                            // batch at a time while this request is still open.
                            for chunk in script.chunks(9) {
                                {
                                    let mut guard = journals.lock().unwrap();
                                    if let Some((events, _)) = guard.get_mut(&rollout_id) {
                                        for event in chunk {
                                            let mut event = event.clone();
                                            if let Some(url) = event.pointer("/payload/url").and_then(Value::as_str) {
                                                let resolved = url.replace(
                                                    "__ROLLOUT__",
                                                    &format!("/rollouts/{rollout_id}"),
                                                );
                                                event["payload"]["url"] = json!(resolved);
                                            }
                                            events.push(event);
                                        }
                                    }
                                }
                                tokio::time::sleep(Duration::from_millis(15)).await;
                            }
                            journals.lock().unwrap().entry(rollout_id.clone())
                                .and_modify(|entry| entry.1 = true);
                            if opts.trailing_hold_ms > 0 {
                                tokio::time::sleep(Duration::from_millis(opts.trailing_hold_ms)).await;
                            }
                            JsonHttpResponse::ok(json!({
                                "rollout_id": rollout_id,
                                "status": "completed",
                                "terminated": true,
                                "usage": {"prompt_tokens": 1000, "completion_tokens": 200, "calls": CRAFTAX_POLICY_CALLS},
                                "trace": {"url": format!("/rollouts/{rollout_id}/trace"), "closed": true},
                            }))
                        }
                        ("GET", path) if path.ends_with("/reward") => JsonHttpResponse::ok(json!({
                            "status": "scored",
                            "reward": CRAFTAX_TOTAL_REWARD,
                        })),
                        // No self-contained bundle: this container seals a lite
                        // trace only, and the record must say so rather than
                        // claiming an inspectable replay.
                        ("GET", path) if path.ends_with("/trace/bundle") =>
                            JsonHttpResponse::error(StatusCode::NOT_FOUND, "no bundle"),
                        ("GET", path) if path.ends_with("/trace") =>
                            JsonHttpResponse::error(StatusCode::NOT_FOUND, "no seal"),
                        ("GET", path) if path.starts_with("/rollouts/") =>
                            JsonHttpResponse::ok(json!({"status": "completed", "terminated": true})),
                        _ => JsonHttpResponse::error(
                            StatusCode::NOT_FOUND,
                            format!("unexpected {} {path}", request.method),
                        ),
                    }
                }
            })
            .await;
        });
        CraftaxMock { base: format!("http://{addr}"), task }
    }

    async fn declare_craftax_recipe(svc: &OptimizerService, session: &str, extra: &str) {
        let workspace = tempfile::Builder::new().prefix("ws-craftax-").tempdir().unwrap().into_path();
        std::fs::create_dir_all(workspace.join("workshop.recipes")).unwrap();
        std::fs::write(
            workspace.join("workshop.recipes/craftax.toml"),
            format!(
                r#"
id = "{CRAFTAX_EVAL}"
algorithm = "eval"
title = "Craftax baseline eval"
container = "craftax"
provider = "openai"
model = "gpt-5.6-luna"
locality = "host"
family = "craftax"
harness = "craftax_react"
policy_config = "craftax_luna_low"
concurrency = 1
train_seeds = [780003]
[bounds]
max_cost_usd = 0.50
max_total_rollouts = 1
{extra}
"#
            ),
        )
        .unwrap();
        let db = svc.database().clone();
        let session_id = session.to_string();
        let workspace_path = workspace.to_string_lossy().into_owned();
        db.run_transaction(move |conn| {
            conn.execute(
                "INSERT OR IGNORE INTO sessions(id,title,target_json,status,metadata_json,created_at,updated_at) VALUES(?1,?1,'{}','ready',?2,datetime('now'),datetime('now'))",
                rusqlite::params![session_id, serde_json::json!({"workspace": workspace_path}).to_string()],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        crate::workspace_scope::provision(svc.database(), session, workspace.to_str().unwrap())
            .await
            .unwrap();
    }

    /// Start one Craftax seed and wait for its run to settle.
    async fn run_craftax(
        opts: CraftaxMockOptions,
        extra_recipe: &str,
    ) -> (OptimizerService, String, CraftaxMock) {
        let (svc, dir, _events) = service().await;
        // The store outlives the assertions: a relayed frame must still be
        // readable after the container is gone, and a dropped temp dir would
        // hide that by deleting the evidence instead of proving it survived.
        std::mem::forget(dir);
        let mock = spawn_craftax_mock(opts).await;
        insert_container(&svc, "craftax", &mock.base, "ready").await;
        declare_craftax_recipe(&svc, "sess_craftax", extra_recipe).await;
        let (run, _) = svc
            .start_recipe(OptimizerRecipeRunRequest {
                recipe_id: CRAFTAX_EVAL.into(),
                session_ref: Some("sess_craftax".into()),
                open_visual: Some(false),
                base_model: None,
                dataset_shard: None,
                candidate_set_id: None,
                container_id: None,
                training_artifact_id: None,
                search: None,
                plan_override: None,
            })
            .await
            .unwrap();
        wait_terminal(&svc, &run.id).await;
        (svc, run.id, mock)
    }

    /// Every relayed container event, in optimizer-log order.
    async fn relayed_events(svc: &OptimizerService, run_id: &str) -> Vec<Value> {
        svc.events_after(run_id.to_string(), 0, Some(20_000))
            .await
            .unwrap()
            .into_iter()
            .map(|event| serde_json::to_value(event).unwrap())
            .collect()
    }

    fn container_events(events: &[Value]) -> Vec<&Value> {
        events
            .iter()
            .filter(|event| event.get("type").and_then(Value::as_str) == Some("eval.trial.event"))
            .collect()
    }

    fn kind_of(event: &Value) -> &str {
        event
            .pointer("/delta/containerEvent/kind")
            .and_then(Value::as_str)
            .unwrap_or("")
    }

    fn count_kind(events: &[Value], kind: &str) -> usize {
        container_events(events).iter().filter(|event| kind_of(event) == kind).count()
    }

    fn index_of_type(events: &[Value], event_type: &str) -> Option<usize> {
        events
            .iter()
            .position(|event| event.get("type").and_then(Value::as_str) == Some(event_type))
    }

    #[tokio::test]
    async fn a_running_craftax_rollout_relays_its_whole_journal_before_it_settles() {
        let (svc, run_id, mock) = run_craftax(CraftaxMockOptions::default(), "").await;
        let events = relayed_events(&svc, &run_id).await;

        // 1. The trial exists before any of its container events.
        let started = index_of_type(&events, "eval.trial.started").expect("no eval.trial.started");
        let first_container = events
            .iter()
            .position(|event| event.get("type").and_then(Value::as_str) == Some("eval.trial.event"))
            .expect("no relayed container events");
        assert!(started < first_container, "trial.started must precede its events");

        // 2. Policy and frame events land before the trial settles.
        let terminal = index_of_type(&events, "eval.trial.terminal").expect("no terminal");
        let last_frame = events
            .iter()
            .rposition(|event| kind_of(event) == "frame")
            .expect("no frame events relayed");
        let last_call = events
            .iter()
            .rposition(|event| kind_of(event) == "span.policy.data")
            .expect("no policy data relayed");
        assert!(last_frame < terminal && last_call < terminal);

        // 3. The regression fixture's own counts, which "0 native frames" denied.
        assert_eq!(count_kind(&events, "frame"), CRAFTAX_FRAMES);
        assert_eq!(count_kind(&events, "span.policy.data"), CRAFTAX_POLICY_CALLS);
        assert_eq!(count_kind(&events, "action_applied"), CRAFTAX_APPLIED_ACTIONS);
        assert_eq!(count_kind(&events, "achievement_unlocked"), CRAFTAX_ACHIEVEMENTS);
        // A proposed action the environment refused is visible and is not an
        // applied action.
        assert_eq!(count_kind(&events, "action_rejected"), 2);

        // 4. Every frame carries a real Workshop SHA-256, and the producer's
        //    16-character digest travels beside it without being mistaken for one.
        let frames: Vec<&Value> = container_events(&events)
            .into_iter()
            .filter(|event| kind_of(event) == "frame")
            .collect();
        for frame in &frames {
            let media = frame
                .pointer("/delta/containerEvent/payload/media")
                .expect("frame kept no media reference");
            let digest = media.get("casDigest").and_then(Value::as_str).unwrap();
            assert_eq!(digest.len(), 64, "casDigest must be a full SHA-256");
            assert_eq!(media["mediaType"], json!("image/png"));
            assert_eq!(media["width"], json!(8));
            assert_eq!(media["height"], json!(8));
            let producer = media.get("producerDigest").and_then(Value::as_str).unwrap();
            assert_eq!(producer.len(), 16);
            assert_ne!(producer, digest);
            assert!(
                svc.content().exists("eval_frames", digest),
                "frame bytes are not in the content store"
            );
        }

        // 5. No PNG body was ever inlined into the optimizer log.
        let encoded = serde_json::to_string(&events).unwrap();
        assert!(!encoded.contains("data:image/png;base64"), "a PNG leaked into optimizer_events");

        // 6. The record reports the reward and its relay receipt honestly.
        let run = svc.get(run_id.clone()).await.unwrap();
        let record = run.summary.pointer("/records/0").cloned().unwrap();
        assert_eq!(record["reward"], json!(CRAFTAX_TOTAL_REWARD));
        assert_eq!(record["relay"]["framesDeclared"], json!(CRAFTAX_FRAMES));
        assert_eq!(record["relay"]["framesRetained"], json!(CRAFTAX_FRAMES));
        assert_eq!(record["relay"]["journalClosed"], json!(true));
        // The container offers no self-contained bundle, and the record says
        // exactly that rather than implying an inspectable replay.
        assert_eq!(record["sealedTrace"]["imported"], json!(false));
        mock.task.abort();
    }

    #[tokio::test]
    async fn small_pages_relay_every_event_exactly_once() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions::default(),
            "[event_stream]\npoll_interval_ms = 20\npage_limit = 3\n",
        )
        .await;
        let events = relayed_events(&svc, &run_id).await;
        let mut sequences: Vec<u64> = container_events(&events)
            .into_iter()
            .filter_map(|event| {
                event.pointer("/delta/containerEvent/sequence").and_then(Value::as_u64)
            })
            .collect();
        let relayed = sequences.len();
        sequences.sort_unstable();
        sequences.dedup();
        assert_eq!(sequences.len(), relayed, "a producer sequence was relayed twice");
        assert_eq!(sequences, (1..=relayed as u64).collect::<Vec<_>>(), "the relay is not contiguous");
        assert!(relayed > 100, "paging lost events: only {relayed} relayed");
        mock.task.abort();
    }

    #[tokio::test]
    async fn a_sequence_gap_fails_the_trial_visibly() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions { skip_sequence: Some(12), ..Default::default() },
            "",
        )
        .await;
        let run = svc.get(run_id.clone()).await.unwrap();
        assert_eq!(run.status, "failed", "a gapped journal must not settle completed");
        let record = run.summary.pointer("/records/0").cloned().unwrap();
        let error = record["error"].as_str().unwrap_or_default();
        assert!(error.contains("sequence gap"), "gap was not named: {error}");
        mock.task.abort();
    }

    #[tokio::test]
    async fn identical_frames_deduplicate_without_losing_a_step() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions { identical_frames: true, ..Default::default() },
            "",
        )
        .await;
        let events = relayed_events(&svc, &run_id).await;
        let digests: Vec<String> = container_events(&events)
            .into_iter()
            .filter(|event| kind_of(event) == "frame")
            .filter_map(|event| {
                event
                    .pointer("/delta/containerEvent/payload/media/casDigest")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        // Thirteen logical frames in the timeline; one object on disk.
        assert_eq!(digests.len(), CRAFTAX_FRAMES);
        let unique: std::collections::BTreeSet<&String> = digests.iter().collect();
        assert_eq!(unique.len(), 1, "identical PNGs did not collapse in the store");
        mock.task.abort();
    }

    #[tokio::test]
    async fn a_corrupt_frame_is_refused_and_reported_rather_than_shown() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions { corrupt_frame_step: Some(3), ..Default::default() },
            "",
        )
        .await;
        let events = relayed_events(&svc, &run_id).await;
        let refused: Vec<&Value> = container_events(&events)
            .into_iter()
            .filter(|event| event.pointer("/delta/containerEvent/payload/mediaError").is_some())
            .collect();
        assert_eq!(refused.len(), 1, "the corrupt frame was not refused");
        // The step is still in the timeline: the environment did render it.
        assert_eq!(count_kind(&events, "frame"), CRAFTAX_FRAMES);
        let degraded: Vec<&Value> = events
            .iter()
            .filter(|event| event.get("type").and_then(Value::as_str) == Some("eval.trial.degraded"))
            .collect();
        assert_eq!(degraded.len(), 1, "a refused frame produced no degradation receipt");
        assert_eq!(degraded[0]["delta"]["dropped"], json!(1));
        let run = svc.get(run_id.clone()).await.unwrap();
        let record = run.summary.pointer("/records/0").cloned().unwrap();
        assert_eq!(record["relay"]["framesDeclared"], json!(CRAFTAX_FRAMES));
        assert_eq!(record["relay"]["framesRetained"], json!(CRAFTAX_FRAMES - 1));
        mock.task.abort();
    }

    #[tokio::test]
    async fn a_redirected_frame_is_never_followed() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions { redirect_frame_step: Some(2), ..Default::default() },
            "",
        )
        .await;
        let events = relayed_events(&svc, &run_id).await;
        let detail = container_events(&events)
            .into_iter()
            .find_map(|event| {
                event
                    .pointer("/delta/containerEvent/payload/mediaError/detail")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .expect("the redirect was not refused");
        assert!(detail.contains("redirect"), "{detail}");
        mock.task.abort();
    }

    #[tokio::test]
    async fn an_oversized_frame_is_refused_by_the_declared_budget() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions { oversize_frame_step: Some(1), ..Default::default() },
            "[media]\nmax_frame_bytes = 4096\n",
        )
        .await;
        let events = relayed_events(&svc, &run_id).await;
        let detail = container_events(&events)
            .into_iter()
            .find_map(|event| {
                event
                    .pointer("/delta/containerEvent/payload/mediaError/detail")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .expect("the oversized frame was not refused");
        assert!(detail.contains("max_frame_bytes"), "{detail}");
        mock.task.abort();
    }

    #[tokio::test]
    async fn frame_retention_none_keeps_the_events_and_says_the_bytes_were_not_kept() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions::default(),
            "[media]\nframe_retention = \"none\"\n",
        )
        .await;
        let events = relayed_events(&svc, &run_id).await;
        assert_eq!(count_kind(&events, "frame"), CRAFTAX_FRAMES);
        assert_eq!(
            container_events(&events)
                .into_iter()
                .filter(|event| event.pointer("/delta/containerEvent/payload/media").is_some())
                .count(),
            0
        );
        let run = svc.get(run_id.clone()).await.unwrap();
        let record = run.summary.pointer("/records/0").cloned().unwrap();
        assert_eq!(record["relay"]["framesDeclared"], json!(CRAFTAX_FRAMES));
        assert_eq!(record["relay"]["framesRetained"], json!(0));
        mock.task.abort();
    }

    #[tokio::test]
    async fn a_stopped_container_does_not_take_the_relayed_replay_with_it() {
        let (svc, run_id, mock) = run_craftax(CraftaxMockOptions::default(), "").await;
        // The container is gone. Everything below reads only Workshop's own
        // durable evidence.
        mock.task.abort();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let events = relayed_events(&svc, &run_id).await;
        assert_eq!(count_kind(&events, "frame"), CRAFTAX_FRAMES);
        for event in container_events(&events) {
            let Some(digest) = event
                .pointer("/delta/containerEvent/payload/media/casDigest")
                .and_then(Value::as_str)
            else {
                continue;
            };
            let bytes = svc.content().get_bytes("eval_frames", digest).unwrap();
            assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        }
    }

    #[tokio::test]
    async fn relayed_frames_are_granted_only_to_the_run_that_produced_them() {
        let (svc, run_id, mock) = run_craftax(CraftaxMockOptions::default(), "").await;
        let events = relayed_events(&svc, &run_id).await;
        let digest = container_events(&events)
            .into_iter()
            .find_map(|event| {
                event
                    .pointer("/delta/containerEvent/payload/media/casDigest")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .expect("no frame media to grant");

        // The producing run may read its own frame, and the answer carries the
        // identity that made it readable.
        let granted = svc
            .granted_run_media(&run_id, &digest)
            .await
            .unwrap()
            .expect("the producing run was refused its own frame");
        assert_eq!(granted.media_type, "image/png");
        assert_eq!(granted.width, Some(8));
        assert_eq!(granted.step.is_some(), true);
        assert!(svc.read_media_bytes(&granted).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));

        // A different run is not, even though the object is in the same store.
        // This is the whole gate: possession of a digest is not authority.
        assert!(svc
            .granted_run_media("opt_eval_someone_else", &digest)
            .await
            .unwrap()
            .is_none());

        // A digest the store does not hold is refused rather than probed for.
        assert!(svc
            .granted_run_media(&run_id, &"f".repeat(64))
            .await
            .unwrap()
            .is_none());

        // Anything that is not a SHA-256 is rejected before it reaches the
        // store, so a producer's 16-character label cannot become a lookup.
        assert!(svc.granted_run_media(&run_id, "4e27ac3b1f0a9d55").await.is_err());
        assert!(svc.granted_run_media(&run_id, "../../etc/passwd").await.is_err());
        mock.task.abort();
    }

    #[tokio::test]
    async fn identical_frames_share_one_grant_across_every_step_that_rendered_them() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions { identical_frames: true, ..Default::default() },
            "",
        )
        .await;
        let events = relayed_events(&svc, &run_id).await;
        let digest = container_events(&events)
            .into_iter()
            .find_map(|event| {
                event
                    .pointer("/delta/containerEvent/payload/media/casDigest")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap();
        // Thirteen logical frames, one indexed grant: the index is keyed by
        // content, exactly like the store it authorizes.
        let rows: i64 = svc
            .database()
            .clone()
            .run({
                let run_id = run_id.clone();
                move |conn| {
                    conn.query_row(
                        "SELECT COUNT(*) FROM optimizer_run_media WHERE optimizer_run_id=?1",
                        rusqlite::params![run_id],
                        |row| row.get(0),
                    )
                    .map_err(Into::into)
                }
            })
            .await
            .unwrap();
        assert_eq!(rows, 1);
        assert!(svc.granted_run_media(&run_id, &digest).await.unwrap().is_some());
        mock.task.abort();
    }
}
