//! The one fold: identity, scope, dedupe, conflict, gap scan, projection.
//!
//! Everything that reads a producer's ordered journal — the live-eval replay
//! seam, the live spool, the receipt, the optimizer adapters, the Intern
//! ingestion loop — asks the same questions of it: *have I seen this record,
//! is this the record I expected next, and does the history I hold have a hole
//! in it?* Those questions were answered in seven places, in three languages,
//! with rules that had already drifted: the spool treated a bare `event_id` as
//! globally unique and lane-collapsed every multiplexed run, while the
//! renderer's ingest had a comment warning about exactly that bug.
//!
//! This module is the answer. It is deliberately not a visuals module and not
//! an optimizer module: the two families ask the same questions of different
//! journals, and a boundary that only half the callers can reach is how the
//! second implementation gets written.
//!
//! # Two journal shapes, one set of rules
//!
//! * **Envelope streams** (live-eval): identity is producer-declared and
//!   rollout-local, sequences may be opaque strings, and history is folded by
//!   dedupe rather than by cursor arithmetic. [`LiveFold`].
//! * **Cursor journals** (optimizer runs, Intern sessions): a dense `u64`
//!   sequence per run, where the only questions are replay, next, and hole.
//!   [`sequence_step`].
//!
//! # The rules, and why each one is the way it is
//!
//! 1. **Identity keeps the producer lane.** `sequence` and `event_id` are
//!    monotonic only within a rollout, so a multiplexed run legitimately
//!    carries ten `event_id: "1"` records. `streamId:sequence` first, then
//!    `scope:event_id`, then `scope:sequence`, then kind and stamp. Treating a
//!    bare `event_id` as globally unique drops all but one lane while leaving
//!    the aggregate lane count looking valid.
//! 2. **Control envelopes keep their sequence numbers.** A gap is a claim
//!    about the *producer's* sequence space and control records occupy that
//!    space, so skipping one before recording its sequence manufactures a
//!    permanent phantom gap for any producer that sequences its heartbeats.
//! 3. **`control: true` is honored** alongside the control kinds, so the fold
//!    and the projector cannot disagree about what counts as evidence.
//! 4. **The evidence high-water mark is evidence-only.** Rule 2 admits control
//!    records to the *gap scan*; [`LiveFold::last_sequence`] excludes them.
//!    The two answer different questions, and the divergence recorded in
//!    `visuals/stream_receipt.rs` — per-stream and control-advanced there,
//!    per-scope and evidence-only in TypeScript — is resolved here in favour
//!    of evidence-only. A heartbeat that advances the high-water mark lets a
//!    stream carrying nothing but heartbeats report progress it never made,
//!    which is precisely the failure the receipt exists to expose.
//! 5. **An absent sequence is absent, never zero.** `Number(null)` is `0` and
//!    `Number("")` is `0`; reading either as sequence zero invents a hole
//!    before sequence one.
//! 6. **Only integral sequences are gap-scannable.** A producer may sequence
//!    with opaque strings — the multiplexed Craftax fixture does — and those
//!    lanes are simply not scannable. The same holds for a fractional
//!    sequence: it has no successor, so "the number missing after it" is not a
//!    claim anyone can make. Coercing either would invent a sequence space and
//!    then report holes in it.
//!
//! # The TypeScript mirror
//!
//! Browser preview, fixture replay and the two shipped shells run with no Rust
//! underneath them and still have to draw something, so `visuals/runtime/`
//! keeps a mirror of *identity, dedupe, the control predicate and the
//! projection* — the parts a renderer cannot do without. It keeps no gap scan
//! and no conflict ledger: those are evidence accounting, they are read by the
//! readiness gate and by agents, and a second implementation of them is a
//! second answer to a question that must have one.
//!
//! The mirror is pinned to this module by a golden capture over every
//! checked-in fixture — `visuals/fixtures/live_fold_golden.json`, regenerated
//! by `visuals/tests/live_fold_golden_gen.mjs` — asserted from both sides. A
//! mirror is honest exactly as long as something checks it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

// ===========================================================================
// Cursor journals: replay, next, hole.
// ===========================================================================

