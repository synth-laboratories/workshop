//! Interlanguage boundary (Wave 2).
//!
//! Hand-maintained name constants remain until full tauri-specta migration.
//! Specta scaffolding lives in [`specta`] — one seed command is exported to
//! `src/renderer/src/generated/protocol.ts` while `generate_handler!` still
//! owns invoke registration for the full command set.

pub mod commands;
pub mod events;
pub mod specta;

pub use commands::COMMANDS;
pub use events::{
    origin_for_boundary_kind, origin_for_source_and_kind, tag_event, EventChannel, EventOrigin,
    OriginTagged, EVENT_CHANNELS,
};
