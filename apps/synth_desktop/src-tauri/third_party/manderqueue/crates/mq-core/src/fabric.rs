use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::grants::*;
use crate::memory::MemoryStore;
use crate::store::Store;
use crate::types::*;
use crate::wake::{NoopWake, Wake};

/// Time source for grant expiry. Tests inject a controllable clock.
pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// Verified credential authority for a thread read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryAuthority {
    /// Unrestricted principal credential; persisted membership applies.
    Membership,
    /// Legacy signed `thread_scope` with participant generation.
    Scoped { generation: u64 },
    /// Asymmetric grant credential.
    Grant(GrantFence),
}

/// Fabric API over any [`Store`] (memory or Postgres).
#[derive(Clone)]
pub struct Fabric {
    store: Arc<dyn Store>,
    wake: Arc<dyn Wake>,
    clock: Clock,
}

impl Default for Fabric {
    fn default() -> Self {
        Self::memory()
    }
}

fn system_clock() -> Clock {
    Arc::new(Utc::now)
}

impl Fabric {
    pub fn memory() -> Self {
        Self {
            store: Arc::new(MemoryStore::default()),
            wake: Arc::new(NoopWake),
            clock: system_clock(),
        }
    }

    pub fn from_store(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            wake: Arc::new(NoopWake),
            clock: system_clock(),
        }
    }

    pub fn with_wake(mut self, wake: Arc<dyn Wake>) -> Self {
        self.wake = wake;
        self
    }

    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    pub fn now(&self) -> DateTime<Utc> {
        (self.clock)()
    }

    pub fn store(&self) -> Arc<dyn Store> {
        self.store.clone()
    }

    pub fn wake(&self) -> Arc<dyn Wake> {
        self.wake.clone()
    }

    pub async fn create_thread(&self, actor: &Principal, req: CreateThread) -> Result<Thread> {
        // Workspace is the caller's org — never trust a different org_id in the body.
        if req.org_id != actor.org_id {
            return Err(Error::Forbidden("org_workspace_mismatch"));
        }
        if let Some(key) = req.idempotency_key.as_deref() {
            if let Some(existing) = self
                .store
                .find_thread_by_idempotency(&actor.org_id, key)
                .await?
            {
                // Ensure caller can see it (must be member).
                self.require_cap(actor, existing.thread_id, Cap::Read)
                    .await?;
                return Ok(existing);
            }
        }
        if req.participants.is_empty() {
            return Err(Error::Invalid("participants_required"));
        }
        let participants: Vec<Participant> = req
            .participants
            .into_iter()
            .map(|p| p.normalize())
            .collect();
        if !participants.iter().any(|p| p.principal == *actor) {
            return Err(Error::Invalid("creator_must_be_participant"));
        }
        let creator = participants
            .iter()
            .find(|p| p.principal == *actor)
            .expect("creator present");
        if creator.role != Role::Owner {
            return Err(Error::Invalid("creator_must_be_owner"));
        }
        let owners = participants
            .iter()
            .filter(|p| p.role == Role::Owner)
            .count();
        if owners != 1 {
            return Err(Error::Invalid("exactly_one_owner_required"));
        }
        for p in &participants {
            if p.principal.org_id != actor.org_id {
                return Err(Error::Forbidden("org_workspace_mismatch"));
            }
        }
        let req = CreateThread {
            org_id: actor.org_id.clone(),
            scope: req.scope,
            title: req.title,
            participants,
            idempotency_key: req.idempotency_key,
        };
        let thread = self.store.insert_thread(actor, req).await?;
        // Concurrent ensure may return the winner row; require membership.
        self.require_cap(actor, thread.thread_id, Cap::Read).await?;
        Ok(thread)
    }

    /// Alias for create with idempotency — safe for SMR/Intern binding races.
    pub async fn ensure_thread(&self, actor: &Principal, req: CreateThread) -> Result<Thread> {
        if req.idempotency_key.as_ref().is_none_or(|k| k.is_empty()) {
            return Err(Error::Invalid("idempotency_key_required_for_ensure"));
        }
        self.create_thread(actor, req).await
    }

    pub async fn get_thread(&self, actor: &Principal, thread_id: ThreadId) -> Result<Thread> {
        let thread = self
            .store
            .get_thread(thread_id)
            .await?
            .ok_or(Error::NotFound("thread"))?;
        // Cross-org: indistinguishable from missing (no existence leak).
        if thread.org_id != actor.org_id {
            return Err(Error::NotFound("thread"));
        }
        self.require_cap(actor, thread_id, Cap::Read).await?;
        Ok(thread)
    }

    pub async fn list_threads(
        &self,
        actor: &Principal,
        scope: Option<&ScopeBinding>,
    ) -> Result<Vec<Thread>> {
        // Workspace = token org only. Never list another org's threads.
        let mut out = Vec::new();
        for t in self.store.list_threads(&actor.org_id, scope).await? {
            if t.org_id != actor.org_id {
                continue; // defense in depth
            }
            if self.store.has_cap(t.thread_id, actor, Cap::Read).await? {
                out.push(t);
            }
        }
        Ok(out)
    }

    async fn load_workspace_thread(
        &self,
        actor: &Principal,
        thread_id: ThreadId,
    ) -> Result<Thread> {
        let thread = self
            .store
            .get_thread(thread_id)
            .await?
            .ok_or(Error::NotFound("thread"))?;
        if thread.org_id != actor.org_id {
            return Err(Error::NotFound("thread"));
        }
        Ok(thread)
    }

    pub async fn add_participant(
        &self,
        actor: &Principal,
        thread_id: ThreadId,
        participant: Participant,
    ) -> Result<()> {
        self.store
            .mutate_participant(actor, thread_id, participant.normalize(), true)
            .await
    }

    pub async fn set_participant_role(
        &self,
        actor: &Principal,
        thread_id: ThreadId,
        principal: &Principal,
        role: Role,
    ) -> Result<()> {
        self.store
            .mutate_participant(
                actor,
                thread_id,
                Participant::new(principal.clone(), role),
                false,
            )
            .await
    }

    /// Queued delivery to an enrollment principal needs a live read grant.
    async fn deliverable(&self, thread_id: ThreadId, recipient: &Principal) -> Result<bool> {
        // A new message's sequence is above the current head, so above any floor.
        Ok(!matches!(
            self.store.delivery_grant(thread_id, recipient, u64::MAX, self.now()).await?,
            DeliveryGrant::Denied
        ))
    }

    pub async fn publish(
        &self,
        actor: &Principal,
        thread_id: ThreadId,
        mut req: PublishMessage,
    ) -> Result<Message> {
        let _thread = self.load_workspace_thread(actor, thread_id).await?;
        if let Some(fence) = req.grant_fence.as_mut() {
            // Grant authority is checked atomically in the store with grant codes.
            fence.at = self.now();
        } else {
            self.require_cap(actor, thread_id, Cap::Publish).await?;
        }
        if req.body.is_empty() && req.kind != MessageKind::Notice {
            return Err(Error::Invalid("body_required"));
        }

        let members = self.store.list_participants(thread_id).await?;
        let recipients: Vec<Principal> = if req.recipients.is_empty() {
            let mut out = Vec::new();
            for p in members.iter().filter(|p| {
                p.caps.contains(&Cap::Read)
                    && p.principal != *actor
                    && p.principal.org_id == actor.org_id
            }) {
                if !is_enrollment_principal(&p.principal)
                    || self.deliverable(thread_id, &p.principal).await?
                {
                    out.push(p.principal.clone());
                }
            }
            out
        } else {
            let mut out = Vec::new();
            for target in &req.recipients {
                if target.org_id != actor.org_id {
                    return Err(Error::Forbidden("org_workspace_mismatch"));
                }
                if target == actor {
                    continue;
                }
                let Some(member) = members.iter().find(|p| p.principal == *target) else {
                    return Err(Error::Invalid("recipient_not_a_member"));
                };
                if !member.caps.contains(&Cap::Read) {
                    return Err(Error::Invalid("recipient_missing_read"));
                }
                if is_enrollment_principal(target) && !self.deliverable(thread_id, target).await? {
                    return Err(Error::Invalid("recipient_grant_inactive"));
                }
                out.push(target.clone());
            }
            out
        };

        let (message, _) = self
            .store
            .append_with_delivery(thread_id, actor, req, &recipients)
            .await?;
        // Wake on replay too: acceptance may have repaired missing delivery intent.
        self.wake.notify_worker().await;
        self.wake.notify_thread(thread_id).await;
        Ok(message)
    }

    pub async fn read_messages(
        &self,
        actor: &Principal,
        thread_id: ThreadId,
        after_seq: u64,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let _thread = self.load_workspace_thread(actor, thread_id).await?;
        self.require_cap(actor, thread_id, Cap::Read).await?;
        let limit = limit.clamp(1, 200);
        self.store.read_messages(thread_id, after_seq, limit).await
    }

    /// Scoped reads linearize authority and returned data in the store.
    pub async fn read_scoped(
        &self, actor: &Principal, thread_id: ThreadId, generation: u64,
        after_seq: u64, limit: usize,
    ) -> Result<(Thread, Vec<Message>)> {
        self.store.read_scoped(actor, thread_id, generation, after_seq, limit.min(200)).await
    }

    /// Compare persisted revocation generation; a signed token cannot set it.
    pub async fn validate_grant_generation(&self, actor: &Principal, thread: ThreadId, generation: u64) -> Result<()> {
        self.load_workspace_thread(actor, thread).await?;
        let members = self.store.list_participants(thread).await?;
        if !members.iter().any(|p| p.principal == *actor && p.grant_generation == generation && p.role != Role::Revoked) {
            return Err(Error::Forbidden("stale_grant_generation"));
        }
        Ok(())
    }

    // ---- Enrollment and grants -------------------------------------------

    pub async fn enroll(&self, actor: &Principal, req: EnrollDevice) -> Result<Enrollment> {
        validate_enroll(actor, &req)?;
        self.store.enroll(actor, req, self.now()).await
    }

    /// Owner only; other accounts see `not_found`.
    pub async fn get_enrollment(&self, actor: &Principal, enrollment_id: Uuid) -> Result<Enrollment> {
        match self.store.get_enrollment(enrollment_id).await? {
            Some(enrollment) if enrollment.owner == *actor => Ok(enrollment),
            _ => Err(Error::NotFound("enrollment")),
        }
    }

    pub async fn list_enrollments(&self, actor: &Principal) -> Result<Vec<Enrollment>> {
        self.store.list_enrollments(actor).await
    }

    /// Device sign-out. Owner only; other accounts see `not_found`.
    pub async fn revoke_enrollment(&self, actor: &Principal, enrollment_id: Uuid) -> Result<Enrollment> {
        let enrollment = self.store.revoke_enrollment(actor, enrollment_id, self.now()).await?;
        // Wake open streams on every affected thread so they recheck now.
        let filter = GrantFilter { enrollment_id: Some(enrollment_id), thread_id: None };
        for (grant, _) in self.store.list_grants(&actor.org_id, &filter, self.now()).await? {
            self.wake.notify_thread(grant.thread_id).await;
        }
        Ok(enrollment)
    }

    pub async fn create_grant(&self, actor: &Principal, mut req: CreateGrant) -> Result<Grant> {
        validate_operations(&req.operations)?;
        validate_ttl(req.ttl_seconds)?;
        if is_enrollment_principal(actor) {
            return Err(Error::Forbidden("invite_required"));
        }
        req.operations.sort_by_key(|op| *op as u8);
        self.store.create_grant(actor, req, self.now()).await
    }

    pub async fn get_grant(&self, actor: &Principal, grant_id: Uuid) -> Result<Grant> {
        let (grant, enrollment) = self
            .store
            .get_grant(grant_id, self.now())
            .await?
            .ok_or(Error::NotFound("grant"))?;
        if grant.org_id != actor.org_id {
            return Err(Error::NotFound("grant"));
        }
        let members = self.store.list_participants(grant.thread_id).await?;
        if !can_view_grant(actor, &enrollment, &members) {
            return Err(Error::NotFound("grant"));
        }
        Ok(grant)
    }

    pub async fn list_grants(&self, actor: &Principal, filter: &GrantFilter) -> Result<Vec<Grant>> {
        let rows = self.store.list_grants(&actor.org_id, filter, self.now()).await?;
        let mut members: HashMap<ThreadId, Vec<Participant>> = HashMap::new();
        let mut out = Vec::new();
        for (grant, enrollment) in rows {
            if !members.contains_key(&grant.thread_id) {
                let loaded = self.store.list_participants(grant.thread_id).await?;
                members.insert(grant.thread_id, loaded);
            }
            if can_view_grant(actor, &enrollment, &members[&grant.thread_id]) {
                out.push(grant);
            }
        }
        Ok(out)
    }

    async fn mutate_grant(&self, actor: &Principal, grant_id: Uuid, mutation: GrantMutation) -> Result<Grant> {
        let grant = self.store.mutate_grant(actor, grant_id, mutation, self.now()).await?;
        // Wake open streams so they recheck authority immediately.
        self.wake.notify_thread(grant.thread_id).await;
        Ok(grant)
    }

    pub async fn revoke_grant(&self, actor: &Principal, grant_id: Uuid) -> Result<Grant> {
        self.mutate_grant(actor, grant_id, GrantMutation::Revoke).await
    }

    pub async fn restore_grant(&self, actor: &Principal, grant_id: Uuid) -> Result<Grant> {
        self.mutate_grant(actor, grant_id, GrantMutation::Restore).await
    }

    pub async fn renew_grant(&self, actor: &Principal, grant_id: Uuid, ttl_seconds: i64) -> Result<Grant> {
        validate_ttl(ttl_seconds)?;
        let expires_at = self.now() + chrono::Duration::seconds(ttl_seconds);
        self.mutate_grant(actor, grant_id, GrantMutation::Renew { expires_at }).await
    }

    /// Live authority for the backend issuer; the caller cannot choose the
    /// principal, generation or incarnation.
    pub async fn grant_issuance(
        &self, actor: &Principal, grant_id: Uuid, req: GrantIssuanceRequest,
    ) -> Result<GrantIssuance> {
        let grant = self.store.grant_issuance(actor, grant_id, req, self.now()).await?;
        Ok(GrantIssuance { not_after: grant.expires_at, grant })
    }

    /// Live verification for the backend delivery bridge (contract §6.1).
    /// Only the org-scoped `system:mq-delivery-bridge` verifier may ask; the
    /// route additionally requires an asymmetric (backend-issued) signature.
    pub async fn delivery_check(
        &self, actor: &Principal, grant_id: Uuid, req: DeliveryCheckRequest,
    ) -> Result<DeliveryCheck> {
        if actor.kind != PrincipalKind::System || actor.id != DELIVERY_VERIFIER_ID {
            return Err(Error::Forbidden("delivery_verifier_required"));
        }
        let now = self.now();
        let (grant, enrollment) = self
            .store
            .get_grant(grant_id, now)
            .await?
            .ok_or(Error::NotFound("grant"))?;
        if grant.org_id != actor.org_id || req.recipient.org_id != actor.org_id {
            return Err(Error::NotFound("grant"));
        }
        let members = self.store.list_participants(grant.thread_id).await?;
        check_delivery_authority(&grant, &enrollment, &members, &req, now)
    }

    /// Check a grant credential for `read` without returning data (SSE, thread metadata).
    pub async fn authorize_grant_read(
        &self, actor: &Principal, thread_id: ThreadId, mut fence: GrantFence,
    ) -> Result<Thread> {
        fence.at = self.now();
        Ok(self.store.read_granted(actor, thread_id, &fence, 0, 0).await?.0)
    }

    /// Legacy array read under a grant: honours the floor and refuses a cursor
    /// below it instead of silently filtering.
    pub async fn read_granted_messages(
        &self, actor: &Principal, thread_id: ThreadId, mut fence: GrantFence,
        after_seq: u64, limit: usize,
    ) -> Result<Vec<Message>> {
        fence.at = self.now();
        let (_, grant, messages) = self
            .store
            .read_granted(actor, thread_id, &fence, after_seq, limit.clamp(1, 200))
            .await?;
        if after_seq < grant.history_after_seq {
            return Err(Error::Conflict("history_cursor_before_floor"));
        }
        Ok(messages)
    }

    /// Cursor page with explicit history skips (contract §8).
    pub async fn read_history(
        &self, actor: &Principal, thread_id: ThreadId, authority: HistoryAuthority,
        after_seq: u64, limit: usize,
    ) -> Result<HistoryPage> {
        let limit = limit.clamp(1, HISTORY_MAX_LIMIT);
        match authority {
            HistoryAuthority::Membership => {
                let thread = self.get_thread(actor, thread_id).await?;
                let messages = self.store.read_messages(thread_id, after_seq, limit + 1).await?;
                Ok(HistoryPage::build(&thread, after_seq, 0, messages, limit))
            }
            HistoryAuthority::Scoped { generation } => {
                let (thread, messages) = self
                    .store
                    .read_scoped(actor, thread_id, generation, after_seq, limit + 1)
                    .await?;
                Ok(HistoryPage::build(&thread, after_seq, 0, messages, limit))
            }
            HistoryAuthority::Grant(mut fence) => {
                fence.at = self.now();
                let (thread, grant, messages) = self
                    .store
                    .read_granted(actor, thread_id, &fence, after_seq, limit + 1)
                    .await?;
                Ok(HistoryPage::build(&thread, after_seq, grant.history_after_seq, messages, limit))
            }
        }
    }

    /// Pre-dispatch recheck for the delivery worker.
    pub async fn delivery_grant(&self, job: &DeliveryJob, message_seq: u64) -> Result<DeliveryGrant> {
        self.store
            .delivery_grant(job.thread_id, &job.recipient, message_seq, self.now())
            .await
    }

    pub async fn claim_delivery_jobs(&self, limit: usize) -> Result<Vec<DeliveryJob>> {
        self.store.claim_delivery_jobs(limit).await
    }

    pub async fn settle_delivery_job(
        &self,
        job_id: DeliveryJobId,
        expected_attempt: u32,
        status: DeliveryStatus,
    ) -> Result<()> {
        self.store
            .settle_delivery_job(job_id, expected_attempt, status)
            .await
    }

    pub async fn get_message(&self, message_id: MessageId) -> Result<Option<Message>> {
        self.store.get_message(message_id).await
    }

    async fn require_cap(&self, actor: &Principal, thread_id: ThreadId, cap: Cap) -> Result<()> {
        if self.store.has_cap(thread_id, actor, cap).await? {
            Ok(())
        } else {
            Err(Error::Forbidden("not_a_member_or_missing_cap"))
        }
    }
}
