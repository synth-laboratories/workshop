//! The admission pipeline: materialize, validate and pin, canonicalize, approve.
//!
//! Each stage is its own type. That is the whole mechanism: `execute` takes an
//! [`ApprovedExecutionSpec`], and there is no constructor for one that does not
//! run through validation, hashing, and an approval binding. An unvalidated
//! draft is not "discouraged" at the executor — it is not expressible.
//!
//! Nothing here reads the recipe catalog unless the caller handed in a
//! [`RecipeSource::Catalog`]. Inline is not a fallback for a catalog miss; it
//! is the ordinary path.

use super::canonical::CanonicalJson;
use super::error::{AdmissionError, AdmissionErrorCode};
use super::ids::{
    non_zero_u32, ApprovalReceiptId, ContainerId, ContainerRegistrationId, CostMicros,
    DeclarationDigest, Digest, EvaluatorId, ModelCallCount, ModelId, PolicyRevision, ProviderId,
    RecipeId, RolloutCount, Seed, SourceRevision, StepCount,
};
use super::spec::{
    ApprovalBinding, ContainerPin, CredentialCapabilityScope, CredentialRoute, EvaluatorSpec,
    ExecutionSpec, InlineRecipe, LiveEvalProtocol, ModelPin, OutputContract, PolicyMaterialRef,
    PolicyPin, RecipeSource, RecipeSourceKind, ResourceLimits, RolloutPlan,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// What a conversational request explicitly supplied.
///
/// Every field a specification needs but a request may omit is `Option`, and
/// each omission has its own named error. There is no "sensible default" layer
/// between this and a draft: a default would be Workshop choosing a seed, a
/// model, or a spending limit on the operator's behalf.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InlineRequest {
    /// Exact container id, when the user named one. Absent means discovery
    /// must resolve to exactly one candidate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_id: Option<ContainerId>,
    /// Narrowing hint used only when no container id was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_name: Option<String>,
    /// Repository-relative policy source to pin from the container's declared
    /// source revision. The path is explicit; Workshop never guesses one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_source_path: Option<String>,
    /// Explicit overrides layered onto the policy declaration's own values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_overrides: Option<CanonicalJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<ModelId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seeds: Vec<Seed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_rollouts: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_model_calls_per_rollout: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_steps_per_rollout: Option<u32>,
    /// The hard ceiling, in US dollars as a human supplies it. Converted to
    /// integer micros at draft time and compared only as micros thereafter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_total_cost_usd: Option<f64>,
    /// An explicitly requested evaluator, when the caller is not taking the
    /// container's declared one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator: Option<RequestedEvaluator>,
}

impl InlineRequest {
    /// Parse an MCP/IPC body. Host identity fields are stripped so they cannot
    /// travel as unknown specification fields, and unknown cost/limit names fail
    /// at this boundary instead of being dropped.
    pub fn from_tool_arguments(body: Value) -> Result<Self, serde_json::Error> {
        let mut value = body.get("request").cloned().unwrap_or(body);
        if let Some(object) = value.as_object_mut() {
            for field in [
                "sessionRef",
                "session_ref",
                "openVisual",
                "open_visual",
                "idempotencyKey",
                "idempotency_key",
                "request",
            ] {
                object.remove(field);
            }
        }
        serde_json::from_value(value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestedEvaluator {
    pub evaluator_id: EvaluatorId,
    pub configuration: CanonicalJson,
}

/// One container that discovery returned, together with the declaration read
/// from it. Workshop never asserts a container's capabilities on its behalf;
/// this is a projection of what the service (or an operator declaration) said.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerCandidate {
    pub container_id: ContainerId,
    pub registration_id: ContainerRegistrationId,
    pub source_revision: SourceRevision,
    /// Health as observed, verbatim. `"ready"` is the only admissible value;
    /// anything else, including an unknown string, fails closed.
    pub health: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    pub declaration: EvalDeclaration,
}

impl ContainerCandidate {
    pub fn is_ready(&self) -> bool {
        self.health.trim().eq_ignore_ascii_case("ready")
    }
}

/// What a container declares about running live evaluations.
///
/// Tri-state where it matters: `Option<bool>` for seed control distinguishes
/// "declared unsupported" from "never said", and both fail closed but with
/// different remediation. Numeric ceilings are `Option` because an absent
/// ceiling means the container did not declare one, not that it is unlimited.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalDeclaration {
    /// The advertised protocol string, verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// Digest over the declaration itself, so a changed declaration is
    /// detectable after approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration_digest: Option<DeclarationDigest>,
    /// The evaluator the container owns, when it declares one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator: Option<DeclaredEvaluator>,
    /// Provider/model pairs the container will accept.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_models: Vec<ModelPin>,
    /// Whether the container accepts caller-supplied seeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_seed_control: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_rollouts: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_model_calls_per_rollout: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_steps_per_rollout: Option<u32>,
    /// Normalized operation wire names the container advertises as supported.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operations: Vec<String>,
}

/// A container's own scoring declaration. All three fields are required: an
/// evaluator without a version or a scoring digest cannot be pinned, and a run
/// scored by an unpinned evaluator is not reproducible.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredEvaluator {
    pub evaluator_id: EvaluatorId,
    pub evaluator_version: String,
    pub scoring_digest: Digest,
}

