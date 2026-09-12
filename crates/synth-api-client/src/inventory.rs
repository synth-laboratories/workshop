//! Runtime inventory from the pinned research schema; no global cleanup claim.
use crate::RuntimeKind;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum InventoryCoverage {
    #[serde(rename = "registered-runtime-resources-v1")]
    RegisteredRuntimeResourcesV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    #[serde(rename = "self")]
    Self_,
    Owned,
    Borrowed,
    Shared,
    Retained,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Settled,
    Pending,
    Unknown,
    Excluded,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceDisposition {
    pub resource_kind: String,
    pub resource_id: String,
    pub cleanup_owner_run_id: Option<String>,
    pub relation: Relation,
    pub disposition: Disposition,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeInventory {
    pub runtime_kind: RuntimeKind,
    pub runtime_id: String,
    pub observed_at: String,
    pub coverage: InventoryCoverage,
    pub coverage_complete: bool,
    pub incomplete_reasons: Vec<String>,
    pub resources: Vec<ResourceDisposition>,
}

impl RuntimeInventory {
    pub fn validate_identity(&self, kind: RuntimeKind, id: &str) -> Result<(), &'static str> {
        if self.runtime_kind != kind || self.runtime_id != id || self.observed_at.trim().is_empty()
        {
            return Err("runtime inventory identity is invalid");
        }
        if self.coverage_complete && !self.incomplete_reasons.is_empty() {
            return Err("runtime inventory coverage contradicts its reasons");
        }
        Ok(())
    }
}
