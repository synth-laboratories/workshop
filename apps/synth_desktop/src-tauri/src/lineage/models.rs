//! Lineage graph payload. The canvas consumes these; it does not own experiment identity.

use serde::{Deserialize, Serialize};

use crate::contract::specta::OpaqueJson;

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentEvidenceRef {
    pub evidence_id: String,
    pub kind: String,
    pub label: String,
    pub digest: Option<String>,
    pub container_id: Option<String>,
    pub rollout_id: Option<String>,
    pub trace_id: Option<String>,
    pub visual_id: Option<String>,
    pub artifact_uri: Option<String>,
    pub metadata: OpaqueJson,
    pub attached_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentNode {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub status: String,
    pub config: OpaqueJson,
    pub metrics: Option<OpaqueJson>,
    pub cost_usd: Option<f64>,
    pub artifact_refs: Vec<String>,
    pub trace_refs: Vec<String>,
    pub evidence_refs: Vec<ExperimentEvidenceRef>,
    pub provenance: OpaqueJson,
    pub created_at: String,
    pub updated_at: String,
    /// Durable Candidate rows for an `optimizer_run` member. Empty for other
    /// kinds and for SFT/CISPO runs that never emitted candidate identity.
    #[serde(default)]
    pub candidates: Vec<CandidateRecord>,
}

/// Producer identity projected onto an `optimizer_run` member. Not a member
/// kind and not an experiment edge.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CandidateRecord {
    pub id: String,
    pub experiment_id: String,
    pub optimizer_run_id: String,
    pub producer_candidate_id: String,
    pub kind: Option<String>,
    pub protocol_id: Option<String>,
    pub status: Option<String>,
    #[serde(default)]
    pub parent_ids: Vec<String>,
    pub metrics: Option<OpaqueJson>,
    pub content_digest: Option<String>,
    #[serde(default)]
    pub compared_with: Vec<String>,
    pub promoted_to: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentEdge {
    pub id: String,
    pub source_node_id: String,
    pub target_node_id: String,
    pub relation: String,
    pub created_at: String,
}

/// Read model for the lineage canvas. Not a stored table.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DagView {
    pub nodes: Vec<ExperimentNode>,
    pub edges: Vec<ExperimentEdge>,
}
