//! Agent-facing document routes over the loopback visuals IPC.
//!
//! Mirrors `dispatch_traces`: the agent names a path, the host resolves it
//! against the conversation's workspace scope, and the same durable
//! `visual.show` event that opens a visual opens the document. The agent never
//! receives a file handle and never names a path the host has not re-resolved.
//!
//! Deliberately read-only. `document_show` is the whole agent surface plus the
//! two reads a viewer needs to navigate; there is no write route, because a
//! panel that could write would be a second, unapproved edit path beside the
//! agent's own tools.
//!
//! # Registration
//!
//! `visuals_ipc::dispatch_request` routes to this module with one guard,
//! beside the `/v1/traces` one it is modelled on:
//!
//! ```ignore
//! if path.starts_with("/v1/documents") {
//!     return crate::documents::ipc::dispatch_documents(method, path, json_body, core).await;
//! }
//! ```

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::core_runtime::CoreRuntime;
use crate::presentation;

/// Session the route acts for.
///
/// The body wins over the environment so a multiplexed agent can name the
/// conversation explicitly; `SYNTH_SESSION_ID` is the single-session fallback
/// the visual routes already use, kept identical so one rail does not have two
/// session conventions.
fn session_ref(body: &Value) -> Result<String> {
    body.get("sessionRef")
        .or_else(|| body.get("session_id"))
        .or_else(|| body.get("sessionId"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| std::env::var("SYNTH_SESSION_ID").ok())
        .filter(|value| !value.trim().is_empty())
        .context("session_id required: a document is read through its conversation's workspace")
}

fn requested_path(body: &Value) -> Result<String> {
    body.get("path")
        .or_else(|| body.get("document_path"))
        .or_else(|| body.get("documentPath"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .context("path required")
}

pub async fn dispatch_documents(
    method: &str,
    path: &str,
    body: Value,
    core: &CoreRuntime,
) -> Result<Value> {
    match (method, path) {
        ("POST", "/v1/documents/show") => {
            let session_id = session_ref(&body)?;
            let requested = requested_path(&body)?;
            let shown = crate::documents::show(core, &session_id, &requested).await?;
            Ok(json!({
                "opened": true,
                "path": shown.document.path,
                "relativePath": shown.document.relative_path,
                "language": shown.document.language,
                "truncated": shown.document.truncated,
                "contentDigest": shown.document.content_digest,
                "visualId": shown.visual.id,
                "templateId": shown.visual.template_id,
                "visual": shown.visual,
            }))
        }
        ("POST", "/v1/documents/read") => {
            let session_id = session_ref(&body)?;
            let requested = requested_path(&body)?;
            let document =
                crate::documents::read(core.storage().database(), &session_id, &requested)?;
            Ok(serde_json::to_value(document)?)
        }
        ("POST", "/v1/documents/list") => {
            let session_id = session_ref(&body)?;
            let requested = requested_path(&body)?;
            let listing =
                crate::documents::list_dir(core.storage().database(), &session_id, &requested)?;
            Ok(serde_json::to_value(listing)?)
        }
        ("GET", "/v1/documents/template") => Ok(json!({
            "templateId": presentation::DOCUMENT_VIEWER_TEMPLATE,
            "projectionSchema": presentation::DOCUMENT_PROJECTION_SCHEMA,
            "bindingKind": presentation::WORKSPACE_FILE_BINDING_KIND,
        })),
        _ => anyhow::bail!("unsupported document IPC route {method} {path}"),
    }
}
