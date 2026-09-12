//! Host composition of the native MQ mailbox.
//!
//! Every remote step runs inside `await_scoped`, so sign-out, account switch
//! or expiry cancels it; every durable step runs inside `scoped_transaction`.
//! Credentials come only from the verified grant authority and are held in
//! memory for at most their ≤300 s lifetime; sign-out and sleep drop them.
//!
//! Delivery happens only at a safe turn boundary of the bound local session
//! and never through the normal (unrestricted) Codex turn path. Heartbeats,
//! notices, answers and status requests are handled without any model call.
//! Work requests reach a [`RestrictedExecutor`] only under the Respond preset
//! and handler admission; the executor sees the message as untrusted data and
//! holds nothing but the per-turn [`ToolGate`].
use super::*;
use crate::cloud::mailbox::{
    grant::{AuthorityError, CreateGrantRequest, CredentialRequest, EndpointPolicy, EnrollRequest, GrantAuthority, GrantDoc},
    policy::{classify, refused_requested_actions, InboundIntent, ParticipantPolicy, Preset, ToolGate},
    wire::{MqCallError, MqGrantTransport, WakeEvent},
};
use crate::cloud::storage::{
    ActingAdmission, DeliveryAdmission, DeliveryFence, DeliverySettlement, MqDeliveryView, MqOutboxView,
    OutboundDisposition, OutboundDraft, ParticipantRecord, ParticipantSpec, PeerRef, SendAdmission,
};
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

/// Fresh identity verification (a network read of the identity document).
pub trait IdentityVerifier: Send + Sync {
    fn verify(&self) -> BoxFuture<'_, Result<IdentityObservation>>;
}

/// Whether the bound local session is between turns. Delivery waits otherwise.
pub trait TurnBoundary: Send + Sync {
    fn session_idle(&self, session_id: String) -> BoxFuture<'_, Result<bool>>;
}

#[derive(Clone, Debug)]
pub struct RestrictedTurn {
    pub thread_id: String,
    pub message_id: String,
    pub correlation_id: Option<String>,
    pub sender: mq_core::Principal,
    /// Untrusted message text. It is data for the turn, never instructions
    /// that can change the grant, tools or budget.
    pub untrusted_body: String,
    pub deadline: Duration,
    pub cost_cap_usd_micros: u64,
}

#[derive(Clone, Debug)]
pub struct RestrictedOutcome {
    pub answer: String,
    pub cost_usd_micros: u64,
}

/// A bounded, tool-restricted execution path. It receives only the gate;
/// every file, artifact, tool, spend or side effect must be authorized by it.
pub trait RestrictedExecutor: Send + Sync {
    fn run(&self, turn: RestrictedTurn, gate: Arc<ToolGate>) -> BoxFuture<'_, Result<RestrictedOutcome>>;
}

#[derive(Clone)]
pub struct MailboxDeps {
    pub origin: String,
    pub verifier: Arc<dyn IdentityVerifier>,
    pub authority: Arc<dyn GrantAuthority>,
    pub boundary: Arc<dyn TurnBoundary>,
    /// No production executor is registered in this release: automatic
    /// Respond handling stays gated and requests wait for an operator.
    pub executor: Option<Arc<dyn RestrictedExecutor>>,
    pub endpoint_policy: EndpointPolicy,
}

