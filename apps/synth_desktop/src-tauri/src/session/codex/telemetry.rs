//! Turn performance / usage telemetry for the Codex app-server pump.
use crate::domain::RunStatus;
use crate::storage::{CostSource, MeasurementKind, UsageRecord};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use super::home::ProviderClass;

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
    pub(crate) started_at_ms: i64,
    pub(crate) first_output_at_ms: Option<i64>,
    pub(crate) last_output_at_ms: Option<i64>,
    pub(crate) usage: TurnTokenUsage,
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
        reasoning_tokens: details.and_then(|details| {
            positive_i64(integer_field(
                details,
                &["reasoning_tokens", "reasoningTokens"],
            ))
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

pub(crate) async fn track_performance_event(
    persistence: &crate::session::SessionPersistence,
    trackers: &PerformanceTrackers,
    receipts: &crate::credential_broker::ReceiptStore,
    session_id: &str,
    method: &str,
    params: &Value,
) {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let terminal = matches!(
        method,
        "turn/completed" | "turn/failed" | "turn/interrupted"
    );
    {
        let mut trackers = trackers.lock().await;
        let Some(tracker) = trackers.get_mut(session_id) else {
            return;
        };
        if is_output_delta(method, params) {
            tracker.first_output_at_ms.get_or_insert(now_ms);
            tracker.last_output_at_ms = Some(now_ms);
        }
        if method.to_ascii_lowercase().contains("usage") || terminal {
            if let Some(usage) = extract_turn_usage(params) {
                tracker.usage = usage;
            }
        }
    }
    if terminal {
        let status = match method {
            "turn/completed" => RunStatus::Completed.as_str(),
            "turn/failed" => RunStatus::Failed.as_str(),
            _ => RunStatus::Interrupted.as_str(),
        };
        finalize_performance_tracker(
            persistence,
            trackers,
            receipts,
            session_id,
            status,
            Some(now_ms),
        )
        .await;
    }
}

pub(crate) async fn finalize_performance_tracker(
    persistence: &crate::session::SessionPersistence,
    trackers: &PerformanceTrackers,
    receipts: &crate::credential_broker::ReceiptStore,
    session_id: &str,
    status: &str,
    completed_at_ms: Option<i64>,
) {
    let Some(tracker) = trackers.lock().await.remove(session_id) else {
        return;
    };
    let completed_at_ms = completed_at_ms.unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let output_tokens = tracker.usage.output_tokens.filter(|tokens| *tokens > 0);
    let stream_seconds = tracker
        .first_output_at_ms
        .zip(tracker.last_output_at_ms)
        .map(|(first, last)| (last - first) as f64 / 1_000.0)
        .filter(|seconds| *seconds > 0.0);
    let end_to_end_seconds = ((completed_at_ms - tracker.started_at_ms) as f64 / 1_000.0).max(0.0);
    let observed_output_tps = output_tokens
        .zip(stream_seconds)
        .map(|(tokens, seconds)| tokens as f64 / seconds);
    let end_to_end_output_tps = output_tokens
        .filter(|_| end_to_end_seconds > 0.0)
        .map(|tokens| tokens as f64 / end_to_end_seconds);
    let measurement_kind = if observed_output_tps.is_some() {
        MeasurementKind::ObservedStream
    } else {
        MeasurementKind::EndToEnd
    };
    // A failed or interrupted turn still consumed whatever the provider
    // reported, so it is recorded — and estimated — like any other request.
    let estimated_cost_usd = crate::tariffs::estimate_cost_usd(
        &tracker.provider,
        &tracker.model_id,
        completed_at_ms,
        crate::tariffs::BillableTokens {
            input_tokens: tracker.usage.input_tokens,
            cached_input_tokens: tracker.usage.cached_input_tokens,
            cache_write_tokens: tracker.usage.cache_write_tokens,
            output_tokens: tracker.usage.output_tokens,
        },
    );
    // Settled Synth Cloud accounting, captured by the credential broker as the
    // child's responses streamed through it. Only cloud turns drain: local /
    // on-device providers have no provider charge and their rows stay exactly
    // as the tracker built them — billed stays `None`, never $0.
    //
    // Laguna-local turns (`ProviderClass::LocalLaguna` / `local-laguna`) write
    // into this same `usage_records` ledger via finalize — tokens and
    // throughput, no dollar charge. The LagunaManager inference SSE is
    // telemetry for the Inference pane only and is intentionally exempt from
    // writing usage rows (not a session turn authority).
    //
    // Exactly-once contract: draining removes the receipts, and the
    // `(provider, request_id)` upsert key dedupes a replayed finalize. A
    // receipt landing after this drain (cancellation race) stays queued no
    // longer than the session's next finalize; if the session closes first,
    // the broker logs one line and drops it rather than inventing a row.
    // The drain reads the injected receipt store — it never starts a broker.
    let settled_cost_usd = if super::home::provider_class(Some(&tracker.provider))
        == super::home::ProviderClass::SynthCloud
    {
        settled_cost_from_receipts(&receipts.drain(session_id))
    } else {
        None
    };
    // A settled receipt is authoritative; the tariff figure stays in
    // `estimated_cost_usd` and must never override it.
    let cost_source = if settled_cost_usd.is_some() {
        CostSource::SynthCloud
    } else if estimated_cost_usd.is_some() {
        CostSource::TariffEstimate
    } else {
        CostSource::None
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
        eprintln!("usage record could not be persisted: {error:#}");
    }
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
