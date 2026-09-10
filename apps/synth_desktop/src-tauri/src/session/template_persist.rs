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
    #[cfg(test)]
    pub(crate) fn for_test(request: PersistRequest) -> Self {
        Self { request }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::approval::{ApprovalDecision, ApprovalScope};

    fn request() -> PersistRequest {
        PersistRequest { template_id: "custom.viewer.v1".into(), destination: "/approved/template".into(), package_digest: "sha256:reviewed".into(), byte_size: 12, overwrites: false, source_kind: "managed".into() }
    }

    #[test]
    fn consent_binds_every_material_field() {
        let original = request();
        let mut changes = vec![original.clone(); 6];
        changes[0].template_id = "other".into();
        changes[1].destination = "/other".into();
        changes[2].package_digest = "sha256:changed".into();
        changes[3].byte_size += 1;
        changes[4].overwrites = true;
        changes[5].source_kind = "user".into();
        for changed in changes {
            assert!(PersistConsent { request: original.clone() }.bind(&changed).is_err());
        }
        assert!(PersistConsent { request: original.clone() }.bind(&original).is_ok());
    }

    #[test]
    fn persistent_code_requires_a_person_even_under_never_policy() {
        let kind = request().kind();
        assert!(kind.requires_human());
        assert!(crate::session::approval_policy::auto_decision("never", &kind).unwrap().is_none());
        assert!(kind.validate_decision(&ApprovalDecision::Approve { scope: ApprovalScope::Session }).is_err());
        assert!(kind.validate_decision(&ApprovalDecision::Approve { scope: ApprovalScope::Once }).is_ok());
    }
}
