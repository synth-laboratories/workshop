//! Shared identifiers, domain hierarchy, and exhaustive failure policy.
//!
//! See `notes/specifications/workshop/failure_runtime.md`.

use super::remediation::{FailureRemediation, RepairRequest, ResourceRef, SettingsRoute};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const FAILURE_SCHEMA_VERSION: &str = "synth.failure-view.v1";
pub const CODE_CONTRACT_INVALID: &str = "failure_contract_invalid";
pub const CODE_HISTORICAL_UNCLASSIFIED: &str = "historical_failure_unclassified";
pub const CODE_BOOTSTRAP_UNAVAILABLE: &str = "sqlite_unavailable_at_bootstrap";

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FailureId(pub String);

impl FailureId {
    pub fn generate() -> Self {
        Self(format!("fail_{}", uuid::Uuid::new_v4().simple()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FailureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    Admission,
    Approval,
    Authentication,
    Container,
    Evaluation,
    Persistence,
    Provider,
    Session,
    Telemetry,
    Visual,
    Contract,
}

impl FailureCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admission => "admission",
            Self::Approval => "approval",
            Self::Authentication => "authentication",
            Self::Container => "container",
            Self::Evaluation => "evaluation",
            Self::Persistence => "persistence",
            Self::Provider => "provider",
            Self::Session => "session",
            Self::Telemetry => "telemetry",
            Self::Visual => "visual",
            Self::Contract => "contract",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureDisposition {
    ApprovalRequired,
    RepairRequired,
    Retryable,
    Terminal,
    Cancelled,
    ProgrammerError,
}

impl FailureDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApprovalRequired => "approval_required",
            Self::RepairRequired => "repair_required",
            Self::Retryable => "retryable",
            Self::Terminal => "terminal",
            Self::Cancelled => "cancelled",
            Self::ProgrammerError => "programmer_error",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureStateEffect {
    None,
    MarkContainerUnhealthy,
    BlockContainerMutation,
    TerminalizeEvaluation,
    TerminalizeSessionTurn,
    MarkVisualFailed,
    DegradeIndex,
    RecordEmergencyMode,
}

impl FailureStateEffect {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::MarkContainerUnhealthy => "mark_container_unhealthy",
            Self::BlockContainerMutation => "block_container_mutation",
            Self::TerminalizeEvaluation => "terminalize_evaluation",
            Self::TerminalizeSessionTurn => "terminalize_session_turn",
            Self::MarkVisualFailed => "mark_visual_failed",
            Self::DegradeIndex => "degrade_index",
            Self::RecordEmergencyMode => "record_emergency_mode",
        }
    }
}

pub trait FailureDefinition {
    fn code(&self) -> &'static str;
    fn category(&self) -> FailureCategory;
    fn disposition(&self) -> FailureDisposition;
    fn remediation(&self) -> Option<FailureRemediation>;
    fn state_effect(&self) -> FailureStateEffect;
    fn message(&self) -> String;
    fn safe_facts(&self) -> serde_json::Value;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    Admission(AdmissionFailure),
    Approval(ApprovalFailure),
    Authentication(AuthenticationFailure),
    Container(ContainerFailure),
    Evaluation(EvaluationFailure),
    Persistence(PersistenceFailure),
    Provider(ProviderFailure),
    Session(SessionFailure),
    Telemetry(TelemetryFailure),
    Visual(VisualFailure),
    Contract(ContractFailure),
}

impl FailureKind {
    pub fn domain(&self) -> FailureCategory {
        self.category()
    }
}

impl FailureDefinition for FailureKind {
    fn code(&self) -> &'static str {
        match self {
            Self::Admission(v) => v.code(),
            Self::Approval(v) => v.code(),
            Self::Authentication(v) => v.code(),
            Self::Container(v) => v.code(),
            Self::Evaluation(v) => v.code(),
            Self::Persistence(v) => v.code(),
            Self::Provider(v) => v.code(),
            Self::Session(v) => v.code(),
            Self::Telemetry(v) => v.code(),
            Self::Visual(v) => v.code(),
            Self::Contract(v) => v.code(),
        }
    }

