use serde::{Deserialize, Serialize};

pub const TASK_SCHEMA: &str = "synth.human-annotation-task.v1";
pub const SESSION_SCHEMA: &str = "synth.human-annotation-session.v1";
pub const RESULT_SCHEMA: &str = "synth.human-annotation-result.v1";
pub const SEAL_SCHEMA: &str = "synth.human-annotation-result-seal.v1";
pub const PREVIEW_SCHEMA: &str = "synth.human-annotation-task-preview.v1";
pub const CAMPAIGN_SCHEMA: &str = "synth.human-annotation-campaign.v1";

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationCreateRequest {
    pub task: crate::contract::specta::OpaqueJson,
    /// Evaluator-owned answer keys. Persisted separately and never returned in
    /// the reviewer task or agent status projection.
    pub sealed_answer_keys: Option<crate::contract::specta::OpaqueJson>,
    pub idempotency_key: String,
    pub reviewer_id: Option<String>,
    pub created_by: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationPreviewRequest {
    pub task: crate::contract::specta::OpaqueJson,
    pub sealed_answer_keys: Option<crate::contract::specta::OpaqueJson>,
    pub reviewer_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationTaskPreview {
    pub schema_version: String,
    pub task_digest: String,
    pub title: String,
    pub question_count: i32,
    pub required_question_count: i32,
    pub evidence_count: i32,
    pub evidence: crate::contract::specta::OpaqueJson,
    pub presentation: crate::contract::specta::OpaqueJson,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationTaskRef {
    pub task_id: String,
    pub session_id: String,
    pub task_digest: String,
    pub state: String,
    pub created: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationSessionView {
    pub schema_version: String,
    pub task_id: String,
    pub session_id: String,
    pub task_digest: String,
    pub task: crate::contract::specta::OpaqueJson,
    pub state: String,
    pub draft_revision: i32,
    pub answers: crate::contract::specta::OpaqueJson,
    pub presentation: crate::contract::specta::OpaqueJson,
    pub comments: Vec<crate::contract::specta::OpaqueJson>,
    pub attachments: Vec<crate::contract::specta::OpaqueJson>,
    pub result: Option<crate::contract::specta::OpaqueJson>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationStatus {
    pub task_id: String,
    pub session_id: String,
    pub task_digest: String,
    pub state: String,
    pub revision: i32,
    pub result_id: Option<String>,
    pub result_digest: Option<String>,
    pub seal_digest: Option<String>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationAnswerRequest {
    pub session_id: String,
    pub expected_revision: i32,
    pub question_id: String,
    pub answer: crate::contract::specta::OpaqueJson,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationCommentRequest {
    pub session_id: String,
    pub expected_revision: i32,
    pub evidence_digest: String,
    pub selector: crate::contract::specta::OpaqueJson,
    pub body_text: Option<String>,
    pub audio_attachment_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationAudioBeginRequest {
    pub session_id: String,
    pub media_type: String,
    pub metadata: Option<crate::contract::specta::OpaqueJson>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationAudioChunkRequest {
    pub attachment_id: String,
    pub chunk_index: i32,
    pub base64_data: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationAudioFinishRequest {
    pub attachment_id: String,
    pub duration_ms: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationAudioTranscribeRequest {
    pub session_id: String,
    pub expected_revision: i32,
    pub attachment_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationTranscriptRequest {
    pub session_id: String,
    pub expected_revision: i32,
    pub attachment_id: String,
    pub corrected_text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationSubmitRequest {
    pub session_id: String,
    pub expected_revision: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationMutationReceipt {
    pub session_id: String,
    pub draft_revision: i32,
    pub state: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationAudioReceipt {
    pub attachment_id: String,
    pub state: String,
    pub cas_digest: Option<String>,
    pub byte_size: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationAudioData {
    pub attachment_id: String,
    pub media_type: String,
    pub base64_data: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationResultReceipt {
    pub task_id: String,
    pub session_id: String,
    pub result_id: String,
    pub result_digest: String,
    pub seal_digest: String,
    pub state: String,
    pub submitted_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationCancelRequest {
    pub task_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationExportRequest {
    pub result_id: String,
    pub format: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationExportReceipt {
    pub result_id: String,
    pub format: String,
    pub export_digest: String,
    pub byte_size: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationListQuery {
    pub campaign_id: Option<String>,
    pub state: Option<String>,
    pub limit: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationCampaignCreateRequest {
    pub campaign: crate::contract::specta::OpaqueJson,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationCampaignActionRequest {
    pub campaign_id: String,
    pub rationale: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationCampaignAdjudicateRequest {
    pub campaign_id: String,
    pub result_ids: Vec<String>,
    pub decision: crate::contract::specta::OpaqueJson,
    pub rationale: String,
    pub adjudicator_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanAnnotationSupersedeRequest {
    pub result_id: String,
    pub reason: String,
    pub reviewer_id: Option<String>,
}
