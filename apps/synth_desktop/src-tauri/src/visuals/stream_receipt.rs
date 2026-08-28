//! Host-observed receipt for a visual's declared live streams.
//!
//! Every renderer poll of a live stream lands in `visual_stream_poll`, which
//! already holds the bytes, the declared bindings, the visual id and the
//! revision. That makes this the one place a stream's behaviour can be recorded
//! without asking the thing under test to describe itself: the receipt is not
//! renderer-reported and not agent-authored, so an agent that reads it is
//! reading the transport, not its own narration of the transport.
//!
//! # What this is not
//!
//! This is **not** the live-eval fold. It does not project, it does not keep
//! envelope bodies, and it answers no question about what the visual should
//! draw. It keeps the small amount of bookkeeping the poll seam can observe
//! honestly — identity, sequence, kind, cursor, latency — so that "declared ten
//! streams and opened none" and "opened fine, received only control envelopes"
//! stop being the same empty pane. When the fold moves into Rust wholesale, the
//! fold subsumes this bookkeeping; until then this seam is the only server-side
//! observation there is.
//!
//! # Where the rules live now
//!
//! Identity, scope, the control predicate, dedupe, conflict detection and the
//! sequence-gap scan are [`crate::stream_fold`]. This module keeps only what
//! the *poll seam* can observe that a fold cannot: which declared stream a
//! page came back on, how long it took to answer, whether it closed, and what
//! failed. Everything else here is that fold, read.
//!
//! Two rules were written here and in the TypeScript ingest independently, in
//! the same afternoon, and arrived at the same answer; both now have one home:
//!
//! 1. **Control envelopes keep their sequence numbers.** A gap is a claim about
//!    the producer's sequence space and control records occupy that space.
//! 2. **`control: true` is honored** alongside control kinds.
//!
//! The one real divergence — `last_sequence` per *declared stream* and
//! advanced by control here, `lastSequenceByScope` and evidence-only in
//! TypeScript — is **resolved in favour of evidence-only**. A stream carrying
//! nothing but sequenced heartbeats has not advanced its evidence, and this
//! receipt exists precisely so that a gate cannot be told otherwise. The gap
//! scan still counts those heartbeats; the high-water mark does not. See
//! `stream_fold.rs` rule 4.

use super::models::canonicalize_bindings;
use crate::stream_fold::{self, FoldLimits, LiveFold};
use crate::visuals_ipc::RenderedVisualObservation;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

/// One identity delivered twice with two different bodies. As above.
pub use crate::stream_fold::EnvelopeConflict as StreamConflict;
/// A hole in a producer's sequence space. Defined by the fold; re-exported so
/// a reader of the receipt does not have to know where the scan lives.
pub use crate::stream_fold::SequenceGap as StreamGap;

/// Receipt envelope version. A reader that does not know this string should
/// refuse the receipt rather than guess at its fields.
pub const VISUAL_STREAM_RECEIPT_SCHEMA: &str = "synth.visual-stream-receipt.v1";

/// Bookkeeping bounds for one visual's fold.
///
/// The receipt is a live, unbounded-lifetime observation of a stream that may
/// carry hundreds of thousands of envelopes, so the bookkeeping is bounded and
/// says when it stopped being complete. A truncated receipt reports lower
/// bounds; it never reports a smaller number as if it were the whole count.
/// Envelope bodies are never retained: they carry model output and rollout
/// payloads, and a receipt is identifiers and counts.
const RECEIPT_FOLD_LIMITS: FoldLimits = FoldLimits {
    max_identities: 50_000,
    max_sequences_per_scope: 50_000,
    max_defects: 64,
    retain_events: false,
};

/// Evidence bodies retained per stream before the store reports lower bounds.
///
/// A live stream has an unbounded lifetime and frame envelopes are not small,
/// so retention is bounded and says when it stopped being complete. A seal
/// over a truncated prefix is still a seal over real, replayable evidence —
/// it just says so, rather than presenting a prefix as the whole run.
const MAX_RETAINED_EVIDENCE: usize = 20_000;

/// Bytes of evidence retained per stream.
///
/// A sealed bundle carries its evidence twice — once in `data.json` and once
/// inlined into `index.html` — against a 64 MiB hosted-viewer limit, and a
/// Craftax frame envelope is not small. A prefix that seals is worth more than
/// a whole run that cannot be shared, so retention stops here and says so.
const MAX_RETAINED_BYTES: usize = 8 * 1024 * 1024;

