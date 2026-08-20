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
        crate::runtime_contracts,
        crate::core_events_after,
        crate::core_session_events_after,
        crate::core_session_events_tail,
        crate::core_session_events_before,
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
        crate::model_performance_turn_samples,
        crate::usage_summary,
        crate::tariff_catalog,
        crate::update_status,
        crate::update_open_download,
        crate::data_counts,
        crate::optimizers_algorithms_list,
        crate::optimizers_recipes_list,
        crate::optimizers_recipe_start,
        crate::optimizers_stage_eval_candidates,
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
        crate::optimizers_saved_loras_search,
        crate::optimizers_run_checkpoints_list,
        crate::optimizers_run_outputs,
        crate::optimizers_training_models,
        crate::optimizers_saved_lora_archive,
        crate::optimizers_saved_lora_download,
        crate::optimizers_training_reconcile,
        crate::plugins_status,
        crate::plugins_list,
        crate::plugins_manage,
        crate::plugins_set_release_channel,
        crate::computer_use_status,
        crate::computer_use_install,
        crate::computer_use_remove,
        crate::computer_use_revoke_app,
        crate::computer_use_open_settings,
        crate::browser_runtime_status,
        crate::browser_policy_allow_origin,
        crate::browser_policy_revoke_origin,
        crate::visual_subscription_ready,
        crate::visual_stream_poll,
        crate::diagnostics_report,
        crate::diagnostics_status,
        crate::diagnostics_query,
        crate::diagnostics_explain,
        crate::diagnostics_bundle,
        crate::diagnostics_clear_index,
        crate::optimizers::manager::optimizer_sidecar_status,
        crate::optimizers::manager::optimizer_sidecar_install,
        crate::optimizers::manager::optimizer_sidecar_start,
        crate::optimizers::manager::optimizer_sidecar_stop,
        crate::optimizers::manager::optimizer_sidecar_version,
        crate::optimizers::manager::optimizer_sidecar_uninstall,
        crate::visuals_templates_list,
        crate::visuals_templates_get,
        crate::visuals_list,
        crate::visuals_get,
        crate::visuals_observation_report,
        crate::visuals_revisions,
        crate::visuals_annotations_list,
        crate::visuals_annotation_create,
        crate::visuals_seals_list,
        crate::visuals_seal,
        crate::visuals_seal_get,
        crate::visuals_upload_status,
        crate::visuals_share_seal,
        crate::visuals_open_shared,
        crate::visuals_create,
        crate::visuals_update,
        crate::visuals_save,
        crate::visuals_fork,
        crate::visuals_archive,
        crate::visuals_show,
        crate::visuals_content,
        crate::visuals_renditions,
        crate::visuals_rendition,
        crate::visuals_render,
        crate::reports_list,
        crate::reports_get,
        crate::reports_revision_get,
        crate::reports_validate,
        crate::reports_pin_all,
        crate::reports_create,
        crate::reports_update,
        crate::reports_archive,
        crate::reports_restore,
        crate::reports_visibility_requests,
        crate::reports_visibility_request,
        crate::reports_visibility_decide,
        crate::reports_seal,
        crate::reports_seals_list,
        crate::reports_seal_get,
        crate::reports_seals_compare,
        crate::reports_experiments_list,
        crate::reports_experiment_upsert,
        crate::reports_log_list,
        crate::reports_log_append,
        crate::reports_upload_status,
        crate::reports_share,
        crate::reports_audience_set,
        crate::reports_audience_revoke,
        crate::reports_promote,
        crate::reports_open_shared,
        crate::reports_comments_list,
        crate::reports_comment_create,
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
        crate::codex_oauth_begin,
        crate::codex_oauth_complete_manual,
        crate::codex_oauth_ensure_ready,
        crate::codex_oauth_status,
        crate::codex_oauth_disconnect,
        crate::codex_oauth_cancel,
        crate::model_multi_agent_list,
        crate::model_multi_agent_update,
        crate::workspace_access_get,
        crate::workspace_access_update,
        crate::desktop_permissions_get,
        crate::desktop_permissions_update,
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
        crate::training_models::training_models_list,
        crate::training_models::training_models_download,
        crate::training_models::training_models_delete,
        crate::training_artifacts::training_artifacts_list,
        crate::training_artifacts::training_artifacts_get,
        crate::training_artifacts_launch_inference,
        crate::whisper::whisper_models_list,
        crate::whisper::whisper_model_download,
        crate::whisper::whisper_models_set_selected,
        crate::whisper::whisper_models_clear,
        crate::whisper::whisper_runtime_status,
        crate::whisper::whisper_runtime_warm,
        crate::whisper::whisper_transcribe,
        crate::whisper::whisper_transcribe_base64,
        crate::skills::skills_list,
        crate::context::context_snapshot,
        crate::context::context_workspace_agents_update,
        crate::context::context_skill_update,
        crate::context::context_mcp_group_update,
        crate::context::context_cookbooks_install,
        crate::context::context_cookbooks_cancel,
        crate::context::context_cookbooks_set_enabled,
        crate::context::context_cookbooks_uninstall,
        crate::workspace_choose_directory,
        crate::codex_session_start,
        crate::codex_turn_start,
        crate::codex_turn_send,
        crate::codex_turn_interrupt,
        crate::codex_thread_compact,
        crate::codex_thread_read,
        crate::codex_thread_items_list,
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
        crate::secrets::secrets_list,
        crate::secrets::secrets_create,
        crate::secrets::secrets_replace,
        crate::secrets::secrets_delete,
        crate::secrets::secrets_test,
        crate::secrets::secrets_request_use,
        crate::secrets::secrets_grant_use,
        crate::secrets::secrets_deny_use,
        crate::secrets::secrets_capabilities_list,
        crate::secrets::secrets_revoke_capability,
        crate::secrets::secrets_request_env_import,
        crate::secrets::secrets_commit_env_import,
        crate::secrets::secrets_audit_list,
        crate::secrets::secrets_proxy_status,
        crate::secrets::secrets_pending,
        crate::secrets::secrets_deny_env_import,
        crate::telemetry::product_telemetry_get_policy,
        crate::telemetry::product_telemetry_set_opt_out,
    ])
}

