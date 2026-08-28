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
const POLL_INTERVAL: Duration = Duration::from_millis(80);
const DEFAULT_BLOCKING_EVAL_HTTP_TIMEOUT: Duration =
    crate::limits::CONTAINER_POLICY_ROLLOUT_TIMEOUT;

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

    /// Keep terminal observation alive for the full approved credential lease.
    /// A harness may spend most of its model-call window before running a
    /// verifier, and revoking at the shorter HTTP default would orphan that
    /// otherwise bounded work.
    fn terminal_poll_timeout(&self) -> Duration {
        let lease_seconds = self
            .admitted_use_policy
            .as_ref()
            .map(|policy| policy.lifetime_seconds)
            .unwrap_or_else(|| crate::limits::SECRETS_CAPABILITY_TTL.as_secs());
        self.blocking_http_timeout()
            .max(Duration::from_secs(lease_seconds))
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
    let info = crate::http::http_client()
        .get(format!("{}/info", container.base_url))
        .send()
        .await
        .context("refresh inline container declaration")?
        .error_for_status()
        .context("inline container /info was not successful")?
        .json::<Value>()
        .await
        .context("decode inline container /info")?;
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
            if let Err(settlement_error) =
                settle_worker_failure(&worker, &worker_run_id, error).await
            {
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
                    None,
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
                None,
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
    provider_usage: Option<&Value>,
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
    let usage = usage_from_records(records, spec.cost_ceiling_usd);
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
    let provider_requests = provider_usage
        .and_then(|usage| usage.get("requestAttempts"))
        .and_then(Value::as_u64);
    let usage_label = if total_tokens > 0 && provider_requests.is_some() {
        format!(
            "{total_tokens} tokens · {} provider requests",
            provider_requests.unwrap_or_default()
        )
    } else if total_tokens > 0 {
        format!("{total_tokens} tokens")
    } else if let Some(provider_requests) = provider_requests {
        format!("{provider_requests} provider requests")
    } else if status == "running" {
        "awaiting telemetry".to_string()
    } else {
        "unavailable".to_string()
    };
    let cost_label = usage
        .cost_usd
        .map(|cost| format!("${cost:.4} / ${:.2}", spec.cost_ceiling_usd))
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
                    {"label": "Overall mean", "value": mean_reward, "detail": "missing rewards stay missing"},
                    {"label": "Provider requests", "value": provider_requests, "detail": "authoritative Workshop proxy request attempts across the run"}
                ],
                "providerUsage": provider_usage,
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
                        return Err(error).context(
                            "inline rollout task could not be joined during cancellation",
                        );
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
            row.pointer("/raw/evaluatorOutcome/status")
                .or_else(|| row.pointer("/evaluatorOutcome/status"))
                .and_then(Value::as_str)
                == Some("failed")
        })
        .count();
    let missing_measurements = records
        .iter()
        .filter(|row| {
            matches!(
                row.pointer("/raw/evaluatorOutcome/reason")
                    .or_else(|| row.pointer("/evaluatorOutcome/reason"))
                    .and_then(Value::as_str),
                Some("evaluator_measurement_missing" | "evaluator_numeric_reward_missing")
            ) || (row.get("reportedStatus").and_then(Value::as_str) == Some("completed")
                && !has_evaluator_measurement(&json!({
                    "reward": row.get("reward").cloned().unwrap_or(Value::Null),
                    "metrics": row.get("metrics").cloned().unwrap_or(Value::Null),
                })))
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
        (
            "calls".into(),
            json!(receipt.calls.saturating_sub(current.calls)),
        ),
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
            vec![
                OptimizerEventDraft::new("optimizer.usage.reconciled", EVAL_ALGORITHM_ID)
                    .idempotency_key("eval:usage:provider:reconciled")
                    .item(item.clone())
                    .usage_delta(usage_delta)
                    .raw(json!({
                        "source": "workshop_secrets_proxy",
                        "receiptDigest": item["receiptDigest"],
                    })),
            ],
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
            Some((prompt, completion))
                if prompt >= span_prompt && completion >= span_completion =>
            {
                (
                    prompt - span_prompt,
                    completion - span_completion,
                    "reconciled",
                )
            }
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
        let trace_kind =
            if record.get("evidenceState").and_then(Value::as_str) == Some("sealed_partial") {
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
    // Harbor's Codex adapter uses the Responses API, while other supported
    // eval clients still use Chat Completions. Grant only the operation the
    // declared adapter uses; widening every workspace recipe to both routes
    // would violate least privilege.
    let mut models = Vec::new();
    if !spec.model.is_empty() {
        models.push(spec.model.clone());
        if let Some(model) = spec.model.strip_prefix("openai/") {
            models.push(model.to_string());
        }
        models.sort();
        models.dedup();
    }
    // The exact Codex SWE policy runs Luna with high reasoning. Bind that
    // workload-owned setting into the same narrow capability; every other
    // recipe keeps its declared effort.
    let exact_codex_swe_pin = spec.harness.eq_ignore_ascii_case("openrouter")
        && spec.policy_config == "codex-cli-openrouter-swe-proxy-v1"
        && spec.provider.eq_ignore_ascii_case("openrouter")
        && spec.model == "openai/gpt-5.6-luna";
    let reasoning_efforts = if exact_codex_swe_pin {
        vec!["high".to_string()]
    } else {
        spec.policy
            .get("reasoning_effort")
            .or_else(|| spec.policy.get("effort"))
            .and_then(Value::as_str)
            .map(|value| vec![value.to_string()])
            .unwrap_or_default()
    };
    let operations = if exact_codex_swe_pin {
        vec!["responses.create".into()]
    } else {
        vec!["chat.completions.create".into()]
    };
    super::admission::provider_use_policy_from_bounds(
        operations,
        models,
        reasoning_efforts,
        total_calls.min(u32::MAX as u64) as u32,
        (spec.cost_ceiling_usd * 1_000_000.0).round().max(0.0) as u64,
        crate::limits::SECRETS_CAPABILITY_TTL.as_secs(),
        input_tokens,
        (output_per_call > 0).then(|| total_calls.saturating_mul(output_per_call)),
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
                .context("evaluation run has no durable kernel projection")?;
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
            .and_then(|ledger| {
                ledger
                    .iter()
                    .find(|entry| entry.work_item_id == work_item_id)
            });
        let rollout_id = record
            .get("rolloutId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| ledger_identity.and_then(|entry| entry.rollout_id.clone()))
            .with_context(|| {
                format!(
                    "terminal record and durable trial ledger for seed {seed} have no rolloutId"
                )
            })?;
        let trial_id = record
            .get("trialId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| ledger_identity.and_then(|entry| entry.trial_id.clone()))
            .with_context(|| {
                format!("terminal record and durable trial ledger for seed {seed} have no trialId")
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
        let frame_mode =
            if record.get("evidenceState").and_then(Value::as_str) == Some("sealed_partial") {
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
        let trace_ref = imported_trace
            .get("traceId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .with_context(|| {
                format!("sealed bundle for rollout `{rollout_id}` indexed no trace")
            })?;
        let trace_digest = imported_trace
            .get("digest")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|digest| valid_sha256_digest(digest))
            .with_context(|| {
                format!("sealed bundle for rollout `{rollout_id}` indexed a trace without an immutable digest")
            })?
            .to_string();
        if trace_ref != producer_trace_id {
            bail!(
                "trace_identity_mismatch: rollout `{rollout_id}` declared `{producer_trace_id}` but the immutable bundle indexed `{trace_ref}`"
            );
        }
        record
            .as_object_mut()
            .with_context(|| format!("terminal record for seed {seed} is not an object"))?
            .insert("sealedTrace".into(), imported);
        attach_reported_facts(record);
        evidence_refs.push(json!({
            "kind": "trace",
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
    persist_progress(
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
            Some("scored") => {
                row.pointer("/evaluatorOutcome/reward")
                    .and_then(Value::as_f64)
                    .is_some_and(f64::is_finite)
                    || has_evaluator_measurement(row)
            }
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
    let provider_usage =
        crate::secrets::live().and_then(|secrets| secrets.provider_usage_for_run(run_id));
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
    let provider_usage_for_summary = provider_usage.clone();
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
            if let Some(provider_usage) = provider_usage_for_summary.clone() {
                summary.insert("providerUsage".into(), provider_usage);
            }
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
                    "provider": provider_usage_for_summary,
                }),
            );
            run.summary = Value::Object(summary);
            run.usage = usage_with_authoritative_provider_receipt(usage, &run.usage);
            Ok(())
        })
        .await?;

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
                    provider_usage.as_ref(),
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
                    None,
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

fn complete_usage_u64_sum(usage: &Value, read: impl Fn(&Value) -> Option<u64>) -> Option<u64> {
    let lanes = usage_lanes(usage)?;
    if lanes.is_empty() {
        return None;
    }
    lanes.into_iter().try_fold(0_u64, |sum, lane| {
        read(lane).and_then(|value| sum.checked_add(value))
    })
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
    let steps_source =
        if record.get("stepsSource").and_then(Value::as_str) == Some("retained_event_log") {
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
            "seed": example.seed,
            "policy_ref": { "harness": spec.harness, "config": spec.policy_config },
            "max_steps": spec.maximum_steps_per_rollout,
            "require_trace_v5": true,
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
        state = poll_until_terminal(
            ctx.client,
            ctx.base,
            &rollout_id,
            spec.terminal_poll_timeout(),
        )
        .await?;
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
                .and_then(|error| {
                    error.as_str().map(str::to_string).or_else(|| {
                        error
                            .get("detail")
                            .or_else(|| error.get("reason"))
                            .or_else(|| error.get("message"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                })
                .or_else(|| state.get("reason").and_then(Value::as_str).map(str::to_string))
                .or_else(|| state.get("detail").and_then(Value::as_str).map(str::to_string))
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
    timeout: Duration,
) -> Result<Value> {
    let deadline = Instant::now() + timeout;
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
                // the same bounded timeout as the rollout's blocking HTTP
                // request. Long-running harnesses can legitimately outlive a
                // generic UI request timeout.
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
            .with_context(|| {
                format!("terminal reward evaluation failed for rollout `{rollout_id}`")
            })?
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
        "/capabilities/protocol",
        "/capabilities/optimizer_contracts/gepa/version",
        "/optimizer_contracts/gepa/version",
        "/metadata/optimizer_contracts/gepa/version",
        "/info/capabilities/optimizer_contracts/gepa/version",
        "/info/optimizer_contracts/gepa/version",
        "/info/metadata/optimizer_contracts/gepa/version",
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

