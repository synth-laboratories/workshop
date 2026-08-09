use super::{
    LegacyCandidate, MigrationApplyRequest, MigrationPlan, MigrationReceipt, MigrationService,
};
use serde::Serialize;
use std::path::PathBuf;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationCancelResult {
    pub cancelled: bool,
}

#[tauri::command]
pub async fn migration_scan(
    state: State<'_, MigrationService>,
) -> Result<Vec<LegacyCandidate>, String> {
    state
        .scan_default_candidates()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn migration_prepare(
    state: State<'_, MigrationService>,
    source_path: String,
) -> Result<MigrationPlan, String> {
    state
        .prepare(PathBuf::from(source_path))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn migration_apply(
    state: State<'_, MigrationService>,
    request: MigrationApplyRequest,
) -> Result<MigrationReceipt, String> {
    state
        .apply(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn migration_cancel(
    state: State<'_, MigrationService>,
    confirmation_token: String,
) -> MigrationCancelResult {
    MigrationCancelResult {
        cancelled: state.cancel(&confirmation_token),
    }
}