#[derive(Clone, Debug)]
pub struct ConnectRequest {
    pub thread_id: String,
    pub local_session_id: String,
    pub peers: Vec<PeerRef>,
    pub preset: Preset,
    pub policy: ParticipantPolicy,
    pub grant_ttl_seconds: u64,
    pub history_after_seq: Option<u64>,
    pub label: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct PassBudget {
    pub max_pages: usize,
    pub max_sends: usize,
    pub max_deliveries: usize,
}
impl Default for PassBudget {
    fn default() -> Self {
        Self { max_pages: 10, max_sends: 20, max_deliveries: 20 }
    }
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MailboxPassReport {
    pub pages: usize,
    pub committed: usize,
    pub gaps: usize,
    pub caught_up: bool,
    pub sent: usize,
    pub accepted: usize,
    pub unknown: usize,
    pub rejected: usize,
    pub fenced: usize,
    pub reconciled: usize,
    pub observed: usize,
    pub answered_without_model: usize,
    pub declined: usize,
    pub expired: usize,
    pub executor_runs: usize,
    pub deferred_busy: bool,
    pub stopped: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxStatus {
    pub generation: u32,
    pub participant: Option<ParticipantRecord>,
    pub outbox: Vec<MqOutboxView>,
    pub deliveries: Vec<MqDeliveryView>,
    pub gaps: Vec<(u64, u64, String)>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSignOut {
    pub revoked: Vec<String>,
    pub unconfirmed: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum OperatorReply {
    Answer(String),
    Decline(String),
}

pub(super) struct CachedTransport {
    scope_generation: u32,
    grant_generation: u64,
    incarnation: u64,
    expires_at: DateTime<Utc>,
    transport: Arc<MqGrantTransport>,
}

#[derive(Default)]
pub(super) struct MailboxCache {
    transports: HashMap<String, CachedTransport>,
}
impl MailboxCache {
    pub(super) fn clear(&mut self) {
        self.transports.clear();
    }
}

/// Stop reasons that end a supervisor: authority is gone, not flaky.
fn terminal_stop(reason: &str) -> bool {
    matches!(reason, "revoked" | "fenced" | "expired" | "not_connected" | "identity_revoked")
}

fn denial_error(error: MqCallError) -> anyhow::Error {
    anyhow::Error::new(error)
}

impl ScopedCloudRuntime {
    async fn identity_now(&self, generation: u32) -> Result<CloudScopeIdentity> {
        let state = self.state.lock().await;
        if state.view.generation != generation {
            bail!("cloud operation was superseded");
        }
        Ok(state.active.as_ref().context("cloud identity unavailable")?.identity.clone())
    }

    /// Drop every cached MQ credential (sign-out, account switch, OS sleep).
    pub async fn fence_mailbox_for_sleep(&self) {
        self.mailbox.lock().await.clear();
    }

    async fn authority_call<T, F, Fut>(&self, generation: u32, call: F) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = std::result::Result<T, AuthorityError>>,
    {
        let result = self.await_scoped(generation, || async { call().await.map_err(anyhow::Error::new) }).await;
        if let Err(error) = &result {
            if error.downcast_ref::<AuthorityError>().is_some_and(AuthorityError::identity_revoked) {
                let _ = self.invalidate().await;
            }
        }
        result
    }

    /// Explicitly connect a selected existing local session to one MQ thread.
    /// Enrolls this device/session (new incarnation), binds the session and
    /// creates or adopts exactly this participant's own grant.
    pub async fn connect_mq_session_with(&self, deps: &MailboxDeps, request: ConnectRequest) -> Result<ParticipantRecord> {
        let generation = self.revalidate_with(&deps.origin, || deps.verifier.verify()).await?.generation;
        let identity = self.identity_now(generation).await?;
        let device_id = self.scoped_transaction(generation, |store, _| store.mq_device_id()).await?;
        let enroll = EnrollRequest { device_id: device_id.clone(), session_id: request.local_session_id.clone(), label: request.label.clone() };
        let enrolled = self.authority_call(generation, || deps.authority.enroll(enroll)).await?;
        let binding = enrolled.validate(&identity, &device_id, &request.local_session_id, deps.endpoint_policy)?;
        let spec = ParticipantSpec {
            thread_id: request.thread_id.clone(),
            local_session_id: request.local_session_id.clone(),
            peers: request.peers.clone(),
            preset: request.preset,
            policy: request.policy.clone(),
        };
        let participant = self.scoped_transaction(generation, move |store, lease| store.connect_mq_participant(&lease, &spec, &binding)).await?;
        let operations = if request.preset.may_publish() { vec!["read".to_owned(), "publish".to_owned()] } else { vec!["read".to_owned()] };
        let create = CreateGrantRequest {
            thread_id: request.thread_id.clone(),
            enrollment_id: participant.enrollment_id.clone(),
            operations,
            ttl_seconds: request.grant_ttl_seconds.clamp(60, 2_592_000),
            history_after_seq: request.history_after_seq,
        };
        let grant = match participant.grant_id.clone() {
            Some(grant_id) => self.authority_call(generation, || deps.authority.get_grant(grant_id)).await?,
            None => match self.authority_call(generation, || deps.authority.create_grant(create)).await {
                Ok(grant) => grant,
                // §3/§5: a duplicate or an uncertain create is resolved by a
                // read, never by retrying the mutation.
                Err(error) if error.downcast_ref::<AuthorityError>().is_some_and(|e| matches!(e, AuthorityError::Uncertain(_)) || e.code() == Some("grant_exists")) => {
                    let (enrollment, thread) = (participant.enrollment_id.clone(), request.thread_id.clone());
                    let grants = self.authority_call(generation, || deps.authority.list_grants(enrollment, thread)).await?;
                    grants.into_iter().find(|grant| grant.enrollment_id == participant.enrollment_id && grant.thread_id == request.thread_id).context("grant creation outcome unknown and no grant is listed")?
                }
                Err(error) => return Err(error),
            },
        };
        self.attach_grant_doc(generation, &request.thread_id, &participant, &grant).await
    }

    /// A new process re-enrolls (fresh incarnation, fencing any older
    /// process), then re-reads its grant. No connection is created here.
    pub async fn resume_mq_session_with(&self, deps: &MailboxDeps, thread_id: &str) -> Result<ParticipantRecord> {
        let generation = self.revalidate_with(&deps.origin, || deps.verifier.verify()).await?.generation;
        let identity = self.identity_now(generation).await?;
        let thread = thread_id.to_owned();
        let participant = self.scoped_transaction(generation, move |store, lease| store.mq_participant(&lease, &thread)).await?.context("MQ thread has no connected local participant")?;
        let enroll = EnrollRequest { device_id: participant.device_id.clone(), session_id: participant.local_session_id.clone(), label: None };
        let enrolled = self.authority_call(generation, || deps.authority.enroll(enroll)).await?;
        let binding = enrolled.validate(&identity, &participant.device_id, &participant.local_session_id, deps.endpoint_policy)?;
        let thread = thread_id.to_owned();
        let participant = self.scoped_transaction(generation, move |store, lease| store.refresh_mq_incarnation(&lease, &thread, &binding)).await?;
        self.mailbox.lock().await.transports.remove(thread_id);
        let grant_id = participant.grant_id.clone().context("participant has no grant")?;
        let grant = self.authority_call(generation, || deps.authority.get_grant(grant_id)).await?;
        self.attach_grant_doc(generation, thread_id, &participant, &grant).await
    }

    async fn attach_grant_doc(&self, generation: u32, thread_id: &str, participant: &ParticipantRecord, grant: &GrantDoc) -> Result<ParticipantRecord> {
        let snapshot = grant.validate(participant)?;
        let thread = thread_id.to_owned();
        self.scoped_transaction(generation, move |store, lease| store.attach_mq_grant(&lease, &thread, &snapshot)).await
    }

    /// Queue an outbound message locally. Nothing is sent here.
    pub async fn publish_mq_with(&self, deps: &MailboxDeps, thread_id: &str, draft: OutboundDraft) -> Result<MqOutboxView> {
        let generation = self.revalidate_with(&deps.origin, || deps.verifier.verify()).await?.generation;
        let thread = thread_id.to_owned();
        self.scoped_transaction(generation, move |store, lease| store.enqueue_mq_publish(&lease, &thread, &draft)).await
    }

    /// Operator (human) answer or decline for an observed request. This is
    /// the existing authority path for Collaborate; no model is involved.
    pub async fn answer_mq_with(&self, deps: &MailboxDeps, thread_id: &str, message_id: &str, reply: OperatorReply) -> Result<MqOutboxView> {
        let generation = self.revalidate_with(&deps.origin, || deps.verifier.verify()).await?.generation;
        let (thread, message) = (thread_id.to_owned(), message_id.to_owned());
        self.scoped_transaction(generation, move |store, lease| {
            let participant = store.mq_participant(&lease, &thread)?.context("MQ thread has no connected local participant")?;
            let delivery = store.mq_delivery(&lease, &thread, &message)?;
            let inbound = delivery.message.clone().context("delivery message unavailable")?;
            let fence = participant_fence(&participant)?;
            let (settlement, draft, detail) = match reply {
                OperatorReply::Answer(body) => (DeliverySettlement::Answered, reply_draft(&participant, &inbound, &delivery, OutboundDisposition::Answer, body, json!({"by":"operator"})), json!({"by":"operator"})),
                OperatorReply::Decline(reason) => (DeliverySettlement::Declined, reply_draft(&participant, &inbound, &delivery, OutboundDisposition::Decline, format!("Declined: {reason}"), json!({"reason": reason})), json!({"by":"operator","reason":reason})),
            };
            store
                .settle_mq_delivery(&lease, &thread, &message, Some(&fence), settlement, &detail, Some(&draft))?
                .context("reply was not queued")
        }).await
    }

    /// Local status read: no model call and no MQ traffic.
    pub async fn mailbox_status_with(&self, deps: &MailboxDeps, thread_id: &str) -> Result<MailboxStatus> {
        let generation = self.revalidate_with(&deps.origin, || deps.verifier.verify()).await?.generation;
        let thread = thread_id.to_owned();
        let (participant, outbox, deliveries, gaps) = self.scoped_transaction(generation, move |store, lease| {
            let participant = store.mq_participant(&lease, &thread)?;
            if participant.is_none() {
                return Ok((None, Vec::new(), Vec::new(), Vec::new()));
            }
            Ok((participant, store.mq_outbox(&lease, &thread, 500)?, store.mq_deliveries(&lease, &thread, &[], 500)?, store.mq_history_gaps(&lease, &thread)?))
        }).await?;
        Ok(MailboxStatus { generation, participant, outbox, deliveries, gaps })
    }

    /// Revoke this participant's grant. A lost response is resolved by a
    /// fresh read (§5); queued writes and open deliveries are fenced.
    pub async fn disconnect_mq_with(&self, deps: &MailboxDeps, thread_id: &str) -> Result<ParticipantRecord> {
        let generation = self.revalidate_with(&deps.origin, || deps.verifier.verify()).await?.generation;
        let thread = thread_id.to_owned();
        let participant = self.scoped_transaction(generation, move |store, lease| store.mq_participant(&lease, &thread)).await?.context("MQ thread has no connected local participant")?;
        let grant_id = participant.grant_id.clone().context("participant has no grant")?;
        let revoke_id = grant_id.clone();
        let grant = match self.authority_call(generation, || deps.authority.revoke_grant(revoke_id)).await {
            Ok(grant) => grant,
            Err(error) if error.downcast_ref::<AuthorityError>().is_some_and(|e| matches!(e, AuthorityError::Uncertain(_))) => {
                self.authority_call(generation, || deps.authority.get_grant(grant_id)).await?
            }
            Err(error) => return Err(error),
        };
        self.mailbox.lock().await.transports.remove(thread_id);
        self.attach_grant_doc(generation, thread_id, &participant, &grant).await
    }

    /// Device sign-out (grant contract v2 §5.1). Revokes every enrollment the
    /// account's participants use (server-side: all grants and incarnations
    /// refused, queued deliveries dead-lettered), fences local queued writes
    /// and open deliveries, then signs out locally, which cancels network work
    /// and drops credentials. A lost revoke response is resolved by a GET,
    /// never a retried mutation. The local sign-out always happens; any
    /// enrollment not confirmed revoked is reported as unconfirmed.
    pub async fn sign_out_device_with(&self, deps: &MailboxDeps) -> Result<DeviceSignOut> {
        let generation = self.revalidate_with(&deps.origin, || deps.verifier.verify()).await?.generation;
        let participants = self.scoped_transaction(generation, |store, lease| store.mq_participants(&lease)).await?;
        let mut enrollments: Vec<String> = participants.iter().map(|p| p.enrollment_id.clone()).collect();
        enrollments.sort();
        enrollments.dedup();
        let mut outcome = DeviceSignOut::default();
        for enrollment in enrollments {
            let id = enrollment.clone();
            let result = match self.authority_call(generation, || deps.authority.revoke_enrollment(id)).await {
                Ok(document) => Ok(document),
                Err(error) if error.downcast_ref::<AuthorityError>().is_some_and(|e| matches!(e, AuthorityError::Uncertain(_))) => {
                    let id = enrollment.clone();
                    self.authority_call(generation, || deps.authority.get_enrollment(id)).await
                }
                Err(error) => Err(error),
            };
            match result {
                Ok(document) if document.revoked_at.is_some() && document.enrollment_id == enrollment => outcome.revoked.push(enrollment),
                _ => outcome.unconfirmed.push(enrollment),
            }
        }
        for participant in participants.iter().filter(|p| outcome.revoked.contains(&p.enrollment_id)) {
            let thread = participant.thread_id.clone();
            self.scoped_transaction(generation, move |store, lease| store.set_mq_participant_state(&lease, &thread, "revoked", "enrollment_revoked")).await?;
        }
        self.invalidate().await?;
        Ok(outcome)
    }

    /// Obtain a transport from a freshly issued, validated grant credential.
    async fn mailbox_transport(&self, generation: u32, deps: &MailboxDeps, participant: &ParticipantRecord) -> Result<std::result::Result<(Arc<MqGrantTransport>, ParticipantRecord), String>> {
        {
            let cache = self.mailbox.lock().await;
            if let Some(cached) = cache.transports.get(&participant.thread_id) {
                if cached.scope_generation == generation
                    && Some(cached.grant_generation) == participant.grant_generation
                    && cached.incarnation == participant.incarnation
                    && cached.expires_at > Utc::now() + chrono::Duration::seconds(30)
                {
                    return Ok(Ok((cached.transport.clone(), participant.clone())));
                }
            }
        }
        let request = CredentialRequest {
            grant_id: participant.grant_id.clone().context("participant has no grant")?,
            enrollment_id: participant.enrollment_id.clone(),
            incarnation: participant.incarnation,
            ttl_seconds: Some(300),
        };
        let credential = match self.authority_call(generation, || deps.authority.credential(request)).await {
            Ok(credential) => credential,
            Err(error) => {
                let Some(code) = error.downcast_ref::<AuthorityError>().and_then(|e| e.code().map(str::to_owned)) else { return Err(error) };
                let thread = participant.thread_id.clone();
                let terminal = match code.as_str() {
                    "grant_revoked" | "grant_membership_required" | "enrollment_revoked" => Some(("revoked", code.clone())),
                    "grant_expired" => Some(("expired", code.clone())),
                    "grant_incarnation_fenced" | "invalid_incarnation" => Some(("fenced", code.clone())),
                    "desktop_cloud_identity_revoked_or_unavailable" => return Ok(Err("identity_revoked".into())),
                    _ => None,
                };
                if let Some((state, reason)) = terminal {
                    self.scoped_transaction(generation, move |store, lease| store.set_mq_participant_state(&lease, &thread, state, &reason)).await?;
                    return Ok(Err(state.into()));
                }
                return Err(error);
            }
        };
        let snapshot = credential.validate(participant, Utc::now(), deps.endpoint_policy)?;
        let thread = participant.thread_id.clone();
        let updated = self.scoped_transaction(generation, move |store, lease| store.attach_mq_grant(&lease, &thread, &snapshot)).await?;
        let thread_uuid = uuid::Uuid::parse_str(&participant.thread_id).context("MQ thread id must be a UUID")?;
        let transport = Arc::new(MqGrantTransport::try_new(&credential.mq_endpoint, credential.token.clone(), mq_core::ThreadId(thread_uuid), deps.endpoint_policy)?);
        self.mailbox.lock().await.transports.insert(participant.thread_id.clone(), CachedTransport {
            scope_generation: generation,
            grant_generation: credential.grant.generation,
            incarnation: participant.incarnation,
            expires_at: credential.expires_at,
            transport: transport.clone(),
        });
        Ok(Ok((transport, updated)))
    }

    /// Map an MQ authorization denial onto participant state (contract §7).
    async fn handle_mq_denial(&self, generation: u32, thread_id: &str, code: &str) -> Result<String> {
        self.mailbox.lock().await.transports.remove(thread_id);
        let (state, reason) = match code {
            "grant_revoked" | "grant_membership_required" | "enrollment_revoked" => ("revoked", code),
            "grant_expired" => ("expired", code),
            "grant_incarnation_fenced" => ("fenced", code),
            // Stale generation or an expired/unknown credential: one fresh
            // credential on the next pass; revocation shows up there.
            _ => return Ok(format!("credential_refresh:{code}")),
        };
        let thread = thread_id.to_owned();
        let reason = reason.to_owned();
        self.scoped_transaction(generation, move |store, lease| store.set_mq_participant_state(&lease, &thread, state, &reason)).await?;
        Ok(state.into())
    }

    /// One bounded mailbox pass. Safe to call repeatedly (poll, wake, E02/E03
    /// driver). Never resends an uncertain publication.
    pub async fn mailbox_pass_with(&self, deps: &MailboxDeps, thread_id: &str, budget: PassBudget) -> Result<MailboxPassReport> {
        let mut report = MailboxPassReport::default();
        let generation = self.revalidate_with(&deps.origin, || deps.verifier.verify()).await?.generation;
        let thread = thread_id.to_owned();
        let Some(participant) = self.scoped_transaction(generation, move |store, lease| store.mq_participant(&lease, &thread)).await? else {
            report.stopped = Some("not_connected".into());
            return Ok(report);
        };
        if matches!(participant.state.as_str(), "revoked" | "fenced" | "awaiting_grant") {
            report.stopped = Some(participant.state.clone());
            return Ok(report);
        }
        let (transport, participant) = match self.mailbox_transport(generation, deps, &participant).await? {
            Ok(value) => value,
            Err(stop) => {
                if stop == "identity_revoked" {
                    let _ = self.invalidate().await;
                }
                report.stopped = Some(stop);
                return Ok(report);
            }
        };
        if participant.state != "active" {
            report.stopped = Some(participant.state.clone());
            return Ok(report);
        }

        // 1. Granted-history catch-up, committed page by page with the cursor.
        for _ in 0..budget.max_pages.clamp(1, 100) {
            let thread = thread_id.to_owned();
            let (expected, cursor) = self.scoped_transaction(generation, move |store, lease| store.mq_history_cursor(&lease, &thread)).await?;
            let page = match self.await_scoped(generation, || async { transport.history(cursor, 200).await.map_err(denial_error) }).await {
                Ok(page) => page,
                Err(error) => match error.downcast_ref::<MqCallError>() {
                    Some(MqCallError::Denied { code, .. }) => {
                        report.stopped = Some(self.handle_mq_denial(generation, thread_id, code).await?);
                        return Ok(report);
                    }
                    _ => return Err(error),
                },
            };
            let has_more = page.has_more;
            let thread = thread_id.to_owned();
            let commit = self.scoped_transaction(generation, move |store, lease| store.commit_mq_history(&lease, &thread, expected.as_ref(), &page)).await?;
            report.pages += 1;
            report.committed += commit.committed;
            report.reconciled += commit.own_reconciled.len();
            report.gaps += usize::from(commit.gap.is_some());
            if !has_more {
                report.caught_up = true;
                break;
            }
        }

        // 2. Flush queued writes with their original identities.
        let thread = thread_id.to_owned();
        let queued = self.scoped_transaction(generation, move |store, lease| store.mq_outbox_ids(&lease, &thread, "pending", budget.max_sends.clamp(1, 200))).await?;
        for command_id in queued {
            let (thread, id) = (thread_id.to_owned(), command_id.clone());
            let admission = self.scoped_transaction(generation, move |store, lease| store.begin_mq_send(&lease, &thread, &id)).await?;
            let request = match admission {
                SendAdmission::Send(request) => request,
                SendAdmission::Fenced(_) => {
                    report.fenced += 1;
                    continue;
                }
                SendAdmission::Deferred(_) => break,
            };
            report.sent += 1;
            let publish = request.publish.clone();
            let outcome = self.await_scoped(generation, || async { transport.publish(publish).await.map_err(denial_error) }).await;
            let id = command_id.clone();
            match outcome {
                Ok(message) => {
                    let (message_id, seq) = (message.message_id.0.to_string(), message.seq);
                    self.scoped_transaction(generation, move |store, lease| store.record_mq_accepted(&lease, &id, &message_id, seq, "publish")).await?;
                    report.accepted += 1;
                }
                Err(error) => match error.downcast_ref::<MqCallError>().cloned() {
                    Some(MqCallError::Denied { status, code }) => {
                        let detail = json!({"status": status, "code": code});
                        self.scoped_transaction(generation, move |store, lease| store.record_mq_rejected(&lease, &id, false, &detail)).await?;
                        report.rejected += 1;
                        report.stopped = Some(self.handle_mq_denial(generation, thread_id, &code).await?);
                        break;
                    }
                    Some(MqCallError::Rejected { status, code }) => {
                        let detail = json!({"status": status, "code": code});
                        self.scoped_transaction(generation, move |store, lease| store.record_mq_rejected(&lease, &id, status == 409, &detail)).await?;
                        report.rejected += 1;
                    }
                    // Uncertain (or cancelled mid-flight): the entry stays
                    // outcome_unknown. Stop so later writes keep their order.
                    _ => {
                        report.unknown += 1;
                        if self.state.lock().await.view.generation != generation {
                            return Err(error);
                        }
                        break;
                    }
                },
            }
        }

        // 3. Reconcile uncertain sends through authoritative history. Own
        // publications found while catching up already settled above; what
        // remains after a complete catch-up is recorded as absent-so-far and
        // stays unknown. Nothing is resent.
        if report.caught_up {
            let thread = thread_id.to_owned();
            let unknown = self.scoped_transaction(generation, move |store, lease| {
                let (_, cursor) = store.mq_history_cursor(&lease, &thread)?;
                let ids = store.mq_outbox_ids(&lease, &thread, "outcome_unknown", 50)?;
                for id in &ids {
                    store.record_mq_lookup(&lease, id, &json!({"method":"granted_history","absentThroughSeq":cursor,"checkedAt":Utc::now().to_rfc3339(),"outcome":"unknown"}))?;
                }
                Ok(ids.len())
            }).await?;
            report.unknown = report.unknown.max(unknown);
        }

        // 4. Expire overdue deliveries: never executed late.
        let thread = thread_id.to_owned();
        let overdue = self.scoped_transaction(generation, move |store, lease| store.overdue_mq_deliveries(&lease, &thread, Utc::now().timestamp_millis(), 50)).await?;
        for delivery in overdue {
            self.expire_delivery(generation, &participant, delivery).await?;
            report.expired += 1;
        }

        // 5. Deliver at a safe turn boundary through the restricted path.
        if !deps.boundary.session_idle(participant.local_session_id.clone()).await? {
            report.deferred_busy = true;
            return Ok(report);
        }
        let fence = participant_fence(&participant)?;
        let thread = thread_id.to_owned();
        let pending = self.scoped_transaction(generation, move |store, lease| store.mq_deliveries(&lease, &thread, &["delivered"], budget.max_deliveries.clamp(1, 200))).await?;
        for delivery in pending {
            let (thread, id, check) = (thread_id.to_owned(), delivery.message_id.clone(), fence.clone());
            let view = match self.scoped_transaction(generation, move |store, lease| store.observe_mq_delivery(&lease, &thread, &id, &check)).await? {
                DeliveryAdmission::Observed(view) => view,
                DeliveryAdmission::Fenced(_) => {
                    report.fenced += 1;
                    continue;
                }
            };
            report.observed += 1;
            let message = view.message.clone().context("delivery message unavailable")?;
            match classify(&message) {
                InboundIntent::Heartbeat | InboundIntent::Notice | InboundIntent::Answer => {}
                InboundIntent::StatusRequest => {
                    self.answer_status(generation, &participant, &message, &view).await?;
                    report.answered_without_model += 1;
                }
                InboundIntent::WorkRequest => {
                    let refused = refused_requested_actions(&message.payload);
                    if !refused.is_empty() {
                        self.decline(generation, &participant, &message, &view, json!({"reasons": refused}), true).await?;
                        report.declined += 1;
                    } else if view.causal_depth >= participant.policy.limits.max_causal_depth {
                        // Loop guard: record locally, do not reply.
                        self.decline(generation, &participant, &message, &view, json!({"reasons": ["causal_depth_exceeded"]}), false).await?;
                        report.declined += 1;
                    } else if participant.preset.automatic_handlers() {
                        if let Some(executor) = deps.executor.clone() {
                            match self.run_restricted(generation, &participant, &fence, executor, &message, &view).await? {
                                DeliverySettlement::Answered => report.executor_runs += 1,
                                DeliverySettlement::Declined => report.declined += 1,
                                DeliverySettlement::Expired => report.expired += 1,
                            }
                        }
                        // Without a registered executor the request stays
                        // observed and waits for an operator answer.
                    }
                }
            }
        }
        Ok(report)
    }

    async fn settle(&self, generation: u32, participant: &ParticipantRecord, view: &MqDeliveryView, settlement: DeliverySettlement, detail: Value, reply: Option<OutboundDraft>, fenced: bool) -> Result<()> {
        let (thread, id) = (participant.thread_id.clone(), view.message_id.clone());
        let fence = if fenced { Some(participant_fence(participant)?) } else { None };
        let reply = reply.filter(|_| participant.can_publish());
        self.scoped_transaction(generation, move |store, lease| store.settle_mq_delivery(&lease, &thread, &id, fence.as_ref(), settlement, &detail, reply.as_ref())).await?;
        Ok(())
    }

    async fn answer_status(&self, generation: u32, participant: &ParticipantRecord, message: &mq_core::Message, view: &MqDeliveryView) -> Result<()> {
        let thread = participant.thread_id.clone();
        let counts = self.scoped_transaction(generation, move |store, lease| {
            let open = store.mq_deliveries(&lease, &thread, &["delivered", "observed", "acting"], 500)?.len();
            let queued = store.mq_outbox(&lease, &thread, 500)?.into_iter().filter(|entry| entry.status == "queued").count();
            Ok((open, queued))
        }).await?;
        // Authoritative local projection only: availability and counts.
        let status = json!({"participant": participant.principal_id, "state": participant.state, "preset": participant.preset, "openRequests": counts.0, "queuedReplies": counts.1});
        let draft = reply_draft(participant, message, view, OutboundDisposition::Answer, status.to_string(), json!({"status": status}));
        self.settle(generation, participant, view, DeliverySettlement::Answered, json!({"handler":"status","model":false}), Some(draft), true).await
    }

    async fn decline(&self, generation: u32, participant: &ParticipantRecord, message: &mq_core::Message, view: &MqDeliveryView, detail: Value, reply: bool) -> Result<()> {
        let draft = reply.then(|| reply_draft(participant, message, view, OutboundDisposition::Decline, format!("Declined: {detail}"), detail.clone()));
        self.settle(generation, participant, view, DeliverySettlement::Declined, detail, draft, true).await
    }

    async fn expire_delivery(&self, generation: u32, participant: &ParticipantRecord, view: MqDeliveryView) -> Result<()> {
        let Some(message) = view.message.clone() else { return Ok(()) };
        let request = matches!(classify(&message), InboundIntent::WorkRequest | InboundIntent::StatusRequest);
        let draft = request.then(|| reply_draft(participant, &message, &view, OutboundDisposition::Expiry, "Expired before it could be handled.".into(), json!({"reason":"expired"})));
        self.settle(generation, participant, &view, DeliverySettlement::Expired, json!({"reason":"deadline_passed"}), draft, false).await
    }

    async fn run_restricted(&self, generation: u32, participant: &ParticipantRecord, fence: &DeliveryFence, executor: Arc<dyn RestrictedExecutor>, message: &mq_core::Message, view: &MqDeliveryView) -> Result<DeliverySettlement> {
        let (thread, id, check, session) = (participant.thread_id.clone(), view.message_id.clone(), fence.clone(), participant.local_session_id.clone());
        let (admission, budget) = self.scoped_transaction(generation, move |store, lease| {
            let admission = store.begin_mq_acting(&lease, &thread, &id, &check, Utc::now().timestamp_millis())?;
            Ok((admission, store.mq_work_budget(&lease, &session)?))
        }).await?;
        if let ActingAdmission::Refused(reason) = admission {
            let reply = reason != "causal_depth_exceeded";
            if reason == "expired" {
                self.expire_delivery(generation, participant, view.clone()).await?;
                return Ok(DeliverySettlement::Expired);
            }
            self.decline(generation, participant, message, view, json!({"reasons":[reason]}), reply).await?;
            return Ok(DeliverySettlement::Declined);
        }
        let limits = participant.policy.limits;
        let cost_cap = limits.max_cost_usd_micros.min(budget);
        let gate = Arc::new(ToolGate::new(participant.policy.clone(), cost_cap));
        let deadline = view
            .deadline_ms
            .map(|ms| Duration::from_millis(u64::try_from(ms - Utc::now().timestamp_millis()).unwrap_or(0)))
            .unwrap_or(Duration::from_secs(u64::from(limits.deadline_secs)))
            .min(Duration::from_secs(u64::from(limits.deadline_secs)));
        let turn = RestrictedTurn {
            thread_id: participant.thread_id.clone(),
            message_id: view.message_id.clone(),
            correlation_id: message.correlation_id.clone(),
            sender: message.sender.clone(),
            untrusted_body: message.body.clone(),
            deadline,
            cost_cap_usd_micros: cost_cap,
        };
        let run_gate = gate.clone();
        let outcome = self.await_scoped(generation, || async move {
            Ok(tokio::time::timeout(deadline, executor.run(turn, run_gate)).await)
        }).await?;
        let audit = serde_json::to_value(gate.audit())?;
        match outcome {
            Ok(Ok(outcome)) if outcome.cost_usd_micros <= cost_cap => {
                let draft = reply_draft(participant, message, view, OutboundDisposition::Answer, outcome.answer, json!({"handler":"restricted"}));
                self.settle(generation, participant, view, DeliverySettlement::Answered, json!({"handler":"restricted","gate":audit,"costUsdMicros":outcome.cost_usd_micros}), Some(draft), true).await?;
                Ok(DeliverySettlement::Answered)
            }
            Ok(Ok(outcome)) => {
                self.decline(generation, participant, message, view, json!({"reasons":["exceeds_work_authorization"],"reportedCost":outcome.cost_usd_micros,"gate":audit}), true).await?;
                Ok(DeliverySettlement::Declined)
            }
            Ok(Err(_)) => {
                self.decline(generation, participant, message, view, json!({"reasons":["handler_failed"],"gate":audit}), true).await?;
                Ok(DeliverySettlement::Declined)
            }
            Err(_) => {
                let draft = reply_draft(participant, message, view, OutboundDisposition::Expiry, "Handler deadline reached.".into(), json!({"reason":"handler_deadline"}));
                self.settle(generation, participant, view, DeliverySettlement::Expired, json!({"reason":"handler_deadline","gate":audit}), Some(draft), true).await?;
                Ok(DeliverySettlement::Expired)
            }
        }
    }

    /// Long-running outward poll with SSE wake-then-fetch. Ends on explicit
    /// sign-out, cancellation or terminal authority loss; transient failures
    /// back off. OS sleep (wall clock jumping past the monotonic clock) drops
    /// cached credentials so the next pass re-verifies identity and grant.
    pub fn spawn_mailbox_supervisor(&self, deps: MailboxDeps, thread_id: String, config: MailboxLoopConfig) -> MailboxSupervisor {
        let (cancel, mut cancelled) = watch::channel(false);
        let runtime = self.clone();
        let task = tokio::spawn(async move {
            let mut signouts = runtime.signouts.subscribe();
            signouts.borrow_and_update();
            let mut backoff = config.poll_interval;
            let mut wakes: Option<tokio::sync::mpsc::Receiver<WakeEvent>> = None;
            let mut wake_task: Option<tokio::task::JoinHandle<()>> = None;
            let mut passes = 0u64;
            let exit = loop {
                let pass = tokio::select! {
                    biased;
                    _ = cancelled.changed() => break MailboxExit::Cancelled,
                    _ = signouts.changed() => break MailboxExit::SignedOut,
                    pass = runtime.mailbox_pass_with(&deps, &thread_id, config.budget) => pass,
                };
                passes += 1;
                match pass {
                    Ok(report) => {
                        if let Some(stop) = report.stopped.as_deref().filter(|stop| terminal_stop(stop)) {
                            break MailboxExit::Stopped(stop.to_owned());
                        }
                        backoff = config.poll_interval;
                        if config.use_wakes && wake_task.as_ref().is_none_or(|task| task.is_finished()) {
                            let transport = runtime.mailbox.lock().await.transports.get(&thread_id).map(|cached| cached.transport.clone());
                            if let Some(transport) = transport {
                                let (sender, receiver) = tokio::sync::mpsc::channel(8);
                                wakes = Some(receiver);
                                wake_task = Some(tokio::spawn(async move {
                                    let Ok(mut stream) = transport.wakes().await else { return };
                                    while let Some(Ok(event)) = stream.next().await {
                                        if sender.send(event).await.is_err() || event == WakeEvent::Revoked {
                                            break;
                                        }
                                    }
                                }));
                            }
                        }
                    }
                    Err(_) => backoff = (backoff * 2).min(config.max_backoff),
                }
                let started = (std::time::Instant::now(), std::time::SystemTime::now());
                tokio::select! {
                    biased;
                    _ = cancelled.changed() => break MailboxExit::Cancelled,
                    _ = signouts.changed() => break MailboxExit::SignedOut,
                    _ = tokio::time::sleep(backoff) => {}
                    Some(_event) = async { match wakes.as_mut() { Some(receiver) => receiver.recv().await, None => std::future::pending().await } } => {}
                }
                let monotonic = started.0.elapsed();
                let wall = started.1.elapsed().unwrap_or(monotonic);
                if wall.saturating_sub(monotonic) > config.sleep_gap {
                    runtime.fence_mailbox_for_sleep().await;
                }
            };
            if let Some(task) = wake_task {
                task.abort();
            }
            (exit, passes)
        });
        MailboxSupervisor { cancel, task }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MailboxLoopConfig {
    pub poll_interval: Duration,
    pub max_backoff: Duration,
    pub sleep_gap: Duration,
    pub use_wakes: bool,
    pub budget: PassBudget,
}
impl Default for MailboxLoopConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(15),
            max_backoff: Duration::from_secs(120),
            sleep_gap: Duration::from_secs(30),
            use_wakes: true,
            budget: PassBudget::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MailboxExit {
    Cancelled,
    SignedOut,
    Stopped(String),
}

pub struct MailboxSupervisor {
    cancel: watch::Sender<bool>,
    task: tokio::task::JoinHandle<(MailboxExit, u64)>,
}
impl MailboxSupervisor {
    pub async fn stop(self) -> Result<(MailboxExit, u64)> {
        let _ = self.cancel.send(true);
        self.task.await.context("join mailbox supervisor")
    }
    pub async fn join(self) -> Result<(MailboxExit, u64)> {
        self.task.await.context("join mailbox supervisor")
    }
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

#[cfg(test)]
mod tests;

fn participant_fence(participant: &ParticipantRecord) -> Result<DeliveryFence> {
    Ok(DeliveryFence {
        local_session_id: participant.local_session_id.clone(),
        incarnation: participant.incarnation,
        grant_generation: participant.grant_generation.context("participant grant is not attached")?,
    })
}

/// Correlated reply: preserves the request's correlation id (or uses its
/// message id), names it as parent and causation, targets the sender only if
/// they are a named peer, and advances the hop count. The local id is
/// deterministic, so a crash cannot mint a second reply.
fn reply_draft(participant: &ParticipantRecord, message: &mq_core::Message, view: &MqDeliveryView, disposition: OutboundDisposition, body: String, extra: Value) -> OutboundDraft {
    let message_id = message.message_id.0.to_string();
    let sender = PeerRef {
        kind: serde_json::to_value(message.sender.kind).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default(),
        id: message.sender.id.clone(),
        org_id: message.sender.org_id.clone(),
    };
    let recipients = if participant.peers.contains(&sender) { vec![sender] } else { Vec::new() };
    let tag = match disposition {
        OutboundDisposition::Message => "message",
        OutboundDisposition::Answer => "answer",
        OutboundDisposition::Decline => "decline",
        OutboundDisposition::Expiry => "expiry",
    };
    OutboundDraft {
        local_message_id: format!("reply-{message_id}"),
        kind: mq_core::MessageKind::Answer,
        body,
        payload: json!({"disposition": tag, "in_reply_to": message_id, "detail": extra}),
        correlation_id: Some(message.correlation_id.clone().unwrap_or_else(|| message_id.clone())),
        causation_id: Some(message_id.clone()),
        parent_message_id: Some(message_id),
        recipients,
        disposition,
        reply_to_message_id: Some(view.message_id.clone()),
        causal_depth: view.causal_depth.saturating_add(1),
    }
}
