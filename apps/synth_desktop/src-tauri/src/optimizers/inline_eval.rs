//! Live bridge for inline-first evaluation admission.
//!
//! This module is the only adapter from mutable discovery records into the
//! immutable admission model. Execution accepts `ApprovedExecutionSpec`, never
//! an unvalidated request or arbitrary JSON.

use super::admission::{
    self, ApprovalBinding, ApprovalReceiptId, ContainerCandidate, ContainerId,
    ContainerRegistrationId, DeclarationDigest, DeclaredEvaluator, DiscoveryContext,
    EvalDeclaration, EvaluatorId, InlineRequest, ModelPin, PolicyResolution, PolicyRevision,
    RecipeSource, RolloutCount, SourceRevision,
};
use super::models::OptimizerRunRecord;
use super::{container_eval, OptimizerService};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::num::NonZeroU32;
use std::path::{Component, Path};
use std::process::Command;

/// Resolve current authority, construct the default inline recipe, validate it,
/// and assign its immutable digest. No recipe catalog is consulted here.
pub async fn admit_inline(
    service: &OptimizerService,
    request: InlineRequest,
) -> Result<admission::AdmissibleExecutionSpec> {
    let (context, normalized) = discovery_context(service, request).await?;
    let recipe =
        admission::pipeline::draft_inline(&normalized, &context).map_err(anyhow::Error::new)?;
    admission::materialize(RecipeSource::Inline(recipe), &context)
        .and_then(|draft| draft.validate())
        .and_then(|validated| validated.admit())
        .map_err(anyhow::Error::new)
}

/// Bind the host approval receipt to the exact admitted specification.
pub fn bind_approval(
    admissible: admission::AdmissibleExecutionSpec,
    receipt_id: &str,
) -> Result<admission::ApprovedExecutionSpec> {
    let recipe = &admissible.spec().recipe;
    let binding = ApprovalBinding {
        receipt_id: ApprovalReceiptId::new(receipt_id)?,
        execution_spec_digest: admissible.digest().clone(),
        container_declaration_digest: recipe.container.declaration_digest.clone(),
        policy_revision: recipe.policy.revision.clone(),
        policy_configuration_digest: recipe.policy.configuration_digest.clone(),
        approved_cost_micros: recipe.resource_limits.hard_total_cost_micros,
        approved_rollouts: recipe.rollout_plan.maximum_rollouts,
    };
    admissible.approve(binding).map_err(anyhow::Error::new)
}

/// Dispatch an approved inline evaluation through the existing evidence worker.
pub async fn execute(
    service: &OptimizerService,
    approved: admission::ApprovedExecutionSpec,
    session_ref: Option<String>,
) -> Result<(OptimizerRunRecord, Option<crate::storage::AppEvent>)> {
    container_eval::start_inline(service, approved, session_ref).await
}

/// Re-read the exact declaration and policy revision immediately before spend.
pub async fn reverify(
    service: &OptimizerService,
    approved: &admission::ApprovedExecutionSpec,
) -> Result<()> {
    let recipe = approved.recipe();
    let request = InlineRequest {
        container_id: Some(recipe.container.container_id.clone()),
        family: None,
        policy_namespace: Some(recipe.policy.namespace.clone()),
        policy_name: Some(recipe.policy.name.clone()),
        policy_source_path: None,
        policy_overrides: Some(recipe.policy.configuration.clone()),
        provider: Some(recipe.model.provider.clone()),
        model_id: Some(recipe.model.model_id.clone()),
        seeds: recipe.rollout_plan.seeds.clone(),
        maximum_rollouts: Some(recipe.rollout_plan.maximum_rollouts.0.get()),
        maximum_model_calls_per_rollout: Some(
            recipe
                .resource_limits
                .maximum_model_calls_per_rollout
                .0
                .get(),
        ),
        maximum_steps_per_rollout: Some(recipe.resource_limits.maximum_steps_per_rollout.0.get()),
        hard_total_cost_usd: Some(
            recipe.resource_limits.hard_total_cost_micros.as_micros() as f64 / 1_000_000.0,
        ),
        evaluator: None,
    };
    let (context, _) = discovery_context(service, request).await?;
    let container = context
        .containers
        .first()
        .context("approved container disappeared during pre-spend verification")?;
    let declaration_digest = container
        .declaration
        .declaration_digest
        .as_ref()
        .context("current container declaration has no digest")?;
    let policy_revision = context
        .policy
        .as_ref()
        .and_then(|policy| policy.revision.as_ref())
        .context("current policy revision is unresolved")?;
    approved
        .reverify(declaration_digest, policy_revision)
        .map_err(anyhow::Error::new)
}

