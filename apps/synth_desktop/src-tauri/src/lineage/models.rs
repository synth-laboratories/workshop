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
