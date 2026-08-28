//! One durable, algorithm-independent run kernel.
//!
//! Optimizers owns run domain, reducers, evidence, and terminal semantics.
//! CoreRuntime remains the only product database writer. Producer journals are
//! recovery logs, not competing product records.

pub mod admission;
pub mod algorithm;
pub mod algorithms;
pub mod bridge;
pub mod commit;
pub mod driver;
pub mod error;
pub mod evidence;
pub mod outbox;
pub mod persist;
pub mod sequences;
pub mod settle;
pub mod types;
pub mod view;
pub mod work;

pub use admission::{AdmissionCommit, RunDraft};
pub use algorithm::{AlgorithmProjection, AlgorithmResult};
pub use commit::{commit, settle_result, CommitPlan, RunKernelState};
pub use driver::{resolve as resolve_driver, DriverKind, ExternalRunRef, SealedRunSpec};
pub use error::{KernelError, KernelErrorCode, KernelResult};
pub use evidence::{EvidenceAmendment, EvidenceState, SealedTerminal, UsageCompleteness};
pub use sequences::{
    assign_aggregate_sequences, plan_producer_batch, CommittedEvent, DurableProducerLog,
    ProducerEvent, ProducerVerdict,
};
pub use settle::SettleCause;
pub use types::{
    classify_legacy_status, AdmissionState, AlgorithmKind, CancellationCause, CancellationRequest,
    CancelledError, EvidenceCompleteness, ExecutionPlacement, RunCondition, RunLifecycle, RunPhase,
    TerminalKind, TerminalReason, WorkItemKind, WorkItemLifecycle, GELO_HOSTED_RECIPE_ID,
    KERNEL_SCHEMA_VERSION, PRODUCER_EVENT_SCHEMA_VERSION, RUN_VIEW_SCHEMA_VERSION,
};
pub use view::{project_view, project_view_with_context, OptimizerRunViewV2};
pub use work::{WorkItem, WorkSummary};