/// The transport lifecycle, as the host observed it.
///
/// The same six states the renderer's `TransportState` names, read from the
/// poll seam rather than from renderer state. The mapping is exact for `idle`,
/// `declared` and `terminal`; `replaying` here means "a poll was issued and has
/// not answered yet", and `error` is the last observation rather than a resting
/// state — a poll that fails and then succeeds reports `live` with a non-zero
/// `pollFailures`, because the transport did in fact recover and a gate that
/// blocked on the memory of a recovered failure would block honest runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum StreamTransportState {
    /// No stream is declared. Nothing is pending and nothing is wrong.
    Idle,
    /// Streams are declared and the host has issued no poll for them.
    Declared,
    /// A poll is outstanding and no page has come back yet.
    Replaying,
    /// At least one page arrived and some declared stream is still open.
    Live,
    /// Every declared stream reported a closed cursor.
    Terminal,
    /// The most recent observation was a refusal or a transport failure.
    Error,
}

impl Default for StreamTransportState {
    /// A visual nobody declared a stream for is idle, not broken.
    fn default() -> Self {
        Self::Idle
    }
}

impl StreamTransportState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Declared => "declared",
            Self::Replaying => "replaying",
            Self::Live => "live",
            Self::Terminal => "terminal",
            Self::Error => "error",
        }
    }
}

/// Why the last poll of one stream failed, kept whole.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StreamPollFailure {
    /// A `diagnostics::codes` constant, so the failure joins its remediation.
    pub code: String,
    pub message: String,
    #[specta(type = Option<specta_typescript::Number>)]
    pub status: Option<u16>,
    pub retryable: bool,
    pub observed_at: String,
}

/// Envelopes delivered under one `kind`, so an all-heartbeat stream is legible.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StreamKindCount {
    pub kind: String,
    #[specta(type = specta_typescript::Number)]
    pub count: u64,
    pub control: bool,
}

/// One declared stream, as the host saw it behave.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StreamReceiptStream {
    /// The renderer's `streamId`: the declared `source`, or the poll URL when
    /// the binding declares no source. Derived from the same bindings the
    /// renderer reads, so the two agree by construction.
    pub stream_id: String,
    /// The declared durable poll authority. Replay works from this alone.
    pub declared_source: String,
    /// The declared incremental transport, when the binding names one.
    pub sse_source: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub poll_attempts: u64,
    #[specta(type = specta_typescript::Number)]
    pub poll_responses: u64,
    #[specta(type = specta_typescript::Number)]
    pub poll_failures: u64,
    /// Milliseconds from the first poll issued to the first page returned.
    /// `null` while a declared stream has never answered.
    #[specta(type = Option<specta_typescript::Number>)]
    pub first_response_latency_ms: Option<u64>,
    /// Highest numeric sequence delivered on this stream. `null` when the
    /// producer sequences with non-numeric strings, which is legitimate — the
    /// multiplexed Craftax fixture does exactly that — and is not a defect.
    #[specta(type = Option<specta_typescript::Number>)]
    pub last_sequence: Option<i64>,
    /// The producer's own cursor, passed through rather than recomputed.
    #[specta(type = Option<specta_typescript::Number>)]
    pub cursor_next: Option<i64>,
    /// Envelopes handed to the renderer, duplicates included: what the
    /// transport delivered, before any fold has an opinion about it.
    #[specta(type = specta_typescript::Number)]
    pub envelope_count: u64,
    /// Envelopes with a distinct identity: what a fold would keep.
    #[specta(type = specta_typescript::Number)]
    pub distinct_envelope_count: u64,
    pub closed: bool,
    pub last_failure: Option<StreamPollFailure>,
}