/// A resolved policy: its immutable revision and the configuration it declares.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyResolution {
    pub namespace: String,
    pub name: String,
    /// `None` means the policy source is still mutable — the pin is impossible
    /// and admission fails rather than pinning "latest".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<PolicyRevision>,
    pub declared_configuration: CanonicalJson,
    /// Exact source bytes read from the declared immutable source revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<PolicyMaterialRef>,
}

/// Everything admission is allowed to read. Passing this explicitly, rather
/// than reaching for ambient state, is what makes the rules testable and keeps
/// an unrelated global from becoming an authority.
#[derive(Clone, Debug, Default)]
pub struct DiscoveryContext {
    /// Containers discovery returned for this request.
    pub containers: Vec<ContainerCandidate>,
    /// The policy the request named, once resolved. `None` means not found.
    pub policy: Option<PolicyResolution>,
    /// Whether the Workshop secrets proxy can currently route this provider.
    pub credential_route_available: bool,
    /// Detail to report when the route is unavailable.
    pub credential_route_detail: Option<String>,
    /// The capability scope a minted credential would carry.
    pub credential_capability_scope: Option<CredentialCapabilityScope>,
    /// Catalog recipes, consulted only for an explicit catalog request.
    pub catalog: Vec<CatalogEntry>,
}

/// A catalog recipe, already expressed as the same inline shape. The catalog is
/// a store of presets, so a preset is just a specification somebody saved.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogEntry {
    pub recipe_id: RecipeId,
    pub digest: Digest,
    pub recipe: InlineRecipe,
}

// ---------------------------------------------------------------------------
// Stage 1: materialize
// ---------------------------------------------------------------------------

/// A specification that has been assembled but not yet checked.
///
/// Deliberately not constructible from outside this module except through
/// [`materialize`], so "I already have a draft" always means "materialize
/// produced it".
#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionSpecDraft {
    spec: ExecutionSpec,
}

impl ExecutionSpecDraft {
    pub fn spec(&self) -> &ExecutionSpec {
        &self.spec
    }

    pub fn source_kind(&self) -> RecipeSourceKind {
        self.spec.source_kind
    }
}

/// Bring either source to the one draft shape.
///
/// The catalog is queried only for [`RecipeSource::Catalog`]. An explicit
/// catalog request that misses fails with `catalog_recipe_not_found`; it is
/// never quietly converted into an inline specification, because the caller
/// asked for a specific reviewed preset and silently running something else is
/// the substitution this design forbids.
pub fn materialize(
    source: RecipeSource,
    context: &DiscoveryContext,
) -> Result<ExecutionSpecDraft, AdmissionError> {
    match source {
        RecipeSource::Inline(recipe) => Ok(ExecutionSpecDraft {
            spec: ExecutionSpec {
                source_kind: RecipeSourceKind::Inline,
                catalog_recipe_id: None,
                recipe,
            },
        }),
        RecipeSource::Catalog(reference) => {
            let entry = context
                .catalog
                .iter()
                .find(|entry| entry.recipe_id == reference.recipe_id)
                .ok_or_else(|| {
                    AdmissionError::catalog_recipe_not_found(
                        &reference.recipe_id,
                        context.catalog.len(),
                    )
                })?;
            if let Some(expected) = &reference.expected_digest {
                if expected.as_str() != entry.digest.as_str() {
                    return Err(AdmissionError::new(
                        AdmissionErrorCode::ExecutionSpecDigestMismatch,
                        format!(
                            "catalog recipe `{}` is at a different revision than the one pinned",
                            reference.recipe_id
                        ),
                        "Re-read the catalog recipe and pin its current digest, or drop the \
                         expected digest to accept the catalog's current revision.",
                    )
                    .with_context(json!({
                        "recipeId": reference.recipe_id,
                        "expectedDigest": expected.as_str(),
                        "actualDigest": entry.digest.as_str(),
                    })));
                }
            }
            Ok(ExecutionSpecDraft {
                spec: ExecutionSpec {
                    source_kind: RecipeSourceKind::Catalog,
                    catalog_recipe_id: Some(entry.recipe_id.clone()),
                    recipe: entry.recipe.clone(),
                },
            })
        }
    }
}

