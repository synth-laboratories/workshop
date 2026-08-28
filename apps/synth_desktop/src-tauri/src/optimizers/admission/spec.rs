//! The immutable execution specification and the two sources it can come from.
//!
//! Everything here is data a run can be reproduced from. There are no options
//! that mean "decide later", no booleans standing in for a choice with more
//! than two outcomes, and no free-form JSON where a field has known semantics.
//! A value that is genuinely absent is `Option::None`; a value that is present
//! is one an authoritative source actually supplied.

use super::canonical::{CanonicalError, CanonicalJson};
use super::ids::{
    ApprovalReceiptId, ContainerId, ContainerRegistrationId, CostMicros, DeclarationDigest, Digest,
    EvaluatorId, ModelCallCount, ModelId, PolicyRevision, ProviderId, RecipeDigest, RecipeId,
    RolloutCount, Seed, SourceRevision, StepCount,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// The one live-eval protocol this pipeline admits. A container advertising
/// anything else is `container_protocol_unsupported`; it is never adapted.
pub const LIVE_EVAL_PROTOCOL_V1: &str = "synth.container.live-eval.v1";

/// Where a specification came from. Inline is the default path; catalog is
/// entered only on an explicit request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecipeSource {
    Inline(InlineRecipe),
    Catalog(CatalogRecipeRef),
}

impl RecipeSource {
    pub fn kind(&self) -> RecipeSourceKind {
        match self {
            Self::Inline(_) => RecipeSourceKind::Inline,
            Self::Catalog(_) => RecipeSourceKind::Catalog,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeSourceKind {
    Inline,
    Catalog,
}

impl RecipeSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::Catalog => "catalog",
        }
    }
}

impl fmt::Display for RecipeSourceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// An explicit reference to a catalog recipe. `expected_digest` lets a caller
/// pin the exact revision it reviewed; when present it is enforced, and a
/// mismatch fails rather than resolving to whatever the catalog holds now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRecipeRef {
    pub recipe_id: RecipeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_digest: Option<RecipeDigest>,
}

/// A complete inline evaluation specification.
///
/// Every field is required because every field is semantically necessary: a
/// specification missing any one of them cannot be executed, reproduced, or
/// meaningfully approved. Optionality here would only move the failure from
/// admission to execution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineRecipe {
    pub container: ContainerPin,
    pub protocol: LiveEvalProtocol,
    pub evaluator: EvaluatorSpec,
    pub policy: PolicyPin,
    pub model: ModelPin,
    pub rollout_plan: RolloutPlan,
    pub resource_limits: ResourceLimits,
    pub credential_route: CredentialRoute,
    pub output_contract: OutputContract,
}

/// The container, pinned to the exact registration and revision that was read.
///
/// `registration_id` and `source_revision` are separate on purpose. The same
/// container id can be re-registered against a different build; pinning only
/// the id would let the thing that runs differ from the thing that was
/// approved, with no way to detect it afterwards.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerPin {
    pub container_id: ContainerId,
    pub registration_id: ContainerRegistrationId,
    pub source_revision: SourceRevision,
    pub declaration_digest: DeclarationDigest,
}

/// The normalized live-eval protocol, as a closed set rather than a string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveEvalProtocol {
    /// `synth.container.live-eval.v1`
    SynthContainerLiveEvalV1,
}

impl LiveEvalProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SynthContainerLiveEvalV1 => LIVE_EVAL_PROTOCOL_V1,
        }
    }

    /// Parse an advertised protocol string. Unknown protocols return `None`;
    /// they are never coerced to the one variant this pipeline knows.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            LIVE_EVAL_PROTOCOL_V1 => Some(Self::SynthContainerLiveEvalV1),
            _ => None,
        }
    }
}

