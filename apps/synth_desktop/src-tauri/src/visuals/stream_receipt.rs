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
    #[specta(type = specta_typescript::Number)]
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
    #[specta(type = specta_typescript::Number)]
    pub first_response_latency_ms: Option<u64>,
    /// Highest numeric sequence delivered on this stream. `null` when the
    /// producer sequences with non-numeric strings, which is legitimate — the
    /// multiplexed Craftax fixture does exactly that — and is not a defect.
    #[specta(type = specta_typescript::Number)]
    pub last_sequence: Option<i64>,
    /// The producer's own cursor, passed through rather than recomputed.
    #[specta(type = specta_typescript::Number)]
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
    observed: bool,
    failed_last: bool,
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
            observed: false,
            failed_last: false,
        }
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

/// Process-global receipt store, keyed by visual id.
///
/// The same shape as the rendered-observation store next door, and for the same
/// reason: this is an observation of a running process, not a durable record,
/// and it must not survive a restart claiming a stream was seen that this
/// process never saw.
static STREAM_RECEIPTS: OnceLock<Mutex<BTreeMap<String, VisualState>>> = OnceLock::new();

/// Take the store lock, recovering the map if some other caller panicked while
/// holding it.
///
/// A poisoned lock here means an unrelated panic, not a broken map: nothing in
/// this module can leave the bookkeeping half-updated across an unwind, because
/// every mutation completes inside one call. Refusing to observe a stream
/// because of someone else's panic would turn a receipt into a second failure
/// report about itself.
fn store() -> std::sync::MutexGuard<'static, BTreeMap<String, VisualState>> {
    STREAM_RECEIPTS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn entry<'a>(
    store: &'a mut BTreeMap<String, VisualState>,
    visual_id: &str,
    revision: i64,
    declared: &DeclaredStreams,
) -> &'a mut VisualState {
    let stale = store.get(visual_id).is_some_and(|state| {
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
        // for this one.
        store.insert(visual_id.to_string(), VisualState::new(revision, declared));
    }
    store
        .entry(visual_id.to_string())
        .or_insert_with(|| VisualState::new(revision, declared))
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
    let closed = page_closed(page);
    let cursor_next = page_cursor_next(page);
    {
        let entry = state.stream_mut(&stream);
        entry.poll_responses += 1;
        entry.closed |= closed;
        if let Some(next) = cursor_next {
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
    for step in &batch.steps {
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
        }
    }

    state.recompute(declared);
    // The receipt still keeps no envelope bodies — it is identifiers and
    // counts, and that has not changed. The bodies go next door, to the
    // live-evidence store the seal reads: this is the only seam every polled
    // envelope passes through, so it is the only place a live-eval seal can
    // get replayable evidence without a caller being asked to remember to
    // attach it. A required key nobody wrote is how sealing came to be dead
    // for every live visual; nothing here has to be remembered.
    super::live_eval::record_live_evidence(visual_id, revision, &stream.stream_id, events);
    PollOutcome {
        new_gaps: batch.new_gaps,
        new_conflicts: batch.new_conflicts,
        state: state.state,
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

/// The three page shapes producers emit, read the way `parseReplayPage` reads
/// them. A bare array is one closed page: the only reading that cannot silently
/// drop rows.
fn page_events(page: &Value) -> &[Value] {
    if let Some(rows) = page.as_array() {
        return rows;
    }
    page.pointer("/page/events")
        .or_else(|| page.get("events"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn page_closed(page: &Value) -> bool {
    if page.is_array() {
        return true;
    }
    page.pointer("/cursor/closed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The producer's own cursor, passed through rather than recomputed.
///
/// A cursor is never derived from a sequence number: the multiplexed Craftax
/// fixture sequences with opaque strings, and a recomputed cursor there walks
/// a stream that does not exist.
fn page_cursor_next(page: &Value) -> Option<i64> {
    match page.pointer("/cursor/next")? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// One declared stream and a visual id nothing else in this file uses.
    ///
    /// The receipt store is process-global and these tests share it, so each
    /// test keys its own visual rather than asking production code for a reset
    /// hook it has no other reason to expose.
    fn seam(name: &str) -> (String, String, DeclaredStreams) {
        let visual_id = format!("vis.{name}");
        let poll_url = format!("https://poll.test/{name}");
        let declared = DeclaredStreams {
            streams: vec![DeclaredStream {
                stream_id: poll_url.clone(),
                poll_url: poll_url.clone(),
                sse_url: None,
            }],
            missing_transport: Vec::new(),
        };
        (visual_id, poll_url, declared)
    }

    /// Poll the way `visual_stream_poll` polls: the attempt is recorded before
    /// the request goes out, then the page it answered with.
    fn deliver(
        visual_id: &str,
        declared: &DeclaredStreams,
        poll_url: &str,
        body: Value,
    ) -> PollOutcome {
        record_poll_attempt(visual_id, 1, declared, poll_url);
        record_poll_page(visual_id, 1, declared, poll_url, &body)
    }

    fn page(events: Value) -> Value {
        json!({ "page": { "events": events } })
    }

    fn kind_row(card: &StreamReceipt, kind: &str) -> Option<(u64, bool)> {
        card.envelopes_by_kind
            .iter()
            .find(|row| row.kind == kind)
            .map(|row| (row.count, row.control))
    }

    fn gap(scope: &str, after: i64, before: i64) -> StreamGap {
        StreamGap {
            scope: scope.to_string(),
            after,
            before,
        }
    }

    // Identity, scope, the control predicate, the sequence reading and the gap
    // scan are `crate::stream_fold`, and are tested there — including against
    // every checked-in fixture through the golden suite. What is tested here is
    // what only this seam can be wrong about: which declared stream a page came
    // back on, and what the receipt then says out loud.

    // ------------------------------------------------------- the poll seam

    #[test]
    fn a_control_envelope_keeps_its_sequence_but_is_not_evidence() {
        let (visual, url, declared) = seam("control-continuity");
        let outcome = deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e1", "sequence": 1},
                {"kind": "heartbeat", "rollout_id": "roll_a", "event_id": "e2", "sequence": 2},
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e3", "sequence": 3},
            ])),
        );
        // The TypeScript ingest once skipped the control record before
        // recording its sequence, which made every sequenced heartbeat a
        // permanent phantom gap. A gap is a claim about the producer's
        // sequence space, and a heartbeat occupies that space.
        assert!(
            outcome.new_gaps.is_empty(),
            "a sequenced heartbeat is not a hole: {:?}",
            outcome.new_gaps
        );
        let card = receipt(&visual, 1, &declared);
        assert!(card.gaps.is_empty());
        assert_eq!(card.envelope_count, 3);
        assert_eq!(
            card.non_control_envelope_count, 2,
            "the heartbeat holds a sequence but is not evidence"
        );
        assert_eq!(card.recovered, 2);
        assert_eq!(kind_row(&card, "heartbeat"), Some((1, true)));
        assert_eq!(kind_row(&card, "rollout.step"), Some((2, false)));
        assert_eq!(card.streams[0].envelope_count, 3);
        assert_eq!(card.streams[0].distinct_envelope_count, 3);
        // The resolved rule (`stream_fold` rule 4): `last_sequence` is an
        // evidence high-water mark. The heartbeat at sequence 2 holds the
        // producer's numbering contiguous for the gap scan and stops there.
        assert_eq!(card.streams[0].last_sequence, Some(3));
        assert_eq!(card.state, StreamTransportState::Live);
        assert!(card.observed);
        assert!(card.ever_left_declared);
        assert!(!card.ready, "no subscription notice was delivered");
    }

    #[test]
    fn control_true_is_honoured_alongside_the_control_kinds() {
        let (visual, url, declared) = seam("control-flag");
        deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "rollout.step", "control": true, "rollout_id": "roll_a", "event_id": "e1"},
                {"kind": "ping", "rollout_id": "roll_a", "event_id": "e2"},
                {"kind": "stream.heartbeat", "rollout_id": "roll_a", "event_id": "e3"},
                {"kind": "stream.subscribed", "rollout_id": "roll_a", "event_id": "e4"},
            ])),
        );
        let card = receipt(&visual, 1, &declared);
        assert_eq!(card.envelope_count, 4);
        assert_eq!(
            card.non_control_envelope_count, 0,
            "an ordinary kind flagged `control: true` is still not evidence"
        );
        assert_eq!(card.recovered, 0);
        assert!(
            card.ready,
            "a subscription notice is what makes a stream ready"
        );
        assert_eq!(kind_row(&card, "rollout.step"), Some((1, true)));
        assert_eq!(kind_row(&card, "ping"), Some((1, true)));
        assert_eq!(kind_row(&card, "stream.heartbeat"), Some((1, true)));
        assert_eq!(kind_row(&card, "stream.subscribed"), Some((1, true)));
    }

    #[test]
    fn the_reported_control_flag_for_a_kind_is_last_write_wins() {
        // Pins current behaviour rather than a specified one: the per-kind
        // `control` flag is overwritten by each envelope of that kind, so a
        // kind delivered both ways reports whichever arrived last. The counts
        // are unaffected — only the label is.
        let (visual, url, declared) = seam("control-flag-last-write");
        deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "rollout.step", "control": true, "rollout_id": "roll_a", "event_id": "e1"},
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e2"},
            ])),
        );
        let card = receipt(&visual, 1, &declared);
        assert_eq!(kind_row(&card, "rollout.step"), Some((2, false)));
        assert_eq!(card.envelope_count, 2);
        assert_eq!(card.non_control_envelope_count, 1);
    }

    #[test]
    fn a_missing_sequence_is_one_gap_reported_once() {
        let (visual, url, declared) = seam("real-gap");
        let outcome = deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e1", "sequence": 1},
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e2", "sequence": 2},
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e4", "sequence": 4},
            ])),
        );
        assert_eq!(outcome.new_gaps, vec![gap("roll_a", 2, 4)]);
        let card = receipt(&visual, 1, &declared);
        assert_eq!(card.gaps, vec![gap("roll_a", 2, 4)]);

        // The same page again: every identity is already known, so nothing is
        // re-scanned and the caller does not file the diagnostic twice. A
        // 500 ms poll loop over a permanent hole must not emit forever.
        let again = deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e1", "sequence": 1},
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e2", "sequence": 2},
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e4", "sequence": 4},
            ])),
        );
        assert!(again.new_gaps.is_empty());
        let card = receipt(&visual, 1, &declared);
        assert_eq!(card.gaps, vec![gap("roll_a", 2, 4)]);
        assert_eq!(card.envelope_count, 6, "duplicates are delivered");
        assert_eq!(card.recovered, 3, "duplicates are not accepted");
    }

    #[test]
    fn a_late_envelope_heals_the_gap_it_fills() {
        let (visual, url, declared) = seam("gap-healing");
        let first = deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e1", "sequence": 1},
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e3", "sequence": 3},
            ])),
        );
        assert_eq!(first.new_gaps, vec![gap("roll_a", 1, 3)]);
        assert_eq!(
            receipt(&visual, 1, &declared).gaps,
            vec![gap("roll_a", 1, 3)]
        );

        let second = deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e2", "sequence": 2},
            ])),
        );
        assert!(second.new_gaps.is_empty());
        assert!(
            receipt(&visual, 1, &declared).gaps.is_empty(),
            "a hole that was filled is no longer a hole"
        );
    }

    #[test]
    fn opaque_string_sequences_produce_no_gaps_and_no_last_sequence() {
        // The real multiplexed Craftax fixture shape. There is no ordinal
        // sequence space here to have holes in, and inventing one would report
        // a defect in a healthy stream.
        let (visual, url, declared) = seam("opaque-sequences");
        let outcome = deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "rollout.step", "stream_id": "craftax:0", "sequence": "a1"},
                {"kind": "rollout.step", "stream_id": "craftax:0", "sequence": "a3"},
                {"kind": "rollout.step", "stream_id": "craftax:0", "sequence": "a7"},
            ])),
        );
        assert!(outcome.new_gaps.is_empty());
        let card = receipt(&visual, 1, &declared);
        assert!(card.gaps.is_empty());
        assert_eq!(card.streams[0].last_sequence, None);
        assert_eq!(card.envelope_count, 3);
        assert_eq!(
            card.streams[0].distinct_envelope_count, 3,
            "an opaque sequence still names the envelope for dedupe"
        );
    }

    #[test]
    fn a_null_or_empty_sequence_reads_as_absent_rather_than_zero() {
        // `Number(null) === 0` was a live phantom-gap bug in the TypeScript
        // twin: an unsequenced envelope read as sequence zero and opened a
        // gap between 0 and the first real sequence.
        let (visual, url, declared) = seam("null-sequence");
        let outcome = deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e1", "sequence": null},
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e2", "sequence": ""},
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e3"},
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e4", "sequence": 5},
            ])),
        );
        assert!(
            outcome.new_gaps.is_empty(),
            "an unsequenced envelope is not sequence zero: {:?}",
            outcome.new_gaps
        );
        let card = receipt(&visual, 1, &declared);
        assert!(card.gaps.is_empty());
        assert_eq!(card.streams[0].last_sequence, Some(5));
        assert_eq!(card.envelope_count, 4);
        assert_eq!(card.recovered, 4);
    }

    #[test]
    fn a_multiplexed_run_scans_each_lane_separately() {
        // Lane `roll_a` skips sequence 2 and lane `roll_b` supplies its own
        // sequence 2. Collapsing the lanes would read as one contiguous run
        // 1, 2, 3 and report no defect at all.
        let (visual, url, declared) = seam("multiplexed-lanes");
        let outcome = deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "1", "sequence": 1},
                {"kind": "rollout.step", "rollout_id": "roll_b", "event_id": "1", "sequence": 2},
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "2", "sequence": 3},
            ])),
        );
        assert_eq!(outcome.new_gaps, vec![gap("roll_a", 1, 3)]);
        let card = receipt(&visual, 1, &declared);
        assert_eq!(card.gaps, vec![gap("roll_a", 1, 3)]);
        assert_eq!(
            card.recovered, 3,
            "two lanes carrying `event_id: 1` are two records, not one"
        );
        // Pins current behaviour: `last_sequence` is per declared stream, so a
        // multiplexed stream reports the maximum across its lanes.
        assert_eq!(card.streams[0].last_sequence, Some(3));
    }

    #[test]
    fn a_shared_stream_id_collapses_lanes_into_one_sequence_space() {
        // Pins current behaviour rather than a specified one, and it matches
        // `envelopeScope`: a declared `stream_id` outranks `rollout_id`, so a
        // producer that multiplexes several rollouts under ONE stream id
        // shares both a sequence space and an identity namespace. Two lanes at
        // sequence 1 then name the same envelope and the second is read as a
        // duplicate. A multiplexed producer has to give each rollout its own
        // stream id, the way the Craftax fixture does; this test exists so the
        // cost of not doing so is visible rather than silent.
        let (visual, url, declared) = seam("shared-stream-id");
        deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "rollout.step", "stream_id": "craftax", "rollout_id": "roll_a", "sequence": 1},
                {"kind": "rollout.step", "stream_id": "craftax", "rollout_id": "roll_b", "sequence": 1},
            ])),
        );
        let card = receipt(&visual, 1, &declared);
        assert_eq!(card.envelope_count, 2);
        assert_eq!(
            card.recovered, 1,
            "the second lane's sequence 1 is read as a duplicate of the first"
        );
        assert_eq!(
            card.conflicts.len(),
            1,
            "their bodies differ, so the collapse is at least reported as a conflict"
        );
    }

    #[test]
    fn one_identity_with_two_bodies_is_a_conflict() {
        let (visual, url, declared) = seam("conflict");
        let outcome = deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e1", "payload": {"x": 1}},
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e1", "payload": {"x": 2}},
            ])),
        );
        assert_eq!(
            outcome.new_conflicts,
            vec![StreamConflict {
                identity: "roll_a:e1".to_string(),
                scope: "roll_a".to_string(),
                message: "Conflicting duplicate envelope roll_a:e1".to_string(),
            }]
        );
        let card = receipt(&visual, 1, &declared);
        assert_eq!(card.conflicts.len(), 1);
        assert_eq!(card.envelope_count, 2);
        assert_eq!(card.recovered, 1);
        assert_eq!(card.streams[0].distinct_envelope_count, 1);
    }

    #[test]
    fn a_producer_declared_digest_decides_equality() {
        // Two bodies, one declared digest: the producer says these are the
        // same record, and the receipt keeps the hash rather than the body.
        let (visual, url, declared) = seam("digest");
        let outcome = deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"rollout_id": "roll_a", "event_id": "e1", "digest": "d1", "payload": {"x": 1}},
                {"rollout_id": "roll_a", "event_id": "e1", "digest": "d1", "payload": {"x": 2}},
            ])),
        );
        assert!(outcome.new_conflicts.is_empty());
        assert_eq!(receipt(&visual, 1, &declared).recovered, 1);
    }

    #[test]
    fn an_envelope_with_no_identity_of_its_own_is_never_deduplicated() {
        // Pins current behaviour: the last-resort identity is the delivery
        // ordinal, so two byte-identical unidentified envelopes are two
        // records. The seam cannot tell a re-delivery from a genuine repeat,
        // and counting them as one would drop real evidence.
        let (visual, url, declared) = seam("unidentified");
        deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "tick", "rollout_id": "roll_a"},
                {"kind": "tick", "rollout_id": "roll_a"},
            ])),
        );
        let card = receipt(&visual, 1, &declared);
        assert_eq!(card.envelope_count, 2);
        assert_eq!(card.recovered, 2);
        assert!(card.conflicts.is_empty());
    }

    #[test]
    fn a_bare_array_page_is_one_closed_page() {
        let (visual, url, declared) = seam("bare-array");
        deliver(
            &visual,
            &declared,
            &url,
            json!([
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e1", "sequence": 1},
            ]),
        );
        let card = receipt(&visual, 1, &declared);
        assert_eq!(card.envelope_count, 1);
        assert!(card.streams[0].closed);
        assert_eq!(card.closed_stream_count, 1);
        assert_eq!(card.state, StreamTransportState::Terminal);
    }

    #[test]
    fn a_declared_stream_nobody_polled_is_declared_and_unobserved() {
        let (visual, _url, declared) = seam("never-polled");
        let card = receipt(&visual, 1, &declared);
        assert!(
            !card.observed,
            "a pane no reviewer rendered was not observed"
        );
        assert!(!card.ever_left_declared);
        assert_eq!(card.state, StreamTransportState::Declared);
        assert_eq!(card.declared_stream_count, 1);
        assert_eq!(card.responding_stream_count, 0);
        assert_eq!(card.envelope_count, 0);
        assert_eq!(card.first_observed_at, None);
        assert_eq!(card.schema_version, VISUAL_STREAM_RECEIPT_SCHEMA);
    }

    #[test]
    fn tracking_stops_at_its_bound_and_the_receipt_says_so() {
        let (visual, url, declared) = seam("tracking-bound");
        assert_eq!(
            RECEIPT_FOLD_LIMITS.max_identities, RECEIPT_FOLD_LIMITS.max_sequences_per_scope,
            "this test fills both bounds with a single run"
        );
        // A contiguous run exactly to the bound. Both the identity map and the
        // scope's sequence set are now full, but nothing has been dropped, so
        // the receipt is still complete.
        let full_run: Vec<Value> = (1..=RECEIPT_FOLD_LIMITS.max_sequences_per_scope as i64)
            .map(|sequence| {
                json!({
                    "kind": "rollout.step",
                    "rollout_id": "roll_a",
                    "event_id": format!("e{sequence}"),
                    "sequence": sequence,
                })
            })
            .collect();
        deliver(&visual, &declared, &url, page(Value::Array(full_run)));
        let card = receipt(&visual, 1, &declared);
        assert!(
            !card.tracking_truncated,
            "the bound itself is still complete"
        );
        assert!(card.gaps.is_empty());
        assert_eq!(
            card.envelope_count,
            RECEIPT_FOLD_LIMITS.max_sequences_per_scope as u64
        );
        assert_eq!(
            card.streams[0].distinct_envelope_count,
            RECEIPT_FOLD_LIMITS.max_sequences_per_scope as u64
        );
        assert_eq!(
            card.streams[0].last_sequence,
            Some(RECEIPT_FOLD_LIMITS.max_sequences_per_scope as i64)
        );

        // One real hole, past the bound, delivered twice. Neither the identity
        // nor the sequence is retained, so the duplicate is not recognised and
        // the hole is not scanned: gaps and conflicts are now lower bounds and
        // the distinct count is an upper bound. `trackingTruncated` is the
        // field that keeps that honest instead of reporting a clean receipt.
        deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {
                    "kind": "rollout.step",
                    "rollout_id": "roll_a",
                    "event_id": "overflow",
                    "sequence": RECEIPT_FOLD_LIMITS.max_sequences_per_scope as i64 + 2,
                },
                {
                    "kind": "rollout.step",
                    "rollout_id": "roll_a",
                    "event_id": "overflow",
                    "sequence": RECEIPT_FOLD_LIMITS.max_sequences_per_scope as i64 + 2,
                },
            ])),
        );
        let card = receipt(&visual, 1, &declared);
        assert!(card.tracking_truncated);
        assert!(
            card.gaps.is_empty(),
            "the hole past the bound is real but unscannable, so it is not claimed"
        );
        assert!(card.conflicts.is_empty());
        assert_eq!(
            card.envelope_count,
            RECEIPT_FOLD_LIMITS.max_sequences_per_scope as u64 + 2
        );
        assert_eq!(
            card.streams[0].distinct_envelope_count,
            RECEIPT_FOLD_LIMITS.max_sequences_per_scope as u64 + 2,
            "past the bound an unrecognised duplicate counts as distinct"
        );
        // The high-water mark is read before the bound check, so it still
        // advances. Pins current behaviour.
        assert_eq!(
            card.streams[0].last_sequence,
            Some(RECEIPT_FOLD_LIMITS.max_sequences_per_scope as i64 + 2)
        );
    }

    #[test]
    fn a_revision_change_replaces_the_observation() {
        let (visual, url, declared) = seam("revision-reset");
        record_poll_attempt(&visual, 1, &declared, &url);
        record_poll_page(
            &visual,
            1,
            &declared,
            &url,
            &page(json!([
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e1", "sequence": 1},
            ])),
        );
        assert_eq!(receipt(&visual, 1, &declared).envelope_count, 1);
        // A previous revision's evidence must not answer for this one.
        let next = receipt(&visual, 2, &declared);
        assert_eq!(next.envelope_count, 0);
        assert_eq!(next.revision, 2);
        assert!(!next.observed);
    }

    #[test]
    fn a_failed_poll_is_an_error_and_a_recovered_one_is_live() {
        let (visual, url, declared) = seam("failure-recovery");
        record_poll_attempt(&visual, 1, &declared, &url);
        record_poll_failure(
            &visual,
            1,
            &declared,
            &url,
            StreamPollFailure {
                code: "stream.replay.unreachable".to_string(),
                message: "connection refused".to_string(),
                status: None,
                retryable: true,
                observed_at: "2026-08-28T00:00:00Z".to_string(),
            },
        );
        let card = receipt(&visual, 1, &declared);
        assert_eq!(card.state, StreamTransportState::Error);
        assert_eq!(card.streams[0].poll_failures, 1);

        deliver(
            &visual,
            &declared,
            &url,
            page(json!([
                {"kind": "rollout.step", "rollout_id": "roll_a", "event_id": "e1", "sequence": 1},
            ])),
        );
        let card = receipt(&visual, 1, &declared);
        assert_eq!(
            card.state,
            StreamTransportState::Live,
            "a transport that recovered is live, with the failure still counted"
        );
        assert_eq!(card.streams[0].poll_failures, 1);
        assert!(card.streams[0].last_failure.is_some());
    }
}
