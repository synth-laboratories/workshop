//! Experiment identity and members. Lineage graph types live in `crate::lineage`.

use serde::{Deserialize, Serialize};

use crate::contract::specta::OpaqueJson;
use crate::lineage::{ExperimentEdge, ExperimentNode};

pub const MEMBER_CAMPAIGN: &str = "eval_campaign";
pub const MEMBER_OPTIMIZER: &str = "optimizer_run";
pub const MEMBER_DIRECT: &str = "direct_evaluation";

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentCreateRequest {
    pub session_id: String,
    pub request_id: String,
    pub title: String,
    pub task: Option<String>,
    pub model: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentFinalizeRequest {
    pub experiment_id: String,
    pub session_id: String,
    pub status: String,
    pub result: OpaqueJson,
    pub assessment: Option<OpaqueJson>,
    pub finalized_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentMember {
    pub member_kind: String,
    pub member_id: String,
    pub title: String,
    pub attached_at: String,
}

/// Composition DTO: experiment row + members + lineage projection.
/// Assembled at the command boundary; not a stored graph blob.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentGroup {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
    pub task: Option<String>,
    pub model: Option<String>,
    pub best_result: Option<OpaqueJson>,
    pub members: Vec<ExperimentMember>,
    pub nodes: Vec<ExperimentNode>,
    pub edges: Vec<ExperimentEdge>,
    #[serde(default)]
    pub lineage: Vec<ExperimentLineageEdge>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentChildCreateRequest {
    pub parent_experiment_id: String,
    pub session_id: Option<String>,
    pub request_id: String,
    pub title: String,
    pub task: Option<String>,
    pub model: Option<String>,
    pub created_at: String,
    /// `follow_up` (default) | `forked_from` | `rerun_of`. Unknown fails closed.
    #[serde(default)]
    pub relation: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentRelateRequest {
    pub experiment_id: String,
    /// `compared_with` | `promoted_to`. Unknown fails closed.
    pub relation: String,
    /// `member` | `candidate`. Mixed kinds fail closed.
    pub source_kind: String,
    pub source_id: String,
    pub target_kind: String,
    pub target_id: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentLineageEdge {
    pub id: String,
    pub source_experiment_id: String,
    pub target_experiment_id: String,
    pub relation: String,
    pub created_at: String,
}