/// Build an inline recipe from a request plus what discovery found.
///
/// This is where every construction rule lives. It derives only facts an
/// authoritative source actually supplied, and refuses ambiguity rather than
/// resolving it.
pub fn draft_inline(
    request: &InlineRequest,
    context: &DiscoveryContext,
) -> Result<InlineRecipe, AdmissionError> {
    let container = select_container(request, context)?;
    let declaration = &container.declaration;

    if !container.is_ready() {
        return Err(AdmissionError::container_unhealthy(
            &container.container_id,
            &container.health,
        ));
    }

    // -- protocol: container declaration only --------------------------------
    let protocol = declaration
        .protocol
        .as_deref()
        .and_then(LiveEvalProtocol::parse)
        .ok_or_else(|| {
            AdmissionError::container_protocol_unsupported(
                &container.container_id,
                super::spec::LIVE_EVAL_PROTOCOL_V1,
                declaration.protocol.as_deref(),
            )
        })?;

    let declaration_digest = declaration.declaration_digest.clone().ok_or_else(|| {
        AdmissionError::scoring_contract_invalid(
            &container.container_id,
            None,
            "the container's capability declaration carries no digest, so it cannot be pinned",
        )
    })?;

    // -- evaluator: container declaration or explicit request ----------------
    let evaluator =
        match &request.evaluator {
            Some(requested) => {
                // An explicitly named evaluator still needs a scoring digest, and
                // Workshop will not compute one over configuration the container
                // has not agreed to.
                let declared = declaration.evaluator.as_ref().ok_or_else(|| {
                    AdmissionError::evaluator_not_declared(&container.container_id)
                })?;
                if declared.evaluator_id != requested.evaluator_id {
                    return Err(AdmissionError::scoring_contract_invalid(
                        &container.container_id,
                        Some(&requested.evaluator_id),
                        format!(
                            "container declares evaluator `{}`, not `{}`",
                            declared.evaluator_id, requested.evaluator_id
                        ),
                    ));
                }
                EvaluatorSpec::Explicit {
                    evaluator_id: requested.evaluator_id.clone(),
                    configuration: requested.configuration.clone(),
                    scoring_digest: declared.scoring_digest.clone(),
                }
            }
            None => {
                let declared = declaration.evaluator.as_ref().ok_or_else(|| {
                    AdmissionError::evaluator_not_declared(&container.container_id)
                })?;
                if declared.evaluator_version.trim().is_empty() {
                    return Err(AdmissionError::scoring_contract_invalid(
                        &container.container_id,
                        Some(&declared.evaluator_id),
                        "the declared evaluator carries no version",
                    ));
                }
                EvaluatorSpec::ContainerDeclared {
                    evaluator_id: declared.evaluator_id.clone(),
                    evaluator_version: declared.evaluator_version.clone(),
                    scoring_digest: declared.scoring_digest.clone(),
                }
            }
        };

    // -- policy: name from the request, revision from the resolved source ----
    let namespace = request
        .policy_namespace
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AdmissionError::execution_spec_invalid("no policy namespace was supplied")
        })?;
    let name = request
        .policy_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AdmissionError::execution_spec_invalid("no policy name was supplied"))?;
    let resolution = context
        .policy
        .as_ref()
        .filter(|policy| policy.namespace == namespace && policy.name == name)
        .ok_or_else(|| AdmissionError::policy_not_found(namespace, name))?;
    let revision = resolution
        .revision
        .clone()
        .ok_or_else(|| AdmissionError::policy_revision_unresolved(namespace, name))?;
    let configuration = merge_policy_configuration(
        &resolution.declared_configuration,
        request.policy_overrides.as_ref(),
        namespace,
        name,
    )?;
    let mut policy = PolicyPin::new(namespace, name, revision, configuration)
        .with_source_code(resolution.source_code.clone());
    if let Some(material) = resolution.material.clone() {
        if let Some(source_code) = resolution.source_code.clone() {
            policy = policy.with_material(material, source_code);
        } else {
            policy.source_digest = Some(material.content_digest.clone());
            policy.material = Some(material);
        }
    }

    // -- model: request only, checked against the declaration ----------------
    let provider = request.provider.clone().ok_or_else(|| {
        AdmissionError::execution_spec_invalid("no inference provider was supplied")
    })?;
    let model_id = request
        .model_id
        .clone()
        .ok_or_else(|| AdmissionError::execution_spec_invalid("no model was supplied"))?;
    let model = ModelPin {
        provider: provider.clone(),
        model_id: model_id.clone(),
    };
    if declaration.supported_models.is_empty() {
        // No advertised model set is not permission to run anything. It means
        // the container never said, and admission cannot verify the pairing.
        return Err(AdmissionError::model_unsupported(
            &provider,
            &model_id,
            &container.container_id,
        ));
    }
    if !declaration.supported_models.contains(&model) {
        return Err(AdmissionError::model_unsupported(
            &provider,
            &model_id,
            &container.container_id,
        ));
    }

    // -- seeds and rollout plan ----------------------------------------------
    if request.seeds.is_empty() {
        return Err(AdmissionError::execution_spec_invalid(
            "no seeds were supplied; a rollout plan with no seeds declares no work",
        ));
    }
    if declaration.supports_seed_control != Some(true) {
        return Err(AdmissionError::seed_control_unsupported(
            &container.container_id,
        ));
    }
    let requested_rollouts = request.maximum_rollouts.ok_or_else(|| {
        AdmissionError::execution_spec_invalid("no maximum rollout count was supplied")
    })?;
    check_ceiling(
        "maximum_rollouts",
        requested_rollouts,
        declaration.maximum_rollouts,
    )?;
    if (requested_rollouts as usize) < request.seeds.len() {
        return Err(AdmissionError::requested_limit_unsupported(
            "maximum_rollouts",
            requested_rollouts as u64,
            Some(request.seeds.len() as u64),
        ));
    }
    let rollout_plan = RolloutPlan {
        seeds: request.seeds.clone(),
        maximum_rollouts: RolloutCount(
            non_zero_u32(requested_rollouts, "maximum_rollouts")
                .map_err(|error| AdmissionError::execution_spec_invalid(error.to_string()))?,
        ),
    };

    // -- resource limits ------------------------------------------------------
    let calls = request.maximum_model_calls_per_rollout.ok_or_else(|| {
        AdmissionError::execution_spec_invalid("no per-rollout model-call limit was supplied")
    })?;
    check_ceiling(
        "maximum_model_calls_per_rollout",
        calls,
        declaration.maximum_model_calls_per_rollout,
    )?;
    let steps = request.maximum_steps_per_rollout.ok_or_else(|| {
        AdmissionError::execution_spec_invalid("no per-rollout step limit was supplied")
    })?;
    check_ceiling(
        "maximum_steps_per_rollout",
        steps,
        declaration.maximum_steps_per_rollout,
    )?;
    let ceiling = request
        .hard_total_cost_usd
        .ok_or_else(AdmissionError::cost_ceiling_required)?;
    let hard_total_cost_micros = CostMicros::from_usd(ceiling).map_err(|error| {
        AdmissionError::new(
            AdmissionErrorCode::CostCeilingRequired,
            format!("the supplied cost ceiling is unusable: {error}"),
            "Supply a positive hard total cost ceiling in the request.",
        )
    })?;
    let resource_limits = ResourceLimits {
        maximum_model_calls_per_rollout: ModelCallCount(
            non_zero_u32(calls, "maximum_model_calls_per_rollout")
                .map_err(|error| AdmissionError::execution_spec_invalid(error.to_string()))?,
        ),
        maximum_steps_per_rollout: StepCount(
            non_zero_u32(steps, "maximum_steps_per_rollout")
                .map_err(|error| AdmissionError::execution_spec_invalid(error.to_string()))?,
        ),
        hard_total_cost_micros,
    };

    // -- credential route -----------------------------------------------------
    if !context.credential_route_available {
        return Err(AdmissionError::credential_route_unavailable(
            &provider,
            context
                .credential_route_detail
                .clone()
                .unwrap_or_else(|| "the Workshop secrets proxy is not running".into()),
        ));
    }
    let capability_scope = context.credential_capability_scope.clone().ok_or_else(|| {
        AdmissionError::credential_route_unavailable(
            &provider,
            "no credential capability scope was offered for this route",
        )
    })?;
    let credential_route = CredentialRoute::WorkshopSecretsProxy {
        provider: provider.clone(),
        capability_scope,
    };

    // -- output contract: container declaration ------------------------------
    let output_contract = OutputContract::new(
        true,
        declaration
            .operations
            .iter()
            .any(|op| op == "trace_v5.capture"),
        declaration.operations.iter().any(|op| op == "usage.get"),
        declaration.operations.clone(),
    );
    let required = ["rollouts.prepare", "rollouts.start_prepared", "reward.get"];
    let missing: Vec<String> = required
        .iter()
        .filter(|needed| !declaration.operations.iter().any(|op| op == *needed))
        .map(|needed| needed.to_string())
        .collect();
    if !missing.is_empty() {
        return Err(AdmissionError::output_contract_unsupported(
            &container.container_id,
            &missing,
        ));
    }

    Ok(InlineRecipe {
        container: ContainerPin {
            container_id: container.container_id.clone(),
            registration_id: container.registration_id.clone(),
            source_revision: container.source_revision.clone(),
            declaration_digest,
        },
        protocol,
        evaluator,
        policy,
        model,
        rollout_plan,
        resource_limits,
        credential_route,
        output_contract,
    })
}