/// Write TypeScript bindings for the complete command boundary.
///
/// Safe to call from tests and from `run()` under `debug_assertions`. Does not
/// touch invoke registration.
pub fn export_typescript_bindings() -> Result<(), String> {
    // CARGO_MANIFEST_DIR is src-tauri/; PROTOCOL_TS_RELATIVE is relative to that.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(PROTOCOL_TS_RELATIVE);
    export_typescript_bindings_to(&path)
}

fn export_typescript_bindings_to(path: &std::path::Path) -> Result<(), String> {
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
        .trim_end_matches('\n')
        .to_owned()
        + "\n";
    std::fs::write(&path, normalized).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// specta-typescript walks the command graph recursively; the default
    /// cargo-test stack overflows on this crate.
    fn with_export_stack<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::Builder::new()
            .name("specta-export".into())
            .stack_size(64 * 1024 * 1024)
            .spawn(work)
            .expect("start specta export thread")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    }

    #[test]
    fn export_specta_protocol_bindings() {
        let committed_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(PROTOCOL_TS_RELATIVE);
        // The thread name under `cargo test` is the full test path, which
        // contains `::` — an illegal filename character on Windows. Keep only
        // characters that are portable across the platforms we build on.
        let thread = std::thread::current().name().unwrap_or("test").replace(
            |c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_',
            "_",
        );
        let path = std::env::temp_dir().join(format!(
            "synth-desktop-protocol-{}-{thread}.ts",
            std::process::id()
        ));
        let export_path = path.clone();
        with_export_stack(move || {
            export_typescript_bindings_to(&export_path).expect("render specta TypeScript bindings")
        });
        let body = std::fs::read_to_string(&path).expect("read generated protocol.ts");
        let committed =
            std::fs::read_to_string(&committed_path).expect("read committed protocol.ts");
        let _ = std::fs::remove_file(&path);
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
        // Hand-maintained on purpose: the exporter dropping commands must fail
        // here rather than pass quietly. Bump it only alongside a reviewed
        // `collect_commands!` change.
        // 190 → 197: `runtime_contracts` backs the Settings → About runtime
        // version rows, and the six diagnostics commands expose the local
        // diagnostics boundary.
        // 198 → 203: the five Computer Use commands. All five are human-only —
        // status, install, remove, revoke an app, open the System Settings
        // pane — and none is reachable from the agent's MCP surface.
        // 203 → 206: managed browser status plus human-only origin allow/revoke.
        // 206 → 220: local secrets vault + provider proxy (list/create/replace/
        // delete/test, request/grant/deny use, capabilities, env import, audit,
        // proxy status). No get/reveal/export/readValue command is included.
        // 220 → 222: pending agent import/use inbox and deny-import (human-only).
        // 228 → 230: privacy-safe product telemetry policy and opt-out. Event
        // construction remains host-owned; renderer feature code has no
        // arbitrary event-name/property IPC.
        // 230 → 235: hosted training model and saved-LoRA checkpoint search,
        // run-output, archive, and download commands retained from main.
        assert_eq!(
            exported, 235,
            "generated bindings must contain the complete desktop command set"
        );
        assert_eq!(
            body, committed,
            "committed protocol.ts is stale; regenerate it with\n  \
             cargo test -p synth-desktop --lib regenerate_protocol_bindings -- --ignored\n  \
             then review the diff before committing"
        );
    }

    /// The canonical regeneration flow, and the only caller of
    /// [`export_typescript_bindings`] outside the app.
    ///
    /// Ignored by default because it writes into the repository: a plain
    /// `cargo test` must report drift, never silently repair it. Debug startup
    /// used to regenerate this file as a side effect, which is how a stale
    /// build could quietly delete types from the committed contract; making
    /// regeneration an explicit, named command is what keeps that from
    /// happening again.
    #[test]
    #[ignore = "writes generated/protocol.ts; run explicitly to regenerate"]
    fn regenerate_protocol_bindings() {
        with_export_stack(|| {
            export_typescript_bindings().expect("regenerate committed protocol.ts")
        });
    }
}
