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
//! # Two deliberate divergences from `visuals/runtime/liveStream.ts`
//!
//! 1. **Control envelopes keep their sequence numbers here.** The TypeScript
//!    ingest skips a control envelope before recording its sequence, so a
//!    producer that sequences its heartbeats manufactures a permanent phantom
//!    gap. A gap is a claim about the producer's sequence space, and control
//!    records occupy that space, so they are counted for the gap scan and
//!    excluded only from the evidence counts.
//! 2. **`control: true` is honored.** The TS ingest classifies on `kind` alone
//!    while the projector also honors the flag. Honoring both here can only
//!    move an envelope out of the non-control count, which is the conservative
//!    direction for a receipt something else will gate on.

use super::models::canonicalize_bindings;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

/// Receipt envelope version. A reader that does not know this string should
/// refuse the receipt rather than guess at its fields.
pub const VISUAL_STREAM_RECEIPT_SCHEMA: &str = "synth.visual-stream-receipt.v1";

/// Envelope identities tracked per visual before dedupe stops being exact.
///
/// The receipt is a live, unbounded-lifetime observation of a stream that may
/// carry hundreds of thousands of envelopes, so the bookkeeping is bounded and
/// says when it stopped being complete. A truncated receipt reports lower
/// bounds; it never reports a smaller number as if it were the whole count.
const MAX_TRACKED_IDENTITIES: usize = 50_000;

/// Sequence numbers tracked per scope for the gap scan. Same reasoning.
const MAX_TRACKED_SEQUENCES: usize = 50_000;

/// Retained gaps and conflicts. Both are defects; a run that produces more than
/// this many has been answered long before the list is exhausted.
const MAX_RETAINED_DEFECTS: usize = 64;

/// Control envelope kinds, mirroring `isControlEnvelope` in `liveStream.ts`.
const CONTROL_KINDS: &[&str] = &["stream.subscribed", "heartbeat", "stream.heartbeat", "ping"];

/// The control kind that declares a subscription established.
const SUBSCRIBED_KIND: &str = "stream.subscribed";

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

/// A hole in one scope's sequence space, reported as the two envelopes that
/// bracket it rather than as a rendered sentence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StreamGap {
    /// Producer lane, as `envelopeScope` derives it: stream, rollout, or run.
    pub scope: String,
    #[specta(type = specta_typescript::Number)]
    pub after: i64,
    #[specta(type = specta_typescript::Number)]
    pub before: i64,
}