/// Resolve exactly one container, or refuse.
fn select_container<'a>(
    request: &InlineRequest,
    context: &'a DiscoveryContext,
) -> Result<&'a ContainerCandidate, AdmissionError> {
    if let Some(requested) = &request.container_id {
        return context
            .containers
            .iter()
            .find(|candidate| candidate.container_id == *requested)
            .ok_or_else(|| AdmissionError::container_not_found(requested.as_str()));
    }
    let matches: Vec<&ContainerCandidate> = context
        .containers
        .iter()
        .filter(|candidate| match (&request.family, &candidate.family) {
            (Some(wanted), Some(declared)) => wanted == declared,
            (Some(_), None) => false,
            (None, _) => true,
        })
        .collect();
    match matches.len() {
        0 => Err(AdmissionError::container_not_found(
            request.family.as_deref().unwrap_or("<any>"),
        )),
        1 => Ok(matches[0]),
        // Picking the first discovered container is exactly the silent
        // substitution this refuses.
        _ => Err(AdmissionError::container_selection_ambiguous(
            &matches
                .iter()
                .map(|candidate| candidate.container_id.clone())
                .collect::<Vec<_>>(),
        )),
    }
}

/// Layer explicit overrides onto a policy's declared configuration.
///
/// An override may only replace a key the declaration already defines. Adding
/// an unknown key would be the caller inventing policy semantics, which is not
/// an override — it is a different policy.
fn merge_policy_configuration(
    declared: &CanonicalJson,
    overrides: Option<&CanonicalJson>,
    namespace: &str,
    name: &str,
) -> Result<CanonicalJson, AdmissionError> {
    let Some(overrides) = overrides else {
        return Ok(declared.clone());
    };
    let declared_object = declared.as_value().as_object().ok_or_else(|| {
        AdmissionError::policy_configuration_invalid(
            namespace,
            name,
            "the declared policy configuration is not an object",
        )
    })?;
    let override_object = overrides.as_value().as_object().ok_or_else(|| {
        AdmissionError::policy_configuration_invalid(
            namespace,
            name,
            "the supplied policy overrides are not an object",
        )
    })?;
    let mut merged = declared_object.clone();
    for (key, value) in override_object {
        if !declared_object.contains_key(key) {
            return Err(AdmissionError::policy_configuration_invalid(
                namespace,
                name,
                format!("`{key}` is not a key this policy declares"),
            ));
        }
        merged.insert(key.clone(), value.clone());
    }
    CanonicalJson::new(Value::Object(merged)).map_err(|error| {
        AdmissionError::policy_configuration_invalid(namespace, name, error.to_string())
    })
}

