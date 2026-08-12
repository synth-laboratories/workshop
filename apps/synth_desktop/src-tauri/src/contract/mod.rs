//! Interlanguage boundary constants (Wave 2 interim).
//!
//! Full tauri-specta codegen lands later; until then event channel names and
//! command strings are shared via these modules so Rust and TS stay aligned
//! without hand-written string literals at call sites.

pub mod commands;
pub mod events;

pub use commands::COMMANDS;
pub use events::{
    origin_for_boundary_kind, origin_for_source_and_kind, tag_event, EventChannel, EventOrigin,
    OriginTagged, EVENT_CHANNELS,
};
