//! Fail-closed ingest of `optimizer_event_page.v1` into the local mirror.
//!
//! Workshop remaps the producer cursor onto the local SQLite cursor and stores
//! `sourceSequenceNumber`. Heartbeats must not appear in the page.

use super::events::OptimizerEventDraft;
use super::models::OptimizerEventEnvelope;
use super::{normalize, OptimizerService};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};

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
    if let Some(terminal_at) = events.iter().position(|event| {
        matches!(
            event.event_type.as_str(),
            "optimizer.run.completed"
                | "optimizer.run.failed"
                | "optimizer.run.degraded"
                | "optimizer.run.cancelled"
        )
    }) {
        let terminal = events[terminal_at].clone();
        let before = events[..terminal_at].to_vec();
        let after = events[terminal_at + 1..].to_vec();
        if !before.is_empty() {
            service.append_events(run_id.to_string(), before).await?;
        }
        let detail = terminal
            .error
            .as_ref()
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .unwrap_or_else(|| match terminal.event_type.as_str() {
                "optimizer.run.degraded" => "hosted run degraded",
                "optimizer.run.failed" => "hosted run failed",
                _ => "hosted run settled",
            })
            .to_string();
        let cause = match terminal.event_type.as_str() {
            "optimizer.run.completed" => super::kernel::SettleCause::Completed,
            "optimizer.run.degraded" => super::kernel::SettleCause::Degraded {
                detail: detail.clone(),
            },
            "optimizer.run.cancelled" => super::kernel::SettleCause::Cancelled {
                request: std::sync::Arc::new(super::kernel::CancellationRequest::new(
                    super::kernel::CancellationCause::ContainerRequested,
                    "hosted-ingest:remote",
                    format!("run:{run_id}"),
                )),
            },
            _ => super::kernel::SettleCause::Failed {
                detail: detail.clone(),
            },
        };
        service
            .settle_run(run_id.to_string(), cause, terminal.error.clone())
            .await?;
        if !after.is_empty() {
            let terminal_sequence = service
                .terminal_manifest(run_id.to_string())
                .await?
                .and_then(|manifest| manifest.get("terminalCursor").and_then(Value::as_u64))
                .ok_or_else(|| anyhow!("hosted ingest terminal seal omitted terminalCursor"))?;
            let amendments = after
                .into_iter()
                .map(|fact| {
                    OptimizerEventDraft::new("optimizer.evidence.amended", algorithm_id)
                        .idempotency_key(format!(
                            "hosted-ingest-post-terminal:{}",
                            fact.event_id
                                .clone()
                                .unwrap_or_else(|| fact.sequence_number.to_string())
                        ))
                        .delta(Map::from_iter([
                            ("terminalSequence".into(), json!(terminal_sequence)),
                            ("postTerminalFact".into(), json!(fact)),
                        ]))
                        .raw(json!({"source":"hosted_event_ingest"}))
                })
                .collect();
            service
                .append_event_payloads(run_id.to_string(), amendments)
                .await?;
        }
    } else if !events.is_empty() {
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

