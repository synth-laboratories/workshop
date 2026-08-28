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
//! its own domain — [`trace`] and [`document`] today. The host decides whether
//! a record is ready to present; a pane declares only what it would present.

mod document;
mod trace;

pub use document::{
    document_path_binding, ensure_document_viewer, DOCUMENT_PROJECTION_SCHEMA,
    DOCUMENT_VIEWER_TEMPLATE, WORKSPACE_FILE_BINDING_KIND,
};
pub use trace::{
    ensure_query_catalog, ensure_trace_inspector, trace_digest_binding, trace_inspectability,
    trace_inspector_visual_id, TRACE_CATALOG_TEMPLATE, TRACE_INSPECTOR_TEMPLATE,
    TRACE_PROJECTION_SCHEMA,
};

use crate::data::TraceRecord;
use crate::documents::DocumentRecord;

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
///
/// Reasons are host vocabulary, not per-pane vocabulary: `Missing` means the
/// same thing whichever pane raised it, and a pane that needed a private reason
/// would be telling the catalog something the catalog cannot render.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnavailableReason {
    Quarantined,
    ArchiveIncomplete,
    Unsupported,
    /// The record names a place that is not there. Distinct from `Unsupported`:
    /// nothing about the request was wrong, the thing is simply gone.
    Missing,
    /// Bytes the pane would render as mojibake — a binary, or a file in an
    /// encoding this build does not decode.
    NotText,
    /// A folder where a document was asked for. The folder is fine; it is not
    /// a thing the document pane can typeset, and the listing view is.
    NotADocument,
    /// A document where a folder was asked for.
    NotADirectory,
    /// Metadata read, bytes refused — a permissions or I/O failure that is a
    /// property of this machine rather than of the record.
    Unreadable,
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
            Self::Missing => "Missing",
            Self::NotText => "Not text",
            Self::NotADocument => "Not a document",
            Self::NotADirectory => "Not a folder",
            Self::Unreadable => "Unreadable",
        }
    }

    /// What the reader can do next. A named reason with no next step is still a
    /// dead end; §6 of the style guide asks for the recovery action beside the
    /// state, and the panel is where the reader is standing when they read it.
    pub fn remediation(self) -> &'static str {
        match self {
            Self::Quarantined => "Re-import the archive from a trusted source.",
            Self::ArchiveIncomplete => "Re-seal the trace so its archive is self-contained.",
            Self::Unsupported => "Open it with an application that understands this format.",
            Self::Missing => "Check the path, or reopen it from the folder listing.",
            Self::NotText => "Open it externally with the Open menu.",
            Self::NotADocument => "Open it as a folder to see what is inside.",
            Self::NotADirectory => "Open the containing folder instead.",
            Self::Unreadable => "Check the file's permissions, then try again.",
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
///
/// Two providers now, and the enum still earns its keep. [`Document`] is the
/// one that tested it: it is not trace-shaped — no digest identity, no sealed
/// archive, a mutable subject — and adding it needed exactly one new arm per
/// method plus two host reasons, with the compiler naming every place that had
/// to answer. Lifting to a `trait PanelProvider` buys dynamic dispatch nothing
/// asks for here and loses that exhaustiveness. The rule for the next person:
/// a third provider that is again a variation on "a record with an identity and
/// an eligibility test" still belongs in the enum; the trait becomes right when
/// providers arrive from *outside* this crate — a plugin supplying a pane —
/// because at that point the set is no longer closed and there is nothing left
/// for the compiler to be exhaustive over.
///
/// [`Document`]: Pane::Document
#[derive(Clone, Copy, Debug)]
pub enum Pane<'a> {
    Trace(&'a TraceRecord),
    Document(&'a DocumentRecord),
}

impl<'a> Pane<'a> {
    /// Shares the plugin id namespace, not the plugin lifecycle: a pane is
    /// compiled in and has no install phases.
    pub fn provider_id(self) -> &'static str {
        match self {
            Self::Trace(_) => trace::PROVIDER_ID,
            Self::Document(_) => document::PROVIDER_ID,
        }
    }

    /// The template the host renders this pane through — host vocabulary, so
    /// the host resolves it rather than asking the record.
    pub fn template_id(self) -> &'static str {
        match self {
            Self::Trace(_) => TRACE_INSPECTOR_TEMPLATE,
            Self::Document(_) => DOCUMENT_VIEWER_TEMPLATE,
        }
    }

    /// The schema of the projection the pane's binding addresses.
    pub fn projection_schema(self) -> &'static str {
        match self {
            Self::Trace(_) => TRACE_PROJECTION_SCHEMA,
            Self::Document(_) => DOCUMENT_PROJECTION_SCHEMA,
        }
    }

    /// Whether this record can be shown, and when it cannot, why.
    pub fn presentable(self) -> Presentability {
        match self {
            Self::Trace(record) => trace::presentable(record),
            Self::Document(record) => document::presentable(record),
        }
    }

    /// Deterministic identity for this record's visual, stable across restarts,
    /// windows, and callers. Reuse is decided by it alone.
    pub fn visual_id(self) -> String {
        match self {
            Self::Trace(record) => trace::visual_id(record),
            Self::Document(record) => document::visual_id(record),
        }
    }
}
