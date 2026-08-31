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
use serde::{Deserialize, Serialize};
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
const WORKBENCH_TEMPLATE: &str = "trace.workbench.v1";
const EVAL_ALGORITHM_ID: &str = "eval";
const POLL_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(80);
const DEFAULT_BLOCKING_EVAL_HTTP_TIMEOUT: Duration = crate::limits::CONTAINER_POLICY_ROLLOUT_TIMEOUT;

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
    policy_source_revision: String,
    policy_configuration_digest: String,
    provider: String,
    model: String,
    concurrency: usize,
    train: Vec<i64>,
    heldout: Vec<i64>,
    cost_ceiling_usd: f64,
    maximum_model_calls_per_rollout: u32,
    maximum_steps_per_rollout: u32,
    admitted_use_policy: Option<crate::secrets::SecretsUsePolicy>,
    requires_credential_advertisement: bool,
    relay: RelaySettings,
}

impl EvalSpec {
    fn from_execution_spec(
        execution: &super::admission::ExecutionSpec,
        family: String,
        world_ref: String,
    ) -> Result<Self> {
        let recipe = &execution.recipe;
        let policy = recipe
            .policy
            .configuration
            .as_value()
            .as_object()
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("approved inline policy configuration is not an object")
            })?;
        let admitted_use_policy = execution.provider_use_policy();
        Ok(Self {
            recipe_id: format!("inline:{}", execution.digest()?.as_str()),
            family,
            title: format!(
                "{} · {}",
                recipe.policy.qualified_name(),
                recipe.model.model_id
            ),
            question: format!(
                "Score {} with the container-declared evaluator.",
                recipe.policy.qualified_name()
            ),
            world_ref,
            evaluation_plan_ref: recipe.evaluator.evaluator_id().as_str().to_string(),
            harness: recipe.policy.namespace.clone(),
            policy_config: recipe.policy.name.clone(),
            policy,
            policy_code: recipe.policy.source_code.clone(),
            policy_source_revision: recipe.policy.revision.as_str().to_string(),
            policy_configuration_digest: recipe.policy.configuration_digest.as_str().to_string(),
            provider: recipe.model.provider.as_str().to_string(),
            model: recipe.model.model_id.as_str().to_string(),
            concurrency: recipe.rollout_plan.maximum_rollouts.0.get() as usize,
            train: recipe
                .rollout_plan
                .seeds
                .iter()
                .map(|seed| seed.0)
                .collect(),
            heldout: Vec::new(),
            cost_ceiling_usd: recipe.resource_limits.hard_total_cost_micros.as_micros() as f64
                / 1_000_000.0,
            maximum_model_calls_per_rollout: recipe
                .resource_limits
                .maximum_model_calls_per_rollout
                .0
                .get(),
            maximum_steps_per_rollout: recipe.resource_limits.maximum_steps_per_rollout.0.get(),
            admitted_use_policy: Some(admitted_use_policy),
            requires_credential_advertisement: false,
            relay: RelaySettings::default(),
        })
    }

    fn requires_credential_advertisement(&self) -> bool {
        self.requires_credential_advertisement
    }

    fn blocking_http_timeout(&self) -> Duration {
        let per_call_timeout = self
            .policy
            .get("timeout_seconds")
            .and_then(Value::as_f64)
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0);
        let configured = per_call_timeout
            .map(|per_call| {
                let calls = self.maximum_model_calls_per_rollout.max(1) as f64;
                Duration::from_secs_f64((per_call * calls + 60.0).clamp(60.0, 86_400.0))
            })
            .unwrap_or(DEFAULT_BLOCKING_EVAL_HTTP_TIMEOUT);
        // Inline capabilities are the authoritative time bound on provider
        // work. Waiting less than their lifetime converts a still-authorized
        // producer retry sequence into a host transport failure. One final
        // request may already be in flight when authority expires, so retain
        // its declared timeout plus terminal-settlement grace. This grants no
        // additional calls or spend.
        let capability_bound = self.admitted_use_policy.as_ref().map(|policy| {
            let terminal_grace = per_call_timeout.unwrap_or(60.0).ceil() as u64;
            Duration::from_secs(
                policy
                    .lifetime_seconds
                    .saturating_add(terminal_grace)
                    .saturating_add(60)
                    .min(86_400),
            )
        });
        capability_bound.map_or(configured, |bound| configured.max(bound))
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
        let policy_configuration_digest =
            super::admission::CanonicalJson::new(Value::Object(recipe.policy.clone()))?
                .digest()
                .as_str()
                .to_string();
        Ok(Self {
            recipe_id: recipe.id.clone(),
            family: recipe.family.clone(),
            title: recipe.title.clone(),
            question: format!(
                "Score the advertised {} policy on the declared eval pool.",
                recipe.family
            ),
            world_ref: format!("world:{}@eval", recipe.family),
            evaluation_plan_ref: format!("{}_eval.v1", recipe.family),
            harness: recipe.harness.clone(),
            policy_config: recipe.policy_config.clone(),
            policy: recipe.policy.clone(),
            policy_code,
            policy_source_revision: recipe.source_hash.clone(),
            policy_configuration_digest,
            provider: recipe.provider.clone(),
            model: recipe.model.clone(),
            concurrency: recipe.concurrency,
            train: recipe.train_seeds.clone(),
            heldout: recipe.heldout_seeds.clone(),
            cost_ceiling_usd: recipe.bounds.max_cost_usd,
            maximum_model_calls_per_rollout: recipe
                .policy
                .get("max_calls")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0)
                .unwrap_or(1),
            maximum_steps_per_rollout: recipe
                .policy
                .get("max_steps")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0)
                .unwrap_or(1),
            admitted_use_policy: None,
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
            policy_source_revision: "fixture-revision".into(),
            policy_configuration_digest:
                "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a".into(),
            provider: "openrouter".into(),
            model: "openai/gpt-4.1-nano".into(),
            concurrency: 10,
            train: (0..10).collect(),
            heldout: Vec::new(),
            cost_ceiling_usd: 0.50,
            maximum_model_calls_per_rollout: 10,
            maximum_steps_per_rollout: 2_000,
            admitted_use_policy: None,
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
            policy_source_revision: "fixture-revision".into(),
            policy_configuration_digest:
                "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a".into(),
            provider: "openai".into(),
            model: "gpt-4.1-mini-2025-04-14".into(),
            concurrency: 2,
            train: vec![0, 1],
            heldout: vec![100, 101],
            cost_ceiling_usd: 0.50,
            maximum_model_calls_per_rollout: 8,
            maximum_steps_per_rollout: 64,
            admitted_use_policy: None,
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
        ]));
        if self.harness != "nanohorizon" {
            config.insert("api_key_env".into(), json!("OPENAI_API_KEY"));
        }
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
    producer_source_revision: Option<String>,
    metadata: Value,
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
    start_eval(service, request.session_ref.clone(), spec, container, None).await
}

/// The inline executor accepts only the final, approval-bound stage.
pub(super) async fn start_inline(
    service: &OptimizerService,
    approved: super::admission::ApprovedExecutionSpec,
    session_ref: Option<String>,
) -> Result<(OptimizerRunRecord, Option<crate::storage::AppEvent>)> {
    let recipe = approved.recipe();
    let (mut container, family) =
        find_ready_container_by_id(service, recipe.container.container_id.as_str()).await?;
    let info = fresh_container_info(&container.base_url, "inline container").await?;
    refresh_inline_container_provenance(&mut container, &info)?;
    let evaluator_ref = info
        .pointer("/logical_service_ids/evaluator")
        .and_then(Value::as_str)
        .context("container /info no longer declares an evaluator")?;
    if evaluator_ref != recipe.evaluator.evaluator_id().as_str() {
        bail!(
            "container evaluator changed after approval: approved {}, current {}",
            recipe.evaluator.evaluator_id(),
            evaluator_ref
        );
    }
    let world_ref = info
        .pointer("/logical_service_ids/world")
        .and_then(Value::as_str)
        .context("container /info does not declare a world identity")?
        .to_string();
    let mut spec = EvalSpec::from_execution_spec(approved.spec(), family, world_ref)?;
    if spec.policy_code.is_none() {
        if let Some(material) = &recipe.policy.material {
            let origin = super::workspace_recipe::ContainerDeclarationOrigin {
                manifest_path: std::path::PathBuf::from(&material.source_root)
                    .join("workshop.containers.toml"),
                source_root: std::path::PathBuf::from(&material.source_root),
                declaration_id: recipe.container.container_id.as_str().to_string(),
                source_revision: Some(material.tracked_revision.clone()),
                source_digest: Some(material.content_digest.as_str().to_string()),
            };
            let resolved = super::workspace_recipe::resolve_repository_path(
                &origin,
                &material.repository_relative_path,
            )
            .map_err(super::workspace_recipe::LaunchDeclarationError::into_anyhow)?;
            let bytes = std::fs::read_to_string(&resolved.absolute_path).with_context(|| {
                format!(
                    "policy_source_unavailable: could not re-read {} from {}",
                    material.repository_relative_path, material.source_root
                )
            })?;
            let digest = super::admission::digest_bytes(bytes.as_bytes());
            anyhow::ensure!(
                digest == material.content_digest
                    && recipe
                        .policy
                        .source_digest
                        .as_ref()
                        .is_none_or(|expected| *expected == digest),
                "policy_source_drift: approved digest {}, current {}",
                material.content_digest,
                digest
            );
            spec.policy_code = Some(bytes);
        }
    }
    start_eval(service, session_ref, spec, container, Some(approved)).await
}