impl fmt::Display for LiveEvalProtocol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// How scoring is determined.
///
/// `ContainerDeclared` is the ordinary case: the container owns its own scoring
/// and Workshop pins the digest it declared. `Explicit` covers a caller that
/// names an evaluator the container supports and supplies its configuration.
/// There is deliberately no third variant for "infer it" — inferred scoring
/// semantics are the thing this type exists to make unrepresentable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvaluatorSpec {
    ContainerDeclared {
        evaluator_id: EvaluatorId,
        evaluator_version: String,
        scoring_digest: Digest,
    },
    Explicit {
        evaluator_id: EvaluatorId,
        configuration: CanonicalJson,
        scoring_digest: Digest,
    },
}

impl EvaluatorSpec {
    pub fn evaluator_id(&self) -> &EvaluatorId {
        match self {
            Self::ContainerDeclared { evaluator_id, .. } | Self::Explicit { evaluator_id, .. } => {
                evaluator_id
            }
        }
    }

    pub fn scoring_digest(&self) -> &Digest {
        match self {
            Self::ContainerDeclared { scoring_digest, .. }
            | Self::Explicit { scoring_digest, .. } => scoring_digest,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::ContainerDeclared { .. } => "container_declared",
            Self::Explicit { .. } => "explicit",
        }
    }
}

/// Where the approved policy bytes came from. Identity (`namespace/name@rev`)
/// is not enough: dispatch must re-read these bytes from this root and verify
/// `content_digest` immediately before spend.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyMaterialRef {
    pub source_root: String,
    pub repository_relative_path: String,
    pub tracked_revision: String,
    pub content_digest: Digest,
}

/// The policy, pinned to a revision and to the exact configuration bytes.
///
/// `configuration_digest` is stored alongside the configuration rather than
/// recomputed on read, so that a configuration rewritten in place — by a
/// migration, a re-serialization, a well-meant normalization — is detectable
/// instead of silently becoming the new truth.
///
/// Source bytes themselves are not part of the canonical digest. They are an
/// in-memory materialization of `material`; hashing them would make a draft
/// without bytes and a start with bytes look like two specifications.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyPin {
    pub namespace: String,
    pub name: String,
    pub revision: PolicyRevision,
    pub configuration: CanonicalJson,
    pub configuration_digest: Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<PolicyMaterialRef>,
    #[serde(default, skip_serializing)]
    pub source_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_digest: Option<Digest>,
}

impl PolicyPin {
    /// Build a pin, computing the digest from the canonical configuration.
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        revision: PolicyRevision,
        configuration: CanonicalJson,
    ) -> Self {
        let configuration_digest = configuration.digest();
        Self {
            namespace: namespace.into(),
            name: name.into(),
            revision,
            configuration,
            configuration_digest,
            material: None,
            source_code: None,
            source_digest: None,
        }
    }

    pub fn with_source_code(mut self, source_code: Option<String>) -> Self {
        self.source_digest = source_code
            .as_deref()
            .map(|code| super::canonical::digest_bytes(code.as_bytes()));
        self.source_code = source_code;
        self
    }

    pub fn with_material(mut self, material: PolicyMaterialRef, source_code: String) -> Self {
        let digest = super::canonical::digest_bytes(source_code.as_bytes());
        self.source_digest = Some(digest.clone());
        self.material = Some(PolicyMaterialRef {
            content_digest: digest,
            ..material
        });
        self.source_code = Some(source_code);
        self
    }

    /// Whether the stored digest still matches the stored configuration.
    /// Checked on read-back rather than assumed.
    pub fn digest_matches(&self) -> bool {
        if self.configuration.digest() != self.configuration_digest {
            return false;
        }
        match (&self.source_code, &self.source_digest, &self.material) {
            (Some(code), Some(digest), material) => {
                let actual = super::canonical::digest_bytes(code.as_bytes());
                actual == *digest
                    && material
                        .as_ref()
                        .is_none_or(|item| item.content_digest == actual)
            }
            (None, Some(digest), Some(material)) => material.content_digest == *digest,
            (None, None, None) => true,
            (None, Some(_), None) => true,
            _ => false,
        }
    }

    pub fn qualified_name(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }
}