/// What the host observed of one visual's declared streams.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StreamReceipt {
    pub schema_version: String,
    pub visual_id: String,
    #[specta(type = specta_typescript::Number)]
    pub revision: i64,
    pub state: StreamTransportState,
    /// Milliseconds the host has held the reported state. A visual resting in
    /// `declared` for a minute is the failure this number exists to name.
    #[specta(type = specta_typescript::Number)]
    pub time_in_state_ms: u64,
    /// False when the host has recorded no poll at all for this visual and
    /// revision. A browser preview polls with raw `fetch` and never reaches
    /// this seam, so `observed: false` reads as "not shown in Desktop" — which
    /// is the right answer for a pane no reviewer ever rendered.
    pub observed: bool,
    /// Whether the host ever saw this visual advance past `declared`. Distinct
    /// from `state`: a stream that answered once and then failed has left
    /// `declared`, and one that never answered has not.
    pub ever_left_declared: bool,
    #[specta(type = specta_typescript::Number)]
    pub declared_stream_count: u64,
    /// Declared streams that returned at least one page.
    #[specta(type = specta_typescript::Number)]
    pub responding_stream_count: u64,
    #[specta(type = specta_typescript::Number)]
    pub closed_stream_count: u64,
    /// Declared `live_sse` bindings carrying no `poll_url`. The renderer cannot
    /// replay these at all, so they are declared and unreachable rather than
    /// declared and quiet.
    pub streams_missing_transport: Vec<String>,
    pub streams: Vec<StreamReceiptStream>,
    pub gaps: Vec<StreamGap>,
    pub conflicts: Vec<StreamConflict>,
    /// A `stream.subscribed` control envelope was delivered. The same signal
    /// the renderer's ingest folds into `ready`.
    pub ready: bool,
    /// Distinct non-control envelopes accepted across every declared stream:
    /// the evidence a fold would have to work with.
    #[specta(type = specta_typescript::Number)]
    pub recovered: u64,
    #[specta(type = specta_typescript::Number)]
    pub envelope_count: u64,
    /// Envelopes that are not heartbeats, pings or subscription notices.
    /// A stream can be perfectly healthy on every other field and still have
    /// carried no evidence at all; this is the field that says so.
    #[specta(type = specta_typescript::Number)]
    pub non_control_envelope_count: u64,
    pub envelopes_by_kind: Vec<StreamKindCount>,
    /// Set once bookkeeping hit its bound. Dedupe, gaps and conflicts become
    /// lower bounds from that point; the counts of delivered envelopes do not.
    pub tracking_truncated: bool,
    pub first_observed_at: Option<String>,
    pub last_observed_at: Option<String>,
}

/// A live stream the visual's bindings declare, read the way the renderer's
/// `replayStreamsFromBindings` reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredStream {
    pub stream_id: String,
    pub poll_url: String,
    pub sse_url: Option<String>,
}

/// Declared streams plus the ones that cannot be replayed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeclaredStreams {
    pub streams: Vec<DeclaredStream>,
    /// Declared `live_sse` bindings with no durable poll authority, named by
    /// their `source` so the report identifies which binding is unreachable.
    pub missing_transport: Vec<String>,
}

/// Read the declared live streams out of a visual's bindings.
///
/// One authority decides what a visual declared, and this reads the same
/// canonical envelope `declared_poll_urls` reads. Identity matches the
/// renderer's `streamId` rule — declared `source`, falling back to the poll URL
/// — so the receipt and the pane name the same stream.
pub fn declared_streams(bindings: &Value) -> DeclaredStreams {
    let Ok(canonical) = canonicalize_bindings(bindings) else {
        return DeclaredStreams::default();
    };
    let mut declared = DeclaredStreams::default();
    let Some(slots) = canonical
        .value
        .get("inputs")
        .or_else(|| canonical.value.get("slots"))
        .and_then(Value::as_array)
    else {
        return declared;
    };
    for (index, slot) in slots.iter().enumerate() {
        if slot.get("kind").and_then(Value::as_str) != Some("live_sse") {
            continue;
        }
        let source = slot
            .get("source")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        match slot
            .get("poll_url")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            Some(poll_url) => declared.streams.push(DeclaredStream {
                stream_id: source.unwrap_or(poll_url).to_string(),
                poll_url: poll_url.to_string(),
                sse_url: source.map(str::to_string),
            }),
            None => declared
                .missing_transport
                .push(source.map(str::to_string).unwrap_or_else(|| {
                    let input = slot
                        .get("input")
                        .or_else(|| slot.get("slot"))
                        .and_then(Value::as_str)
                        .unwrap_or("stream");
                    format!("{input}[{index}]")
                })),
        }
    }
    declared
}

/// What a recorded page changed, for the caller that has to report it.
#[derive(Clone, Debug, Default)]
pub struct PollOutcome {
    /// Gaps observed for the first time by this page. Emitted as
    /// `STREAM_REPLAY_GAP` by the caller, once per gap rather than once per
    /// poll: a 500 ms loop over a permanent hole would otherwise be a
    /// diagnostic every 500 ms forever.
    pub new_gaps: Vec<StreamGap>,
    /// Conflicts observed for the first time by this page.
    pub new_conflicts: Vec<StreamConflict>,
    pub state: StreamTransportState,
    /// Retention of this stream's evidence bodies has stopped short of the
    /// run, so anything folded from the prefix is a lower bound.
    pub evidence_truncated: bool,
}

impl PollOutcome {
    /// The state name, for a diagnostic detail bag.
    pub fn state_str(&self) -> &'static str {
        self.state.as_str()
    }
}

#[derive(Debug)]
struct StreamState {
    stream_id: String,
    declared_source: String,
    sse_source: Option<String>,
    poll_attempts: u64,
    poll_responses: u64,
    poll_failures: u64,
    first_attempt_at: Option<Instant>,
    first_response_latency_ms: Option<u64>,
    last_sequence: Option<i64>,
    cursor_next: Option<i64>,
    envelope_count: u64,
    distinct_envelope_count: u64,
    closed: bool,
    last_failure: Option<StreamPollFailure>,
}

