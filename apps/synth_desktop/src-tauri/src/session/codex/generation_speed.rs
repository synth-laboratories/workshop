//! Observed generation TPS: exact provider response output divided by the
//! matching model-output window.
//!
//! The invariant this module exists to enforce: tokens and elapsed time in a
//! published rate come from *the same* measured segment. A segment is one
//! output-text content part of one model response. Nothing is ever combined
//! across a tool call, a different output item or content part, an
//! interruption, or a separate model response.
//!
//! Boundaries are semantic, never temporal. A silence of two seconds or twenty
//! stays in the denominator if the protocol says the same content part is
//! still in progress — the previous implementation's 2,000 ms gap rule silently
//! shrank the denominator and produced dimensionally plausible, semantically
//! invalid numbers. Segments end only on an explicit protocol event: the part
//! or item finishing, the response finishing, an interruption, or a different
//! item taking over the stream.
//!
//! The number is *observed* generation TPS: timestamps are taken when Workshop
//! decoded the stream frame, so it measures delivery as this client saw it, not
//! the model's decoder. That label is load-bearing and must survive to the UI.
//!
//! Token authority is strict, in this precedence:
//!   1. provider-reported counts scoped exactly to this output item/content part,
//!   2. exact local tokenization under a verified model→tokenizer mapping,
//!   3. no TPS.
//! Response usage may enrich the completed final answer when its denominator
//! spans that same response's complete model-output window. Characters, words,
//! bytes, delta counts, and historical ratios are not tokens; none of them may
//! stand in for one. When no exact source covers the segment, the measurement
//! is published with `tps: null` and a machine-readable reason.

use serde::Serialize;
use serde_json::Value;
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

/// Versioned so history can tell these apart from anything recorded earlier.
/// Nothing produced before this schema is a segment measurement.
pub(crate) const MEASUREMENT_SCHEMA_VERSION: &str = "synth.generation-speed.v1";

/// The one measurement kind this module produces.
pub(crate) const MEASUREMENT_KIND: &str = "observed_stream_segment";

/// The journal event a finished measurement is published as.
pub(crate) const MEASUREMENT_EVENT: &str = "turn/generationSpeed";

/// Eligibility thresholds. Below any of these the samples do not support a
/// rate, and the measurement publishes `tps: null` rather than a number the
/// evidence cannot carry.
const MIN_DISTINCT_SAMPLES: usize = 4;
const MIN_TOKENS_AFTER_FIRST_SAMPLE: i64 = 16;
const MIN_DURATION_US: i64 = 500_000;

/// Hard ceiling on retained samples per segment, so a pathological stream
/// cannot grow one measurement without bound. Real segments are hundreds of
/// deltas; reaching this is recorded as a quality flag rather than hidden.
const MAX_SAMPLES: usize = 65_536;

/// Process-monotonic microseconds.
///
/// Deliberately not `chrono::Utc::now()`: the wall clock can step backwards
/// across an NTP correction or a sleep/wake, and one negative interval inside a
/// segment would distort the slope with nothing in the record to show why.
/// Microseconds, not milliseconds, because adjacent deltas arrive ~2 ms apart
/// and millisecond truncation would quantize the regression.
pub(crate) fn monotonic_us() -> i64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    let origin = ORIGIN.get_or_init(Instant::now);
    Instant::now()
        .saturating_duration_since(*origin)
        .min(Duration::from_secs(i64::MAX as u64 / 1_000_000))
        .as_micros() as i64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SegmentPhase {
    Commentary,
    FinalAnswer,
    Other,
}

