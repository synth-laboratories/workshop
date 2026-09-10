//! Reviewed exclusions from the generated desktop projection. An implementation
//! callback is not a public product action, and an agent cannot decide its own
//! permissions or manufacture human/renderer evidence.

pub fn internal(name: &str) -> bool {
    matches!(
        name,
        "visual_subscription_ready"
            | "visual_stream_poll"
            | "visuals_observation_report"
            | "terminal_ghostty_mount"
            | "terminal_ghostty_set_frame"
            | "terminal_ghostty_set_visible"
            | "terminal_ghostty_focus"
            | "terminal_ghostty_unmount"
    )
}

pub fn human_surface(name: &str) -> Option<&'static str> {
    match name {
        "context_mcp_group_update"
        | "desktop_state_commit"
        | "codex_approval_resolve"
        | "approvals_approve_digest"
        | "project_source_add"
        | "project_source_remove"
        | "project_source_deny"
        | "project_source_approve"
        | "workspace_scope_approve_request"
        | "workspace_scope_deny_request"
        | "desktop_permissions_update"
        | "workspace_access_update"
        | "browser_policy_allow_origin"
        | "browser_policy_revoke_origin" => Some("Workshop access and approval controls"),
        "secrets_create"
        | "secrets_replace"
        | "secrets_delete"
        | "secrets_test"
        | "secrets_grant_use"
        | "secrets_deny_use"
        | "secrets_commit_env_import"
        | "secrets_locator_register"
        | "secrets_locator_remember_external"
        | "secrets_locator_forget"
        | "secrets_revoke_capability"
        | "secrets_deny_env_import" => Some("Workshop credential consent controls"),
        "codex_oauth_complete_manual" | "synth_config_update" => {
            Some("Workshop account and configuration controls")
        }
        "human_annotation_answer_set"
        | "human_annotation_answer_clear"
        | "human_annotation_comment_create"
        | "human_annotation_audio_begin"
        | "human_annotation_audio_append"
        | "human_annotation_audio_finish"
        | "human_annotation_transcript_correct"
        | "human_annotation_submit"
        | "human_annotation_campaign_adjudicate"
        | "human_annotation_supersede" => Some("Workshop human annotation task"),
        _ => None,
    }
}

