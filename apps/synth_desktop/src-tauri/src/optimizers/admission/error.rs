//! Typed admission errors with stable public codes.
//!
//! Two rules shape this module.
//!
//! First, the code is the signal. A caller — agent or UI — must be able to
//! branch on `evaluator_not_declared` without parsing prose, so the code is an
//! enum with a fixed wire spelling and the message is explanatory only.
//!
//! Second, an error names the component that is actually missing. The failure
//! this replaces was a catalog lookup reporting `recipe_not_found` when the
//! real problem was that a container declared no scoring contract; the operator
//! then went looking for a recipe to author, which was never the fix. Every
//! variant here therefore carries the subject it failed on and exactly one
//! remediation describing what to do about that subject.

use super::ids::{ContainerId, EvaluatorId, ModelId, ProviderId, RecipeId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;

/// The stable public vocabulary. These strings appear in tool results, run
/// records, and the UI; they are API surface and must not be respelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionErrorCode {
    CatalogRecipeNotFound,
    ContainerNotFound,
    ContainerSelectionAmbiguous,
    ContainerUnhealthy,
    ContainerProtocolUnsupported,
    ContainerRuntimeStale,
    EvaluatorNotDeclared,
    ScoringContractInvalid,
    PolicyNotFound,
    PolicyRevisionUnresolved,
    PolicyConfigurationInvalid,
    ModelUnsupported,
    SeedControlUnsupported,
    RequestedLimitUnsupported,
    CostCeilingRequired,
    CredentialRouteUnavailable,
    OutputContractUnsupported,
    ExecutionSpecInvalid,
    ExecutionSpecDigestMismatch,
}

impl AdmissionErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CatalogRecipeNotFound => "catalog_recipe_not_found",
            Self::ContainerNotFound => "container_not_found",
            Self::ContainerSelectionAmbiguous => "container_selection_ambiguous",
            Self::ContainerUnhealthy => "container_unhealthy",
            Self::ContainerProtocolUnsupported => "container_protocol_unsupported",
            Self::ContainerRuntimeStale => "container_runtime_stale",
            Self::EvaluatorNotDeclared => "evaluator_not_declared",
            Self::ScoringContractInvalid => "scoring_contract_invalid",
            Self::PolicyNotFound => "policy_not_found",
            Self::PolicyRevisionUnresolved => "policy_revision_unresolved",
            Self::PolicyConfigurationInvalid => "policy_configuration_invalid",
            Self::ModelUnsupported => "model_unsupported",
            Self::SeedControlUnsupported => "seed_control_unsupported",
            Self::RequestedLimitUnsupported => "requested_limit_unsupported",
            Self::CostCeilingRequired => "cost_ceiling_required",
            Self::CredentialRouteUnavailable => "credential_route_unavailable",
            Self::OutputContractUnsupported => "output_contract_unsupported",
            Self::ExecutionSpecInvalid => "execution_spec_invalid",
            Self::ExecutionSpecDigestMismatch => "execution_spec_digest_mismatch",
        }
    }

    /// Which part of the specification the caller has to change. The UI groups
    /// remediation by this, so a scoring problem never renders under "recipe".
    pub fn subject(self) -> AdmissionSubject {
        match self {
            Self::CatalogRecipeNotFound => AdmissionSubject::CatalogRecipe,
            Self::ContainerNotFound
            | Self::ContainerSelectionAmbiguous
            | Self::ContainerUnhealthy
            | Self::ContainerProtocolUnsupported
            | Self::ContainerRuntimeStale => AdmissionSubject::Container,
            Self::EvaluatorNotDeclared | Self::ScoringContractInvalid => {
                AdmissionSubject::Evaluator
            }
            Self::PolicyNotFound
            | Self::PolicyRevisionUnresolved
            | Self::PolicyConfigurationInvalid => AdmissionSubject::Policy,
            Self::ModelUnsupported => AdmissionSubject::Model,
            Self::SeedControlUnsupported => AdmissionSubject::Seeds,
            Self::RequestedLimitUnsupported | Self::CostCeilingRequired => AdmissionSubject::Limits,
            Self::CredentialRouteUnavailable => AdmissionSubject::Credentials,
            Self::OutputContractUnsupported => AdmissionSubject::OutputContract,
            Self::ExecutionSpecInvalid | Self::ExecutionSpecDigestMismatch => {
                AdmissionSubject::Specification
            }
        }
    }
}

