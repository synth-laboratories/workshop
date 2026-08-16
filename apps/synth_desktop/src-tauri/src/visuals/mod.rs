//! Local Visual Registry: durable visual instances, revisions, and template catalog.

mod artifacts;
mod backfill;
mod live_eval;
pub mod mermaid;
mod models;
mod registry;
mod renditions;
pub mod systems;
mod templates;

pub use backfill::{canonicalize_persisted_bindings, BindingsBackfill};
pub use live_eval::{
    assert_declared_stream_source, assert_digbench_live_frames, assert_live_eval_slot,
    assert_no_live_secrets, assert_template_matches_family, classify_live_eval_family,
    craftax_ten_lane_pins, is_guessed_stream_url, is_never_declared_stream_url,
    live_eval_bind_metadata, live_sse_bindings, pending_stream_bindings,
    require_digbench_policy_pins, require_harbor_policy_pins, require_visualsbench_start_policy,
    resolve_live_eval_template, reward_from_env_status, visualsbench_policy_pins, LiveEvalFamily,
    CRAFTAX_TEN_LANE_SEEDS, LIVE_CRAFTAX_TEMPLATE, LIVE_DIGBENCH_TEMPLATE, LIVE_EVAL_SLOT,
    LIVE_HARBOR_TEMPLATE,
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