async fn discovery_context(
    service: &OptimizerService,
    mut request: InlineRequest,
) -> Result<(DiscoveryContext, InlineRequest)> {
    let requested_id = request
        .container_id
        .as_ref()
        .map(ContainerId::as_str)
        .map(str::to_owned);
    let family = request.family.clone();
    let rows = service
        .database()
        .clone()
        .run(move |conn| {
            let mut statement = conn.prepare(
                "SELECT id, status, task_family, metadata_json, base_url FROM containers ORDER BY updated_at DESC, id",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await?;

    let mut candidates = Vec::new();
    let mut policy_revisions = Vec::new();
    let mut policy_source_code = None;
    let mut selected_base_url = None;
    for (id, status, task_family, metadata_json, base_url) in rows {
        let metadata: Value = serde_json::from_str(&metadata_json)
            .with_context(|| format!("decode container declaration for `{id}`"))?;
        let spec_id = metadata.get("specId").and_then(Value::as_str);
        if requested_id
            .as_deref()
            .is_some_and(|wanted| wanted != id && Some(wanted) != spec_id)
        {
            continue;
        }
        if requested_id.is_none()
            && family
                .as_deref()
                .is_some_and(|wanted| task_family.as_deref() != Some(wanted))
        {
            continue;
        }
        let policy_revision = metadata
            .pointer("/capabilities/revision")
            .or_else(|| metadata.get("gitRevision"))
            .and_then(Value::as_str)
            .context("container declaration has no immutable policy revision")?;
        let source_revision = metadata
            .get("gitRevision")
            .and_then(Value::as_str)
            .context("container declaration has no immutable source revision")?;
        policy_revisions.push(PolicyRevision::new(policy_revision)?);
        let declared_policy_source = request.policy_source_path.as_deref().or_else(|| {
            metadata
                .get("policySourcePath")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
        });
        if let Some(relative) = declared_policy_source {
            policy_source_code = Some(read_policy_source_at_revision(
                &metadata,
                source_revision,
                relative,
            )?);
        }
        candidates.push(container_candidate(
            &id,
            &status,
            task_family,
            &metadata,
            &request,
        )?);
        if selected_base_url.is_none() {
            selected_base_url = base_url;
        }
    }

    let selected = candidates
        .first()
        .context("no matching registered container")?;
    request.container_id = Some(selected.container_id.clone());
    materialize_seed_instances(
        selected_base_url
            .as_deref()
            .context("registered container has no base URL")?,
        &request.seeds,
    )
    .await?;
    let revision = policy_revisions
        .into_iter()
        .next()
        .context("matching container has no policy revision")?;
    let declared_configuration = request.policy_overrides.clone().unwrap_or_else(|| {
        admission::CanonicalJson::new(json!({})).expect("empty object is canonical")
    });
    // The supplied object is the complete declared configuration for an inline
    // policy pin. It is not applied twice as an override.
    request.policy_overrides = None;
    let namespace = request
        .policy_namespace
        .clone()
        .context("inline evaluation requires policyNamespace")?;
    let name = request
        .policy_name
        .clone()
        .context("inline evaluation requires policyName")?;
    let provider = request
        .provider
        .clone()
        .context("inline evaluation requires provider")?;
    let scope = admission::CredentialCapabilityScope::new(["provider.request".to_string()], 3_600);
    Ok((
        DiscoveryContext {
            containers: candidates,
            policy: Some(PolicyResolution {
                namespace,
                name,
                revision: Some(revision),
                declared_configuration,
                source_code: policy_source_code,
            }),
            credential_route_available: crate::secrets::live().is_some(),
            credential_route_detail: Some(format!(
                "Workshop secrets proxy has no active route for `{}`",
                provider.as_str()
            )),
            credential_capability_scope: Some(scope),
            catalog: Vec::new(),
        },
        request,
    ))
}

async fn materialize_seed_instances(base_url: &str, seeds: &[admission::Seed]) -> Result<()> {
    anyhow::ensure!(
        !seeds.is_empty(),
        "inline evaluation requires at least one seed"
    );
    let client = crate::http::http_client_builder().build()?;
    let task = client
        .get(format!("{}/task_info", base_url.trim_end_matches('/')))
        .send()
        .await
        .context("GET /task_info before task-instance materialization")?
        .error_for_status()
        .context("task_info_unavailable")?
        .json::<Value>()
        .await
        .context("task_info_invalid")?;
    let task_id = task
        .get("id")
        .or_else(|| task.get("task_id"))
        .and_then(Value::as_str)
        .context("task_info_invalid: task id missing")?;
    let requested = seeds.iter().map(|seed| seed.0).collect::<Vec<_>>();
    let response = client
        .post(format!(
            "{}/task_instances/materialize",
            base_url.trim_end_matches('/')
        ))
        .json(&json!({"task_id": task_id, "seeds": requested}))
        .send()
        .await
        .context("POST /task_instances/materialize")?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        anyhow::bail!("task_instance_materialization_failed: {status} {detail}");
    }
    let payload = response
        .json::<Value>()
        .await
        .context("task_instance_materialization_invalid")?;
    let instances = payload
        .get("instances")
        .and_then(Value::as_array)
        .context("task_instance_materialization_invalid: instances missing")?;
    anyhow::ensure!(
        instances.len() == seeds.len(),
        "task_instance_materialization_incomplete: expected {}, received {}",
        seeds.len(),
        instances.len()
    );
    for (seed, instance) in seeds.iter().zip(instances) {
        let expected = format!("{task_id}:seed:{}", seed.0);
        anyhow::ensure!(
            instance.get("task_instance_id").and_then(Value::as_str) == Some(expected.as_str())
                && instance.get("seed").and_then(Value::as_i64) == Some(seed.0),
            "task_instance_identity_mismatch: expected {expected}"
        );
    }
    Ok(())
}

fn read_policy_source_at_revision(
    metadata: &Value,
    revision: &str,
    relative: &str,
) -> Result<String> {
    let path = Path::new(relative);
    anyhow::ensure!(
        !path.is_absolute()
            && !path.as_os_str().is_empty()
            && path
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "policySourcePath must be a non-empty repository-relative path without traversal"
    );
    let source_root = metadata
        .get("sourcePath")
        .and_then(Value::as_str)
        .context("container declaration has no sourcePath for policy source resolution")?;
    let output = Command::new("git")
        .arg("-C")
        .arg(source_root)
        .arg("show")
        .arg(format!("{revision}:{relative}"))
        .output()
        .context("read policy source from declared git revision")?;
    anyhow::ensure!(
        output.status.success(),
        "policySourcePath `{relative}` is absent from declared source revision `{revision}`: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    String::from_utf8(output.stdout).context("policy source is not UTF-8")
}

fn container_candidate(
    id: &str,
    status: &str,
    family: Option<String>,
    metadata: &Value,
    request: &InlineRequest,
) -> Result<ContainerCandidate> {
    let protocol = metadata
        .pointer("/capabilities/protocol")
        .or_else(|| metadata.pointer("/info/capabilities/protocol"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let declaration_digest = metadata
        .get("manifestHash")
        .and_then(Value::as_str)
        .map(DeclarationDigest::new)
        .transpose()?;
    let source_revision = metadata
        .get("gitRevision")
        .or_else(|| metadata.pointer("/capabilities/revision"))
        .and_then(Value::as_str)
        .context("container declaration has no source revision")?;
    let evaluator_id = metadata
        .pointer("/info/logical_service_ids/evaluator")
        .and_then(Value::as_str)
        .context("container did not declare an evaluator identity")?;
    let evaluator_version = metadata
        .pointer("/info/evaluation_plan_ref")
        .and_then(Value::as_str)
        .context("container did not declare an evaluation plan")?;
    let scoring = admission::digest_of(&json!({
        "evaluatorId": evaluator_id,
        "evaluationPlanRef": evaluator_version,
        "rewardAuthority": metadata.pointer("/info/reward_authority"),
        "contractVersion": metadata.pointer("/info/capabilities/contract_version"),
    }))?;
    let operations = metadata
        .pointer("/capabilities/operations")
        .and_then(Value::as_object)
        .map(|values| {
            values
                .iter()
                .filter(|(_, value)| {
                    value.as_str() == Some("supported") || value.as_bool() == Some(true)
                })
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let bind_policy_config = metadata
        .pointer("/info/affordance_booleans/environment/bind_policy_config")
        .and_then(Value::as_bool)
        == Some(true);
    let supported_models = if bind_policy_config {
        request
            .provider
            .clone()
            .zip(request.model_id.clone())
            .map(|(provider, model_id)| vec![ModelPin { provider, model_id }])
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let maximum_steps = metadata
        .pointer("/info/max_episode_steps")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let maximum_rollouts = metadata
        .pointer("/info/scale_leases")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    Ok(ContainerCandidate {
        container_id: ContainerId::new(id)?,
        registration_id: ContainerRegistrationId::new(id)?,
        source_revision: SourceRevision::new(source_revision)?,
        health: status.to_owned(),
        family,
        declaration: EvalDeclaration {
            protocol,
            declaration_digest,
            evaluator: Some(DeclaredEvaluator {
                evaluator_id: EvaluatorId::new(evaluator_id)?,
                evaluator_version: evaluator_version.to_owned(),
                scoring_digest: scoring,
            }),
            supported_models,
            supports_seed_control: Some(bind_policy_config),
            maximum_rollouts,
            maximum_model_calls_per_rollout: request.maximum_model_calls_per_rollout,
            maximum_steps_per_rollout: maximum_steps,
            operations,
        },
    })
}

pub fn approved_rollouts(value: u32) -> Result<RolloutCount> {
    Ok(RolloutCount(
        NonZeroU32::new(value).context("approved rollout cap must be non-zero")?,
    ))
}
