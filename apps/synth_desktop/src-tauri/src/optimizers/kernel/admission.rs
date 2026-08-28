//! Pre-run admission. No optimizer run exists until approval consumes a draft.

use serde::{Deserialize, Serialize};

use super::error::{KernelError, KernelErrorCode, KernelResult};
use super::types::{AdmissionState, AlgorithmKind, ExecutionPlacement};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunDraft {
    pub draft_id: String,
    pub algorithm: AlgorithmKind,
    pub spec_digest: String,
    pub spec_json: String,
    pub admission: AdmissionState,
    #[serde(default)]
    pub authorization_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl RunDraft {
    pub fn new(
        draft_id: impl Into<String>,
        algorithm: AlgorithmKind,
        spec_digest: impl Into<String>,
        spec_json: impl Into<String>,
        created_at: impl Into<String>,
    ) -> Self {
        let created_at = created_at.into();
        Self {
            draft_id: draft_id.into(),
            algorithm,
            spec_digest: spec_digest.into(),
            spec_json: spec_json.into(),
            admission: AdmissionState::Draft,
            authorization_ref: None,
            created_at: created_at.clone(),
            updated_at: created_at,
        }
    }

    pub fn transition(&mut self, next: AdmissionState, at: impl Into<String>) -> KernelResult<()> {
        self.admission = self.admission.transition_to(next)?;
        self.updated_at = at.into();
        Ok(())
    }
}

/// The one transaction that creates an optimizer run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmissionCommit {
    pub draft_id: String,
    pub run_id: String,
    pub algorithm: AlgorithmKind,
    pub placement: ExecutionPlacement,
    pub spec_digest: String,
    pub spec_json: String,
    pub authorization_ref: Option<String>,
    pub admitted_at: String,
}

impl AdmissionCommit {
    pub fn from_approved_draft(
        draft: &RunDraft,
        run_id: impl Into<String>,
        placement: ExecutionPlacement,
        admitted_at: impl Into<String>,
    ) -> KernelResult<Self> {
        if !matches!(
            draft.admission,
            AdmissionState::Approved | AdmissionState::NotRequired
        ) {
            return Err(KernelError::new(
                KernelErrorCode::DraftNotApproved,
                format!(
                    "draft {} is {}, neither approved nor marked admission-not-required",
                    draft.draft_id,
                    draft.admission.as_str()
                ),
            ));
        }
        if draft.spec_digest.trim().is_empty() {
            return Err(KernelError::new(
                KernelErrorCode::EventSchemaMismatch,
                "approved draft is missing a spec digest",
            ));
        }
        Ok(Self {
            draft_id: draft.draft_id.clone(),
            run_id: run_id.into(),
            algorithm: draft.algorithm,
            placement,
            spec_digest: draft.spec_digest.clone(),
            spec_json: draft.spec_json.clone(),
            authorization_ref: draft.authorization_ref.clone(),
            admitted_at: admitted_at.into(),
        })
    }
}

