//! Public contract projections and the generated desktop boundary.
//!
//! [`specta`] owns live Tauri registration and TypeScript exports. Migrated
//! domain operations project their MCP schemas through [`capabilities`].
//! Hand-maintained names remain explicitly inventoried until migrated.

pub mod capabilities;
pub mod desktop_dispatch;
pub mod desktop_policy;
pub mod commands;
pub mod events;
pub mod runtimes;
pub mod specta;

pub use commands::COMMANDS;
pub use events::{
    origin_for_boundary_kind, origin_for_source_and_kind, tag_event, EventChannel, EventOrigin,
    OriginTagged, EVENT_CHANNELS,
};
