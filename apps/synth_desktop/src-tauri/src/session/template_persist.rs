//! Once-only consent for the exact persistent renderer package being written.
use super::approval::{ApprovalBroker, ApprovalKind};
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PersistRequest {
    pub template_id: String,
    pub destination: String,
    pub package_digest: String,
    pub byte_size: u64,
    pub overwrites: bool,
    pub source_kind: String,
}

impl PersistRequest {
    pub(crate) fn kind(&self) -> ApprovalKind {
        ApprovalKind::VisualTemplatePersist {
            template_id: self.template_id.clone(),
            destination: self.destination.clone(),
            package_digest: self.package_digest.clone(),
            byte_size: self.byte_size,
            overwrites: self.overwrites,
            source_kind: self.source_kind.clone(),
        }
    }
}

/// Not cloneable or publicly constructible: the writer consumes this proof.
pub(crate) struct PersistConsent {
    request: PersistRequest,
}

impl PersistConsent {


    pub(crate) fn bind(self, request: &PersistRequest) -> Result<()> {
        if self.request != *request {
            return Err(anyhow!("template package or destination changed after approval"));
        }
        Ok(())
    }
}

pub(crate) async fn authorize<R: tauri::Runtime>(
    app: &AppHandle<R>,
    session_id: Option<&str>,
    request: &PersistRequest,
) -> Result<PersistConsent> {
    let broker = app.try_state::<Arc<ApprovalBroker>>()
        .ok_or_else(|| anyhow!("approval broker unavailable"))?;
    broker.authorize_host(app, session_id, request.kind()).await?;
    Ok(PersistConsent { request: request.clone() })
}

pub(crate) fn unapproved() -> anyhow::Error {
    anyhow!("persisting visual template code requires a once-only visual_template_persist approval in a conversation")
}