/// Provider and model, both explicit. A model is never substituted, upgraded,
/// or aliased between admission and execution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPin {
    pub provider: ProviderId,
    pub model_id: ModelId,
}

/// The seeds to run and the hard cap on how many rollouts may exist.
///
/// `maximum_rollouts` is not derived from `seeds.len()`: a caller may cap below
/// the seed count deliberately, and validation checks the relationship rather
/// than silently making one follow the other.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolloutPlan {
    pub seeds: Vec<Seed>,
    pub maximum_rollouts: RolloutCount,
}

impl RolloutPlan {
    /// How many rollouts this plan actually declares. Terminal success requires
    /// exactly this many valid terminal records.
    pub fn declared_rollouts(&self) -> usize {
        self.seeds.len()
    }
}

/// Per-rollout and total bounds. All three are required for paid execution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLimits {
    pub maximum_model_calls_per_rollout: ModelCallCount,
    pub maximum_steps_per_rollout: StepCount,
    pub hard_total_cost_micros: CostMicros,
}

/// How the container gets provider credentials. An enum with one variant today
/// because the shape must survive a second route being added without every
/// call site having to guess whether a missing value meant "proxy".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CredentialRoute {
    WorkshopSecretsProxy {
        provider: ProviderId,
        capability_scope: CredentialCapabilityScope,
    },
}

impl CredentialRoute {
    pub fn provider(&self) -> &ProviderId {
        match self {
            Self::WorkshopSecretsProxy { provider, .. } => provider,
        }
    }

    pub fn capability_scope(&self) -> &CredentialCapabilityScope {
        match self {
            Self::WorkshopSecretsProxy {
                capability_scope, ..
            } => capability_scope,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::WorkshopSecretsProxy { .. } => "workshop_secrets_proxy",
        }
    }
}

/// What the minted credential capability is allowed to do, and for how long.
/// Displayed at approval and revoked at terminal state; revocation being
/// unconfirmed is one of the conditions that blocks `completed`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialCapabilityScope {
    /// Wire names of the operations the capability permits, sorted.
    pub operations: Vec<String>,
    /// Lifetime in seconds. Bounded so a leaked capability expires on its own.
    pub lifetime_seconds: u32,
}

impl CredentialCapabilityScope {
    pub fn new(operations: impl IntoIterator<Item = String>, lifetime_seconds: u32) -> Self {
        let mut operations: Vec<String> = operations.into_iter().collect();
        operations.sort();
        operations.dedup();
        Self {
            operations,
            lifetime_seconds,
        }
    }
}

/// The evidence the run must produce. Each entry is an operation the container
/// has to advertise; a specification requiring evidence the container cannot
/// emit is `output_contract_unsupported` at admission rather than a run that
/// finishes with nothing to show.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputContract {
    /// Required per-rollout reward records.
    pub requires_reward: bool,
    /// Required sealed trace capture.
    pub requires_trace: bool,
    /// Required token/cost usage reporting.
    pub requires_usage: bool,
    /// Extra container operations this contract depends on, sorted wire names.
    pub required_operations: Vec<String>,
}

impl OutputContract {
    pub fn new(
        requires_reward: bool,
        requires_trace: bool,
        requires_usage: bool,
        required_operations: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut required_operations: Vec<String> = required_operations.into_iter().collect();
        required_operations.sort();
        required_operations.dedup();
        Self {
            requires_reward,
            requires_trace,
            requires_usage,
            required_operations,
        }
    }
}

/// The specification an approval is taken over and an executor runs. Both the
/// inline and catalog paths converge on this type before validation, so there
/// is exactly one thing to validate, hash, approve, execute, and persist.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSpec {
    /// Which path produced this specification. Recorded so a run can be
    /// labelled truthfully in the UI without re-deriving it.
    pub source_kind: RecipeSourceKind,
    /// Present only when the caller explicitly asked for catalog resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_recipe_id: Option<RecipeId>,
    pub recipe: InlineRecipe,
}