    fn category(&self) -> FailureCategory {
        match self {
            Self::Admission(_) => FailureCategory::Admission,
            Self::Approval(_) => FailureCategory::Approval,
            Self::Authentication(_) => FailureCategory::Authentication,
            Self::Container(_) => FailureCategory::Container,
            Self::Evaluation(_) => FailureCategory::Evaluation,
            Self::Persistence(_) => FailureCategory::Persistence,
            Self::Provider(_) => FailureCategory::Provider,
            Self::Session(_) => FailureCategory::Session,
            Self::Telemetry(_) => FailureCategory::Telemetry,
            Self::Visual(_) => FailureCategory::Visual,
            Self::Contract(_) => FailureCategory::Contract,
        }
    }

    fn disposition(&self) -> FailureDisposition {
        match self {
            Self::Admission(v) => v.disposition(),
            Self::Approval(v) => v.disposition(),
            Self::Authentication(v) => v.disposition(),
            Self::Container(v) => v.disposition(),
            Self::Evaluation(v) => v.disposition(),
            Self::Persistence(v) => v.disposition(),
            Self::Provider(v) => v.disposition(),
            Self::Session(v) => v.disposition(),
            Self::Telemetry(v) => v.disposition(),
            Self::Visual(v) => v.disposition(),
            Self::Contract(v) => v.disposition(),
        }
    }

    fn remediation(&self) -> Option<FailureRemediation> {
        match self {
            Self::Admission(v) => v.remediation(),
            Self::Approval(v) => v.remediation(),
            Self::Authentication(v) => v.remediation(),
            Self::Container(v) => v.remediation(),
            Self::Evaluation(v) => v.remediation(),
            Self::Persistence(v) => v.remediation(),
            Self::Provider(v) => v.remediation(),
            Self::Session(v) => v.remediation(),
            Self::Telemetry(v) => v.remediation(),
            Self::Visual(v) => v.remediation(),
            Self::Contract(v) => v.remediation(),
        }
    }

    fn state_effect(&self) -> FailureStateEffect {
        match self {
            Self::Admission(v) => v.state_effect(),
            Self::Approval(v) => v.state_effect(),
            Self::Authentication(v) => v.state_effect(),
            Self::Container(v) => v.state_effect(),
            Self::Evaluation(v) => v.state_effect(),
            Self::Persistence(v) => v.state_effect(),
            Self::Provider(v) => v.state_effect(),
            Self::Session(v) => v.state_effect(),
            Self::Telemetry(v) => v.state_effect(),
            Self::Visual(v) => v.state_effect(),
            Self::Contract(v) => v.state_effect(),
        }
    }

    fn message(&self) -> String {
        match self {
            Self::Admission(v) => v.message(),
            Self::Approval(v) => v.message(),
            Self::Authentication(v) => v.message(),
            Self::Container(v) => v.message(),
            Self::Evaluation(v) => v.message(),
            Self::Persistence(v) => v.message(),
            Self::Provider(v) => v.message(),
            Self::Session(v) => v.message(),
            Self::Telemetry(v) => v.message(),
            Self::Visual(v) => v.message(),
            Self::Contract(v) => v.message(),
        }
    }