/// One envelope identity delivered twice with two different bodies.
///
/// Structured rather than the TypeScript ingest's `string[]`: the identity and
/// the lane are the parts a caller acts on, and a message that has already been
/// formatted for a human cannot be grouped, counted or matched.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StreamConflict {
    pub identity: String,
    pub scope: String,
    pub message: String,
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
    identities: HashMap<String, u64>,
    sequences: BTreeMap<String, BTreeSet<i64>>,
    gaps: Vec<StreamGap>,
    conflicts: Vec<StreamConflict>,
    kinds: BTreeMap<String, (u64, bool)>,
    envelope_count: u64,
    non_control_envelope_count: u64,
    recovered: u64,
    ready: bool,
    tracking_truncated: bool,
    /// Delivered-envelope ordinal, used only to name an envelope that carries
    /// no identity of its own — the same last resort `envelopeIdentity` takes.
    ordinal: u64,
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
            identities: HashMap::new(),
            sequences: BTreeMap::new(),
            gaps: Vec::new(),
            conflicts: Vec::new(),
            kinds: BTreeMap::new(),
            envelope_count: 0,
            non_control_envelope_count: 0,
            recovered: 0,
            ready: false,
            tracking_truncated: false,
            ordinal: 0,
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

    let mut outcome = PollOutcome::default();
    let mut touched_scopes: BTreeSet<String> = BTreeSet::new();
    for event in events {
        state.envelope_count += 1;
        state.ordinal += 1;
        let ordinal = state.ordinal;
        {
            let entry = state.stream_mut(&stream);
            entry.envelope_count += 1;
        }

        let kind = envelope_kind(event);
        let control = is_control(event, &kind);
        {
            let counted = state.kinds.entry(kind.clone()).or_insert((0, control));
            counted.0 += 1;
            counted.1 = control;
        }
        if !control {
            state.non_control_envelope_count += 1;
        }
        if kind == SUBSCRIBED_KIND {
            state.ready = true;
        }

        let scope = envelope_scope(event);
        let identity = envelope_identity(event, &scope, ordinal);
        let digest = digest_hash(event);
        let known = state.identities.get(&identity).copied();
        match known {
            Some(previous) => {
                if previous != digest && state.conflicts.len() < MAX_RETAINED_DEFECTS {
                    let conflict = StreamConflict {
                        identity: identity.clone(),
                        scope: scope.clone(),
                        message: format!("Conflicting duplicate envelope {identity}"),
                    };
                    state.conflicts.push(conflict.clone());
                    outcome.new_conflicts.push(conflict);
                }
                // A duplicate is delivered, not accepted: it never becomes
                // evidence and it never re-opens a closed sequence gap.
                continue;
            }
            None => {
                if state.identities.len() >= MAX_TRACKED_IDENTITIES {
                    state.tracking_truncated = true;
                } else {
                    state.identities.insert(identity, digest);
                }
                {
                    let entry = state.stream_mut(&stream);
                    entry.distinct_envelope_count += 1;
                }
                if !control {
                    state.recovered += 1;
                }
            }
        }

        // Control envelopes keep their sequence: see the module note. Their
        // sequence belongs to the producer's space whether or not the record
        // is evidence, and dropping it invents a gap that never happened.
        let Some(sequence) = numeric_sequence(event) else {
            continue;
        };
        {
            let entry = state.stream_mut(&stream);
            entry.last_sequence = Some(
                entry
                    .last_sequence
                    .map_or(sequence, |last| last.max(sequence)),
            );
        }
        let full = state
            .sequences
            .get(&scope)
            .is_some_and(|observed| observed.len() >= MAX_TRACKED_SEQUENCES);
        if full {
            state.tracking_truncated = true;
            continue;
        }
        state
            .sequences
            .entry(scope.clone())
            .or_default()
            .insert(sequence);
        touched_scopes.insert(scope);
    }

    for scope in touched_scopes {
        let rescanned = scan_gaps(&scope, state.sequences.get(&scope));
        let previously: BTreeSet<(i64, i64)> = state
            .gaps
            .iter()
            .filter(|gap| gap.scope == scope)
            .map(|gap| (gap.after, gap.before))
            .collect();
        for gap in &rescanned {
            if !previously.contains(&(gap.after, gap.before)) {
                outcome.new_gaps.push(gap.clone());
            }
        }
        state.gaps.retain(|gap| gap.scope != scope);
        state.gaps.extend(rescanned);
        if state.gaps.len() > MAX_RETAINED_DEFECTS {
            state.gaps.truncate(MAX_RETAINED_DEFECTS);
            state.tracking_truncated = true;
        }
    }

    state.recompute(declared);
    outcome.state = state.state;
    outcome
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
        gaps: state.gaps.clone(),
        conflicts: state.conflicts.clone(),
        ready: state.ready,
        recovered: state.recovered,
        envelope_count: state.envelope_count,
        non_control_envelope_count: state.non_control_envelope_count,
        envelopes_by_kind: state
            .kinds
            .iter()
            .map(|(kind, (count, control))| StreamKindCount {
                kind: kind.clone(),
                count: *count,
                control: *control,
            })
            .collect(),
        tracking_truncated: state.tracking_truncated,
        first_observed_at: state.observed.then(|| state.first_observed_at.clone()),
        last_observed_at: state.observed.then(|| state.last_observed_at.clone()),
    }
}