impl ExecutionSpec {
    /// The canonical form. This is what gets hashed, displayed, approved, and
    /// persisted — one encoding, so the digest a user approved is the digest
    /// the executor checks.
    pub fn canonical(&self) -> Result<CanonicalJson, CanonicalError> {
        let raw = serde_json::to_value(self).map_err(CanonicalError::Serialize)?;
        CanonicalJson::new(raw)
    }

    pub fn digest(&self) -> Result<Digest, CanonicalError> {
        Ok(self.canonical()?.digest())
    }

    /// The only run-lease policy derived from an admitted execution spec.
    /// Calls and cost come directly from the approved resource envelope;
    /// operation/lifetime from its credential scope; model from its pin.
    pub fn provider_use_policy(&self) -> crate::secrets::SecretsUsePolicy {
        let recipe = &self.recipe;
        let total_calls = u64::from(
            recipe
                .resource_limits
                .maximum_model_calls_per_rollout
                .0
                .get(),
        )
        .saturating_mul(u64::from(recipe.rollout_plan.maximum_rollouts.0.get()))
        .min(u64::from(u32::MAX)) as u32;
        let configuration = recipe.policy.configuration.as_value();
        let reasoning_efforts = configuration
            .get("reasoning_effort")
            .and_then(Value::as_str)
            .map(|value| vec![value.to_string()])
            .unwrap_or_default();
        let input_per_call = configuration
            .get("context_token_budget")
            .and_then(Value::as_u64);
        let output_per_call = configuration
            .get("answer_max_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .saturating_add(
                configuration
                    .get("thinking_budget")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            );
        provider_use_policy_from_bounds(
            recipe
                .credential_route
                .capability_scope()
                .operations
                .clone(),
            vec![recipe.model.model_id.as_str().to_string()],
            reasoning_efforts,
            total_calls,
            recipe.resource_limits.hard_total_cost_micros.as_micros(),
            u64::from(recipe.credential_route.capability_scope().lifetime_seconds),
            input_per_call.map(|value| value.saturating_mul(u64::from(total_calls))),
            (output_per_call > 0).then(|| output_per_call.saturating_mul(u64::from(total_calls))),
        )
    }
}

/// Explicit bounded-policy constructor for legacy catalog lanes while they
/// converge on `ExecutionSpec`. It intentionally has no 40-call/$0.60
/// fallback: every limit is supplied by admission data at the caller.
pub(crate) fn provider_use_policy_from_bounds(
    operations: Vec<String>,
    models: Vec<String>,
    reasoning_efforts: Vec<String>,
    max_calls: u32,
    max_cost_usd_micros: u64,
    lifetime_seconds: u64,
    max_input_tokens: Option<u64>,
    max_output_tokens: Option<u64>,
) -> crate::secrets::SecretsUsePolicy {
    crate::secrets::SecretsUsePolicy {
        operations,
        models,
        reasoning_efforts,
        max_calls,
        max_input_tokens: max_input_tokens.unwrap_or(u64::MAX),
        max_output_tokens: max_output_tokens.unwrap_or(u64::MAX),
        max_cost_usd: max_cost_usd_micros as f64 / 1_000_000.0,
        lifetime_seconds,
    }
}

/// The approval receipt, bound to a specification digest and to the exact
/// bounds shown. Reusing a receipt for a different specification or looser
/// bounds is what `approval_bounds_exceeded` refuses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalBinding {
    pub receipt_id: ApprovalReceiptId,
    /// The digest that was displayed and approved.
    pub execution_spec_digest: Digest,
    /// The container declaration digest at approval time.
    pub container_declaration_digest: DeclarationDigest,
    /// The policy revision at approval time.
    pub policy_revision: PolicyRevision,
    /// The policy configuration digest at approval time.
    pub policy_configuration_digest: Digest,
    /// The ceiling the operator consented to, in micros.
    pub approved_cost_micros: CostMicros,
    /// The rollout count the operator consented to.
    pub approved_rollouts: RolloutCount,
}