impl fmt::Display for AdmissionErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionSubject {
    CatalogRecipe,
    Container,
    Evaluator,
    Policy,
    Model,
    Seeds,
    Limits,
    Credentials,
    OutputContract,
    Specification,
}

impl AdmissionSubject {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CatalogRecipe => "catalog_recipe",
            Self::Container => "container",
            Self::Evaluator => "evaluator",
            Self::Policy => "policy",
            Self::Model => "model",
            Self::Seeds => "seeds",
            Self::Limits => "limits",
            Self::Credentials => "credentials",
            Self::OutputContract => "output_contract",
            Self::Specification => "specification",
        }
    }
}

/// One admission failure: a stable code, the structured facts a caller needs to
/// act, and exactly one remediation.
///
/// `context` is deliberately structured rather than interpolated into the
/// message. An agent reading `containerSelectionAmbiguous` needs the candidate
/// list as data so it can ask the user to pick one; a sentence listing them is
/// not that.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmissionError {
    pub code: AdmissionErrorCode,
    pub subject: AdmissionSubject,
    /// Human-readable statement of what was wrong. Never the only signal.
    pub message: String,
    /// Exactly one actionable next step. Never "try another recipe" — that is
    /// advice to abandon the request rather than to fix it.
    pub remediation: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub context: Value,
}

impl AdmissionError {
    pub fn new(
        code: AdmissionErrorCode,
        message: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Self {
        Self {
            code,
            subject: code.subject(),
            message: message.into(),
            remediation: remediation.into(),
            context: Value::Null,
        }
    }

    pub fn with_context(mut self, context: Value) -> Self {
        self.context = context;
        self
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| {
            json!({
                "code": self.code.as_str(),
                "message": self.message,
            })
        })
    }

    // -- Constructors for the cases the handoff calls out by name -----------
    //
    // These exist so that a call site cannot accidentally report a catalog
    // miss for an evaluator problem: the only convenient way to build the
    // error is the one that names the right subject.

    pub fn catalog_recipe_not_found(recipe_id: &RecipeId, searched: usize) -> Self {
        Self::new(
            AdmissionErrorCode::CatalogRecipeNotFound,
            format!("no catalog recipe is registered under `{recipe_id}`"),
            "Correct the recipe id, or drop the catalog reference and submit an inline \
             specification instead — inline admission does not require a catalog entry.",
        )
        .with_context(json!({ "recipeId": recipe_id, "recipesSearched": searched }))
    }

    pub fn container_not_found(requested: &str) -> Self {
        Self::new(
            AdmissionErrorCode::ContainerNotFound,
            format!("no registered container matches `{requested}`"),
            "Register the container, or name one that container discovery already returns.",
        )
        .with_context(json!({ "requested": requested }))
    }

    /// Ambiguity is a refusal, never a reason to take the first result.
    pub fn container_selection_ambiguous(candidates: &[ContainerId]) -> Self {
        Self::new(
            AdmissionErrorCode::ContainerSelectionAmbiguous,
            format!(
                "{} registered containers match this request; admission will not choose one",
                candidates.len()
            ),
            "Name the exact container id in the request.",
        )
        .with_context(json!({
            "candidates": candidates.iter().map(ContainerId::as_str).collect::<Vec<_>>(),
        }))
    }

    pub fn container_unhealthy(container: &ContainerId, observed: &str) -> Self {
        Self::new(
            AdmissionErrorCode::ContainerUnhealthy,
            format!("container `{container}` reported health `{observed}`"),
            "Bring the container back to a ready state, then re-run discovery so the \
             specification pins a healthy registration.",
        )
        .with_context(json!({ "containerId": container, "observedHealth": observed }))
    }

