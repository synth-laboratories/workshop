//! First-class Optimizer noun: durable local mirror, cursor, relationships, and projection.

mod cloud;
mod container_eval;
mod eval_candidates;
mod eval_recipes;
mod hosted_client;
mod hosted_gelo;
mod hosted_sft;
mod ingest;
mod local;
pub(crate) mod manager;
mod models;
mod normalize;
mod recipes;
mod service;
mod sft_client;
mod sft_recipes;
mod tinker_catalog;

pub use eval_candidates::EvalStageCandidatesRequest;
pub(crate) use eval_recipes::paid_compute_bounds;
pub(crate) use recipes::{
    BANKING77_EVAL_BASELINE_RECIPE, HEALTHBENCH_EVAL_SMOKE_RECIPE,
};
#[allow(unused_imports)] // public sidecar status/version types for Desktop callers
pub use manager::{OptimizerManager, OptimizerSidecarStatus, OptimizerSidecarVersion};
pub use models::{
    OptimizerCreateRequest, OptimizerEventEnvelope, OptimizerImportLocalRequest, OptimizerQuery,
    OptimizerRecipeRunRequest, OptimizerReconcileRequest, OptimizerRelationship,
    OptimizerRunRecord, OptimizerStateSlice,
};
pub use service::OptimizerService;