/// Where one sequence falls relative to a cursor over a dense journal.
///
/// Every caller that folds an optimizer or Intern journal asks this and only
/// this. Naming the four answers once means a caller chooses a *policy* —
/// skip a replay, or refuse it — instead of re-deriving the arithmetic and
/// getting `<=` where it meant `<`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequenceStep {
    /// The sequence the cursor already stands on. Callers that carry an event
    /// id can tell an honest retransmission from a collision here.
    Duplicate,
    /// Behind the cursor: already folded, and folding it again would double
    /// count.
    Replay,
    /// Exactly one past the cursor: the record this fold was waiting for.
    Next,
    /// Past the cursor with numbers missing in between. A gap means the
    /// producer's journal and this cursor disagree about history, and a viewer
    /// folded from a gapped stream shows a trajectory that never happened.
    Gap {
        /// The sequence the fold expected instead.
        expected: u64,
    },
}

/// Classify one sequence against a cursor. See [`SequenceStep`].
pub fn sequence_step(cursor: u64, sequence: u64) -> SequenceStep {
    if sequence == cursor {
        SequenceStep::Duplicate
    } else if sequence < cursor {
        SequenceStep::Replay
    } else if sequence == cursor.saturating_add(1) {
        SequenceStep::Next
    } else {
        SequenceStep::Gap {
            expected: cursor.saturating_add(1),
        }
    }
}

/// True when `sequence` is the next contiguous record after `cursor`.
///
/// The shape for callers that treat anything else — replay included — as a
/// contract violation rather than as something to skip.
pub fn is_next_sequence(cursor: u64, sequence: u64) -> bool {
    matches!(sequence_step(cursor, sequence), SequenceStep::Next)
}

/// Advance `cursor` if `sequence` is ahead of it, reporting whether it moved.
///
/// The looser policy some producers require: a page whose numbering is
/// monotonic but not dense, where a skipped number is the producer's business
/// and not evidence of loss. Returns false without moving the cursor when the
/// record is at or behind it.
pub fn accept_if_ahead(cursor: &mut u64, sequence: u64) -> bool {
    if sequence <= *cursor {
        return false;
    }
    *cursor = sequence;
    true
}

/// Whether a page's declared next cursor agrees with what the fold committed.
///
/// A producer that hands back a cursor the committed page does not reach has
/// either dropped records or renumbered them; either way the fold's history is
/// not the producer's history.
pub fn cursor_reconciles(committed: u64, page_next: u64) -> bool {
    committed == page_next
}

// ===========================================================================
// Envelope streams: reading one envelope.
// ===========================================================================

/// Control envelope kinds. Transport bookkeeping, never evidence.
pub const CONTROL_KINDS: &[&str] = &["stream.subscribed", "heartbeat", "stream.heartbeat", "ping"];

/// The control kind that declares a subscription established.
pub const SUBSCRIBED_KIND: &str = "stream.subscribed";

/// Blob names a projection may never carry off the fold.
const FORBIDDEN_BLOBS: &[&str] = &["collector", "capability_blob", "capabilities_blob"];

/// A scalar JSON value as the string a template literal would produce, with
/// `null` and absent both reading as absent.
///
/// The renderer reaches these fields through `??`, which skips only null and
/// undefined — so an empty string is a value, and a number is a stamp. Objects
/// and arrays are not scalars and read as absent rather than as
/// `[object Object]`.
fn scalar(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => Some(number_string(number)),
        _ => None,
    }
}

/// A JSON number as JavaScript would print it: integral values without a
/// trailing `.0`, everything else in its shortest round-tripping form.
fn number_string(number: &serde_json::Number) -> String {
    if let Some(value) = number.as_i64() {
        return value.to_string();
    }
    if let Some(value) = number.as_u64() {
        return value.to_string();
    }
    match number.as_f64() {
        Some(value) if value.fract() == 0.0 && value.abs() < 9.0e15 => (value as i64).to_string(),
        Some(value) => value.to_string(),
        None => number.to_string(),
    }
}

