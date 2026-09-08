use crate::{
    core_runtime::CoreRuntime,
    domains::desktop_state::{self, Entry, Snapshot, Write},
};
use std::sync::Arc;

#[tauri::command]
#[specta::specta]
pub fn desktop_state_get(core: tauri::State<'_, Arc<CoreRuntime>>) -> Result<Snapshot, String> {
    desktop_state::read(&core).map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn desktop_state_commit(
    core: tauri::State<'_, Arc<CoreRuntime>>,
    input: Write,
) -> Result<Entry, String> {
    desktop_state::write(&core, input, false).map_err(|error| error.to_string())
}
