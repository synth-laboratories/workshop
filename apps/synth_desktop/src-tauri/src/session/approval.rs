//! Origin-neutral approval lifecycle and policy.
//!
//! The broker owns pending state and durable lifecycle events. Protocol-specific
//! delivery (Codex JSON-RPC today, local oneshots later) lives behind
//! [`ApprovalResolver`].

use crate::session::SessionPersistence;
use crate::storage::EventSource;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tauri::AppHandle;
use tokio::sync::Mutex;

pub(crate) type ResolverFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ApprovalDelivery>> + Send + 'a>>;

pub(crate) const APPROVAL_REQUESTED_KIND: &str = "approval.requested";

/// Every durable kind that settles a request. Restore both queries and matches
/// on this one list — a terminal kind added in only one of those places would
/// silently resurrect settled approvals and re-expire them on next start.
pub(crate) const APPROVAL_TERMINAL_KINDS: [&str; 3] =
    ["approval.granted", "approval.rejected", "approval.expired"];

/// Protocol-specific delivery only. Persistence, policy, expiry and redaction
/// remain broker responsibilities.
pub(crate) trait ApprovalResolver: Send + Sync {
    fn resolve<'a>(&'a self, decision: &'a ApprovalDecision) -> ResolverFuture<'a>;

    /// Best-effort release when the request's origin dies. A resolver should
    /// reject or otherwise unblock its peer; the broker terminalizes the card
    /// even when a dead peer can no longer receive that release.
    fn expire<'a>(&'a self, reason: &'a str) -> ResolverFuture<'a>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ApprovalDelivery {
    pub resolver_decision: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ApprovalScope {
    Once,
    Session,
    Workspace,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PaidComputeCap {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd_micros: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rollouts: Option<u64>,
}

impl PaidComputeCap {
    pub(crate) fn is_bounded(&self) -> bool {
        self.max_cost_usd_micros.is_some_and(|value| value > 0)
            || self.max_rollouts.is_some_and(|value| value > 0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub(crate) enum ApprovalDecision {
    Reject,
    Approve { scope: ApprovalScope },
    ApproveWithCap { cap: PaidComputeCap },
    Credential { outcome: CredentialDecision },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CredentialConsent {
    RememberLocator,
    RegisterSource,
    IssueLease,
}

impl CredentialConsent {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RememberLocator => "remember_locator",
            Self::RegisterSource => "register_source",
            Self::IssueLease => "issue_lease",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CredentialDecision {
    RememberLocator,
    RegisterSource,
    IssueLease,
}

impl CredentialDecision {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RememberLocator => "remember-locator",
            Self::RegisterSource => "register-source",
            Self::IssueLease => "issue-lease",
        }
    }
}

impl ApprovalDecision {
    pub(crate) fn from_shell_wire(value: &str) -> Result<Self> {
        match value {
            "reject" => Ok(Self::Reject),
            "once" => Ok(Self::Approve {
                scope: ApprovalScope::Once,
            }),
            "always" | "always-this-session" => Ok(Self::Approve {
                scope: ApprovalScope::Session,
            }),
            "always-this-workspace" => Ok(Self::Approve {
                scope: ApprovalScope::Workspace,
            }),
            _ => Err(anyhow!("unsupported approval decision: {value}")),
        }
    }

    pub(crate) fn event_value(&self) -> &'static str {
        match self {
            Self::Reject => "reject",
            Self::Approve {
                scope: ApprovalScope::Once,
            } => "once",
            Self::Approve {
                scope: ApprovalScope::Session,
            } => "always-this-session",
            Self::Approve {
                scope: ApprovalScope::Workspace,
            } => "always-this-workspace",
            Self::ApproveWithCap { .. } => "approve-with-cap",
            Self::Credential { outcome } => outcome.as_str(),
        }
    }

    fn remembered_scope(&self) -> Option<&ApprovalScope> {
        match self {
            Self::Approve { scope }
                if matches!(scope, ApprovalScope::Session | ApprovalScope::Workspace) =>
            {
                Some(scope)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ApprovalKind {
    ShellCommand {
        request_method: String,
        detail: String,
        scope: Option<String>,
        always_supported: bool,
    },
    PaidCompute {
        operation: String,
        parameters: Value,
        estimated_cost_usd_micros: Option<u64>,
        requested_cap: PaidComputeCap,
        requesting_agent: String,
        recipe_id: Option<String>,
        dataset: Option<String>,
        proposer_model: Option<String>,
        evaluator_model: Option<String>,
        timeout_seconds: Option<u64>,
        credential_names: Vec<String>,
        preparation_digest: Option<String>,
    },
    SidecarLifecycle {
        sidecar: String,
        action: String,
    },
    ContainerLifecycle {
        container_id: String,
        declaration_id: String,
        declaration_digest: String,
        manifest_path: String,
        source_root: String,
        source_revision: Option<String>,
        source_digest: Option<String>,
        action: String,
        effect: String,
    },
    PluginLifecycle {
        plugin_id: String,
        action: String,
        version: Option<String>,
        publisher: String,
        digest: Option<String>,
        download_size_bytes: Option<u64>,
        network_host: Option<String>,
        service_effect: String,
        active_runs: u64,
        retention: String,
        always_supported: bool,
    },
    CredentialAccess {
        consent: CredentialConsent,
        provider: String,
        purpose: String,
        locator_id: Option<String>,
        display_path: Option<String>,
        variable: Option<String>,
        switch_from_display: Option<String>,
    },
    /// One computer-use action against one native app. See `docs/COMPUTER_USE.md`.
    ///
    /// `hazard` marks an action whose effect leaves the machine or cannot be
    /// undone from inside the app — sending a message, submitting a form,
    /// confirming a payment. Consent for those is bound to `payload`, not to the
    /// app, because "you may drive Mail" is not consent to send *this* mail.
    ComputerUse {
        /// Bundle identifier of the target app.
        app: String,
        /// Action verb from the vocabulary in `docs/COMPUTER_USE.md` §5.
        action: String,
        /// What the action will actually do, already redacted by the producer:
        /// recipient, text, destination. Shown verbatim on the card.
        payload: Value,
        hazard: bool,
        /// Accessibility element the action targets, when it targets one.
        element_index: Option<u64>,
    },
}

impl ApprovalKind {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::ShellCommand { .. } => "shell_command",
            Self::PaidCompute { .. } => "paid_compute",
            Self::SidecarLifecycle { .. } => "sidecar_lifecycle",
            Self::ContainerLifecycle { .. } => "container_lifecycle",
            Self::PluginLifecycle { .. } => "plugin_lifecycle",
            Self::CredentialAccess { .. } => "credential_access",
            Self::ComputerUse { .. } => "computer_use",
        }
    }

    /// Requests no policy may settle and no remembered grant may satisfy.
    ///
    /// The permissive `approval_policy = "never"` is honored everywhere else in
    /// Workshop, deliberately: the operator asked for it. It is not honored here.
    /// A hazard action commits content on the operator's behalf, paid compute
    /// commits a new digest-bound spend, and credential access creates a new
    /// run-scoped provider capability. Consent is about that exact payload — a
    /// permissive shell policy is not a substitute for any of these grants.
    ///
    /// This is the single owner of that judgment. Both policy engines and the
    /// remembered-grant path consult it rather than re-deriving it.
    pub(crate) fn requires_human(&self) -> bool {
        matches!(
            self,
            Self::ComputerUse { hazard: true, .. }
                | Self::PaidCompute { .. }
                | Self::CredentialAccess { .. }
                | Self::ContainerLifecycle { .. }
        )
    }

    fn source(&self) -> EventSource {
        // These receipts belong to the agent session whose mutation is being
        // gated. Keeping every typed kind on the Codex session stream makes
        // the request, decision, and restart terminalization observable by
        // the same live UI path as shell approvals.
        EventSource::Codex
    }

    pub(crate) fn validate_decision(&self, decision: &ApprovalDecision) -> Result<()> {
        if decision.remembered_scope().is_some()
            && (self.requires_human()
                || matches!(
                    self,
                    Self::PaidCompute { .. } | Self::CredentialAccess { .. }
                ))
        {
            return Err(anyhow!("{} approvals cannot be remembered", self.name()));
        }
        match (self, decision) {
            (_, ApprovalDecision::Reject) => Ok(()),
            // A remembered scope the origin never offered cannot be delivered.
            // Refusing here keeps the failure at the policy boundary with a
            // usable message, instead of surfacing a decision-translation
            // error from the resolver after the user has already chosen.
            (
                Self::ShellCommand {
                    always_supported: false,
                    ..
                },
                ApprovalDecision::Approve { scope },
            ) if matches!(scope, ApprovalScope::Session | ApprovalScope::Workspace) => {
                Err(anyhow!(
                    "this request does not offer a remembered approval; approve once or reject"
                ))
            }
            (Self::ShellCommand { .. }, ApprovalDecision::Approve { .. }) => Ok(()),
            (Self::SidecarLifecycle { .. }, ApprovalDecision::Approve { .. }) => Ok(()),
            (
                Self::ContainerLifecycle { .. },
                ApprovalDecision::Approve {
                    scope: ApprovalScope::Once,
                },
            ) => Ok(()),
            (Self::PluginLifecycle { .. }, ApprovalDecision::Approve { .. }) => Ok(()),
            // Remembered scopes on a hazard action were already refused above,
            // so what reaches here is either a once-off hazard approval or an
            // app-scope grant, and both are valid.
            (Self::ComputerUse { .. }, ApprovalDecision::Approve { .. }) => Ok(()),
            (
                Self::CredentialAccess {
                    consent: CredentialConsent::RememberLocator,
                    ..
                },
                ApprovalDecision::Credential {
                    outcome: CredentialDecision::RememberLocator,
                },
            )
            | (
                Self::CredentialAccess {
                    consent: CredentialConsent::RegisterSource,
                    ..
                },
                ApprovalDecision::Credential {
                    outcome:
                        CredentialDecision::RememberLocator | CredentialDecision::RegisterSource,
                },
            )
            | (
                Self::CredentialAccess {
                    consent: CredentialConsent::IssueLease,
                    ..
                },
                ApprovalDecision::Credential {
                    outcome: CredentialDecision::IssueLease,
                },
            ) => Ok(()),
            (Self::PaidCompute { .. }, ApprovalDecision::ApproveWithCap { cap })
                if cap.is_bounded() =>
            {
                Ok(())
            }
            (Self::PaidCompute { .. }, ApprovalDecision::ApproveWithCap { .. }) => {
                Err(anyhow!("paid_compute approval requires a non-zero cap"))
            }
            (Self::CredentialAccess { .. }, _) => Err(crate::secrets::lease::CredentialError::new(
                crate::secrets::lease::CREDENTIAL_DECISION_EXCEEDS_REQUEST,
                "approval",
                false,
                "credential decision exceeds the requested consent",
            )
            .anyhow()),
            _ => Err(anyhow!(
                "{} does not support decision {}",
                self.name(),
                decision.event_value()
            )),
        }
    }

    pub(crate) fn safe_payload(&self, approval_id: &str) -> Value {
        match self {
            Self::ShellCommand {
                request_method,
                detail,
                scope,
                always_supported,
            } => json!({
                "approvalId": approval_id,
                "requestMethod": request_method,
                "kind": self.name(),
                "detail": detail,
                "scope": scope,
                "alwaysSupported": always_supported,
            }),
            Self::PaidCompute {
                operation,
                parameters,
                estimated_cost_usd_micros,
                requested_cap,
                requesting_agent,
                recipe_id,
                dataset,
                proposer_model,
                evaluator_model,
                timeout_seconds,
                credential_names,
                preparation_digest,
            } => json!({
                "approvalId": approval_id,
                "kind": self.name(),
                "operation": operation,
                "parameters": parameters,
                "estimatedCostUsdMicros": estimated_cost_usd_micros,
                "requestedCap": requested_cap,
                "requestingAgent": requesting_agent,
                "recipeId": recipe_id,
                "dataset": dataset,
                "proposerModel": proposer_model,
                "evaluatorModel": evaluator_model,
                "timeoutSeconds": timeout_seconds,
                "credentialNames": credential_names,
                "preparationDigest": preparation_digest,
                "alwaysSupported": false,
            }),
            Self::SidecarLifecycle { sidecar, action } => json!({
                "approvalId": approval_id,
                "kind": self.name(),
                "sidecar": sidecar,
                "action": action,
                "alwaysSupported": true,
            }),
            Self::ContainerLifecycle {
                container_id,
                declaration_id,
                declaration_digest,
                manifest_path,
                source_root,
                source_revision,
                source_digest,
                action,
                effect,
            } => json!({
                "approvalId": approval_id,
                "kind": self.name(),
                "containerId": container_id,
                "declarationId": declaration_id,
                "declarationDigest": declaration_digest,
                "manifestPath": manifest_path,
                "sourceRoot": source_root,
                "sourceRevision": source_revision,
                "sourceDigest": source_digest,
                "action": action,
                "effect": effect,
                "alwaysSupported": false,
            }),
            Self::PluginLifecycle {
                plugin_id,
                action,
                version,
                publisher,
                digest,
                download_size_bytes,
                network_host,
                service_effect,
                active_runs,
                retention,
                always_supported,
            } => json!({
                "approvalId": approval_id,
                "kind": self.name(),
                "pluginId": plugin_id,
                "action": action,
                "version": version,
                "publisher": publisher,
                "digest": digest,
                "downloadSizeBytes": download_size_bytes,
                "networkHost": network_host,
                "serviceEffect": service_effect,
                "activeRuns": active_runs,
                "retention": retention,
                "alwaysSupported": always_supported,
            }),
            Self::CredentialAccess {
                consent,
                provider,
                purpose,
                locator_id,
                display_path,
                variable,
                switch_from_display,
            } => json!({
                "approvalId": approval_id,
                "kind": self.name(),
                "consent": consent.as_str(),
                "provider": provider,
                "purpose": purpose,
                "locatorId": locator_id,
                "displayPath": display_path,
                "variable": variable,
                "switchFromDisplay": switch_from_display,
                "alwaysSupported": false,
            }),
            Self::ComputerUse {
                app,
                action,
                payload,
                hazard,
                element_index,
            } => json!({
                "approvalId": approval_id,
                "kind": self.name(),
                "app": app,
                "action": action,
                // The card shows this. An empty payload on a hazard action is a
                // producer bug, not a reason for the card to omit the field.
                "payload": payload,
                "hazard": hazard,
                "elementIndex": element_index,
                "alwaysSupported": !hazard,
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ApprovalOrigin {
    pub session_id: String,
    /// Generation fence for the process or local mutation that requested the
    /// approval. An old attachment must never expire a replacement's request.
    pub instance_id: String,
}

struct PendingApproval {
    origin: ApprovalOrigin,
    kind: ApprovalKind,
    resolver: Arc<dyn ApprovalResolver>,
    /// Settles exactly once, and is held across delivery. A concurrent expiry
    /// for *this* approval waits here and then observes the settled flag, so
    /// resolve and expire cannot both terminalize one request. Scoping the
    /// guard per approval rather than to the whole pending map means a wedged
    /// resolver peer stalls its own request, not every session's approvals.
    settle: Mutex<bool>,
}

pub(crate) struct ApprovalBroker {
    pending: Mutex<HashMap<String, Arc<PendingApproval>>>,
    session_grants: Mutex<HashSet<(String, String)>>,
    /// Effective policy sealed at session start. A restarted session
    /// overwrites its entry; host authorization reads this instead of
    /// re-reading machine config so the layers cannot diverge mid-session.
    effective_policies: Mutex<HashMap<String, super::approval_policy::EffectiveApprovalProfile>>,
    persistence: SessionPersistence,
    restore_started: AtomicBool,
    /// Serializes conversation-budget reserve/release so concurrent host
    /// authorizations cannot oversubscribe even before SQLite queues them.
    paid_compute_lock: Mutex<()>,
}

impl ApprovalBroker {
    pub(crate) fn new(persistence: SessionPersistence) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            session_grants: Mutex::new(HashSet::new()),
            effective_policies: Mutex::new(HashMap::new()),
            persistence,
            restore_started: AtomicBool::new(false),
            paid_compute_lock: Mutex::new(()),
        }
    }

    /// Seal the profile a session start resolved and persist the atomic
    /// `approval.policy.effective` receipt for it.
    pub(crate) async fn record_policy_effective<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: &str,
        profile: super::approval_policy::EffectiveApprovalProfile,
    ) -> Result<()> {
        self.persistence
            .append_boundary_event(
                app,
                session_id.to_owned(),
                EventSource::Codex,
                "approval.policy.effective",
                profile.receipt_payload(session_id),
            )
            .await?;
        if let Some(database) = self.persistence.database() {
            let session = session_id.to_owned();
            let policy = profile.paid_compute.clone();
            database
                .run_transaction(move |conn| {
                    super::paid_compute_budget::seed_conversation_budget(conn, &session, &policy)
                })
                .await?;
        }
        self.effective_policies
            .lock()
            .await
            .insert(session_id.to_owned(), profile);
        Ok(())
    }

    pub(crate) async fn effective_policy(&self, session_id: &str) -> Option<String> {
        self.effective_policies
            .lock()
            .await
            .get(session_id)
            .map(|profile| profile.approval_policy.clone())
    }

    pub(crate) async fn request<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        origin: ApprovalOrigin,
        kind: ApprovalKind,
        resolver: Arc<dyn ApprovalResolver>,
    ) -> Result<String> {
        let approval_id = format!("approval-{}", uuid::Uuid::new_v4().simple());
        let payload = kind.safe_payload(&approval_id);
        let source = kind.source();
        self.pending.lock().await.insert(
            approval_id.clone(),
            Arc::new(PendingApproval {
                origin: origin.clone(),
                kind,
                resolver: resolver.clone(),
                settle: Mutex::new(false),
            }),
        );
        if let Err(error) = self
            .persistence
            .append_boundary_event(
                app,
                origin.session_id,
                source,
                "approval.requested",
                payload,
            )
            .await
        {
            self.pending.lock().await.remove(&approval_id);
            let _ = resolver.expire("request_not_persisted").await;
            return Err(error);
        }
        Ok(approval_id)
    }

    /// Persist a policy-authorized request as a receipt even though no modal
    /// needs to be shown. Permissive policy must never mean unaudited work.
    pub(crate) async fn record_auto<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: &str,
        kind: &ApprovalKind,
        decision: &ApprovalDecision,
        policy: &str,
    ) -> Result<String> {
        kind.validate_decision(decision)?;
        let approval_id = format!("approval-auto-{}", uuid::Uuid::new_v4().simple());
        self.write_auto_grant(app, session_id, &approval_id, kind, decision, policy, None)
            .await?;
        Ok(approval_id)
    }

    async fn write_auto_grant<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: &str,
        approval_id: &str,
        kind: &ApprovalKind,
        decision: &ApprovalDecision,
        policy: &str,
        extra: Option<Value>,
    ) -> Result<()> {
        kind.validate_decision(decision)?;
        let mut payload = kind.safe_payload(approval_id);
        payload["decision"] = json!(decision.event_value());
        payload["policyAuto"] = json!(true);
        payload["approvalPolicy"] = json!(policy);
        if let ApprovalDecision::ApproveWithCap { cap } = decision {
            payload["cap"] = serde_json::to_value(cap)?;
        }
        if let Some(extra) = extra {
            if let (Some(object), Some(extra_object)) = (payload.as_object_mut(), extra.as_object())
            {
                for (key, value) in extra_object {
                    object.insert(key.clone(), value.clone());
                }
            }
        }
        self.persistence
            .append_boundary_event(
                app,
                session_id.to_owned(),
                kind.source(),
                "approval.granted",
                payload,
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn resolve<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: &str,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> Result<ApprovalDelivery> {
        // Take a handle out of the map, then arbitrate on this approval's own
        // settle guard. A concurrent expiry sweep for the same request blocks
        // on that guard rather than on every other session's approvals; a
        // failed delivery leaves the request unsettled and still present, so
        // the sweep can terminalize it instead of the entry being reinserted
        // after the sweep has already passed.
        let pending = {
            let pending_set = self.pending.lock().await;
            pending_set
                .get(approval_id)
                .cloned()
                .ok_or_else(|| anyhow!("approval is no longer pending: {approval_id}"))?
        };
        if pending.origin.session_id != session_id {
            return Err(anyhow!("approval does not belong to session {session_id}"));
        }
        pending.kind.validate_decision(&decision)?;
        let mut settled = pending.settle.lock().await;
        if *settled {
            return Err(anyhow!("approval is no longer pending: {approval_id}"));
        }
        let delivery = pending.resolver.resolve(&decision).await?;
        if matches!(
            decision,
            ApprovalDecision::Approve {
                scope: ApprovalScope::Session
            }
        ) {
            if let Some(key) = remembered_key(&pending.kind) {
                self.session_grants
                    .lock()
                    .await
                    .insert((session_id.to_owned(), key));
            }
        }
        *settled = true;
        drop(settled);
        self.pending.lock().await.remove(approval_id);
        let event_kind = if matches!(decision, ApprovalDecision::Reject) {
            "approval.rejected"
        } else {
            "approval.granted"
        };
        let mut payload = json!({
            "approvalId": approval_id,
            "kind": pending.kind.name(),
            "decision": decision.event_value(),
        });
        if let ApprovalDecision::ApproveWithCap { cap } = &decision {
            payload["cap"] = serde_json::to_value(cap)?;
        }
        if let Some(value) = delivery.resolver_decision.as_deref() {
            payload["resolverDecision"] = Value::String(value.to_owned());
            // Compatibility for the current shell-approval transcript.
            if matches!(pending.kind, ApprovalKind::ShellCommand { .. }) {
                payload["appServerDecision"] = Value::String(value.to_owned());
            }
        }
        self.persistence
            .append_boundary_event(
                app,
                session_id.to_owned(),
                pending.kind.source(),
                event_kind,
                payload,
            )
            .await?;
        Ok(delivery)
    }

    pub(crate) async fn expire_origin<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        origin: &ApprovalOrigin,
        reason: &str,
    ) -> Result<usize> {
        let expired = {
            let mut pending = self.pending.lock().await;
            let ids = pending
                .iter()
                .filter_map(|(id, value)| (value.origin == *origin).then_some(id.clone()))
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| pending.remove(&id).map(|value| (id, value)))
                .collect::<Vec<_>>()
        };
        let mut count = 0usize;
        for (approval_id, pending) in expired {
            // A resolve that is mid-delivery holds this guard. Waiting for it
            // and then observing the settled flag is what keeps one request
            // from producing both a granted and an expired terminal event.
            let mut settled = pending.settle.lock().await;
            if *settled {
                continue;
            }
            *settled = true;
            drop(settled);
            count += 1;
            let delivery = pending.resolver.expire(reason).await.ok();
            let mut payload = json!({
                "approvalId": approval_id,
                "kind": pending.kind.name(),
                "decision": "expired",
                "reason": reason,
            });
            if let Some(value) = delivery.and_then(|value| value.resolver_decision) {
                payload["resolverDecision"] = Value::String(value);
            }
            self.persistence
                .append_boundary_event(
                    app,
                    origin.session_id.clone(),
                    pending.kind.source(),
                    "approval.expired",
                    payload,
                )
                .await?;
        }
        Ok(count)
    }

    pub(crate) async fn expire_credential_locator<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        locator_id: &str,
        reason: &str,
    ) -> Result<usize> {
        let expired = {
            let mut pending = self.pending.lock().await;
            let ids = pending
                .iter()
                .filter_map(|(id, value)| {
                    matches!(
                        &value.kind,
                        ApprovalKind::CredentialAccess {
                            locator_id: Some(pending_locator),
                            ..
                        } if pending_locator == locator_id
                    )
                    .then_some(id.clone())
                })
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| pending.remove(&id).map(|value| (id, value)))
                .collect::<Vec<_>>()
        };
        let mut count = 0;
        for (approval_id, pending) in expired {
            let mut settled = pending.settle.lock().await;
            if *settled {
                continue;
            }
            *settled = true;
            drop(settled);
            count += 1;
            let _ = pending.resolver.expire(reason).await;
            self.persistence
                .append_boundary_event(
                    app,
                    pending.origin.session_id.clone(),
                    pending.kind.source(),
                    "approval.expired",
                    json!({
                        "approvalId": approval_id,
                        "kind": pending.kind.name(),
                        "decision": "expired",
                        "reason": reason,
                    }),
                )
                .await?;
        }
        Ok(count)
    }

    /// Drain every origin attached to a session. Interrupt and terminal turn
    /// events use this so Workshop-owned waiters cannot outlive the agent turn
    /// merely because their origin generation is not the Codex attachment id.
    pub(crate) async fn expire_session<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: &str,
        reason: &str,
    ) -> Result<usize> {
        let origins = self
            .pending
            .lock()
            .await
            .values()
            .filter(|pending| pending.origin.session_id == session_id)
            .map(|pending| pending.origin.clone())
            .collect::<HashSet<_>>();
        let mut count = 0;
        for origin in origins {
            count += self.expire_origin(app, &origin, reason).await?;
        }
        Ok(count)
    }

    /// Reconcile requests persisted by an earlier Desktop process. Their
    /// resolver endpoints died with that process, so restore makes them
    /// durably expired instead of recreating live-looking dead buttons.
    pub(crate) async fn expire_restored<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
    ) -> Result<usize> {
        if self.restore_started.swap(true, Ordering::AcqRel) {
            return Ok(0);
        }
        let result = self.expire_restored_inner(app).await;
        if result.is_err() {
            self.restore_started.store(false, Ordering::Release);
        }
        result
    }

    async fn expire_restored_inner<R: tauri::Runtime>(&self, app: &AppHandle<R>) -> Result<usize> {
        let mut after = 0;
        let mut restored: HashMap<String, (String, Value, EventSource)> = HashMap::new();
        // Restore only reads the four approval lifecycle kinds. Scanning the
        // whole journal here would JSON-parse every payload ever written just
        // to find them, and this runs on every Desktop start.
        let kinds = std::iter::once(APPROVAL_REQUESTED_KIND)
            .chain(APPROVAL_TERMINAL_KINDS)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        loop {
            let events = self
                .persistence
                .events_of_kinds_after(after, kinds.clone(), 2_000)
                .await?;
            if events.is_empty() {
                break;
            }
            for event in &events {
                after = after.max(event.sequence);
                let Some(approval_id) = event
                    .payload
                    .get("approvalId")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                else {
                    continue;
                };
                if event.kind == APPROVAL_REQUESTED_KIND {
                    if let Some(session_id) = event.session_id.clone() {
                        restored.insert(
                            approval_id,
                            (session_id, event.payload.clone(), event.source.clone()),
                        );
                    }
                } else if APPROVAL_TERMINAL_KINDS.contains(&event.kind.as_str()) {
                    restored.remove(&approval_id);
                }
            }
            if events.len() < 2_000 {
                break;
            }
        }
        let live_ids = self
            .pending
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for approval_id in live_ids {
            restored.remove(&approval_id);
        }
        let count = restored.len();
        for (approval_id, (session_id, requested, source)) in restored {
            self.persistence
                .append_boundary_event(
                    app,
                    session_id,
                    source,
                    "approval.expired",
                    json!({
                        "approvalId": approval_id,
                        "kind": requested.get("kind").and_then(Value::as_str).unwrap_or("permission"),
                        "decision": "expired",
                        "reason": "origin_unavailable_after_restart",
                    }),
                )
                .await?;
        }
        Ok(count)
    }

    /// Gate a Workshop-owned mutation. Permissive policy and remembered
    /// session grants stay auditable; otherwise the UI must settle the card.
    pub(crate) async fn authorize_host<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: Option<&str>,
        kind: ApprovalKind,
    ) -> Result<String> {
        self.authorize_host_outcome(app, session_id, kind)
            .await
            .map(|(approval_id, _)| approval_id)
    }

    pub(crate) async fn authorize_host_outcome<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: Option<&str>,
        kind: ApprovalKind,
    ) -> Result<(String, ApprovalDecision)> {
        // The session's sealed profile is the authority. Machine config is
        // consulted only for host approvals arriving with no session context
        // (operator-driven mutations) or for sessions started before the
        // profile existed; those cannot present a sealed value to honor.
        let sealed = match session_id {
            Some(session_id) => self.effective_policy(session_id).await,
            None => None,
        };
        let policy = match sealed {
            Some(policy) => policy,
            None => crate::synth_config::desktop_permission_settings()?.approval_policy,
        };
        if let Some(session_id) = session_id {
            if let Some(decision) = self.session_decision(session_id, &kind).await {
                let approval_id = self
                    .record_auto(app, session_id, &kind, &decision, "remembered-session")
                    .await?;
                return Ok((approval_id, decision));
            }
        }
        if let Some(decision) = super::approval_policy::auto_decision(&policy, &kind)? {
            let approval_id = self
                .record_auto(
                    app,
                    session_id.unwrap_or("policy-auto"),
                    &kind,
                    &decision,
                    &policy,
                )
                .await?;
            return Ok((approval_id, decision));
        }
        if let Some(session_id) = session_id {
            if let Some(outcome) = self
                .try_auto_authorize_paid_compute(app, session_id, &kind)
                .await?
            {
                return Ok(outcome);
            }
        }
        if let Some(session_id) = session_id {
            let (resolver, receiver) = HostDecisionResolver::pair();
            let approval_id = self
                .request(
                    app,
                    ApprovalOrigin {
                        session_id: session_id.to_owned(),
                        instance_id: format!("host-{}", std::process::id()),
                    },
                    kind,
                    resolver,
                )
                .await?;
            let decision = receiver
                .await
                .map_err(|_| anyhow!("approval waiter closed"))?
                .map_err(|reason| anyhow!("approval expired: {reason}"))?;
            if matches!(decision, ApprovalDecision::Reject) {
                return Err(anyhow!("approval rejected"));
            }
            return Ok((approval_id, decision));
        }
        if matches!(kind, ApprovalKind::SidecarLifecycle { .. }) {
            let decision = super::approval_policy::operator_decision(&kind);
            let approval_id = self
                .record_auto(app, "operator", &kind, &decision, "operator-command")
                .await?;
            return Ok((approval_id, decision));
        }
        Err(anyhow!(
            "this mutation requires an agent session for approval"
        ))
    }