impl StreamState {
    fn new(declared: &DeclaredStream) -> Self {
        Self {
            stream_id: declared.stream_id.clone(),
            declared_source: declared.poll_url.clone(),
            sse_source: declared.sse_url.clone(),
            poll_attempts: 0,
            poll_responses: 0,
            poll_failures: 0,
            first_attempt_at: None,
            first_response_latency_ms: None,
            last_sequence: None,
            cursor_next: None,
            envelope_count: 0,
            distinct_envelope_count: 0,
            closed: false,
            last_failure: None,
        }
    }

    fn view(&self) -> StreamReceiptStream {
        StreamReceiptStream {
            stream_id: self.stream_id.clone(),
            declared_source: self.declared_source.clone(),
            sse_source: self.sse_source.clone(),
            poll_attempts: self.poll_attempts,
            poll_responses: self.poll_responses,
            poll_failures: self.poll_failures,
            first_response_latency_ms: self.first_response_latency_ms,
            last_sequence: self.last_sequence,
            cursor_next: self.cursor_next,
            envelope_count: self.envelope_count,
            distinct_envelope_count: self.distinct_envelope_count,
            closed: self.closed,
            last_failure: self.last_failure.clone(),
        }
    }
}

#[derive(Debug)]
struct VisualState {
    revision: i64,
    /// Declared stream ids in binding order. The renderer resets its ingest
    /// when this changes; so does this, for the same reason.
    stream_key: Vec<String>,
    state: StreamTransportState,
    state_since: Instant,
    ever_left_declared: bool,
    first_observed_at: String,
    last_observed_at: String,
    streams: BTreeMap<String, StreamState>,
    /// The envelope accounting, in its one home. Everything this module used
    /// to keep by hand — identities, per-scope sequence spaces, gaps,
    /// conflicts, kind counts, `ready`, the truncation flag and the delivered
    /// ordinal — is this.
    fold: LiveFold,
    /// Accepted evidence bodies in arrival order across every declared stream,
    /// each tagged with the declared stream it came back on.
    ///
    /// The receipt reads none of this: a receipt is identifiers and counts, and
    /// that promise has not changed. It is here because the seal and the
    /// projection need replayable bodies and this is the one seam every polled
    /// envelope passes through — the same key, the same fold, the same
    /// revision lifetime, so keeping it in a second process-global bought two
    /// answers to one question and a second lock on the poll path.
    ///
    /// One arrival order, not one per stream: the renderer folds every
    /// declared stream into one ingest, so a projection served from here has
    /// to be able to answer in the order the pane saw.
    evidence: Vec<(String, Value)>,
    /// Per-stream retention accounting, so the bound stays per stream and a
    /// quiet stream is never charged for a loud one.
    evidence_books: BTreeMap<String, EvidenceBook>,
    observed: bool,
    failed_last: bool,
}

/// What one declared stream's retained prefix has cost, and whether it stopped
/// being the whole run.
#[derive(Debug, Default)]
struct EvidenceBook {
    kept: usize,
    bytes: usize,
    truncated: bool,
}

