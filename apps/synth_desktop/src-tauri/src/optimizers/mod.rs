//! First-class Optimizer noun: durable local mirror, cursor, relationships, and projection.

mod cispo;
pub(crate) mod cloud;
mod container_eval;
mod eval_candidates;
mod eval_recipes;
pub(crate) mod eval_runtime;
mod events;
mod gepa_evidence;
mod hosted_client;
mod hosted_gelo;
mod hosted_sft;
mod ingest;
mod local;
mod local_lora;
pub(crate) mod manager;
mod mlx_runtime;
mod mlx_sft;
mod models;
mod normalize;
mod recipes;
mod results;
mod service;
mod sft_client;
mod sft_recipes;
mod sft_result;
mod sidecar_training;
mod terminal;
mod tinker_catalog;
mod training;
mod training_adapter;
pub(crate) mod typed_capabilities;

pub use eval_candidates::EvalStageCandidatesRequest;
pub(crate) use eval_recipes::{paid_compute_bounds, resolve_eval_candidate_set};
pub(crate) use sidecar_training::launch_artifact_inference;
#[allow(unused_imports)] // public sidecar status/version types for Desktop callers
pub use manager::{OptimizerManager, OptimizerSidecarStatus, OptimizerSidecarVersion};
#[allow(unused_imports)] // Nested Specta type is part of HostedTrainingModelCatalog.
pub use models::{
    CheckpointInferRequest, HostedTrainingModel, HostedTrainingModelCatalog, OptimizerCreateRequest,
    OptimizerEventEnvelope, OptimizerImportLocalRequest, OptimizerQuery, OptimizerRecipeRunRequest,
    OptimizerReconcileRequest, OptimizerRelationship, OptimizerRunOutputArtifact,
    OptimizerRunStatus,
    OptimizerRunOutputCounts, OptimizerRunOutputIdentity, OptimizerRunOutputs, OptimizerRunRecord,
    OptimizerStateSlice, SavedLoraCheckpoint, SavedLoraCheckpointPage, SavedLoraCheckpointQuery,
    SavedLoraDownload, SavedLoraLineage, SavedLoraPatchRequest, SavedLoraRunCounts,
    SavedLoraRunIdentity, SavedLoraRunPage,
};
pub(crate) use recipes::{BANKING77_EVAL_BASELINE_RECIPE, HEALTHBENCH_EVAL_SMOKE_RECIPE};
pub use service::OptimizerService;
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
