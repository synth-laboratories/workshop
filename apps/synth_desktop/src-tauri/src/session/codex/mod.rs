//! Codex SessionKind transport (app-server).
mod event_pump;
mod home;
mod manager;
mod proto;
mod telemetry;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub use home::{
    apply_brokered_credential, apply_openrouter_provider, apply_synth_cloud_provider, codex_root,
    oauth_auth_path, provider_class, scrub_oauth_auth_files, stage_brokered_credential,
    ProviderClass,
};
pub use manager::CodexManager;
pub use proto::{
    CodexApprovalDecisionRequest, CodexSessionInfo, CodexSessionRecord, CodexSessionRequest,
    CodexSessionStartRequest, CodexSteerRequest, CodexThreadItemsRequest, CodexThreadReadRequest,
    CodexTurnFailure, CodexTurnSendRequest, CodexTurnStartRequest, ProviderTransport,
    CODEX_SESSION_DETACHED, CODEX_TURN_START_FAILED,
};