impl VisualState {
    fn new(revision: i64, declared: &DeclaredStreams) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            revision,
            stream_key: declared
                .streams
                .iter()
                .map(|stream| stream.stream_id.clone())
                .collect(),
            state: if declared.streams.is_empty() {
                StreamTransportState::Idle
            } else {
                StreamTransportState::Declared
            },
            state_since: Instant::now(),
            ever_left_declared: false,
            first_observed_at: now.clone(),
            last_observed_at: now,
            streams: BTreeMap::new(),
            fold: LiveFold::new(RECEIPT_FOLD_LIMITS),
            evidence: Vec::new(),
            evidence_books: BTreeMap::new(),
            observed: false,
            failed_last: false,
        }
    }

    /// Retain one accepted evidence body, inside the bound.
    ///
    /// Returns false once the bound is reached, so a caller stops walking the
    /// rest of the batch instead of asking the same refused question per
    /// envelope. The prefix is then a lower bound on the run and says so.
    fn retain_evidence(&mut self, stream_id: &str, envelope: &Value) -> bool {
        let size = serde_json::to_string(envelope)
            .map(|text| text.len())
            .unwrap_or(0);
        let book = self
            .evidence_books
            .entry(stream_id.to_string())
            .or_default();
        if book.kept >= MAX_RETAINED_EVIDENCE
            || book.bytes.saturating_add(size) > MAX_RETAINED_BYTES
        {
            book.truncated = true;
            return false;
        }
        book.bytes += size;
        book.kept += 1;
        self.evidence
            .push((stream_id.to_string(), envelope.clone()));
        true
    }

    fn stream_mut(&mut self, declared: &DeclaredStream) -> &mut StreamState {
        self.streams
            .entry(declared.poll_url.clone())
            .or_insert_with(|| StreamState::new(declared))
    }

    fn enter(&mut self, next: StreamTransportState) {
        if next != StreamTransportState::Idle && next != StreamTransportState::Declared {
            self.ever_left_declared = true;
        }
        if self.state != next {
            self.state = next;
            self.state_since = Instant::now();
        }
    }

    /// Derive the state from what the host has observed, not from what the
    /// renderer says about itself.
    fn recompute(&mut self, declared: &DeclaredStreams) {
        let next = if declared.streams.is_empty() {
            StreamTransportState::Idle
        } else if self.failed_last {
            StreamTransportState::Error
        } else if self
            .streams
            .values()
            .all(|stream| stream.poll_attempts == 0)
        {
            StreamTransportState::Declared
        } else if self
            .streams
            .values()
            .all(|stream| stream.poll_responses == 0)
        {
            StreamTransportState::Replaying
        } else if declared
            .streams
            .iter()
            .all(|stream| self.stream_closed(&stream.poll_url))
        {
            StreamTransportState::Terminal
        } else {
            StreamTransportState::Live
        };
        self.enter(next);
    }

    fn stream_closed(&self, poll_url: &str) -> bool {
        self.streams
            .get(poll_url)
            .is_some_and(|stream| stream.closed)
    }

    fn touch(&mut self) {
        self.last_observed_at = chrono::Utc::now().to_rfc3339();
        self.observed = true;
    }
}

/// Everything this host observed about one visual, in one place.
///
/// Three stores used to hold this: what the pane reported after it rendered,
/// what the poll seam saw of the transport, and the envelope bodies the seal
/// replays. Same key, same seam, same process lifetime, three globals — and so
/// three answers to "what happened to this visual", each with its own lock and
/// its own reset rule.
///
/// They are one store now and three responsibilities still, because the
/// promises differ and blurring them would cost more than the duplication did:
///
/// * [`RenderedVisualObservation`] is what only the DOM can know — rendered
///   frames, semantic events — and is therefore renderer-reported. It is kept
///   apart from the transport observation precisely so a gate can tell the two
///   apart.
/// * [`VisualState`] is what the host itself saw at the poll seam. Nothing in
///   it is reported by the thing under test.
/// * The evidence prefix inside it is bodies, bounded, for the seal. The
///   receipt still retains none of its own.
#[derive(Debug, Default)]
struct VisualObservation {
    /// Renderer-reported, replaced whole on each report and never revision
    /// reset: a report carries its own `rendered_revision` and the gate reads
    /// it there.
    rendered: Option<RenderedVisualObservation>,
    /// Host-observed at the poll seam, reset when the revision or the declared
    /// stream set changes.
    transport: Option<VisualState>,
}

/// Process-global observation store, keyed by visual id.
///
/// An observation of a running process, not a durable record: it must not
/// survive a restart claiming a stream was seen that this process never saw.
static VISUAL_OBSERVATIONS: OnceLock<Mutex<BTreeMap<String, VisualObservation>>> = OnceLock::new();

/// Take the store lock, recovering the map if some other caller panicked while
/// holding it.
///
/// A poisoned lock here means an unrelated panic, not a broken map: nothing in
/// this module can leave the bookkeeping half-updated across an unwind, because
/// every mutation completes inside one call. Refusing to observe a stream
/// because of someone else's panic would turn a receipt into a second failure
/// report about itself.
fn store() -> std::sync::MutexGuard<'static, BTreeMap<String, VisualObservation>> {
    VISUAL_OBSERVATIONS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

// ---------------------------------------------------------------------------
// Responsibility 1: what the pane reported after it rendered.
// ---------------------------------------------------------------------------

/// Record what the renderer saw of its own DOM.
///
/// Deliberately not merged into the receipt: this is the one observation the
/// host cannot make for itself, and a gate that could not tell it apart from a
/// host observation would be reading the thing under test's own account of
/// itself without knowing.
pub fn record_rendered(observation: RenderedVisualObservation) {
    let visual_id = observation.visual_id.clone();
    store().entry(visual_id).or_default().rendered = Some(observation);
}

/// The last rendered observation for a visual, if the pane ever reported one.
pub fn rendered(visual_id: &str) -> Option<RenderedVisualObservation> {
    store().get(visual_id)?.rendered.clone()
}

