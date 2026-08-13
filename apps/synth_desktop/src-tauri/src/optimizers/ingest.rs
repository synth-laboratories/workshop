//! Fail-closed ingest of `optimizer_event_page.v1` into the local mirror.
//!
//! Workshop remaps the producer cursor onto the local SQLite cursor and stores
//! `sourceSequenceNumber`. Heartbeats must not appear in the page.

use super::models::OptimizerEventEnvelope;
use super::{normalize, OptimizerService};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

pub async fn ingest_event_page(
    service: &OptimizerService,
    run_id: &str,
    algorithm_id: &str,
    page: &Value,
    upstream_cursor: &mut u64,
) -> Result<usize> {
    if page.get("run_id").and_then(Value::as_str) != Some(run_id) {
        bail!("optimizer event page run_id did not match {run_id}");
    }
    let raw = page
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow!("optimizer event page omitted events"))?;
    if raw.is_empty() {
        return Ok(0);
    }
    let (events, next_cursor) = remap_page_events(
        &raw,
        run_id,
        algorithm_id,
        service.get(run_id.to_string()).await?.cursor_seq,
        *upstream_cursor,
    )?;
    if !events.is_empty() {
        service.append_events(run_id.to_string(), events).await?;
    }
    *upstream_cursor = next_cursor;
    Ok(raw.len())
}

pub fn remap_page_events(
    raw: &[Value],
    run_id: &str,
    algorithm_id: &str,
    local_cursor: u64,
    upstream_cursor: u64,
) -> Result<(Vec<OptimizerEventEnvelope>, u64)> {
    if raw.is_empty() {
        return Ok((Vec::new(), upstream_cursor));
    }
    let source_sequences = raw
        .iter()
        .map(|event| {
            event
                .get("sequence_number")
                .or_else(|| event.get("sequenceNumber"))
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("optimizer event omitted sequence_number"))
        })
        .collect::<Result<Vec<_>>>()?;
    let expected = upstream_cursor.saturating_add(1);
    if source_sequences.first().copied() != Some(expected)
        || source_sequences
            .windows(2)
            .any(|pair| pair[1] != pair[0].saturating_add(1))
    {
        bail!(
            "optimizer event endpoint returned a sequence gap after {upstream_cursor}: {source_sequences:?}"
        );
    }
    let mut events = normalize::normalize_events(raw, run_id, algorithm_id);
    if events.len() != raw.len() {
        bail!(
            "optimizer event page dropped {} events during normalize",
            raw.len() - events.len()
        );
    }
    for (index, event) in events.iter_mut().enumerate() {
        if event.optimizer_run_id != run_id {
            bail!(
                "optimizer event {} belongs to {}, not {run_id}",
                event.event_id.clone().unwrap_or_default(),
                event.optimizer_run_id
            );
        }
        if event.algorithm_id != algorithm_id {
            bail!(
                "optimizer event {} has algorithm_id {}, expected {algorithm_id}",
                event.event_id.clone().unwrap_or_default(),
                event.algorithm_id
            );
        }
        event.sequence_number = local_cursor + index as u64 + 1;
        if let Some(object) = event.raw.as_object_mut() {
            object.insert(
                "sourceSequenceNumber".into(),
                json!(source_sequences[index]),
            );
        }
    }
    let next_cursor = *source_sequences
        .last()
        .ok_or_else(|| anyhow!("optimizer event page had no source cursor"))?;
    Ok((events, next_cursor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(run_id: &str, sequence: u64, event_type: &str) -> Value {
        json!({
            "schema_version": "optimizer_event.v1",
            "type": event_type,
            "sequence_number": sequence,
            "created_at": "2026-08-12T19:40:00Z",
            "run_id": run_id,
            "algorithm_id": "sft",
            "delta": { "step": sequence, "train_loss": 1.2 }
        })
    }

    #[test]
    fn remaps_contiguous_sft_page_and_keeps_source_cursor() {
        let raw = vec![
            event("sft_hosted_1", 1, "optimizer.visual.ready"),
            event("sft_hosted_1", 2, "sft.training.metrics"),
        ];
        let (events, next) = remap_page_events(&raw, "sft_hosted_1", "sft", 4, 0).unwrap();
        assert_eq!(next, 2);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence_number, 5);
        assert_eq!(events[1].sequence_number, 6);
        assert_eq!(events[0].event_type, "optimizer.visual.ready");
        assert_eq!(events[1].event_type, "sft.training.metrics");
        assert_eq!(events[1].raw["sourceSequenceNumber"], 2);
        assert_eq!(events[1].algorithm_id, "sft");
    }

    #[test]
    fn fails_closed_on_a_gap_and_does_not_invent_sequence() {
        let raw = vec![event("sft_hosted_1", 3, "sft.training.metrics")];
        let error = remap_page_events(&raw, "sft_hosted_1", "sft", 0, 0)
            .unwrap_err()
            .to_string();
        assert!(error.contains("sequence gap"), "{error}");
    }

    #[test]
    fn refuses_goex_plugin_algorithm_id() {
        let mut raw = event("sft_hosted_1", 1, "sft.training.metrics");
        raw["algorithm_id"] = json!("goex.sft.v1");
        let error = remap_page_events(&[raw], "sft_hosted_1", "sft", 0, 0)
            .unwrap_err()
            .to_string();
        assert!(error.contains("algorithm_id"), "{error}");
    }
}
