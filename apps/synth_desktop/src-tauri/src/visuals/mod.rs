//! Local Visual Registry: durable visual instances, revisions, and template catalog.

mod models;
mod registry;
mod templates;

pub use models::{
    validate_bindings, validate_template_bindings, RendererKind, VisualCreateRequest, VisualQuery,
    VisualRecord, VisualRevision, VisualStatus, VisualUpdateRequest,
    VISUAL_BINDINGS_SCHEMA_VERSION, VISUAL_SCHEMA_VERSION,
};
pub use registry::VisualRegistry;
pub use templates::{list_templates, resolve_template, TemplateMeta};