fn entry<'a>(
    store: &'a mut BTreeMap<String, VisualObservation>,
    visual_id: &str,
    revision: i64,
    declared: &DeclaredStreams,
) -> &'a mut VisualState {
    let observation = store.entry(visual_id.to_string()).or_default();
    let stale = observation.transport.as_ref().is_some_and(|state| {
        state.revision != revision
            || state.stream_key
                != declared
                    .streams
                    .iter()
                    .map(|stream| stream.stream_id.clone())
                    .collect::<Vec<_>>()
    });
    if stale {
        // A revision or a re-binding replaces the stream set. Carrying the old
        // observation forward would let a previous revision's evidence answer
        // for this one. The rendered report beside it is untouched: it carries
        // its own revision and answers a different question.
        observation.transport = None;
    }
    observation
        .transport
        .get_or_insert_with(|| VisualState::new(revision, declared))
}

/// The transport observation, for a caller that must not create or reset one.
///
/// A read never resets: `observed_evidence` asking about a revision this host
/// never polled must answer "no", not erase the revision it did poll.
fn read<'a>(
    store: &'a BTreeMap<String, VisualObservation>,
    visual_id: &str,
    revision: i64,
) -> Option<&'a VisualState> {
    store
        .get(visual_id)?
        .transport
        .as_ref()
        .filter(|state| state.revision == revision)
}

/// The evidence-side entry: revision scoped, and blind to the declared set.
///
/// Recording evidence is not an observation of a declared stream — a caller
/// may hand over envelopes for a stream this host never polled — so this one
/// resets on the revision alone, which is exactly what the evidence store next
/// door did before the two became one.
fn evidence_entry<'a>(
    store: &'a mut BTreeMap<String, VisualObservation>,
    visual_id: &str,
    revision: i64,
) -> &'a mut VisualState {
    let observation = store.entry(visual_id.to_string()).or_default();
    if observation
        .transport
        .as_ref()
        .is_some_and(|state| state.revision != revision)
    {
        observation.transport = None;
    }
    observation
        .transport
        .get_or_insert_with(|| VisualState::new(revision, &DeclaredStreams::default()))
}

/// Record that a poll was issued, before its answer is known.
///
/// Without this the host could never report `replaying`: a stream that is being
/// asked and a stream nobody asked would both read as `declared`.
pub fn record_poll_attempt(
    visual_id: &str,
    revision: i64,
    declared: &DeclaredStreams,
    poll_url: &str,
) {
    let mut store = store();
    let state = entry(&mut store, visual_id, revision, declared);
    let Some(stream) = declared
        .streams
        .iter()
        .find(|stream| stream.poll_url == poll_url)
        .cloned()
    else {
        return;
    };
    let entry = state.stream_mut(&stream);
    entry.poll_attempts += 1;
    if entry.first_attempt_at.is_none() {
        entry.first_attempt_at = Some(Instant::now());
    }
    state.recompute(declared);
}

/// Record a page the host actually fetched.
pub fn record_poll_page(
    visual_id: &str,
    revision: i64,
    declared: &DeclaredStreams,
    poll_url: &str,
    page: &Value,
) -> PollOutcome {
    let mut store = store();
    let state = entry(&mut store, visual_id, revision, declared);
    state.touch();
    state.failed_last = false;
    let Some(stream) = declared
        .streams
        .iter()
        .find(|stream| stream.poll_url == poll_url)
        .cloned()
    else {
        state.recompute(declared);
        return PollOutcome {
            state: state.state,
            ..PollOutcome::default()
        };
    };

    let events = page_events(page);
    let cursor = page_cursor(page);
    {
        let entry = state.stream_mut(&stream);
        entry.poll_responses += 1;
        entry.closed |= cursor.closed;
        if let Some(next) = cursor.next {
            entry.cursor_next = Some(next);
        }
        if entry.first_response_latency_ms.is_none() {
            entry.first_response_latency_ms = Some(
                entry
                    .first_attempt_at
                    .map(|at| at.elapsed().as_millis() as u64)
                    .unwrap_or_default(),
            );
        }
    }

    // One fold decides what these envelopes are; this loop only writes down
    // which declared stream they came back on. Every rule the loop used to
    // re-implement — identity, scope, control, dedupe, conflict, the gap scan
    // — is `stream_fold`, and the seam cannot drift from the renderer's fold
    // because there is no second implementation to drift from.
    let batch = state.fold.accept_batch(events.iter());
    // The bodies go into the same entry, from the same verdicts. The receipt
    // keeps none of them — it is identifiers and counts, and every field it
    // reports below is a count or an identifier. What the retention buys is
    // the seal: this is the only seam every polled envelope passes through, so
    // it is the only place a live-eval seal can get replayable evidence
    // without a caller being asked to remember to attach it. A required key
    // nobody wrote is how sealing came to be dead for every live visual;
    // nothing here has to be remembered.
    let mut retaining = true;
    for (step, envelope) in batch.steps.iter().zip(events.iter()) {
        {
            let entry = state.stream_mut(&stream);
            entry.envelope_count += 1;
            if step.verdict.accepted() {
                entry.distinct_envelope_count += 1;
            }
        }
        // Rule 4: the evidence high-water mark is evidence-only. A duplicate
        // was never accepted and a control record is not evidence, so neither
        // advances it — a stream carrying nothing but sequenced heartbeats has
        // made no progress and this number must not say it has.
        if step.verdict == stream_fold::FoldVerdict::Evidence {
            if let Some(sequence) = step.sequence {
                let entry = state.stream_mut(&stream);
                entry.last_sequence = Some(
                    entry
                        .last_sequence
                        .map_or(sequence, |last| last.max(sequence)),
                );
            }
            if retaining {
                retaining = state.retain_evidence(&stream.stream_id, envelope);
            }
        }
    }

    state.recompute(declared);
    let evidence_truncated = state
        .evidence_books
        .get(&stream.stream_id)
        .is_some_and(|book| book.truncated);
    PollOutcome {
        new_gaps: batch.new_gaps,
        new_conflicts: batch.new_conflicts,
        state: state.state,
        evidence_truncated,
    }
}