    /// Conversation-scoped paid-compute auto-approval. Ineligible requests
    /// return `None` so the native modal stays in charge. Accounting lives on
    /// the broker (and SQLite), never in the synchronous policy table.
    pub(crate) async fn try_auto_authorize_paid_compute<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: &str,
        kind: &ApprovalKind,
    ) -> Result<Option<(String, ApprovalDecision)>> {
        let ApprovalKind::PaidCompute {
            requested_cap,
            preparation_digest,
            ..
        } = kind
        else {
            return Ok(None);
        };
        let Some(ceiling) = requested_cap.max_cost_usd_micros.filter(|value| *value > 0) else {
            return Ok(None);
        };
        let Some(provider) = paid_compute_provider(kind) else {
            return Ok(None);
        };
        let Some(database) = self.persistence.database() else {
            return Ok(None);
        };
        let _lock = self.paid_compute_lock.lock().await;
        let approval_id = format!("approval-auto-{}", uuid::Uuid::new_v4().simple());
        let session = session_id.to_owned();
        let digest = preparation_digest.clone();
        let reserved_id = approval_id.clone();
        let grant = database
            .run_transaction(move |conn| {
                if !super::paid_compute_budget::budget_allows_provider(conn, &session, &provider)? {
                    return Ok(None);
                }
                super::paid_compute_budget::try_reserve(
                    conn,
                    &session,
                    &reserved_id,
                    digest.as_deref(),
                    ceiling,
                )
            })
            .await?;
        let Some(grant) = grant else {
            return Ok(None);
        };
        let decision = ApprovalDecision::ApproveWithCap {
            cap: requested_cap.clone(),
        };
        if let Err(error) = self
            .write_auto_grant(
                app,
                session_id,
                &approval_id,
                kind,
                &decision,
                super::paid_compute_budget::CONVERSATION_POLICY,
                Some(grant.receipt_fields()),
            )
            .await
        {
            let release_id = approval_id.clone();
            let _ = database
                .run_transaction(move |conn| {
                    super::paid_compute_budget::release_reservation(conn, &release_id)
                })
                .await;
            return Err(error);
        }
        Ok(Some((approval_id, decision)))
    }

    pub(crate) async fn is_pending(&self, approval_id: &str) -> bool {
        self.pending.lock().await.contains_key(approval_id)
    }

    pub(crate) async fn pending_kind(&self, approval_id: &str) -> Option<ApprovalKind> {
        self.pending
            .lock()
            .await
            .get(approval_id)
            .map(|pending| pending.kind.clone())
    }

    pub(crate) async fn session_decision(
        &self,
        session_id: &str,
        kind: &ApprovalKind,
    ) -> Option<ApprovalDecision> {
        let key = remembered_key(kind)?;
        self.session_grants
            .lock()
            .await
            .contains(&(session_id.to_owned(), key))
            .then_some(ApprovalDecision::Approve {
                scope: ApprovalScope::Session,
            })
    }

    pub(crate) async fn decision_from_shell(
        &self,
        approval_id: &str,
        requested: &str,
    ) -> Result<ApprovalDecision> {
        let kind = self
            .pending_kind(approval_id)
            .await
            .ok_or_else(|| anyhow!("approval is no longer pending: {approval_id}"))?;
        match (&kind, requested) {
            (ApprovalKind::PaidCompute { requested_cap, .. }, "once") => {
                Ok(ApprovalDecision::ApproveWithCap {
                    cap: requested_cap.clone(),
                })
            }
            (ApprovalKind::PaidCompute { .. }, "always") => {
                Err(anyhow!("paid_compute approvals cannot be remembered"))
            }
            (ApprovalKind::CredentialAccess { consent, .. }, "once") => {
                Ok(ApprovalDecision::Credential {
                    outcome: match consent {
                        CredentialConsent::RememberLocator => CredentialDecision::RememberLocator,
                        CredentialConsent::RegisterSource => CredentialDecision::RegisterSource,
                        CredentialConsent::IssueLease => CredentialDecision::IssueLease,
                    },
                })
            }
            (ApprovalKind::CredentialAccess { .. }, "remember-locator") => {
                Ok(ApprovalDecision::Credential {
                    outcome: CredentialDecision::RememberLocator,
                })
            }
            (ApprovalKind::CredentialAccess { .. }, "register-source") => {
                Ok(ApprovalDecision::Credential {
                    outcome: CredentialDecision::RegisterSource,
                })
            }
            _ => ApprovalDecision::from_shell_wire(requested),
        }
    }

    #[cfg(test)]
    pub(crate) async fn pending_len(&self) -> usize {
        self.pending.lock().await.len()
    }
}

