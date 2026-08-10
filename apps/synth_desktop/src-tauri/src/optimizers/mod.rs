//! First-class Optimizer noun: durable local mirror, cursor, relationships, and projection.

mod cloud;
mod local;
mod models;
mod normalize;
mod recipes;
mod service;
mod sft_recipes;

pub use models::{
    OptimizerCreateRequest, OptimizerEventEnvelope, OptimizerImportLocalRequest, OptimizerQuery,
    OptimizerRecipeRunRequest, OptimizerReconcileRequest, OptimizerRelationship,
    OptimizerRunRecord, OptimizerStateSlice,
};
pub use service::OptimizerService;
