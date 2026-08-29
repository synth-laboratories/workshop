//! Origin-neutral approval lifecycle and policy.
//!
//! The broker owns pending state and durable lifecycle events. Protocol-specific
//! delivery (Codex JSON-RPC today, local oneshots later) lives behind
//! [`ApprovalResolver`].

use crate::limits::PAID_COMPUTE_APPROVAL_TTL;
use crate::session::SessionPersistence;
use crate::storage::EventSource;
use anyhow::{anyhow, Result};
use chrono::Utc;
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
    /// Persist renderer code the app will compile at every launch.
    ///
    /// A visual template under `<state root>/visuals/templates/` is not a
    /// rendered view that dies with the pane: the registry scans that directory
    /// on every start, for every future session, until someone deletes it.
    /// `internal_readme.md` fixes the precedent for that class — entering an API
    /// key "creates persistent access and must follow the Computer Use
    /// confirmation policy" — so this is `requires_human`, and no policy,
    /// `never` included, may settle it.
    ///
    /// The fields are chosen to answer the question a person actually has in
    /// front of this card: *which* id, in *which* capability model, how much
    /// code, and does it replace something that is already there.
    VisualTemplatePersist {
        /// Template id, which is also the directory name under the root.
        template_id: String,
        /// `user` — `shell.tsx` compiled in the pane under the sourced
        /// allowlist — or `managed` — `renderer.html` in a sandboxed iframe
        /// under a CSP. Two different capability models, so the card must say
        /// which one is being granted, not just "a template".
        source_kind: String,
        /// `save`, `fork`, or `import`.
        action: String,
        /// Bytes of code this write persists.
        byte_size: u64,
        /// True when an indexed template already owns this id and its source is
        /// being replaced. "Add a template" and "silently replace the template
        /// you already reviewed" are not the same decision.
        overwrites: bool,
        /// Template this one was copied from, when it is a fork.
        forked_from: Option<String>,
        /// Directory that will be written.
        destination: String,
        /// SHA-256 of the source bytes, so the card and the receipt name the
        /// same code.
        source_digest: String,
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
    pub(crate) fn approval_digest(&self) -> Option<&str> {
        match self {
            Self::PaidCompute {
                preparation_digest, ..
            } => preparation_digest.as_deref(),
            _ => None,
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::ShellCommand { .. } => "shell_command",
            Self::PaidCompute { .. } => "paid_compute",
            Self::SidecarLifecycle { .. } => "sidecar_lifecycle",
            Self::ContainerLifecycle { .. } => "container_lifecycle",
            Self::PluginLifecycle { .. } => "plugin_lifecycle",
            Self::CredentialAccess { .. } => "credential_access",
            Self::VisualTemplatePersist { .. } => "visual_template_persist",
            Self::ComputerUse { .. } => "computer_use",
        }
    }

    /// Requests no policy may settle and no remembered grant may satisfy.
    ///
    /// The permissive `approval_policy = "never"` is honored everywhere else in
    /// Workshop, deliberately: the operator asked for it. It is not honored here.
    /// A hazard action commits content on the operator's behalf, paid compute
    /// commits a new digest-bound spend, credential access creates a new
    /// run-scoped provider capability, and persisting a visual template leaves
    /// code behind that the app compiles at every launch. Consent is about
    /// that exact payload — a previous yes or a permissive shell policy is not
    /// a substitute for any of these grants.
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
                | Self::VisualTemplatePersist { .. }
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
            // Once only, and the remembered scopes were already refused above.
            // "Let this agent write templates for the rest of the session" is a
            // standing grant to persist arbitrary renderer code; approving one
            // named id with one named digest is the whole decision.
            (
                Self::VisualTemplatePersist { .. },
                ApprovalDecision::Approve {
                    scope: ApprovalScope::Once,
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

    /// One sentence naming every fact the decision turns on.
    ///
    /// Built here rather than at the producer so the card, the durable
    /// `approval.requested` receipt, and any later reader all read the same
    /// words. Nothing in it can carry a secret: every input is an id, a tier
    /// name, a byte count, or a path under the instance state root.
    fn visual_template_detail(
        action: &str,
        template_id: &str,
        source_kind: &str,
        byte_size: u64,
        overwrites: bool,
        forked_from: Option<&str>,
    ) -> String {
        let disposition = if overwrites {
            "replacing the template already installed under that id"
        } else {
            "adding a new template id"
        };
        let origin = match forked_from {
            Some(origin) => format!(", forked from {origin}"),
            None => String::new(),
        };
        format!(
            "{action} visual template {template_id} ({source_kind}, {byte_size} bytes{origin}) — \
             {disposition}. This code is compiled every time the app starts, in every future \
             session, until the directory is deleted."
        )
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
            Self::VisualTemplatePersist {
                template_id,
                source_kind,
                action,
                byte_size,
                overwrites,
                forked_from,
                destination,
                source_digest,
            } => json!({
                "approvalId": approval_id,
                "kind": self.name(),
                "templateId": template_id,
                "sourceKind": source_kind,
                "action": action,
                "byteSizeBytes": byte_size,
                "overwrites": overwrites,
                "forkedFrom": forked_from,
                "destination": destination,
                "sourceDigest": source_digest,
                // A renderer that predates this variant falls through
                // `sessionView.ts`'s typed branches to `payload.path`, and
                // drops `payload.detail` because its `safeKind` list does not
                // name this kind. So `path` is what a person actually sees
                // today — the directory about to be written, which is at least
                // the truth — and `detail` is the sentence the typed branch
                // should show once it exists. Both are written now so that
                // adding the branch is a renderer-only change.
                "detail": Self::visual_template_detail(
                    action,
                    template_id,
                    source_kind,
                    *byte_size,
                    *overwrites,
                    forked_from.as_deref(),
                ),
                "path": destination,
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

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingApprovalView {
    pub approval_id: String,
    pub session_id: String,
    pub kind: String,
    pub requires_human: bool,
    pub preparation_digest: Option<String>,
}

pub(crate) struct ApprovalBroker {
    pending: Mutex<HashMap<String, Arc<PendingApproval>>>,
    session_grants: Mutex<HashSet<(String, String)>>,
    settled_exact: Mutex<HashSet<(String, String, String)>>,
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
            settled_exact: Mutex::new(HashSet::new()),
            effective_policies: Mutex::new(HashMap::new()),
            persistence,
            restore_started: AtomicBool::new(false),
            paid_compute_lock: Mutex::new(()),
        }
    }

    pub(crate) async fn pending_snapshot(&self) -> Vec<PendingApprovalView> {
        let mut views = self
            .pending
            .lock()
            .await
            .iter()
            .map(|(approval_id, pending)| PendingApprovalView {
                approval_id: approval_id.clone(),
                session_id: pending.origin.session_id.clone(),
                kind: pending.kind.name().to_owned(),
                requires_human: pending.kind.requires_human(),
                preparation_digest: pending.kind.approval_digest().map(str::to_owned),
            })
            .collect::<Vec<_>>();
        views.sort_by(|left, right| left.approval_id.cmp(&right.approval_id));
        views
    }

    pub(crate) async fn approve_digest<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        digest: &str,
    ) -> Result<(String, bool)> {
        let matched = self.pending.lock().await.iter().find_map(|(id, pending)| {
            (pending.kind.approval_digest() == Some(digest)).then(|| {
                (
                    id.clone(),
                    pending.origin.session_id.clone(),
                    pending.kind.clone(),
                )
            })
        });
        let Some((approval_id, session_id, kind)) = matched else {
            return Err(anyhow!("no approval sheet is open for digest {digest}"));
        };
        let decision = match kind {
            ApprovalKind::PaidCompute { requested_cap, .. } => {
                ApprovalDecision::ApproveWithCap { cap: requested_cap }
            }
            _ => ApprovalDecision::Approve {
                scope: ApprovalScope::Once,
            },
        };
        self.resolve(app, &session_id, &approval_id, decision)
            .await?;
        Ok((approval_id, false))
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
        let mut payload = kind.safe_payload(&approval_id);
        if let Some(digest) = kind.approval_digest() {
            let requested_at = Utc::now();
            let expires_at = requested_at
                + chrono::Duration::from_std(PAID_COMPUTE_APPROVAL_TTL)
                    .expect("paid-compute approval TTL fits chrono duration");
            payload["approvalDigest"] = json!(digest);
            payload["requestedAt"] = json!(requested_at.to_rfc3339());
            payload["expiresAt"] = json!(expires_at.to_rfc3339());
        }
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
        if let Some(digest) = pending.kind.approval_digest() {
            let mut settled_exact = self.settled_exact.lock().await;
            settled_exact.insert((
                session_id.to_owned(),
                digest.to_owned(),
                decision.event_value().to_owned(),
            ));
            if matches!(decision, ApprovalDecision::ApproveWithCap { .. }) {
                settled_exact.insert((session_id.to_owned(), digest.to_owned(), "once".to_owned()));
            }
        }
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

    pub(crate) async fn validate_exact_digest(
        &self,
        approval_id: &str,
        expected_digest: &str,
    ) -> Result<()> {
        let pending = self.pending.lock().await;
        let pending = pending
            .get(approval_id)
            .ok_or_else(|| anyhow!("approval is no longer pending: {approval_id}"))?;
        let actual = pending.kind.approval_digest().ok_or_else(|| {
            anyhow!("approval is not bound to an immutable proposal digest: {approval_id}")
        })?;
        if actual != expected_digest {
            return Err(anyhow!(
                "approval digest mismatch for {approval_id}: expected {expected_digest}, current {actual}"
            ));
        }
        Ok(())
    }

    pub(crate) async fn was_resolved_exact(
        &self,
        session_id: &str,
        digest: &str,
        decision: &ApprovalDecision,
    ) -> bool {
        self.settled_exact.lock().await.contains(&(
            session_id.to_owned(),
            digest.to_owned(),
            decision.event_value().to_owned(),
        ))
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

