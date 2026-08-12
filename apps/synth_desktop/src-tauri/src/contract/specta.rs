//! tauri-specta boundary scaffold (Wave 2 stretch).
//!
//! # Dual-path registration (do not break invoke)
//!
//! [`crate::run`] registers this complete command collection through
//! [`Builder::invoke_handler`].
//!
//! This module owns a growing `tauri_specta::Builder` used **only** for:
//! 1. TypeScript binding export → `src/renderer/src/generated/protocol.ts`
//! 2. Documenting the eventual cutover sketch (below)
//!
//! The seed command lives here (not in `lib.rs`) so `#[tauri::command]` +
//! `pub` do not collide with macro reimports at the crate root.
//!
//! # Cutover sketch (when every command is collected)
//!
//! ```ignore
//! let specta = contract::specta::builder();
//! // #[cfg(debug_assertions)] { specta.export(...) }
//! tauri::Builder::default()
//!     .invoke_handler(specta.invoke_handler())  // replaces generate_handler!
//!     .setup(move |app| { specta.mount_events(app); Ok(()) })
//! ```
//!
//! Until then: never call `builder().invoke_handler()` from `run()` — it would
//! drop every command not listed in `collect_commands!`.
//!
//! # Migrating the next command
//!
//! 1. `#[derive(specta::Type)]` on arg/result DTOs (and `AppError` when ready).
//! 2. Add `#[specta::specta]` next to `#[tauri::command]` (keep commands in a
//!    submodule — not `lib.rs` root if `pub`).
//! 3. Append the fn to `collect_commands!` in [`builder`].
//! 4. Re-run `cargo test -p synth-desktop export_specta_protocol_bindings -- --nocapture`
//!    (or debug-assert export in [`export_typescript_bindings`]).
//! 5. Prefer generated types from `generated/protocol.ts` at the bridge edge.

use crate::instance::{self, InstanceDiagnostics};
use serde::{Deserialize, Serialize};
use specta_typescript::Typescript;
use tauri_specta::{collect_commands, Builder};

/// Opaque JSON avoids recursively expanding serde_json::Value's i64/u64 number variants.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OpaqueJson(pub serde_json::Value);

impl specta::Type for OpaqueJson {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        <specta_typescript::Unknown as specta::Type>::definition(types)
    }
}

/// Existing Tauri integers arrive as JSON numbers; do not claim in generated
/// TypeScript that every Rust i64/u64 is losslessly representable as `number`.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(transparent)]
pub struct OpaqueInteger<T>(pub T);

impl<T> specta::Type for OpaqueInteger<T> {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        <specta_typescript::Unknown as specta::Type>::definition(types)
    }
}

/// Absolute-ish path from `src-tauri/` to the committed renderer binding file.
pub const PROTOCOL_TS_RELATIVE: &str = "../src/renderer/src/generated/protocol.ts";

/// Instance diagnostics command included in the generated desktop boundary.
#[tauri::command]
#[specta::specta]
pub fn desktop_instance_diagnostics() -> InstanceDiagnostics {
    instance::diagnostics()
}