/// A requested limit above a declared ceiling fails. It is never clamped: a run
/// that quietly did less than the operator asked answers a different question.
fn check_ceiling(
    limit: &'static str,
    requested: u32,
    declared: Option<u32>,
) -> Result<(), AdmissionError> {
    match declared {
        Some(maximum) if requested > maximum => Err(AdmissionError::requested_limit_unsupported(
            limit,
            requested as u64,
            Some(maximum as u64),
        )),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Stage 2: validate and pin
// ---------------------------------------------------------------------------

/// A draft whose internal consistency has been checked.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedExecutionSpec {
    spec: ExecutionSpec,
}

impl ExecutionSpecDraft {
    /// Check the invariants that must hold regardless of which source built
    /// the draft. Both paths run this, so a catalog preset gets exactly the
    /// same scrutiny as a freshly authored inline specification.
    pub fn validate(self) -> Result<ValidatedExecutionSpec, AdmissionError> {
        let recipe = &self.spec.recipe;

        if recipe.rollout_plan.seeds.is_empty() {
            return Err(AdmissionError::execution_spec_invalid(
                "the rollout plan declares no seeds",
            ));
        }
        let declared = recipe.rollout_plan.seeds.len();
        let maximum = recipe.rollout_plan.maximum_rollouts.0.get() as usize;
        if declared > maximum {
            return Err(AdmissionError::requested_limit_unsupported(
                "maximum_rollouts",
                maximum as u64,
                Some(declared as u64),
            ));
        }
        // Duplicate seeds would make two rollouts indistinguishable in the
        // evidence, so the plan is rejected rather than deduplicated.
        let mut seen = recipe.rollout_plan.seeds.clone();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        if seen.len() != before {
            return Err(AdmissionError::execution_spec_invalid(
                "the rollout plan repeats a seed; each rollout must be distinguishable",
            ));
        }

        if !recipe.policy.digest_matches() {
            return Err(AdmissionError::policy_configuration_invalid(
                &recipe.policy.namespace,
                &recipe.policy.name,
                "the stored configuration digest does not match the configuration",
            ));
        }

        if recipe.evaluator.evaluator_id().as_str().is_empty() {
            return Err(AdmissionError::execution_spec_invalid(
                "the evaluator carries no id",
            ));
        }

        // The credential route's provider and the model's provider must agree,
        // or the run would authenticate to one service and call another.
        if recipe.credential_route.provider() != &recipe.model.provider {
            return Err(AdmissionError::credential_route_unavailable(
                recipe.credential_route.provider(),
                format!(
                    "the credential route is for `{}` but the model is served by `{}`",
                    recipe.credential_route.provider(),
                    recipe.model.provider
                ),
            ));
        }

        if recipe.output_contract.requires_reward
            && !recipe
                .output_contract
                .required_operations
                .iter()
                .any(|operation| operation == "reward.get")
        {
            return Err(AdmissionError::output_contract_unsupported(
                &recipe.container.container_id,
                &["reward.get".to_string()],
            ));
        }

        Ok(ValidatedExecutionSpec { spec: self.spec })
    }
}

impl ValidatedExecutionSpec {
    pub fn spec(&self) -> &ExecutionSpec {
        &self.spec
    }

    /// Canonicalize and hash. After this the specification has an identity that
    /// an approval can be bound to.
    pub fn admit(self) -> Result<AdmissibleExecutionSpec, AdmissionError> {
        let canonical = self.spec.canonical().map_err(|error| {
            AdmissionError::execution_spec_invalid(format!(
                "the specification could not be canonicalized: {error}"
            ))
        })?;
        let digest = canonical.digest();
        Ok(AdmissibleExecutionSpec {
            spec: self.spec,
            canonical,
            digest,
        })
    }
}

// ---------------------------------------------------------------------------
// Stage 3: admissible
// ---------------------------------------------------------------------------

/// A validated specification with a stable identity. This is what the approval
/// card displays, and what an approval receipt is bound to.
#[derive(Clone, Debug, PartialEq)]
pub struct AdmissibleExecutionSpec {
    spec: ExecutionSpec,
    canonical: CanonicalJson,
    digest: Digest,
}

impl AdmissibleExecutionSpec {
    pub fn spec(&self) -> &ExecutionSpec {
        &self.spec
    }

    pub fn canonical(&self) -> &CanonicalJson {
        &self.canonical
    }

    pub fn digest(&self) -> &Digest {
        &self.digest
    }

    /// Exactly the facts the approval must display. Building this here, rather
    /// than in the UI, is what keeps the displayed bounds and the enforced
    /// bounds the same values.
    pub fn approval_disclosure(&self) -> Value {
        let recipe = &self.spec.recipe;
        json!({
            "executionSpecDigest": self.digest.as_str(),
            "sourceKind": self.spec.source_kind.as_str(),
            "catalogRecipeId": self.spec.catalog_recipe_id.as_ref().map(RecipeId::as_str),
            "container": {
                "containerId": recipe.container.container_id.as_str(),
                "registrationId": recipe.container.registration_id.as_str(),
                "sourceRevision": recipe.container.source_revision.as_str(),
                "declarationDigest": recipe.container.declaration_digest.as_str(),
            },
            "protocol": recipe.protocol.as_str(),
            "evaluator": {
                "kind": recipe.evaluator.kind(),
                "evaluatorId": recipe.evaluator.evaluator_id().as_str(),
                "scoringDigest": recipe.evaluator.scoring_digest().as_str(),
            },
            "policy": {
                "namespace": recipe.policy.namespace,
                "name": recipe.policy.name,
                "revision": recipe.policy.revision.as_str(),
                "configurationDigest": recipe.policy.configuration_digest.as_str(),
                "sourceDigest": recipe.policy.source_digest.as_ref().map(Digest::as_str),
            },
            "model": {
                "provider": recipe.model.provider.as_str(),
                "modelId": recipe.model.model_id.as_str(),
            },
            "seeds": recipe.rollout_plan.seeds.iter().map(|seed| seed.0).collect::<Vec<_>>(),
            "rolloutCount": recipe.rollout_plan.maximum_rollouts.0.get(),
            "maximumModelCallsPerRollout":
                recipe.resource_limits.maximum_model_calls_per_rollout.0.get(),
            "maximumStepsPerRollout": recipe.resource_limits.maximum_steps_per_rollout.0.get(),
            "hardTotalCostMicros": recipe.resource_limits.hard_total_cost_micros.as_micros(),
            "hardTotalCostDisplay": recipe.resource_limits.hard_total_cost_micros.render_usd(),
            "credentialRoute": {
                "kind": recipe.credential_route.kind(),
                "provider": recipe.credential_route.provider().as_str(),
                "capabilityScope": recipe.credential_route.capability_scope(),
            },
        })
    }

    /// Bind an approval receipt. Every check here is a re-check: the receipt
    /// must describe this specification, not merely accompany it.
    pub fn approve(
        self,
        binding: ApprovalBinding,
    ) -> Result<ApprovedExecutionSpec, ExecutionDriftError> {
        if binding.execution_spec_digest != self.digest {
            return Err(ExecutionDriftError::new(
                DriftCode::ApprovedSpecDigestMismatch,
                json!({
                    "approvedDigest": binding.execution_spec_digest.as_str(),
                    "actualDigest": self.digest.as_str(),
                }),
            ));
        }
        let recipe = &self.spec.recipe;
        if binding.container_declaration_digest != recipe.container.declaration_digest {
            return Err(ExecutionDriftError::new(
                DriftCode::ContainerDeclarationChanged,
                json!({
                    "approvedDeclarationDigest": binding.container_declaration_digest.as_str(),
                    "actualDeclarationDigest": recipe.container.declaration_digest.as_str(),
                }),
            ));
        }
        if binding.policy_revision != recipe.policy.revision {
            return Err(ExecutionDriftError::new(
                DriftCode::PolicyRevisionChanged,
                json!({
                    "approvedRevision": binding.policy_revision.as_str(),
                    "actualRevision": recipe.policy.revision.as_str(),
                }),
            ));
        }
        if binding.policy_configuration_digest != recipe.policy.configuration_digest {
            return Err(ExecutionDriftError::new(
                DriftCode::PolicyRevisionChanged,
                json!({
                    "approvedConfigurationDigest": binding.policy_configuration_digest.as_str(),
                    "actualConfigurationDigest": recipe.policy.configuration_digest.as_str(),
                }),
            ));
        }
        // The receipt is a ceiling, not a licence. A specification may spend
        // less than was approved but never more.
        if recipe.resource_limits.hard_total_cost_micros.as_micros()
            > binding.approved_cost_micros.as_micros()
        {
            return Err(ExecutionDriftError::new(
                DriftCode::ApprovalBoundsExceeded,
                json!({
                    "bound": "hard_total_cost_micros",
                    "approved": binding.approved_cost_micros.as_micros(),
                    "requested": recipe.resource_limits.hard_total_cost_micros.as_micros(),
                }),
            ));
        }
        if recipe.rollout_plan.maximum_rollouts.0.get() > binding.approved_rollouts.0.get() {
            return Err(ExecutionDriftError::new(
                DriftCode::ApprovalBoundsExceeded,
                json!({
                    "bound": "maximum_rollouts",
                    "approved": binding.approved_rollouts.0.get(),
                    "requested": recipe.rollout_plan.maximum_rollouts.0.get(),
                }),
            ));
        }
        Ok(ApprovedExecutionSpec {
            spec: self.spec,
            canonical: self.canonical,
            digest: self.digest,
            binding,
        })
    }
}

// ---------------------------------------------------------------------------
// Stage 4: approved
// ---------------------------------------------------------------------------

/// The only thing an executor accepts.
///
/// There is no public constructor and no way to mutate the specification
/// afterwards, so "the thing that ran" and "the thing that was approved" are
/// the same value by construction rather than by convention.
#[derive(Clone, Debug, PartialEq)]
pub struct ApprovedExecutionSpec {
    spec: ExecutionSpec,
    canonical: CanonicalJson,
    digest: Digest,
    binding: ApprovalBinding,
}

impl ApprovedExecutionSpec {
    pub fn spec(&self) -> &ExecutionSpec {
        &self.spec
    }

    pub fn recipe(&self) -> &InlineRecipe {
        &self.spec.recipe
    }

    pub fn canonical(&self) -> &CanonicalJson {
        &self.canonical
    }

    pub fn digest(&self) -> &Digest {
        &self.digest
    }

    pub fn binding(&self) -> &ApprovalBinding {
        &self.binding
    }

    pub fn receipt_id(&self) -> &ApprovalReceiptId {
        &self.binding.receipt_id
    }

    /// Re-check drift immediately before dispatch, against whatever the world
    /// looks like now. Approval proves consent at a moment; this proves the
    /// inputs still match that moment.
    pub fn reverify(
        &self,
        current_declaration_digest: &DeclarationDigest,
        current_policy_revision: &PolicyRevision,
    ) -> Result<(), ExecutionDriftError> {
        if current_declaration_digest != &self.spec.recipe.container.declaration_digest {
            return Err(ExecutionDriftError::new(
                DriftCode::ContainerDeclarationChanged,
                json!({
                    "approvedDeclarationDigest":
                        self.spec.recipe.container.declaration_digest.as_str(),
                    "currentDeclarationDigest": current_declaration_digest.as_str(),
                }),
            ));
        }
        if current_policy_revision != &self.spec.recipe.policy.revision {
            return Err(ExecutionDriftError::new(
                DriftCode::PolicyRevisionChanged,
                json!({
                    "approvedRevision": self.spec.recipe.policy.revision.as_str(),
                    "currentRevision": current_policy_revision.as_str(),
                }),
            ));
        }
        // Guard against the specification itself having been altered in memory
        // between approval and dispatch.
        let recomputed = self.spec.canonical().map(|value| value.digest());
        match recomputed {
            Ok(recomputed) if recomputed == self.digest => Ok(()),
            Ok(recomputed) => Err(ExecutionDriftError::new(
                DriftCode::ApprovedSpecDigestMismatch,
                json!({
                    "approvedDigest": self.digest.as_str(),
                    "actualDigest": recomputed.as_str(),
                }),
            )),
            Err(error) => Err(ExecutionDriftError::new(
                DriftCode::ApprovedSpecDigestMismatch,
                json!({ "detail": error.to_string() }),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Drift
// ---------------------------------------------------------------------------

/// Stable codes for post-approval drift. Separate from [`AdmissionErrorCode`]
/// because these are refusals to execute an already-approved specification, and
/// the remediation is always "get a new approval" rather than "fix a field".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftCode {
    ApprovedSpecDigestMismatch,
    ContainerDeclarationChanged,
    PolicyRevisionChanged,
    ApprovalBoundsExceeded,
}

impl DriftCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApprovedSpecDigestMismatch => "approved_spec_digest_mismatch",
            Self::ContainerDeclarationChanged => "container_declaration_changed",
            Self::PolicyRevisionChanged => "policy_revision_changed",
            Self::ApprovalBoundsExceeded => "approval_bounds_exceeded",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionDriftError {
    pub code: DriftCode,
    pub context: Value,
}

impl ExecutionDriftError {
    pub fn new(code: DriftCode, context: Value) -> Self {
        Self { code, context }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "code": self.code.as_str(),
            "context": self.context,
            // Never "patch the run": a changed input needs a new approval, and
            // reusing the old receipt is what this refuses.
            "remediation": "Re-admit the specification and request a new paid-compute approval. \
                            The existing receipt does not cover these inputs.",
        })
    }
}

impl fmt::Display for ExecutionDriftError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.context)
    }
}

impl std::error::Error for ExecutionDriftError {}
