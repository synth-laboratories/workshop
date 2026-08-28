use crate::error::AppError;
use crate::platform::failure::{FailureQuery, FailureQueryResult, FailureView, OperationalFailure};
use crate::platform::logging::{LogQuery, LogQueryResult};
use crate::CoreRuntime;
use std::sync::Arc;
use tauri::State;

#[allow(dead_code)]
pub fn from_failure(failure: &OperationalFailure) -> AppError {
    AppError::from_occurrence(failure)
}

#[allow(dead_code)]
pub fn from_view(view: FailureView) -> AppError {
    AppError::from_view(view)
}

#[tauri::command]
#[specta::specta]
pub fn failures_query(
    state: State<'_, Arc<CoreRuntime>>,
    request: FailureQuery,
) -> Result<FailureQueryResult, AppError> {
    state
        .observability()
        .failures()
        .ok_or_else(|| {
            AppError::coded(
                "sqlite_unavailable_at_bootstrap",
                "failure ledger is in emergency file mode",
            )
        })?
        .query(request)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn failures_get(
    state: State<'_, Arc<CoreRuntime>>,
    failure_id: String,
) -> Result<Option<FailureView>, AppError> {
    state
        .observability()
        .failures()
        .ok_or_else(|| {
            AppError::coded(
                "sqlite_unavailable_at_bootstrap",
                "failure ledger is in emergency file mode",
            )
        })?
        .get(&failure_id)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn failures_timeline(
    state: State<'_, Arc<CoreRuntime>>,
    failure_id: String,
) -> Result<crate::contract::specta::OpaqueJson, AppError> {
    state
        .observability()
        .failures()
        .ok_or_else(|| {
            AppError::coded(
                "sqlite_unavailable_at_bootstrap",
                "failure ledger is in emergency file mode",
            )
        })?
        .timeline(&failure_id)
        .map(|events| crate::contract::specta::OpaqueJson(serde_json::json!(events)))
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn logs_query(
    state: State<'_, Arc<CoreRuntime>>,
    request: LogQuery,
) -> Result<LogQueryResult, AppError> {
    state
        .observability()
        .logs
        .query(request)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn failure_export_bundle(
    state: State<'_, Arc<CoreRuntime>>,
    failure_id: String,
) -> Result<crate::contract::specta::OpaqueJson, AppError> {
    state
        .observability()
        .failures()
        .ok_or_else(|| {
            AppError::coded(
                "sqlite_unavailable_at_bootstrap",
                "failure ledger is in emergency file mode",
            )
        })?
        .export_bundle(&failure_id)
        .map(crate::contract::specta::OpaqueJson)
        .map_err(AppError::from)
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ObservabilityStatus {
    pub mode: String,
    pub emergency: bool,
}

#[tauri::command]
#[specta::specta]
pub fn observability_status(state: State<'_, Arc<CoreRuntime>>) -> ObservabilityStatus {
    let mode = state.observability().mode;
    ObservabilityStatus {
        mode: match mode {
            crate::platform::logging::ObservabilityMode::Durable => "durable".into(),
            crate::platform::logging::ObservabilityMode::EmergencyFile => "emergency_file".into(),
        },
        emergency: matches!(
            mode,
            crate::platform::logging::ObservabilityMode::EmergencyFile
        ),
    }
}