    /// The running container loaded a build the declaration did not name.
    ///
    /// Raised even when the declaration digest is unchanged: the digest covers
    /// what was declared, not what is loaded, and treating an unchanged digest
    /// as evidence of freshness is how a rebuilt harness ran under an old
    /// managed container.
    pub fn container_runtime_stale(
        container: &ContainerId,
        declared: &str,
        observed: &str,
    ) -> Self {
        Self::new(
            AdmissionErrorCode::ContainerRuntimeStale,
            format!(
                "container `{container}` is running revision `{observed}` while its declaration \
                 names `{declared}`"
            ),
            "Replace the managed container so it loads the declared revision, then re-run \
             discovery; the declaration digest alone does not prove the runtime was replaced.",
        )
        .with_context(json!({
            "containerId": container,
            "declaredSourceRevision": declared,
            "loadedRuntimeRevision": observed,
        }))
    }

    pub fn container_protocol_unsupported(
        container: &ContainerId,
        required: &str,
        advertised: Option<&str>,
    ) -> Self {
        Self::new(
            AdmissionErrorCode::ContainerProtocolUnsupported,
            match advertised {
                Some(advertised) => format!(
                    "container `{container}` advertises protocol `{advertised}`, not `{required}`"
                ),
                None => format!("container `{container}` advertises no live-eval protocol"),
            },
            format!(
                "Have the container advertise `{required}` in its capability block, or select \
                 a container that already does."
            ),
        )
        .with_context(json!({
            "containerId": container,
            "requiredProtocol": required,
            "advertisedProtocol": advertised,
        }))
    }

    /// The precise failure the NanoHorizon acceptance test is allowed to hit.
    /// It names the evaluator, not a recipe, because authoring a recipe would
    /// not supply the missing declaration.
    pub fn evaluator_not_declared(container: &ContainerId) -> Self {
        Self::new(
            AdmissionErrorCode::EvaluatorNotDeclared,
            format!(
                "container `{container}` declares no evaluator, so admission has no scoring \
                 semantics to pin"
            ),
            "Have the container declare its evaluator id, version, and scoring digest in its \
             live-eval capability block, or supply an explicit evaluator in the request.",
        )
        .with_context(json!({ "containerId": container }))
    }

    pub fn scoring_contract_invalid(
        container: &ContainerId,
        evaluator: Option<&EvaluatorId>,
        detail: impl Into<String>,
    ) -> Self {
        let detail = detail.into();
        Self::new(
            AdmissionErrorCode::ScoringContractInvalid,
            format!("container `{container}` declared an unusable scoring contract: {detail}"),
            "Correct the container's scoring declaration so it carries a well-formed evaluator \
             id, version, and scoring digest.",
        )
        .with_context(json!({
            "containerId": container,
            "evaluatorId": evaluator.map(EvaluatorId::as_str),
            "detail": detail,
        }))
    }

    pub fn policy_not_found(namespace: &str, name: &str) -> Self {
        Self::new(
            AdmissionErrorCode::PolicyNotFound,
            format!("no policy `{namespace}/{name}` is available to this session"),
            "Correct the policy namespace and name, or declare the policy in the session \
             workspace before requesting it.",
        )
        .with_context(json!({ "namespace": namespace, "name": name }))
    }

    /// A policy that can still change underneath the run is not pinnable, and a
    /// specification that cannot pin it is not immutable.
    pub fn policy_revision_unresolved(namespace: &str, name: &str) -> Self {
        Self::new(
            AdmissionErrorCode::PolicyRevisionUnresolved,
            format!(
                "policy `{namespace}/{name}` resolved to no immutable revision, so it cannot be \
                 pinned"
            ),
            "Resolve the policy to an immutable revision — commit or version the policy source — \
             before requesting execution.",
        )
        .with_context(json!({ "namespace": namespace, "name": name }))
    }