/// Record a poll the host could not complete, or refused to issue.
///
/// A refusal — an undeclared URL — names no declared stream, so it is recorded
/// against the visual and not invented as an eleventh stream.
pub fn record_poll_failure(
    visual_id: &str,
    revision: i64,
    declared: &DeclaredStreams,
    poll_url: &str,
    failure: StreamPollFailure,
) {
    let mut store = store();
    let state = entry(&mut store, visual_id, revision, declared);
    state.touch();
    state.failed_last = true;
    if let Some(stream) = declared
        .streams
        .iter()
        .find(|stream| stream.poll_url == poll_url)
        .cloned()
    {
        let entry = state.stream_mut(&stream);
        entry.poll_failures += 1;
        entry.last_failure = Some(failure);
    }
    state.recompute(declared);
}

/// The receipt for a visual, from whatever the host has observed so far.
///
/// A visual the host has never polled still gets a receipt: `observed: false`
/// with its declared streams listed, which is the difference between "ten
/// streams declared and none opened" and "no streams declared at all".
pub fn receipt(visual_id: &str, revision: i64, declared: &DeclaredStreams) -> StreamReceipt {
    let mut store = store();
    let state = entry(&mut store, visual_id, revision, declared);
    let streams: Vec<StreamReceiptStream> = declared
        .streams
        .iter()
        .map(|stream| {
            state
                .streams
                .get(&stream.poll_url)
                .map(StreamState::view)
                .unwrap_or_else(|| StreamState::new(stream).view())
        })
        .collect();
    StreamReceipt {
        schema_version: VISUAL_STREAM_RECEIPT_SCHEMA.to_string(),
        visual_id: visual_id.to_string(),
        revision,
        state: state.state,
        time_in_state_ms: state.state_since.elapsed().as_millis() as u64,
        observed: state.observed,
        ever_left_declared: state.ever_left_declared,
        declared_stream_count: declared.streams.len() as u64,
        responding_stream_count: streams
            .iter()
            .filter(|stream| stream.poll_responses > 0)
            .count() as u64,
        closed_stream_count: streams.iter().filter(|stream| stream.closed).count() as u64,
        streams_missing_transport: declared.missing_transport.clone(),
        streams,
        gaps: state.fold.gaps().to_vec(),
        conflicts: state.fold.conflicts().to_vec(),
        ready: state.fold.ready(),
        recovered: state.fold.evidence_count(),
        envelope_count: state.fold.delivered(),
        non_control_envelope_count: state.fold.delivered_non_control(),
        envelopes_by_kind: state
            .fold
            .kinds()
            .map(|(kind, count, control)| StreamKindCount {
                kind: kind.to_string(),
                count,
                control,
            })
            .collect(),
        tracking_truncated: state.fold.truncated(),
        first_observed_at: state.observed.then(|| state.first_observed_at.clone()),
        last_observed_at: state.observed.then(|| state.last_observed_at.clone()),
    }
}

// ---------------------------------------------------------------------------
// Responsibility 3: the evidence prefix the seal and the projection replay.
// ---------------------------------------------------------------------------

