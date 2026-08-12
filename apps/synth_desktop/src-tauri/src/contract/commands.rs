//! Tauri invoke command names. Keep in sync with
//! `src/renderer/src/bridge/protocolConstants.ts`.
//!
//! Names match `#[tauri::command]` / `generate_handler!` identifiers.

/// Const map of high-traffic / bridge-facing command names.
pub struct Commands;

impl Commands {
    // Core / journal
    pub const CORE_DIAGNOSTICS: &'static str = "core_diagnostics";
    pub const CORE_EVENTS_AFTER: &'static str = "core_events_after";
    pub const CORE_SESSION_EVENTS_AFTER: &'static str = "core_session_events_after";

    // Intern
    pub const INTERN_SESSIONS_LIST: &'static str = "intern_sessions_list";
    pub const INTERN_SESSION_CREATE: &'static str = "intern_session_create";
    pub const INTERN_SESSION_SEND: &'static str = "intern_session_send";
    pub const INTERN_SESSION_CONTROL: &'static str = "intern_session_control";
    pub const INTERN_SESSION_EVENTS_AFTER: &'static str = "intern_session_events_after";

    // Codex
    pub const CODEX_DEFAULT_WORKSPACE: &'static str = "codex_default_workspace";
    pub const CODEX_SESSIONS_LIST: &'static str = "codex_sessions_list";
    pub const CODEX_SESSION_START: &'static str = "codex_session_start";
    pub const CODEX_TURN_START: &'static str = "codex_turn_start";
    pub const CODEX_TURN_SEND: &'static str = "codex_turn_send";
    pub const CODEX_TURN_INTERRUPT: &'static str = "codex_turn_interrupt";
    pub const CODEX_THREAD_COMPACT: &'static str = "codex_thread_compact";
    pub const CODEX_TURN_STEER: &'static str = "codex_turn_steer";
    pub const CODEX_APPROVAL_RESOLVE: &'static str = "codex_approval_resolve";
    pub const CODEX_SESSION_CLOSE: &'static str = "codex_session_close";

    // Account
    pub const ACCOUNT_BEGIN_SIGN_IN: &'static str = "account_begin_sign_in";
    pub const ACCOUNT_POLL_SIGN_IN: &'static str = "account_poll_sign_in";
    pub const ACCOUNT_CANCEL_SIGN_IN: &'static str = "account_cancel_sign_in";
    pub const ACCOUNT_SIGN_OUT: &'static str = "account_sign_out";
    pub const ACCOUNT_GET_SUMMARY: &'static str = "account_get_summary";
    pub const ACCOUNT_REFRESH: &'static str = "account_refresh";
    pub const ACCOUNT_OPEN_BILLING: &'static str = "account_open_billing";

    // Config / workspace
    pub const SYNTH_CONFIG_GET: &'static str = "synth_config_get";
    pub const SYNTH_CONFIG_UPDATE: &'static str = "synth_config_update";
    pub const MODEL_MULTI_AGENT_LIST: &'static str = "model_multi_agent_list";
    pub const MODEL_MULTI_AGENT_UPDATE: &'static str = "model_multi_agent_update";
    pub const WORKSPACE_ACCESS_GET: &'static str = "workspace_access_get";
    pub const WORKSPACE_ACCESS_UPDATE: &'static str = "workspace_access_update";
    pub const WORKSPACE_SCOPE_GET: &'static str = "workspace_scope_get";
    pub const WORKSPACE_SCOPE_CHOOSE_AND_ATTACH: &'static str = "workspace_scope_choose_and_attach";
    pub const WORKSPACE_SCOPE_RECENT_FOLDERS: &'static str = "workspace_scope_recent_folders";
    pub const WORKSPACE_SCOPE_ATTACH_RECENT: &'static str = "workspace_scope_attach_recent";
    pub const WORKSPACE_SCOPE_REMOVE_ATTACHMENT: &'static str = "workspace_scope_remove_attachment";
    pub const WORKSPACE_SCOPE_GRANTS_LIST: &'static str = "workspace_scope_grants_list";
    pub const WORKSPACE_SCOPE_APPROVE_REQUEST: &'static str = "workspace_scope_approve_request";
    pub const WORKSPACE_SCOPE_DENY_REQUEST: &'static str = "workspace_scope_deny_request";