fn paid_compute_provider(kind: &ApprovalKind) -> Option<String> {
    let ApprovalKind::PaidCompute {
        parameters,
        credential_names,
        ..
    } = kind
    else {
        return None;
    };
    let raw = parameters
        .pointer("/model/provider")
        .and_then(Value::as_str)
        .or_else(|| {
            parameters
                .pointer("/credentialRoute/provider")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            credential_names
                .first()
                .and_then(|name| name.split(':').next())
        })?;
    crate::synth_config::normalize_provider(raw).ok()
}

fn remembered_key(kind: &ApprovalKind) -> Option<String> {
    // No key means no remembered grant can satisfy the request. Checked here
    // rather than at each call site so that a kind added later cannot become
    // rememberable by omission.
    if kind.requires_human() {
        return None;
    }
    match kind {
        ApprovalKind::SidecarLifecycle { sidecar, action } => {
            Some(format!("sidecar:{sidecar}:{action}"))
        }
        _ => None,
    }
}

/// Resolves a Workshop-owned mutation without coupling the broker to a JSON-RPC
/// transport. The waiter dies on restart, so restore expires its durable card.
pub(crate) struct HostDecisionResolver {
    tx: Mutex<Option<tokio::sync::oneshot::Sender<Result<ApprovalDecision, String>>>>,
}