    pub fn policy_configuration_invalid(
        namespace: &str,
        name: &str,
        detail: impl Into<String>,
    ) -> Self {
        let detail = detail.into();
        Self::new(
            AdmissionErrorCode::PolicyConfigurationInvalid,
            format!("policy `{namespace}/{name}` configuration is invalid: {detail}"),
            "Correct the offending configuration key, or remove the override so the policy \
             declaration's own value is used.",
        )
        .with_context(json!({ "namespace": namespace, "name": name, "detail": detail }))
    }

    pub fn model_unsupported(
        provider: &ProviderId,
        model: &ModelId,
        container: &ContainerId,
    ) -> Self {
        Self::new(
            AdmissionErrorCode::ModelUnsupported,
            format!(
                "container `{container}` does not advertise support for `{provider}` model \
                 `{model}`"
            ),
            "Request a model the container advertises, or have the container advertise this one. \
             Admission will not substitute a different model.",
        )
        .with_context(json!({
            "providerId": provider,
            "modelId": model,
            "containerId": container,
        }))
    }

    pub fn seed_control_unsupported(container: &ContainerId) -> Self {
        Self::new(
            AdmissionErrorCode::SeedControlUnsupported,
            format!("container `{container}` does not accept caller-supplied seeds"),
            "Drop the explicit seeds only if the request genuinely does not need them; otherwise \
             select a container that advertises seed control. Admission will not silently \
             substitute its own seeds.",
        )
        .with_context(json!({ "containerId": container }))
    }

    /// A requested limit above capability fails; it is never quietly reduced,
    /// because a run that silently did less than asked reports a result for a
    /// question nobody posed.
    pub fn requested_limit_unsupported(
        limit: &'static str,
        requested: u64,
        maximum_supported: Option<u64>,
    ) -> Self {
        Self::new(
            AdmissionErrorCode::RequestedLimitUnsupported,
            match maximum_supported {
                Some(maximum) => format!(
                    "requested {limit} of {requested} exceeds the supported maximum of {maximum}"
                ),
                None => format!("requested {limit} of {requested} is not supported"),
            },
            format!(
                "Lower {limit} in the request to a supported value. Admission will not clamp it \
                 for you."
            ),
        )
        .with_context(json!({
            "limit": limit,
            "requested": requested,
            "maximumSupported": maximum_supported,
        }))
    }

    pub fn cost_ceiling_required() -> Self {
        Self::new(
            AdmissionErrorCode::CostCeilingRequired,
            "this specification spends provider credit and declares no cost ceiling",
            "Supply an explicit hard total cost ceiling in the request.",
        )
    }

    pub fn credential_route_unavailable(provider: &ProviderId, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self::new(
            AdmissionErrorCode::CredentialRouteUnavailable,
            format!("the credential route for `{provider}` is unavailable: {detail}"),
            "Start the Workshop secrets proxy and configure this provider's credential, then \
             re-admit the specification.",
        )
        .with_context(json!({ "providerId": provider, "detail": detail }))
    }

    pub fn output_contract_unsupported(container: &ContainerId, missing: &[String]) -> Self {
        Self::new(
            AdmissionErrorCode::OutputContractUnsupported,
            format!(
                "container `{container}` does not advertise the evidence operations this \
                 specification requires"
            ),
            "Require only the evidence operations the container advertises, or select a \
             container that advertises the missing ones.",
        )
        .with_context(json!({ "containerId": container, "missingOperations": missing }))
    }

    pub fn execution_spec_invalid(detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self::new(
            AdmissionErrorCode::ExecutionSpecInvalid,
            format!("the execution specification is not admissible: {detail}"),
            "Correct the named field and re-validate the draft.",
        )
        .with_context(json!({ "detail": detail }))
    }

    pub fn execution_spec_digest_mismatch(expected: &str, actual: &str) -> Self {
        Self::new(
            AdmissionErrorCode::ExecutionSpecDigestMismatch,
            "the execution specification changed after it was admitted",
            "Re-admit the specification and obtain a new approval. An existing receipt is not \
             transferable to a changed specification.",
        )
        .with_context(json!({ "expectedDigest": expected, "actualDigest": actual }))
    }
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for AdmissionError {}

