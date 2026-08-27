//! First-class Optimizer noun: durable local mirror, cursor, relationships, and projection.

pub mod admission;
mod cispo;
pub(crate) mod cloud;
mod container_eval;
pub(crate) mod container_lifecycle;
mod container_training;
mod eval_candidates;
mod eval_recipes;
mod eval_relay;
pub(crate) mod eval_runtime;
mod events;
pub mod inline_eval;
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
pub(crate) mod workspace_recipe;

pub use eval_candidates::EvalStageCandidatesRequest;
pub(crate) use eval_recipes::{paid_compute_bounds, resolve_eval_candidate_set};
pub use frames::{OptimizerFrameContent, OptimizerFrameDelta, OptimizerFrameRef};
#[allow(unused_imports)] // public sidecar status/version types for Desktop callers
pub use manager::{OptimizerManager, OptimizerSidecarStatus, OptimizerSidecarVersion};
#[allow(unused_imports)] // Nested Specta type is part of HostedTrainingModelCatalog.
pub use models::{
    CheckpointInferRequest, HostedTrainingModel, HostedTrainingModelCatalog,
    OptimizerCreateRequest, OptimizerEventEnvelope, OptimizerImportLocalRequest, OptimizerQuery,
    OptimizerRecipeRunRequest, OptimizerReconcileRequest, OptimizerRelationship,
    OptimizerRunOutputArtifact, OptimizerRunOutputCounts, OptimizerRunOutputIdentity,
    OptimizerRunOutputs, OptimizerRunRecord, OptimizerRunStatus, OptimizerStateSlice,
    SavedLoraCheckpoint, SavedLoraCheckpointPage, SavedLoraCheckpointQuery, SavedLoraDownload,
    SavedLoraLineage, SavedLoraPatchRequest, SavedLoraRunCounts, SavedLoraRunIdentity,
    SavedLoraRunPage,
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