/// Scan one scope's observed sequences for holes.
fn scan_gaps(scope: &str, observed: Option<&BTreeSet<i64>>) -> Vec<StreamGap> {
    let Some(observed) = observed else {
        return Vec::new();
    };
    let mut gaps = Vec::new();
    let mut previous: Option<i64> = None;
    for sequence in observed {
        if let Some(last) = previous {
            if *sequence > last.saturating_add(1) {
                gaps.push(StreamGap {
                    scope: scope.to_string(),
                    after: last,
                    before: *sequence,
                });
            }
        }
        previous = Some(*sequence);
    }
    gaps
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

fn page_cursor_next(page: &Value) -> Option<i64> {
    page.pointer("/cursor/next").and_then(as_integer)
}

fn envelope_kind(event: &Value) -> String {
    non_empty(event.get("kind"))
        .or_else(|| non_empty(event.get("type")))
        .unwrap_or_default()
}

fn is_control(event: &Value, kind: &str) -> bool {
    if event.get("control").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    CONTROL_KINDS.contains(&kind)
}

/// Producers may carry transport identity in the envelope payload, so the
/// declared identity is promoted here exactly as `envelopeScope` promotes it.
fn envelope_scope(event: &Value) -> String {
    stream_id(event)
        .or_else(|| non_empty(event.get("rollout_id")))
        .or_else(|| payload_string(event, &["rollout_id"]))
        .or_else(|| non_empty(event.get("lane")))
        .or_else(|| payload_string(event, &["lane"]))
        .or_else(|| non_empty(event.get("run_id")))
        .or_else(|| payload_string(event, &["run_id"]))
        .unwrap_or_else(|| "run".to_string())
}

/// Sequence and `event_id` are monotonic only within a rollout. A multiplexed
/// run legitimately contains ten `event_id: "1"` records, so identity keeps the
/// producer lane: treating `event_id` as globally unique drops all but one lane
/// while still making the aggregate lane count look valid.
fn envelope_identity(event: &Value, scope: &str, ordinal: u64) -> String {
    let sequence = sequence_label(event);
    if let (Some(stream), Some(sequence)) = (stream_id(event), sequence.as_deref()) {
        if !sequence.is_empty() {
            return format!("{stream}:{sequence}");
        }
    }
    if let Some(event_id) = non_empty(event.get("event_id")) {
        return format!("{scope}:{event_id}");
    }
    if let Some(sequence) = sequence.as_deref().filter(|value| !value.is_empty()) {
        return format!("{scope}:{sequence}");
    }
    let kind = non_empty(event.get("kind"))
        .or_else(|| non_empty(event.get("type")))
        .unwrap_or_else(|| "event".to_string());
    let stamp = non_empty(event.get("occurred_at"))
        .or_else(|| non_empty(event.get("ts")))
        .unwrap_or_else(|| ordinal.to_string());
    format!("{scope}:{kind}:{stamp}")
}

fn stream_id(event: &Value) -> Option<String> {
    non_empty(event.get("stream_id")).or_else(|| payload_string(event, &["stream_id", "stream.id"]))
}

/// `sequence_number ?? sequence`, with an explicit JSON `null` treated as
/// absent the way the renderer's nullish coalescing treats it.
fn raw_sequence(event: &Value) -> Option<&Value> {
    for key in ["sequence_number", "sequence"] {
        match event.get(key) {
            Some(value) if !value.is_null() => return Some(value),
            _ => {}
        }
    }
    None
}

fn sequence_label(event: &Value) -> Option<String> {
    raw_sequence(event).map(|value| match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    })
}

/// The sequence as a number, or nothing.
///
/// A producer may sequence with opaque strings — the multiplexed Craftax
/// fixture does — and those lanes are simply not gap-scannable. Coercing them
/// to an ordinal would invent a sequence space and then report holes in it.
fn numeric_sequence(event: &Value) -> Option<i64> {
    raw_sequence(event).and_then(as_integer)
}

fn as_integer(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_f64()
                .filter(|f| f.fract() == 0.0)
                .map(|f| f as i64)
        }),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn payload_string(event: &Value, keys: &[&str]) -> Option<String> {
    let payload = event.get("payload")?;
    keys.iter().find_map(|key| non_empty(payload.get(*key)))
}

fn non_empty(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// The producer's own digest when it declares one, the body otherwise.
///
/// Only equality matters: this decides whether the same identity arrived twice
/// with two different bodies. Envelope bodies carry model output and rollout
/// payloads, so the receipt keeps the hash and never the body.
fn digest_hash(event: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    match event.get("digest").and_then(Value::as_str) {
        Some(digest) => digest.hash(&mut hasher),
        None => serde_json::to_string(event)
            .unwrap_or_default()
            .hash(&mut hasher),
    }
    hasher.finish()
}