/// A non-empty string field, the way the renderer's `||` chain reads one.
fn non_empty(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn payload_string(event: &Value, keys: &[&str]) -> Option<String> {
    let payload = event.get("payload")?;
    keys.iter().find_map(|key| non_empty(payload.get(*key)))
}

/// The envelope's kind: `kind`, else `type`, else empty.
pub fn envelope_kind(event: &Value) -> String {
    scalar(event.get("kind"))
        .or_else(|| scalar(event.get("type")))
        .unwrap_or_default()
}

/// Whether this envelope is transport bookkeeping rather than evidence.
///
/// The single definition of "control" for the whole pipeline. An explicit
/// `control: true` flag counts, not just a known control kind: the projector
/// already honoured the flag while the fold checked kind only, so an envelope
/// flagged `control: true` under an ordinary kind was evidence to one and not
/// the other.
pub fn is_control(event: &Value) -> bool {
    is_control_kind(event, &envelope_kind(event))
}

/// [`is_control`] for a caller that has already read the kind.
pub fn is_control_kind(event: &Value, kind: &str) -> bool {
    if event.get("control").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    CONTROL_KINDS.contains(&kind)
}

/// The producer's declared stream id, from the envelope or its payload.
pub fn stream_id(event: &Value) -> Option<String> {
    non_empty(event.get("stream_id")).or_else(|| payload_string(event, &["stream_id", "stream.id"]))
}

/// The producer lane an envelope belongs to: stream, rollout, lane, or run.
///
/// Producers may carry transport identity in the payload, so the declared
/// identity is promoted at the ingestion boundary — every viewer then gets the
/// same rollout-local dedupe without knowing a producer's wire shape.
pub fn envelope_scope(event: &Value) -> String {
    stream_id(event)
        .or_else(|| non_empty(event.get("rollout_id")))
        .or_else(|| payload_string(event, &["rollout_id"]))
        .or_else(|| non_empty(event.get("lane")))
        .or_else(|| payload_string(event, &["lane"]))
        .or_else(|| non_empty(event.get("run_id")))
        .or_else(|| payload_string(event, &["run_id"]))
        .unwrap_or_else(|| "run".to_string())
}

/// The stream an envelope was delivered on, for the cutoff cursor vector.
///
/// The declared stream when the producer names one, the lane otherwise. A
/// cutoff addresses arrival order *within a stream*, which is the one total
/// order that exists whatever a producer does with its sequence numbers.
pub fn envelope_stream(event: &Value) -> String {
    stream_id(event).unwrap_or_else(|| envelope_scope(event))
}

/// `sequence_number ?? sequence`, with an explicit `null` read as absent.
fn raw_sequence(event: &Value) -> Option<&Value> {
    for key in ["sequence_number", "sequence"] {
        match event.get(key) {
            Some(value) if !value.is_null() => return Some(value),
            _ => {}
        }
    }
    None
}

/// The sequence as the string that names it in an identity, if it has one.
pub fn sequence_label(event: &Value) -> Option<String> {
    scalar(raw_sequence(event)).filter(|label| !label.is_empty())
}

/// The sequence as a gap-scannable integer, or nothing.
///
/// See rule 6: opaque strings and fractional numbers are legitimate producer
/// choices that simply carry no scannable sequence space.
pub fn numeric_sequence(event: &Value) -> Option<i64> {
    match raw_sequence(event)? {
        Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_f64()
                .filter(|value| value.fract() == 0.0)
                .map(|value| value as i64)
        }),
        Value::String(text) if !text.is_empty() => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// The envelope's identity: what makes two deliveries the same record.
///
/// `ordinal` is the delivered-envelope ordinal, one-based, and is consulted
/// only for an envelope carrying no identity of its own at all — no stream,
/// no event id, no sequence and no timestamp.
pub fn envelope_identity(event: &Value, scope: &str, ordinal: u64) -> String {
    let sequence = sequence_label(event);
    if let (Some(stream), Some(sequence)) = (stream_id(event), sequence.as_deref()) {
        return format!("{stream}:{sequence}");
    }
    if let Some(event_id) = non_empty(event.get("event_id")) {
        return format!("{scope}:{event_id}");
    }
    if let Some(sequence) = sequence.as_deref() {
        return format!("{scope}:{sequence}");
    }
    let kind = scalar(event.get("kind"))
        .or_else(|| scalar(event.get("type")))
        .unwrap_or_else(|| "event".to_string());
    let stamp = scalar(event.get("occurred_at"))
        .or_else(|| scalar(event.get("ts")))
        .unwrap_or_else(|| ordinal.to_string());
    format!("{scope}:{kind}:{stamp}")
}

/// Promote payload-carried identity onto the envelope itself.
///
/// A viewer that only reads top-level fields still sees the lane the producer
/// declared in its payload, so the same envelope projects the same way
/// wherever it is read.
pub fn normalize_identity(event: &Value) -> Value {
    let rollout_id =
        non_empty(event.get("rollout_id")).or_else(|| payload_string(event, &["rollout_id"]));
    let lane = non_empty(event.get("lane"))
        .or_else(|| payload_string(event, &["lane"]))
        .or_else(|| rollout_id.clone());
    let run_id = non_empty(event.get("run_id")).or_else(|| payload_string(event, &["run_id"]));
    let stream = stream_id(event);
    if rollout_id.is_none() && lane.is_none() && run_id.is_none() && stream.is_none() {
        return event.clone();
    }
    let mut normalized = event.clone();
    let Some(object) = normalized.as_object_mut() else {
        return normalized;
    };
    for (key, value) in [
        ("rollout_id", rollout_id),
        ("lane", lane),
        ("run_id", run_id),
        ("stream_id", stream),
    ] {
        if let Some(value) = value {
            object.insert(key.to_string(), Value::String(value));
        }
    }
    normalized
}

/// The producer's own digest when it declares one, the body otherwise.
///
/// Only equality matters: this decides whether one identity arrived twice with
/// two different bodies. Envelope bodies carry model output and rollout
/// payloads, so the fold keeps the hash and never the body.
pub fn digest_hash(event: &Value) -> u64 {
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

// ===========================================================================
// Gaps and conflicts.
// ===========================================================================

/// A hole in one scope's sequence space, reported as the two envelopes that
/// bracket it rather than as a rendered sentence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGap {
    /// Producer lane, as [`envelope_scope`] derives it.
    pub scope: String,
    #[specta(type = specta_typescript::Number)]
    pub after: i64,
    #[specta(type = specta_typescript::Number)]
    pub before: i64,
}

/// One envelope identity delivered twice with two different bodies.
///
/// Structured rather than a formatted string: the identity and the lane are
/// the parts a caller acts on, and a message already formatted for a human
/// cannot be grouped, counted or matched.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvelopeConflict {
    pub identity: String,
    pub scope: String,
    pub message: String,
}

