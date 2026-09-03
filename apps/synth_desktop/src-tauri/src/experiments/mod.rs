//! Local-first experiment registry.
//!
//! A chat that starts an evaluation campaign and a GEPA run should be able to
//! name both as members of the same experiment without either leaking into
//! another task's right pane.
//!
//! Lineage nodes/edges are owned by `crate::lineage`. This module keeps
//! experiment identity, members, and evidence attach.

mod candidates;
mod evidence;
mod models;
mod registry;

pub use crate::lineage::{CandidateRecord, ExperimentEdge, ExperimentEvidenceRef, ExperimentNode};
pub use candidates::{upsert as upsert_candidate, CandidateUpsert};
pub use evidence::{attach_evidence, attach_member_evidence, ExperimentEvidenceAttachRequest};
pub use models::{
    ExperimentChildCreateRequest, ExperimentCreateRequest, ExperimentFinalizeRequest,
    ExperimentGroup, ExperimentLineageEdge, ExperimentMember, ExperimentRelateRequest,
    ExperimentUpdateRequest, ResearchJournalAppendRequest, ResearchJournalEntry, MEMBER_OPTIMIZER,
};
#[cfg(test)]
pub use models::{MEMBER_CAMPAIGN, MEMBER_DIRECT};
pub use registry::{
    activate, attach, create, create_child, finalize, get, list, load_for_session, relate,
    research_log_append, research_log_list, settle_member, update,
};

#[cfg(test)]
mod tests;
