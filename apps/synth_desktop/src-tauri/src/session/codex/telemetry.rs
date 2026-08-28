//! Turn performance / usage telemetry for the Codex app-server pump.
//!
//! This module owns *turn-level* facts: token usage, time to first output,
//! end-to-end latency, and cost. It deliberately no longer computes a
//! generation rate. Turn-wide output tokens over a turn-wide (or gap-filtered)
//! denominator mixes several model calls, tool execution, and reasoning into
//! one ratio; the number it produced was dimensionally plausible and
//! semantically invalid. Generation speed is measured per output-text segment
//! in `super::generation_speed`, and only that measurement may be shown as
//! token/s.
use crate::domain::RunStatus;
use crate::storage::{CostSource, GenerationSpeedRow, MeasurementKind, UsageRecord};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use super::generation_speed::{
    protocol_event, GenerationSpeedMeasurement, SegmentPhase, SegmentStatus, TurnSegmentTracker,
};

#[derive(Clone, Debug, Default)]
pub(crate) struct TurnTokenUsage {
    pub(crate) input_tokens: Option<i64>,
    pub(crate) cached_input_tokens: Option<i64>,
    pub(crate) cache_write_tokens: Option<i64>,
    pub(crate) reasoning_tokens: Option<i64>,
    pub(crate) output_tokens: Option<i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct TurnPerformanceTracker {
    pub(crate) provider: String,
    pub(crate) model_id: String,
    pub(crate) turn_id: String,
    pub(crate) receipt_scope: String,
    pub(crate) started_at_ms: i64,
    pub(crate) first_output_at_ms: Option<i64>,
    pub(crate) last_output_at_ms: Option<i64>,
    pub(crate) usage: TurnTokenUsage,
    /// Per-segment generation-speed measurement for this turn. Separate from
    /// every field above: those describe the turn, this describes one
    /// uninterrupted stretch of one model response.
    pub(crate) segments: TurnSegmentTracker,
}

pub(crate) type PerformanceTrackers = Arc<Mutex<HashMap<String, TurnPerformanceTracker>>>;

pub(crate) fn is_context_compaction_notification(method: &str, params: &Value) -> bool {
    method == "thread/compacted"
        || params
            .get("item")
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str)
            == Some("contextCompaction")
}

pub(crate) fn positive_i64(value: Option<i64>) -> Option<i64> {
    value.filter(|value| *value >= 0)
}

pub(crate) fn integer_field(value: &Value, aliases: &[&str]) -> Option<i64> {
    aliases
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_i64))
}

pub(crate) fn usage_from_object(value: &Value) -> Option<TurnTokenUsage> {
    let output_tokens = positive_i64(integer_field(
        value,
        &[
            "output_tokens",
            "outputTokens",
            "completion_tokens",
            "completionTokens",
        ],
    ));
    output_tokens?;
    let details = value
        .get("output_tokens_details")
        .or_else(|| value.get("outputTokensDetails"));
    Some(TurnTokenUsage {
        input_tokens: positive_i64(integer_field(
            value,
            &[
                "input_tokens",
                "inputTokens",
                "prompt_tokens",
                "promptTokens",
            ],
        )),
        cached_input_tokens: positive_i64(integer_field(
            value,
            &[
                "cached_input_tokens",
                "cachedInputTokens",
                "cached_tokens",
                "cachedTokens",
            ],
        )),
        cache_write_tokens: positive_i64(integer_field(
            value,
            &[
                "cache_write_input_tokens",
                "cacheWriteInputTokens",
                "cache_creation_input_tokens",
                "cacheCreationInputTokens",
            ],
        )),
        reasoning_tokens: positive_i64(integer_field(
            value,
            &["reasoning_output_tokens", "reasoningOutputTokens"],
        ))
        .or_else(|| {
            details.and_then(|details| {
                positive_i64(integer_field(
                    details,
                    &["reasoning_tokens", "reasoningTokens"],
                ))
            })
        }),
        output_tokens,
    })
}