    fn safe_facts(&self) -> serde_json::Value {
        match self {
            Self::Admission(v) => v.safe_facts(),
            Self::Approval(v) => v.safe_facts(),
            Self::Authentication(v) => v.safe_facts(),
            Self::Container(v) => v.safe_facts(),
            Self::Evaluation(v) => v.safe_facts(),
            Self::Persistence(v) => v.safe_facts(),
            Self::Provider(v) => v.safe_facts(),
            Self::Session(v) => v.safe_facts(),
            Self::Telemetry(v) => v.safe_facts(),
            Self::Visual(v) => v.safe_facts(),
            Self::Contract(v) => v.safe_facts(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Ready,
    Unhealthy,
    Stopped,
    Unknown,
}

impl HealthStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Unhealthy => "unhealthy",
            Self::Stopped => "stopped",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "ready" | "healthy" => Self::Ready,
            "stopped" => Self::Stopped,
            "unhealthy" | "error" | "fail" | "failed" | "degraded" | "down" => Self::Unhealthy,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthSource {
    Registry,
    LiveProbe,
}

impl HealthSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Registry => "registry",
            Self::LiveProbe => "live_probe",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthObservation {
    pub source: HealthSource,
    pub status: HealthStatus,
    pub observed_at: DateTime<Utc>,
    pub http_status: Option<u16>,
    pub summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerFailure {
    NotFound {
        requested: String,
    },
    SelectionAmbiguous {
        candidates: Vec<String>,
    },
    Unhealthy {
        container_id: String,
        observation: HealthObservation,
    },
    HealthAuthorityConflict {
        container_id: String,
        registry: HealthObservation,
        live_probe: HealthObservation,
    },
    LaunchDeclarationMissing {
        container_id: String,
    },
    LaunchFailed {
        container_id: String,
        reason: String,
    },
    SourceRevisionMismatch {
        container_id: String,
        registered: String,
        observed: String,
    },
    ProtocolUnsupported {
        container_id: String,
        required: String,
        observed: Option<String>,
    },
    CapabilitiesStale {
        container_id: String,
        observed_at: Option<String>,
    },
    CapabilityMismatch {
        container_id: String,
        missing: Vec<String>,
    },
}

impl FailureDefinition for ContainerFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "container_not_found",
            Self::SelectionAmbiguous { .. } => "container_selection_ambiguous",
            Self::Unhealthy { .. } => "container_unhealthy",
            Self::HealthAuthorityConflict { .. } => "container_health_authority_conflict",
            Self::LaunchDeclarationMissing { .. } => "container_launch_declaration_missing",
            Self::LaunchFailed { .. } => "container_launch_failed",
            Self::SourceRevisionMismatch { .. } => "container_source_revision_mismatch",
            Self::ProtocolUnsupported { .. } => "container_protocol_unsupported",
            Self::CapabilitiesStale { .. } => "container_capabilities_stale",
            Self::CapabilityMismatch { .. } => "container_capability_mismatch",
        }
    }

    fn category(&self) -> FailureCategory {
        FailureCategory::Container
    }

    fn disposition(&self) -> FailureDisposition {
        match self {
            Self::NotFound { .. } | Self::SelectionAmbiguous { .. } => FailureDisposition::Terminal,
            Self::Unhealthy { .. } | Self::HealthAuthorityConflict { .. } => {
                FailureDisposition::ApprovalRequired
            }
            Self::LaunchDeclarationMissing { .. } | Self::ProtocolUnsupported { .. } => {
                FailureDisposition::RepairRequired
            }
            Self::LaunchFailed { .. } | Self::CapabilitiesStale { .. } => {
                FailureDisposition::Retryable
            }
            Self::SourceRevisionMismatch { .. } | Self::CapabilityMismatch { .. } => {
                FailureDisposition::RepairRequired
            }
        }
    }

    fn remediation(&self) -> Option<FailureRemediation> {
        match self {
            Self::Unhealthy { container_id, .. }
            | Self::HealthAuthorityConflict { container_id, .. }
            | Self::LaunchFailed { container_id, .. } => Some(FailureRemediation::ApproveRestart {
                container_id: container_id.clone(),
            }),
            Self::NotFound { .. } | Self::SelectionAmbiguous { .. } => {
                Some(FailureRemediation::OpenResource(ResourceRef::Containers))
            }
            Self::LaunchDeclarationMissing { .. } | Self::ProtocolUnsupported { .. } => {
                Some(FailureRemediation::OpenSettings(SettingsRoute::Containers))
            }
            Self::CapabilitiesStale { container_id, .. } => Some(FailureRemediation::Retry {
                resume_token: format!("probe:{container_id}"),
            }),
            Self::SourceRevisionMismatch { container_id, .. }
            | Self::CapabilityMismatch { container_id, .. } => {
                Some(FailureRemediation::Repair(RepairRequest {
                    target: container_id.clone(),
                    action: "redeclare_capabilities".into(),
                }))
            }
        }
    }

    fn state_effect(&self) -> FailureStateEffect {
        match self {
            Self::Unhealthy { .. } | Self::HealthAuthorityConflict { .. } => {
                FailureStateEffect::MarkContainerUnhealthy
            }
            Self::LaunchFailed { .. } | Self::LaunchDeclarationMissing { .. } => {
                FailureStateEffect::BlockContainerMutation
            }
            _ => FailureStateEffect::None,
        }
    }

    fn message(&self) -> String {
        match self {
            Self::NotFound { requested } => {
                format!("no registered container matches `{requested}`")
            }
            Self::SelectionAmbiguous { candidates } => format!(
                "{} registered containers match this request; Workshop will not choose one",
                candidates.len()
            ),
            Self::Unhealthy {
                container_id,
                observation,
            } => format!(
                "container `{container_id}` reported health `{}`",
                observation.status.as_str()
            ),
            Self::HealthAuthorityConflict { container_id, .. } => format!(
                "registry health for `{container_id}` contradicts the live probe"
            ),
            Self::LaunchDeclarationMissing { container_id } => {
                format!("container `{container_id}` has no versioned launch declaration")
            }
            Self::LaunchFailed {
                container_id,
                reason,
            } => format!("container `{container_id}` failed to launch: {reason}"),
            Self::SourceRevisionMismatch {
                container_id,
                registered,
                observed,
            } => format!(
                "container `{container_id}` source revision `{observed}` does not match `{registered}`"
            ),
            Self::ProtocolUnsupported {
                container_id,
                required,
                observed,
            } => match observed {
                Some(observed) => format!(
                    "container `{container_id}` advertises `{observed}`, not `{required}`"
                ),
                None => format!("container `{container_id}` advertises no live-eval protocol"),
            },
            Self::CapabilitiesStale { container_id, .. } => {
                format!("container `{container_id}` capability observation is stale")
            }
            Self::CapabilityMismatch {
                container_id,
                missing,
            } => format!(
                "container `{container_id}` is missing {}",
                missing.join(", ")
            ),
        }
    }

    fn safe_facts(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionFailure {
    CatalogRecipeNotFound { recipe_id: String, searched: usize },
    EvaluatorNotDeclared { container_id: String },
    ScoringContractInvalid { container_id: String, detail: String },
    PolicyNotFound { namespace: String, name: String },
    PolicyRevisionUnresolved { namespace: String, name: String },
    PolicyConfigurationInvalid { namespace: String, name: String, detail: String },
    ModelUnsupported { provider: String, model: String, container_id: String },
    SeedControlUnsupported { container_id: String },
    RequestedLimitUnsupported { limit: String, requested: u64, maximum_supported: Option<u64> },
    CostCeilingRequired,
    CredentialRouteUnavailable { provider: String, detail: String },
    OutputContractUnsupported { container_id: String, missing: Vec<String> },
    ExecutionSpecInvalid { detail: String },
    ExecutionSpecDigestMismatch { expected: String, actual: String },
    PolicySourceUnavailable { namespace: String, name: String },
}

impl FailureDefinition for AdmissionFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::CatalogRecipeNotFound { .. } => "catalog_recipe_not_found",
            Self::EvaluatorNotDeclared { .. } => "evaluator_not_declared",
            Self::ScoringContractInvalid { .. } => "scoring_contract_invalid",
            Self::PolicyNotFound { .. } => "policy_not_found",
            Self::PolicyRevisionUnresolved { .. } => "policy_revision_unresolved",
            Self::PolicyConfigurationInvalid { .. } => "policy_configuration_invalid",
            Self::ModelUnsupported { .. } => "model_unsupported",
            Self::SeedControlUnsupported { .. } => "seed_control_unsupported",
            Self::RequestedLimitUnsupported { .. } => "requested_limit_unsupported",
            Self::CostCeilingRequired => "cost_ceiling_required",
            Self::CredentialRouteUnavailable { .. } => "credential_route_unavailable",
            Self::OutputContractUnsupported { .. } => "output_contract_unsupported",
            Self::ExecutionSpecInvalid { .. } => "execution_spec_invalid",
            Self::ExecutionSpecDigestMismatch { .. } => "execution_spec_digest_mismatch",
            Self::PolicySourceUnavailable { .. } => "policy_source_unavailable",
        }
    }
    fn category(&self) -> FailureCategory {
        FailureCategory::Admission
    }
    fn disposition(&self) -> FailureDisposition {
        match self {
            Self::CredentialRouteUnavailable { .. } => FailureDisposition::RepairRequired,
            Self::PolicySourceUnavailable { .. } => FailureDisposition::Terminal,
            _ => FailureDisposition::Terminal,
        }
    }
    fn remediation(&self) -> Option<FailureRemediation> {
        match self {
            Self::CredentialRouteUnavailable { .. } => {
                Some(FailureRemediation::OpenSettings(SettingsRoute::Secrets))
            }
            Self::PolicySourceUnavailable { .. } => {
                Some(FailureRemediation::OpenSettings(SettingsRoute::Containers))
            }
            _ => Some(FailureRemediation::OpenResource(ResourceRef::Evaluations)),
        }
    }
    fn state_effect(&self) -> FailureStateEffect {
        match self {
            Self::PolicySourceUnavailable { .. } => FailureStateEffect::TerminalizeEvaluation,
            _ => FailureStateEffect::None,
        }
    }
    fn message(&self) -> String {
        match self {
            Self::CatalogRecipeNotFound { recipe_id, .. } => {
                format!("no catalog recipe is registered under `{recipe_id}`")
            }
            Self::EvaluatorNotDeclared { container_id } => format!(
                "container `{container_id}` declares no evaluator, so admission has no scoring semantics to pin"
            ),
            Self::ScoringContractInvalid { container_id, detail } => {
                format!("container `{container_id}` declared an unusable scoring contract: {detail}")
            }
            Self::PolicyNotFound { namespace, name } => {
                format!("no policy `{namespace}/{name}` is available to this session")
            }
            Self::PolicyRevisionUnresolved { namespace, name } => format!(
                "policy `{namespace}/{name}` resolved to no immutable revision"
            ),
            Self::PolicyConfigurationInvalid { namespace, name, detail } => {
                format!("policy `{namespace}/{name}` configuration is invalid: {detail}")
            }
            Self::ModelUnsupported { provider, model, container_id } => format!(
                "container `{container_id}` does not advertise support for `{provider}` model `{model}`"
            ),
            Self::SeedControlUnsupported { container_id } => {
                format!("container `{container_id}` does not accept caller-supplied seeds")
            }
            Self::RequestedLimitUnsupported { limit, requested, maximum_supported } => {
                match maximum_supported {
                    Some(maximum) => format!(
                        "requested {limit} of {requested} exceeds the supported maximum of {maximum}"
                    ),
                    None => format!("requested {limit} of {requested} is not supported"),
                }
            }
            Self::CostCeilingRequired => {
                "this specification spends provider credit and declares no cost ceiling".into()
            }
            Self::CredentialRouteUnavailable { provider, detail } => {
                format!("the credential route for `{provider}` is unavailable: {detail}")
            }
            Self::OutputContractUnsupported { container_id, .. } => format!(
                "container `{container_id}` does not advertise the evidence operations this specification requires"
            ),
            Self::ExecutionSpecInvalid { detail } => {
                format!("the execution specification is not admissible: {detail}")
            }
            Self::ExecutionSpecDigestMismatch { .. } => {
                "the execution specification changed after it was admitted".into()
            }
            Self::PolicySourceUnavailable { namespace, name } => {
                format!("policy source `{namespace}/{name}` is unavailable")
            }
        }
    }
    fn safe_facts(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalFailure {
    Denied { request_id: String, kind: String },
    BrokerUnavailable,
    DigestMismatch { expected: String, actual: String },
}

impl FailureDefinition for ApprovalFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::Denied { .. } => "approval_denied",
            Self::BrokerUnavailable => "approval_broker_unavailable",
            Self::DigestMismatch { .. } => "approval_digest_mismatch",
        }
    }
    fn category(&self) -> FailureCategory {
        FailureCategory::Approval
    }
    fn disposition(&self) -> FailureDisposition {
        match self {
            Self::Denied { .. } => FailureDisposition::Terminal,
            Self::BrokerUnavailable => FailureDisposition::Retryable,
            Self::DigestMismatch { .. } => FailureDisposition::Terminal,
        }
    }
    fn remediation(&self) -> Option<FailureRemediation> {
        match self {
            Self::BrokerUnavailable => Some(FailureRemediation::Retry {
                resume_token: "approval_broker".into(),
            }),
            _ => None,
        }
    }
    fn state_effect(&self) -> FailureStateEffect {
        FailureStateEffect::None
    }
    fn message(&self) -> String {
        match self {
            Self::Denied { kind, .. } => format!("operator denied {kind} approval"),
            Self::BrokerUnavailable => "approval broker is unavailable".into(),
            Self::DigestMismatch { .. } => {
                "the approved digest does not match the current specification".into()
            }
        }
    }
    fn safe_facts(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationFailure {
    Unauthorized,
    CredentialMissing { provider: String },
}

impl FailureDefinition for AuthenticationFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::CredentialMissing { .. } => "credential_missing",
        }
    }
    fn category(&self) -> FailureCategory {
        FailureCategory::Authentication
    }
    fn disposition(&self) -> FailureDisposition {
        FailureDisposition::RepairRequired
    }
    fn remediation(&self) -> Option<FailureRemediation> {
        Some(FailureRemediation::OpenSettings(SettingsRoute::Secrets))
    }
    fn state_effect(&self) -> FailureStateEffect {
        FailureStateEffect::None
    }
    fn message(&self) -> String {
        match self {
            Self::Unauthorized => "unauthorized".into(),
            Self::CredentialMissing { provider } => {
                format!("no credential is configured for `{provider}`")
            }
        }
    }
    fn safe_facts(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationFailure {
    ChildStillLive { run_id: String, rollout_id: String, state: String },
    TerminalManifestMissing { run_id: String },
    FailedEvidence { run_id: String, reason: String },
    CostUnavailable { run_id: String },
    MidRolloutContainerDeath { run_id: String, rollout_id: String, container_id: String },
}

impl FailureDefinition for EvaluationFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::ChildStillLive { .. } => "evaluation_child_still_live",
            Self::TerminalManifestMissing { .. } => "evaluation_terminal_manifest_missing",
            Self::FailedEvidence { .. } => "failed_evidence",
            Self::CostUnavailable { .. } => "cost_telemetry_unavailable",
            Self::MidRolloutContainerDeath { .. } => "evaluation_rollout_container_death",
        }
    }
    fn category(&self) -> FailureCategory {
        FailureCategory::Evaluation
    }
    fn disposition(&self) -> FailureDisposition {
        match self {
            Self::CostUnavailable { .. } => FailureDisposition::Terminal,
            Self::MidRolloutContainerDeath { .. } => FailureDisposition::RepairRequired,
            _ => FailureDisposition::Terminal,
        }
    }
    fn remediation(&self) -> Option<FailureRemediation> {
        match self {
            Self::MidRolloutContainerDeath { container_id, .. } => {
                Some(FailureRemediation::ApproveRestart {
                    container_id: container_id.clone(),
                })
            }
            Self::CostUnavailable { .. } => None,
            _ => Some(FailureRemediation::OpenResource(ResourceRef::Evaluations)),
        }
    }
    fn state_effect(&self) -> FailureStateEffect {
        FailureStateEffect::TerminalizeEvaluation
    }
    fn message(&self) -> String {
        match self {
            Self::ChildStillLive { rollout_id, state, .. } => {
                format!("parent cannot terminalize while rollout `{rollout_id}` is `{state}`")
            }
            Self::TerminalManifestMissing { run_id } => {
                format!("run `{run_id}` is terminal without a sealed manifest")
            }
            Self::FailedEvidence { reason, .. } => format!("evidence failed: {reason}"),
            Self::CostUnavailable { run_id } => {
                format!("cost telemetry for run `{run_id}` is unavailable")
            }
            Self::MidRolloutContainerDeath { rollout_id, container_id, .. } => format!(
                "container `{container_id}` died during rollout `{rollout_id}`"
            ),
        }
    }
    fn safe_facts(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceFailure {
    DatabaseLocked,
    SqliteUnavailable { detail: String },
    HistoricalUnclassified { source_table: String, source_id: String },
}

impl FailureDefinition for PersistenceFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::DatabaseLocked => "database_locked",
            Self::SqliteUnavailable { .. } => CODE_BOOTSTRAP_UNAVAILABLE,
            Self::HistoricalUnclassified { .. } => CODE_HISTORICAL_UNCLASSIFIED,
        }
    }
    fn category(&self) -> FailureCategory {
        FailureCategory::Persistence
    }
    fn disposition(&self) -> FailureDisposition {
        match self {
            Self::DatabaseLocked => FailureDisposition::Retryable,
            Self::SqliteUnavailable { .. } => FailureDisposition::RepairRequired,
            Self::HistoricalUnclassified { .. } => FailureDisposition::Terminal,
        }
    }
    fn remediation(&self) -> Option<FailureRemediation> {
        match self {
            Self::DatabaseLocked => Some(FailureRemediation::Retry {
                resume_token: "sqlite".into(),
            }),
            Self::SqliteUnavailable { .. } => Some(FailureRemediation::OpenDiagnostics),
            Self::HistoricalUnclassified { .. } => None,
        }
    }
    fn state_effect(&self) -> FailureStateEffect {
        match self {
            Self::SqliteUnavailable { .. } => FailureStateEffect::RecordEmergencyMode,
            _ => FailureStateEffect::None,
        }
    }
    fn message(&self) -> String {
        match self {
            Self::DatabaseLocked => "database is locked".into(),
            Self::SqliteUnavailable { detail } => {
                format!("SQLite is unavailable at bootstrap: {detail}")
            }
            Self::HistoricalUnclassified { source_table, source_id } => {
                format!("historical {source_table} row `{source_id}` could not be classified")
            }
        }
    }
    fn safe_facts(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFailure {
    Unavailable { provider: String, detail: String },
    ProtocolMismatch { expected: String, observed: String },
}

impl FailureDefinition for ProviderFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::Unavailable { .. } => "provider_unavailable",
            Self::ProtocolMismatch { .. } => "protocol_mismatch",
        }
    }
    fn category(&self) -> FailureCategory {
        FailureCategory::Provider
    }
    fn disposition(&self) -> FailureDisposition {
        match self {
            Self::Unavailable { .. } => FailureDisposition::Retryable,
            Self::ProtocolMismatch { .. } => FailureDisposition::Terminal,
        }
    }
    fn remediation(&self) -> Option<FailureRemediation> {
        match self {
            Self::Unavailable { .. } => Some(FailureRemediation::Retry {
                resume_token: "provider".into(),
            }),
            Self::ProtocolMismatch { .. } => None,
        }
    }
    fn state_effect(&self) -> FailureStateEffect {
        FailureStateEffect::None
    }
    fn message(&self) -> String {
        match self {
            Self::Unavailable { provider, detail } => {
                format!("provider `{provider}` is unavailable: {detail}")
            }
            Self::ProtocolMismatch { expected, observed } => {
                format!("protocol mismatch: expected {expected}, got {observed}")
            }
        }
    }
    fn safe_facts(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionFailure {
    Detached { session_id: String, reason: String },
    LeaseExpired { session_id: String },
    TurnNotPersisted { session_id: String },
}

impl FailureDefinition for SessionFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::Detached { .. } => "session_detached",
            Self::LeaseExpired { .. } => "session_lease_expired",
            Self::TurnNotPersisted { .. } => "run_not_persisted",
        }
    }
    fn category(&self) -> FailureCategory {
        FailureCategory::Session
    }
    fn disposition(&self) -> FailureDisposition {
        match self {
            Self::Detached { .. } | Self::LeaseExpired { .. } => FailureDisposition::Retryable,
            Self::TurnNotPersisted { .. } => FailureDisposition::Terminal,
        }
    }
    fn remediation(&self) -> Option<FailureRemediation> {
        match self {
            Self::Detached { session_id, .. } | Self::LeaseExpired { session_id } => {
                Some(FailureRemediation::Retry {
                    resume_token: format!("session:{session_id}"),
                })
            }
            Self::TurnNotPersisted { .. } => None,
        }
    }
    fn state_effect(&self) -> FailureStateEffect {
        FailureStateEffect::TerminalizeSessionTurn
    }
    fn message(&self) -> String {
        match self {
            Self::Detached { session_id, reason } => {
                format!("session `{session_id}` detached ({reason})")
            }
            Self::LeaseExpired { session_id } => {
                format!("session `{session_id}` owner lease expired")
            }
            Self::TurnNotPersisted { session_id } => {
                format!("session `{session_id}` turn was not persisted")
            }
        }
    }
    fn safe_facts(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryFailure {
    CostUnavailable { run_id: Option<String> },
    IndexDegraded { reason: String },
}

impl FailureDefinition for TelemetryFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::CostUnavailable { .. } => "cost_telemetry_unavailable",
            Self::IndexDegraded { .. } => "diagnostics_index_degraded",
        }
    }
    fn category(&self) -> FailureCategory {
        FailureCategory::Telemetry
    }
    fn disposition(&self) -> FailureDisposition {
        FailureDisposition::Terminal
    }
    fn remediation(&self) -> Option<FailureRemediation> {
        match self {
            Self::IndexDegraded { .. } => Some(FailureRemediation::OpenDiagnostics),
            Self::CostUnavailable { .. } => None,
        }
    }
    fn state_effect(&self) -> FailureStateEffect {
        match self {
            Self::IndexDegraded { .. } => FailureStateEffect::DegradeIndex,
            Self::CostUnavailable { .. } => FailureStateEffect::None,
        }
    }
    fn message(&self) -> String {
        match self {
            Self::CostUnavailable { .. } => "cost telemetry is unavailable".into(),
            Self::IndexDegraded { reason } => {
                format!("diagnostics index is degraded: {reason}")
            }
        }
    }
    fn safe_facts(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualFailure {
    RenderFailed { visual_id: String, detail: String },
    BindingUnresolved { visual_id: String },
}

impl FailureDefinition for VisualFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::RenderFailed { .. } => "visual_render_failed",
            Self::BindingUnresolved { .. } => "visual_binding_unresolved",
        }
    }
    fn category(&self) -> FailureCategory {
        FailureCategory::Visual
    }
    fn disposition(&self) -> FailureDisposition {
        FailureDisposition::RepairRequired
    }
    fn remediation(&self) -> Option<FailureRemediation> {
        Some(FailureRemediation::OpenResource(ResourceRef::Visuals))
    }
    fn state_effect(&self) -> FailureStateEffect {
        FailureStateEffect::MarkVisualFailed
    }
    fn message(&self) -> String {
        match self {
            Self::RenderFailed { visual_id, detail } => {
                format!("visual `{visual_id}` failed to render: {detail}")
            }
            Self::BindingUnresolved { visual_id } => {
                format!("visual `{visual_id}` has an unresolved binding")
            }
        }
    }
    fn safe_facts(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractFailure {
    InvalidEnvelope { detail: String },
}

impl FailureDefinition for ContractFailure {
    fn code(&self) -> &'static str {
        CODE_CONTRACT_INVALID
    }
    fn category(&self) -> FailureCategory {
        FailureCategory::Contract
    }
    fn disposition(&self) -> FailureDisposition {
        FailureDisposition::ProgrammerError
    }
    fn remediation(&self) -> Option<FailureRemediation> {
        Some(FailureRemediation::OpenDiagnostics)
    }
    fn state_effect(&self) -> FailureStateEffect {
        FailureStateEffect::None
    }
    fn message(&self) -> String {
        match self {
            Self::InvalidEnvelope { detail } => {
                format!("failure contract is invalid: {detail}")
            }
        }
    }
    fn safe_facts(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}
