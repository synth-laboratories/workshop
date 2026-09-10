pub mod models;
mod service;
mod validation;

pub use service::HumanAnnotationService;

use crate::{error::AppError, CoreRuntime};
use models::*;
use std::sync::Arc;
use tauri::{Emitter, State};

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_create(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationCreateRequest,
) -> Result<HumanAnnotationTaskRef, AppError> {
    state.create(request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_preview(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationPreviewRequest,
) -> Result<HumanAnnotationTaskPreview, AppError> {
    state.preview(request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_session_open(
    state: State<'_, Arc<HumanAnnotationService>>,
    session_id: String,
) -> Result<HumanAnnotationSessionView, AppError> {
    state.open(session_id).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_show(
    app: tauri::AppHandle,
    state: State<'_, Arc<HumanAnnotationService>>,
    session_id: String,
) -> Result<HumanAnnotationSessionView, AppError> {
    let view = state
        .open(session_id.clone())
        .await
        .map_err(AppError::from)?;
    app.emit(
        "human-annotation:show",
        serde_json::json!({"sessionId":session_id}),
    )
    .map_err(|e| AppError::untyped(e.to_string()))?;
    Ok(view)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_answer_set(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationAnswerRequest,
) -> Result<HumanAnnotationMutationReceipt, AppError> {
    state.answer(request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_answer_clear(
    state: State<'_, Arc<HumanAnnotationService>>,
    session_id: String,
    expected_revision: i32,
    question_id: String,
) -> Result<HumanAnnotationMutationReceipt, AppError> {
    state
        .clear_answer(session_id, expected_revision, question_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_comment_create(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationCommentRequest,
) -> Result<HumanAnnotationMutationReceipt, AppError> {
    state.comment(request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_audio_begin(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationAudioBeginRequest,
) -> Result<HumanAnnotationAudioReceipt, AppError> {
    state.audio_begin(request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_audio_append(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationAudioChunkRequest,
) -> Result<HumanAnnotationAudioReceipt, AppError> {
    state.audio_append(request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_audio_finish(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationAudioFinishRequest,
) -> Result<HumanAnnotationAudioReceipt, AppError> {
    state.audio_finish(request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_audio_read(
    state: State<'_, Arc<HumanAnnotationService>>,
    session_id: String,
    attachment_id: String,
) -> Result<HumanAnnotationAudioData, AppError> {
    use base64::Engine as _;
    let (bytes, media_type) = state
        .audio_source(session_id, attachment_id.clone())
        .await
        .map_err(AppError::from)?;
    Ok(HumanAnnotationAudioData {
        attachment_id,
        media_type,
        base64_data: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_audio_transcribe(
    app: tauri::AppHandle,
    annotations: State<'_, Arc<HumanAnnotationService>>,
    whisper: State<'_, Arc<crate::whisper::WhisperManager>>,
    request: HumanAnnotationAudioTranscribeRequest,
) -> Result<HumanAnnotationMutationReceipt, AppError> {
    let (bytes, media_type) = annotations
        .audio_source(request.session_id.clone(), request.attachment_id.clone())
        .await
        .map_err(AppError::from)?;
    let whisper = Arc::clone(whisper.inner());
    let transcription = tauri::async_runtime::spawn_blocking(move || {
        whisper.transcribe_persisted_bytes(&bytes, &media_type)
    })
    .await
    .map_err(|error| AppError::untyped(error.to_string()))?
    .map_err(AppError::from)?;
    let receipt = annotations
        .save_machine_transcript(
            request,
            transcription.text,
            "local-whisper".into(),
            "en".into(),
        )
        .await
        .map_err(AppError::from)?;
    let _ = app.emit("human-annotation:transcript-ready", &receipt);
    Ok(receipt)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_transcript_correct(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationTranscriptRequest,
) -> Result<HumanAnnotationMutationReceipt, AppError> {
    state
        .correct_transcript(request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_submit(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationSubmitRequest,
) -> Result<HumanAnnotationResultReceipt, AppError> {
    state.submit(request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_list(
    state: State<'_, Arc<HumanAnnotationService>>,
    query: HumanAnnotationListQuery,
) -> Result<Vec<crate::contract::specta::OpaqueJson>, AppError> {
    state
        .list(query)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(crate::contract::specta::OpaqueJson)
                .collect()
        })
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_status(
    state: State<'_, Arc<HumanAnnotationService>>,
    id: String,
) -> Result<HumanAnnotationStatus, AppError> {
    state.status(id).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_cancel(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationCancelRequest,
) -> Result<HumanAnnotationStatus, AppError> {
    state.cancel(request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_export(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationExportRequest,
) -> Result<HumanAnnotationExportReceipt, AppError> {
    state.export(request).await.map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_campaign_create(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationCampaignCreateRequest,
) -> Result<crate::contract::specta::OpaqueJson, AppError> {
    state
        .campaign_create(request)
        .await
        .map(crate::contract::specta::OpaqueJson)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_campaign_status(
    state: State<'_, Arc<HumanAnnotationService>>,
    campaign_id: String,
) -> Result<crate::contract::specta::OpaqueJson, AppError> {
    state
        .campaign_status(campaign_id)
        .await
        .map(crate::contract::specta::OpaqueJson)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_campaign_close(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationCampaignActionRequest,
) -> Result<crate::contract::specta::OpaqueJson, AppError> {
    state
        .campaign_close(request)
        .await
        .map(crate::contract::specta::OpaqueJson)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_campaign_adjudicate(
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationCampaignAdjudicateRequest,
) -> Result<crate::contract::specta::OpaqueJson, AppError> {
    state
        .campaign_adjudicate(request)
        .await
        .map(crate::contract::specta::OpaqueJson)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn human_annotation_supersede(
    app: tauri::AppHandle,
    state: State<'_, Arc<HumanAnnotationService>>,
    request: HumanAnnotationSupersedeRequest,
) -> Result<HumanAnnotationTaskRef, AppError> {
    let task = state.supersede(request).await.map_err(AppError::from)?;
    app.emit(
        "human-annotation:show",
        serde_json::json!({"sessionId":task.session_id}),
    )
    .map_err(|error| AppError::untyped(error.to_string()))?;
    Ok(task)
}

pub fn from_core(core: &CoreRuntime) -> Arc<HumanAnnotationService> {
    Arc::new(HumanAnnotationService::new(
        core.storage().database().clone(),
        core.storage().content_root().to_path_buf(),
    ))
}