impl EnvelopeConflict {
    fn new(identity: &str, scope: &str) -> Self {
        Self {
            identity: identity.to_string(),
            scope: scope.to_string(),
            message: format!("Conflicting duplicate envelope {identity}"),
        }
    }
}

/// Scan one scope's observed sequences for holes.
pub fn scan_gaps(scope: &str, observed: &BTreeSet<i64>) -> Vec<SequenceGap> {
    let mut gaps = Vec::new();
    let mut previous: Option<i64> = None;
    for sequence in observed {
        if let Some(last) = previous {
            if *sequence > last.saturating_add(1) {
                gaps.push(SequenceGap {
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

// ===========================================================================
// The envelope fold.
// ===========================================================================

/// Bounds on a fold's bookkeeping.
///
/// A live stream has an unbounded lifetime and may carry hundreds of thousands
/// of envelopes, so the bookkeeping is bounded and says when it stopped being
/// complete. A truncated fold reports lower bounds; it never reports a smaller
/// number as if it were the whole count.
#[derive(Clone, Copy, Debug)]
pub struct FoldLimits {
    pub max_identities: usize,
    pub max_sequences_per_scope: usize,
    pub max_defects: usize,
    /// Whether accepted evidence bodies are retained for projection.
    ///
    /// Off for every live caller, deliberately, and the decision is not
    /// "projections are not worth the memory" — it is that this retention has
    /// no byte bound and no ceiling on `events`, so a hundred-thousand-envelope
    /// run would hold every body in a process-global for as long as the process
    /// lives. The live seam needs a projection and does not need this: the host
    /// already retains a *bounded* evidence prefix per stream so a live-eval
    /// visual can be sealed at all, and a projection folded from that prefix
    /// costs a read of memory that is spent either way. See
    /// `visuals/stream_receipt.rs`, which owns that bound and reports when it
    /// is reached.
    ///
    /// So this stays what it is: the affordance for a caller folding a
    /// *finite* log it already holds — a fixture, a closed rollout, a test.
    /// Turning it on for a live stream would be a second, unbounded copy of
    /// evidence the host is already keeping under a bound.
    pub retain_events: bool,
}

impl Default for FoldLimits {
    fn default() -> Self {
        Self {
            max_identities: 50_000,
            max_sequences_per_scope: 50_000,
            max_defects: 64,
            retain_events: false,
        }
    }
}

impl FoldLimits {
    /// Limits for a fold whose evidence will be projected.
    pub fn retaining() -> Self {
        Self {
            retain_events: true,
            ..Self::default()
        }
    }
}

/// What the fold decided about one delivered envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldVerdict {
    /// A new, non-control envelope: evidence.
    Evidence,
    /// A new control envelope: bookkeeping, not evidence.
    Control,
    /// An identity already folded, with the same body.
    Duplicate,
    /// An identity already folded, with a different body.
    Conflict,
}

impl FoldVerdict {
    /// Whether this delivery was folded rather than dropped as a repeat.
    pub fn accepted(self) -> bool {
        matches!(self, Self::Evidence | Self::Control)
    }
}

/// The fold's reading of one delivered envelope, for a caller keeping its own
/// per-stream books beside it.
#[derive(Clone, Debug)]
pub struct FoldStep {
    pub identity: String,
    pub scope: String,
    pub stream: String,
    pub kind: String,
    pub control: bool,
    /// The gap-scannable sequence, when the producer has one.
    pub sequence: Option<i64>,
    pub verdict: FoldVerdict,
}

/// What a batch changed, for the caller that has to report it.
#[derive(Clone, Debug, Default)]
pub struct FoldBatch {
    pub steps: Vec<FoldStep>,
    /// Gaps observed for the first time by this batch. A caller emits these
    /// once per gap rather than once per poll: a 500 ms loop over a permanent
    /// hole would otherwise file the same diagnostic twice a second forever.
    pub new_gaps: Vec<SequenceGap>,
    pub new_conflicts: Vec<EnvelopeConflict>,
}

/// A live envelope stream, folded.
///
/// Stateful across deliveries: the dedupe set, the per-scope sequence space,
/// the conflict ledger and the evidence high-water marks all persist, so a
/// caller polls repeatedly and the fold answers as if it had seen one log.
#[derive(Debug)]
pub struct LiveFold {
    limits: FoldLimits,
    identities: HashMap<String, u64>,
    sequences: BTreeMap<String, BTreeSet<i64>>,
    last_sequence_by_scope: BTreeMap<String, i64>,
    gaps: Vec<SequenceGap>,
    conflicts: Vec<EnvelopeConflict>,
    kinds: BTreeMap<String, (u64, bool)>,
    stream_evidence: BTreeMap<String, u64>,
    events: Vec<Value>,
    delivered: u64,
    delivered_non_control: u64,
    distinct: u64,
    evidence: u64,
    ready: bool,
    truncated: bool,
    ordinal: u64,
}

impl Default for LiveFold {
    fn default() -> Self {
        Self::new(FoldLimits::default())
    }
}

impl LiveFold {
    pub fn new(limits: FoldLimits) -> Self {
        Self {
            limits,
            identities: HashMap::new(),
            sequences: BTreeMap::new(),
            last_sequence_by_scope: BTreeMap::new(),
            gaps: Vec::new(),
            conflicts: Vec::new(),
            kinds: BTreeMap::new(),
            stream_evidence: BTreeMap::new(),
            events: Vec::new(),
            delivered: 0,
            delivered_non_control: 0,
            distinct: 0,
            evidence: 0,
            ready: false,
            truncated: false,
            ordinal: 0,
        }
    }

    /// A fold that keeps evidence bodies, for a caller that will project them.
    pub fn retaining() -> Self {
        Self::new(FoldLimits::retaining())
    }

    /// Fold a batch of delivered envelopes.
    ///
    /// Gaps are rescanned once per touched scope at the end of the batch
    /// rather than per envelope: a live transport can deliver thousands of
    /// messages in one task, and rescanning per message made a 100k-envelope
    /// run quadratic.
    pub fn accept_batch<'a>(&mut self, events: impl IntoIterator<Item = &'a Value>) -> FoldBatch {
        let mut batch = FoldBatch::default();
        let mut touched: BTreeSet<String> = BTreeSet::new();
        for event in events {
            let step = self.accept_one(event, &mut batch, &mut touched);
            batch.steps.push(step);
        }
        for scope in touched {
            let observed = self.sequences.get(&scope).cloned().unwrap_or_default();
            let rescanned = scan_gaps(&scope, &observed);
            let known: BTreeSet<(i64, i64)> = self
                .gaps
                .iter()
                .filter(|gap| gap.scope == scope)
                .map(|gap| (gap.after, gap.before))
                .collect();
            for gap in &rescanned {
                if !known.contains(&(gap.after, gap.before)) {
                    batch.new_gaps.push(gap.clone());
                }
            }
            self.gaps.retain(|gap| gap.scope != scope);
            self.gaps.extend(rescanned);
            if self.gaps.len() > self.limits.max_defects {
                self.gaps.truncate(self.limits.max_defects);
                self.truncated = true;
            }
        }
        batch
    }

    /// Fold one delivered envelope.
    pub fn accept(&mut self, event: &Value) -> FoldStep {
        let mut batch = self.accept_batch(std::iter::once(event));
        batch
            .steps
            .pop()
            .expect("accept_batch yields one step per envelope")
    }

    fn accept_one(
        &mut self,
        event: &Value,
        batch: &mut FoldBatch,
        touched: &mut BTreeSet<String>,
    ) -> FoldStep {
        self.delivered += 1;
        self.ordinal += 1;
        let ordinal = self.ordinal;

        let kind = envelope_kind(event);
        let control = is_control_kind(event, &kind);
        let counted = self.kinds.entry(kind.clone()).or_insert((0, control));
        counted.0 += 1;
        counted.1 = control;
        if !control {
            self.delivered_non_control += 1;
        }
        if kind == SUBSCRIBED_KIND {
            self.ready = true;
        }

        let scope = envelope_scope(event);
        let stream = envelope_stream(event);
        let identity = envelope_identity(event, &scope, ordinal);
        let digest = digest_hash(event);
        let sequence = numeric_sequence(event);

        if let Some(previous) = self.identities.get(&identity).copied() {
            let verdict = if previous == digest {
                FoldVerdict::Duplicate
            } else {
                if self.conflicts.len() < self.limits.max_defects {
                    let conflict = EnvelopeConflict::new(&identity, &scope);
                    self.conflicts.push(conflict.clone());
                    batch.new_conflicts.push(conflict);
                } else {
                    self.truncated = true;
                }
                FoldVerdict::Conflict
            };
            // A duplicate is delivered, not accepted: it never becomes
            // evidence and it never re-opens a closed sequence gap.
            return FoldStep {
                identity,
                scope,
                stream,
                kind,
                control,
                sequence,
                verdict,
            };
        }

        if self.identities.len() >= self.limits.max_identities {
            self.truncated = true;
        } else {
            self.identities.insert(identity.clone(), digest);
        }
        self.distinct += 1;
        if !control {
            self.evidence += 1;
            *self.stream_evidence.entry(stream.clone()).or_insert(0) += 1;
            if self.limits.retain_events {
                self.events.push(normalize_identity(event));
            }
        }

        // Rule 2: a control envelope keeps its sequence. Rule 4: it does not
        // advance the evidence high-water mark.
        if let Some(sequence) = sequence {
            let observed = self.sequences.entry(scope.clone()).or_default();
            if observed.len() >= self.limits.max_sequences_per_scope {
                self.truncated = true;
            } else {
                observed.insert(sequence);
                touched.insert(scope.clone());
            }
            if !control {
                let last = self
                    .last_sequence_by_scope
                    .entry(scope.clone())
                    .or_insert(sequence);
                *last = (*last).max(sequence);
            }
        }

        FoldStep {
            identity,
            scope,
            stream,
            kind,
            control,
            sequence,
            verdict: if control {
                FoldVerdict::Control
            } else {
                FoldVerdict::Evidence
            },
        }
    }

    pub fn gaps(&self) -> &[SequenceGap] {
        &self.gaps
    }

    pub fn conflicts(&self) -> &[EnvelopeConflict] {
        &self.conflicts
    }

    /// Accepted evidence bodies, in arrival order. Empty unless the fold was
    /// built with [`FoldLimits::retaining`].
    pub fn events(&self) -> &[Value] {
        &self.events
    }

    /// Envelopes delivered, duplicates included: what the transport handed
    /// over, before the fold has an opinion about it.
    pub fn delivered(&self) -> u64 {
        self.delivered
    }

    /// Delivered envelopes that are not heartbeats, pings or subscription
    /// notices. A stream can be healthy on every other count and still have
    /// carried no evidence; this is the number that says so.
    pub fn delivered_non_control(&self) -> u64 {
        self.delivered_non_control
    }

    /// Envelopes with a distinct identity: what the fold kept.
    pub fn distinct(&self) -> u64 {
        self.distinct
    }

    /// Distinct non-control envelopes: the evidence a projection works from.
    pub fn evidence_count(&self) -> u64 {
        self.evidence
    }

    /// A `stream.subscribed` control envelope was delivered.
    pub fn ready(&self) -> bool {
        self.ready
    }

    /// Set once bookkeeping hit its bound. Dedupe, gaps and conflicts become
    /// lower bounds from that point; the delivered counts do not.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Envelopes delivered under each kind, with whether that kind is control.
    pub fn kinds(&self) -> impl Iterator<Item = (&str, u64, bool)> + '_ {
        self.kinds
            .iter()
            .map(|(kind, (count, control))| (kind.as_str(), *count, *control))
    }

    /// The highest sequence *evidence* reached in one scope. See rule 4.
    pub fn last_sequence(&self, scope: &str) -> Option<i64> {
        self.last_sequence_by_scope.get(scope).copied()
    }

    pub fn last_sequence_by_scope(&self) -> &BTreeMap<String, i64> {
        &self.last_sequence_by_scope
    }

    /// The cutoff addressing everything folded so far.
    pub fn cursor(&self) -> CursorVector {
        CursorVector(self.stream_evidence.clone())
    }
}

// ===========================================================================
// Cutoff: a per-stream cursor vector.
// ===========================================================================

/// A logical cutoff into a folded stream set: how many evidence envelopes of
/// each stream to include.
///
/// Not a sequence. Verification killed both simpler candidates on the real
/// multiplexed fixture (`live.craftax.v1/examples/cua-luna-low-10.json`, one
/// stream and ten lanes): `sequence` there is a non-numeric string
/// (`"suites/…#s0:<uuid>:frame:0"`), so a scalar numeric cutoff is a no-op and
/// a per-scope numeric vector cannot address the events either. The one
/// durable total order that always exists is arrival order within a stream —
/// persisted verbatim by the spool and preserved by the fold — so a cutoff is
/// a prefix length per stream.
///
/// Streams absent from the vector contribute nothing: a cutoff names what is
/// included, so an unnamed stream is excluded rather than silently whole.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CursorVector(pub BTreeMap<String, u64>);

impl CursorVector {
    pub fn new(counts: impl IntoIterator<Item = (String, u64)>) -> Self {
        Self(counts.into_iter().collect())
    }

    pub fn get(&self, stream: &str) -> u64 {
        self.0.get(stream).copied().unwrap_or(0)
    }

    /// Total envelopes addressed. The filmstrip orders snapshots by this,
    /// breaking ties on stream id.
    pub fn total(&self) -> u64 {
        self.0.values().sum()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ===========================================================================
// The live-eval projection.
// ===========================================================================

/// What a live-eval template renders: literal values, never raw envelopes to
/// be re-folded downstream.
///
/// `events` is the folded evidence prefix and stays available beside the
/// derived fields on purpose. A sourced visual may aggregate an eval in a way
/// nobody anticipated, and making a novel aggregation require a Rust change
/// would spend expressiveness — already this system's weak axis against
/// general codegen — on tidiness.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveEvalProjection {
    pub events: Vec<Value>,
    pub kinds: Vec<String>,
    pub has_live_frames: bool,
    pub has_reward_txt: bool,
    pub reward: Option<f64>,
    pub usage: Option<LiveEvalUsage>,
    /// The cutoff this projection was folded at, or absent for the whole
    /// prefix. Reported so a filmstrip frame carries the cutoff that made it.
    pub cutoff: Option<CursorVector>,
}

/// Token and cost accounting, as the last envelope that carried any reported
/// it. A field the producer omitted stays absent rather than becoming zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LiveEvalUsage {
    pub prompt_tokens: Option<f64>,
    pub completion_tokens: Option<f64>,
    pub total_tokens: Option<f64>,
    pub cost_usd: Option<f64>,
}

fn finite_number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite())
}

fn payload_number(payload: Option<&Value>, keys: &[&str]) -> Option<f64> {
    let payload = payload?;
    keys.iter().find_map(|key| finite_number(payload.get(*key)))
}

/// Whether any key anywhere under `value` is named `name`.
fn has_key(value: &Value, name: &str) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(key, nested)| key == name || has_key(nested, name)),
        Value::Array(rows) => rows.iter().any(|nested| has_key(nested, name)),
        _ => false,
    }
}

