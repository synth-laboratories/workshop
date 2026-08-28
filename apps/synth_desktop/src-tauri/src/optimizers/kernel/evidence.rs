//! Evidence completeness and late amendments.
//!
//! Missing evidence is a typed state, never a successful no-op. Late evidence
//! cannot rewrite a sealed terminal manifest; it appends an amendment.

use serde::{Deserialize, Serialize};

use super::error::{KernelError, KernelErrorCode, KernelResult};
use super::types::EvidenceCompleteness;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceState {
    pub completeness: EvidenceCompleteness,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub refs: Vec<EvidenceRef>,
}

impl EvidenceState {
    pub fn absent() -> Self {
        Self {
            completeness: EvidenceCompleteness::Absent,
            reason: Some("no evidence has been recorded".into()),
            refs: Vec::new(),
        }
    }

    pub fn require_present(&self) -> KernelResult<()> {
        match self.completeness {
            EvidenceCompleteness::Absent => Err(KernelError::new(
                KernelErrorCode::EvidenceMissing,
                self.reason
                    .clone()
                    .unwrap_or_else(|| "evidence is absent".into()),
            )),
            EvidenceCompleteness::Unusable => Err(KernelError::new(
                KernelErrorCode::EvidenceMissing,
                self.reason
                    .clone()
                    .unwrap_or_else(|| "evidence is unusable".into()),
            )),
            EvidenceCompleteness::Partial | EvidenceCompleteness::Complete => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRef {
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub digest: Option<String>,
}

/// Append-only correction linked to the original terminal sequence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceAmendment {
    pub amendment_id: String,
    pub optimizer_run_id: String,
    #[specta(type = specta_typescript::Number)]
    pub terminal_sequence: u64,
    pub recorded_at: String,
    pub refs: Vec<EvidenceRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SealedTerminal {
    pub kind: super::types::TerminalKind,
    #[serde(default)]
    pub reason: Option<super::types::TerminalReason>,
    #[specta(type = specta_typescript::Number)]
    pub final_sequence: u64,
    pub evidence: EvidenceState,
    #[serde(default)]
    pub failure_ref: Option<String>,
    pub sealed_at: String,
}

impl SealedTerminal {
    pub fn amend(&self, amendment: EvidenceAmendment) -> KernelResult<EvidenceAmendment> {
        if amendment.terminal_sequence != self.final_sequence {
            return Err(KernelError::new(
                KernelErrorCode::TerminalAlreadySealed,
                format!(
                    "amendment {} must link to sealed sequence {}, not {}",
                    amendment.amendment_id, self.final_sequence, amendment.terminal_sequence
                ),
            ));
        }
        Ok(amendment)
    }
}

/// Usage that was never reported stays unavailable. Zero is a measured zero.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UsageCompleteness {
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub calls: Option<u64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub prompt_tokens: Option<u64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub completion_tokens: Option<u64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub steps: Option<u64>,
}

impl UsageCompleteness {
    pub fn add_reported(
        &mut self,
        cost_usd: Option<f64>,
        prompt: Option<u64>,
        completion: Option<u64>,
    ) {
        if let Some(cost) = cost_usd {
            self.cost_usd = Some(self.cost_usd.unwrap_or(0.0) + cost);
        }
        if let Some(tokens) = prompt {
            self.prompt_tokens = Some(self.prompt_tokens.unwrap_or(0) + tokens);
        }
        if let Some(tokens) = completion {
            self.completion_tokens = Some(self.completion_tokens.unwrap_or(0) + tokens);
        }
    }
}