pub(crate) fn extract_turn_usage(params: &Value) -> Option<TurnTokenUsage> {
    // Prefer explicitly per-turn/last-usage objects. Thread-wide totals are
    // excluded: after a restart they cannot produce an honest turn rate.
    const PATHS: &[&str] = &[
        "/lastUsage",
        "/lastTokenUsage",
        "/tokenUsage/last",
        "/tokenUsage/lastUsage",
        "/tokenUsage/lastTokenUsage",
        "/turn/lastUsage",
        "/turn/lastTokenUsage",
        "/turn/tokenUsage/last",
        "/turn/tokenUsage/lastUsage",
        "/turn/usage",
        "/usage",
    ];
    PATHS
        .iter()
        .find_map(|path| params.pointer(path).and_then(usage_from_object))
}

pub(crate) fn is_output_delta(method: &str, params: &Value) -> bool {
    let normalized = method.to_ascii_lowercase();
    let is_agent_delta = normalized.contains("agentmessage/delta")
        || normalized.contains("agent_message/delta")
        || normalized.contains("outputtext/delta")
        || normalized.contains("output_text/delta");
    is_agent_delta
        && params
            .get("delta")
            .and_then(Value::as_str)
            .is_some_and(|delta| !delta.is_empty())
}

/// Record one pump event against the turn's telemetry.
///
/// Returns the generation-speed measurements this event finalized, so the pump
/// can publish them onto the journal. They are returned rather than emitted
/// here because publishing needs the Tauri app handle and this module must stay
/// usable from tests that have no app.
///
/// `received_at_us` is taken by the caller the instant the stream frame was
/// decoded — before IPC, persistence, or any renderer work — because that
/// instant, not this function's, is what "observed delivery" means.
pub(crate) async fn track_performance_event(
    persistence: &crate::session::SessionPersistence,
    trackers: &PerformanceTrackers,
    receipts: &crate::credential_broker::ReceiptStore,
    session_id: &str,
    method: &str,
    params: &Value,
    received_at_us: i64,
) -> Vec<GenerationSpeedMeasurement> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let terminal = matches!(
        method,
        "turn/completed" | "turn/failed" | "turn/interrupted"
    );
    let finalized = {
        let mut trackers = trackers.lock().await;
        let Some(tracker) = trackers.get_mut(session_id) else {
            return Vec::new();
        };
        if is_output_delta(method, params) {
            // Turn-level output bounds: they still serve time-to-first-token
            // and end-to-end latency. They are never a generation denominator.
            tracker.first_output_at_ms.get_or_insert(now_ms);
            tracker.last_output_at_ms = Some(now_ms);
        }
        let usage_event = method.to_ascii_lowercase().contains("usage");
        let response_output_tokens = if usage_event || terminal {
            if let Some(usage) = extract_turn_usage(params) {
                let output = usage.output_tokens;
                tracker.usage = usage;
                output
            } else {
                None
            }
        } else {
            None
        };
        let mut finalized = match protocol_event(method, params) {
            Some(event) => tracker.segments.observe(event, received_at_us),
            None => Vec::new(),
        };
        if usage_event {
            if let Some(output_tokens) = response_output_tokens {
                if let Some(updated) = tracker
                    .segments
                    .apply_final_response_output_usage(output_tokens)
                {
                    finalized.push(updated);
                }
            }
        }
        finalized
    };
    for measurement in &finalized {
        if let Err(error) = persistence
            .record_generation_speed(generation_speed_row(measurement))
            .await
        {
            crate::platform::logging::report(
                "session",
                "eprintln",
                format!("generation speed measurement could not be persisted: {error:#}"),
            );
        }
    }
    let mut finalized = finalized;
    if terminal {
        let status = match method {
            "turn/completed" => RunStatus::Completed.as_str(),
            "turn/failed" => RunStatus::Failed.as_str(),
            _ => RunStatus::Interrupted.as_str(),
        };
        finalized.extend(
            finalize_performance_tracker(
                persistence,
                trackers,
                receipts,
                session_id,
                status,
                Some(now_ms),
            )
            .await,
        );
    }
    finalized
}

