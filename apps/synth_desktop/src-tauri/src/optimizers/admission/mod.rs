//! Inline-first evaluation admission.
//!
//! An evaluation runs whenever Workshop can build a complete, immutable
//! specification out of the request, the selected container's declaration, the
//! policy and model, and explicit bounds. A catalog recipe is a reusable
//! preset, not a prerequisite — nothing on this path queries the catalog unless
//! the caller explicitly asked for a catalog recipe by id.
//!
//! The pipeline is a sequence of distinct types, so an unvalidated draft simply
//! cannot be handed to the executor:
//!
//! ```text
//! RecipeSource
//!     → materialize      → ExecutionSpecDraft
//!     → validate and pin → ValidatedExecutionSpec
//!     → canonicalize     → AdmissibleExecutionSpec
//!     → approve          → ApprovedExecutionSpec
//!     → execute
//! ```
//!
//! Both sources converge at `materialize`, so validation, hashing, approval,
//! execution, and persistence exist once rather than once per source.

pub mod canonical;
pub mod error;
pub mod ids;
pub mod persistence;
pub mod pipeline;
pub mod spec;
pub mod state;

#[cfg(test)]
mod tests;

pub use canonical::{digest_bytes, digest_of, CanonicalError, CanonicalJson};
pub use error::{AdmissionError, AdmissionErrorCode, AdmissionSubject};
pub use ids::{
    ApprovalReceiptId, ContainerId, ContainerRegistrationId, CostMicros, DeclarationDigest, Digest,
    EvaluatorId, ModelCallCount, ModelId, PolicyRevision, ProviderId, RecipeDigest, RecipeId,
    RolloutCount, RolloutId, Seed, SourceRevision, StepCount,
};
pub use persistence::{
    consume_approved_eval_draft, load_admitted_execution_spec, stage_admissible,
    stage_approved_eval_draft,
};
pub use pipeline::{
    materialize, AdmissibleExecutionSpec, ApprovedExecutionSpec, ContainerCandidate,
    DeclaredEvaluator, DiscoveryContext, EvalDeclaration, ExecutionSpecDraft, InlineRequest,
    PolicyResolution, ValidatedExecutionSpec,
};
pub(crate) use spec::provider_use_policy_from_bounds;
pub use spec::{
    ApprovalBinding, CatalogRecipeRef, ContainerPin, CredentialCapabilityScope, CredentialRoute,
    EvaluatorSpec, ExecutionSpec, InlineRecipe, LiveEvalProtocol, ModelPin, OutputContract,
    PolicyMaterialRef, PolicyPin, RecipeSource, RecipeSourceKind, ResourceLimits, RolloutPlan,
    LIVE_EVAL_PROTOCOL_V1,
};
pub use state::{
    EvidenceGap, EvidenceRequirements, RolloutRecord, RolloutState, RolloutStateHolder,
    RunProgress, RunState, SettlementRefusal, StateTransitionError, TransitionKind,
};
