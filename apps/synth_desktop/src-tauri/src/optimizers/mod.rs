//! First-class Optimizer noun: durable local mirror, cursor, relationships, and projection.

pub mod admission;
pub(crate) mod annotation_stage;
mod artifacts;
mod cispo;
mod cispo_client;
pub(crate) mod cloud;
mod container_eval;
pub(crate) mod container_lifecycle;
mod container_training;
mod effective_contract;
mod eval_candidates;
mod eval_recipes;
mod eval_relay;
pub(crate) mod eval_runtime;
mod event_contract;
pub mod events;
pub mod inline_eval;
pub(crate) mod live_annotation;
pub(crate) use events::strip_frame_bodies_for_ipc;
mod experiment_bind;
mod frames;
mod gepa_evidence;
mod hosted_client;
mod hosted_gelo;
mod hosted_sft;
mod ingest;
pub mod kernel;
mod local;
mod local_lora;
pub(crate) mod manager;
pub mod mlx_runtime;
mod mlx_sft;
pub mod models;
mod normalize;
mod recipes;
mod results;
mod service;
mod snapshot;
pub use snapshot::{OptimizerSnapshotImportRequest, OptimizerSnapshotReceipt};
mod sft_client;
mod sft_recipes;
mod sft_result;
mod sidecar_training;
mod terminal;
mod tinker_catalog;
mod training;
mod training_adapter;
pub(crate) mod typed_capabilities;
pub(crate) mod workspace_recipe;

/// The cancel signal for a local run worker. `None` until a typed
/// [`kernel::CancellationRequest`] arrives; observers read what cancelled
/// them, not a bare boolean.
pub(crate) type CancelSignal =
    tokio::sync::watch::Sender<Option<std::sync::Arc<kernel::CancellationRequest>>>;
pub(crate) type CancelObserver =
    tokio::sync::watch::Receiver<Option<std::sync::Arc<kernel::CancellationRequest>>>;

pub use eval_candidates::EvalStageCandidatesRequest;
pub(crate) use eval_recipes::{paid_compute_bounds, resolve_eval_candidate_set};
pub use frames::{OptimizerFrameContent, OptimizerFrameDelta, OptimizerFrameRef};
#[allow(unused_imports)] // public sidecar status/version types for Desktop callers
pub use manager::{OptimizerManager, OptimizerSidecarStatus, OptimizerSidecarVersion};
#[allow(unused_imports)] // Nested Specta type is part of HostedTrainingModelCatalog.
pub use models::{
    CheckpointInferRequest, EffectiveContract, HostedTrainingModel, HostedTrainingModelCatalog,
    OptimizerArtifactPage, OptimizerArtifactRange, OptimizerCreateRequest, OptimizerEventEnvelope,
    OptimizerImportLocalRequest, OptimizerQuery, OptimizerRecipeRunRequest,
    OptimizerReconcileRequest, OptimizerRelationship, OptimizerRunOutputArtifact,
    OptimizerRunOutputCounts, OptimizerRunOutputIdentity, OptimizerRunOutputs, OptimizerRunRecord,
    OptimizerRunStatus, OptimizerStateSlice, SavedLoraCheckpoint, SavedLoraCheckpointPage,
    SavedLoraCheckpointQuery, SavedLoraDownload, SavedLoraLineage, SavedLoraPatchRequest,
    SavedLoraRunCounts, SavedLoraRunIdentity, SavedLoraRunPage,
};
pub(crate) use service::reconcile_stale_local_runs_in_tx;
pub use service::OptimizerService;
pub(crate) use sidecar_training::launch_artifact_inference;
pub use training::{TrainingEvent, TrainingLifecycle, TrainingProjection};

/// The adapter-tree digest the catalog keys on. Re-exported so the installer
/// and the publisher share one definition of identity.
pub fn digest_adapter_directory(root: &std::path::Path) -> anyhow::Result<String> {
    local_lora::digest_directory(root)
}

pub use local_lora::durable_lora_root;

pub fn local_lora_is_laguna_compatible(checkpoint: &SavedLoraCheckpoint) -> bool {
    local_lora::is_laguna_compatible(checkpoint)
}

/// A module's own source with every `#[cfg(test)]` item removed.
///
/// The guards that scan production source for forbidden URLs used to take
/// everything before the *first* `#[cfg(test)]`. That silently stopped covering
/// production the moment a second test module was added above the functions
/// being guarded: the scan then ran over a prefix containing none of them, and
/// the guard failed instead of guarding. Strip every annotated item instead, so
/// the guard sees all of the production source and none of the tests.
///
/// Test modules are top level and rustfmt closes them with a `}` in column
/// zero, which is the boundary this walks to.
#[cfg(test)]
pub(crate) fn production_source(source: &str) -> String {
    let mut kept = Vec::new();
    let mut skipping = false;
    for line in source.lines() {
        if !skipping && line.trim_start().starts_with("#[cfg(test)]") {
            skipping = true;
            continue;
        }
        if skipping {
            if line == "}" {
                skipping = false;
            }
            continue;
        }
        kept.push(line);
    }
    kept.join("\n")
}

#[cfg(test)]
mod production_source_tests {
    #[test]
    fn strips_every_test_module_not_just_the_first() {
        let source = "fn a() {}\n#[cfg(test)]\nmod early {\n    fn t() {}\n}\nfn guarded() {}\n#[cfg(test)]\nmod tests {\n    fn u() {}\n}\n";
        let production = super::production_source(source);
        assert!(production.contains("fn guarded()"), "{production}");
        assert!(!production.contains("mod early"), "{production}");
        assert!(!production.contains("mod tests"), "{production}");
    }
}