impl SegmentPhase {
    fn parse(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("commentary") => Self::Commentary,
            Some("final_answer") | Some("finalanswer") => Self::FinalAnswer,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SegmentStatus {
    /// The segment ended on its own protocol lifecycle event.
    Completed,
    /// The segment was still open when the stream ended. Labelled, and kept
    /// out of headline and history aggregates.
    Partial,
    /// No rate could be published for this segment.
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TokenCountSource {
    /// Provider-reported tokens scoped exactly to this output item/content part.
    ProviderItemUsage,
    /// Exact provider output usage for one response, including reasoning,
    /// divided by that response's observed model-output window. TTFT and tool
    /// execution are outside the denominator.
    ProviderResponseOutputUsage,
    /// Exact local tokenization under a verified model→tokenizer mapping, with
    /// the tokenizer's identity recorded. Declared because it is the second
    /// rung of the token-authority ladder; no shipped provider mapping is
    /// verified yet, so nothing constructs it. A guess dressed as a tokenizer
    /// would be worse than the `Unavailable` below it.
    #[allow(dead_code)]
    ExactTokenizer,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClockSource {
    /// Provider-supplied per-event timestamps from one clock domain. Declared
    /// as the preferred source; no transport here supplies them, and mixing a
    /// provider clock with a local one inside a segment would be unsound.
    #[allow(dead_code)]
    ProviderEventTimestamp,
    /// Taken as this process decoded the stream frame, before any IPC or
    /// renderer work — hence *observed* delivery.
    WorkshopMonotonicReceive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UnavailableReason {
    InsufficientSamples,
    InsufficientTokens,
    InsufficientDuration,
    MissingExactTokenSource,
    UsageScopeMismatch,
    SequenceGap,
    MixedSegment,
    /// Part of the published vocabulary; never produced here. An interruption
    /// does not by itself disqualify a rate — a cut segment that still cleared
    /// every threshold is `partial` with a real number, and one that did not
    /// reports the threshold it missed, which is the more useful answer.
    #[allow(dead_code)]
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QualityFlag {
    /// Two or more deltas shared one receipt timestamp.
    BatchedDelivery,
    OutOfOrderEvent,
    /// This segment's delta sequence numbers skipped a value. Recorded, not
    /// fatal — see `observe_delta`.
    SequenceGapObserved,
    /// `MAX_SAMPLES` was reached; later deltas are not in the record.
    SampleLimitReached,
}

/// Identity of one output-text content part. Every field must stay identical
/// for samples to belong to the same segment.
///
/// `response_id` is `Option` on purpose. Some transports (the Codex app-server
/// among them) never expose the provider's response id. Substituting a turn id
/// for it would be a lie — a turn can span several model calls — so it stays
/// absent, and `item_id` carries identity. That is sound here because
/// `item_id` is the provider's own per-message id: two model responses in one
/// turn always carry different item ids and can never merge into one segment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SegmentKey {
    pub(crate) response_id: Option<String>,
    pub(crate) item_id: String,
    pub(crate) output_index: i64,
    pub(crate) content_index: i64,
}

/// One retained delivery observation. Persisted alongside the derived scalar so
/// a reported value can be recomputed offline from its own raw evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Sample {
    pub(crate) at_us: i64,
    /// Exact cumulative output tokens for this content part at this instant.
    /// `None` when no exact source covers the segment — never an estimate.
    pub(crate) cumulative_tokens: Option<i64>,
    pub(crate) sequence_number: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenerationSpeedMeasurement {
    pub(crate) schema_version: &'static str,
    pub(crate) measurement_kind: &'static str,
    pub(crate) measurement_id: String,
    pub(crate) session_id: String,
    pub(crate) turn_id: String,
    #[serde(flatten)]
    pub(crate) key: SegmentKey,
    pub(crate) phase: SegmentPhase,
    pub(crate) status: SegmentStatus,
    /// `None` whenever the evidence does not support a rate. Never carried
    /// forward from an earlier segment.
    pub(crate) tps: Option<f64>,
    pub(crate) exact_tokens_after_first_sample: i64,
    pub(crate) duration_ms: f64,
    pub(crate) sample_count: usize,
    pub(crate) token_count_source: TokenCountSource,
    pub(crate) tokenizer_id: Option<String>,
    pub(crate) clock_source: ClockSource,
    pub(crate) unavailable_reason: Option<UnavailableReason>,
    pub(crate) quality_flags: Vec<QualityFlag>,
    pub(crate) provider: Option<String>,
    pub(crate) model_id: Option<String>,
    pub(crate) samples: Vec<Sample>,
}

impl GenerationSpeedMeasurement {
    /// Whether this measurement may drive a headline label or enter an
    /// aggregate. A partial segment carries a rate for diagnostics only.
    pub(crate) fn is_publishable(&self) -> bool {
        self.tps.is_some() && self.status == SegmentStatus::Completed
    }
}

/// Ordinary least-squares slope of cumulative tokens on time, in tokens/s.
///
/// A single delta is instantaneous and has no rate; a rate only exists across
/// samples. Regression uses every observed delivery point, so it is far less
/// sensitive to one batched arrival than an adjacent-pair difference would be.
/// It still measures client-observed delivery — the label must say so.
///
/// `None` when the points are degenerate (all at one instant), which would
/// otherwise divide by zero.
pub(crate) fn ols_tokens_per_second(points: &[(f64, f64)]) -> Option<f64> {
    if points.len() < 2 {
        return None;
    }
    let count = points.len() as f64;
    let mean_x = points.iter().map(|(x, _)| x).sum::<f64>() / count;
    let mean_y = points.iter().map(|(_, y)| y).sum::<f64>() / count;
    let mut covariance = 0.0;
    let mut variance = 0.0;
    for (x, y) in points {
        covariance += (x - mean_x) * (y - mean_y);
        variance += (x - mean_x) * (x - mean_x);
    }
    if variance <= 0.0 {
        return None;
    }
    // Points are (microseconds, tokens); scale the slope to tokens/second.
    let slope = 1_000_000.0 * covariance / variance;
    slope.is_finite().then_some(slope)
}

/// One open output-text segment, accumulating its own samples.
#[derive(Clone, Debug)]
pub(crate) struct SegmentRecorder {
    key: SegmentKey,
    phase: SegmentPhase,
    samples: Vec<Sample>,
    quality_flags: Vec<QualityFlag>,
    token_count_source: TokenCountSource,
    tokenizer_id: Option<String>,
    /// Set when the stream showed a duplicate, regression, or hole in the
    /// protocol's own sequence numbering.
    sequence_broken: bool,
    highest_sequence: Option<i64>,
    /// Set when samples that are not this segment's were offered to it.
    mixed: bool,
    /// Set when the only usage the stream offered was response- or turn-scoped.
    response_scoped_usage_only: bool,
}

impl SegmentRecorder {
    fn new(key: SegmentKey, phase: SegmentPhase) -> Self {
        Self {
            key,
            phase,
            samples: Vec::new(),
            quality_flags: Vec::new(),
            token_count_source: TokenCountSource::Unavailable,
            tokenizer_id: None,
            sequence_broken: false,
            highest_sequence: None,
            mixed: false,
            response_scoped_usage_only: false,
        }
    }

    fn flag(&mut self, flag: QualityFlag) {
        if !self.quality_flags.contains(&flag) {
            self.quality_flags.push(flag);
        }
    }

    fn observe_delta(&mut self, delta: &OutputTextDelta<'_>, at_us: i64) {
        if delta.key != self.key {
            // Refusing to fold a foreign sample in is the whole point; record
            // that it was offered so the measurement can disqualify itself.
            self.mixed = true;
            return;
        }
        if let Some(sequence) = delta.sequence_number {
            match self.highest_sequence {
                // A repeat or a regression means these are not the samples the
                // provider sent in the order it sent them. A duplicate adds a
                // later timestamp carrying no new tokens, which flattens the
                // slope; a reordering breaks the pairing outright. Neither can
                // be corrected after the fact, so the segment publishes nothing.
                Some(previous) if sequence <= previous => {
                    self.sequence_broken = true;
                    self.flag(QualityFlag::OutOfOrderEvent);
                }
                // A hole is recorded but does not disqualify. Samples carry
                // *cumulative* token counts, so a missing delta removes a point
                // from the regression without changing its slope — it cannot
                // inflate a rate. Treating holes as fatal would also misjudge
                // transports whose `sequence_number` counts every event in the
                // response rather than only this part's deltas, where gaps are
                // the normal case.
                Some(previous) if sequence > previous + 1 => {
                    self.flag(QualityFlag::SequenceGapObserved);
                }
                _ => {}
            }
            self.highest_sequence =
                Some(self.highest_sequence.map_or(sequence, |p| p.max(sequence)));
        }
        if delta.response_scoped_usage_only {
            self.response_scoped_usage_only = true;
        }
        if self.samples.len() >= MAX_SAMPLES {
            self.flag(QualityFlag::SampleLimitReached);
            return;
        }
        if self.samples.last().is_some_and(|last| last.at_us == at_us) {
            self.flag(QualityFlag::BatchedDelivery);
        }
        if let Some(tokens) = delta.cumulative_exact_tokens {
            self.token_count_source = delta.token_source;
            self.tokenizer_id = delta.tokenizer_id.map(str::to_owned);
            self.samples.push(Sample {
                at_us,
                cumulative_tokens: Some(tokens),
                sequence_number: delta.sequence_number,
            });
        } else {
            self.samples.push(Sample {
                at_us,
                cumulative_tokens: None,
                sequence_number: delta.sequence_number,
            });
        }
    }

    fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Turn accumulated evidence into one measurement. Every rejection path
    /// yields `tps: None` plus the reason it was rejected.
    fn finish(
        self,
        session_id: &str,
        turn_id: &str,
        provider: Option<&str>,
        model_id: Option<&str>,
        ended: SegmentEnd,
    ) -> GenerationSpeedMeasurement {
        let first = self.samples.first().copied();
        let last = self.samples.last().copied();
        let duration_us = match (first, last) {
            (Some(first), Some(last)) => (last.at_us - first.at_us).max(0),
            _ => 0,
        };
        // The first sample is the origin. It deliberately does not claim the
        // tokens already delivered at that instant were generated after
        // measurement began.
        let tokens_after_first = match (
            first.and_then(|s| s.cumulative_tokens),
            last.and_then(|s| s.cumulative_tokens),
        ) {
            (Some(start), Some(end)) => (end - start).max(0),
            _ => 0,
        };
        let distinct_timestamps = {
            let mut stamps: Vec<i64> = self.samples.iter().map(|sample| sample.at_us).collect();
            stamps.dedup();
            stamps.len()
        };
        let rejection = self.reject(distinct_timestamps, tokens_after_first, duration_us);
        let tps = match rejection {
            Some(_) => None,
            None => {
                let origin = first.expect("a segment with samples has a first sample");
                let start_tokens = origin.cumulative_tokens.unwrap_or(0);
                let points: Vec<(f64, f64)> = self
                    .samples
                    .iter()
                    .filter_map(|sample| {
                        Some((
                            (sample.at_us - origin.at_us) as f64,
                            (sample.cumulative_tokens? - start_tokens) as f64,
                        ))
                    })
                    .collect();
                ols_tokens_per_second(&points).filter(|rate| *rate > 0.0)
            }
        };
        // A rate that survived every gate but came out non-finite or negative
        // is still not a rate; it must not reach the UI as one.
        let (tps, unavailable_reason) = match (tps, rejection) {
            (Some(rate), _) => (Some(rate), None),
            (None, Some(reason)) => (None, Some(reason)),
            (None, None) => (None, Some(UnavailableReason::InsufficientSamples)),
        };
        GenerationSpeedMeasurement {
            schema_version: MEASUREMENT_SCHEMA_VERSION,
            measurement_kind: MEASUREMENT_KIND,
            // The full segment key, not just the item: one item may carry more
            // than one content part, and each is its own measurement. A
            // narrower id would let the store's idempotent insert silently drop
            // the second one.
            measurement_id: format!(
                "gs:{session_id}:{turn_id}:{}:{}:{}",
                self.key.item_id, self.key.output_index, self.key.content_index
            ),
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
            // A rate that exists but came off an unclosed part is `partial`:
            // real evidence, deliberately kept out of headline and history.
            status: match (tps.is_some(), ended) {
                (false, _) => SegmentStatus::Unavailable,
                (true, SegmentEnd::Interrupted) => SegmentStatus::Partial,
                (true, SegmentEnd::Lifecycle) => SegmentStatus::Completed,
            },
            phase: self.phase,
            tps,
            exact_tokens_after_first_sample: tokens_after_first,
            duration_ms: duration_us as f64 / 1_000.0,
            sample_count: self.samples.len(),
            token_count_source: self.token_count_source,
            tokenizer_id: self.tokenizer_id,
            clock_source: ClockSource::WorkshopMonotonicReceive,
            unavailable_reason,
            quality_flags: self.quality_flags,
            provider: provider.map(str::to_owned),
            model_id: model_id.map(str::to_owned),
            key: self.key,
            samples: self.samples,
        }
    }

    /// The first reason this segment cannot publish a rate, in the order a
    /// reader would want to hear it: what is missing outright, then what is
    /// untrustworthy, then what is merely too small.
    fn reject(
        &self,
        distinct_timestamps: usize,
        tokens_after_first: i64,
        duration_us: i64,
    ) -> Option<UnavailableReason> {
        if self.mixed {
            return Some(UnavailableReason::MixedSegment);
        }
        if self.token_count_source == TokenCountSource::Unavailable
            || self
                .samples
                .iter()
                .any(|sample| sample.cumulative_tokens.is_none())
        {
            return Some(if self.response_scoped_usage_only {
                UnavailableReason::UsageScopeMismatch
            } else {
                UnavailableReason::MissingExactTokenSource
            });
        }
        if self.sequence_broken {
            return Some(UnavailableReason::SequenceGap);
        }
        if distinct_timestamps < MIN_DISTINCT_SAMPLES {
            return Some(UnavailableReason::InsufficientSamples);
        }
        if tokens_after_first < MIN_TOKENS_AFTER_FIRST_SAMPLE {
            return Some(UnavailableReason::InsufficientTokens);
        }
        if duration_us < MIN_DURATION_US {
            return Some(UnavailableReason::InsufficientDuration);
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegmentEnd {
    /// The protocol said this part, item, or response finished.
    Lifecycle,
    /// The stream ended, was cancelled, or failed with the part still open.
    Interrupted,
}

/// One output-text delta, already normalized out of whatever transport
/// delivered it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OutputTextDelta<'a> {
    pub(crate) key: SegmentKey,
    pub(crate) phase: SegmentPhase,
    pub(crate) text: &'a str,
    pub(crate) sequence_number: Option<i64>,
    /// Exact cumulative output tokens for this content part, from the source
    /// named by `token_source`. Never derived from characters or delta counts.
    pub(crate) cumulative_exact_tokens: Option<i64>,
    pub(crate) token_source: TokenCountSource,
    pub(crate) tokenizer_id: Option<&'a str>,
    /// The event carried token usage, but scoped to the response or the turn
    /// rather than to this content part. That is a different quantity — it can
    /// include reasoning and other items — and using it here would be the
    /// original defect. Recorded so the refusal names the real cause.
    pub(crate) response_scoped_usage_only: bool,
}

/// A protocol event reduced to the parts that bear on segmentation.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ProtocolEvent<'a> {
    /// An output-text item opened; its phase is known from here on.
    TextItemStarted {
        item_id: String,
        phase: SegmentPhase,
    },
    OutputTextDelta(OutputTextDelta<'a>),
    /// This content part, item, or response text finished normally.
    TextSegmentDone {
        item_id: String,
        phase: SegmentPhase,
    },
    /// A reasoning item began generating. Its output belongs to the response
    /// numerator and its lifecycle start anchors the otherwise-hidden output
    /// interval.
    ReasoningStarted {
        item_id: Option<String>,
    },
    /// A reasoning item completed. It closes visible text but does not split
    /// the model response.
    ReasoningDone {
        item_id: Option<String>,
    },
    /// Tool execution/call material separates model responses. It closes any
    /// visible segment and resets the output-window anchor.
    ToolBoundary {
        item_id: Option<String>,
    },
    ResponseTerminal {
        interrupted: bool,
    },
}

fn text_field<'a>(params: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| params.get(*key).and_then(Value::as_str))
}

fn integer_field(params: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| params.get(*key).and_then(Value::as_i64))
}

/// Exact cumulative output tokens for *this content part*, if the transport
/// carried them: a provider extension stating the running count directly, or a
/// usage object attached to the item.
///
/// Deliberately narrow. Anything that is not explicitly an exact token count
/// for this content part is absent, never approximated — not characters, not
/// words, not bytes, not the number of deltas seen, not a historical ratio.
fn exact_cumulative_tokens(params: &Value) -> Option<(i64, TokenCountSource)> {
    if let Some(tokens) = integer_field(
        params,
        &[
            "cumulative_output_tokens",
            "cumulativeOutputTokens",
            "item_output_tokens",
            "itemOutputTokens",
        ],
    ) {
        return Some((tokens, TokenCountSource::ProviderItemUsage));
    }
    let usage = params
        .pointer("/item/usage")
        .filter(|usage| usage.is_object())?;
    let tokens = integer_field(usage, &["output_tokens", "outputTokens"])?;
    Some((tokens, TokenCountSource::ProviderItemUsage))
}

/// Whether the event carries usage that describes the response or the turn
/// rather than this content part. Present so a refusal can say *why* — usage
/// existed, but not at a scope this metric may use.
fn carries_response_scoped_usage(params: &Value) -> bool {
    [
        "/usage",
        "/tokenUsage",
        "/token_usage",
        "/response/usage",
        "/turn/usage",
    ]
    .iter()
    .any(|path| params.pointer(path).is_some_and(Value::is_object))
}

fn segment_key(params: &Value, item_id: String) -> SegmentKey {
    SegmentKey {
        response_id: text_field(params, &["response_id", "responseId"]).map(str::to_owned),
        item_id,
        output_index: integer_field(params, &["output_index", "outputIndex"]).unwrap_or(0),
        content_index: integer_field(params, &["content_index", "contentIndex"]).unwrap_or(0),
    }
}

fn item_id_of(params: &Value) -> Option<String> {
    text_field(params, &["item_id", "itemId"])
        .map(str::to_owned)
        .or_else(|| {
            params
                .pointer("/item/id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

/// Whether an item type carries visible answer text. Reasoning summaries and
/// tool-call arguments are generated tokens but they are not answer tokens, and
/// folding them into this metric is exactly the error being removed.
fn is_text_item(item_type: Option<&str>) -> bool {
    matches!(
        item_type.map(str::to_ascii_lowercase).as_deref(),
        Some("agentmessage") | Some("message") | Some("output_text")
    )
}

fn is_reasoning_item(item_type: Option<&str>) -> bool {
    matches!(
        item_type.map(str::to_ascii_lowercase).as_deref(),
        Some("reasoning") | Some("reasoning_summary") | Some("reasoningsummary")
    )
}

/// Normalize one transport event into the segmentation vocabulary.
///
/// Two shapes are recognized: the Codex app-server's JSON-RPC notifications and
/// OpenResponses' own event names, since both describe the same item and
/// content-part lifecycle. A transport that exposes neither identity nor
/// lifecycle yields `None`, and its segments are simply never measured — that
/// is the intended outcome, not a gap to paper over with a guess.
pub(crate) fn protocol_event<'a>(method: &str, params: &'a Value) -> Option<ProtocolEvent<'a>> {
    let normalized = method.to_ascii_lowercase();
    let item_type = params
        .pointer("/item/type")
        .and_then(Value::as_str)
        .or_else(|| params.get("type").and_then(Value::as_str));
    let phase = SegmentPhase::parse(
        params
            .pointer("/item/phase")
            .and_then(Value::as_str)
            .or_else(|| params.get("phase").and_then(Value::as_str)),
    );

    let is_output_text_delta = normalized.contains("agentmessage/delta")
        || normalized.contains("agent_message/delta")
        || normalized.contains("output_text.delta")
        || normalized.contains("outputtext/delta")
        || normalized.contains("output_text/delta");
    if is_output_text_delta {
        let text = params.get("delta").and_then(Value::as_str)?;
        if text.is_empty() {
            // An empty delta starts nothing and delivers nothing.
            return None;
        }
        let item_id = item_id_of(params)?;
        let (cumulative_exact_tokens, token_source) = match exact_cumulative_tokens(params) {
            Some((tokens, source)) => (Some(tokens), source),
            None => (None, TokenCountSource::Unavailable),
        };
        return Some(ProtocolEvent::OutputTextDelta(OutputTextDelta {
            key: segment_key(params, item_id),
            phase,
            text,
            sequence_number: integer_field(params, &["sequence_number", "sequenceNumber"]),
            cumulative_exact_tokens,
            token_source,
            tokenizer_id: text_field(params, &["tokenizer_id", "tokenizerId"]),
            response_scoped_usage_only: cumulative_exact_tokens.is_none()
                && carries_response_scoped_usage(params),
        }));
    }

    // Reasoning is part of full provider output; tool material is a response
    // boundary and must never enter the model-generation interval.
    if normalized.contains("reasoning") && normalized.contains("delta") {
        return Some(ProtocolEvent::ReasoningStarted {
            item_id: item_id_of(params),
        });
    }
    if normalized.contains("reasoning") && normalized.contains("done") {
        return Some(ProtocolEvent::ReasoningDone {
            item_id: item_id_of(params),
        });
    }
    let is_tool_delta = normalized.contains("function_call_arguments")
        || normalized.contains("custom_tool_call_input")
        || normalized.contains("_call.arguments")
        || normalized.contains("commandexecution/outputdelta")
        || normalized.contains("command_execution/output_delta");
    if is_tool_delta && (normalized.contains("delta") || normalized.contains("done")) {
        return Some(ProtocolEvent::ToolBoundary {
            item_id: item_id_of(params),
        });
    }

    let ends_text_part = normalized == "response.output_text.done"
        || normalized == "response.content_part.done"
        || normalized == "response.output_item.done"
        || normalized == "item/completed";
    if ends_text_part {
        let item_id = item_id_of(params)?;
        return Some(if is_text_item(item_type) {
            ProtocolEvent::TextSegmentDone { item_id, phase }
        } else if is_reasoning_item(item_type) {
            ProtocolEvent::ReasoningDone {
                item_id: Some(item_id),
            }
        } else {
            ProtocolEvent::ToolBoundary {
                item_id: Some(item_id),
            }
        });
    }

    if normalized == "item/started" || normalized == "response.output_item.added" {
        let item_id = item_id_of(params)?;
        return Some(if is_text_item(item_type) {
            ProtocolEvent::TextItemStarted { item_id, phase }
        } else if is_reasoning_item(item_type) {
            ProtocolEvent::ReasoningStarted {
                item_id: Some(item_id),
            }
        } else {
            ProtocolEvent::ToolBoundary {
                item_id: Some(item_id),
            }
        });
    }

    match normalized.as_str() {
        "turn/completed" | "response.completed" => {
            Some(ProtocolEvent::ResponseTerminal { interrupted: false })
        }
        "turn/failed" | "turn/interrupted" | "response.failed" | "response.incomplete" => {
            Some(ProtocolEvent::ResponseTerminal { interrupted: true })
        }
        _ => None,
    }
}

/// The open segments of one turn.
///
/// Usually holds zero or one recorder: a response streams one content part at a
/// time. It is a list rather than a single slot so that a transport which
/// interleaves parts still keeps them apart instead of merging them.
#[derive(Clone, Debug)]
pub(crate) struct TurnSegmentTracker {
    session_id: String,
    turn_id: String,
    provider: Option<String>,
    model_id: Option<String>,
    open: Vec<SegmentRecorder>,
    finished: Vec<GenerationSpeedMeasurement>,
    response_output_started_at_us: Option<i64>,
}

impl TurnSegmentTracker {
    pub(crate) fn new(
        session_id: impl Into<String>,
        turn_id: impl Into<String>,
        provider: Option<String>,
        model_id: Option<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            turn_id: turn_id.into(),
            provider,
            model_id,
            open: Vec::new(),
            finished: Vec::new(),
            response_output_started_at_us: None,
        }
    }

    /// Feed one normalized protocol event, stamped with the instant the frame
    /// was decoded. Returns the measurements this event finalized, in the order
    /// their segments ended.
    pub(crate) fn observe(
        &mut self,
        event: ProtocolEvent<'_>,
        at_us: i64,
    ) -> Vec<GenerationSpeedMeasurement> {
        let before = self.finished.len();
        match event {
            ProtocolEvent::TextItemStarted { item_id, phase } => {
                self.close_others(Some(&item_id), SegmentEnd::Lifecycle);
                if !self.open.iter().any(|open| open.key.item_id == item_id) {
                    // Opened, not sampled: a part that never delivers a delta
                    // produces no measurement at all rather than an empty one.
                    self.open.push(SegmentRecorder::new(
                        SegmentKey {
                            response_id: None,
                            item_id,
                            output_index: 0,
                            content_index: 0,
                        },
                        phase,
                    ));
                }
            }
            ProtocolEvent::OutputTextDelta(delta) => {
                self.response_output_started_at_us.get_or_insert(at_us);
                self.close_others(Some(&delta.key.item_id), SegmentEnd::Lifecycle);
                let existing = self
                    .open
                    .iter_mut()
                    .find(|open| open.key.item_id == delta.key.item_id);
                let recorder = match existing {
                    Some(recorder) => {
                        // The item is the same but the part is not: a new
                        // content part is a new segment, never a continuation.
                        if recorder.key != delta.key && !recorder.is_empty() {
                            let key = recorder.key.clone();
                            self.close_key(&key, SegmentEnd::Lifecycle);
                            self.open
                                .push(SegmentRecorder::new(delta.key.clone(), delta.phase));
                            self.open.last_mut().expect("just pushed")
                        } else {
                            if recorder.is_empty() {
                                // The item was opened before its part identity
                                // was known; adopt the delta's. The phase is
                                // only adopted when the delta actually states
                                // one — most transports carry it on the item
                                // lifecycle event, not on every delta, and
                                // overwriting a known phase with an absent one
                                // would mislabel a segment that never closes.
                                recorder.key = delta.key.clone();
                                if delta.phase != SegmentPhase::Other {
                                    recorder.phase = delta.phase;
                                }
                            }
                            recorder
                        }
                    }
                    None => {
                        self.open
                            .push(SegmentRecorder::new(delta.key.clone(), delta.phase));
                        self.open.last_mut().expect("just pushed")
                    }
                };
                recorder.observe_delta(&delta, at_us);
            }
            ProtocolEvent::TextSegmentDone { item_id, phase } => {
                if let Some(recorder) = self
                    .open
                    .iter_mut()
                    .find(|open| open.key.item_id == item_id)
                {
                    // `item/started` may not have carried the final phase.
                    if phase != SegmentPhase::Other {
                        recorder.phase = phase;
                    }
                }
                self.close_item(&item_id, SegmentEnd::Lifecycle);
            }
            ProtocolEvent::ReasoningStarted { item_id } => {
                self.close_others(item_id.as_deref(), SegmentEnd::Lifecycle);
                self.response_output_started_at_us.get_or_insert(at_us);
            }
            ProtocolEvent::ReasoningDone { item_id } => {
                self.close_others(item_id.as_deref(), SegmentEnd::Lifecycle);
            }
            ProtocolEvent::ToolBoundary { item_id } => {
                self.close_others(item_id.as_deref(), SegmentEnd::Lifecycle);
                self.response_output_started_at_us = None;
            }
            ProtocolEvent::ResponseTerminal { interrupted: _ } => {
                // Anything still open when the response ends never received the
                // lifecycle event that closes a part, so what it holds is
                // partial evidence — whether the response completed or failed.
                self.close_others(None, SegmentEnd::Interrupted);
            }
        }
        self.finished[before..].to_vec()
    }

    /// Finalize everything still open, e.g. because the transport died.
    pub(crate) fn finish(&mut self) -> Vec<GenerationSpeedMeasurement> {
        let before = self.finished.len();
        self.close_others(None, SegmentEnd::Interrupted);
        self.finished[before..].to_vec()
    }

    /// Every measurement this turn produced, including those already returned.
    pub(crate) fn measurements(&self) -> &[GenerationSpeedMeasurement] {
        &self.finished
    }

    /// Bind late-arriving provider usage to the immediately preceding final
    /// answer. Codex reports `lastUsage` after `item/completed`, so the segment
    /// has already been finalized by the time its exact response totals arrive.
    ///
    /// `output_tokens` is the provider's complete output count for this
    /// response, including reasoning. The denominator starts at the first
    /// observed reasoning/output lifecycle event and ends at the last final
    /// answer delta. TTFT, tool execution, and other model calls stay out.
    pub(crate) fn apply_final_response_output_usage(
        &mut self,
        output_tokens: i64,
    ) -> Option<GenerationSpeedMeasurement> {
        let response_started_at_us = self.response_output_started_at_us?;
        let measurement = self.finished.last_mut()?;
        if measurement.phase != SegmentPhase::FinalAnswer
            || measurement.status != SegmentStatus::Unavailable
            || measurement.token_count_source != TokenCountSource::Unavailable
            || !matches!(
                measurement.unavailable_reason,
                Some(UnavailableReason::MissingExactTokenSource)
                    | Some(UnavailableReason::UsageScopeMismatch)
            )
            || measurement.sample_count < MIN_DISTINCT_SAMPLES
        {
            return None;
        }

        let response_ended_at_us = measurement.samples.last()?.at_us;
        let duration_us = response_ended_at_us.checked_sub(response_started_at_us)?;
        if duration_us < MIN_DURATION_US {
            return None;
        }

        // The interval begins at the first observed delivery, so conservatively
        // remove one token from the exact response total. The first
        // delivery may contain more, never fewer; the result is explicitly an
        // observed segment estimate rather than a decoder benchmark.
        let tokens_after_first = output_tokens.saturating_sub(1);
        if tokens_after_first < MIN_TOKENS_AFTER_FIRST_SAMPLE {
            return None;
        }
        let duration_ms = duration_us as f64 / 1_000.0;
        let rate = tokens_after_first as f64 / (duration_ms / 1_000.0);
        if !rate.is_finite() || rate <= 0.0 {
            return None;
        }

        measurement.tps = Some(rate);
        measurement.duration_ms = duration_ms;
        measurement.status = SegmentStatus::Completed;
        measurement.exact_tokens_after_first_sample = tokens_after_first;
        measurement.token_count_source = TokenCountSource::ProviderResponseOutputUsage;
        measurement.unavailable_reason = None;
        self.response_output_started_at_us = None;
        Some(measurement.clone())
    }

    fn close_others(&mut self, keep_item_id: Option<&str>, ended: SegmentEnd) {
        let closing: Vec<SegmentKey> = self
            .open
            .iter()
            .filter(|open| keep_item_id != Some(open.key.item_id.as_str()))
            .map(|open| open.key.clone())
            .collect();
        for key in closing {
            self.close_key(&key, ended);
        }
    }

    fn close_item(&mut self, item_id: &str, ended: SegmentEnd) {
        let closing: Vec<SegmentKey> = self
            .open
            .iter()
            .filter(|open| open.key.item_id == item_id)
            .map(|open| open.key.clone())
            .collect();
        for key in closing {
            self.close_key(&key, ended);
        }
    }

    fn close_key(&mut self, key: &SegmentKey, ended: SegmentEnd) {
        let Some(index) = self.open.iter().position(|open| &open.key == key) else {
            return;
        };
        let recorder = self.open.remove(index);
        if recorder.is_empty() {
            return;
        }
        self.finished.push(recorder.finish(
            &self.session_id,
            &self.turn_id,
            self.provider.as_deref(),
            self.model_id.as_deref(),
            ended,
        ));
    }
}
