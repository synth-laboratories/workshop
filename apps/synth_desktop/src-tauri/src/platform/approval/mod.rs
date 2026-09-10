//! Typed recovery approval requirement. Clickable UI is generated from this,
//! never inferred from chat text.
//!
//! See `notes/specifications/workshop/failure_runtime.md`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRequirement {
    None,
    OperatorClick { kind: String },
    CredentialCapability { provider: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalContext {
    pub requirement: ApprovalRequirement,
    pub request_id: Option<String>,
}
