//! Stable diagnostic codes, their causal rank, and their remediation.
//!
//! A code is the join key between a failure and what to do about it. Emitters
//! use the constants; `diagnostics_explain` uses the rank to tell an upstream
//! cause from a downstream symptom, and the remediation table to answer
//! "what now" without a model call.
//!
//! Rank is a small integer: **lower is more upstream**. It encodes the one
//! thing timestamps cannot — that a container capability rejection explains a
//! blank visual, and never the reverse, even when the renderer noticed first.

/// Sidecars, capabilities, and preflight — nothing downstream can succeed.
pub const RANK_INFRASTRUCTURE: u8 = 10;
/// Transport and lifecycle: streams, connections, adapters.
pub const RANK_TRANSPORT: u8 = 30;
/// Contract mismatches surfaced where the data is consumed.
pub const RANK_CONTRACT: u8 = 50;
/// Presentation-level symptoms. Almost always caused by something above.
pub const RANK_SYMPTOM: u8 = 70;

pub const DEFAULT_RANK: u8 = RANK_CONTRACT;

// Visuals.
pub const UNSUPPORTED_TRACE_PROJECTION_SCHEMA: &str = "unsupported_trace_projection_schema";
pub const VISUAL_BINDING_UNRESOLVED: &str = "visual_binding_unresolved";
pub const VISUAL_TEMPLATE_UNAVAILABLE: &str = "visual_template_unavailable";
pub const VISUAL_SHELL_LOAD_FAILED: &str = "visual_shell_load_failed";
pub const VISUAL_RENDER_FAILED: &str = "visual_render_failed";

// Live streams.
pub const STREAM_INTERRUPTED: &str = "stream_interrupted";
pub const STREAM_SUBSCRIBE_TIMEOUT: &str = "stream_subscribe_timeout";
pub const STREAM_REPLAY_GAP: &str = "stream_replay_gap";

// Containers.
pub const CONTAINER_CAPABILITY_REJECTED: &str = "container_capability_rejected";
pub const CONTAINER_HEALTH_FAILED: &str = "container_health_failed";
pub const CONTAINER_ROLLOUT_FAILED: &str = "container_rollout_failed";

// MCP.
pub const MCP_REQUEST_FAILED: &str = "mcp_request_failed";

// Optimizers.
pub const OPTIMIZER_SIDECAR_UNAVAILABLE: &str = "optimizer_sidecar_unavailable";
pub const OPTIMIZER_WORKER_FAILED: &str = "optimizer_worker_failed";

// Providers and sessions.
pub const PROVIDER_DISCONNECTED: &str = "provider_disconnected";
pub const PROVIDER_STALLED: &str = "provider_stalled";

// Diagnostics itself.
pub const DIAGNOSTICS_QUEUE_SATURATED: &str = "diagnostics_queue_saturated";
pub const DIAGNOSTICS_INDEX_DEGRADED: &str = "diagnostics_index_degraded";

const RANKS: &[(&str, u8)] = &[
    (CONTAINER_CAPABILITY_REJECTED, RANK_INFRASTRUCTURE),
    (CONTAINER_HEALTH_FAILED, RANK_INFRASTRUCTURE),
    (OPTIMIZER_SIDECAR_UNAVAILABLE, RANK_INFRASTRUCTURE),
    (DIAGNOSTICS_INDEX_DEGRADED, RANK_INFRASTRUCTURE),
    (CONTAINER_ROLLOUT_FAILED, RANK_TRANSPORT),
    (STREAM_SUBSCRIBE_TIMEOUT, RANK_TRANSPORT),
    (STREAM_INTERRUPTED, RANK_TRANSPORT),
    (PROVIDER_DISCONNECTED, RANK_TRANSPORT),
    (MCP_REQUEST_FAILED, RANK_TRANSPORT),
    (OPTIMIZER_WORKER_FAILED, RANK_TRANSPORT),
    (UNSUPPORTED_TRACE_PROJECTION_SCHEMA, RANK_CONTRACT),
    (VISUAL_BINDING_UNRESOLVED, RANK_CONTRACT),
    (VISUAL_TEMPLATE_UNAVAILABLE, RANK_CONTRACT),
    (STREAM_REPLAY_GAP, RANK_CONTRACT),
    (PROVIDER_STALLED, RANK_CONTRACT),
    (VISUAL_SHELL_LOAD_FAILED, RANK_SYMPTOM),
    (VISUAL_RENDER_FAILED, RANK_SYMPTOM),
    (DIAGNOSTICS_QUEUE_SATURATED, RANK_SYMPTOM),
];

pub fn rank(code: &str) -> u8 {
    RANKS
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, rank)| *rank)
        .unwrap_or(DEFAULT_RANK)
}