async fn find_ready_container_by_id(
    service: &OptimizerService,
    container_id: &str,
) -> Result<(ReadyContainer, String)> {
    let wanted = container_id.to_string();
    let family = service
        .database()
        .clone()
        .run(move |conn| {
            conn.query_row(
                "SELECT task_family FROM containers WHERE id = ?1",
                [&wanted],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(anyhow::Error::from)
        })
        .await?
        .filter(|value| !value.trim().is_empty())
        .context("approved container has no task family")?;
    let container = find_ready_container(service, &family, Some(container_id)).await?;
    Ok((container, family))
}

async fn start_eval(
    service: &OptimizerService,
    session_ref: Option<String>,
    spec: EvalSpec,
    mut container: ReadyContainer,
    approved: Option<super::admission::ApprovedExecutionSpec>,
) -> Result<(OptimizerRunRecord, Option<crate::storage::AppEvent>)> {
    let preflight_info = preflight_container_credentials(&container, &spec).await?;
    if approved.is_some() {
        // Credential preflight is the last producer read before durable run
        // creation. Recheck the same immutable facts here to close the gap
        // between the post-approval identity refresh and dispatch.
        refresh_inline_container_provenance(&mut container, &preflight_info)?;
    }
    let examples = spec.examples();
    let suffix = Uuid::new_v4().simple().to_string();
    let run_id = format!("opt_eval_{}_{}", spec.family, &suffix[..12]);
    let effective_contract = service.negotiate_effective_contract(
        &run_id,
        &container.id,
        Some(&spec.family),
        &container.metadata,
    )?;
    let summary = json!({
        "recipeId": spec.recipe_id,
        "task": spec.family,
        "semantics": "baseline_eval",
        "containerProtocol": container.protocol,
        "containerId": container.id,
        "containerBaseUrl": container.base_url,
        "containerImageDigest": container.image_digest,
        "containerProducerSourceRevision": container.producer_source_revision,
        "expectedVisual": effective_contract.primary_visual.template_id.clone(),
        "effectiveContract": effective_contract,
        "policyRef": { "harness": spec.harness, "config": spec.policy_config },
        "taskPools": { "train": spec.train.len(), "heldout": spec.heldout.len() },
        "concurrency": spec.concurrency,
        "costCeilingUsd": spec.cost_ceiling_usd,
        "bounds": {
            "maximumRollouts": examples.len(),
            "maximumModelCallsPerRollout": spec.maximum_model_calls_per_rollout,
            "maximumStepsPerRollout": spec.maximum_steps_per_rollout,
            "maximumTokens": Value::Null,
            "hardTotalCostUsd": spec.cost_ceiling_usd,
        },
        "recipeSourceKind": if approved.is_some() { "inline" } else { "catalog" },
        "startedAt": chrono::Utc::now().to_rfc3339(),
        "records": [],
        "progress": { "completed": 0, "total": examples.len(), "failed": 0 },
    });
    let create = OptimizerCreateRequest {
        algorithm_id: EVAL_ALGORITHM_ID.into(),
        algorithm_version: Some("1".into()),
        objective: Some(spec.title.clone()),
        source: Some("local".into()),
        project_ref: Some(format!("{}@eval", spec.family)),
        session_ref,
        id: Some(run_id.clone()),
        execution_bindings: Some(vec![OptimizerExecutionBinding {
            kind: "container_http".into(),
            id: container.id.clone(),
            label: Some(format!("{} container", spec.family)),
            status: Some("starting".into()),
            metadata: json!({
                "recipeId": spec.recipe_id,
                "baseUrl": container.base_url,
                "imageDigest": container.image_digest,
                "producerSourceRevision": container.producer_source_revision,
            }),
        }]),
        input_refs: Some(vec![
            OptimizerResourceRef {
                kind: "recipe".into(),
                id: spec.recipe_id.clone(),
                digest: spec
                    .recipe_id
                    .strip_prefix("inline:")
                    .filter(|digest| valid_sha256_digest(digest))
                    .map(str::to_string),
                role: Some("configuration".into()),
                title: Some(spec.title.clone()),
                metadata: json!({ "semantics": "baseline_eval" }),
            },
            OptimizerResourceRef {
                kind: "container".into(),
                id: container.id.clone(),
                digest: container.image_digest.clone(),
                role: Some("runtime".into()),
                title: Some(format!("Registered {} container", spec.family)),
                metadata: json!({
                    "baseUrl": container.base_url,
                    "imageDigest": container.image_digest,
                    "producerSourceRevision": container.producer_source_revision,
                }),
            },
        ]),
        capabilities: Some(OptimizerCapabilities::for_algorithm(EVAL_ALGORITHM_ID)),
        summary: Some(summary),
        open_visual: Some(false),
        seed_fixture: None,
        cloud_config: None,
        local_path: None,
    };
    let (run, event) = if let Some(approved) = approved {
        service
            .create_admitted_eval(create, approved, examples.len())
            .await?
    } else {
        service.create(create).await?
    };
    if spec.admitted_use_policy.is_some() {
        // Inline runs derive their provider grant only from the immutable,
        // operator-approved execution envelope. Do this before visuals or the
        // worker are started so an existing narrower capability for this run
        // is refused pre-dispatch rather than failing on call 41. The worker's
        // later route lookup reuses this exact capability.
        if let Err(error) = container_openai_proxy_base(&run.id, &spec) {
            let detail = format!("provider capability preflight failed: {error:#}");
            append_terminal(service, &run.id, "failed", detail).await?;
            return Err(error);
        }
    }
    let visual_id = mint_experiment_visual(service, &run, &spec, examples.len()).await?;
    // Minted with the run, not when the first seed finishes: a workstation that
    // appears only after there is something to see cannot show a rollout
    // starting, which is the thing it exists to show.
    let workbench_id = mint_workbench_visual(service, &run, &spec).await?;
    let (cancel_tx, cancel_rx) = watch::channel(None);
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
            if let Err(settlement_error) = settle_worker_failure(&worker, &worker_run_id, error).await {
                if let Err(record_error) = worker
                    .record_visual_projection_delivery_failure(&worker_run_id, &settlement_error)
                    .await
                {
                    crate::platform::logging::report(
                        "container_eval", "projection_outbox",
                        format!("could not record settlement delivery failure for {worker_run_id}: {record_error:#}"),
                    );
                }
            } else if let Err(projection_error) = project_worker_failure_visual(
                &worker,
                &worker_run_id,
                &worker_spec,
                &worker_visual_id,
                &worker_workbench_id,
                planned_trials,
            )
            .await
            {
                if let Err(record_error) = worker
                    .record_visual_projection_delivery_failure(&worker_run_id, &projection_error)
                    .await
                {
                    crate::platform::logging::report(
                        "container_eval", "projection_outbox",
                        format!("could not record visual delivery failure for {worker_run_id}: {record_error:#}"),
                    );
                }
            }
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
    let progress = inline_progress_projection(service, run_id).await?;
    let (updated, event) = service
        .visuals()
        .update(
            visual_id.to_string(),
            VisualUpdateRequest {
                title: None,
                bindings: Some(experiment_bindings(
                    spec,
                    run_id,
                    "failed",
                    records.len(),
                    total,
                    &records,
                    mean,
                    workbench_id,
                    progress.as_ref(),
                    run.started_at.as_deref().unwrap_or(&run.created_at),
                    Some(&run.usage),
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
async fn preflight_container_credentials(
    container: &ReadyContainer,
    spec: &EvalSpec,
) -> Result<Value> {
    let info = fresh_container_info(
        &container.base_url,
        &format!("{} credential preflight", spec.family),
    )
    .await?;
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
        return Ok(info);
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
    Ok(info)
}

async fn fresh_container_info(base_url: &str, label: &str) -> Result<Value> {
    let client = crate::http::http_client_with_timeout(Duration::from_secs(15));
    let mut info = client
        .get(format!("{}/info", base_url.trim_end_matches('/')))
        .send()
        .await
        .with_context(|| format!("{label} GET /info"))?
        .error_for_status()
        .with_context(|| format!("{label} /info was not successful"))?
        .json::<Value>()
        .await
        .with_context(|| format!("decode {label} /info"))?;
    if info.get("imageDigest").is_none() || info.get("producerSourceRevision").is_none() {
        let health = client
            .get(format!("{}/health", base_url.trim_end_matches('/')))
            .send()
            .await
            .with_context(|| format!("{label} GET /health identity"))?
            .error_for_status()
            .with_context(|| format!("{label} /health identity was not successful"))?
            .json::<Value>()
            .await
            .with_context(|| format!("decode {label} /health identity"))?;
        if let Some(object) = info.as_object_mut() {
            if let Some(identity) = health.get("runtime_identity") {
                if object.get("imageDigest").is_none() {
                    if let Some(value) = identity.get("image_digest") {
                        object.insert("imageDigest".into(), value.clone());
                    }
                }
                if object.get("producerSourceRevision").is_none() {
                    if let Some(value) = identity.get("producer_source_revision") {
                        object.insert("producerSourceRevision".into(), value.clone());
                    }
                }
            }
        }
    }
    Ok(info)
}

/// Settle a worker that did not reach its own terminal event.
///
/// A compute failure is a failed run and says why. An evidence failure is not:
/// the rollouts may all have succeeded and been paid for, so the run settles as
/// `degraded` with a named, retryable diagnostic, and the records it did gather
/// stay in the summary. Either way the run reaches a terminal state — a worker
/// that dies silently leaves a card spinning forever.
async fn settle_worker_failure(
    service: &OptimizerService,
    run_id: &str,
    error: anyhow::Error,
) -> Result<()> {
    // Provider usage is durable in the secrets proxy independently of worker
    // control flow. Fold it into the optimizer journal before every terminal
    // path, including setup and evidence failures that bypass normal campaign
    // settlement. This is idempotent if cancellation already appended it.
    let usage_error = append_provider_usage_reconciliation(service, run_id)
        .await
        .err()
        .map(|failure| format!("provider usage reconciliation failed: {failure:#}"));
    if let Some(usage_error) = usage_error {
        return append_terminal(
            service,
            run_id,
            "failed",
            format!("{error:#}; {usage_error}"),
        )
        .await;
    }
    // Cancellation is not a generic application error. A typed cancellation
    // settles `cancelled` with its request as the terminal event's payload,
    // instead of laundering the interruption into `failed/producer_failed`.
    if let Some(cancelled) = error.downcast_ref::<super::kernel::CancelledError>() {
        return append_cancelled_terminal(service, run_id, &cancelled.request).await;
    }
    if let Some(failure) = error.downcast_ref::<EvidenceLaneFailure>() {
        let stage = failure.stage;
        let detail = failure.detail.clone();
        if service
            .settle_evidence_degraded(run_id.to_string(), stage, detail)
            .await
            .is_ok()
        {
            return Ok(());
        }
        // The degraded settlement itself failed. Fall through: a terminal event
        // the user can see beats a run stuck at `running` with no explanation.
    }
    append_terminal(service, run_id, "failed", format!("{error:#}")).await
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
    let template_id = run
        .summary
        .pointer("/effectiveContract/traceVisual/templateId")
        .and_then(Value::as_str)
        .unwrap_or(WORKBENCH_TEMPLATE);
    let (visual_id, _event) = service
        .publish_chat_owned_visual(ChatVisualPublication {
            run_id: run.id.clone(),
            session_ref: run.session_ref.clone(),
            template_id: template_id.into(),
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
    let template_id = run
        .summary
        .pointer("/effectiveContract/primaryVisual/templateId")
        .and_then(Value::as_str)
        .unwrap_or(EXPERIMENT_TEMPLATE);
    let progress = inline_progress_projection(service, &run.id).await?;
    // One publication, not five calls: mint-or-reuse, bind to the run, publish
    // the durable show, select it for the owning chat, and shelve it in that
    // chat's Outputs. Repeating it returns the same visual.
    let (visual_id, _event) = service
        .publish_chat_owned_visual(ChatVisualPublication {
            run_id: run.id.clone(),
            session_ref: run.session_ref.clone(),
            template_id: template_id.into(),
            title: spec.title.clone(),
            // The overview is minted before the workstation exists, so the
            // first projection carries no drill-down target. `persist_progress`
            // supplies it from the next update onward; a seed row with no
            // target simply has no chip, rather than one that goes nowhere.
            bindings: experiment_bindings(
                spec,
                &run.id,
                "running",
                0,
                total,
                &[],
                None,
                "",
                progress.as_ref(),
                &run.created_at,
                Some(&run.usage),
            ),
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
    optimizer_run_id: &str,
    status: &str,
    completed: usize,
    total: usize,
    records: &[Value],
    mean_reward: Option<f64>,
    workbench_id: &str,
    progress_projection: Option<&Value>,
    started_at: &str,
    authoritative_usage: Option<&super::models::OptimizerUsageSummary>,
) -> Value {
    let train_mean = mean_for_pool(records, "train");
    let heldout_mean = mean_for_pool(records, "heldout");
    let rollouts = spec
        .examples()
        .iter()
        .enumerate()
        .map(|(index, example)| {
            let record = records
                .iter()
                .find(|record| record.get("seed").and_then(Value::as_i64) == Some(example.seed));
            seed_row(
                record,
                example,
                progress_projection
                    .and_then(|projection| projection.get("rollouts"))
                    .and_then(|rollouts| rollouts.get(index.to_string()))
                    .and_then(|rollout| rollout.get("state"))
                    .and_then(Value::as_str),
                workbench_id,
            )
        })
        .collect::<Vec<_>>();
    let state_counts = progress_projection
        .and_then(|projection| projection.get("rolloutStateCounts"))
        .cloned()
        .unwrap_or_else(|| {
            if matches!(status, "completed" | "failed" | "cancelled" | "degraded") {
                json!({
                    "completed": records.iter().filter(|record| is_successful_eval_record(record)).count(),
                    "failed": records.iter().filter(|record| !is_successful_eval_record(record)).count(),
                    "cancelled": total.saturating_sub(records.len()),
                    "queued": 0,
                })
            } else {
                json!({
                    "completed": records.iter().filter(|record| is_successful_eval_record(record)).count(),
                    "failed": records.iter().filter(|record| !is_successful_eval_record(record)).count(),
                    "queued": total.saturating_sub(records.len()),
                })
            }
        });
    let active = progress_projection
        .and_then(|projection| projection.get("inFlight"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let elapsed = elapsed_label(started_at);
    let measured_usage = usage_from_records(records, spec.cost_ceiling_usd);
    let usage = authoritative_usage
        .map(|current| usage_with_authoritative_provider_receipt(measured_usage.clone(), current))
        .unwrap_or(measured_usage);
    let total_tokens = usage.prompt_tokens + usage.completion_tokens;
    let phase = if matches!(
        status,
        "completed" | "failed" | "failed_evidence" | "cancelled" | "degraded"
    ) {
        status.to_string()
    } else if active > 0 {
        format!("running · {active} active")
    } else {
        "queued".to_string()
    };
    let terminal = matches!(
        status,
        "completed" | "failed" | "failed_evidence" | "cancelled" | "degraded"
    );
    let eta = if terminal {
        "terminal".to_string()
    } else {
        eta_label(&elapsed, completed, total)
    };
    let usage_label = if total_tokens > 0 {
        format!("{total_tokens} tokens")
    } else if status == "running" {
        "awaiting telemetry".to_string()
    } else {
        "unavailable".to_string()
    };
    let cost_label = usage
        .cost_usd
        .map(|cost| format!("${cost:.6} / ${:.2}", spec.cost_ceiling_usd))
        .unwrap_or_else(|| {
            if status == "running" {
                format!("awaiting telemetry / ${:.2}", spec.cost_ceiling_usd)
            } else {
                format!("unavailable / ${:.2}", spec.cost_ceiling_usd)
            }
        });
    let mut limitations =
        vec!["Baseline-only. No candidate generation and no uplift claim.".to_string()];
    let failed_rollouts = records
        .iter()
        .filter(|record| record.get("status").and_then(Value::as_str) == Some("failed"))
        .count();
    if failed_rollouts > 0 {
        limitations.push(format!("{failed_rollouts} of {total} rollouts failed."));
    }
    let trace_import_failures = records
        .iter()
        .filter(|record| {
            record
                .pointer("/sealedTrace/imported")
                .and_then(Value::as_bool)
                == Some(false)
        })
        .count();
    if trace_import_failures > 0 {
        limitations.push(format!(
            "Workshop could not import {trace_import_failures} sealed trace bundles; producer trace identities remain available."
        ));
    }
    let relay_degradations = records
        .iter()
        .filter(|record| {
            record
                .pointer("/relay/degradations")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
        })
        .count();
    if relay_degradations > 0 {
        limitations.push(format!(
            "Live relay degraded for {relay_degradations} rollouts; terminal container evidence was still collected."
        ));
    }
    if terminal && usage.cost_usd.is_none() {
        limitations.push(
            "The producer reported no cost telemetry; cost is unavailable, not zero.".to_string(),
        );
    }
    let assessment_summary = match status {
        "running" => format!(
            "The exact baseline is active: {completed} of {total} rollouts have reached terminal evidence."
        ),
        "completed" => format!(
            "All {total} approved rollouts completed{}.",
            mean_reward
                .map(|value| format!(" with mean reward {value:.3}"))
                .unwrap_or_default()
        ),
        "degraded" => format!(
            "The run is terminal but partial: {completed} of {total} rollouts completed and {failed_rollouts} failed."
        ),
        "failed" => format!(
            "The evaluation failed: {failed_rollouts} of {total} rollouts did not complete successfully."
        ),
        "failed_evidence" => {
            "The rollouts stopped, but required evaluator evidence is missing or unusable."
                .to_string()
        }
        "cancelled" => format!(
            "The evaluation was cancelled after {completed} of {total} rollouts completed."
        ),
        other => format!("The evaluation reported unsupported status `{other}`."),
    };
    let next_step = match status {
        "running" => "Continue following this authoritative run to a terminal state.",
        "degraded" if trace_import_failures > 0 => {
            "Reconcile the already-sealed trace evidence; do not rerun paid compute."
        }
        "degraded" => "Inspect the failed rollout and retained traces before drawing a conclusion.",
        "failed" => "Inspect the per-seed failure reasons before deciding whether to run again.",
        "failed_evidence" => {
            "Inspect the typed evidence failure and retained receipts; do not treat this as a successful evaluation."
        }
        "cancelled" => "Start a new approved run only if the cancelled work is still required.",
        "completed" => "Use the retained per-seed traces to inspect behavior behind the aggregate.",
        _ => "Inspect the terminal evidence.",
    };
    let retained_traces = rollouts
        .iter()
        .filter(|row| {
            row.get("traceId").is_some_and(|value| !value.is_null())
                && row.get("summary").and_then(Value::as_str)
                    != Some("no relay receipt was recorded for this seed")
        })
        .cloned()
        .collect::<Vec<_>>();
    let streams_opened = records
        .iter()
        .filter(|record| record.get("relay").is_some())
        .count();
    let sealed_traces = records
        .iter()
        .filter(|record| {
            record.pointer("/sealedTrace/traces/0/traceId").is_some()
                || record.pointer("/trace/bundle_trace_id").is_some()
        })
        .count();
    let queued = state_counts
        .get("queued")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut reconciliation_errors = progress_projection
        .and_then(|projection| projection.get("reconciliationErrors"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if terminal && queued > 0 {
        reconciliation_errors.push(json!(
            "The campaign finished, but queued rollouts were never started."
        ));
    }
    if spec.maximum_model_calls_per_rollout == 0 || spec.maximum_steps_per_rollout == 0 {
        reconciliation_errors.push(json!(
            "Approved call and step limits are missing from the run receipt."
        ));
    }
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
                    "phase": phase,
                    "completed": completed,
                    "total": total,
                    "active": active,
                    "stateCounts": state_counts,
                    "elapsed": elapsed,
                    "eta": eta,
                    "usage": usage_label,
                    "cost": cost_label,
                },
                "task": {
                    "name": spec.family,
                    "evaluator": spec.evaluation_plan_ref,
                    "world": spec.world_ref,
                    "seeds": spec.train.len() + spec.heldout.len(),
                },
                "runtime": {
                    "model": spec.model,
                    "provider": spec.provider,
                    "policy": format!("{} / {}", spec.harness, spec.policy_config),
                    "parallelism": spec.concurrency,
                    "maximumModelCallsPerRollout": spec.maximum_model_calls_per_rollout,
                    "maximumStepsPerRollout": spec.maximum_steps_per_rollout,
                },
                "provenance": {
                    "source": if spec.recipe_id.starts_with("inline:") { "inline" } else { "catalog" },
                    "executionSpecDigest": spec.recipe_id.strip_prefix("inline:").unwrap_or("catalog recipe"),
                },
                "metrics": [
                    {"label": "Train mean", "value": train_mean, "detail": if train_mean.is_none() { "omitted until the split is complete" } else { "mean of present rewards" }},
                    {"label": "Heldout mean", "value": heldout_mean, "detail": if spec.heldout.is_empty() { "this recipe has no heldout pool" } else if heldout_mean.is_none() { "omitted until the split is complete" } else { "mean of present rewards" }},
                    {"label": "Overall mean", "value": mean_reward, "detail": "missing rewards stay missing"}
                ],
                "results": {
                    "rollouts": rollouts,
                },
                "assessment": {
                    "summary": assessment_summary,
                    "nextStep": next_step,
                },
                // Each seed opens the workstation, which is bound to the run
                // rather than to this snapshot — so a seed row is a way in, not
                // a second copy of the evidence that can disagree with it.
                "traces": {
                    "prominence": "detail",
                    "plannedSlots": total,
                    "streamsOpened": streams_opened,
                    "receiptsRetained": retained_traces.len(),
                    "sealed": sealed_traces,
                    "evidenceGaps": total.saturating_sub(retained_traces.len()),
                    "items": retained_traces
                },
                "reconciliationErrors": reconciliation_errors,
                "aggregate": progress_projection
                    .and_then(|projection| projection.get("aggregate"))
                    .cloned()
                    .unwrap_or(Value::Null),
                "arms": [{
                    "id": "baseline",
                    "label": spec.policy_config,
                    "baseline": true,
                    "status": status,
                    "score": mean_reward
                }],
                "records": records,
                "limitations": limitations
            }
        }, {
            "input": "optimizer_run",
            "kind": "optimizer_run",
            "source": optimizer_run_id,
        }]
    })
}

/// One seed's row in the overview, and its way into the workstation.
fn seed_row(
    record: Option<&Value>,
    example: &EvalExample,
    state: Option<&str>,
    workbench_id: &str,
) -> Value {
    let seed = json!(example.seed);
    let reported_facts = record
        .and_then(|record| record.get("reportedFacts"))
        .cloned()
        .unwrap_or(Value::Null);
    let frames = reported_facts
        .pointer("/frames/value")
        .and_then(Value::as_u64);
    let frame_reason = reported_facts
        .pointer("/frames/unavailableReason")
        .and_then(Value::as_str);
    let summary = match frames {
        Some(frames) => format!("{frames} trusted native frame observations retained"),
        None => frame_reason
            .map(|reason| format!("frames unavailable · {reason}"))
            .unwrap_or_else(|| "rollout telemetry has not settled".to_string()),
    };
    json!({
        "id": record.and_then(|record| record.get("rolloutId")).cloned().unwrap_or_else(|| json!(format!("planned:{}", example.seed))),
        "label": match seed { Value::Null => "seed".to_string(), ref value => format!("Seed {value}") },
        "seed": seed,
        "reward": record.and_then(|record| record.get("reward")).cloned().unwrap_or(Value::Null),
        "status": state.map(|value| json!(value)).or_else(|| record.and_then(|record| record.get("status")).cloned()).unwrap_or_else(|| json!("planned")),
        "stopReason": record.and_then(rollout_stop_reason).unwrap_or(Value::Null),
        // `sealedTrace.maxStep` is an archive/frame high-water mark, not the
        // environment's reported execution-step count. If the rollout omitted
        // `steps`, the honest value is unavailable even when its trace happens
        // to contain a maximum frame step.
        "steps": reported_facts.pointer("/steps/value").cloned().unwrap_or(Value::Null),
        "modelCalls": reported_facts.pointer("/calls/value").cloned().unwrap_or(Value::Null),
        "tokens": reported_facts.pointer("/tokens/value").cloned().unwrap_or(Value::Null),
        "costUsd": reported_facts.pointer("/costUsd/value").cloned().unwrap_or(Value::Null),
        "traceId": record.and_then(|record| {
            record
                .pointer("/sealedTrace/traces/0/traceId")
                .or_else(|| record.pointer("/trace/bundle_trace_id"))
                .or_else(|| record.pointer("/trace/trace_id"))
        }).cloned().unwrap_or(Value::Null),
        "achievements": reported_facts
            .pointer("/achievements/value")
            .and_then(Value::as_array)
            .map(Vec::len),
        "reportedFacts": reported_facts,
        "summary": summary,
        "visualId": workbench_id,
    })
}

fn rollout_stop_reason(record: &Value) -> Option<Value> {
    if record.get("status").and_then(Value::as_str) == Some("failed") {
        return Some(
            record
                .get("error")
                .cloned()
                .unwrap_or_else(|| json!("producer reported failed without a reason")),
        );
    }
    record.get("rewardStatus").cloned()
}

fn elapsed_label(started_at: &str) -> String {
    let elapsed = chrono::DateTime::parse_from_rfc3339(started_at)
        .ok()
        .map(|started| {
            chrono::Utc::now().signed_duration_since(started.with_timezone(&chrono::Utc))
        })
        .map(|duration| duration.num_seconds().max(0) as u64)
        .unwrap_or(0);
    if elapsed >= 60 {
        format!("{}m {:02}s", elapsed / 60, elapsed % 60)
    } else {
        format!("{elapsed}s")
    }
}

fn eta_label(elapsed: &str, completed: usize, total: usize) -> String {
    if completed == 0 {
        return "estimating after first completion".to_string();
    }
    if completed >= total {
        return "complete".to_string();
    }
    let seconds = elapsed.split_whitespace().fold(0_u64, |sum, part| {
        if let Some(minutes) = part
            .strip_suffix('m')
            .and_then(|value| value.parse::<u64>().ok())
        {
            sum + minutes * 60
        } else if let Some(seconds) = part
            .strip_suffix('s')
            .and_then(|value| value.parse::<u64>().ok())
        {
            sum + seconds
        } else {
            sum
        }
    });
    let remaining = seconds.saturating_mul((total - completed) as u64) / completed as u64;
    if remaining >= 60 {
        format!("~{}m {:02}s", remaining / 60, remaining % 60)
    } else {
        format!("~{remaining}s")
    }
}

fn mean_for_pool(records: &[Value], pool: &str) -> Option<f64> {
    let values = records
        .iter()
        .filter(|row| row.get("pool").and_then(Value::as_str) == Some(pool))
        .filter(|row| is_successful_eval_record(row))
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
    cancel: super::CancelObserver,
) -> Result<()> {
    let _revoke_capabilities = crate::secrets::RevokeRunOnDrop(run_id.clone());
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
    evidence(
        "initial_progress_projection",
        persist_progress(
            &service,
            &run_id,
            spec,
            &visual_id,
            &workbench_id,
            &[],
            examples.len(),
            "running",
        ),
    )
    .await?;
    // These recipes intentionally use the container's blocking rollout mode.
    // HealthBench can make one policy call plus many rubric-grader calls, so
    // the generic UI HTTP timeout is not an honest bound for this endpoint.
    let client = crate::http::http_client_with_timeout(spec.blocking_http_timeout());
    let info = client
        .get(format!("{}/info", container.base_url))
        .send()
        .await
        .context("refresh container declaration before rollout dispatch")?
        .error_for_status()
        .context("container declaration refresh was not successful")?
        .json::<Value>()
        .await
        .context("decode refreshed container declaration")?;
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
    let mut remaining = examples.into_iter().enumerate();
    let mut tasks: JoinSet<(u32, EvalExample, Result<Value>)> = JoinSet::new();
    let mut records = Vec::new();
    let mut halt = None::<String>;

    loop {
        let cancel_request = cancel.borrow().clone();
        if let Some(request) = cancel_request {
            // Stop dispatching and drain every in-flight trial to quiescence
            // instead of dropping the JoinSet. The children observe the same
            // typed signal, settle their relays, and each gets an explicit
            // `cancelled` trial terminal — never `failed`; interrupted work
            // did not fail. Trials that never dispatched are closed by the
            // kernel's terminal seal.
            while let Some(joined) = tasks.join_next().await {
                let (index, record) = match joined {
                    Ok((index, _example, Ok(record))) => (index, record),
                    Ok((index, example, Err(error))) => (
                        index,
                        settled_child_error_record(example, spec, &policy_pin, error),
                    ),
                    Err(error) => {
                        return Err(error)
                            .context("inline rollout task could not be joined during cancellation");
                    }
                };
                evidence(
                    "trial_terminal",
                    append_eval_terminal(&service, &run_id, spec, index, &record),
                )
                .await?;
                records.push(record);
            }
            append_provider_usage_reconciliation(&service, &run_id)
                .await
                .context("reconcile authoritative provider usage after cancellation")?;
            return Err(super::kernel::CancelledError { request }.into());
        }
        while tasks.len() < permits && halt.is_none() {
            if over_cost_ceiling(&records, spec.cost_ceiling_usd) {
                halt = Some(format!(
                    "cost ceiling ${:.2} reached; remaining rollouts were not dispatched",
                    spec.cost_ceiling_usd
                ));
                break;
            }
            let Some((index, example)) = remaining.next() else {
                break;
            };
            evidence(
                "dispatch_progress_projection",
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
                let result =
                    run_one_example(&trial, index as u32, example, &mut trial_cancel).await;
                (index as u32, example, result)
            });
        }
        let Some(joined) = tasks.join_next().await else {
            break;
        };
        let (index, record) = match joined {
            Ok((index, _example, Ok(record))) => (index, record),
            // The loop can be parked in join_next when the cancel arrives, so
            // an interrupted child can surface here before the loop-top check
            // runs. The downcast keeps its settlement `cancelled`.
            Ok((index, example, Err(error))) => (
                index,
                settled_child_error_record(example, spec, &policy_pin, error),
            ),
            Err(error) => {
                return Err(error).context("inline rollout task could not be joined");
            }
        };
        evidence(
            "trial_terminal",
            append_eval_terminal(&service, &run_id, spec, index, &record),
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

    for (_index, example) in remaining {
        let record = failed_record(
            example,
            spec,
            &policy_pin,
            halt.clone()
                .unwrap_or_else(|| "required rollout was not dispatched".into()),
        );
        records.push(record);
    }

    // Every child has settled and no provider call can still debit this run's
    // capabilities. Reconcile the durable proxy receipt before the final
    // mutable projection and before the terminal manifest freezes usage.
    // A receipt contradiction is a usage-contract failure, not a missing
    // evidence attachment. Retain it through the final summary projection so
    // every planned rollout (including undispatched cost-ceiling rows) remains
    // visible, then settle the run failed with the contradiction in its detail.
    // Returning here used to lose those rows; wrapping it as EvidenceLaneFailure
    // incorrectly settled the same cost-ceiling run as retryable `degraded`.
    let provider_usage_failure = append_provider_usage_reconciliation(&service, &run_id)
        .await
        .err()
        .map(|error| format!("provider usage reconciliation failed: {error:#}"));

    let failed = records
        .iter()
        .filter(|row| !is_successful_eval_record(row))
        .count();
    let evaluator_failures = records
        .iter()
        .filter(|row| {
            row.pointer("/evaluatorOutcome/status")
                .and_then(Value::as_str)
                == Some("failed")
        })
        .count();
    let missing_measurements = records
        .iter()
        .filter(|row| {
            matches!(
                row.pointer("/evaluatorOutcome/reason")
                    .and_then(Value::as_str),
                Some("evaluator_measurement_missing" | "evaluator_numeric_reward_missing")
            )
        })
        .count();
    let budget_exceeded = over_cost_ceiling(&records, spec.cost_ceiling_usd);
    let status = if failed == 0
        && records.len() == total
        && !budget_exceeded
        && provider_usage_failure.is_none()
    {
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
    // This is the final mutable summary/visual projection. `append_terminal`
    // seals the terminal manifest, after which `patch_run` correctly refuses
    // summary rewrites. Projecting once more after the seal used to turn a
    // successful worker return into a failure path (and repaint its saved
    // visual as failed).
    evidence(
        "selection",
        append_eval_selection(&service, &run_id, status, mean_reward(&records)),
    )
    .await?;
    let mut detail = String::new();
    if status != "completed" {
        let mut parts = Vec::new();
        if failed != 0 || records.len() != total {
            parts.push(format!("{failed} of {total} required rollouts failed"));
        }
        if missing_measurements != 0 {
            parts.push(format!(
                "evaluator_measurement_missing: {missing_measurements} container-completed rollouts supplied no evaluator measurement required by the recipe"
            ));
        }
        if evaluator_failures > missing_measurements {
            parts.push(format!(
                "evaluator_failure: {} rollouts could not obtain a valid evaluator result",
                evaluator_failures - missing_measurements
            ));
        }
        if budget_exceeded {
            parts.push(format!("cost exceeded ${:.2}", spec.cost_ceiling_usd));
        }
        if let Some(halt) = halt {
            parts.push(halt);
        }
        if let Some(provider_usage_failure) = provider_usage_failure {
            parts.push(provider_usage_failure);
        }
        detail = parts.join("; ");
    }
    evidence(
        "run_terminal",
        append_terminal(&service, &run_id, status, detail),
    )
    .await?;
    evidence(
        "terminal_visual_projection",
        publish_terminal_visual_projection(
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
    Ok(())
}

async fn append_provider_usage_reconciliation(
    service: &OptimizerService,
    run_id: &str,
) -> Result<()> {
    let Some(secrets) = crate::secrets::live() else {
        return Ok(());
    };
    let Some(receipt) = secrets.provider_usage_receipt(run_id)? else {
        return Ok(());
    };
    append_provider_usage_receipt(service, run_id, receipt).await
}

async fn append_provider_usage_receipt(
    service: &OptimizerService,
    run_id: &str,
    receipt: crate::secrets::ProviderUsageReceipt,
) -> Result<()> {
    if receipt.run_id != run_id {
        bail!(
            "provider_usage_reconciliation_conflict: receipt run {} does not match {run_id}",
            receipt.run_id,
        );
    }
    if let Some(existing) = service
        .events_after(run_id.to_string(), 0, Some(5_000))
        .await?
        .into_iter()
        .find(|event| event.event_type == "optimizer.usage.reconciled")
    {
        let existing_digest = existing
            .item
            .as_ref()
            .and_then(|item| item.get("receiptDigest"))
            .and_then(Value::as_str);
        if existing_digest == Some(receipt.digest.as_str()) {
            return Ok(());
        }
        bail!(
            "provider_usage_reconciliation_conflict: run {run_id} already has receipt {existing_digest:?}, refusing {}",
            receipt.digest,
        );
    }
    let current = service.get(run_id.to_string()).await?.usage;
    if current.calls > receipt.calls
        || current.prompt_tokens > receipt.input_tokens
        || current.completion_tokens > receipt.output_tokens
    {
        bail!(
            "provider_usage_reconciliation_conflict: receipt totals for {run_id} are below committed provider usage (calls {} < {}, prompt {} < {}, completion {} < {})",
            receipt.calls,
            current.calls,
            receipt.input_tokens,
            current.prompt_tokens,
            receipt.output_tokens,
            current.completion_tokens,
        );
    }
    if let (Some(receipt_cost), Some(committed_cost)) = (receipt.cost_usd, current.cost_usd) {
        if receipt_cost + f64::EPSILON < committed_cost {
            bail!(
                "provider_usage_reconciliation_conflict: receipt cost ${receipt_cost:.6} is below committed cost ${committed_cost:.6}"
            );
        }
    }

    let cost_delta = match (receipt.cost_usd, current.cost_usd) {
        (Some(receipt_cost), Some(committed_cost)) => json!(receipt_cost - committed_cost),
        (Some(receipt_cost), None) => json!(receipt_cost),
        (None, _) => Value::Null,
    };
    let item = json!({
        "schemaVersion": receipt.schema_version,
        "receiptDigest": receipt.digest,
        "authority": "workshop.secrets_proxy",
        "calls": receipt.calls,
        "promptTokens": receipt.input_tokens,
        "completionTokens": receipt.output_tokens,
        "costUsd": receipt.cost_usd,
        "capabilities": receipt.capabilities,
    });
    let usage_delta = Map::from_iter([
        ("calls".into(), json!(receipt.calls.saturating_sub(current.calls))),
        (
            "prompt_tokens".into(),
            json!(receipt.input_tokens - current.prompt_tokens),
        ),
        (
            "completion_tokens".into(),
            json!(receipt.output_tokens - current.completion_tokens),
        ),
        ("cost_usd".into(), cost_delta),
        ("usage_completeness".into(), json!("reconciled")),
    ]);
    service
        .append_event_payloads(
            run_id.to_string(),
            vec![OptimizerEventDraft::new("optimizer.usage.reconciled", EVAL_ALGORITHM_ID)
                .idempotency_key("eval:usage:provider:reconciled")
                .item(item.clone())
                .usage_delta(usage_delta)
                .raw(json!({
                    "source": "workshop_secrets_proxy",
                    "receiptDigest": item["receiptDigest"],
                }))],
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
                    "workItemIds".into(),
                    json!((0..examples.len())
                        .map(|index| format!("eval:trial:{index}"))
                        .collect::<Vec<_>>()),
                ),
                (
                    "candidates".into(),
                    json!([{"id": spec.policy_config, "label": spec.policy_config}]),
                ),
            ]))
            .raw(json!({ "source": "container_eval" })),
    ];
    for (index, example) in examples.iter().enumerate() {
        let trial_id = format!("trial:{}:{}", spec.family, example.seed);
        drafts.push(
            OptimizerEventDraft::new("eval.trial.queued", EVAL_ALGORITHM_ID)
                .idempotency_key(format!("eval:queued:{trial_id}"))
                .delta(Map::from_iter([
                    ("workItemId".into(), json!(format!("eval:trial:{index}"))),
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
    index: u32,
    record: &Value,
) -> Result<()> {
    let seed = record.get("seed").cloned().unwrap_or(Value::Null);
    let id = format!("eval:trial:{index}");
    let cancelled = record.get("status").and_then(Value::as_str) == Some("cancelled");
    let valid = is_successful_eval_record(record);
    let metrics = record
        .get("metrics")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(
            || json!({ "reward": record.get("reward").cloned().unwrap_or(Value::Null) }),
        );
    let evidence_refs = eval_terminal_evidence_refs(spec, record)?;
    let usage_delta = terminal_usage_reconciliation(record, cancelled, spec.cost_ceiling_usd);
    let evidence_state = record
        .get("evidenceState")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            if cancelled && record.get("lastObservedStep").is_some() {
                "sealed_partial"
            } else if cancelled {
                "aborted"
            } else if valid && !evidence_refs.is_empty() {
                "sealed_complete"
            } else if !evidence_refs.is_empty() {
                "sealed_partial"
            } else {
                "missing"
            }
        });
    let mut draft = OptimizerEventDraft::new("eval.trial.terminal", EVAL_ALGORITHM_ID)
        // One settlement per trial. A retried append of the same trial is the
        // same fact, not a second completion.
        .idempotency_key(format!("eval:terminal:{id}"))
        .item(json!({
            "kind": "trial",
            "id": id,
            "workItemId": id,
            "status": if cancelled {
                "cancelled"
            } else if valid {
                "evaluated"
            } else {
                "failed"
            },
            "cancelled": cancelled,
            "cancellation": record.get("cancellation").cloned().unwrap_or(Value::Null),
            "cancellationReceipt": record.get("cancellationReceipt").cloned().unwrap_or(Value::Null),
            "rolloutId": record.get("rolloutId").cloned().unwrap_or(Value::Null),
            "trialId": record.get("trialId").cloned().unwrap_or(Value::Null),
            "lastObservedStep": record.get("lastObservedStep").or_else(|| record.get("steps")).cloned().unwrap_or(Value::Null),
            "partialSeal": record.get("partialSeal").cloned().unwrap_or(Value::Null),
            "evidenceState": evidence_state,
            "valid": valid,
            "candidateId": spec.policy_config,
            "stage": "screen",
            "seed": seed,
            "scenario": spec.family,
            "reward": record.get("reward").cloned().unwrap_or(Value::Null),
            "metrics": metrics,
            "raw": record,
        }))
        .artifact_refs(evidence_refs)
        .usage_delta(usage_delta)
        .raw(json!({ "source": "container_eval" }));
    if !valid {
        draft = draft.level("warn");
    }
    service
        .append_event_payloads(run_id.to_string(), vec![draft])
        .await?;
    Ok(())
}

/// The stream owns per-call token facts. The terminal owns only the positive
/// reconciliation from the container's aggregate receipt, plus the rollout
/// count. A producer aggregate below already-committed span usage is marked
/// partial rather than subtracting durable usage.
fn terminal_usage_reconciliation(
    record: &Value,
    cancelled: bool,
    cost_ceiling_usd: f64,
) -> Map<String, Value> {
    let measured = usage_from_records(std::slice::from_ref(record), cost_ceiling_usd);
    let reported_blob = record.get("usage").unwrap_or(&Value::Null);
    let reported_tokens = usage_token_pair(reported_blob);
    let relay = record.get("relay").unwrap_or(&Value::Null);
    let span_events = relay
        .pointer("/observedUsage/events")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let span_prompt = relay
        .pointer("/observedUsage/prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let span_completion = relay
        .pointer("/observedUsage/completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let span_cost_complete = relay
        .pointer("/observedUsage/cost_complete")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let span_cost = relay
        .pointer("/observedUsage/cost_usd")
        .and_then(Value::as_f64);

    let (prompt_delta, completion_delta, completeness) = if cancelled {
        (0, 0, "partial")
    } else if span_events > 0 {
        match reported_tokens {
            Some((prompt, completion)) if prompt >= span_prompt && completion >= span_completion => (
                prompt - span_prompt,
                completion - span_completion,
                "reconciled",
            ),
            _ => (0, 0, "partial"),
        }
    } else if let Some((prompt, completion)) = reported_tokens {
        (prompt, completion, "container_reported")
    } else {
        (0, 0, "partial")
    };

    let mut delta = Map::from_iter([
        ("prompt_tokens".into(), json!(prompt_delta)),
        ("completion_tokens".into(), json!(completion_delta)),
        ("rollouts".into(), json!(1)),
        ("usage_completeness".into(), json!(completeness)),
    ]);
    if span_events == 0 {
        if let Some(cost) = measured.cost_usd {
            delta.insert("cost_usd".into(), json!(cost));
        }
    } else if span_cost_complete {
        if let (Some(reported), Some(committed)) = (measured.cost_usd, span_cost) {
            if reported >= committed {
                delta.insert("cost_usd".into(), json!(reported - committed));
            }
        }
    }
    delta
}

fn usage_token_pair(usage: &Value) -> Option<(u64, u64)> {
    if usage.get("policy").is_some() || usage.get("grader").is_some() {
        let mut prompt = 0u64;
        let mut completion = 0u64;
        let mut saw = false;
        for lane in [usage.get("policy"), usage.get("grader")]
            .into_iter()
            .flatten()
        {
            if let Some(value) = u64_field(lane, &["prompt_tokens", "promptTokens"]) {
                prompt = prompt.saturating_add(value);
                saw = true;
            }
            if let Some(value) = u64_field(lane, &["completion_tokens", "completionTokens"]) {
                completion = completion.saturating_add(value);
                saw = true;
            }
        }
        return saw.then_some((prompt, completion));
    }
    let prompt = u64_field(usage, &["prompt_tokens", "promptTokens"]);
    let completion = u64_field(usage, &["completion_tokens", "completionTokens"]);
    (prompt.is_some() || completion.is_some())
        .then_some((prompt.unwrap_or(0), completion.unwrap_or(0)))
}

/// Immutable resources that make one terminal trial evaluable after the
/// container has gone away.
///
/// The evaluator receipt is the exact scored payload retained in the terminal
/// event, addressed by rollout identity and canonical digest. Imported traces
/// are additional evidence when the container actually supplied them; a
/// missing trace is never replaced by a guessed reference.
fn eval_terminal_evidence_refs(spec: &EvalSpec, record: &Value) -> Result<Vec<Value>> {
    let mut refs = Vec::new();
    if let (Some(rollout_id), Some(reward)) = (
        record.get("rolloutId").and_then(Value::as_str),
        record.get("reward").filter(|value| !value.is_null()),
    ) {
        let receipt = json!({
            "evaluationPlanRef": spec.evaluation_plan_ref,
            "rolloutId": rollout_id,
            "reward": reward,
            "rewardStatus": record.get("rewardStatus").cloned().unwrap_or(Value::Null),
        });
        refs.push(json!({
            "kind": "evaluator_result",
            "id": format!("eval:{rollout_id}"),
            "digest": super::admission::digest_of(&receipt)?.to_string(),
        }));
    }
    if let Some(traces) = record
        .pointer("/sealedTrace/traces")
        .and_then(Value::as_array)
    {
        let trace_kind = if record.get("evidenceState").and_then(Value::as_str)
            == Some("sealed_partial")
        {
            "trace_v5_partial"
        } else {
            "trace_v5"
        };
        for trace in traces {
            let Some(id) = trace.get("traceId").and_then(Value::as_str) else {
                continue;
            };
            let digest = trace
                .get("digest")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|digest| valid_sha256_digest(digest))
                .with_context(|| format!("sealed Trace V5 `{id}` omitted its immutable digest"))?;
            refs.push(json!({
                "kind": trace_kind,
                "id": id,
                "digest": digest,
            }));
        }
    }
    Ok(refs)
}

async fn append_eval_selection(
    service: &OptimizerService,
    run_id: &str,
    _status: &str,
    mean: Option<f64>,
) -> Result<()> {
    let run = service.get(run_id.to_string()).await?;
    let selection_status = serde_json::to_value(
        super::kernel::algorithms::eval::EvalSelection::PromotionNotApplicable,
    )?;
    let selection = json!({
        "status": selection_status,
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
    if let Some(policy) = &spec.admitted_use_policy {
        return policy.clone();
    }
    let trials = spec.examples().len() as u64;
    let calls_per_trial = spec.maximum_model_calls_per_rollout.max(1) as u64;
    let total_calls = trials.saturating_mul(calls_per_trial).max(1);
    let input_tokens = spec
        .policy
        .get("context_token_budget")
        .and_then(Value::as_u64)
        .map(|context_tokens| total_calls.saturating_mul(context_tokens));
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
    let output_per_call = answer_tokens.saturating_add(thinking_tokens);
    super::admission::provider_use_policy_from_bounds(
        vec!["chat.completions.create".into()],
        (!spec.model.is_empty()).then(|| spec.model.clone()).into_iter().collect(),
        spec.policy
            .get("reasoning_effort")
            .and_then(Value::as_str)
            .map(|value| vec![value.to_string()])
            .unwrap_or_default(),
        total_calls.min(u32::MAX as u64) as u32,
        (spec.cost_ceiling_usd * 1_000_000.0).round().max(0.0) as u64,
        crate::limits::SECRETS_CAPABILITY_TTL.as_secs(),
        input_tokens,
        (output_per_call > 0)
            .then(|| total_calls.saturating_mul(output_per_call)),
    )
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
        .map_err(|error| {
            if error.to_string().contains("capability_underscoped") {
                error
            } else {
                secrets_proxy_error("secrets_proxy_denied", &error.to_string())
            }
        })?;
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
    if spec.harness == "nanohorizon" {
        return register_nanohorizon_policy_pin(client, base, spec, run_id).await;
    }
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
    if spec.policy_code.is_none() && !spec.requires_credential_advertisement() && advertised {
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

#[derive(Debug, serde::Deserialize)]
struct ContainerPolicyRef {
    namespace: String,
    name: String,
}

#[derive(Debug, serde::Deserialize)]
struct ContainerPolicyState {
    schema_version: String,
    status: String,
    policy_ref: Option<ContainerPolicyRef>,
    policy_revision_id: Option<String>,
    source_revision: Option<String>,
    configuration_digest: Option<String>,
    model_digest: Option<String>,
    credential_state: String,
}

async fn read_container_policy(
    client: &reqwest::Client,
    base: &str,
) -> Result<ContainerPolicyState> {
    let response = client
        .get(format!("{base}/policy"))
        .send()
        .await
        .context("GET /policy")?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        bail!("policy inspection failed: {status} {text}");
    }
    let state = response
        .json::<ContainerPolicyState>()
        .await
        .context("decode synth.container-policy.v1")?;
    if state.schema_version != "synth.container-policy.v1" {
        bail!(
            "unsupported policy schema `{}`; expected synth.container-policy.v1",
            state.schema_version
        );
    }
    if state.credential_state != "not_exposed" {
        bail!("container policy endpoint violated credential non-disclosure contract");
    }
    Ok(state)
}

fn expected_model_digest(spec: &EvalSpec) -> Result<String> {
    Ok(super::admission::CanonicalJson::new(json!({
        "provider": spec.provider,
        "model_id": spec.model,
    }))?
    .digest()
    .as_str()
    .to_string())
}

fn installed_policy_matches(
    state: &ContainerPolicyState,
    spec: &EvalSpec,
    model_digest: &str,
) -> bool {
    state.status == "installed"
        && state
            .policy_revision_id
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        && state.policy_ref.as_ref().is_some_and(|policy| {
            policy.namespace == spec.harness && policy.name == spec.policy_config
        })
        && state.source_revision.as_deref() == Some(spec.policy_source_revision.as_str())
        && state.configuration_digest.as_deref() == Some(spec.policy_configuration_digest.as_str())
        && state.model_digest.as_deref() == Some(model_digest)
}

async fn register_nanohorizon_policy_pin(
    client: &reqwest::Client,
    base: &str,
    spec: &EvalSpec,
    run_id: &str,
) -> Result<Value> {
    let catalog = client
        .get(format!("{base}/task_catalog"))
        .send()
        .await
        .context("GET /task_catalog")?
        .error_for_status()
        .context("container task catalog was not successful")?
        .json::<Value>()
        .await
        .context("decode container task catalog")?;
    if catalog.get("schema_version").and_then(Value::as_str)
        != Some("synth.container.task-catalog.v1")
    {
        bail!("container returned an unsupported task catalog schema");
    }
    let task = client
        .get(format!("{base}/task_info"))
        .send()
        .await
        .context("GET /task_info")?
        .error_for_status()
        .context("container task info was not successful")?
        .json::<Value>()
        .await
        .context("decode container task info")?;
    if task.get("family").and_then(Value::as_str) != Some(spec.family.as_str()) {
        bail!(
            "container task family mismatch: requested {}, declared {:?}",
            spec.family,
            task.get("family")
        );
    }

    // Register only a scoped Workshop proxy route. No provider credential is
    // serialized into either the sampler configuration or policy lifecycle.
    let openai_base = container_openai_proxy_base(run_id, spec)?;
    let body = spec
        .policy_config_body(Some(&openai_base))
        .context("NanoHorizon policy config is missing an immutable config id")?;
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
    if registered
        .get("config_id")
        .or_else(|| registered.get("configId"))
        .and_then(Value::as_str)
        != Some(spec.policy_config.as_str())
    {
        bail!("container registered a different NanoHorizon policy config");
    }

    let model_digest = expected_model_digest(spec)?;
    let mut state = read_container_policy(client, base).await?;
    if !installed_policy_matches(&state, spec, &model_digest) {
        let code = spec.policy_code.as_deref().context(
            "policy_source_unavailable: NanoHorizon requires source bytes from the approved immutable revision",
        )?;
        let response = client
            .put(format!("{base}/policy"))
            .json(&json!({
                "code": code,
                "harness": spec.harness,
                "namespace": spec.harness,
                "name": spec.policy_config,
                "configuration": spec.policy,
                "model": { "provider": spec.provider, "model_id": spec.model },
                "source_revision": spec.policy_source_revision,
            }))
            .send()
            .await
            .context("PUT /policy")?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            bail!("policy provisioning failed: {status} {text}");
        }
        state = read_container_policy(client, base).await?;
    }
    if !installed_policy_matches(&state, spec, &model_digest) {
        bail!("policy_installation_mismatch: container did not report the approved policy pin");
    }
    let revision = state
        .policy_revision_id
        .context("installed policy omitted policy_revision_id")?;
    Ok(json!({
        "harness": spec.harness,
        "config": spec.policy_config,
        "configId": spec.policy_config,
        "policyRevisionId": revision,
        "sourceRevision": spec.policy_source_revision,
        "configurationDigest": spec.policy_configuration_digest,
        "modelDigest": model_digest,
        "immutable": true,
        "authority": "synth.container-policy.v1",
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

fn failed_record(
    example: EvalExample,
    spec: &EvalSpec,
    policy_pin: &Value,
    error: String,
) -> Value {
    let mut record = json!({
        "pool": example.pool,
        "seed": example.seed,
        "taskInstanceId": format!("seed:{}", example.seed),
        "status": "failed",
        "error": error,
        "evaluatorOutcome": {
            "status": "failed",
            "reason": "evaluator_not_reached",
            "detail": error,
            "source": "container_evaluator",
        },
        "evidenceState": "missing",
        "evidenceOutcome": {
            "status": "failed",
            "reason": "trace_not_reached",
            "detail": error,
            "source": "trusted_trace_v5",
        },
        "policyRef": policy_pin,
        "worldRef": spec.world_ref,
    });
    attach_reported_facts(&mut record);
    record
}

/// A rollout that was dispatched has durable identity even when its journal
/// fails integrity validation before evaluator/trace settlement. Keep that
/// identity on the terminal record instead of rebuilding a pre-dispatch-shaped
/// failure in the parent task.
fn dispatched_failure_record(
    example: EvalExample,
    spec: &EvalSpec,
    policy_pin: &Value,
    rollout_id: &str,
    trial_id: &str,
    task_instance_id: &str,
    container_id: &str,
    poll_url: &str,
    relay: &eval_relay::RelayOutcome,
    error: &anyhow::Error,
) -> Value {
    let detail = format!("{error:#}");
    let integrity = error
        .downcast_ref::<eval_relay::RelayIntegrityError>()
        .is_some();
    let evidence_state = if integrity { "rejected" } else { "missing" };
    let mut record = json!({
        "rolloutId": rollout_id,
        "trialId": trial_id,
        "pool": example.pool,
        "seed": example.seed,
        "taskInstanceId": task_instance_id,
        "status": "failed",
        "terminated": true,
        "error": detail,
        "steps": relay.last_relayed_step,
        "lastObservedStep": relay.last_relayed_step,
        "usage": relay.to_json()["observedUsage"].clone(),
        "relay": relay.to_json(),
        "evaluatorOutcome": {
            "status": "failed",
            "reason": "evaluator_not_reached",
            "detail": detail,
            "source": "container_evaluator",
        },
        "evidenceState": evidence_state,
        "evidenceOutcome": {
            "status": "failed",
            "reason": if integrity { "journal_integrity_rejected" } else { "trace_not_reached" },
            "detail": detail,
            "source": "trusted_trace_v5",
        },
        "evidence": {
            "containerId": container_id,
            "eventsUrl": poll_url,
            "journalRejected": integrity,
        },
        "policyRef": policy_pin,
        "worldRef": spec.world_ref,
    });
    attach_reported_facts(&mut record);
    record
}

/// Settle one child's error into its trial record: a typed cancellation
/// becomes a cancelled record carrying its request; anything else failed.
fn settled_child_error_record(
    example: EvalExample,
    spec: &EvalSpec,
    policy_pin: &Value,
    error: anyhow::Error,
) -> Value {
    match error.downcast_ref::<super::kernel::CancelledError>() {
        Some(cancelled) => cancelled_record(
            example,
            spec,
            policy_pin,
            &cancelled.request,
            format!("{error:#}"),
        ),
        None => failed_record(example, spec, policy_pin, format!("{error:#}")),
    }
}

/// A trial that was interrupted, with the request that interrupted it. Not a
/// [`failed_record`]: cancellation is not an application error and must not
/// settle as one.
fn cancelled_record(
    example: EvalExample,
    spec: &EvalSpec,
    policy_pin: &Value,
    request: &std::sync::Arc<super::kernel::CancellationRequest>,
    detail: String,
) -> Value {
    let mut record = json!({
        "pool": example.pool,
        "seed": example.seed,
        "taskInstanceId": format!("seed:{}", example.seed),
        "status": "cancelled",
        "cancellation": request.as_ref(),
        "cancellationReceipt": request.as_ref(),
        "detail": detail,
        "evidenceState": "missing",
        "evaluatorOutcome": {
            "status": "failed",
            "reason": "evaluation_cancelled",
            "detail": detail,
            "source": "container_evaluator",
        },
        "evidenceOutcome": {
            "status": "aborted",
            "reason": "evaluation_cancelled",
            "source": "trusted_trace_v5",
        },
        "policyRef": policy_pin,
        "worldRef": spec.world_ref,
    });
    attach_reported_facts(&mut record);
    record
}

/// Retry only the evidence lane of an existing inline evaluation.
///
/// This never dispatches compute and never mints a credential capability. It
/// re-imports the container's already-sealed Trace V5 bundles, binds their
/// identities into the durable rollout state, and rebuilds both Workshop
/// visuals from the authoritative run record. Every required identity is read
/// from the approved specification or terminal record; none is guessed.
pub(super) async fn reconcile_evidence(
    service: &OptimizerService,
    run_id: &str,
) -> Result<OptimizerRunRecord> {
    let run = service.get(run_id.to_string()).await?;
    if run.algorithm_id != EVAL_ALGORITHM_ID {
        bail!("optimizer run `{run_id}` is not an evaluation run");
    }
    let summary = run
        .summary
        .as_object()
        .context("evaluation run summary is not an object")?;

    let owned_run_id = run_id.to_string();
    let (execution_spec, kernel_state) = service
        .database()
        .clone()
        .run(move |conn| {
            let execution_spec =
                super::admission::load_admitted_execution_spec(conn, &owned_run_id)?
                    .context("evaluation run has no approved execution specification")?;
            let kernel_state = super::kernel::persist::load_state(conn, &owned_run_id)?
                .context("evaluation run has no saved kernel projection")?;
            Ok((execution_spec, kernel_state))
        })
        .await?;
    let terminal = kernel_state
        .terminal
        .as_ref()
        .context("evaluation evidence may be reconciled only after a sealed terminal state")?;
    if execution_spec.source_kind != super::admission::RecipeSourceKind::Inline {
        bail!("evidence reconciliation currently requires an inline evaluation specification");
    }

    let records_value = summary
        .get("records")
        .and_then(Value::as_array)
        .context("evaluation run has no terminal records")?;
    if records_value.len() != execution_spec.recipe.rollout_plan.seeds.len() {
        bail!(
            "terminal record count {} does not match approved rollout count {}",
            records_value.len(),
            execution_spec.recipe.rollout_plan.seeds.len()
        );
    }
    let mut records = records_value.clone();
    let family = summary
        .get("task")
        .and_then(Value::as_str)
        .map(str::to_string)
        .context("evaluation run summary has no task family")?;
    let world_ref = records
        .first()
        .and_then(|record| record.get("worldRef"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .context("evaluation terminal records have no worldRef")?;
    let spec = EvalSpec::from_execution_spec(&execution_spec, family, world_ref)?;
    let container_id = execution_spec
        .recipe
        .container
        .container_id
        .as_str()
        .to_string();

    let approved_seeds = &execution_spec.recipe.rollout_plan.seeds;
    let mut reconciled_indices = std::collections::BTreeSet::new();
    let mut evidence_refs = Vec::new();
    for (record_position, record) in records.iter_mut().enumerate() {
        let seed = record
            .get("seed")
            .and_then(Value::as_i64)
            .with_context(|| format!("terminal record {record_position} has no seed"))?;
        let index = approved_seeds
            .iter()
            .position(|approved| approved.0 == seed)
            .with_context(|| format!("terminal record seed {seed} was not in the approved plan"))?;
        if !reconciled_indices.insert(index) {
            bail!("terminal records contain duplicate seed {seed}");
        }
        let work_item_id = format!("eval:trial:{index}");
        let ledger_identity = kernel_state
            .projection
            .eval_evidence_ledger()
            .and_then(|ledger| ledger.iter().find(|entry| entry.work_item_id == work_item_id));
        let rollout_id = record
            .get("rolloutId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| ledger_identity.and_then(|entry| entry.rollout_id.clone()))
            .with_context(|| {
                format!(
                    "terminal record and saved trial ledger for seed {seed} have no rolloutId"
                )
            })?;
        let trial_id = record
            .get("trialId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| ledger_identity.and_then(|entry| entry.trial_id.clone()))
            .with_context(|| {
                format!(
                    "terminal record and saved trial ledger for seed {seed} have no trialId"
                )
            })?;
        // Repair the weaker summary copy from the append-only kernel ledger so
        // a successful evidence retry also leaves future readers consistent.
        record["rolloutId"] = json!(rollout_id);
        record["trialId"] = json!(trial_id);
        let producer_trace_id = record
            .pointer("/trace/bundle_trace_id")
            .or_else(|| record.pointer("/trace/trace_id"))
            .and_then(Value::as_str)
            .with_context(|| {
                format!("terminal record for rollout `{rollout_id}` names no sealed trace")
            })?;
        // Always reopen the immutable container bundle, even when its trace
        // identity is already indexed. Trace identity and optimizer-run media
        // authority are separate bindings: the old index shortcut returned a
        // trace id without replaying its frame artifacts into this run, which
        // left a completed Craftax trace with only the live step-0 frame.
        let imported = service
            .import_container_trace(&container_id, &rollout_id, run_id, &trial_id)
            .await
            .with_context(|| format!("reconcile sealed trace for rollout `{rollout_id}`"))?;
        let frame_mode = if record.get("evidenceState").and_then(Value::as_str)
            == Some("sealed_partial")
        {
            FrameTraceMode::SealedPartial {
                last_pre_cancellation_step: record
                    .get("lastObservedStep")
                    .or_else(|| record.get("steps"))
                    .and_then(Value::as_u64)
                    .context("partial trace record omitted lastObservedStep")?,
            }
        } else {
            FrameTraceMode::SealedComplete
        };
        verify_complete_native_frame_trace(record, &imported, &rollout_id, frame_mode)?;
        let imported_trace = imported
            .get("traces")
            .and_then(Value::as_array)
            .and_then(|traces| traces.first())
            .context("sealed bundle indexed no trace")?;
        let (trace_ref, trace_digest) = verify_reconciled_trace_identity(
            imported_trace,
            producer_trace_id,
            &rollout_id,
        )?;
        record
            .as_object_mut()
            .with_context(|| format!("terminal record for seed {seed} is not an object"))?
            .insert("sealedTrace".into(), imported);
        attach_reported_facts(record);
        let trace_kind = if matches!(frame_mode, FrameTraceMode::SealedPartial { .. }) {
            "trace_v5_partial"
        } else {
            "trace_v5"
        };
        evidence_refs.push(json!({
            "kind": trace_kind,
            "id": trace_ref,
            "digest": trace_digest,
        }));
    }
    service
        .append_event_payloads(
            run_id.to_string(),
            vec![
                OptimizerEventDraft::new("optimizer.evidence.amended", EVAL_ALGORITHM_ID)
                    .idempotency_key(format!(
                        "eval:evidence-reconcile:{}",
                        terminal.final_sequence
                    ))
                    .delta(Map::from_iter([
                        ("terminalSequence".into(), json!(terminal.final_sequence)),
                        ("reconciledRollouts".into(), json!(evidence_refs.len())),
                    ]))
                    .artifact_refs(evidence_refs)
                    .raw(json!({ "source": "container_eval_reconcile" })),
            ],
        )
        .await?;

    let visual_id = summary
        .get("visualId")
        .and_then(Value::as_str)
        .context("evaluation run has no primary visualId")?;
    let workbench_id = summary
        .get("visualIds")
        .and_then(|visuals| visuals.get("trace_workbench"))
        .and_then(Value::as_str)
        .context("evaluation run has no trace workbench visualId")?;
    // The terminal manifest and summary are immutable. Reconciliation amends
    // evidence through the append-only event above, then republishes only the
    // derived visuals from that authoritative projection.
    publish_terminal_visual_projection(
        service,
        run_id,
        &spec,
        visual_id,
        workbench_id,
        &records,
        records.len(),
        terminal.kind.as_str(),
    )
    .await?;
    service.get(run_id.to_string()).await
}

/// Validate the identity sealed by the producer while returning Workshop's
/// local trace reference. Producer ids (often the rollout id) and local
/// `tracev5_...` row ids are separate namespaces and must never be compared.
fn verify_reconciled_trace_identity(
    imported_trace: &Value,
    declared_producer_trace_id: &str,
    rollout_id: &str,
) -> Result<(String, String)> {
    let trace_ref = imported_trace
        .get("traceId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|identity| !identity.is_empty())
        .with_context(|| format!("sealed bundle for rollout `{rollout_id}` indexed no trace"))?
        .to_string();
    let indexed_producer_trace_id = imported_trace
        .get("producerTraceId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|identity| !identity.is_empty())
        .with_context(|| {
            format!(
                "sealed bundle for rollout `{rollout_id}` indexed no producer trace identity"
            )
        })?;
    if indexed_producer_trace_id != declared_producer_trace_id {
        bail!(
            "trace_identity_mismatch: rollout `{rollout_id}` declared producer trace `{declared_producer_trace_id}` but the immutable bundle declared `{indexed_producer_trace_id}` (Workshop index `{trace_ref}`)"
        );
    }
    let trace_digest = imported_trace
        .get("digest")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|digest| valid_sha256_digest(digest))
        .with_context(|| {
            format!("sealed bundle for rollout `{rollout_id}` indexed a trace without an immutable digest")
        })?
        .to_string();
    Ok((trace_ref, trace_digest))
}

/// Refresh a completed inline evaluation's visual from its authoritative run
/// projection when a newer Workshop build can present previously omitted
/// provider-receipt fields. Reopening a visual must not require replaying or
/// mutating its immutable Trace V5 evidence.
pub(super) async fn refresh_terminal_visual_projection_if_stale(
    service: &OptimizerService,
    run: &OptimizerRunRecord,
) -> Result<bool> {
    if run.algorithm_id != EVAL_ALGORITHM_ID
        || run.summary.pointer("/recipeSourceKind").and_then(Value::as_str) != Some("inline")
        || !matches!(
            run.status.as_str(),
            "completed" | "failed" | "failed_evidence" | "cancelled" | "degraded"
        )
    {
        return Ok(false);
    }
    let Some(cost) = run.usage.cost_usd else {
        return Ok(false);
    };
    let summary = run
        .summary
        .as_object()
        .context("evaluation run summary is not an object")?;
    let visual_id = summary
        .get("visualId")
        .and_then(Value::as_str)
        .context("evaluation run has no primary visualId")?;
    let expected_cost = format!(
        "${cost:.6} / ${:.2}",
        summary
            .get("costCeilingUsd")
            .and_then(Value::as_f64)
            .context("evaluation run has no cost ceiling")?
    );
    let visual = service.visuals().get(visual_id.to_string()).await?;
    if visual
        .bindings
        .pointer("/inputs/0/data/progress/cost")
        .and_then(Value::as_str)
        == Some(expected_cost.as_str())
    {
        return Ok(false);
    }

    let owned_run_id = run.id.clone();
    let execution_spec = service
        .database()
        .clone()
        .run(move |conn| {
            super::admission::load_admitted_execution_spec(conn, &owned_run_id)?
                .context("evaluation run has no approved execution specification")
        })
        .await?;
    let records = summary
        .get("records")
        .and_then(Value::as_array)
        .context("evaluation run has no terminal records")?;
    let family = summary
        .get("task")
        .and_then(Value::as_str)
        .map(str::to_string)
        .context("evaluation run summary has no task family")?;
    let world_ref = records
        .first()
        .and_then(|record| record.get("worldRef"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .context("evaluation terminal records have no worldRef")?;
    let spec = EvalSpec::from_execution_spec(&execution_spec, family, world_ref)?;
    let workbench_id = summary
        .get("visualIds")
        .and_then(|visuals| visuals.get("trace_workbench"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    publish_terminal_visual_projection(
        service,
        &run.id,
        &spec,
        visual_id,
        workbench_id,
        records,
        execution_spec.recipe.rollout_plan.seeds.len(),
        &run.status,
    )
    .await?;
    Ok(true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameTraceMode {
    SealedComplete,
    SealedPartial { last_pre_cancellation_step: u64 },
}

fn verify_complete_native_frame_trace(
    terminal_record: &Value,
    imported: &Value,
    rollout_id: &str,
    mode: FrameTraceMode,
) -> Result<()> {
    let steps = match mode {
        FrameTraceMode::SealedComplete => terminal_record
            .get("steps")
            .and_then(Value::as_u64)
            .with_context(|| {
                format!(
                    "full_trace_step_count_missing: rollout `{rollout_id}` has no terminal environment-step count"
                )
            })?,
        FrameTraceMode::SealedPartial {
            last_pre_cancellation_step,
        } => last_pre_cancellation_step,
    };
    let observed = imported
        .get("importedFrameSteps")
        .and_then(Value::as_array)
        .context(
            "full_trace_frame_steps_missing: imported Trace V5 bundle omitted frame-step coverage",
        )?
        .iter()
        .map(|value| {
            value
                .as_u64()
                .context("full_trace_frame_step_invalid: frame step is not an unsigned integer")
        })
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    let expected = (0..=steps).collect::<std::collections::BTreeSet<_>>();
    if observed != expected {
        let missing = expected.difference(&observed).copied().collect::<Vec<_>>();
        return Err(anyhow::Error::new(
            crate::error::StructuredFailure::new(
                "full_trace_frame_coverage_incomplete",
                format!(
                    "rollout `{rollout_id}` retained {} of {} required native frame steps ({})",
                    observed.len(),
                    expected.len(),
                    if matches!(mode, FrameTraceMode::SealedPartial { .. }) {
                        "pre-cancellation"
                    } else {
                        "complete"
                    }
                ),
                "Re-import the immutable Trace V5 bundle after confirming the container sealed one native PNG for every environment step, including step 0.",
            )
            .retryable(true)
            .with_details(json!({
                "rolloutId": rollout_id,
                "terminalSteps": steps,
                "observedFrameSteps": observed,
                "missingFrameSteps": missing,
            })),
        ));
    }
    Ok(())
}

fn verify_required_sealed_trace(
    terminal_record: &Value,
    imported: &Value,
    rollout_id: &str,
) -> Result<()> {
    anyhow::ensure!(
        imported.get("imported").and_then(Value::as_bool) == Some(true),
        "required_trace_import_failed: rollout `{rollout_id}` did not import a trusted Trace V5 bundle"
    );
    anyhow::ensure!(
        imported.get("sourceKind").and_then(Value::as_str) == Some("container_bundle")
            && imported.get("trusted").and_then(Value::as_bool) == Some(true)
            && imported.get("inspectable").and_then(Value::as_bool) == Some(true),
        "required_trace_not_trusted: rollout `{rollout_id}` did not import an inspectable trusted container bundle"
    );
    imported
        .get("bundleDigest")
        .and_then(Value::as_str)
        .filter(|digest| valid_sha256_digest(digest))
        .with_context(|| {
            format!("required_trace_bundle_digest_missing: rollout `{rollout_id}` omitted its immutable bundle digest")
        })?;
    let trace = imported
        .get("traces")
        .and_then(Value::as_array)
        .and_then(|traces| traces.first())
        .with_context(|| {
            format!("required_trace_identity_missing: rollout `{rollout_id}` indexed no trace")
        })?;
    trace
        .get("traceId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|trace_id| !trace_id.is_empty())
        .with_context(|| {
            format!("required_trace_identity_missing: rollout `{rollout_id}` indexed no trace id")
        })?;
    trace
        .get("digest")
        .and_then(Value::as_str)
        .filter(|digest| valid_sha256_digest(digest))
        .with_context(|| {
            format!("required_trace_digest_missing: rollout `{rollout_id}` omitted its immutable trace digest")
        })?;
    let binding = imported
        .get("provenanceBinding")
        .and_then(Value::as_object)
        .with_context(|| {
            format!("required_trace_provenance_missing: rollout `{rollout_id}` has no admitted-runtime binding")
        })?;
    for digest_key in ["imageDigest", "traceDigest", "bundleDigest"] {
        binding
            .get(digest_key)
            .and_then(Value::as_str)
            .filter(|digest| valid_sha256_digest(digest))
            .with_context(|| {
                format!("required_trace_provenance_missing: rollout `{rollout_id}` has no valid `{digest_key}` binding")
            })?;
    }
    binding
        .get("producerSourceRevision")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|revision| !revision.is_empty())
        .with_context(|| {
            format!("required_trace_provenance_missing: rollout `{rollout_id}` has no producer revision binding")
        })?;
    verify_complete_native_frame_trace(
        terminal_record,
        imported,
        rollout_id,
        FrameTraceMode::SealedComplete,
    )
}

fn is_successful_eval_record(row: &Value) -> bool {
    row.get("status").and_then(Value::as_str) == Some("completed")
        && match row
            .pointer("/evaluatorOutcome/status")
            .and_then(Value::as_str)
        {
            Some("scored") => row
                .pointer("/evaluatorOutcome/reward")
                .and_then(Value::as_f64)
                .is_some_and(f64::is_finite)
                || has_evaluator_measurement(row),
            Some(_) => false,
            None => has_evaluator_measurement(row),
        }
}

fn has_evaluator_measurement(row: &Value) -> bool {
    row.get("reward").is_some_and(|value| !value.is_null())
        || row
            .get("metrics")
            .and_then(Value::as_object)
            .is_some_and(|metrics| metrics.values().any(|value| !value.is_null()))
}

fn over_cost_ceiling(records: &[Value], cost_ceiling_usd: f64) -> bool {
    recorded_cost_usd(records).is_some_and(|cost| cost >= cost_ceiling_usd)
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
    let progress_projection = inline_progress_projection(service, run_id).await?;
    let completed = progress_projection
        .as_ref()
        .and_then(|projection| projection.pointer("/rolloutStateCounts/completed"))
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or_else(|| {
            records
                .iter()
                .filter(|record| is_successful_eval_record(record))
                .count()
        });
    let mean = mean_for_pool(records, "train").or_else(|| mean_reward(records));
    let usage = usage_from_records(records, spec.cost_ceiling_usd);
    let failed_count = records
        .iter()
        .filter(|row| !is_successful_eval_record(row))
        .count();
    let records_value = json!(records);
    let status_value = status.to_string();
    let cost_ceiling_usd = spec.cost_ceiling_usd;
    let provider = spec.provider.clone();
    let model = spec.model.clone();
    let run_before_patch = service.get(run_id.to_string()).await?;
    let started_at = run_before_patch
        .started_at
        .as_deref()
        .unwrap_or(&run_before_patch.created_at)
        .to_string();
    let progress_for_summary = progress_projection.clone();
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
                    "failed": failed_count,
                    "authoritative": progress_for_summary,
                }),
            );
            if let Some(mean) = mean {
                summary.insert("meanReward".into(), json!(mean));
            }
            summary.insert("evalStatus".into(), json!(status_value));
            summary.insert("costCeilingUsd".into(), json!(cost_ceiling_usd));
            summary.insert(
                "modelIdentity".into(),
                json!({
                    "provider": provider,
                    "model": model,
                    "authority": "approved_evaluation_spec",
                }),
            );
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
            run.usage = usage_with_authoritative_provider_receipt(usage, &run.usage);
            Ok(())
        })
        .await?;
    let run_after_patch = service.get(run_id.to_string()).await?;

    let visual_status = match status {
        "failed" | "failed_evidence" => VisualStatus::Failed,
        "completed" | "cancelled" | "degraded" => VisualStatus::Saved,
        _ => VisualStatus::Live,
    };
    let visual_update = service
        .visuals()
        .update(
            visual_id.to_string(),
            VisualUpdateRequest {
                title: None,
                bindings: Some(experiment_bindings(
                    spec,
                    run_id,
                    status,
                    completed,
                    total,
                    records,
                    mean,
                    workbench_id,
                    progress_projection.as_ref(),
                    &started_at,
                    Some(&run_after_patch.usage),
                )),
                status: Some(visual_status.clone()),
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
    if !workbench_id.is_empty() {
        let (_, event) = service
            .visuals()
            .update(
                workbench_id.to_string(),
                VisualUpdateRequest {
                    title: None,
                    bindings: None,
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
            .await?;
        service.publish_visual_event(event)?;
    }
    Ok(())
}

/// Publish the terminal kernel aggregate after its seal without rewriting the
/// immutable optimizer record. This second visual revision is intentional:
/// the pre-seal projection cannot carry the terminal sequence/revision that
/// chat and the workbench read from V2.
#[allow(clippy::too_many_arguments)]
async fn publish_terminal_visual_projection(
    service: &OptimizerService,
    run_id: &str,
    spec: &EvalSpec,
    visual_id: &str,
    workbench_id: &str,
    records: &[Value],
    total: usize,
    status: &str,
) -> Result<()> {
    let progress_projection = inline_progress_projection(service, run_id)
        .await?
        .context("terminal eval projection is unavailable after seal")?;
    let completed = progress_projection
        .pointer("/rolloutStateCounts/completed")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let mean = progress_projection
        .pointer("/aggregate/meanReward")
        .and_then(Value::as_f64);
    let run = service.get(run_id.to_string()).await?;
    let started_at = run.started_at.as_deref().unwrap_or(&run.created_at);
    let visual_status = match status {
        "failed" | "failed_evidence" => VisualStatus::Failed,
        "completed" | "cancelled" | "degraded" => VisualStatus::Saved,
        other => bail!("terminal visual projection received non-terminal status `{other}`"),
    };
    let (_, event) = service
        .visuals()
        .update(
            visual_id.to_string(),
            VisualUpdateRequest {
                title: None,
                bindings: Some(experiment_bindings(
                    spec,
                    run_id,
                    status,
                    completed,
                    total,
                    records,
                    mean,
                    workbench_id,
                    Some(&progress_projection),
                    started_at,
                    Some(&run.usage),
                )),
                status: Some(visual_status.clone()),
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
    if !workbench_id.is_empty() {
        let (_, event) = service
            .visuals()
            .update(
                workbench_id.to_string(),
                VisualUpdateRequest {
                    title: None,
                    bindings: None,
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
            .await?;
        service.publish_visual_event(event)?;
    }
    Ok(())
}

async fn inline_progress_projection(
    service: &OptimizerService,
    run_id: &str,
) -> Result<Option<Value>> {
    let run_id = run_id.to_string();
    service
        .database()
        .clone()
        .run(move |conn| {
            let Some(state) = super::kernel::persist::load_state(conn, &run_id)? else {
                return Ok(None);
            };
            let super::kernel::AlgorithmProjection::Eval(eval) = &state.projection else {
                bail!("optimizer run `{run_id}` does not have an eval projection");
            };
            let work = state.work_summary();
            let aggregate = match super::kernel::project_view(&state) {
                super::kernel::OptimizerRunViewV2::Eval(view) => {
                    serde_json::to_value(view.aggregate)?
                }
                _ => bail!("optimizer run `{run_id}` did not project an eval view"),
            };
            let rollouts = eval
                .work_items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let item_state = item
                        .terminal
                        .map(|terminal| terminal.as_str())
                        .unwrap_or_else(|| item.lifecycle.as_str());
                    (
                        index.to_string(),
                        json!({
                            "state": item_state,
                            "workItemId": item.work_item_id,
                            "externalRef": item.external_ref,
                        }),
                    )
                })
                .collect::<Map<String, Value>>();
            Ok(Some(json!({
                "schemaVersion": super::kernel::RUN_VIEW_SCHEMA_VERSION,
                "asOfSequence": state.aggregate_sequence,
                "projectionRevision": state.projection_revision,
                "runState": state.lifecycle.as_str(),
                "rolloutStateCounts": {
                    "queued": work.queued,
                    "running": work.running,
                    "completed": work.succeeded,
                    "failed": work.failed,
                    "cancelled": work.cancelled,
                },
                "inFlight": work.running,
                "evidence": state.evidence_state(),
                "aggregate": aggregate,
                "rollouts": rollouts,
            })))
        })
        .await
}

fn usage_from_records(
    records: &[Value],
    cost_ceiling_usd: f64,
) -> super::models::OptimizerUsageSummary {
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
        .insert("costCeilingUsd".into(), json!(cost_ceiling_usd));
    usage.extra.insert("policyUsage".into(), policy.to_json());
    usage.extra.insert("graderUsage".into(), grader.to_json());
    usage
}

/// Keep the proxy's settled provider receipt canonical across later mutable
/// projections. Per-rollout records describe producer/runtime telemetry and
/// may legitimately have fewer tokens or no cost; they must not overwrite the
/// provider authority that was reconciled immediately before settlement.
fn usage_with_authoritative_provider_receipt(
    mut measured: super::models::OptimizerUsageSummary,
    current: &super::models::OptimizerUsageSummary,
) -> super::models::OptimizerUsageSummary {
    for (key, value) in &current.extra {
        measured
            .extra
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
    let receipt = current
        .extra
        .get("providerUsageReceipt")
        .and_then(Value::as_object);
    if receipt
        .and_then(|receipt| receipt.get("authority"))
        .and_then(Value::as_str)
        != Some("workshop.secrets_proxy")
    {
        return measured;
    }
    if let Some(calls) = receipt
        .and_then(|receipt| receipt.get("calls"))
        .and_then(Value::as_u64)
    {
        measured.calls = calls;
    }
    if let Some(tokens) = receipt
        .and_then(|receipt| receipt.get("promptTokens"))
        .and_then(Value::as_u64)
    {
        measured.prompt_tokens = tokens;
    }
    if let Some(tokens) = receipt
        .and_then(|receipt| receipt.get("completionTokens"))
        .and_then(Value::as_u64)
    {
        measured.completion_tokens = tokens;
    }
    measured.cost_usd = receipt
        .and_then(|receipt| receipt.get("costUsd"))
        .and_then(Value::as_f64);
    measured
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
        .filter(|row| is_successful_eval_record(row))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RolloutReportedStatus {
    Queued,
    Starting,
    Running,
    Completed,
    Failed,
    Cancelled,
    Truncated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EvaluatorFailureReason {
    RewardEndpointFailed,
    MeasurementMissing,
    NumericRewardMissing,
}

/// Authority for one independently reported rollout fact. An unavailable fact
/// still names the authority that was asked; absence is not a synthetic zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReportedFactSource {
    ContainerRuntime,
    RetainedEventLog,
    TrustedTraceV5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReportedFactUnavailableReason {
    CallsNotReported,
    StepsNotReported,
    TokensNotReported,
    CostNotReported,
    AchievementsNotReported,
    FramesNotRetained,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportedFact {
    value: Value,
    source: ReportedFactSource,
    unavailable_reason: Option<ReportedFactUnavailableReason>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RolloutReportedFacts {
    calls: ReportedFact,
    steps: ReportedFact,
    tokens: ReportedFact,
    cost_usd: ReportedFact,
    achievements: ReportedFact,
    frames: ReportedFact,
}

fn reported_fact(
    value: Option<Value>,
    source: ReportedFactSource,
    unavailable_reason: ReportedFactUnavailableReason,
) -> ReportedFact {
    let value = value.filter(|value| !value.is_null());
    ReportedFact {
        unavailable_reason: value.is_none().then_some(unavailable_reason),
        value: value.unwrap_or(Value::Null),
        source,
    }
}

fn usage_lanes(usage: &Value) -> Option<Vec<&Value>> {
    if usage.get("policy").is_some() || usage.get("grader").is_some() {
        Some(
            [usage.get("policy"), usage.get("grader")]
                .into_iter()
                .flatten()
                .filter(|lane| lane.is_object())
                .collect(),
        )
    } else {
        usage.is_object().then(|| vec![usage])
    }
}

fn lane_calls(lane: &Value) -> Option<u64> {
    u64_field(lane, &["calls", "model_calls", "modelCalls"])
}

fn lane_tokens(lane: &Value) -> Option<u64> {
    if let Some(total) = u64_field(lane, &["total_tokens", "totalTokens"]) {
        return Some(total);
    }
    let prompt = u64_field(lane, &["prompt_tokens", "promptTokens"])?;
    let completion = u64_field(lane, &["completion_tokens", "completionTokens"])?;
    Some(prompt.saturating_add(completion))
}

fn complete_usage_u64_sum(
    usage: &Value,
    read: impl Fn(&Value) -> Option<u64>,
) -> Option<u64> {
    let lanes = usage_lanes(usage)?;
    if lanes.is_empty() {
        return None;
    }
    lanes
        .into_iter()
        .try_fold(0_u64, |sum, lane| read(lane).and_then(|value| sum.checked_add(value)))
}

fn complete_usage_cost_sum(usage: &Value) -> Option<f64> {
    let lanes = usage_lanes(usage)?;
    if lanes.is_empty() {
        return None;
    }
    lanes.into_iter().try_fold(0.0, |sum, lane| {
        lane_cost_usd(lane).map(|value| sum + value)
    })
}

fn rollout_reported_facts(record: &Value) -> RolloutReportedFacts {
    let usage = record.get("usage").unwrap_or(&Value::Null);
    let calls = complete_usage_u64_sum(usage, lane_calls).map(|value| json!(value));
    let tokens = complete_usage_u64_sum(usage, lane_tokens).map(|value| json!(value));
    let cost = complete_usage_cost_sum(usage)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| json!(value));
    let steps = record
        .get("steps")
        .and_then(Value::as_u64)
        .map(|value| json!(value));
    let steps_source = if record.get("stepsSource").and_then(Value::as_str)
        == Some("retained_event_log")
    {
        ReportedFactSource::RetainedEventLog
    } else {
        ReportedFactSource::ContainerRuntime
    };
    let achievements = record
        .get("checkpointAchievements")
        .filter(|value| value.is_array())
        .cloned();
    let frames = record
        .pointer("/sealedTrace/importedFrameSteps")
        .and_then(Value::as_array)
        .and_then(|steps| {
            steps
                .iter()
                .map(Value::as_u64)
                .collect::<Option<std::collections::BTreeSet<_>>>()
        })
        .map(|steps| json!(steps.len()));
    RolloutReportedFacts {
        calls: reported_fact(
            calls,
            ReportedFactSource::ContainerRuntime,
            ReportedFactUnavailableReason::CallsNotReported,
        ),
        steps: reported_fact(
            steps,
            steps_source,
            ReportedFactUnavailableReason::StepsNotReported,
        ),
        tokens: reported_fact(
            tokens,
            ReportedFactSource::ContainerRuntime,
            ReportedFactUnavailableReason::TokensNotReported,
        ),
        cost_usd: reported_fact(
            cost,
            ReportedFactSource::ContainerRuntime,
            ReportedFactUnavailableReason::CostNotReported,
        ),
        achievements: reported_fact(
            achievements,
            ReportedFactSource::RetainedEventLog,
            ReportedFactUnavailableReason::AchievementsNotReported,
        ),
        frames: reported_fact(
            frames,
            ReportedFactSource::TrustedTraceV5,
            ReportedFactUnavailableReason::FramesNotRetained,
        ),
    }
}

fn attach_reported_facts(record: &mut Value) {
    record["reportedFacts"] = serde_json::to_value(rollout_reported_facts(record))
        .expect("rollout reported facts serialize");
}

impl EvaluatorFailureReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::RewardEndpointFailed => "evaluator_reward_endpoint_failed",
            Self::MeasurementMissing => "evaluator_measurement_missing",
            Self::NumericRewardMissing => "evaluator_numeric_reward_missing",
        }
    }
}

impl RolloutReportedStatus {
    fn parse(state: &Value) -> Result<Self> {
        match state.get("status").and_then(Value::as_str) {
            Some("queued") => Ok(Self::Queued),
            Some("starting") => Ok(Self::Starting),
            Some("running") => Ok(Self::Running),
            Some("completed") => Ok(Self::Completed),
            Some("failed") => Ok(Self::Failed),
            Some("cancelled") => Ok(Self::Cancelled),
            Some("truncated") => Ok(Self::Truncated),
            Some(other) => bail!("container reported unknown rollout status `{other}`"),
            None => bail!("container rollout state omitted required status"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Truncated => "truncated",
        }
    }

    fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Truncated
        )
    }
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
    work_index: u32,
    example: EvalExample,
    cancel: &mut super::CancelObserver,
) -> Result<Value> {
    let spec = ctx.spec;
    let policy_revision_id = ctx
        .policy_pin
        .get("policyRevisionId")
        .and_then(Value::as_str);
    if spec.harness == "nanohorizon" && policy_revision_id.is_none() {
        bail!("policy_revision_unbound: refusing NanoHorizon rollout before prepare");
    }
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
    let task_instance_id = format!("{}:seed:{}", spec.family, example.seed);
    let trial_id = format!("trial:{}:{}", spec.family, example.seed);
    let work_item_id = format!("eval:trial:{work_index}");
    let prepare = ctx
        .client
        .post(format!("{}/rollouts/prepare", ctx.base))
        .json(&json!({
            "rollout_id": rollout_id,
            "task_instance_id": task_instance_id,
            "telemetry": telemetry
        }))
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
        &work_item_id,
        &trial_id,
        &rollout_id,
        example.seed,
        example.pool,
        &spec.family,
        &spec.policy_config,
    )
    .await?;

    let mut start_body = json!({
        "rollout_id": rollout_id,
        "submission_mode": "sync",
        "slot": "stream",
        "telemetry": telemetry,
        "task_instance_id": task_instance_id,
        "world_ref": spec.world_ref,
        "evaluation_plan_ref": spec.evaluation_plan_ref,
        "policy_ref": { "harness": spec.harness, "config": spec.policy_config },
        "max_steps": spec.maximum_steps_per_rollout,
        "max_calls": spec.maximum_model_calls_per_rollout,
    });
    if let Some(revision) = policy_revision_id {
        start_body
            .as_object_mut()
            .expect("rollout start body is an object")
            .insert("policy_revision_id".into(), json!(revision));
    }
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
    let mut state = match started {
        Ok(state) => state,
        Err(error) => {
            let Some(cancelled) = error.downcast_ref::<super::kernel::CancelledError>() else {
                return Ok(dispatched_failure_record(
                    example,
                    spec,
                    ctx.policy_pin,
                    &rollout_id,
                    &trial_id,
                    &task_instance_id,
                    ctx.container_id,
                    &poll_url,
                    &relay,
                    &error,
                ));
            };
            // Dropping the blocking request asks the container to seal. Import
            // immediately while that container still exists; failure remains
            // explicit partial evidence rather than erasing rollout identity.
            let partial_seal = import_sealed_trace(ctx, &rollout_id, &trial_id).await;
            let evidence_state = if relay.last_relayed_step.is_some()
                || partial_seal.get("imported").and_then(Value::as_bool) == Some(true)
                || partial_seal.get("traces").is_some()
            {
                "sealed_partial"
            } else {
                "aborted"
            };
            let mut record = json!({
                "rolloutId": rollout_id,
                "trialId": trial_id,
                "pool": example.pool,
                "seed": example.seed,
                "taskInstanceId": task_instance_id,
                "status": "cancelled",
                "terminated": true,
                "cancellation": cancelled.request.as_ref(),
                "cancellationReceipt": cancelled.request.as_ref(),
                "steps": relay.last_relayed_step,
                "lastObservedStep": relay.last_relayed_step,
                "usage": relay.to_json()["observedUsage"].clone(),
                "policyRef": ctx.policy_pin,
                "worldRef": spec.world_ref,
                "relay": relay.to_json(),
                "partialSeal": partial_seal.clone(),
                "sealedTrace": partial_seal,
                "evidenceState": evidence_state,
                "evaluatorOutcome": {
                    "status": "failed",
                    "reason": "evaluation_cancelled",
                    "detail": "rollout was cancelled before evaluator settlement",
                    "source": "container_evaluator",
                },
                "evidenceOutcome": {
                    "status": evidence_state,
                    "reason": if evidence_state == "sealed_partial" { "evaluation_cancelled" } else { "trace_not_sealed" },
                    "source": "trusted_trace_v5",
                },
                "evidence": {
                    "containerId": ctx.container_id,
                    "eventsUrl": poll_url,
                    "abortedByCancellation": true,
                }
            });
            attach_reported_facts(&mut record);
            return Ok(record);
        }
    };

    if !rollout_terminal(&state)? {
        state = poll_until_terminal(ctx.client, ctx.base, &rollout_id).await?;
    }
    let reported_status = RolloutReportedStatus::parse(&state)?;
    if !reported_status.is_terminal() {
        bail!(
            "container returned non-terminal rollout status `{}` after terminal polling",
            reported_status.as_str()
        );
    }
    let mut terminal_error = if matches!(
        reported_status,
        RolloutReportedStatus::Failed | RolloutReportedStatus::Truncated
    ) {
        Some(
            state
                .get("error")
                .or_else(|| state.get("reason"))
                .or_else(|| state.get("detail"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    "producer_terminal_failure_missing_reason: container reported a failed terminal state without error, reason, or detail"
                        .to_string()
                }),
        )
    } else {
        None
    };
    let (reward, mut evaluator_failure) = match fetch_reward(
        ctx.client,
        ctx.base,
        &rollout_id,
        spec.evaluation_plan_ref.as_str(),
    )
    .await
    {
        Ok(reward) => (reward, None),
        Err(error) => (
            json!({
                "status": "unavailable",
                "reward": Value::Null,
                "metrics": Value::Null,
            }),
            Some((
                EvaluatorFailureReason::RewardEndpointFailed,
                format!("{error:#}"),
            )),
        ),
    };
    let (terminal_steps, terminal_steps_source) = terminal_step_count(&state, &relay, &rollout_id)?;
    let reward_value = reward.get("reward").cloned().unwrap_or(Value::Null);
    let metrics = reward
        .get("metrics")
        .or_else(|| reward.get("reward_details"))
        .cloned()
        .unwrap_or(Value::Null);
    let reward_status = reward
        .get("status")
        .cloned()
        .unwrap_or_else(|| json!("absent"));
    let measurement = json!({
        "reward": reward_value,
        "metrics": metrics,
    });
    if evaluator_failure.is_none() {
        evaluator_failure = if spec.harness == "nanohorizon" && reward_value.as_f64().is_none() {
            Some((
                EvaluatorFailureReason::NumericRewardMissing,
                format!(
                    "container reported the rollout completed, but NanoHorizon returned no numeric reward (reward status `{}`)",
                    reward_status.as_str().unwrap_or("unknown")
                ),
            ))
        } else if !has_evaluator_measurement(&measurement) {
            Some((
                EvaluatorFailureReason::MeasurementMissing,
                format!(
                    "container reported the rollout completed, but the evaluator returned no reward or non-null metric (reward status `{}`)",
                    reward_status.as_str().unwrap_or("unknown")
                ),
            ))
        } else {
            None
        };
    }
    let evaluator_outcome = if let Some((reason, detail)) = &evaluator_failure {
        if reported_status == RolloutReportedStatus::Completed {
            terminal_error = Some(format!("{}: {detail}", reason.as_str()));
        }
        json!({
            "status": "failed",
            "reason": reason.as_str(),
            "detail": detail,
            "source": "container_evaluator",
        })
    } else {
        json!({
            "status": "scored",
            "reward": reward_value,
            "metrics": metrics,
            "source": "container_evaluator",
        })
    };
    let record_status =
        if reported_status == RolloutReportedStatus::Completed && evaluator_failure.is_some() {
            "failed"
        } else {
            reported_status.as_str()
        };
    let usage = state.get("usage").cloned().unwrap_or(Value::Null);
    let retained = fetch_retained_rollout_state(ctx.client, &poll_url).await?;
    // Import the sealed bundle now, while the container is still running. A
    // replay that only works until the container stops is not a replay, and
    // Workshop already owns the import — the eval worker simply never called it.
    let sealed = import_sealed_trace(ctx, &rollout_id, &trial_id).await;
    let mut record = json!({
        "rolloutId": rollout_id,
        "trialId": trial_id,
        "pool": example.pool,
        "seed": example.seed,
        "taskInstanceId": format!("seed:{}", example.seed),
        "status": record_status,
        "reportedStatus": reported_status.as_str(),
        "error": terminal_error,
        "terminated": true,
        "reward": measurement["reward"],
        "rewardStatus": reward_status,
        "evaluatorOutcome": evaluator_outcome,
        "metrics": measurement["metrics"],
        "usage": usage,
        "steps": terminal_steps,
        "stepsSource": terminal_steps_source,
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
    });
    if spec.harness == "nanohorizon" {
        match verify_required_sealed_trace(&record, &sealed, &rollout_id) {
            Ok(()) => {
                record["evidenceState"] = json!("sealed_complete");
                record["evidenceOutcome"] = json!({
                    "status": "sealed_complete",
                    "source": "trusted_trace_v5",
                });
            }
            Err(error) => {
                let detail = format!("{error:#}");
                let prior = record
                    .get("error")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                record["status"] = json!("failed");
                record["error"] = json!(match prior {
                    Some(prior) => format!("{prior}; required_trace_evidence_failed: {detail}"),
                    None => format!("required_trace_evidence_failed: {detail}"),
                });
                record["evidenceState"] = json!("missing");
                record["evidenceOutcome"] = json!({
                    "status": "failed",
                    "reason": "required_trace_evidence_failed",
                    "detail": detail,
                    "source": "trusted_trace_v5",
                });
            }
        }
    } else {
        let imported = sealed.get("imported").and_then(Value::as_bool) == Some(true);
        record["evidenceOutcome"] = json!({
            "status": if imported { "sealed" } else { "unavailable" },
            "source": "optional_trace_import",
        });
    }
    attach_reported_facts(&mut record);
    Ok(record)
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
    retained_rollout_state(&page)
}

fn retained_rollout_state(page: &Value) -> Result<serde_json::Map<String, Value>> {
    let events = page
        .get("events")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .context("retained rollout event page omitted events")?;
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
            return Ok(retained);
        }
    }
    Ok(serde_json::Map::new())
}

fn rollout_terminal(state: &Value) -> Result<bool> {
    let status = RolloutReportedStatus::parse(state)?;
    let terminated = state.get("terminated").and_then(Value::as_bool);
    if terminated == Some(true) && !status.is_terminal() {
        bail!(
            "container marked rollout terminated while status remained `{}`",
            status.as_str()
        );
    }
    if terminated == Some(false) && status.is_terminal() {
        bail!(
            "container reported terminal rollout status `{}` with terminated=false",
            status.as_str()
        );
    }
    Ok(status.is_terminal())
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
                if rollout_terminal(&state)? {
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
) -> Result<Value> {
    let response = client
        .get(format!("{base}/rollouts/{rollout_id}/reward"))
        .send()
        .await
        .with_context(|| format!("GET reward for rollout `{rollout_id}`"))?
        .error_for_status()
        .with_context(|| format!("reward endpoint failed for rollout `{rollout_id}`"))?;
    let mut body = response
        .json::<Value>()
        .await
        .with_context(|| format!("decode reward for rollout `{rollout_id}`"))?;
    // GET observes an already-materialized evaluator receipt. Terminal
    // scoring is an explicit, idempotent producer operation; NanoHorizon can
    // close its trusted episode journal before that receipt exists. Ask the
    // named evaluation plan to score the closed rollout instead of borrowing
    // policy.session.closed.reward, which is not evaluator authority.
    if body.get("status").and_then(Value::as_str) == Some("absent") {
        body = client
            .post(format!("{base}/reward"))
            .json(&json!({
                "rollout_id": rollout_id,
                "mode": "terminal",
                "rescore": false,
                "evaluation_plan_ref": plan_ref,
            }))
            .send()
            .await
            .with_context(|| format!("POST terminal reward for rollout `{rollout_id}`"))?
            .error_for_status()
            .with_context(|| format!("terminal reward evaluation failed for rollout `{rollout_id}`"))?
            .json::<Value>()
            .await
            .with_context(|| format!("decode terminal reward for rollout `{rollout_id}`"))?;
    }
    validate_reward_record(&body, rollout_id)?;
    Ok(body)
}

fn validate_reward_record(body: &Value, rollout_id: &str) -> Result<()> {
    let status = body
        .get("status")
        .and_then(Value::as_str)
        .with_context(|| format!("reward for rollout `{rollout_id}` omitted status"))?;
    match status {
        "scored" => {
            let reward = body
                .get("reward")
                .and_then(Value::as_f64)
                .with_context(|| format!("scored reward for rollout `{rollout_id}` is missing"))?;
            if !reward.is_finite() {
                bail!("scored reward for rollout `{rollout_id}` is not finite");
            }
        }
        "absent" | "unavailable" => {
            if body.get("reward").is_some_and(|value| !value.is_null()) {
                bail!("unavailable reward for rollout `{rollout_id}` carried a value");
            }
        }
        other => bail!("reward for rollout `{rollout_id}` reported unknown status `{other}`"),
    }
    Ok(())
}

fn terminal_step_count(
    state: &Value,
    relay: &eval_relay::RelayOutcome,
    rollout_id: &str,
) -> Result<(Value, &'static str)> {
    let state_steps = state
        .get("steps")
        .or_else(|| state.get("step_count"))
        .or_else(|| state.get("stepCount"))
        .and_then(Value::as_u64);
    let retained_steps = relay.verified_terminal_steps();
    if let (Some(state_steps), Some(retained_steps)) = (state_steps, retained_steps) {
        if state_steps != retained_steps {
            bail!(
                "terminal_step_count_conflict: rollout `{rollout_id}` reported {state_steps} steps in its terminal response but {retained_steps} in its verified retained lifecycle"
            );
        }
    }
    match (state_steps, retained_steps) {
        (Some(steps), _) => Ok((json!(steps), "container_runtime")),
        (None, Some(steps)) => Ok((json!(steps), "retained_event_log")),
        (None, None) => Ok((Value::Null, "container_runtime")),
    }
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
                        producer_source_revision: container_producer_source_revision(&metadata),
                        metadata: metadata.clone(),
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
    // Registry hydration puts the producer's `/info` document below `info`.
    // Prefer that trusted probe result over registry-root compatibility fields,
    // which may have been supplied by the caller at registration time.
    for pointer in [
        "/info/imageDigest",
        "/info/image_digest",
        "/imageDigest",
        "/image_digest",
        "/digest",
        "/image/digest",
    ] {
        if let Some(digest) = metadata.pointer(pointer).and_then(Value::as_str) {
            let digest = digest.trim();
            if valid_sha256_digest(digest) {
                return Some(digest.to_string());
            }
        }
    }
    None
}

fn container_producer_source_revision(metadata: &Value) -> Option<String> {
    for pointer in [
        "/info/producerSourceRevision",
        "/info/producer_source_revision",
        "/producerSourceRevision",
        "/producer_source_revision",
    ] {
        if let Some(revision) = metadata
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|revision| !revision.is_empty())
        {
            return Some(revision.to_string());
        }
    }
    None
}

fn valid_sha256_digest(digest: &str) -> bool {
    digest
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// Bind the producer facts from the same fresh `/info` response used for the
/// post-approval identity check. An approved eval is the paid path, so missing,
/// malformed, or drifting provenance is refused before the run is created.
fn refresh_inline_container_provenance(container: &mut ReadyContainer, info: &Value) -> Result<()> {
    let image_digest = info
        .get("imageDigest")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|digest| valid_sha256_digest(digest))
        .context("container_image_digest_missing: fresh /info must advertise imageDigest as sha256:<64 lowercase-or-uppercase hex characters>")?
        .to_string();
    let producer_source_revision = info
        .get("producerSourceRevision")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|revision| !revision.is_empty())
        .context("container_producer_source_revision_missing: fresh /info must advertise producerSourceRevision")?
        .to_string();

    if let Some(registered) = container
        .metadata
        .pointer("/info/imageDigest")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|digest| valid_sha256_digest(digest))
    {
        anyhow::ensure!(
            registered == image_digest,
            "container_image_digest_mismatch: registered {registered}, current {image_digest}"
        );
    }
    if let Some(registered) = container
        .metadata
        .pointer("/info/producerSourceRevision")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|revision| !revision.is_empty())
    {
        anyhow::ensure!(
            registered == producer_source_revision,
            "container_producer_source_revision_mismatch: registered {registered}, current {producer_source_revision}"
        );
    }

    container.image_digest = Some(image_digest);
    container.producer_source_revision = Some(producer_source_revision);
    Ok(())
}

/// Verify that a trusted Trace V5 bundle preserved the exact producer facts
/// admitted for the run, then record the immutable cross-boundary binding next
/// to the imported trace reference. The trace bytes stay immutable; Workshop
/// records and verifies their relationship to the approved runtime.
pub(super) fn bind_imported_trace_provenance(
    result: &mut Value,
    run_summary: &Value,
) -> Result<()> {
    if run_summary.get("recipeSourceKind").and_then(Value::as_str) != Some("inline") {
        return Ok(());
    }
    let expected_image = run_summary
        .get("containerImageDigest")
        .and_then(Value::as_str)
        .filter(|digest| valid_sha256_digest(digest))
        .context("inline run omitted its admitted container image digest")?;
    let expected_revision = run_summary
        .get("containerProducerSourceRevision")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|revision| !revision.is_empty())
        .context("inline run omitted its admitted container producer revision")?;
    let provenance = result
        .get("traceProvenance")
        .and_then(Value::as_object)
        .context("trusted Trace V5 bundle omitted producer provenance")?;
    let actual_image = provenance
        .get("container_image_digest")
        .or_else(|| provenance.get("containerImageDigest"))
        .and_then(Value::as_str)
        .context("trusted Trace V5 provenance omitted container_image_digest")?;
    let actual_revision = provenance
        .get("producer_commit")
        .or_else(|| provenance.get("producerSourceRevision"))
        .and_then(Value::as_str)
        .context("trusted Trace V5 provenance omitted producer_commit")?;
    anyhow::ensure!(
        actual_image == expected_image,
        "trace_container_image_digest_mismatch: admitted {expected_image}, trace {actual_image}"
    );
    anyhow::ensure!(
        actual_revision == expected_revision,
        "trace_producer_source_revision_mismatch: admitted {expected_revision}, trace {actual_revision}"
    );
    let trace_digest = result
        .pointer("/traces/0/digest")
        .and_then(Value::as_str)
        .filter(|digest| valid_sha256_digest(digest))
        .context("trusted Trace V5 import omitted its immutable trace digest")?;
    let bundle_digest = result
        .get("bundleDigest")
        .and_then(Value::as_str)
        .filter(|digest| valid_sha256_digest(digest))
        .context("trusted Trace V5 import omitted its immutable bundle digest")?;
    result["provenanceBinding"] = json!({
        "imageDigest": expected_image,
        "producerSourceRevision": expected_revision,
        "traceDigest": trace_digest,
        "bundleDigest": bundle_digest,
    });
    Ok(())
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
    status: &str,
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
    let cause = match status {
        "completed" => super::kernel::SettleCause::Completed,
        "degraded" => super::kernel::SettleCause::Degraded {
            detail: detail.clone(),
        },
        "failed" => super::kernel::SettleCause::Failed {
            detail: detail.clone(),
        },
        "cancelled" => super::kernel::SettleCause::Cancelled {
            request: std::sync::Arc::new(super::kernel::CancellationRequest::new(
                super::kernel::CancellationCause::ContainerRequested,
                "container:terminal",
                format!("run:{run_id}"),
            )),
        },
        other => bail!("unsupported terminal eval status `{other}`"),
    };
    let error = (!detail.trim().is_empty()).then(|| json!({ "message": detail }));
    service.settle_run(run_id.to_string(), cause, error).await?;
    Ok(())
}

/// Seal a cancelled run with the typed request as the terminal event's own
/// payload. The receipt row/table is deferred; the durable journal fact
/// carries request id, cause, requester, time, and scope.
async fn append_cancelled_terminal(
    service: &OptimizerService,
    run_id: &str,
    request: &super::kernel::CancellationRequest,
) -> Result<()> {
    if service
        .terminal_manifest(run_id.to_string())
        .await?
        .is_some()
    {
        return Ok(());
    }
    service
        .settle_run(
            run_id.to_string(),
            super::kernel::SettleCause::Cancelled {
                request: std::sync::Arc::new(request.clone()),
            },
            None,
        )
        .await?;
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
    use sha2::Digest;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    fn ready_container_with_metadata(metadata: Value) -> ReadyContainer {
        ReadyContainer {
            id: "ctr_craftax_test".into(),
            base_url: "http://127.0.0.1:1".into(),
            protocol: crate::container_capabilities::LIVE_EVAL_PROTOCOL.into(),
            image_digest: container_image_digest(&metadata),
            producer_source_revision: container_producer_source_revision(&metadata),
            metadata,
        }
    }

    #[test]
    fn hydrated_info_provenance_wins_over_untrusted_registry_root_fields() {
        let metadata = json!({
            "imageDigest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "producerSourceRevision": "caller-supplied",
            "info": {
                "imageDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "producerSourceRevision": "containers@abc123"
            }
        });
        assert_eq!(
            container_image_digest(&metadata).as_deref(),
            Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(
            container_producer_source_revision(&metadata).as_deref(),
            Some("containers@abc123")
        );
    }

    #[test]
    fn approved_eval_provenance_is_fresh_complete_and_drift_checked() {
        let metadata = json!({"info": {
            "imageDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "producerSourceRevision": "containers@abc123"
        }});
        let mut container = ready_container_with_metadata(metadata);
        refresh_inline_container_provenance(&mut container, &json!({
            "imageDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "producerSourceRevision": "containers@abc123"
        }))
        .unwrap();
        assert_eq!(container.producer_source_revision.as_deref(), Some("containers@abc123"));

        let missing = refresh_inline_container_provenance(
            &mut container.clone(),
            &json!({"producerSourceRevision": "containers@abc123"}),
        )
        .unwrap_err();
        assert!(format!("{missing:#}").contains("container_image_digest_missing"));

        let drift = refresh_inline_container_provenance(&mut container, &json!({
            "imageDigest": "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "producerSourceRevision": "containers@abc123"
        }))
        .unwrap_err();
        assert!(format!("{drift:#}").contains("container_image_digest_mismatch"));
    }

    #[test]
    fn trace_evidence_never_records_a_null_digest() {
        let spec = EvalSpec::classify_fixture();
        let missing = eval_terminal_evidence_refs(
            &spec,
            &json!({"sealedTrace": {"traces": [{"traceId": "trace_1"}]}}),
        )
        .unwrap_err();
        assert!(format!("{missing:#}").contains("omitted its immutable digest"));

        let refs = eval_terminal_evidence_refs(
            &spec,
            &json!({"sealedTrace": {"traces": [{
                "traceId": "trace_1",
                "digest": "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
            }]}}),
        )
        .unwrap();
        assert_eq!(
            refs[0]["digest"],
            json!("sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd")
        );
    }

    #[test]
    fn imported_trace_provenance_must_match_the_admitted_runtime() {
        let summary = json!({
            "recipeSourceKind": "inline",
            "containerImageDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "containerProducerSourceRevision": "containers@abc123"
        });
        let mut imported = json!({
            "bundleDigest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "traces": [{
                "traceId": "trace_1",
                "digest": "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
            }],
            "traceProvenance": {
                "container_image_digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "producer_commit": "containers@abc123"
            }
        });
        bind_imported_trace_provenance(&mut imported, &summary).unwrap();
        assert_eq!(
            imported["provenanceBinding"]["bundleDigest"],
            json!("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );
        assert!(imported["provenanceBinding"]
            .as_object()
            .unwrap()
            .values()
            .all(|value| !value.is_null()));

        imported["traceProvenance"]["container_image_digest"] = json!(
            "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
        );
        let mismatch = bind_imported_trace_provenance(&mut imported, &summary).unwrap_err();
        assert!(format!("{mismatch:#}").contains("trace_container_image_digest_mismatch"));
    }

    #[test]
    fn sealed_trace_high_water_does_not_invent_environment_steps() {
        let record = json!({
            "rolloutId": "roll_1",
            "status": "completed",
            "steps": null,
            "sealedTrace": { "maxStep": 47 },
        });
        let row = seed_row(
            Some(&record),
            &EvalExample {
                pool: "train",
                seed: 1,
            },
            Some("completed"),
            "vis_workbench",
        );
        assert_eq!(row["steps"], Value::Null);
    }

    #[test]
    fn completed_lifecycle_requires_an_evaluator_measurement() {
        assert!(!is_successful_eval_record(&json!({
            "status": "completed",
            "reward": null,
            "rewardStatus": "absent",
        })));
        assert!(is_successful_eval_record(&json!({
            "status": "completed",
            "reward": 0.0,
        })));
        assert!(is_successful_eval_record(&json!({
            "status": "completed",
            "reward": null,
            "metrics": { "accuracy": 0.0 },
        })));
        assert!(!is_successful_eval_record(&json!({
            "status": "failed",
            "reward": 1.0,
        })));
    }

    fn provider_usage_receipt(
        run_id: &str,
        calls: u64,
        prompt_tokens: u64,
        completion_tokens: u64,
        cost_usd: Option<f64>,
        digest_byte: char,
    ) -> crate::secrets::ProviderUsageReceipt {
        crate::secrets::ProviderUsageReceipt {
            schema_version: "workshop.provider-usage-receipt.v1".into(),
            run_id: run_id.into(),
            capabilities: vec![crate::secrets::ProviderUsageCapability {
                capability_id: format!("cap_{run_id}"),
                provider: "openai".into(),
                status: "active".into(),
            }],
            calls,
            input_tokens: prompt_tokens,
            output_tokens: completion_tokens,
            cost_usd,
            digest: format!("sha256:{}", digest_byte.to_string().repeat(64)),
        }
    }

    async fn empty_eval_run(service: &OptimizerService, run_id: &str) -> OptimizerRunRecord {
        service
            .create(
                serde_json::from_value(json!({
                    "algorithmId": "eval",
                    "id": run_id,
                    "openVisual": false,
                }))
                .unwrap(),
            )
            .await
            .unwrap()
            .0
    }

    #[tokio::test]
    async fn known_provider_usage_is_authoritative_idempotent_and_pre_terminal() {
        let (svc, _dir, _) = service().await;
        let run = empty_eval_run(&svc, "opt_eval_provider_usage_known").await;
        append_status(&svc, &run.id, "optimizer.run.started", "running")
            .await
            .unwrap();
        let receipt = provider_usage_receipt(&run.id, 3, 90, 12, Some(0.012345), 'a');
        append_provider_usage_receipt(&svc, &run.id, receipt.clone())
            .await
            .unwrap();
        append_provider_usage_receipt(&svc, &run.id, receipt)
            .await
            .unwrap();

        let projected = svc.get(run.id.clone()).await.unwrap();
        assert_eq!(projected.usage.calls, 3);
        assert_eq!(projected.usage.prompt_tokens, 90);
        assert_eq!(projected.usage.completion_tokens, 12);
        assert_eq!(projected.usage.cost_usd, Some(0.012345));
        let view = serde_json::to_value(svc.run_view_v2(run.id.clone()).await.unwrap()).unwrap();
        assert_eq!(view["header"]["usage"]["calls"], json!(3));
        assert_eq!(view["header"]["usage"]["promptTokens"], json!(90));
        assert_eq!(view["header"]["usage"]["completionTokens"], json!(12));
        assert_eq!(view["header"]["usage"]["costUsd"], json!(0.012345));

        append_terminal(&svc, &run.id, "failed", "fixture settlement".into())
            .await
            .unwrap();
        let events = svc.events_after(run.id.clone(), 0, Some(100)).await.unwrap();
        let reconciliations = events
            .iter()
            .filter(|event| event.event_type == "optimizer.usage.reconciled")
            .collect::<Vec<_>>();
        assert_eq!(reconciliations.len(), 1, "receipt replay is idempotent");
        let reconciliation = reconciliations[0];
        let terminal = events
            .iter()
            .find(|event| event.event_type == "optimizer.run.failed")
            .expect("failed terminal event");
        assert!(reconciliation.sequence_number < terminal.sequence_number);
        assert_eq!(reconciliation.item.as_ref().unwrap()["calls"], json!(3));
        assert_eq!(
            reconciliation.item.as_ref().unwrap()["receiptDigest"],
            json!(format!("sha256:{}", "a".repeat(64)))
        );
        assert_eq!(reconciliation.usage_delta.as_ref().unwrap()["calls"], json!(3));
        assert!(reconciliation
            .raw
            .get("requestId")
            .is_none(), "the aggregate receipt must not fabricate request identity");

        let manifest = svc
            .terminal_manifest(run.id)
            .await
            .unwrap()
            .expect("failed run seals a manifest");
        assert_eq!(manifest["usage"]["calls"], json!(3));
        assert_eq!(manifest["usage"]["promptTokens"], json!(90));
        assert_eq!(manifest["usage"]["completionTokens"], json!(12));
        assert_eq!(manifest["usage"]["costUsd"], json!(0.012345));
        assert_eq!(manifest["usage"]["completeness"], json!("reconciled"));
        assert_eq!(
            manifest["usage"]["providerReceipt"]["receiptDigest"],
            json!(format!("sha256:{}", "a".repeat(64)))
        );
    }

    #[test]
    fn final_record_projection_preserves_authoritative_provider_receipt() {
        let measured = usage_from_records(
            &[json!({
                "usage": {"calls": 10, "prompt_tokens": 100_471, "completion_tokens": 3_517}
            })],
            2.45,
        );
        let mut current = crate::optimizers::models::OptimizerUsageSummary {
            calls: 50,
            prompt_tokens: 116_385,
            completion_tokens: 6_217,
            cost_usd: Some(0.016353),
            ..Default::default()
        };
        current.extra.insert(
            "providerUsageReceipt".into(),
            json!({
                "authority": "workshop.secrets_proxy",
                "calls": 50,
                "promptTokens": 116385,
                "completionTokens": 6217,
                "costUsd": 0.016353,
            }),
        );

        let merged = usage_with_authoritative_provider_receipt(measured, &current);
        assert_eq!(merged.calls, 50);
        assert_eq!(merged.prompt_tokens, 116_385);
        assert_eq!(merged.completion_tokens, 6_217);
        assert_eq!(merged.cost_usd, Some(0.016353));
        assert_eq!(merged.rollouts, 1, "runtime rollout count stays distinct");
        assert_eq!(
            merged.extra["policyUsage"]["promptTokens"],
            json!(100_471),
            "producer/runtime telemetry remains available as a separate lane"
        );

        let mut unknown_cost = current;
        unknown_cost.extra.get_mut("providerUsageReceipt").unwrap()["costUsd"] = Value::Null;
        let merged = usage_with_authoritative_provider_receipt(
            usage_from_records(&[json!({"usage": {"cost_usd": 0.004}})], 2.45),
            &unknown_cost,
        );
        assert_eq!(merged.cost_usd, None, "an unpriced provider receipt is not a producer subtotal");
    }

    #[test]
    fn experiment_visual_uses_authoritative_provider_receipt_cost() {
        let spec = EvalSpec::classify_fixture();
        let records = vec![json!({
            "seed": spec.train[0],
            "status": "completed",
            "reward": 1.0,
            "usage": {
                "calls": 10,
                "prompt_tokens": 1000,
                "completion_tokens": 100
            }
        })];
        let mut authoritative = crate::optimizers::models::OptimizerUsageSummary {
            calls: 10,
            prompt_tokens: 1100,
            completion_tokens: 120,
            cost_usd: Some(0.012345),
            ..Default::default()
        };
        authoritative.extra.insert(
            "providerUsageReceipt".into(),
            json!({
                "authority": "workshop.secrets_proxy",
                "calls": 10,
                "promptTokens": 1100,
                "completionTokens": 120,
                "costUsd": 0.012345
            }),
        );

        let bindings = experiment_bindings(
            &spec,
            "opt_eval_provider_visual",
            "completed",
            1,
            1,
            &records,
            Some(1.0),
            "vis_workbench",
            None,
            "2026-08-28T20:00:00Z",
            Some(&authoritative),
        );
        let data = &bindings["inputs"][0]["data"];
        assert_eq!(data["progress"]["cost"], json!("$0.012345 / $0.50"));
        assert!(data["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| !item.as_str().unwrap_or_default().contains("cost telemetry")));
    }

    #[tokio::test]
    async fn unknown_provider_cost_stays_null_while_tokens_and_calls_reconcile() {
        let (svc, _dir, _) = service().await;
        let run = empty_eval_run(&svc, "opt_eval_provider_usage_unknown").await;
        append_status(&svc, &run.id, "optimizer.run.started", "running")
            .await
            .unwrap();
        svc.append_event_payloads(
            run.id.clone(),
            vec![OptimizerEventDraft::new("optimizer.usage", EVAL_ALGORITHM_ID)
                .usage_delta(Map::from_iter([
                    ("cost_usd".into(), json!(0.001)),
                    ("prompt_tokens".into(), json!(10)),
                    ("completion_tokens".into(), json!(2)),
                ]))],
        )
        .await
        .unwrap();
        append_provider_usage_receipt(
            &svc,
            &run.id,
            provider_usage_receipt(&run.id, 2, 40, 7, None, 'b'),
        )
        .await
        .unwrap();

        let projected = svc.get(run.id.clone()).await.unwrap();
        assert_eq!(projected.usage.calls, 2);
        assert_eq!(projected.usage.prompt_tokens, 40);
        assert_eq!(projected.usage.completion_tokens, 7);
        assert_eq!(projected.usage.cost_usd, None);
        let view = serde_json::to_value(svc.run_view_v2(run.id.clone()).await.unwrap()).unwrap();
        assert_eq!(view["header"]["usage"]["calls"], json!(2));
        assert_eq!(view["header"]["usage"]["promptTokens"], json!(40));
        assert_eq!(view["header"]["usage"]["completionTokens"], json!(7));
        assert_eq!(view["header"]["usage"]["costUsd"], Value::Null);

        append_terminal(&svc, &run.id, "failed", "fixture settlement".into())
            .await
            .unwrap();
        let manifest = svc
            .terminal_manifest(run.id)
            .await
            .unwrap()
            .expect("failed run seals a manifest");
        assert_eq!(manifest["usage"]["calls"], json!(2));
        assert_eq!(manifest["usage"]["promptTokens"], json!(40));
        assert_eq!(manifest["usage"]["completionTokens"], json!(7));
        assert_eq!(manifest["usage"]["costUsd"], Value::Null);
    }

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
        /// Rollouts block until the client goes away: the shape of a long
        /// in-flight trial, used to hold work open across a cancellation.
        stall_rollouts: bool,
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
            stall_rollouts: false,
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
                            if opts.stall_rollouts {
                                // Hold the blocking rollout open. Cancellation
                                // drops the client request, which is how a real
                                // container observes the episode ending.
                                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                            }
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
                            match opts.rewards.get(&(pool, seed)).copied() {
                                Some(reward) => JsonHttpResponse::ok(json!({
                                    "status": "scored",
                                    "reward": reward,
                                    "rollout_id": rollout_id
                                })),
                                None => JsonHttpResponse::ok(json!({
                                    "status": "absent",
                                    "reward": null,
                                    "reason": "fixture evaluator produced no measurement",
                                    "rollout_id": rollout_id
                                })),
                            }
                        }
                        // Terminal scoring is a real container operation, so an
                        // `absent` GET is followed by an explicit POST to the
                        // named evaluation plan. A fixture that 404s here turns
                        // a missing measurement into a reward-endpoint failure
                        // and hides the honest typed reason.
                        ("POST", "/reward") => {
                            if request.body.get("mode") != Some(&json!("terminal"))
                                || request.body.get("rescore") != Some(&json!(false))
                                || request.body.get("evaluation_plan_ref").and_then(Value::as_str).is_none()
                            {
                                return JsonHttpResponse::error(
                                    StatusCode::UNPROCESSABLE_ENTITY,
                                    "terminal reward request omitted its authority binding",
                                );
                            }
                            let Some(rollout_id) = request
                                .body
                                .get("rollout_id")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                            else {
                                return JsonHttpResponse::error(
                                    StatusCode::UNPROCESSABLE_ENTITY,
                                    "terminal reward request omitted its rollout",
                                );
                            };
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
                            match opts.rewards.get(&(pool, seed)).copied() {
                                Some(reward) => JsonHttpResponse::ok(json!({
                                    "status": "scored",
                                    "reward": reward,
                                    "rollout_id": rollout_id,
                                    "evaluation_plan_ref": request.body["evaluation_plan_ref"],
                                })),
                                // The plan ran and produced nothing. That is a
                                // measurement the recipe required and did not
                                // get, not a broken endpoint.
                                None => JsonHttpResponse::ok(json!({
                                    "status": "absent",
                                    "reward": null,
                                    "reason": "fixture evaluation plan produced no measurement",
                                    "rollout_id": rollout_id,
                                    "evaluation_plan_ref": request.body["evaluation_plan_ref"],
                                })),
                            }
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
                                "protocol": crate::container_capabilities::LIVE_EVAL_PROTOCOL,
                                "operations": {
                                    "rollouts.prepare": true,
                                    "rollouts.start": true,
                                    "rollouts.events": true
                                }
                            },
                            "imageDigest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                            "info": {
                                "imageDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                                "producerSourceRevision": "containers@fixture"
                            }
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
        crate::workspace_scope::provision(svc.database(), session, workspace.to_str().unwrap())
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
            json!(crate::container_capabilities::LIVE_EVAL_PROTOCOL)
        );
        assert_eq!(
            finished.summary["containerImageDigest"],
            json!("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(
            finished.summary["containerProducerSourceRevision"],
            json!("containers@fixture")
        );
        let runtime_ref = finished
            .input_refs
            .iter()
            .find(|reference| reference.kind == "container")
            .unwrap();
        assert_eq!(
            runtime_ref.digest.as_deref(),
            Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(
            runtime_ref.metadata["producerSourceRevision"],
            json!("containers@fixture")
        );

        let events = svc
            .events_after(run.id.clone(), 0, Some(500))
            .await
            .unwrap();
        let sequences: Vec<u64> = events.iter().map(|event| event.sequence_number).collect();
        assert_eq!(
            sequences,
            (1..=sequences.len() as u64).collect::<Vec<_>>(),
            "the saved log must be contiguous from 1"
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
        assert_eq!(manifest["terminal"]["kind"], json!("completed"));
        assert_eq!(manifest["work"]["planned"], json!(10));
        assert_eq!(manifest["work"]["succeeded"], json!(10));
        assert_eq!(manifest["work"]["failed"], json!(0));
        assert_eq!(manifest["work"]["cancelled"], json!(0));
        assert_eq!(manifest["terminalCursor"], json!(finished.cursor_seq));
        task.abort();
    }

    /// A completed manifest is immutable. The worker must finish after sealing
    /// instead of attempting one more summary patch, treating that refusal as
    /// a worker failure, and repainting the already-saved primary visual red.
    #[tokio::test]
    async fn a_successful_eval_worker_does_not_project_progress_after_the_terminal_seal() {
        let (svc, _dir, _) = service().await;
        let (run, task) = start_banking77(&svc, "sess_no_post_seal_projection").await;
        let manifest = wait_manifest(&svc, &run.id).await;
        assert_eq!(manifest["terminal"]["kind"], json!("completed"));

        for _ in 0..200 {
            if !svc.registered_local_recipes().await.contains(&run.id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            !svc.registered_local_recipes().await.contains(&run.id),
            "the successful worker must return and release its local owner"
        );

        let finished = svc.get(run.id.clone()).await.unwrap();
        assert_eq!(finished.status, "completed");
        assert_eq!(finished.summary["evalStatus"], json!("completed"));
        let visual_id = finished.summary["visualId"].as_str().unwrap();
        let visual = svc.visuals().get(visual_id.to_string()).await.unwrap();
        let view = serde_json::to_value(svc.run_view_v2(run.id.clone()).await.unwrap()).unwrap();
        assert_eq!(visual.status, VisualStatus::Saved);
        assert_eq!(
            visual.bindings.pointer("/inputs/0/data/status"),
            Some(&json!("completed")),
            "a post-seal patch refusal must not enter the worker-failure visual path"
        );
        assert_eq!(
            visual.bindings.pointer("/inputs/0/data/aggregate"),
            Some(&view["aggregate"]),
            "experiment and V2/chat consumers must receive the same revisioned aggregate bytes"
        );
        assert_eq!(
            view["aggregate"]["projectionRevision"],
            view["header"]["projectionRevision"]
        );
        assert_eq!(
            view["aggregate"]["asOfSequence"],
            view["header"]["asOfSequence"]
        );
        task.abort();
    }

    /// Container lifecycle completion is not evaluator success. If the reward
    /// endpoint says `absent`, every affected trial and the run settle failed
    /// with a measurement-specific reason instead of first counting as
    /// succeeded and then degrading at the evidence gate.
    #[tokio::test]
    async fn container_completed_rollouts_without_measurements_settle_as_normal_failures() {
        let (svc, _dir, _) = service().await;
        let session = "sess_missing_evaluator_measurement";
        let (base, task, starts) = spawn_eval_mock("banking77", BTreeMap::new()).await;
        insert_container(&svc, "banking77", &base, "ready").await;
        declare_eval_recipes(&svc, session).await;
        let (run, _) = svc
            .start_recipe(OptimizerRecipeRunRequest {
                recipe_id: CLASSIFY_EVAL.into(),
                session_ref: Some(session.into()),
                open_visual: Some(false),
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

        let finished = wait_terminal(&svc, &run.id).await;
        let manifest = wait_manifest(&svc, &run.id).await;
        assert_eq!(starts.load(Ordering::SeqCst), 10);
        assert_eq!(finished.status, "failed");
        assert_eq!(finished.summary["progress"]["completed"], json!(0));
        assert_eq!(finished.summary["progress"]["failed"], json!(10));
        assert_eq!(manifest["terminal"]["kind"], json!("failed"));
        assert_eq!(manifest["work"]["succeeded"], json!(0));
        assert_eq!(manifest["work"]["failed"], json!(10));
        let error = finished.error.unwrap_or(Value::Null).to_string();
        assert!(
            error.contains("evaluator_measurement_missing"),
            "{error}\nRECORDS={}",
            finished.summary["records"]
        );
        assert!(error.contains("no evaluator measurement"), "{error}");

        let events = svc
            .events_after(run.id.clone(), 0, Some(500))
            .await
            .unwrap();
        let terminals = events
            .iter()
            .filter(|event| event.event_type == "eval.trial.terminal")
            .collect::<Vec<_>>();
        assert_eq!(terminals.len(), 10);
        assert!(terminals.iter().all(|event| {
            event.item.as_ref().is_some_and(|item| {
                item["valid"] == json!(false)
                    && item["status"] == json!("failed")
                    && item["raw"]["reportedStatus"] == json!("completed")
                    && item["raw"]["evaluatorOutcome"]["status"] == json!("failed")
                    && item["raw"]["evaluatorOutcome"]["reason"]
                        == json!("evaluator_measurement_missing")
                    && item["raw"]["error"]
                        .as_str()
                        .is_some_and(|error| error.starts_with("evaluator_measurement_missing:"))
            })
        }));
        assert!(terminals.iter().all(|event| {
            event.item.as_ref().is_some_and(|item| {
                ["calls", "steps", "tokens", "costUsd", "achievements", "frames"]
                    .iter()
                    .all(|name| {
                        let fact = &item["raw"]["reportedFacts"][name];
                        fact.is_object()
                            && fact.get("value").is_some()
                            && fact.get("source").and_then(Value::as_str).is_some()
                            && fact.get("unavailableReason").is_some()
                    })
            })
        }));
        let selection = events
            .iter()
            .find(|event| event.event_type == "eval.selection.completed")
            .and_then(|event| event.snapshot.as_ref())
            .and_then(|snapshot| snapshot.get("selection"))
            .expect("a baseline evaluation must publish one typed selection outcome");
        assert_eq!(selection["status"], json!("promotion_not_applicable"));
        assert_eq!(selection["score"], Value::Null);

        let view = serde_json::to_value(svc.run_view_v2(run.id.clone()).await.unwrap()).unwrap();
        assert_eq!(
            view["aggregate"]["schemaVersion"],
            json!("eval.aggregate.v1")
        );
        assert_eq!(view["aggregate"]["lifecycle"], json!("terminal"));
        assert_eq!(
            view["aggregate"]["selection"],
            json!("promotion_not_applicable")
        );
        assert_eq!(view["aggregate"]["meanReward"], Value::Null);
        assert_eq!(view["aggregate"]["scoredTrials"], json!(0));
        assert_eq!(view["aggregate"]["evaluatorEvidence"], json!(0));
        assert_eq!(view["aggregate"]["work"]["failed"], json!(10));
        assert_eq!(
            view["aggregate"]["projectionRevision"],
            view["header"]["projectionRevision"]
        );
        assert_eq!(
            view["aggregate"]["asOfSequence"],
            view["header"]["asOfSequence"]
        );
        task.abort();
    }

    /// A cancel while trials are in flight settles every dispatched trial
    /// with an explicit `cancelled` terminal — never `failed` — and seals the
    /// run `cancelled` with a closed-world manifest, through the real
    /// dispatch, relay, reducer, and persistence paths. A step toward
    /// acceptance criterion 16; the drain runs against a mock container whose
    /// blocking rollouts stall until the client disconnects, not the fixture
    /// Craftax journal.
    #[tokio::test]
    async fn cancelling_in_flight_trials_settles_each_as_cancelled() {
        let (svc, _dir, _) = service().await;
        let session = "sess_cancel_inflight";
        let rewards = (0..10).map(|seed| (("train".into(), seed), 1.0)).collect();
        let (base, task, starts) = spawn_eval_mock_opts(MockEvalOptions {
            family: "banking77",
            rewards,
            policy_status: 200,
            policy_config_id: "banking77_gpt_4_1_nano".into(),
            fail_seeds: BTreeSet::new(),
            extra_cost_usd: None,
            policy_credential_present: true,
            grader_credential_present: true,
            advertises_model_roles: true,
            stall_rollouts: true,
        })
        .await;
        insert_container(&svc, "banking77", &base, "ready").await;
        declare_eval_recipes(&svc, session).await;
        let (run, _) = svc
            .start_recipe(OptimizerRecipeRunRequest {
                recipe_id: CLASSIFY_EVAL.into(),
                session_ref: Some(session.into()),
                open_visual: Some(false),
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

        for _ in 0..400 {
            if starts.load(Ordering::SeqCst) >= 5 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            starts.load(Ordering::SeqCst) >= 5,
            "five trials must be in flight before the cancel"
        );

        let request = crate::optimizers::kernel::CancellationRequest::new(
            crate::optimizers::kernel::CancellationCause::UserRequested,
            "user:test",
            format!("run:{}", run.id),
        );
        let request_id = request.request_id.clone();
        svc.cancel(run.id.clone(), request).await.unwrap();

        let finished = wait_terminal(&svc, &run.id).await;
        assert_eq!(finished.status, "cancelled");
        let manifest = wait_manifest(&svc, &run.id).await;
        assert_eq!(manifest["terminal"]["kind"], json!("cancelled"));
        assert_eq!(
            manifest["terminal"]["reason"],
            json!("operator_cancelled"),
            "a user-requested cancel settles as an operator decision"
        );
        // Closed world: nothing survives the seal as running or queued, and
        // interrupted work is cancelled, not failed.
        assert_eq!(manifest["work"]["planned"], json!(10));
        assert_eq!(manifest["work"]["running"], json!(0));
        assert_eq!(manifest["work"]["queued"], json!(0));
        assert_eq!(manifest["work"]["succeeded"], json!(0));
        assert_eq!(manifest["work"]["failed"], json!(0));
        assert_eq!(manifest["work"]["cancelled"], json!(10));

        let dispatched = starts.load(Ordering::SeqCst);
        let events = svc
            .events_after(run.id.clone(), 0, Some(500))
            .await
            .unwrap();
        let cancelled_trials = events
            .iter()
            .filter(|event| {
                event.event_type == "eval.trial.terminal"
                    && event
                        .item
                        .as_ref()
                        .and_then(|item| item.get("status"))
                        .and_then(Value::as_str)
                        == Some("cancelled")
            })
            .count();
        // Every spawned worker settles with its own explicit cancelled
        // terminal (a trial still in prepare when the cancel lands is also
        // interrupted work); at minimum that covers the five rollouts that
        // were in flight at the container.
        assert!(
            cancelled_trials >= dispatched && dispatched >= 5,
            "expected explicit cancelled terminals ({cancelled_trials}) to cover the \
             {dispatched} in-flight rollouts"
        );
        let cancelled_with_receipts = events
            .iter()
            .filter(|event| {
                event.event_type == "eval.trial.terminal"
                    && event
                        .item
                        .as_ref()
                        .and_then(|item| item.get("status"))
                        .and_then(Value::as_str)
                        == Some("cancelled")
                    && event
                        .item
                        .as_ref()
                        .and_then(|item| item.get("rolloutId"))
                        .is_some_and(|value| !value.is_null())
                    && event
                        .item
                        .as_ref()
                        .and_then(|item| item.pointer("/cancellationReceipt/requestId"))
                        .and_then(Value::as_str)
                        == Some(request_id.as_str())
                    && event
                        .usage_delta
                        .as_ref()
                        .and_then(|usage| usage.get("usage_completeness"))
                        .and_then(Value::as_str)
                        == Some("partial")
            })
            .count();
        assert!(
            cancelled_with_receipts >= dispatched,
            "each dispatched rollout must retain identity, cancellation receipt, and partial usage ({cancelled_with_receipts}/{dispatched})"
        );
        let failed_trials = events
            .iter()
            .filter(|event| {
                event.event_type == "eval.trial.terminal"
                    && event
                        .item
                        .as_ref()
                        .and_then(|item| item.get("status"))
                        .and_then(Value::as_str)
                        == Some("failed")
            })
            .count();
        assert_eq!(failed_trials, 0, "interrupted trials never settle failed");
        let ledger = manifest["evidenceLedger"]
            .as_array()
            .expect("cancelled eval manifest carries its per-rollout evidence ledger");
        assert_eq!(ledger.len(), 10);
        assert!(ledger.iter().all(|entry| entry["state"] != json!("open")));
        assert!(ledger.iter().filter(|entry| !entry["rolloutId"].is_null()).count() >= dispatched);
        assert_eq!(manifest["usage"]["completeness"], json!("partial"));
        let terminal_event = events
            .iter()
            .find(|event| event.event_type == "optimizer.run.cancelled")
            .expect("the durable log carries the cancelled terminal fact");
        assert_eq!(
            terminal_event
                .delta
                .get("cancellation")
                .and_then(|cancellation| cancellation.get("requestId"))
                .and_then(Value::as_str),
            Some(request_id.as_str()),
            "the terminal event carries the typed request"
        );
        assert_eq!(
            terminal_event
                .delta
                .get("cancellation")
                .and_then(|cancellation| cancellation.get("requestedBy"))
                .and_then(Value::as_str),
            Some("user:test")
        );

        // The sealed run must load back through terminal replay: closure is a
        // pure function of the terminal event, so the rebuilt projection is
        // closed-world too.
        let run_id = run.id.clone();
        let state = svc
            .database()
            .clone()
            .run(move |conn| {
                crate::optimizers::kernel::persist::load_state(conn, &run_id)?
                    .ok_or_else(|| anyhow::anyhow!("cancelled run lost its kernel projection"))
            })
            .await
            .unwrap();
        assert!(state
            .projection
            .work_items()
            .iter()
            .all(|item| item.lifecycle
                == crate::optimizers::kernel::WorkItemLifecycle::Terminal));
        assert_eq!(
            state.terminal.as_ref().map(|terminal| terminal.kind),
            Some(crate::optimizers::kernel::TerminalKind::Cancelled)
        );
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

    /// Build a settled inline evaluation whose terminal visual predates the
    /// authoritative provider receipt: the projection was last published while
    /// the run was still live and cost was unknown, and the receipt landed
    /// afterwards. This is the state a run reopened under a newer Workshop
    /// build is in.
    async fn settled_inline_eval_with_a_stale_cost_projection(
        svc: &OptimizerService,
        run_id: &str,
        cost_usd: f64,
    ) -> (OptimizerRunRecord, EvalSpec, String) {
        let approved = super::super::admission::tests::nanohorizon_approved_specification();
        let spec = EvalSpec::from_execution_spec(
            approved.spec(),
            "craftax".into(),
            "world:craftax@eval".into(),
        )
        .unwrap();
        let seeds = spec.train.clone();
        let (run, _) = svc
            .create_admitted_eval(
                serde_json::from_value(json!({
                    "algorithmId": EVAL_ALGORITHM_ID,
                    "id": run_id,
                    "openVisual": false,
                    "summary": {
                        "recipeSourceKind": "inline",
                        "task": spec.family,
                        "costCeilingUsd": spec.cost_ceiling_usd,
                        "records": [],
                    },
                }))
                .unwrap(),
                approved,
                seeds.len(),
            )
            .await
            .unwrap();
        append_status(svc, &run.id, "optimizer.run.started", "running")
            .await
            .unwrap();
        append_eval_plan(svc, &run.id, &spec, &spec.examples())
            .await
            .unwrap();

        // Minted while the run is live: the cost lane has no receipt yet.
        let run = svc.get(run.id.clone()).await.unwrap();
        let visual_id = mint_experiment_visual(svc, &run, &spec, seeds.len())
            .await
            .unwrap();
        let records = seeds
            .iter()
            .map(|seed| {
                json!({
                    "seed": seed,
                    "pool": "train",
                    "worldRef": spec.world_ref,
                    "rolloutId": format!("roll_{seed}"),
                    "trialId": format!("trial_{seed}"),
                    "status": "completed",
                    "reward": 1.0,
                    "sealedTrace": {"traces": [{
                        "traceId": format!("trace_{seed}"),
                        "digest": format!("sha256:{}", "a".repeat(64)),
                    }]},
                })
            })
            .collect::<Vec<_>>();
        for (index, record) in records.iter().enumerate() {
            append_eval_terminal(svc, &run.id, &spec, index as u32, record)
                .await
                .unwrap();
        }
        let summary_records = records.clone();
        let owned_visual_id = visual_id.clone();
        svc.patch_run(run.id.clone(), move |run| {
            let mut summary = run.summary.as_object().cloned().unwrap_or_default();
            summary.insert("visualId".into(), json!(owned_visual_id));
            summary.insert("records".into(), json!(summary_records));
            run.summary = Value::Object(summary);
            Ok(())
        })
        .await
        .unwrap();

        // The receipt is the cost authority and lands before settlement, but
        // nothing republishes the visual it postdates.
        append_provider_usage_receipt(
            svc,
            &run.id,
            provider_usage_receipt(&run.id, 37, 116_385, 6_217, Some(cost_usd), 'c'),
        )
        .await
        .unwrap();
        append_terminal(svc, &run.id, "completed", String::new())
            .await
            .unwrap();
        (svc.get(run.id).await.unwrap(), spec, visual_id)
    }

    /// Reopening a settled inline evaluation republishes a terminal visual that
    /// predates the authoritative provider receipt — exactly once. The receipt
    /// is the cost authority, the sealed terminal manifest is immutable, and a
    /// second open must not touch the visual again.
    #[tokio::test]
    async fn reopening_a_terminal_inline_eval_refreshes_a_stale_cost_projection_exactly_once() {
        let (svc, _dir, _) = service().await;
        let cost_usd = 0.018659;
        let (run, _spec, visual_id) = settled_inline_eval_with_a_stale_cost_projection(
            &svc,
            "opt_eval_craftax_stale_projection",
            cost_usd,
        )
        .await;
        assert_eq!(run.status, "completed");
        assert_eq!(run.usage.cost_usd, Some(cost_usd));

        let authoritative = format!("${cost_usd:.6} / $2.45");
        let stale = svc.visuals().get(visual_id.clone()).await.unwrap();
        assert_eq!(
            stale.bindings.pointer("/inputs/0/data/progress/cost"),
            Some(&json!("awaiting telemetry / $2.45")),
            "the fixture must start from a projection that predates the receipt"
        );
        let sealed_manifest = svc
            .terminal_manifest(run.id.clone())
            .await
            .unwrap()
            .expect("a settled run seals a terminal manifest");

        assert!(
            refresh_terminal_visual_projection_if_stale(&svc, &run)
                .await
                .unwrap(),
            "reopening a run whose visual predates the receipt republishes it"
        );

        let refreshed = svc.visuals().get(visual_id.clone()).await.unwrap();
        assert_eq!(
            refreshed.bindings.pointer("/inputs/0/data/progress/cost"),
            Some(&json!(authoritative)),
            "the republished projection shows the provider receipt, not the live estimate"
        );
        assert_eq!(refreshed.status, VisualStatus::Saved);
        assert_eq!(
            refreshed.bindings.pointer("/inputs/0/data/status"),
            Some(&json!("completed"))
        );

        // Immutable evidence is not a projection: republishing must not rewrite
        // the sealed terminal manifest or the run's Trace V5 evidence refs.
        let reopened = svc.get(run.id.clone()).await.unwrap();
        assert_eq!(
            svc.terminal_manifest(run.id.clone()).await.unwrap(),
            Some(sealed_manifest),
            "the sealed terminal manifest is immutable across a reopen"
        );
        assert_eq!(reopened.output_refs, run.output_refs);
        assert_eq!(reopened.summary, run.summary);

        assert!(
            !refresh_terminal_visual_projection_if_stale(&svc, &reopened)
                .await
                .unwrap(),
            "a second open finds the projection current and does nothing"
        );
        let unchanged = svc.visuals().get(visual_id).await.unwrap();
        assert_eq!(
            unchanged.current_revision, refreshed.current_revision,
            "the no-op open must not bump the visual revision"
        );
        assert_eq!(unchanged.bindings, refreshed.bindings);
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
        assert_eq!(result["trials"]["succeeded"], json!(10), "{result}");
        assert_eq!(result["meanReward"], json!(1.0));
        assert_eq!(
            result["selection"],
            json!("promotion_not_applicable"),
            "a baseline-only eval cannot make a promotion claim"
        );
        assert_eq!(
            result["evidence"]["completeness"],
            json!("complete"),
            "settled work must carry an explicit evidence state"
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
        let owned_visuals = svc
            .visuals()
            .list(VisualQuery {
                session_id: Some("sess_owner".into()),
                ..VisualQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(
            owned_visuals.len(),
            2,
            "overview and workstation are Outputs"
        );
        for visual in &owned_visuals {
            assert_eq!(
                visual.run_id, None,
                "optimizer identity must not overload visuals.run_id's runs-table FK"
            );
            assert_eq!(
                crate::visuals::declared_optimizer_run_ids(&visual.bindings),
                vec![run.id.clone()],
                "both Outputs visuals durably bind the optimizer identity"
            );
        }

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
            persist_progress(
                &svc,
                &run.id,
                &spec,
                "vis_missing",
                "vis_workbench",
                &records,
                1,
                "completed",
            ),
        )
        .await
        .unwrap_err();
        settle_worker_failure(&svc, &run.id, failure).await.unwrap();

        let settled = svc.get(run.id.clone()).await.unwrap();
        assert_eq!(settled.status, "degraded");
        assert_eq!(
            settled.summary["records"].as_array().map(Vec::len),
            Some(1),
            "the successful rollout stays on the record"
        );
        let manifest = svc
            .terminal_manifest(run.id.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(manifest["terminal"]["kind"], json!("degraded"));
        let events = svc.events_after(run.id, 0, None).await.unwrap();
        assert_eq!(
            events.last().unwrap().delta["degradation"]["retryable"],
            json!(true)
        );
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
            stall_rollouts: false,
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
            stall_rollouts: false,
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
            stall_rollouts: false,
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
            stall_rollouts: false,
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
    async fn container_eval_refuses_a_ready_pool_that_does_not_advertise_live_eval() {
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
            error.contains("live-eval")
                && error.contains(crate::container_capabilities::LIVE_EVAL_PROTOCOL),
            "expected a live-eval admission refusal, got {error}"
        );
        assert!(
            error.contains("protocol=null"),
            "expected the advertised protocol to be named, got {error}"
        );
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        task.abort();
    }

    async fn recipe(svc: &OptimizerService, id: &str, session: &str) -> OptimizerRecipeRunRequest {
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
            error.contains("ambiguous registered banking77 live-eval containers"),
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
            .start_recipe(
                recipe_on(
                    &svc,
                    CLASSIFY_EVAL,
                    "sess_explicit_isolated",
                    Some("ctr_banking77_isolated"),
                )
                .await,
            )
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
            .start_recipe(
                recipe_on(
                    &svc,
                    CLASSIFY_EVAL,
                    "sess_not_ready",
                    Some("ctr_banking77_offline"),
                )
                .await,
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(
            not_ready.contains(
                "requested banking77 container `ctr_banking77_offline` is not a ready live-eval container"
            ),
            "expected a fail-closed missing-ready-id error, got {not_ready}"
        );
        assert!(
            !not_ready.contains("ambiguous"),
            "must not fall back to the other ready pool, got {not_ready}"
        );

        let wrong_family = svc
            .start_recipe(
                recipe_on(
                    &svc,
                    CLASSIFY_EVAL,
                    "sess_wrong_family",
                    Some("ctr_healthbench_ready"),
                )
                .await,
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(
            wrong_family.contains(
                "requested banking77 container `ctr_healthbench_ready` is not a ready live-eval container"
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
            error.contains("ambiguous registered banking77 live-eval containers"),
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
            stall_rollouts: false,
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
            stall_rollouts: false,
        })
        .await;
        let (svc, _dir, _) = service().await;
        insert_container(&svc, "healthbench", &base, "ready").await;
        let (run, _) = svc
            .start_recipe(recipe(&svc, HEALTH_EVAL, "sess_policy_mismatch").await)
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
            stall_rollouts: false,
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
            stall_rollouts: false,
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
    fn nanohorizon_proxy_config_never_requests_a_container_api_key() {
        let mut spec = EvalSpec::classify_fixture();
        spec.harness = "nanohorizon".into();
        let proxy = "http://host.docker.internal:9/cap/wcap_nanohorizon/v1/providers/openrouter";
        let body = spec.policy_config_body(Some(proxy)).unwrap();
        assert_eq!(body["config"]["base_url"], proxy);
        assert!(body.pointer("/config/api_key_env").is_none());
        assert!(body.pointer("/config/api_key").is_none());
    }

    #[test]
    fn inline_rollout_wait_covers_the_authorized_capability_lifetime() {
        let mut spec = EvalSpec::classify_fixture();
        spec.policy.insert("timeout_seconds".into(), json!(180.0));
        spec.maximum_model_calls_per_rollout = 10;
        spec.admitted_use_policy = Some(crate::secrets::SecretsUsePolicy {
            lifetime_seconds: 3_600,
            ..crate::secrets::SecretsUsePolicy::default()
        });

        assert_eq!(spec.blocking_http_timeout(), Duration::from_secs(3_840));
    }

    #[test]
    fn rollout_wait_without_declared_call_timeout_uses_long_running_default() {
        let mut spec = EvalSpec::classify_fixture();
        spec.policy.remove("timeout_seconds");
        spec.admitted_use_policy = None;

        assert_eq!(
            spec.blocking_http_timeout(),
            DEFAULT_BLOCKING_EVAL_HTTP_TIMEOUT
        );
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
        let retained = retained_rollout_state(&page).unwrap();
        assert_eq!(retained["observation"], json!("final map"));
        assert_eq!(retained["achievements"], json!(["collect_wood"]));
    }

    #[test]
    fn retained_rollout_state_rejects_a_missing_event_collection() {
        assert!(retained_rollout_state(&json!({})).is_err());
    }

    #[test]
    fn trace_placeholders_are_not_counted_as_retained_receipts() {
        let mut spec = EvalSpec::classify_fixture();
        spec.train = vec![780005, 780006, 780007, 780008, 780009];
        spec.heldout.clear();
        spec.maximum_model_calls_per_rollout = 0;
        spec.maximum_steps_per_rollout = 0;
        let bindings = experiment_bindings(
            &spec,
            "opt_eval_test",
            "failed",
            0,
            5,
            &[],
            None,
            "vis_workbench",
            Some(&json!({
                "reconciliationErrors": [
                    "This run is failed, but 5 rollouts are still queued or running."
                ],
                "rolloutStateCounts": { "queued": 5 }
            })),
            "2026-08-27T12:00:00Z",
            None,
        );
        let data = &bindings["inputs"][0]["data"];
        let traces = &data["traces"];
        assert_eq!(traces["plannedSlots"], json!(5));
        assert_eq!(traces["streamsOpened"], json!(0));
        assert_eq!(traces["receiptsRetained"], json!(0));
        assert_eq!(traces["sealed"], json!(0));
        assert_eq!(traces["evidenceGaps"], json!(5));
        assert_eq!(traces["items"].as_array().unwrap().len(), 0);
        let errors = data["reconciliationErrors"].as_array().unwrap();
        assert!(
            errors
                .iter()
                .any(|error| error.as_str().is_some_and(|text| text.contains("queued"))),
            "{errors:?}"
        );
        assert!(
            errors.iter().any(|error| error
                .as_str()
                .is_some_and(|text| text.contains("Approved call and step limits"))),
            "{errors:?}"
        );
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
        /// Serve the additive journal-v2 chain/ack/retention contract.
        journal_v2: bool,
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
        /// GET observes no evaluator receipt until Workshop explicitly asks
        /// the producer to materialize terminal scoring with POST /reward.
        reward_materializes_on_post: bool,
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
        let emit_step = |events: &mut Vec<Value>,
                         push: &dyn Fn(&mut Vec<Value>, &str, Value),
                         step: i64| {
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
        push(
            &mut events,
            "env.episode.opened",
            json!({"seed": 780_003, "max_steps": 64}),
        );
        emit_step(&mut events, &push, 0);
        push(
            &mut events,
            "policy.session.opened",
            json!({"harness": "craftax_react"}),
        );
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
            push(
                &mut events,
                "span.policy.plan",
                json!({"actions": ["move_right"], "length": 1}),
            );
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
                push(
                    &mut events,
                    "action_applied",
                    json!({"step": step, "action": "move_right"}),
                );
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
                    push(
                        &mut events,
                        "reward_delta",
                        json!({"step": step, "delta": 1.0}),
                    );
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
        push(
            &mut events,
            "policy.session.closed",
            json!({"calls": CRAFTAX_POLICY_CALLS}),
        );
        push(
            &mut events,
            "env.episode.closed",
            json!({"status": "completed", "steps": applied}),
        );
        push(
            &mut events,
            "status",
            json!({"status": "completed", "steps": applied}),
        );
        push(
            &mut events,
            "capture.closed",
            json!({"high_water": sequence.get()}),
        );
        if let Some(gap) = opts.skip_sequence {
            events.retain(|event| event.get("sequence").and_then(Value::as_u64) != Some(gap));
        }
        if opts.journal_v2 {
            for event in &mut events {
                let kind = event.get("kind").and_then(Value::as_str).unwrap();
                let sequence = event.get("sequence").and_then(Value::as_u64).unwrap();
                let payload = event.get("payload").cloned().unwrap();
                let canonical = serde_json::to_vec(&json!({
                    "kind": kind,
                    "sequence": sequence,
                    "payload": payload,
                }))
                .unwrap();
                event["digest"] =
                    json!(format!("{:x}", sha2::Sha256::digest(canonical))[..16].to_string());
            }
        }
        events
    }

    struct CraftaxMock {
        base: String,
        acks: Arc<Mutex<BTreeMap<String, u64>>>,
        task: tokio::task::JoinHandle<()>,
    }

    /// A Craftax-shaped container whose blocking rollout stays open while its
    /// journal becomes pollable page by page.
    async fn spawn_craftax_mock(opts: CraftaxMockOptions) -> CraftaxMock {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let journals: Arc<Mutex<BTreeMap<String, (Vec<Value>, bool)>>> =
            Arc::new(Mutex::new(BTreeMap::new()));
        let acks: Arc<Mutex<BTreeMap<String, u64>>> = Arc::new(Mutex::new(BTreeMap::new()));
        let exposed_acks = acks.clone();
        let script = Arc::new(craftax_journal(opts));
        let task = tokio::spawn(async move {
            let _ = serve_json(listener, move |request: JsonHttpRequest| {
                let journals = journals.clone();
                let acks = acks.clone();
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
                            acks.lock().unwrap().insert(rollout_id.clone(), 0);
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
                            let requested_ack = query.split('&')
                                .find_map(|pair| pair.strip_prefix("ack="))
                                .and_then(|value| value.parse::<u64>().ok())
                                .unwrap_or(0)
                                .min(high_water);
                            let acked = {
                                let mut guard = acks.lock().unwrap();
                                let value = guard.entry(rollout_id.clone()).or_default();
                                *value = (*value).max(requested_ack);
                                *value
                            };
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
                            let mut response = json!({
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
                            });
                            if opts.journal_v2 {
                                let mut chain_head = format!(
                                    "{:x}",
                                    sha2::Sha256::digest(rollout_id.as_bytes())
                                );
                                for event in events {
                                    let digest = event.get("digest").and_then(Value::as_str).unwrap();
                                    chain_head = format!(
                                        "{:x}",
                                        sha2::Sha256::digest(
                                            format!("{chain_head}{digest}").as_bytes()
                                        )
                                    );
                                }
                                response["cursor"]["chain_head"] = json!(chain_head);
                                response["cursor"]["acked"] = json!(acked);
                                response["retention"] = json!({
                                    "policy": "until-acked-or-ttl",
                                    "ttl_seconds": 604800,
                                    "acked": acked,
                                    "high_water": high_water,
                                    "closed": *closed,
                                    "released": *closed && acked >= high_water,
                                    "released_reason": if *closed && acked >= high_water {
                                        Value::String("acked".into())
                                    } else {
                                        Value::Null
                                    },
                                    "expires_at": Value::Null,
                                });
                            }
                            JsonHttpResponse::ok(response)
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
                                            if opts.journal_v2 {
                                                let canonical = serde_json::to_vec(&json!({
                                                    "kind": event["kind"],
                                                    "sequence": event["sequence"],
                                                    "payload": event["payload"],
                                                }))
                                                .unwrap();
                                                event["digest"] = json!(format!(
                                                    "{:x}",
                                                    sha2::Sha256::digest(canonical)
                                                )[..16]
                                                    .to_string());
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
                        ("GET", path) if path.ends_with("/reward") => JsonHttpResponse::ok(if opts.reward_materializes_on_post {
                            json!({"status": "absent", "reward": null})
                        } else {
                            json!({"status": "scored", "reward": CRAFTAX_TOTAL_REWARD})
                        }),
                        ("POST", "/reward") if opts.reward_materializes_on_post => {
                            if request.body.get("mode") != Some(&json!("terminal"))
                                || request.body.get("rescore") != Some(&json!(false))
                                || request.body.get("rollout_id").and_then(Value::as_str).is_none()
                                || request.body.get("evaluation_plan_ref").and_then(Value::as_str).is_none()
                            {
                                return JsonHttpResponse::error(
                                    StatusCode::UNPROCESSABLE_ENTITY,
                                    "terminal reward request omitted its authority binding",
                                );
                            }
                            JsonHttpResponse::ok(json!({
                                "status": "scored",
                                "reward": CRAFTAX_TOTAL_REWARD,
                                "evaluation_plan_ref": request.body["evaluation_plan_ref"],
                            }))
                        }
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
        CraftaxMock {
            base: format!("http://{addr}"),
            acks: exposed_acks,
            task,
        }
    }

    async fn declare_craftax_recipe(svc: &OptimizerService, session: &str, extra: &str) {
        let workspace = tempfile::Builder::new()
            .prefix("ws-craftax-")
            .tempdir()
            .unwrap()
            .into_path();
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
            .pointer("/delta/container_event/kind")
            .and_then(Value::as_str)
            .unwrap_or("")
    }

    fn count_kind(events: &[Value], kind: &str) -> usize {
        container_events(events)
            .iter()
            .filter(|event| kind_of(event) == kind)
            .count()
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
        assert!(
            started < first_container,
            "trial.started must precede its events"
        );

        // 2. Policy and frame events land before the trial settles.
        let terminal = match index_of_type(&events, "eval.trial.terminal") {
            Some(terminal) => terminal,
            None => panic!(
                "no terminal; run={:?}",
                svc.get(run_id.clone()).await.unwrap()
            ),
        };
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
        assert_eq!(
            count_kind(&events, "span.policy.data"),
            CRAFTAX_POLICY_CALLS
        );
        assert_eq!(
            count_kind(&events, "action_applied"),
            CRAFTAX_APPLIED_ACTIONS
        );
        assert_eq!(
            count_kind(&events, "achievement_unlocked"),
            CRAFTAX_ACHIEVEMENTS
        );
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
                .pointer("/delta/container_event/payload/media")
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
        assert!(
            !encoded.contains("data:image/png;base64"),
            "a PNG leaked into optimizer_events"
        );

        // 6. The record reports the reward and its relay receipt honestly.
        let run = svc.get(run_id.clone()).await.unwrap();
        let record = run.summary.pointer("/records/0").cloned().unwrap();
        assert_eq!(record["reward"], json!(CRAFTAX_TOTAL_REWARD));
        assert_eq!(
            record["steps"],
            Value::Null,
            "an unverified legacy journal must not become terminal step authority"
        );
        assert_eq!(record["relay"]["framesDeclared"], json!(CRAFTAX_FRAMES));
        assert_eq!(record["relay"]["framesRetained"], json!(CRAFTAX_FRAMES));
        assert_eq!(record["relay"]["journalClosed"], json!(true));
        let policy_spans = container_events(&events)
            .into_iter()
            .filter(|event| kind_of(event) == "span.policy.data")
            .collect::<Vec<_>>();
        assert!(policy_spans.iter().all(|event| {
            event
                .pointer("/usageDelta/prompt_tokens")
                .and_then(Value::as_u64)
                .is_some()
                && event
                    .pointer("/usageDelta/completion_tokens")
                    .and_then(Value::as_u64)
                    .is_some()
        }));
        let trial_terminal = events
            .iter()
            .find(|event| event.get("type").and_then(Value::as_str) == Some("eval.trial.terminal"))
            .expect("no trial terminal usage reconciliation");
        // The container aggregate (1000/200) is below already-committed span
        // usage (1045/245), so it cannot subtract or duplicate durable usage.
        assert_eq!(trial_terminal["usageDelta"]["prompt_tokens"], json!(0));
        assert_eq!(trial_terminal["usageDelta"]["completion_tokens"], json!(0));
        assert_eq!(trial_terminal["usageDelta"]["usage_completeness"], json!("partial"));
        let manifest = wait_manifest(&svc, &run_id).await;
        assert_eq!(manifest["usage"]["promptTokens"], json!(1045));
        assert_eq!(manifest["usage"]["completionTokens"], json!(245));
        assert_eq!(manifest["usage"]["completeness"], json!("partial"));
        assert_eq!(manifest["usage"]["costUsd"], Value::Null);
        // The container offers no self-contained bundle, and the record says
        // exactly that rather than implying an inspectable replay.
        assert_eq!(record["sealedTrace"]["imported"], json!(false));
        mock.task.abort();
    }

    #[tokio::test]
    async fn journal_v2_is_chain_verified_and_acked_after_durable_relay() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions {
                journal_v2: true,
                ..CraftaxMockOptions::default()
            },
            "",
        )
        .await;
        let run = svc.get(run_id).await.unwrap();
        let record = run.summary.pointer("/records/0").unwrap();
        let rollout_id = record["rolloutId"]
            .as_str()
            .unwrap_or_else(|| panic!("journal-v2 rollout failed before receipt: {record}"));
        let high_water = record["relay"]["highWater"].as_u64().unwrap();
        assert!(high_water > 0);
        assert_eq!(record["relay"]["journalAcked"], json!(high_water));
        assert_eq!(
            record["relay"]["journalChainHead"].as_str().unwrap().len(),
            64
        );
        assert_eq!(
            record["relay"]["journalRetention"]["policy"],
            json!("until-acked-or-ttl")
        );
        assert_eq!(
            mock.acks.lock().unwrap().get(rollout_id).copied(),
            Some(high_water),
            "producer never observed Workshop's recorded high-water ack"
        );
        mock.task.abort();
    }

    #[tokio::test]
    async fn verified_terminal_lifecycle_supplies_steps_and_scoring_stays_evaluator_owned() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions {
                journal_v2: true,
                reward_materializes_on_post: true,
                ..CraftaxMockOptions::default()
            },
            "",
        )
        .await;
        let run = svc.get(run_id).await.unwrap();
        let record = run.summary.pointer("/records/0").unwrap();
        assert_eq!(record["status"], json!("completed"), "{record}");
        assert_eq!(record["steps"], json!(CRAFTAX_APPLIED_ACTIONS));
        assert_eq!(record["stepsSource"], json!("retained_event_log"));
        assert_eq!(
            record["reportedFacts"]["steps"]["source"],
            json!("retained_event_log")
        );
        assert_eq!(record["reward"], json!(CRAFTAX_TOTAL_REWARD));
        assert_eq!(record["rewardStatus"], json!("scored"));
        assert_eq!(
            record["relay"]["terminalEnvironmentSteps"],
            json!(CRAFTAX_APPLIED_ACTIONS)
        );
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
                event
                    .pointer("/delta/container_event/sequence")
                    .and_then(Value::as_u64)
            })
            .collect();
        let relayed = sequences.len();
        sequences.sort_unstable();
        sequences.dedup();
        assert_eq!(
            sequences.len(),
            relayed,
            "a producer sequence was relayed twice"
        );
        assert_eq!(
            sequences,
            (1..=relayed as u64).collect::<Vec<_>>(),
            "the relay is not contiguous"
        );
        assert!(relayed > 100, "paging lost events: only {relayed} relayed");
        mock.task.abort();
    }

    #[tokio::test]
    async fn a_sequence_gap_fails_the_trial_visibly() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions {
                skip_sequence: Some(12),
                ..Default::default()
            },
            "",
        )
        .await;
        let run = svc.get(run_id.clone()).await.unwrap();
        assert_eq!(
            run.status, "failed",
            "a gapped journal must not settle completed"
        );
        let record = run.summary.pointer("/records/0").cloned().unwrap();
        let error = record["error"].as_str().unwrap_or_default();
        assert!(error.contains("sequence gap"), "gap was not named: {error}");
        assert_eq!(record["evidenceState"], json!("rejected"));
        let rollout_id = record["rolloutId"]
            .as_str()
            .expect("a dispatched integrity failure keeps rolloutId");
        let trial_id = record["trialId"]
            .as_str()
            .expect("a dispatched integrity failure keeps trialId");
        let events = relayed_events(&svc, &run_id).await;
        let started = events
            .iter()
            .find(|event| event.get("type").and_then(Value::as_str) == Some("eval.trial.started"))
            .expect("trial start identity");
        let terminal = events
            .iter()
            .find(|event| event.get("type").and_then(Value::as_str) == Some("eval.trial.terminal"))
            .expect("trial terminal identity");
        assert_eq!(started["delta"]["rollout_id"], json!(rollout_id));
        assert_eq!(started["delta"]["trial_id"], json!(trial_id));
        assert_eq!(terminal["item"]["rolloutId"], json!(rollout_id));
        assert_eq!(terminal["item"]["trialId"], json!(trial_id));
        mock.task.abort();
    }

    #[tokio::test]
    async fn identical_frames_deduplicate_without_losing_a_step() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions {
                identical_frames: true,
                ..Default::default()
            },
            "",
        )
        .await;
        let events = relayed_events(&svc, &run_id).await;
        let digests: Vec<String> = container_events(&events)
            .into_iter()
            .filter(|event| kind_of(event) == "frame")
            .filter_map(|event| {
                event
                    .pointer("/delta/container_event/payload/media/casDigest")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        // Thirteen logical frames in the timeline; one object on disk.
        assert_eq!(digests.len(), CRAFTAX_FRAMES);
        let unique: std::collections::BTreeSet<&String> = digests.iter().collect();
        assert_eq!(
            unique.len(),
            1,
            "identical PNGs did not collapse in the store"
        );
        let run = svc.get(run_id).await.unwrap();
        let relay = &run.summary["records"][0]["relay"];
        assert_eq!(
            relay["frameObservationsRetained"],
            json!(CRAFTAX_FRAMES),
            "logical frame observations remain on the timeline"
        );
        assert_eq!(
            relay["uniqueFrameBlobs"],
            json!(1),
            "physical CAS objects are reported separately"
        );
        mock.task.abort();
    }

    #[tokio::test]
    async fn a_corrupt_frame_is_refused_and_reported_rather_than_shown() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions {
                corrupt_frame_step: Some(3),
                ..Default::default()
            },
            "",
        )
        .await;
        let events = relayed_events(&svc, &run_id).await;
        let refused: Vec<&Value> = container_events(&events)
            .into_iter()
            .filter(|event| {
                event
                    .pointer("/delta/container_event/payload/mediaError")
                    .is_some()
            })
            .collect();
        assert_eq!(refused.len(), 1, "the corrupt frame was not refused");
        // The step is still in the timeline: the environment did render it.
        assert_eq!(count_kind(&events, "frame"), CRAFTAX_FRAMES);
        let degraded: Vec<&Value> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(Value::as_str) == Some("eval.trial.degraded")
            })
            .collect();
        assert_eq!(
            degraded.len(),
            1,
            "a refused frame produced no degradation receipt"
        );
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
            CraftaxMockOptions {
                redirect_frame_step: Some(2),
                ..Default::default()
            },
            "",
        )
        .await;
        let events = relayed_events(&svc, &run_id).await;
        let detail = container_events(&events)
            .into_iter()
            .find_map(|event| {
                event
                    .pointer("/delta/container_event/payload/mediaError/detail")
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
            CraftaxMockOptions {
                oversize_frame_step: Some(1),
                ..Default::default()
            },
            "[media]\nmax_frame_bytes = 4096\n",
        )
        .await;
        let events = relayed_events(&svc, &run_id).await;
        let detail = container_events(&events)
            .into_iter()
            .find_map(|event| {
                event
                    .pointer("/delta/container_event/payload/mediaError/detail")
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
                .filter(|event| event
                    .pointer("/delta/container_event/payload/media")
                    .is_some())
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
                .pointer("/delta/container_event/payload/media/casDigest")
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
                    .pointer("/delta/container_event/payload/media/casDigest")
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
        assert!(svc
            .read_media_bytes(&granted)
            .unwrap()
            .starts_with(b"\x89PNG\r\n\x1a\n"));

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
        assert!(svc
            .granted_run_media(&run_id, "4e27ac3b1f0a9d55")
            .await
            .is_err());
        assert!(svc
            .granted_run_media(&run_id, "../../etc/passwd")
            .await
            .is_err());
        mock.task.abort();
    }

    #[tokio::test]
    async fn identical_frames_share_one_grant_across_every_step_that_rendered_them() {
        let (svc, run_id, mock) = run_craftax(
            CraftaxMockOptions {
                identical_frames: true,
                ..Default::default()
            },
            "",
        )
        .await;
        let events = relayed_events(&svc, &run_id).await;
        let digest = container_events(&events)
            .into_iter()
            .find_map(|event| {
                event
                    .pointer("/delta/container_event/payload/media/casDigest")
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
        assert!(svc
            .granted_run_media(&run_id, &digest)
            .await
            .unwrap()
            .is_some());
        mock.task.abort();
    }

    #[test]
    fn terminal_trace_requires_every_native_frame_step() {
        let terminal = json!({"steps": 3});
        let complete = json!({"importedFrameSteps": [0, 1, 2, 3]});
        verify_complete_native_frame_trace(
            &terminal,
            &complete,
            "roll_complete",
            FrameTraceMode::SealedComplete,
        )
        .unwrap();

        let incomplete = json!({"importedFrameSteps": [0]});
        let error =
            verify_complete_native_frame_trace(
                &terminal,
                &incomplete,
                "roll_sparse",
                FrameTraceMode::SealedComplete,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("full_trace_frame_coverage_incomplete"));
        assert!(format!("{error:#}").contains("1 of 4 required native frame steps"));
    }

    #[test]
    fn terminal_trace_deduplicates_identical_frame_steps_not_observations() {
        let terminal = json!({"steps": 2});
        let imported = json!({"importedFrameSteps": [0, 1, 1, 2]});
        verify_complete_native_frame_trace(
            &terminal,
            &imported,
            "roll_duplicate_blob",
            FrameTraceMode::SealedComplete,
        )
        .unwrap();

        let partial = json!({"importedFrameSteps": [0, 1, 2]});
        verify_complete_native_frame_trace(
            &json!({"steps": 20, "lastObservedStep": 2}),
            &partial,
            "roll_partial",
            FrameTraceMode::SealedPartial {
                last_pre_cancellation_step: 2,
            },
        )
        .unwrap();
    }

    #[test]
    fn required_trace_gate_rejects_best_effort_import_failures() {
        let terminal = json!({"steps": 2});
        let unavailable = json!({
            "imported": false,
            "error": "container was stopped before import",
        });
        let error = verify_required_sealed_trace(&terminal, &unavailable, "roll_missing")
            .unwrap_err();
        assert!(format!("{error:#}").contains("required_trace_import_failed"));

        let complete = json!({
            "imported": true,
            "sourceKind": "container_bundle",
            "trusted": true,
            "inspectable": true,
            "bundleDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "traces": [{
                "traceId": "trace_complete",
                "digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            }],
            "provenanceBinding": {
                "imageDigest": "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "producerSourceRevision": "containers@abc123",
                "traceDigest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "bundleDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            },
            "importedFrameSteps": [0, 1, 2],
        });
        verify_required_sealed_trace(&terminal, &complete, "roll_complete").unwrap();
    }

    #[test]
    fn reconciliation_keeps_producer_and_workshop_trace_ids_in_separate_namespaces() {
        let imported = json!({
            "traceId": "tracev5_0062f1795c9ada2fba747b94",
            "producerTraceId": "roll_craftax_train_780018_674963f8",
            "digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        });
        let (trace_ref, digest) = verify_reconciled_trace_identity(
            &imported,
            "roll_craftax_train_780018_674963f8",
            "roll_craftax_train_780018_674963f8",
        )
        .unwrap();
        assert_eq!(trace_ref, "tracev5_0062f1795c9ada2fba747b94");
        assert_eq!(
            digest,
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );

        let mismatch = verify_reconciled_trace_identity(
            &imported,
            "roll_craftax_train_780018_other",
            "roll_craftax_train_780018_other",
        )
        .unwrap_err();
        assert!(format!("{mismatch:#}").contains("trace_identity_mismatch"));
        assert!(format!("{mismatch:#}").contains("Workshop index"));
    }

    #[test]
    fn aggregates_exclude_scores_without_a_valid_terminal_outcome() {
        let records = vec![
            json!({
                "pool": "train",
                "status": "completed",
                "reward": 1.0,
                "evaluatorOutcome": {"status": "scored", "reward": 1.0},
            }),
            json!({
                "pool": "train",
                "status": "failed",
                "reward": 99.0,
                "evaluatorOutcome": {"status": "scored", "reward": 99.0},
                "evidenceOutcome": {"status": "failed"},
            }),
        ];
        assert_eq!(mean_for_pool(&records, "train"), Some(1.0));
        assert_eq!(mean_reward(&records), Some(1.0));
    }

    #[test]
    fn rollout_facts_distinguish_authoritative_empty_values_from_unavailable() {
        let mut reported = json!({
            "usage": {
                "calls": 3,
                "prompt_tokens": 10,
                "completion_tokens": 2,
                "cost_usd": 0.25,
            },
            "steps": 4,
            "checkpointAchievements": [],
            "sealedTrace": {"importedFrameSteps": [0, 1, 2, 3, 4]},
        });
        attach_reported_facts(&mut reported);
        assert_eq!(reported["reportedFacts"]["calls"]["value"], json!(3));
        assert_eq!(reported["reportedFacts"]["steps"]["value"], json!(4));
        assert_eq!(reported["reportedFacts"]["tokens"]["value"], json!(12));
        assert_eq!(reported["reportedFacts"]["costUsd"]["value"], json!(0.25));
        assert_eq!(
            reported["reportedFacts"]["achievements"]["value"],
            json!([])
        );
        assert_eq!(reported["reportedFacts"]["frames"]["value"], json!(5));
        assert_eq!(
            reported["reportedFacts"]["achievements"]["source"],
            json!("retained_event_log")
        );
        assert_eq!(
            reported["reportedFacts"]["frames"]["source"],
            json!("trusted_trace_v5")
        );
        assert!(
            ["calls", "steps", "tokens", "costUsd", "achievements", "frames"]
                .iter()
                .all(|name| reported["reportedFacts"][name]["unavailableReason"].is_null())
        );

        let mut missing = json!({});
        attach_reported_facts(&mut missing);
        assert_eq!(missing["reportedFacts"]["calls"]["value"], Value::Null);
        assert_eq!(
            missing["reportedFacts"]["calls"]["unavailableReason"],
            json!("calls_not_reported")
        );
        assert_eq!(
            missing["reportedFacts"]["achievements"]["unavailableReason"],
            json!("achievements_not_reported")
        );
        assert_eq!(
            missing["reportedFacts"]["frames"]["unavailableReason"],
            json!("frames_not_retained")
        );
    }
}