/// Record the envelopes one delivery of one declared stream carried.
///
/// The poll seam does not call this — it retains from the fold verdicts it
/// already has, inside the lock it already holds. This is the door for a
/// caller that has envelopes and no poll: it folds them through the same
/// per-visual fold, so the two paths cannot disagree about what a duplicate is.
///
/// `stream_id` is the renderer's stream identity — the declared `source`,
/// falling back to the poll URL — which is what [`declared_streams`] computes
/// and what the seal resolves from the same binding. The two agree by
/// construction rather than by convention.
///
/// Duplicates, replays and control envelopes are the fold's business and are
/// dropped here; only accepted evidence is retained.
pub fn record_evidence(visual_id: &str, revision: i64, stream_id: &str, envelopes: &[Value]) {
    if envelopes.is_empty() {
        return;
    }
    let mut store = store();
    let state = evidence_entry(&mut store, visual_id, revision);
    let batch = state.fold.accept_batch(envelopes.iter());
    let mut retaining = true;
    for (step, envelope) in batch.steps.iter().zip(envelopes.iter()) {
        if step.verdict != stream_fold::FoldVerdict::Evidence {
            continue;
        }
        if !retaining {
            break;
        }
        retaining = state.retain_evidence(stream_id, envelope);
    }
}

/// The evidence prefix this host observed for one declared stream.
///
/// `None` means this process has recorded no evidence for that stream at that
/// revision — which is the difference between "the stream carried nothing" and
/// "nobody ever opened this visual", and the seal's refusal says which.
///
/// Returns the retained bodies in arrival order and whether retention stopped
/// short of the run.
pub fn observed_evidence(
    visual_id: &str,
    revision: i64,
    stream_id: &str,
) -> Option<(Vec<Value>, bool)> {
    let store = store();
    let state = read(&store, visual_id, revision)?;
    let events: Vec<Value> = state
        .evidence
        .iter()
        .filter(|(stream, _)| stream == stream_id)
        .map(|(_, envelope)| envelope.clone())
        .collect();
    if events.is_empty() {
        return None;
    }
    let truncated = state
        .evidence_books
        .get(stream_id)
        .is_some_and(|book| book.truncated);
    Some((events, truncated))
}

/// Every retained envelope for a visual, in the order the host received them.
///
/// Across streams, not per stream: the renderer folds every declared stream
/// into one ingest, so a projection served from here answers in the order the
/// pane saw. `truncated` is true when any stream's retention stopped short.
pub fn observed_evidence_log(visual_id: &str, revision: i64) -> Option<(Vec<Value>, bool)> {
    let store = store();
    let state = read(&store, visual_id, revision)?;
    let truncated = state.evidence_books.values().any(|book| book.truncated);
    Some((
        state
            .evidence
            .iter()
            .map(|(_, envelope)| envelope.clone())
            .collect(),
        truncated,
    ))
}


/// The three page shapes producers emit, read the way `parseReplayPage` reads
/// them. A bare array is one closed page: the only reading that cannot silently
/// drop rows.
pub fn page_events(page: &Value) -> &[Value] {
    if let Some(rows) = page.as_array() {
        return rows;
    }
    page.pointer("/page/events")
        .or_else(|| page.get("events"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// The producer's own cursor, passed through rather than recomputed.
///
/// Every field is optional because a producer may omit it, and an omitted
/// field must reach the renderer as omitted: a cursor is never derived from a
/// sequence number here, because the multiplexed Craftax fixture sequences
/// with opaque strings and a recomputed cursor there walks a stream that does
/// not exist. The renderer's `parseReplayPage` owns the fallbacks, in one
/// place, and this hands it the same three page shapes it already reads.
#[derive(Clone, Copy, Debug, Default, Serialize, specta::Type)]
pub struct PageCursor {
    #[specta(type = Option<specta_typescript::Number>)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<i64>,
    #[specta(type = Option<specta_typescript::Number>)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub high_water: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
    /// A bare array is one closed page: the only reading that cannot silently
    /// drop rows, and the one field this reader does decide.
    pub closed: bool,
}

/// Read a page's cursor, in the three shapes producers emit.
pub fn page_cursor(page: &Value) -> PageCursor {
    if page.is_array() {
        // A bare array is one closed page with nothing after it, which is what
        // `parseReplayPage` has always made of it.
        return PageCursor {
            has_more: Some(false),
            closed: true,
            ..PageCursor::default()
        };
    }
    let number = |pointer: &str| match page.pointer(pointer) {
        Some(Value::Number(value)) => value.as_i64(),
        Some(Value::String(text)) => text.trim().parse::<i64>().ok(),
        _ => None,
    };
    PageCursor {
        next: number("/cursor/next"),
        high_water: number("/cursor/high_water"),
        has_more: page.pointer("/cursor/has_more").and_then(Value::as_bool),
        closed: page
            .pointer("/cursor/closed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