const REMEDIATIONS: &[(&str, &str)] = &[
    (
        UNSUPPORTED_TRACE_PROJECTION_SCHEMA,
        "The visual received a sealed trace in a schema its template does not accept. Project the trace into the template's declared input contract, or bind a template that accepts this projection schema.",
    ),
    (
        VISUAL_BINDING_UNRESOLVED,
        "A declared slot resolved to nothing. Check the binding's source id against the rollout, trace, or optimizer run it names, then re-bind the visual.",
    ),
    (
        VISUAL_TEMPLATE_UNAVAILABLE,
        "The template id on this visual is not present in the template registry. Re-create the visual against a listed template.",
    ),
    (
        VISUAL_SHELL_LOAD_FAILED,
        "The template's shell module failed to load. Rebuild the visual bundle; if it persists the template package is incomplete.",
    ),
    (
        VISUAL_RENDER_FAILED,
        "The shell threw while rendering. The bound data reached it, so read the error boundary's detail before re-binding — this is usually a template defect, not a data defect.",
    ),
    (
        STREAM_INTERRUPTED,
        "The live stream dropped while durable polling continued. Results are not lost — reconnect the stream, or read the rollout's terminal record instead of the live feed.",
    ),
    (
        STREAM_SUBSCRIBE_TIMEOUT,
        "The visual never reached subscribed state before the deadline. Confirm the container declared the stream transport and that the stream id matches the rollout.",
    ),
    (
        STREAM_REPLAY_GAP,
        "The replayed history has a hole in its sequence. Reload the stream from a durable snapshot rather than patching over the gap; a partial history is not evidence.",
    ),
    (
        CONTAINER_CAPABILITY_REJECTED,
        "The container did not declare the operations this rollout requires. Re-probe the container, then start the rollout only against a declared capability set.",
    ),
    (
        CONTAINER_HEALTH_FAILED,
        "The container failed its health or capability preflight. Re-probe it; a stale observation is refused deliberately rather than paid for.",
    ),
    (
        CONTAINER_ROLLOUT_FAILED,
        "The rollout terminated before producing its result. Inspect the container's own error in the details, then retry only if the failure is marked retryable.",
    ),
    (
        MCP_REQUEST_FAILED,
        "An MCP adapter call failed. Use the stable code and HTTP status in the details; retry only when the diagnostic is marked retryable.",
    ),
    (
        OPTIMIZER_SIDECAR_UNAVAILABLE,
        "The optimizer sidecar is not running or not installed. Start or install it from the Optimizers pane before starting a run.",
    ),
    (
        OPTIMIZER_WORKER_FAILED,
        "An optimizer worker exited before its run completed. The run's own bounded diagnostic evidence is correlated by optimizer_run_id.",
    ),
    (
        PROVIDER_DISCONNECTED,
        "The local agent connection dropped. The turn's durable journal is intact; reconnect and resume rather than restarting the task.",
    ),
    (
        PROVIDER_STALLED,
        "Provider activity stopped without a terminal event. Check the provider heartbeat in the details before interrupting the turn.",
    ),
    (
        DIAGNOSTICS_QUEUE_SATURATED,
        "Diagnostics were dropped under load. Counts by severity and component are in the details; errors are preserved preferentially.",
    ),
    (
        DIAGNOSTICS_INDEX_DEGRADED,
        "The local diagnostic index is unavailable. Queries are answering from the authoritative journal; no evidence is lost.",
    ),
];

pub fn remediation(code: &str) -> Option<&'static str> {
    REMEDIATIONS
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, text)| *text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capability_rejection_outranks_the_blank_visual_it_causes() {
        assert!(rank(CONTAINER_CAPABILITY_REJECTED) < rank(UNSUPPORTED_TRACE_PROJECTION_SCHEMA));
        assert!(rank(UNSUPPORTED_TRACE_PROJECTION_SCHEMA) < rank(VISUAL_RENDER_FAILED));
        assert!(rank(STREAM_SUBSCRIBE_TIMEOUT) < rank(VISUAL_RENDER_FAILED));
    }

    #[test]
    fn unknown_codes_get_a_neutral_rank_rather_than_a_panic() {
        assert_eq!(rank("something_new"), DEFAULT_RANK);
        assert_eq!(remediation("something_new"), None);
    }

    #[test]
    fn every_ranked_code_has_remediation_text() {
        for (code, _) in RANKS {
            assert!(remediation(code).is_some(), "{code} has no remediation");
        }
    }

    #[test]
    fn remediation_text_says_what_to_do_not_what_happened() {
        for (code, text) in REMEDIATIONS {
            assert!(text.len() > 40, "{code} remediation is too thin");
            assert!(!text.ends_with(':'), "{code} remediation is truncated");
        }
    }
}
