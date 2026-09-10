//! Operator inspection and exact-digest resolution of live approval sheets.
//! Inspection reports broker state, never transcript guesses. Resolution is
//! human-only in desktop_policy; it does not mint a remembered permission.
use super::*;
use crate::error::AppError;

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingApprovalView {
    pub approval_id: String,
    pub session_id: String,
    pub kind: String,
    pub requires_human: bool,
    pub preparation_digest: Option<String>,
}

fn preparation_digest(kind: &ApprovalKind) -> Option<&str> {
    match kind {
        ApprovalKind::PaidCompute {
            preparation_digest, ..
        } => preparation_digest.as_deref(),
        _ => None,
    }
}

impl ApprovalBroker {
    /// Resolve only the proposal displayed by the caller. A rejection needs
    /// no digest, so a stale/incomplete card can always be dismissed.
    pub(crate) async fn decision_from_view(
        &self,
        id: &str,
        requested: &str,
        viewed_digest: Option<&str>,
    ) -> Result<ApprovalDecision> {
        let kind = self
            .pending_kind(id)
            .await
            .ok_or_else(|| anyhow!("approval is no longer pending: {id}"))?;
        if requested != "reject" {
            match (preparation_digest(&kind), viewed_digest) {
                (Some(actual), Some(viewed)) if actual == viewed => {}
                (Some(_), None) => {
                    return Err(anyhow!(
                        "paid-compute approval requires the active proposal digest"
                    ))
                }
                (Some(_), Some(_)) => return Err(anyhow!("approval digest mismatch")),
                (None, Some(_)) => {
                    return Err(anyhow!("approval is not bound to a proposal digest"))
                }
                (None, None) => {}
            }
        }
        self.decision_from_shell(id, requested).await
    }

    pub(crate) async fn pending_snapshot(&self) -> Vec<PendingApprovalView> {
        let entries = self
            .pending
            .lock()
            .await
            .iter()
            .map(|(id, pending)| (id.clone(), pending.clone()))
            .collect::<Vec<_>>();
        let mut views = Vec::new();
        for (approval_id, pending) in entries {
            // A slow resolver must not freeze the read-only operator inbox.
            if pending.settle.try_lock().is_ok_and(|settled| *settled) {
                continue;
            }
            views.push(PendingApprovalView {
                approval_id,
                session_id: pending.origin.session_id.clone(),
                kind: pending.kind.name().into(),
                requires_human: pending.kind.requires_human(),
                preparation_digest: preparation_digest(&pending.kind).map(str::to_owned),
            });
        }
        views.sort_by(|a, b| a.approval_id.cmp(&b.approval_id));
        views
    }

    pub(crate) async fn approve_digest<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        digest: &str,
    ) -> Result<String> {
        if digest.trim().is_empty() {
            return Err(anyhow!("approval digest must not be empty"));
        }
        let matched = {
            let pending = self.pending.lock().await;
            let mut matches = pending
                .iter()
                .filter(|(_, entry)| preparation_digest(&entry.kind) == Some(digest));
            let first = matches
                .next()
                .map(|(id, entry)| (id.clone(), entry.clone()));
            if matches.next().is_some() {
                return Err(anyhow!("multiple approval sheets share this digest; resolve by session and approval id"));
            }
            first
        };
        let (id, pending) =
            matched.ok_or_else(|| anyhow!("no approval sheet is open for this digest"))?;
        let ApprovalKind::PaidCompute { requested_cap, .. } = &pending.kind else {
            return Err(anyhow!(
                "approval is not a digest-bound paid-compute request"
            ));
        };
        self.resolve(
            app,
            &pending.origin.session_id,
            &id,
            ApprovalDecision::ApproveWithCap {
                cap: requested_cap.clone(),
            },
        )
        .await?;
        Ok(id)
    }
}

#[tauri::command]
#[specta::specta]
pub async fn approvals_pending(
    approvals: tauri::State<'_, Arc<ApprovalBroker>>,
) -> Result<Vec<PendingApprovalView>, AppError> {
    Ok(approvals.pending_snapshot().await)
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ApproveDigestRequest {
    pub execution_spec_digest: String,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ApproveDigestOutcome {
    pub approval_id: String,
    pub already_settled: bool,
    pub execution_spec_digest: String,
}

/// Human-only compatibility operation. Only a unique currently pending sheet
/// may settle. As on the previous implementation, a replay fails closed; the
/// legacy alreadySettled field is false, not a promise of durable idempotence.
#[tauri::command]
#[specta::specta]
pub async fn approvals_approve_digest(
    app: tauri::AppHandle,
    approvals: tauri::State<'_, Arc<ApprovalBroker>>,
    request: ApproveDigestRequest,
) -> Result<ApproveDigestOutcome, AppError> {
    let approval_id = approvals
        .approve_digest(&app, &request.execution_spec_digest)
        .await
        .map_err(AppError::from)?;
    Ok(ApproveDigestOutcome {
        approval_id,
        already_settled: false,
        execution_spec_digest: request.execution_spec_digest,
    })
}