impl HostDecisionResolver {
    pub(crate) fn pair() -> (
        Arc<Self>,
        tokio::sync::oneshot::Receiver<Result<ApprovalDecision, String>>,
    ) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (
            Arc::new(Self {
                tx: Mutex::new(Some(tx)),
            }),
            rx,
        )
    }
}

impl ApprovalResolver for HostDecisionResolver {
    fn resolve<'a>(&'a self, decision: &'a ApprovalDecision) -> ResolverFuture<'a> {
        Box::pin(async move {
            if let Some(tx) = self.tx.lock().await.take() {
                let _ = tx.send(Ok(decision.clone()));
            }
            Ok(ApprovalDelivery {
                resolver_decision: Some(decision.event_value().into()),
            })
        })
    }

    fn expire<'a>(&'a self, reason: &'a str) -> ResolverFuture<'a> {
        Box::pin(async move {
            if let Some(tx) = self.tx.lock().await.take() {
                let _ = tx.send(Err(reason.to_owned()));
            }
            Ok(ApprovalDelivery {
                resolver_decision: Some("expired".into()),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_runtime::CoreRuntime;
    use crate::storage::EventAppend;
    use std::sync::atomic::AtomicUsize;
    use tempfile::tempdir;

    fn computer_use(hazard: bool) -> ApprovalKind {
        ApprovalKind::ComputerUse {
            app: "com.apple.mail".into(),
            action: "click".into(),
            payload: json!({ "recipient": "board@example.com" }),
            hazard,
            element_index: Some(7),
        }
    }

    fn container_lifecycle() -> ApprovalKind {
        ApprovalKind::ContainerLifecycle {
            container_id: "ctr_fixture".into(),
            declaration_id: "fixture-container".into(),
            declaration_digest: "sha256:validated-declaration".into(),
            manifest_path: "/approved/workshop.containers.toml".into(),
            source_root: "/approved".into(),
            source_revision: Some("revision-1".into()),
            source_digest: Some("sha256:declared-inputs".into()),
            action: "force_replace".into(),
            effect: "replace the exact validated declaration".into(),
        }
    }

    /// Consent for a hazard action is consent for *that payload*. A remembered
    /// grant would answer a question nobody asked.
    #[test]
    fn hazard_approvals_cannot_be_remembered_or_keyed() {
        for scope in [ApprovalScope::Session, ApprovalScope::Workspace] {
            let error = computer_use(true)
                .validate_decision(&ApprovalDecision::Approve { scope })
                .unwrap_err();
            assert!(error.to_string().contains("cannot be remembered"));
        }
        computer_use(true)
            .validate_decision(&ApprovalDecision::Approve {
                scope: ApprovalScope::Once,
            })
            .unwrap();
        assert!(remembered_key(&computer_use(true)).is_none());
    }

    /// The card has to show what the action will do, not just which app it
    /// touches — that distinction is the whole point of the hazard class.
    #[test]
    fn hazard_payload_reaches_the_card() {
        let payload = computer_use(true).safe_payload("approval-1");
        assert_eq!(payload["kind"], "computer_use");
        assert_eq!(payload["payload"]["recipient"], "board@example.com");
        assert_eq!(payload["hazard"], true);
        assert_eq!(payload["alwaysSupported"], false);
        assert_eq!(
            computer_use(false).safe_payload("approval-2")["alwaysSupported"],
            true
        );
    }

    #[test]
    fn container_lifecycle_card_binds_the_exact_declaration_and_origin() {
        let kind = container_lifecycle();
        let payload = kind.safe_payload("approval-lifecycle");
        assert_eq!(payload["kind"], "container_lifecycle");
        assert_eq!(payload["declarationId"], "fixture-container");
        assert_eq!(payload["declarationDigest"], "sha256:validated-declaration");
        assert_eq!(
            payload["manifestPath"],
            "/approved/workshop.containers.toml"
        );
        assert_eq!(payload["sourceRoot"], "/approved");
        assert_eq!(payload["sourceRevision"], "revision-1");
        assert_eq!(payload["sourceDigest"], "sha256:declared-inputs");
        assert!(remembered_key(&kind).is_none());
    }

    struct RecordingResolver {
        expired: Arc<AtomicUsize>,
    }

    impl ApprovalResolver for RecordingResolver {
        fn resolve<'a>(&'a self, _decision: &'a ApprovalDecision) -> ResolverFuture<'a> {
            Box::pin(async {
                Ok(ApprovalDelivery {
                    resolver_decision: Some("accept".into()),
                })
            })
        }

        fn expire<'a>(&'a self, _reason: &'a str) -> ResolverFuture<'a> {
            Box::pin(async move {
                self.expired.fetch_add(1, Ordering::Relaxed);
                Ok(ApprovalDelivery {
                    resolver_decision: Some("decline".into()),
                })
            })
        }
    }

    /// Blocks inside `resolve` until released, so a test can hold a delivery
    /// in flight and race an expiry against it.
    struct GatedResolver {
        resolved: Arc<AtomicUsize>,
        expired: Arc<AtomicUsize>,
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    impl ApprovalResolver for GatedResolver {
        fn resolve<'a>(&'a self, _decision: &'a ApprovalDecision) -> ResolverFuture<'a> {
            Box::pin(async move {
                self.entered.notify_one();
                self.release.notified().await;
                self.resolved.fetch_add(1, Ordering::Relaxed);
                Ok(ApprovalDelivery {
                    resolver_decision: Some("accept".into()),
                })
            })
        }

        fn expire<'a>(&'a self, _reason: &'a str) -> ResolverFuture<'a> {
            Box::pin(async move {
                self.expired.fetch_add(1, Ordering::Relaxed);
                Ok(ApprovalDelivery {
                    resolver_decision: Some("decline".into()),
                })
            })
        }
    }

    #[test]
    fn a_shell_request_without_always_refuses_a_remembered_scope() {
        let kind = ApprovalKind::ShellCommand {
            request_method: "execCommandApproval".into(),
            detail: "rm -rf /tmp/scratch".into(),
            scope: None,
            always_supported: false,
        };
        let message = kind
            .validate_decision(&ApprovalDecision::Approve {
                scope: ApprovalScope::Session,
            })
            .unwrap_err()
            .to_string();
        assert!(message.contains("does not offer a remembered approval"));
        // Once is always deliverable, and always_supported unlocks the rest.
        kind.validate_decision(&ApprovalDecision::Approve {
            scope: ApprovalScope::Once,
        })
        .unwrap();
        ApprovalKind::ShellCommand {
            request_method: "execCommandApproval".into(),
            detail: "ls".into(),
            scope: None,
            always_supported: true,
        }
        .validate_decision(&ApprovalDecision::Approve {
            scope: ApprovalScope::Session,
        })
        .unwrap();
    }

    #[tokio::test]
    async fn an_expiry_racing_an_in_flight_resolve_does_not_double_terminalize() {
        let broker = Arc::new(ApprovalBroker::new(SessionPersistence::Null));
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let origin = ApprovalOrigin {
            session_id: "approval-session".into(),
            instance_id: "process-1".into(),
        };
        let resolved = Arc::new(AtomicUsize::new(0));
        let expired = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let approval_id = broker
            .request(
                &handle,
                origin.clone(),
                ApprovalKind::ShellCommand {
                    request_method: "execCommandApproval".into(),
                    detail: "ls".into(),
                    scope: None,
                    always_supported: false,
                },
                Arc::new(GatedResolver {
                    resolved: resolved.clone(),
                    expired: expired.clone(),
                    entered: entered.clone(),
                    release: release.clone(),
                }),
            )
            .await
            .unwrap();

        let deliver = {
            let broker = broker.clone();
            let handle = handle.clone();
            let approval_id = approval_id.clone();
            tokio::spawn(async move {
                broker
                    .resolve(
                        &handle,
                        "approval-session",
                        &approval_id,
                        ApprovalDecision::Approve {
                            scope: ApprovalScope::Once,
                        },
                    )
                    .await
            })
        };
        // Delivery is now in flight and holds this approval's settle guard.
        entered.notified().await;
        let sweep = {
            let broker = broker.clone();
            let handle = handle.clone();
            let origin = origin.clone();
            tokio::spawn(async move {
                broker
                    .expire_origin(&handle, &origin, "origin_turn_ended")
                    .await
            })
        };
        release.notify_one();

        deliver.await.unwrap().expect("delivery succeeds");
        let swept = sweep.await.unwrap().expect("sweep completes");
        assert_eq!(swept, 0, "a settled approval must not be counted expired");
        assert_eq!(resolved.load(Ordering::Relaxed), 1);
        assert_eq!(
            expired.load(Ordering::Relaxed),
            0,
            "the resolver must not be told to expire a request it already resolved"
        );
        assert_eq!(broker.pending_len().await, 0);
    }

    #[test]
    fn paid_compute_cannot_be_remembered_and_approval_carries_a_cap() {
        let kind = ApprovalKind::PaidCompute {
            operation: "gepa.banking77.smoke.v1".into(),
            parameters: json!({"recipeId":"gepa.banking77.smoke.v1"}),
            estimated_cost_usd_micros: None,
            requested_cap: PaidComputeCap {
                max_cost_usd_micros: Some(500_000),
                max_rollouts: Some(4),
            },
            requesting_agent: "agent:planner".into(),
            recipe_id: Some("gepa.banking77.smoke.v1".into()),
            dataset: Some("banking77".into()),
            proposer_model: Some("gpt-5.6-luna".into()),
            evaluator_model: Some("banking77_candidate".into()),
            timeout_seconds: Some(300),
            credential_names: vec!["OPENAI_API_KEY".into()],
            preparation_digest: Some("sha256:prep".into()),
        };
        let remembered = ApprovalDecision::Approve {
            scope: ApprovalScope::Workspace,
        };
        assert!(kind
            .validate_decision(&remembered)
            .unwrap_err()
            .to_string()
            .contains("cannot be remembered"));

        let capped = ApprovalDecision::ApproveWithCap {
            cap: PaidComputeCap {
                max_cost_usd_micros: Some(500_000),
                max_rollouts: Some(4),
            },
        };
        kind.validate_decision(&capped).unwrap();
        let encoded = serde_json::to_value(&capped).unwrap();
        assert_eq!(encoded["cap"]["maxCostUsdMicros"], 500_000);
        assert_eq!(encoded["cap"]["maxRollouts"], 4);
        let payload = kind.safe_payload("approval-paid");
        assert_eq!(payload["estimatedCostUsdMicros"], Value::Null);
        assert_eq!(payload["requestedCap"]["maxCostUsdMicros"], 500_000);
        assert_eq!(payload["requestedCap"]["maxRollouts"], 4);
    }

    #[test]
    fn typed_host_approvals_stay_on_the_owning_codex_session_stream() {
        let kind = ApprovalKind::SidecarLifecycle {
            sidecar: "optimizers".into(),
            action: "start".into(),
        };
        assert_eq!(kind.source(), EventSource::Codex);
    }

    #[test]
    fn credential_payload_has_no_field_capable_of_carrying_a_secret() {
        let payload = ApprovalKind::CredentialAccess {
            consent: CredentialConsent::IssueLease,
            provider: "openrouter".into(),
            purpose: "start a session".into(),
            locator_id: None,
            display_path: None,
            variable: None,
            switch_from_display: None,
        }
        .safe_payload("approval-credential");
        assert_eq!(payload["kind"], "credential_access");
        let encoded = payload.to_string().to_ascii_lowercase();
        assert!(!encoded.contains("credential\":"));
        assert!(!encoded.contains("token\":"));
        assert!(!encoded.contains("secret\":"));
    }

    #[test]
    fn credential_consent_matrix_never_settles_above_the_requested_ceiling() {
        let kind = |consent| ApprovalKind::CredentialAccess {
            consent,
            provider: "openrouter".into(),
            purpose: "test consent ceiling".into(),
            locator_id: Some("locator_test".into()),
            display_path: Some("project/.env".into()),
            variable: Some("OPENROUTER_API_KEY".into()),
            switch_from_display: None,
        };
        let decision = |outcome| ApprovalDecision::Credential { outcome };

        kind(CredentialConsent::RememberLocator)
            .validate_decision(&decision(CredentialDecision::RememberLocator))
            .unwrap();
        assert!(kind(CredentialConsent::RememberLocator)
            .validate_decision(&decision(CredentialDecision::RegisterSource))
            .is_err());
        assert!(kind(CredentialConsent::RememberLocator)
            .validate_decision(&decision(CredentialDecision::IssueLease))
            .is_err());

        kind(CredentialConsent::RegisterSource)
            .validate_decision(&decision(CredentialDecision::RememberLocator))
            .unwrap();
        kind(CredentialConsent::RegisterSource)
            .validate_decision(&decision(CredentialDecision::RegisterSource))
            .unwrap();
        assert!(kind(CredentialConsent::RegisterSource)
            .validate_decision(&decision(CredentialDecision::IssueLease))
            .is_err());

        kind(CredentialConsent::IssueLease)
            .validate_decision(&decision(CredentialDecision::IssueLease))
            .unwrap();
        assert!(kind(CredentialConsent::IssueLease)
            .validate_decision(&decision(CredentialDecision::RememberLocator))
            .is_err());
        assert!(kind(CredentialConsent::IssueLease)
            .validate_decision(&decision(CredentialDecision::RegisterSource))
            .is_err());
        for consent in [
            CredentialConsent::RememberLocator,
            CredentialConsent::RegisterSource,
            CredentialConsent::IssueLease,
        ] {
            kind(consent)
                .validate_decision(&ApprovalDecision::Reject)
                .unwrap();
        }
    }

    #[tokio::test]
    async fn expiring_an_origin_drains_the_pending_set_and_unblocks_its_resolver() {
        let broker = ApprovalBroker::new(SessionPersistence::Null);
        let app = tauri::test::mock_app();
        let origin = ApprovalOrigin {
            session_id: "approval-session".into(),
            instance_id: "process-1".into(),
        };
        let expired = Arc::new(AtomicUsize::new(0));
        broker
            .request(
                app.handle(),
                origin.clone(),
                ApprovalKind::ShellCommand {
                    request_method: "execCommandApproval".into(),
                    detail: "Run a shell command".into(),
                    scope: None,
                    always_supported: true,
                },
                Arc::new(RecordingResolver {
                    expired: expired.clone(),
                }),
            )
            .await
            .unwrap();
        assert_eq!(broker.pending_len().await, 1);
        assert_eq!(
            broker
                .expire_origin(app.handle(), &origin, "origin_process_exited")
                .await
                .unwrap(),
            1
        );
        assert_eq!(broker.pending_len().await, 0);
        assert_eq!(expired.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn restart_reconciliation_terminalizes_a_request_without_a_resolver() {
        let temp = tempdir().unwrap();
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        core.journal()
            .append(EventAppend::codex(
                "restored-session",
                "approval.requested",
                json!({
                    "approvalId": "approval-before-restart",
                    "kind": "shell_command",
                    "detail": "Run a shell command",
                }),
            ))
            .await
            .unwrap();
        let broker = ApprovalBroker::new(SessionPersistence::from_core(Some(core.clone())));
        let app = tauri::test::mock_app();
        assert_eq!(broker.expire_restored(app.handle()).await.unwrap(), 1);
        assert_eq!(broker.expire_restored(app.handle()).await.unwrap(), 0);

        let events = core
            .journal()
            .session_events_after("restored-session".into(), 0, 100)
            .await
            .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].kind, "approval.expired");
        assert_eq!(
            events[1].payload["reason"],
            "origin_unavailable_after_restart"
        );
    }

    #[tokio::test]
    async fn stale_or_replayed_receipts_fail_closed() {
        let broker = ApprovalBroker::new(SessionPersistence::Null);
        let app = tauri::test::mock_app();
        let (resolver, rx) = HostDecisionResolver::pair();
        let approval_id = broker
            .request(
                app.handle(),
                ApprovalOrigin {
                    session_id: "approval-session".into(),
                    instance_id: "process-1".into(),
                },
                ApprovalKind::PluginLifecycle {
                    plugin_id: "optimizers".into(),
                    action: "install".into(),
                    version: Some("0.2.0".into()),
                    publisher: "Synth Laboratories".into(),
                    digest: None,
                    download_size_bytes: Some(1),
                    network_host: Some("pypi.org".into()),
                    service_effect: "download".into(),
                    active_runs: 0,
                    retention: "keep".into(),
                    always_supported: true,
                },
                resolver,
            )
            .await
            .unwrap();
        broker
            .resolve(
                app.handle(),
                "approval-session",
                &approval_id,
                ApprovalDecision::Approve {
                    scope: ApprovalScope::Once,
                },
            )
            .await
            .unwrap();
        assert!(rx.await.unwrap().is_ok());
        let replayed = broker
            .resolve(
                app.handle(),
                "approval-session",
                &approval_id,
                ApprovalDecision::Approve {
                    scope: ApprovalScope::Once,
                },
            )
            .await
            .unwrap_err();
        assert!(replayed.to_string().contains("no longer pending"));
        let missing = broker
            .resolve(
                app.handle(),
                "approval-session",
                "approval-missing",
                ApprovalDecision::Reject,
            )
            .await
            .unwrap_err();
        assert!(missing.to_string().contains("no longer pending"));
    }

    fn openrouter_paid(max_cost_usd_micros: Option<u64>) -> ApprovalKind {
        ApprovalKind::PaidCompute {
            operation: "optimizer.evaluation.inline.start".into(),
            parameters: json!({
                "model": { "provider": "openrouter", "modelId": "gpt-5.6-luna" },
                "credentialRoute": { "kind": "workshop_proxy", "provider": "openrouter" },
                "executionSpecDigest": "sha256:spec",
            }),
            estimated_cost_usd_micros: None,
            requested_cap: PaidComputeCap {
                max_cost_usd_micros,
                max_rollouts: Some(8),
            },
            requesting_agent: "agent:test".into(),
            recipe_id: None,
            dataset: None,
            proposer_model: Some("gpt-5.6-luna".into()),
            evaluator_model: None,
            timeout_seconds: None,
            credential_names: vec!["openrouter:workshop_secrets_proxy".into()],
            preparation_digest: Some("sha256:spec".into()),
        }
    }

    fn enabled_paid_compute() -> crate::synth_config::PaidComputeAutoApprovalSettings {
        crate::synth_config::PaidComputeAutoApprovalSettings {
            enabled: true,
            max_request_usd: "0.10".into(),
            max_conversation_usd: "0.25".into(),
            providers: vec!["openrouter".into()],
        }
    }

    async fn sealed_broker(
        paid: crate::synth_config::PaidComputeAutoApprovalSettings,
    ) -> (
        crate::synth_config::test_machine_permissions::Guard,
        tempfile::TempDir,
        std::sync::Arc<CoreRuntime>,
        ApprovalBroker,
        tauri::App<tauri::test::MockRuntime>,
    ) {
        let machine = crate::synth_config::test_machine_permissions::install_with_paid_compute(
            "on-request",
            "workspace-write",
            paid,
        );
        let temp = tempdir().unwrap();
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let broker = ApprovalBroker::new(SessionPersistence::from_core(Some(core.clone())));
        let app = tauri::test::mock_app();
        let profile = crate::session::approval_policy::resolve_effective(None, None).unwrap();
        broker
            .record_policy_effective(app.handle(), "sess-a", profile)
            .await
            .unwrap();
        (machine, temp, core, broker, app)
    }

    #[tokio::test]
    async fn default_configuration_keeps_paid_compute_on_the_modal() {
        let (_guard, _temp, _core, broker, app) =
            sealed_broker(crate::synth_config::PaidComputeAutoApprovalSettings::disabled()).await;
        let outcome = broker
            .try_auto_authorize_paid_compute(app.handle(), "sess-a", &openrouter_paid(Some(60_000)))
            .await
            .unwrap();
        assert!(outcome.is_none());
    }

    #[tokio::test]
    async fn eligible_paid_compute_auto_approves_the_requested_cap() {
        let (_guard, _temp, core, broker, app) = sealed_broker(enabled_paid_compute()).await;
        let (approval_id, decision) = broker
            .try_auto_authorize_paid_compute(app.handle(), "sess-a", &openrouter_paid(Some(60_000)))
            .await
            .unwrap()
            .expect("eligible request auto-approves");
        assert!(approval_id.starts_with("approval-auto-"));
        match decision {
            ApprovalDecision::ApproveWithCap { cap } => {
                assert_eq!(cap.max_cost_usd_micros, Some(60_000));
            }
            other => panic!("expected capped approval, got {other:?}"),
        }
        let events = core
            .journal()
            .session_events_after("sess-a".into(), 0, 100)
            .await
            .unwrap();
        let granted = events
            .iter()
            .find(|event| event.kind == "approval.granted")
            .expect("auto-approval writes a granted receipt");
        assert_eq!(granted.payload["policyAuto"], true);
        assert_eq!(
            granted.payload["approvalPolicy"],
            "conversation_paid_compute_budget"
        );
        assert_eq!(granted.payload["reservedUsdMicros"], 60_000);
        assert_eq!(granted.payload["remainingUsdMicros"], 190_000);
        assert_eq!(granted.payload["conversationCapUsdMicros"], 250_000);
        assert_eq!(granted.payload["cap"]["maxCostUsdMicros"], 60_000);
        assert_eq!(granted.payload["kind"], "paid_compute");
        assert!(granted
            .payload
            .get("approvalId")
            .and_then(Value::as_str)
            .is_some());
    }

    #[tokio::test]
    async fn over_request_missing_ceiling_and_unlisted_provider_open_the_modal() {
        let (_guard, _temp, _core, broker, app) = sealed_broker(enabled_paid_compute()).await;
        assert!(broker
            .try_auto_authorize_paid_compute(
                app.handle(),
                "sess-a",
                &openrouter_paid(Some(200_000))
            )
            .await
            .unwrap()
            .is_none());
        assert!(broker
            .try_auto_authorize_paid_compute(app.handle(), "sess-a", &openrouter_paid(None))
            .await
            .unwrap()
            .is_none());
        let mut openai = openrouter_paid(Some(60_000));
        if let ApprovalKind::PaidCompute {
            parameters,
            credential_names,
            ..
        } = &mut openai
        {
            *parameters = json!({
                "model": { "provider": "openai", "modelId": "gpt-5.6-luna" },
                "credentialRoute": { "kind": "workshop_proxy", "provider": "openai" },
            });
            *credential_names = vec!["openai:workshop_secrets_proxy".into()];
        }
        assert!(broker
            .try_auto_authorize_paid_compute(app.handle(), "sess-a", &openai)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn conversation_remainder_blocks_a_second_request() {
        let (_guard, _temp, _core, broker, app) = sealed_broker(enabled_paid_compute()).await;
        broker
            .try_auto_authorize_paid_compute(app.handle(), "sess-a", &openrouter_paid(Some(60_000)))
            .await
            .unwrap()
            .unwrap();
        assert!(broker
            .try_auto_authorize_paid_compute(
                app.handle(),
                "sess-a",
                &openrouter_paid(Some(200_000))
            )
            .await
            .unwrap()
            .is_none());
        broker
            .try_auto_authorize_paid_compute(
                app.handle(),
                "sess-a",
                &openrouter_paid(Some(100_000)),
            )
            .await
            .unwrap()
            .unwrap();
        assert!(broker
            .try_auto_authorize_paid_compute(
                app.handle(),
                "sess-a",
                &openrouter_paid(Some(100_000))
            )
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn concurrent_reserves_cannot_oversubscribe_the_conversation() {
        let (_guard, _temp, core, broker, app) = sealed_broker(enabled_paid_compute()).await;
        let broker = Arc::new(broker);
        broker
            .try_auto_authorize_paid_compute(app.handle(), "sess-a", &openrouter_paid(Some(60_000)))
            .await
            .unwrap()
            .unwrap();
        let handle = app.handle().clone();
        let left = {
            let broker = broker.clone();
            let handle = handle.clone();
            tokio::spawn(async move {
                broker
                    .try_auto_authorize_paid_compute(
                        &handle,
                        "sess-a",
                        &openrouter_paid(Some(100_000)),
                    )
                    .await
            })
        };
        let right = {
            let broker = broker.clone();
            tokio::spawn(async move {
                broker
                    .try_auto_authorize_paid_compute(
                        &handle,
                        "sess-a",
                        &openrouter_paid(Some(100_000)),
                    )
                    .await
            })
        };
        let left = left.await.unwrap().unwrap();
        let right = right.await.unwrap().unwrap();
        assert_eq!(left.is_some() as u8 + right.is_some() as u8, 1);
        let reserved: i64 = core
            .storage()
            .database()
            .with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT COALESCE(SUM(reserved_usd_micros), 0) FROM paid_compute_reservations
                     WHERE session_id='sess-a' AND status='reserved'",
                    [],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(reserved, 160_000);
    }

    #[tokio::test]
    async fn forked_conversations_receive_independent_allowances() {
        let (_guard, _temp, _core, broker, app) = sealed_broker(enabled_paid_compute()).await;
        let profile = crate::session::approval_policy::resolve_effective(None, None).unwrap();
        broker
            .record_policy_effective(app.handle(), "sess-b", profile)
            .await
            .unwrap();
        broker
            .try_auto_authorize_paid_compute(app.handle(), "sess-a", &openrouter_paid(Some(60_000)))
            .await
            .unwrap()
            .unwrap();
        let child = broker
            .try_auto_authorize_paid_compute(
                app.handle(),
                "sess-b",
                &openrouter_paid(Some(100_000)),
            )
            .await
            .unwrap();
        assert!(child.is_some());
    }

    #[tokio::test]
    async fn credential_approvals_are_not_auto_authorized_by_the_budget() {
        let (_guard, _temp, _core, broker, app) = sealed_broker(enabled_paid_compute()).await;
        let credential = ApprovalKind::CredentialAccess {
            consent: CredentialConsent::IssueLease,
            provider: "openrouter".into(),
            purpose: "bounded optimizer recipe".into(),
            locator_id: None,
            display_path: None,
            variable: None,
            switch_from_display: None,
        };
        assert!(broker
            .try_auto_authorize_paid_compute(app.handle(), "sess-a", &credential)
            .await
            .unwrap()
            .is_none());
    }
}
