//! Local, app-bundled diagnostics.
//!
//! One versioned envelope (`synth.diagnostic-event.v1`) emitted by every
//! surface, persisted in the authoritative SQLite journal, indexed
//! asynchronously into a bundled loopback VictoriaLogs, and queried through a
//! typed, bounded MCP. Nothing leaves the machine, and nothing on a producer
//! path ever waits for any of it.
//!
//! ```text
//! renderer / tauri / mcp / containers / visuals / optimizers / providers
//!                              |
//!                     DiagnosticBus (bounded, non-blocking)
//!                              |
//!                     batched writer -> event journal (authoritative)
//!                              |
//!                     indexer (by durable sequence) -> VictoriaLogs
//!                              |
//!                     synth_diagnostics MCP  /  Diagnostics pane
//! ```
//!
//! The index is disposable. It returns journal sequences; the records
//! themselves always come back from the journal, so a wiped, stale, crashed,
//! or absent index changes how fast a question is answered and never what the
//! answer is.

pub mod bus;
pub mod codes;
pub mod event;
pub mod explain;
pub mod indexer;
pub mod query;
pub mod redact;
pub mod service;
pub mod sidecar;
pub mod store;
pub mod victorialogs;

pub use bus::{DiagnosticBus, Enqueued};
pub use event::{
    validate, Correlation, DiagnosticEvent, DiagnosticInput, Severity, DIAGNOSTIC_EVENT_SCHEMA,
    JOURNAL_KIND,
};
pub use query::DiagnosticQuery;
pub use service::{emit_optional, DiagnosticsService};
pub use sidecar::{SidecarState, VictoriaLogsSidecar};
pub use store::{DiagnosticRecord, DiagnosticStore};

/// Emit shorthand used across instrumentation call sites.
///
/// Reads at the call site as one statement about what failed, so instrumenting
/// a path stays a one-line change and no producer grows an error branch for
/// its own telemetry.
#[macro_export]
macro_rules! diagnose {
    ($service:expr, $severity:expr, $component:literal, $event:literal, $code:expr, $message:expr $(, $field:literal = $value:expr)* $(,)?) => {{
        let mut input = $crate::diagnostics::DiagnosticInput::new(
            $severity, $component, $event, $code, $message,
        );
        $( input.details.insert($field.into(), serde_json::json!($value)); )*
        $crate::diagnostics::emit_optional($service, input)
    }};
}
