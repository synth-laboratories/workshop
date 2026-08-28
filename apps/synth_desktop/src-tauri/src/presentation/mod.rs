//! Deterministic right-panel presentation, shared by the native UI and the
//! agent-facing MCP facades.
//!
//! Visual lifecycle for a domain record — identity, eligibility, binding,
//! reuse, and the show event — lives here rather than in whichever caller got
//! there first. The renderer's `DataPage` grew its own copy of this logic; a
//! second copy on the agent path would have drifted from it immediately.
//!
//! This module is the panel *host*: it owns the vocabulary every pane answers
//! in — whether a record can be shown, why not when it cannot, and the
//! deterministic identity that makes reuse possible. A *pane* answers only for
//! its own domain, in [`trace`] today. The host decides whether a record is
//! ready to present; a pane declares only what it would present.

mod trace;

pub use trace::{
    ensure_query_catalog, ensure_trace_inspector, trace_digest_binding, trace_inspectability,
    trace_inspector_visual_id, TRACE_CATALOG_TEMPLATE, TRACE_INSPECTOR_TEMPLATE,
    TRACE_PROJECTION_SCHEMA,
};

use crate::data::TraceRecord;

/// Whether a domain record can be presented in the right panel, and when it
/// cannot, why. The catalog shows every record and names the reason rather than
/// silently omitting the unavailable ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentability {
    Present,
    Unavailable(UnavailableReason),
}

/// Why a record that exists still cannot be presented. Each reason is a
/// distinct thing the catalog says out loud, so reasons are never merged: a
/// quarantined record and an incomplete archive are different problems with
/// different fixes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnavailableReason {
    Quarantined,
    ArchiveIncomplete,
    Unsupported,
}

impl Presentability {
    /// The catalog row label. These strings are wire values: the renderer keeps
    /// a by-hand mirror of this eligibility logic in
    /// `src/renderer/src/runtime/traceInspector.ts`, and the agent-facing trace
    /// rows carry them verbatim. `Present` reads as the pane's affordance
    /// rather than a state, which is why it is `Inspect`; when a second pane
    /// needs a different verb, the affordance moves onto [`Pane`] and this arm
    /// delegates to it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Present => "Inspect",
            Self::Unavailable(reason) => reason.label(),
        }
    }

    pub fn eligible(self) -> bool {
        matches!(self, Self::Present)
    }
}

impl UnavailableReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Quarantined => "Quarantined",
            Self::ArchiveIncomplete => "Archive incomplete",
            Self::Unsupported => "Unsupported",
        }
    }
}

/// A pane, paired with the domain record it would present.
///
/// Deliberately an enum with a `match` rather than a trait. At this provider
/// count a trait buys indirection and gives up exhaustiveness checking: adding
/// a domain to an enum makes every arm below a compiler error until it is
/// answered, where adding an `impl` is silent. Every arm is written
/// one-per-provider and delegates to its pane module, so lifting to a trait
/// once a third provider lands is mechanical — each arm becomes an `impl`
/// method and each `match` becomes a dynamic call. Pairing the pane with its
/// record in one value also means the pane and the record it answers for can
/// never disagree.
#[derive(Clone, Copy, Debug)]
pub enum Pane<'a> {
    Trace(&'a TraceRecord),
}

impl<'a> Pane<'a> {
    /// Shares the plugin id namespace, not the plugin lifecycle: a pane is
    /// compiled in and has no install phases.
    pub fn provider_id(self) -> &'static str {
        match self {
            Self::Trace(_) => trace::PROVIDER_ID,
        }
    }

    /// The template the host renders this pane through — host vocabulary, so
    /// the host resolves it rather than asking the record.
    pub fn template_id(self) -> &'static str {
        match self {
            Self::Trace(_) => TRACE_INSPECTOR_TEMPLATE,
        }
    }

    /// The schema of the projection the pane's binding addresses.
    pub fn projection_schema(self) -> &'static str {
        match self {
            Self::Trace(_) => TRACE_PROJECTION_SCHEMA,
        }
    }

    /// Whether this record can be shown, and when it cannot, why.
    pub fn presentable(self) -> Presentability {
        match self {
            Self::Trace(record) => trace::presentable(record),
        }
    }

    /// Deterministic identity for this record's visual, stable across restarts,
    /// windows, and callers. Reuse is decided by it alone.
    pub fn visual_id(self) -> String {
        match self {
            Self::Trace(record) => trace::visual_id(record),
        }
    }
}
