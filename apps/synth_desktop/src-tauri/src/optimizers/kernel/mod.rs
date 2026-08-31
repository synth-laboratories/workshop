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
pub use view::{
    project_view, project_view_with_context, OptimizerRunViewEnvelope, OptimizerRunViewV2,
};
pub use work::{WorkItem, WorkSummary};

#[cfg(test)]
mod lifecycle_graph_tests {
    use super::*;

    #[test]
    fn run_lifecycle_graph_is_closed_and_terminal_is_absorbing() {
        for from in RunLifecycle::ALL {
            for to in RunLifecycle::ALL {
                let ok = from.may_transition_to(*to);
                if from.is_terminal() {
                    assert_eq!(ok, from == to, "{from:?} -> {to:?}");
                }
                if ok {
                    assert_eq!(from.transition_to(*to).unwrap(), *to);
                } else {
                    assert_eq!(
                        from.transition_to(*to).unwrap_err().code,
                        KernelErrorCode::LifecycleTransitionInvalid
                    );
                }
            }
        }
        assert!(RunLifecycle::Running.may_transition_to(RunLifecycle::Paused));
        assert!(RunLifecycle::Paused.may_transition_to(RunLifecycle::Running));
        assert!(!RunLifecycle::Queued.may_transition_to(RunLifecycle::Paused));
        assert!(RunLifecycle::Queued.may_transition_to(RunLifecycle::Running));
        assert!(!RunLifecycle::Cancelling.may_transition_to(RunLifecycle::Running));
    }

    #[test]
    fn work_item_graph_requires_identity_and_rejects_skips() {
        let mut item = WorkItem::planned("w1", WorkItemKind::EvalTrial).unwrap();
        assert!(item.transition(WorkItemLifecycle::Running).is_err());
        item.transition(WorkItemLifecycle::Queued).unwrap();
        item.transition(WorkItemLifecycle::Starting).unwrap();
        item.transition(WorkItemLifecycle::Running).unwrap();
        item.seal_terminal(TerminalKind::Completed).unwrap();
        assert!(item.transition(WorkItemLifecycle::Queued).is_err());
    }

    #[test]
    fn unknown_algorithm_never_defaults() {
        assert_eq!(
            AlgorithmKind::parse_wire("environment").unwrap_err().code,
            KernelErrorCode::UnknownAlgorithm
        );
    }
}