/// Specta builder for the complete desktop command boundary.
pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        desktop_instance_diagnostics,
        crate::desktop_image_preview,
        crate::core_diagnostics,
        crate::core_events_after,
        crate::core_session_events_after,
        crate::intern_sessions_list,
        crate::intern_session_create,
        crate::intern_session_send,
        crate::intern_session_control,
        crate::intern_session_events_after,
        crate::data_containers_list,
        crate::data_containers_get,
        crate::data_containers_register,
        crate::data_containers_probe,
        crate::data_traces_list,
        crate::data_traces_get,
        crate::data_traces_ingest,
        crate::data_trace_projection_resolve,
        crate::data_usage_list,
        crate::model_performance_summary,
        crate::usage_summary,
        crate::tariff_catalog,
        crate::update_status,
        crate::update_open_download,
        crate::data_counts,
        crate::optimizers_algorithms_list,
        crate::optimizers_recipes_list,
        crate::optimizers_recipe_start,
        crate::optimizers_list,
        crate::optimizers_get,
        crate::optimizers_create,
        crate::optimizers_refresh,
        crate::optimizers_events_after,
        crate::optimizers_get_state,
        crate::optimizers_get_state_batch,
        crate::optimizers_relationships,
        crate::optimizers_cancel,
        crate::optimizers_pause,
        crate::optimizers_resume,
        crate::optimizers_open_visual,
        crate::optimizers_import_local,
        crate::optimizers_reconcile_cloud,
        crate::optimizers_list_cloud,
        crate::visuals_templates_list,
        crate::visuals_templates_get,
        crate::visuals_list,
        crate::visuals_get,
        crate::visuals_revisions,
        crate::visuals_create,
        crate::visuals_update,
        crate::visuals_save,
        crate::visuals_fork,
        crate::visuals_archive,
        crate::visuals_show,
        crate::synth_config_get,
        crate::synth_config_update,
        crate::model_performance_get,
        crate::account_begin_sign_in,
        crate::account_get_summary,
        crate::account_refresh,
        crate::account_open_billing,
        crate::account_poll_sign_in,
        crate::account_cancel_sign_in,
        crate::account_sign_out,
        crate::model_multi_agent_list,
        crate::model_multi_agent_update,
        crate::workspace_access_get,
        crate::workspace_access_update,
        crate::workspace_scope_get,
        crate::workspace_scope_choose_and_attach,
        crate::workspace_scope_recent_folders,
        crate::workspace_scope_attach_recent,
        crate::workspace_scope_remove_attachment,
        crate::workspace_scope_request_agent_grant,
        crate::workspace_scope_grants_list,
        crate::workspace_scope_approve_request,
        crate::workspace_scope_deny_request,
        crate::storage::legacy_migration::commands::migration_scan,
        crate::storage::legacy_migration::commands::migration_prepare,
        crate::storage::legacy_migration::commands::migration_apply,
        crate::storage::legacy_migration::commands::migration_cancel,
        crate::laguna_get_status,
        crate::laguna_reload,
        crate::laguna_models_list,
        crate::laguna_models_set_directory,
        crate::laguna_models_clear_directory,
        crate::laguna::laguna_inference_snapshot,
        crate::laguna::laguna_inference_stream_start,
        crate::laguna::laguna_inference_stream_stop,
        crate::laguna::laguna_model_unload,
        crate::laguna::laguna_model_download,
        crate::laguna::laguna_model_delete,
        crate::laguna::laguna_settings_snapshot,
        crate::laguna::laguna_settings_update,
        crate::whisper::whisper_models_list,
        crate::whisper::whisper_model_download,
        crate::whisper::whisper_models_set_selected,
        crate::whisper::whisper_models_clear,
        crate::whisper::whisper_runtime_status,
        crate::whisper::whisper_runtime_warm,
        crate::whisper::whisper_transcribe,
        crate::whisper::whisper_transcribe_base64,
        crate::skills::skills_list,
        crate::workspace_choose_directory,
        crate::codex_session_start,
        crate::codex_turn_start,
        crate::codex_turn_send,
        crate::codex_turn_interrupt,
        crate::codex_thread_compact,
        crate::codex_turn_steer,
        crate::codex_approval_resolve,
        crate::codex_session_close,
        crate::codex_sessions_list,
        crate::codex_default_workspace,
        crate::terminal_create,
        crate::terminal_list,
        crate::terminal_snapshot,
        crate::terminal_write,
        crate::terminal_resize,
        crate::terminal_close,
    ])
}

/// Write TypeScript bindings for the complete command boundary.
///
/// Safe to call from tests and from `run()` under `debug_assertions`. Does not
/// touch invoke registration.
pub fn export_typescript_bindings() -> Result<(), String> {
    // CARGO_MANIFEST_DIR is src-tauri/; PROTOCOL_TS_RELATIVE is relative to that.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(PROTOCOL_TS_RELATIVE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    builder()
        .export(
            Typescript::default().header("// @generated by tauri-specta — do not edit by hand.\n"),
            &path,
        )
        .map_err(|error| error.to_string())?;
    // specta-typescript currently leaves spaces at a few generated line ends
    // (notably enum unions/JSDoc). Normalize deterministically for repository QA.
    let body = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let normalized = body
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, normalized).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_specta_protocol_bindings() {
        export_typescript_bindings().expect("export specta TypeScript bindings");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(PROTOCOL_TS_RELATIVE);
        let body = std::fs::read_to_string(&path).expect("read generated protocol.ts");
        assert!(
            body.contains("desktop_instance_diagnostics")
                || body.contains("desktopInstanceDiagnostics"),
            "generated bindings should mention the seed command; got {} bytes",
            body.len()
        );
        assert!(
            body.contains("InstanceDiagnostics"),
            "generated bindings should include InstanceDiagnostics"
        );
        let exported =
            body.matches("__TAURI_INVOKE(").count() + body.matches("__TAURI_INVOKE<").count();
        assert_eq!(
            exported, 120,
            "generated bindings must contain the complete desktop command set"
        );
    }
}
