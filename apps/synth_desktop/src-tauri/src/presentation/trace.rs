//! The trace pane: the first implementation of the panel host's vocabulary.
//!
//! Everything trace-shaped lives here — the inspector and catalog templates,
//! the projection schema, eligibility, and the deterministic identity that
//! makes a sealed archive reuse its visual. The host in `super` owns the
//! lifecycle; this module answers only for traces.

use anyhow::{bail, Result};
use serde_json::{json, Value};

use super::{Pane, Presentability, UnavailableReason};
use crate::core_runtime::CoreRuntime;
use crate::data::TraceRecord;
use crate::visuals::{
    binding_descriptors, descriptor_input_name, VisualCreateRequest, VisualQuery, VisualRecord,
};

/// Shares the id namespace with plugins and with the `trace_*` tables.
pub(super) const PROVIDER_ID: &str = "trace";

pub const TRACE_INSPECTOR_TEMPLATE: &str = "trace.rollout_inspector.v1";
pub const TRACE_PROJECTION_SCHEMA: &str = "synth.trace-projection.rollout-inspector.v1";

/// Whether a sealed trace may be inspected, and when it may not, why — the
/// host's law that the catalog names the reason rather than silently omitting
/// the unavailable ones, answered for traces.
///
/// Mirrors `traceInspectability` in the renderer's runtime/traceInspector.ts.
pub(super) fn presentable(trace: &TraceRecord) -> Presentability {
    let metadata = &trace.metadata;
    let lower = |key: &str| {
        metadata
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_lowercase)
    };
    let validation = lower("validationStatus");
    if metadata.get("quarantined").and_then(Value::as_bool) == Some(true)
        || metadata.get("trusted").and_then(Value::as_bool) == Some(false)
        || matches!(validation.as_deref(), Some("invalid") | Some("quarantined"))
    {
        return Presentability::Unavailable(UnavailableReason::Quarantined);
    }
    if metadata.get("selfContained").and_then(Value::as_bool) == Some(false) {
        return Presentability::Unavailable(UnavailableReason::ArchiveIncomplete);
    }
    if matches!(
        lower("compatibilityLevel").as_deref(),
        Some("invalid") | Some("opaque")
    ) {
        return Presentability::Unavailable(UnavailableReason::Unsupported);
    }
    Presentability::Present
}

/// Whether one sealed trace may be inspected, through the panel host.
pub fn trace_inspectability(trace: &TraceRecord) -> Presentability {
    Pane::Trace(trace).presentable()
}

/// Deterministic per-sealed-archive identity, stable across restarts, windows,
/// and callers.
pub(super) fn visual_id(trace: &TraceRecord) -> String {
    let digest: String = trace
        .digest
        .trim_start_matches("sha256:")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.' || *c == '-')
        .take(64)
        .collect();
    if digest.is_empty() {
        let fallback: String = trace
            .id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .take(64)
            .collect();
        format!("vis_trace_{fallback}")
    } else {
        format!("vis_trace_{digest}")
    }
}

/// The inspector visual identity for one sealed trace, through the panel host.
pub fn trace_inspector_visual_id(trace: &TraceRecord) -> String {
    Pane::Trace(trace).visual_id()
}

/// The digest a visual's projection input is bound to.
pub fn trace_digest_binding(visual: &VisualRecord) -> Option<String> {
    if visual.template_id != TRACE_INSPECTOR_TEMPLATE {
        return None;
    }
    binding_descriptors(&visual.bindings)
        .ok()?
        .into_iter()
        .find_map(|slot| {
            if descriptor_input_name(&slot).ok().as_deref() == Some("projection")
                && slot.get("kind").and_then(Value::as_str) == Some("trace_v5")
            {
                slot.get("source")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            } else {
                None
            }
        })
}

fn trace_inspector_create_request(trace: &TraceRecord) -> VisualCreateRequest {
    let pane = Pane::Trace(trace);
    VisualCreateRequest {
        template_id: pane.template_id().into(),
        title: Some(trace.title.clone()),
        bindings: Some(json!({
            "schemaVersion": "synth.visual-bindings.v1",
            "inputs": [{
                "input": "projection",
                "kind": "trace_v5",
                "source": trace.digest,
                "schema": pane.projection_schema(),
            }]
        })),
        id: Some(pane.visual_id()),
        status: None,
        renderer_kind: None,
        session_id: None,
        message_id: None,
        run_id: None,
        trace_id: Some(trace.id.clone()),
        parent_visual_id: None,
        source_agent_id: None,
        source_model: None,
        content: None,
        metadata: Some(json!({
            "traceRecordId": trace.id,
            "traceDigest": trace.digest,
            "projectionSchema": pane.projection_schema(),
        })),
    }
}