/// Project a measurement into its storage row. `samples` and `quality_flags`
/// ride as JSON so the evidence stays with the conclusion in one row.
pub(crate) fn generation_speed_row(measurement: &GenerationSpeedMeasurement) -> GenerationSpeedRow {
    GenerationSpeedRow {
        measurement_id: measurement.measurement_id.clone(),
        schema_version: measurement.schema_version.to_owned(),
        measurement_kind: measurement.measurement_kind.to_owned(),
        session_id: measurement.session_id.clone(),
        turn_id: measurement.turn_id.clone(),
        response_id: measurement.key.response_id.clone(),
        item_id: measurement.key.item_id.clone(),
        output_index: measurement.key.output_index,
        content_index: measurement.key.content_index,
        phase: json_label(&measurement.phase),
        status: json_label(&measurement.status),
        tps: measurement.tps,
        exact_tokens_after_first_sample: measurement.exact_tokens_after_first_sample,
        duration_ms: measurement.duration_ms,
        sample_count: measurement.sample_count as i64,
        token_count_source: json_label(&measurement.token_count_source),
        tokenizer_id: measurement.tokenizer_id.clone(),
        clock_source: json_label(&measurement.clock_source),
        unavailable_reason: measurement
            .unavailable_reason
            .as_ref()
            .map(|reason| json_label(reason)),
        quality_flags_json: serde_json::to_string(&measurement.quality_flags)
            .unwrap_or_else(|_| "[]".into()),
        samples_json: serde_json::to_string(&measurement.samples).unwrap_or_else(|_| "[]".into()),
        provider: measurement.provider.clone(),
        model_id: measurement.model_id.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// The snake_case name a serde enum serializes to, without hand-maintaining a
/// second mapping that could drift from the wire contract.
fn json_label<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

/// The one rate a per-request ledger row may carry for a turn: the completed
/// final-answer segment's own measurement.
///
/// `None` when the turn produced no such segment, or produced more than one —
/// two final answers are two measurements, and a row that can only hold one
/// number must hold none rather than a blend.
fn turn_headline_tps(tracker: &TurnPerformanceTracker) -> Option<f64> {
    let publishable: Vec<&GenerationSpeedMeasurement> = tracker
        .segments
        .measurements()
        .iter()
        .filter(|measurement| {
            measurement.is_publishable()
                && measurement.phase == SegmentPhase::FinalAnswer
                && measurement.status == SegmentStatus::Completed
        })
        .collect();
    match publishable.as_slice() {
        [only] => only.tps,
        _ => None,
    }
}

/// Close the turn: write its usage row, and return any generation-speed
/// measurements this finalize produced so the caller can publish them.
///
/// The return value matters on the crash path. When the app-server dies
/// mid-turn this is the only place a still-open segment is closed, and a
/// measurement the transcript never receives is one the user cannot see.
pub(crate) async fn finalize_performance_tracker(
    persistence: &crate::session::SessionPersistence,
    trackers: &PerformanceTrackers,
    receipts: &crate::credential_broker::ReceiptStore,
    session_id: &str,
    status: &str,
    completed_at_ms: Option<i64>,
) -> Vec<GenerationSpeedMeasurement> {
    let Some(mut tracker) = trackers.lock().await.remove(session_id) else {
        return Vec::new();
    };
    let completed_at_ms = completed_at_ms.unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    // Segments still open at finalize belong to a stream that ended without
    // closing them; they are recorded as partial, never as headline evidence.
    let finalized = tracker.segments.finish();
    for measurement in &finalized {
        if let Err(error) = persistence
            .record_generation_speed(generation_speed_row(measurement))
            .await
        {
            crate::platform::logging::report(
                "session",
                "eprintln",
                format!("generation speed measurement could not be persisted: {error:#}"),
            );
        }
    }
    let output_tokens = tracker.usage.output_tokens.filter(|tokens| *tokens > 0);
    // The ledger's throughput column now carries a real measurement or nothing.
    // Only a completed final-answer segment qualifies: it is the one segment
    // whose scope a per-request row can honestly stand for. A turn with several
    // answer segments has no single rate, and inventing one by blending them is
    // the defect this replaced.
    let observed_output_tps = turn_headline_tps(&tracker);
    // Acceptance-to-completion includes queueing, model warmup, prefill, and
    // tool time. It is latency, never generation TPS. Only the measured text
    // delivery segment above is eligible for a throughput field.
    let end_to_end_output_tps = None;
    let measurement_kind = if observed_output_tps.is_some() {
        MeasurementKind::ObservedStreamSegment
    } else {
        MeasurementKind::EndToEnd
    };
    // Usage rows record tokens even when money is unknown. Workshop never
    // invents a dollar amount from a built-in tariff: only a provider-settled
    // receipt may populate billed_cost_usd.
    let estimated_cost_usd = None;
    // Settled OpenRouter and Synth Cloud accounting is captured by the
    // credential broker as the child's responses stream through it. Only those
    // relayed providers drain receipts; local/on-device rows stay exactly as
    // the tracker built them — billed stays `None`, never $0.
    //
    // Laguna-local turns (`ProviderClass::LocalLaguna` / `local-laguna`) write
    // into this same `usage_records` ledger via finalize — tokens and
    // throughput, no dollar charge. The LagunaManager inference SSE is
    // telemetry for the Inference pane only and is intentionally exempt from
    // writing usage rows (not a session turn authority).
    //
    // Exactly-once contract: the native turn scope selects only receipts born
    // under this turn, and draining removes those receipts. A late receipt
    // keeps the old scope and is never charged to a later turn; session close
    // logs and drops anything that arrived too late to be finalized.
    let provider_class = super::home::provider_class(Some(&tracker.provider));
    let settled_cost_usd = match provider_class {
        super::home::ProviderClass::OpenRouter | super::home::ProviderClass::SynthCloud => {
            settled_cost_from_receipts(&receipts.drain_for_turn(session_id, &tracker.receipt_scope))
        }
        _ => None,
    };
    let cost_source = match (provider_class, settled_cost_usd) {
        (super::home::ProviderClass::OpenRouter, Some(_)) => CostSource::ProviderReported,
        (super::home::ProviderClass::SynthCloud, Some(_)) => CostSource::SynthCloud,
        _ => CostSource::None,
    };
    let record = UsageRecord {
        id: format!("perf:{}:{}", tracker.provider, tracker.turn_id),
        provider: tracker.provider,
        model_id: tracker.model_id,
        model_revision: None,
        session_id: Some(session_id.to_owned()),
        run_id: Some(tracker.turn_id.clone()),
        request_id: tracker.turn_id,
        measurement_kind,
        status: status.to_owned(),
        started_at_ms: tracker.started_at_ms,
        first_output_at_ms: tracker.first_output_at_ms,
        last_output_at_ms: tracker.last_output_at_ms,
        completed_at_ms,
        input_tokens: tracker.usage.input_tokens,
        cached_input_tokens: tracker.usage.cached_input_tokens,
        cache_write_tokens: tracker.usage.cache_write_tokens,
        reasoning_tokens: tracker.usage.reasoning_tokens,
        output_tokens,
        ttft_ms: tracker
            .first_output_at_ms
            .map(|first| (first - tracker.started_at_ms).max(0) as f64),
        observed_output_tps,
        end_to_end_output_tps,
        billed_cost_usd: settled_cost_usd,
        estimated_cost_usd,
        cost_source,
        source: "codex_app_server".into(),
    };
    if let Err(error) = persistence.record_usage(record).await {
        crate::platform::logging::report(
            "session",
            "eprintln",
            format!("usage record could not be persisted: {error:#}"),
        );
    }
    finalized
}

/// Sum of the settled charges a turn's receipts carried. A turn is allowed to
/// span several upstream requests, so several receipts sum into one figure.
/// `None` when no receipt reported money — token-only receipts never fabricate
/// a $0 settled charge.
pub(crate) fn settled_cost_from_receipts(
    receipts: &[crate::credential_broker::SettledReceipt],
) -> Option<f64> {
    receipts
        .iter()
        .filter_map(|receipt| receipt.cost_usd)
        .fold(None, |total, cost| Some(total.unwrap_or(0.0) + cost))
}