/// Fold an ordered evidence log into the values a live-eval template renders.
///
/// `events` is a deduped log in arrival order. Control envelopes are filtered
/// here as well as by the fold, so a caller that hands over a raw log gets the
/// same answer as one that hands over folded evidence.
pub fn project_live_eval(
    events: &[Value],
    cutoff: Option<&CursorVector>,
) -> anyhow::Result<LiveEvalProjection> {
    let mut rows: Vec<&Value> = Vec::new();
    let mut taken: BTreeMap<String, u64> = BTreeMap::new();
    for event in events {
        if is_control(event) {
            continue;
        }
        if let Some(cutoff) = cutoff {
            let stream = envelope_stream(event);
            let taken_here = taken.entry(stream.clone()).or_insert(0);
            if *taken_here >= cutoff.get(&stream) {
                continue;
            }
            *taken_here += 1;
        }
        rows.push(event);
    }

    let kinds: Vec<String> = rows.iter().copied().map(envelope_kind).collect();
    let has_live_frames = kinds.iter().any(|kind| kind == "frame");
    let has_reward_txt = rows.iter().copied().any(|event: &Value| {
        event
            .get("payload")
            .is_some_and(|payload| has_key(payload, "reward.txt"))
    });

    let mut reward = rows
        .iter()
        .copied()
        .rev()
        .find(|event: &&Value| envelope_kind(event) == "verifier")
        .and_then(|event| finite_number(event.get("payload").and_then(|p| p.get("reward.txt"))));
    if reward.is_none() {
        reward = rows
            .iter()
            .copied()
            .rev()
            .find(|event: &&Value| {
                let kind = envelope_kind(event);
                kind == "reward_signal" || kind == "eval.run.terminal"
            })
            .and_then(|event| payload_number(event.get("payload"), &["value", "reward", "total"]));
    }

    let usage = rows
        .iter()
        .copied()
        .rev()
        .find_map(|event: &Value| {
            event
                .get("payload")
                .and_then(|payload| payload.get("usage"))
                .filter(|usage| usage.is_object())
        })
        .map(|usage| LiveEvalUsage {
            prompt_tokens: finite_number(usage.get("prompt_tokens")),
            completion_tokens: finite_number(usage.get("completion_tokens")),
            total_tokens: finite_number(usage.get("total_tokens")),
            cost_usd: finite_number(usage.get("cost_usd")),
        });

    let projection = LiveEvalProjection {
        events: rows.iter().copied().cloned().collect(),
        kinds,
        has_live_frames,
        has_reward_txt,
        reward,
        usage,
        cutoff: cutoff.cloned(),
    };

    // The same refusal the renderer's projector makes, for the same reason: a
    // projection is the thing that gets sealed, and a collector or capability
    // blob that reaches it is exfiltrated evidence, not a rendering bug.
    let blob = serde_json::to_string(&projection)?;
    for name in FORBIDDEN_BLOBS {
        if blob.contains(name) {
            anyhow::bail!("live eval projection leaked forbidden blob \"{name}\"");
        }
    }
    Ok(projection)
}

#[cfg(test)]
mod tests;
