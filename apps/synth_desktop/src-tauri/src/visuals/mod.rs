//! Local Visual Registry: durable visual instances, revisions, and template catalog.

mod artifacts;
mod backfill;
pub mod chart_data;
pub mod charts;
mod live_eval;
pub mod mermaid;
mod models;
mod registry;
mod renditions;
#[cfg(target_os = "macos")]
pub mod snapshot;
pub mod systems;
mod templates;

/// The repository's `visuals/` root, so tests can load the same fixtures the
/// binding resolver reads.
#[cfg(test)]
pub fn templates_root_for_tests() -> std::path::PathBuf {
    templates::visuals_root()
}

/// Templates whose canonical source is the visual itself: create and update
/// refuse them without `content`, and their pixels come from a host renderer
/// rather than a Desktop pane. Callers that just need "some template" — tests,
/// pickers — must skip these rather than sniff the id.
pub fn requires_canonical_source(template_id: &str) -> bool {
    mermaid::is_mermaid_template(template_id)
        || systems::template_kind(template_id).is_some()
        || charts::is_chart_template(template_id)
}

pub use backfill::{canonicalize_persisted_bindings, BindingsBackfill};
pub use live_eval::{
    advertised_live_eval_template, assert_declared_stream_source, assert_live_eval_slot,
    assert_no_live_secrets, is_guessed_stream_url, is_never_declared_stream_url,
    live_eval_bind_metadata, live_sse_bindings, pending_stream_bindings,
    require_advertised_mcp_bind, require_policy_pins, resolve_live_eval_template,
    reward_from_env_status, seed_lane_pins, LIVE_EVAL_SLOT, TEN_LANE_SEEDS,
};
pub use models::{
    canonicalize_bindings, declared_poll_urls, BindingsForm, CanonicalBindings, RendererKind,
    VisualAnnotation, VisualAnnotationCreate, VisualCreateRequest, VisualQuery, VisualRecord,
    VisualRevision, VisualSeal, VisualSealBundle, VisualStatus, VisualUpdateRequest, VisualUpload,
    VISUAL_BINDINGS_SCHEMA_VERSION, VISUAL_BINDING_KINDS, VISUAL_SCHEMA_VERSION,
};
pub use registry::VisualRegistry;
pub use renditions::{VisualAsset, VisualRendition};
pub use templates::{
    list_templates, resolve_template, TemplateMeta, TemplateObservationContract,
    TemplateReadinessContract,
};
