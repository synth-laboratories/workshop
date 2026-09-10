//! Deterministic right-panel presentation, shared by the native UI and the
//! agent-facing MCP facades.
//!
//! Visual lifecycle for a domain record — identity, eligibility, binding,
//! reuse, and the show event — lives here rather than in whichever caller got
//! there first. The renderer's `DataPage` grew its own copy of this logic; a
//! second copy on the agent path would have drifted from it immediately.

use anyhow::{bail, Result};
mod document;
mod host;
pub use document::{document_path_binding, ensure_document_viewer, DOCUMENT_PROJECTION_SCHEMA,
    DOCUMENT_VIEWER_TEMPLATE, WORKSPACE_FILE_BINDING_KIND};
pub use host::{Pane, Presentability, UnavailableReason};
use serde_json::{json, Value};

use crate::core_runtime::CoreRuntime;
use crate::data::TraceRecord;
use crate::visuals::{
    binding_descriptors, descriptor_input_name, VisualCreateRequest, VisualQuery, VisualRecord,
    VisualUpdateRequest,
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
                slot.get("source")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
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
            "inputs": [{
                "input": "projection",
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

    let noun=if snapshot.query_schema_version==crate::trace_research::SCHEMA {"results"}else{"traces"};
    let title=format!("{} {noun} matched",snapshot.result_count);
    let request = VisualCreateRequest {
        template_id: TRACE_CATALOG_TEMPLATE.into(),
        title: Some(title),
        bindings: Some(json!({
            "schemaVersion": "synth.visual-bindings.v1",
            "inputs": [{
                "input": "result",
                "kind": "query_snapshot",
                "source": snapshot.snapshot_id,
                "schema": snapshot.schema_version,
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

pub const ANNOTATION_WORKBENCH_TEMPLATE: &str = "analysis.annotation_workbench.v1";
pub const ANNOTATION_WORKBENCH_SCHEMA: &str = "synth.annotation-workbench.v1";

#[derive(Clone, Debug)]
pub struct AnnotationWorkbenchRequest {
    pub trace: TraceRecord,
    pub evidence_digest: String,
    pub rubric_digest: Option<String>,
    pub campaign_id: Option<String>,
    pub title: Option<String>,
    pub session_id: Option<String>,
}

fn sanitize_id_part(value: &str, take: usize) -> String {
    value
        .trim_start_matches("sha256:")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.' || *c == '-')
        .take(take)
        .collect()
}

/// Deterministic per (trace digest, campaign) identity. A new evidence-head
/// digest revises this visual rather than minting a sibling.
pub fn annotation_workbench_visual_id(trace_digest: &str, campaign_id: Option<&str>) -> String {
    let digest = sanitize_id_part(trace_digest, 48);
    let campaign = campaign_id
        .map(|value| sanitize_id_part(value, 24))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "campaign".into());
    if digest.is_empty() {
        format!("vis_analysis_unknown_{campaign}")
    } else {
        format!("vis_analysis_{digest}_{campaign}")
    }
}

fn annotation_workbench_bindings(
    trace_digest: &str,
    evidence_digest: &str,
    rubric_digest: Option<&str>,
) -> Value {
    let mut inputs = vec![
        json!({
            "input": "trace",
            "kind": "trace_v5",
            "source": trace_digest,
        }),
        json!({
            "input": "evidence",
            "kind": "annotation_evidence_head",
            "source": evidence_digest,
            "schema": ANNOTATION_WORKBENCH_SCHEMA,
        }),
    ];
    if let Some(rubric) = rubric_digest.filter(|value| !value.is_empty()) {
        inputs.push(json!({
            "input": "rubric",
            "kind": "verifier_result_v2",
            "source": rubric,
            "schema": "synth.verifier-result.v2",
        }));
    }
    json!({
        "schemaVersion": "synth.visual-bindings.v1",
        "inputs": inputs,
    })
}

fn evidence_digest_binding(visual: &VisualRecord) -> Option<String> {
    if visual.template_id != ANNOTATION_WORKBENCH_TEMPLATE {
        return None;
    }
    binding_descriptors(&visual.bindings)
        .ok()?
        .into_iter()
        .find_map(|slot| {
            if descriptor_input_name(&slot).ok().as_deref() == Some("evidence")
                && slot.get("kind").and_then(Value::as_str) == Some("annotation_evidence_head")
            {
                slot.get("source")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            } else {
                None
            }
        })
}

/// Resolve, or create, the analysis workbench visual for one sealed evidence head.
///
/// Reuse is the (trace digest, campaign) pair. A new evidence-head digest
/// bumps the visual revision; the previous head remains in revision history.
pub async fn ensure_annotation_workbench(
    core: &CoreRuntime,
    request: AnnotationWorkbenchRequest,
) -> Result<VisualRecord> {
    let inspectability = trace_inspectability(&request.trace);
    if !inspectability.eligible() {
        bail!(
            "trace `{}` cannot be analysed: {}",
            request.trace.id,
            inspectability.label()
        );
    }
    let visual_id =
        annotation_workbench_visual_id(&request.trace.digest, request.campaign_id.as_deref());
    let bindings = annotation_workbench_bindings(
        &request.trace.digest,
        &request.evidence_digest,
        request.rubric_digest.as_deref(),
    );
    let title = request.title.clone().unwrap_or_else(|| {
        format!(
            "{} analysis",
            if request.trace.title.trim().is_empty() {
                "Trace"
            } else {
                request.trace.title.as_str()
            }
        )
    });
    let metadata = json!({
        "traceRecordId": request.trace.id,
        "traceDigest": request.trace.digest,
        "evidenceHeadDigest": request.evidence_digest,
        "rubricDigest": request.rubric_digest,
        "campaignId": request.campaign_id,
        "projectionSchema": ANNOTATION_WORKBENCH_SCHEMA,
        "visualFamily": ANNOTATION_WORKBENCH_TEMPLATE,
    });
    let registry = core.visuals();
    if let Ok(existing) = registry.get(visual_id.clone()).await {
        if evidence_digest_binding(&existing).as_deref() == Some(request.evidence_digest.as_str()) {
            return Ok(existing);
        }
        let (updated, _event) = registry
            .update(
                visual_id,
                VisualUpdateRequest {
                    title: Some(title),
                    bindings: Some(bindings),
                    status: None,
                    renderer_kind: None,
                    message_id: None,
                    run_id: request.trace.run_id.clone(),
                    trace_id: Some(request.trace.id.clone()),
                    content: None,
                    metadata: Some(metadata),
                    bump_revision: Some(true),
                },
            )
            .await?;
        return Ok(updated);
    }
    match registry
        .create(VisualCreateRequest {
            template_id: ANNOTATION_WORKBENCH_TEMPLATE.into(),
            title: Some(title),
            bindings: Some(bindings),
            id: Some(visual_id.clone()),
            status: None,
            renderer_kind: None,
            session_id: request.session_id,
            message_id: None,
            run_id: request.trace.run_id.clone(),
            trace_id: Some(request.trace.id.clone()),
            parent_visual_id: None,
            source_agent_id: None,
            source_model: None,
            content: None,
            metadata: Some(metadata),
        })
        .await
    {
        Ok((visual, _event)) => Ok(visual),
        Err(error) => registry.get(visual_id).await.map_err(|_| error),
    }
}