    // Laguna / whisper / terminal
    pub const LAGUNA_GET_STATUS: &'static str = "laguna_get_status";
    pub const LAGUNA_RELOAD: &'static str = "laguna_reload";
    pub const LAGUNA_MODELS_LIST: &'static str = "laguna_models_list";
    pub const LAGUNA_MODEL_DOWNLOAD: &'static str = "laguna_model_download";
    pub const LAGUNA_MODEL_UNLOAD: &'static str = "laguna_model_unload";
    pub const LAGUNA_INFERENCE_STREAM_START: &'static str = "laguna_inference_stream_start";
    pub const LAGUNA_INFERENCE_STREAM_STOP: &'static str = "laguna_inference_stream_stop";
    pub const WHISPER_RUNTIME_STATUS: &'static str = "whisper_runtime_status";
    pub const WHISPER_RUNTIME_WARM: &'static str = "whisper_runtime_warm";
    pub const WHISPER_MODELS_LIST: &'static str = "whisper_models_list";
    pub const WHISPER_MODEL_DOWNLOAD: &'static str = "whisper_model_download";
    pub const WHISPER_MODELS_SET_SELECTED: &'static str = "whisper_models_set_selected";
    pub const WHISPER_MODELS_CLEAR: &'static str = "whisper_models_clear";
    pub const WHISPER_TRANSCRIBE: &'static str = "whisper_transcribe";
    pub const WHISPER_TRANSCRIBE_BASE64: &'static str = "whisper_transcribe_base64";
    pub const TERMINAL_CREATE: &'static str = "terminal_create";
    pub const TERMINAL_LIST: &'static str = "terminal_list";
    pub const TERMINAL_SNAPSHOT: &'static str = "terminal_snapshot";
    pub const TERMINAL_WRITE: &'static str = "terminal_write";
    pub const TERMINAL_RESIZE: &'static str = "terminal_resize";
    pub const TERMINAL_CLOSE: &'static str = "terminal_close";

    // Data / visuals / optimizers (partial — expand with specta)
    pub const DATA_CONTAINERS_LIST: &'static str = "data_containers_list";
    pub const DATA_COUNTS: &'static str = "data_counts";
    pub const USAGE_SUMMARY: &'static str = "usage_summary";
    pub const VISUALS_LIST: &'static str = "visuals_list";
    pub const VISUALS_GET: &'static str = "visuals_get";
    pub const VISUALS_SHOW: &'static str = "visuals_show";
    pub const OPTIMIZERS_LIST: &'static str = "optimizers_list";
    pub const OPTIMIZERS_GET: &'static str = "optimizers_get";

    pub const DESKTOP_IMAGE_PREVIEW: &'static str = "desktop_image_preview";
    pub const DESKTOP_INSTANCE_DIAGNOSTICS: &'static str = "desktop_instance_diagnostics";
    pub const WORKSPACE_CHOOSE_DIRECTORY: &'static str = "workspace_choose_directory";
    pub const LAGUNA_MODELS_SET_DIRECTORY: &'static str = "laguna_models_set_directory";
    pub const LAGUNA_MODELS_CLEAR_DIRECTORY: &'static str = "laguna_models_clear_directory";
    pub const LAGUNA_MODEL_DELETE: &'static str = "laguna_model_delete";
    pub const MIGRATION_CANCEL: &'static str = "migration_cancel";
}

/// Alias matching the TS `COMMANDS` export shape.
pub use Commands as COMMANDS;
