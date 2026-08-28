//! Typed causality between failures. Diagnostic rank is not a substitute.

use serde::{Deserialize, Serialize};

use super::definition::FailureId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureRelationshipKind {
    CausedBy,
    ConsequenceOf,
    Supersedes,
    RepairOf,
    RetryOf,
}

impl FailureRelationshipKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CausedBy => "caused_by",
            Self::ConsequenceOf => "consequence_of",
            Self::Supersedes => "supersedes",
            Self::RepairOf => "repair_of",
            Self::RetryOf => "retry_of",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureRelationship {
    pub from: FailureId,
    pub to: FailureId,
    pub kind: FailureRelationshipKind,
}
