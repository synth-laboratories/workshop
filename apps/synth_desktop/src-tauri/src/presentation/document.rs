//! The document pane: the panel host's second provider, and the one that was
//! not trace-shaped.
//!
//! A trace is an immutable sealed archive, so its pane's identity is its
//! digest. A document is a mutable place on disk, so its pane's identity is its
//! **canonical path**. That divergence is the point of this module existing
//! beside [`super::trace`] rather than as a parameterization of it: the host
//! vocabulary — presentability, deterministic identity, a declared binding, one
//! show event — held without change, while everything domain-shaped underneath
//! it moved.
//!
//! Why not path + content digest, which the design note proposed: a document is
//! edited. Digest identity would mint a fresh visual on every save, orphan the
//! pane the reader was looking at, and grow the registry by one row per
//! keystroke-batch. The digest is a *read receipt* — it travels on the read
//! result, where it describes the bytes actually rendered — not an identity.
//! The pane addresses the place; each read says what was there at the time.

use anyhow::{bail, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{Pane, Presentability, UnavailableReason};
use crate::core_runtime::CoreRuntime;
use crate::documents::{self, DocumentKind, DocumentRecord};
use crate::visuals::{
    binding_descriptors, descriptor_input_name, VisualCreateRequest, VisualRecord,
};

/// Shares the id namespace with plugins and with the `trace` pane.
pub(super) const PROVIDER_ID: &str = "document";

pub const DOCUMENT_VIEWER_TEMPLATE: &str = "document.viewer.v1";
pub const DOCUMENT_PROJECTION_SCHEMA: &str = documents::DOCUMENT_SCHEMA;

/// The binding kind a document pane declares.
///
/// This is the whole read grant: the pane may read the one path its visual
/// declares, and `workspace_read_file` re-resolves that path through the
/// session roots on every call rather than trusting the declaration. A binding
/// is what the pane is *allowed to ask for*, never what it is handed.
///
/// **Not yet admitted.** The binding vocabulary is shared by two readers, and
/// `visuals/tests/binding_envelope_contract.test.mjs` enforces that they agree:
/// the kind must be added to `visuals::models::VISUAL_BINDING_KINDS`,
/// `visuals/runtime/types.ts`'s `VisualBindingKind`, and `visuals/runtime/
/// bind.ts` in one change, or a visual persists that the renderer cannot read.
/// Until that lands, [`ensure_document_viewer`] fails at create with
/// `unsupported visual binding kind: workspace_file` — deliberately, rather
/// than smuggling the path through `metadata` where nothing validates it.
pub const WORKSPACE_FILE_BINDING_KIND: &str = "workspace_file";

/// The binding input name. One input, so the pane cannot silently address a
/// second file behind the one the reader sees in the breadcrumb.
const DOCUMENT_INPUT: &str = "document";

/// Whether a located path can be typeset in the pane, and when it cannot, why.
///
/// Pure over the record: scope was already decided when the record was located,
/// so nothing here touches the filesystem and every reason is one the catalog
/// and the pane can both show.
pub(super) fn presentable(document: &DocumentRecord) -> Presentability {
    if !document.exists {
        return Presentability::Unavailable(UnavailableReason::Missing);
    }
    if document.kind == DocumentKind::Directory {
        return Presentability::Unavailable(UnavailableReason::NotADocument);
    }
    if document.read_error.is_some() {
        return Presentability::Unavailable(UnavailableReason::Unreadable);
    }
    if !document.is_text {
        return Presentability::Unavailable(UnavailableReason::NotText);
    }
    Presentability::Present
}

/// Deterministic per-path identity, stable across restarts, windows, and
/// callers.
///
/// The path is hashed rather than sanitized into the id. A sanitized path
/// collides — `/a/b.md` and `/a_b.md` sanitize alike — and the id has a 128
/// character ceiling that real paths exceed. The path itself stays legible on
/// the visual's metadata and binding, which is where a human reading the
/// registry looks for it.
pub(super) fn visual_id(document: &DocumentRecord) -> String {
    let digest = Sha256::digest(document.path.as_bytes());
    format!("vis_doc_{:x}", digest).chars().take(48).collect()
}

/// The workspace path a document visual's pane is bound to.
///
/// Returns `None` for any other template, so a caller cannot read this
/// binding off a visual that never declared one.
pub fn document_path_binding(visual: &VisualRecord) -> Option<String> {
    if visual.template_id != DOCUMENT_VIEWER_TEMPLATE {
        return None;
    }
    binding_descriptors(&visual.bindings)
        .ok()?
        .into_iter()
        .find_map(|slot| {
            if descriptor_input_name(&slot).ok().as_deref() == Some(DOCUMENT_INPUT)
                && slot.get("kind").and_then(Value::as_str) == Some(WORKSPACE_FILE_BINDING_KIND)
            {
                slot.get("source")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            } else {
                None
            }
        })
}

fn document_viewer_create_request(
    document: &DocumentRecord,
    session_id: Option<String>,
) -> VisualCreateRequest {
    let pane = Pane::Document(document);
    VisualCreateRequest {
        template_id: pane.template_id().into(),
        title: Some(document.name.clone()),
        bindings: Some(json!({
            "schemaVersion": "synth.visual-bindings.v1",
            "inputs": [{
                "input": DOCUMENT_INPUT,
                "kind": WORKSPACE_FILE_BINDING_KIND,
                "source": document.path,
                "schema": pane.projection_schema(),
            }]
        })),
        id: Some(pane.visual_id()),
        status: None,
        renderer_kind: None,
        session_id,
        message_id: None,
        run_id: None,
        trace_id: None,
        parent_visual_id: None,
        source_agent_id: None,
        source_model: None,
        content: None,
        // Durable facts about the *place* only. The content digest is
        // deliberately absent: it would be stale the moment the file is saved,
        // and a stale receipt on a durable record is worse than no receipt.
        metadata: Some(json!({
            "documentPath": document.path,
            "documentRoot": document.root,
            "documentRelativePath": document.relative_path,
            "documentLanguage": document.language,
            "projectionSchema": pane.projection_schema(),
            "providerId": pane.provider_id(),
        })),
    }
}

/// Resolve, or create, the viewer visual for one workspace document.
///
/// `session_id` is not optional the way it is on the trace path: the scope that
/// decides whether this path may be read at all belongs to the conversation, so
/// a document viewer without one could not be created honestly.
pub async fn ensure_document_viewer(
    core: &CoreRuntime,
    session_id: &str,
    path: &str,
) -> Result<VisualRecord> {
    let document = documents::locate(core.storage().database(), session_id, path)?;
    let pane = Pane::Document(&document);
    let presentability = pane.presentable();
    if !presentability.eligible() {
        bail!(
            "`{}` cannot be shown: {} — {}",
            document.relative_path,
            presentability.label(),
            presentability_remediation(presentability)
        );
    }

    let registry = core.visuals();
    let visual_id = pane.visual_id();
    // Identity is the path, so the direct lookup is the whole reuse check —
    // there is no digest to compare and therefore no list-and-scan.
    if let Ok(existing) = registry.get(visual_id.clone()).await {
        if document_path_binding(&existing).as_deref() == Some(document.path.as_str()) {
            return Ok(existing);
        }
        // The id exists but addresses a different path: a hash collision, or a
        // record written by something that is not this provider. Either way it
        // is not this document's pane, and adopting it would show the reader
        // the wrong file under the right name.
        bail!(
            "visual `{visual_id}` already exists and is not bound to `{}`",
            document.path
        );
    }

    match registry
        .create(document_viewer_create_request(
            &document,
            Some(session_id.to_owned()),
        ))
        .await
    {
        Ok((visual, _event)) => Ok(visual),
        Err(error) => {
            // Another caller may have created the deterministic identity since
            // the lookup above. Adopt it only when it is bound to this exact
            // path.
            let raced = registry.get(visual_id).await.ok();
            match raced {
                Some(raced)
                    if document_path_binding(&raced).as_deref() == Some(document.path.as_str()) =>
                {
                    Ok(raced)
                }
                _ => Err(error),
            }
        }
    }
}

fn presentability_remediation(presentability: Presentability) -> &'static str {
    match presentability {
        Presentability::Present => "",
        Presentability::Unavailable(reason) => reason.remediation(),
    }
}

