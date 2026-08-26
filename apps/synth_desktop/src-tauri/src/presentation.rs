//! Deterministic right-panel presentation, shared by the native UI and the
//! agent-facing MCP facades.
//!
//! Visual lifecycle for a domain record — identity, eligibility, binding,
//! reuse, and the show event — lives here rather than in whichever caller got
//! there first. The renderer's `DataPage` grew its own copy of this logic; a
//! second copy on the agent path would have drifted from it immediately.

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::core_runtime::CoreRuntime;
use crate::data::TraceRecord;
use crate::visuals::{
    binding_descriptors, descriptor_input_name, VisualCreateRequest, VisualQuery, VisualRecord,
};

pub const TRACE_INSPECTOR_TEMPLATE: &str = "trace.rollout_inspector.v1";
pub const TRACE_PROJECTION_SCHEMA: &str = "synth.trace-projection.rollout-inspector.v1";

/// Why a sealed trace may not be inspected. The catalog shows every trace and
/// names the reason rather than silently omitting the unavailable ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceInspectability {
    Inspect,
    Quarantined,
    ArchiveIncomplete,
    Unsupported,
}

impl TraceInspectability {
    pub fn label(self) -> &'static str {
        match self {
            Self::Inspect => "Inspect",
            Self::Quarantined => "Quarantined",
            Self::ArchiveIncomplete => "Archive incomplete",
            Self::Unsupported => "Unsupported",
        }
    }

    pub fn eligible(self) -> bool {
        matches!(self, Self::Inspect)
    }
}

/// Mirrors `traceInspectability` in the renderer's runtime/traceInspector.ts.
pub fn trace_inspectability(trace: &TraceRecord) -> TraceInspectability {
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
        return TraceInspectability::Quarantined;
    }
    if metadata.get("selfContained").and_then(Value::as_bool) == Some(false) {
        return TraceInspectability::ArchiveIncomplete;
    }
    if matches!(
        lower("compatibilityLevel").as_deref(),
        Some("invalid") | Some("opaque")
    ) {
        return TraceInspectability::Unsupported;
    }
    TraceInspectability::Inspect
}

/// Deterministic per-sealed-archive identity, stable across restarts, windows,
/// and callers.
pub fn trace_inspector_visual_id(trace: &TraceRecord) -> String {
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
                slot.get("source").and_then(Value::as_str).map(str::to_owned)
            } else {
                None
            }
        })
}

fn trace_inspector_create_request(trace: &TraceRecord) -> VisualCreateRequest {
    VisualCreateRequest {
        template_id: TRACE_INSPECTOR_TEMPLATE.into(),
        title: Some(trace.title.clone()),
        bindings: Some(json!({
            "schemaVersion": "synth.visual-bindings.v1",
            "slots": [{
                "slot": "projection",
                "kind": "trace_v5",
                "source": trace.digest,
                "schema": TRACE_PROJECTION_SCHEMA,
            }]
        })),
        id: Some(trace_inspector_visual_id(trace)),
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
            "projectionSchema": TRACE_PROJECTION_SCHEMA,
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
    let inspectability = trace_inspectability(&trace);
    if !inspectability.eligible() {
        bail!(
            "trace `{}` cannot be inspected: {}",
            trace.id,
            inspectability.label()
        );
    }

    let registry = core.visuals();
    let existing = registry
        .list(VisualQuery {
            status: None,
            session_id: None,
            template_id: Some(TRACE_INSPECTOR_TEMPLATE.into()),
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

    let visual_id = trace_inspector_visual_id(&trace);
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
            "slots": [{
                "slot": "result",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(digest: &str, metadata: Value) -> TraceRecord {
        TraceRecord {
            id: "trace_alpha".into(),
            digest: digest.into(),
            title: "Craftax Rust · dogfood trace 3".into(),
            source: "local".into(),
            container_id: None,
            session_id: None,
            run_id: None,
            reward: None,
            metrics: json!({}),
            path: None,
            metadata,
            created_at: "2026-08-15T00:00:00Z".into(),
        }
    }

    #[test]
    fn identity_follows_the_digest_not_the_record() {
        let a = trace("sha256:aaaa1111", json!({}));
        let mut b = trace("sha256:bbbb2222", json!({}));
        b.title = "renamed".into();
        assert_eq!(trace_inspector_visual_id(&a), "vis_trace_aaaa1111");
        assert_ne!(trace_inspector_visual_id(&a), trace_inspector_visual_id(&b));
    }

    #[test]
    fn identity_matches_the_renderer_fallback_when_a_digest_is_unusable() {
        assert_eq!(
            trace_inspector_visual_id(&trace("sha256:", json!({}))),
            "vis_trace_trace_alpha"
        );
    }

    #[test]
    fn every_unavailable_reason_is_named_rather_than_hidden() {
        assert_eq!(
            trace_inspectability(&trace("d", json!({"quarantined": true}))),
            TraceInspectability::Quarantined
        );
        assert_eq!(
            trace_inspectability(&trace("d", json!({"trusted": false}))),
            TraceInspectability::Quarantined
        );
        assert_eq!(
            trace_inspectability(&trace("d", json!({"validationStatus": "INVALID"}))),
            TraceInspectability::Quarantined
        );
        assert_eq!(
            trace_inspectability(&trace("d", json!({"selfContained": false}))),
            TraceInspectability::ArchiveIncomplete
        );
        assert_eq!(
            trace_inspectability(&trace("d", json!({"compatibilityLevel": "opaque"}))),
            TraceInspectability::Unsupported
        );
        assert_eq!(
            trace_inspectability(&trace("d", json!({}))),
            TraceInspectability::Inspect
        );
    }

    #[test]
    fn a_binding_is_read_back_only_for_the_inspector_template() {
        let request = trace_inspector_create_request(&trace("sha256:aaaa1111", json!({})));
        let bindings = request.bindings.clone().unwrap();
        let visual = |template: &str| -> VisualRecord {
            serde_json::from_value(json!({
                "schemaVersion": "synth.visual.v1",
                "id": "vis_trace_aaaa1111",
                "currentRevision": 1,
                "title": "t",
                "templateId": template,
                "status": "draft",
                "rendererKind": "template",
                "bindings": bindings,
                "metadata": {},
                "createdAt": "2026-08-15T00:00:00Z",
                "updatedAt": "2026-08-15T00:00:00Z"
            }))
            .expect("visual fixture")
        };
        assert_eq!(
            trace_digest_binding(&visual(TRACE_INSPECTOR_TEMPLATE)).as_deref(),
            Some("sha256:aaaa1111")
        );
        assert_eq!(trace_digest_binding(&visual("analysis.visual.v1")), None);
    }
}
