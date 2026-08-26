//! Lineage graph: stored nodes/edges plus the `DagView` canvas projection.
//!
//! Experiment identity stays in `crate::experiments`. This module does not own
//! session grouping or member attach. Nodes are members; edges are typed facts.

mod models;
pub mod store;

pub use models::{DagView, ExperimentEdge, ExperimentEvidenceRef, ExperimentNode};