/// Resolve, or create, the inspector visual for one sealed trace.
///
/// Reuse is decided by the sealed digest alone. A trace record id, run id, or
/// title is not archive identity: re-sealing a record yields a new digest, and
/// matching on the id would present the previous archive under the new name.
pub async fn ensure_trace_inspector(core: &CoreRuntime, trace_id: &str) -> Result<VisualRecord> {
    let trace = core.data().get_trace(trace_id.to_string()).await?;
    let pane = Pane::Trace(&trace);
    let presentability = pane.presentable();
    if !presentability.eligible() {
        bail!(
            "trace `{}` cannot be inspected: {}",
            trace.id,
            presentability.label()
        );
    }

    let registry = core.visuals();
    let existing = registry
        .list(VisualQuery {
            status: None,
            session_id: None,
            template_id: Some(pane.template_id().into()),
            search: None,
            limit: Some(500),
            offset: None,
        })
        .await?;
    if let Some(found) = existing.into_iter().find(|candidate| {
        candidate
            .metadata
            .get("traceDigest")
            .and_then(Value::as_str)
            == Some(trace.digest.as_str())
            || trace_digest_binding(candidate).as_deref() == Some(trace.digest.as_str())
    }) {
        return Ok(found);
    }

    let visual_id = pane.visual_id();
    match registry
        .create(trace_inspector_create_request(&trace))
        .await
    {
        Ok((visual, _event)) => Ok(visual),
        Err(error) => {
            // Another caller may have created the deterministic identity since
            // the list above. Adopt it only when it is bound to this exact
            // sealed digest; anything else is a different archive.
            let raced = registry.get(visual_id).await.ok();
            match raced {
                Some(raced)
                    if trace_digest_binding(&raced).as_deref() == Some(trace.digest.as_str()) =>
                {
                    Ok(raced)
                }
                _ => Err(error),
            }
        }
    }
}

pub const TRACE_CATALOG_TEMPLATE: &str = "trace.catalog.v1";

/// Resolve, or create, the catalog visual for one frozen query snapshot.
///
/// Identity is the snapshot id, and a snapshot is immutable, so reopening the
/// same result set always lands on the same visual and a refreshed query gets
/// its own. The binding addresses the snapshot rather than the query, which is
/// what keeps a rendered catalog from silently changing underneath the reader.
pub async fn ensure_query_catalog(core: &CoreRuntime, snapshot_id: &str) -> Result<VisualRecord> {
    let snapshot = core.data().query_snapshot(snapshot_id.to_string()).await?;
    let visual_id = format!("vis_query_{}", snapshot.snapshot_id);
    let registry = core.visuals();
    if let Ok(existing) = registry.get(visual_id.clone()).await {
        return Ok(existing);
    }

    let title = if snapshot.result_count == 1 {
        "1 trace matched".to_string()
    } else {
        format!("{} traces matched", snapshot.result_count)
    };
    let request = VisualCreateRequest {
        template_id: TRACE_CATALOG_TEMPLATE.into(),
        title: Some(title),
        bindings: Some(json!({
            "schemaVersion": "synth.visual-bindings.v1",
            "inputs": [{
                "input": "result",
                "kind": "query_snapshot",
                "source": snapshot.snapshot_id,
                "schema": crate::trace_query::TRACE_QUERY_RESULT_SCHEMA,
            }]
        })),
        id: Some(visual_id.clone()),
        status: None,
        renderer_kind: None,
        session_id: None,
        message_id: None,
        run_id: None,
        trace_id: None,
        parent_visual_id: None,
        source_agent_id: None,
        source_model: None,
        content: None,
        metadata: Some(json!({
            "querySnapshotId": snapshot.snapshot_id,
            "resultDigest": snapshot.result_digest,
            "resultCount": snapshot.result_count,
            "queriedAt": snapshot.queried_at,
            "truncated": snapshot.truncated,
        })),
    };
    match registry.create(request).await {
        Ok((visual, _event)) => Ok(visual),
        Err(error) => registry.get(visual_id).await.map_err(|_| error),
    }
}

